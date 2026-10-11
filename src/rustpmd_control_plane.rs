use std::{path::Path, path::PathBuf, time::Duration};

use anyhow::{anyhow, Context, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
    time::{sleep, timeout, Instant},
};

#[cfg(unix)]
use tokio::net::UnixStream;

use tracing::{info, warn};

use crate::ipc::{ControlRequest, ControlResponse, ResponseCode, RuntimeContractView};

use super::{
    ControlRuntime, DaemonRole, ServiceCommand, ServicesFacade,
};
static RESCAN_REQUESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) async fn process_request(
    req: ControlRequest,
    runtime: &ControlRuntime,
) -> ControlResponse {
    let _contract = &runtime.contract;
    match req {
        ControlRequest::Ping => ControlResponse::ok("pong"),
        ControlRequest::RuntimeContract => ControlResponse {
            ok: true,
            message: "ok".to_string(),
            code: ResponseCode::Ok,
            service: None,
            services: None,
            runtime_contract: Some(RuntimeContractView {
                config_schema: runtime.contract.config_schema.as_str().to_string(),
                api_schema: runtime.contract.api_schema.as_str().to_string(),
                policy_provider: runtime.contract.policy_provider.to_string(),
                web_session_ttl_secs: runtime.policies.web_security.session_ttl_secs,
                web_max_sessions: runtime.policies.web_security.max_web_sessions,
                web_program_config_write: runtime
                    .policies
                    .management_surface
                    .allow_web_program_config_write,
                web_service_control: runtime
                    .policies
                    .management_surface
                    .allow_web_service_control,
                web_rescan: runtime.policies.management_surface.allow_web_rescan,
            }),
        },
        ControlRequest::Rescan => {
            let _rescan_guard = RESCAN_REQUESTS.lock().await;
            let previous_elevated = runtime.elevated.read().await.clone();
            let local_reload = ServicesFacade::sync_services_from_config(
                runtime.role,
                &runtime.daemon_config_path,
                &runtime.socket_path,
                &runtime.handles,
                &runtime.state,
                &runtime.elevated,
                &runtime.service_sources,
                &runtime.service_meta,
                runtime.startup_policy.clone(),
            )
            .await;
            let local_summary = match local_reload {
                Ok(summary) => summary,
                Err(err) => {
                    return ControlResponse::err(
                        ResponseCode::InvalidRequest,
                        format!("rescan failed: {}", err),
                    );
                }
            };
            let mut msg = format!(
                "config rescanned: added={}, updated={}, removed={}",
                local_summary.added, local_summary.updated, local_summary.removed
            );
            if runtime.role == DaemonRole::Primary {
                match forward_to_privd(&runtime.privd_socket, ControlRequest::Rescan).await {
                    Ok(resp) if resp.ok => {
                        msg = format!("{} (primary + privd)", msg);
                    }
                    Ok(resp) => {
                        *runtime.elevated.write().await = previous_elevated;
                        return ControlResponse::err(resp.code, format!("{}; privd rescan failed: {}", msg, resp.message));
                    }
                    Err(err) => {
                        *runtime.elevated.write().await = previous_elevated;
                        return ControlResponse::err(ResponseCode::DependencyUnavailable,
                            format!("{}; privd rescan unavailable: {}", msg, err));
                    }
                }
            }
            ControlResponse::ok(msg)
        }
        ControlRequest::List => {
            let mut services =
                ServicesFacade::list_views(&runtime.state, &runtime.service_sources, &runtime.service_meta)
                    .await;
            let elevated_set = runtime.elevated.read().await.clone();
            if !elevated_set.is_empty() {
                if let Ok(resp) = forward_to_privd(&runtime.privd_socket, ControlRequest::List).await {
                    if let Some(remote) = resp.services {
                        services.retain(|v| !elevated_set.contains(&v.name));
                        services.extend(remote);
                        services.sort_by(|a, b| a.name.cmp(&b.name));
                    }
                }
            }
            ControlResponse {
                ok: true,
                message: "ok".to_string(),
                code: ResponseCode::Ok,
                service: None,
                services: Some(services),
                runtime_contract: None,
            }
        }
        ControlRequest::Status { service } => {
            let is_elevated = runtime.elevated.read().await.contains(&service);
            if is_elevated {
                return forward_result(&runtime.privd_socket, ControlRequest::Status { service }).await;
            }
            match ServicesFacade::get_view(
                &runtime.state,
                &runtime.service_sources,
                &runtime.service_meta,
                &service,
            )
            .await
            {
                Some(view) => ControlResponse {
                    ok: true,
                    message: "ok".to_string(),
                    code: ResponseCode::Ok,
                    service: Some(view),
                    services: None,
                    runtime_contract: None,
                },
                None => ControlResponse::err(
                    ResponseCode::UnknownService,
                    format!("unknown service: {}", service),
                ),
            }
        }
        ControlRequest::Start { service } => {
            let is_elevated = runtime.elevated.read().await.contains(&service);
            if is_elevated {
                return forward_result(&runtime.privd_socket, ControlRequest::Start { service }).await;
            }
            ServicesFacade::control_service(
                &runtime.handles,
                &runtime.state,
                &runtime.service_sources,
                &runtime.service_meta,
                &service,
                ServiceCommand::Start,
            )
            .await
        }
        ControlRequest::Stop { service } => {
            let is_elevated = runtime.elevated.read().await.contains(&service);
            if is_elevated {
                return forward_result(&runtime.privd_socket, ControlRequest::Stop { service }).await;
            }
            ServicesFacade::control_service(
                &runtime.handles,
                &runtime.state,
                &runtime.service_sources,
                &runtime.service_meta,
                &service,
                ServiceCommand::Stop,
            )
            .await
        }
        ControlRequest::Restart { service } => {
            let is_elevated = runtime.elevated.read().await.contains(&service);
            if is_elevated {
                return forward_result(&runtime.privd_socket, ControlRequest::Restart { service }).await;
            }
            ServicesFacade::control_service(
                &runtime.handles,
                &runtime.state,
                &runtime.service_sources,
                &runtime.service_meta,
                &service,
                ServiceCommand::Restart,
            )
            .await
        }
        ControlRequest::Reload { service } => {
            let is_elevated = runtime.elevated.read().await.contains(&service);
            if is_elevated {
                return forward_result(&runtime.privd_socket, ControlRequest::Reload { service }).await;
            }
            ServicesFacade::reload_single_service_from_file(
                runtime.role,
                &service,
                &runtime.handles,
                &runtime.state,
                &runtime.service_sources,
                &runtime.service_meta,
                runtime.startup_policy.clone(),
            )
            .await
        }
    }
}

async fn forward_result(privd_socket: &Path, req: ControlRequest) -> ControlResponse {
    match forward_to_privd(privd_socket, req).await {
        Ok(resp) => resp,
        Err(err) => ControlResponse::err(
            ResponseCode::DependencyUnavailable,
            format!("priv-helper unavailable: {}", err),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn reload_does_not_hold_elevated_route_lock_while_waiting_for_config_lock() {
        let runtime = Arc::new(crate::rustpmd::rustpmd_state::test_control_runtime());
        let config_guard = crate::rustpmd::rustpmd_services::SERVICE_UPDATES.lock().await;
        let worker_runtime = Arc::clone(&runtime);
        let request = tokio::spawn(async move {
            process_request(ControlRequest::Reload { service: "normal".into() }, &worker_runtime).await
        });
        tokio::task::yield_now().await;
        let route = timeout(Duration::from_secs(1), runtime.elevated.write()).await;
        let writable = route.is_ok();
        drop(route);
        drop(config_guard);
        request.await.unwrap();
        assert!(writable, "rescan must be able to update routing while reload waits");
    }
}

async fn forward_to_privd(privd_socket: &Path, req: ControlRequest) -> Result<ControlResponse> {
    #[cfg(not(unix))]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        let pipe_name = privd_socket.to_str()
            .ok_or_else(|| anyhow!("invalid priv pipe path: {}", privd_socket.display()))?;
        let op = async {
            let stream = ClientOptions::new()
                .open(pipe_name)
                .with_context(|| format!("failed to connect priv pipe {}", pipe_name))?;
            let (reader, mut writer) = tokio::io::split(stream);
            let reader = BufReader::new(reader);
            let mut payload = serde_json::to_string(&req).context("failed to encode priv request")?;
            payload.push('\n');
            writer
                .write_all(payload.as_bytes())
                .await
                .context("failed to write priv request")?;
            let mut line = String::new();
            let mut limited_reader = reader.take((crate::ipc::MAX_CONTROL_MESSAGE_BYTES + 1) as u64);
            limited_reader
                .read_line(&mut line)
                .await
                .context("failed to read priv response")?;
            if line.len() > crate::ipc::MAX_CONTROL_MESSAGE_BYTES {
                return Err(anyhow!(
                    "priv response too large (>{} bytes)",
                    crate::ipc::MAX_CONTROL_MESSAGE_BYTES
                ));
            }
            if line.trim().is_empty() {
                return Err(anyhow!("empty response from priv-helper"));
            }
            serde_json::from_str(line.trim()).context("failed to decode priv response")
        };

        timeout(Duration::from_secs(2), op)
            .await
            .with_context(|| format!("priv-helper request timeout via {}", pipe_name))?
    }

    #[cfg(unix)]
    {
        let op = async {
            let stream = UnixStream::connect(privd_socket)
                .await
                .with_context(|| format!("failed to connect priv socket {}", privd_socket.display()))?;
            let (reader, mut writer) = stream.into_split();
            let reader = BufReader::new(reader);
            let mut payload = serde_json::to_string(&req).context("failed to encode priv request")?;
            payload.push('\n');
            writer
                .write_all(payload.as_bytes())
                .await
                .context("failed to write priv request")?;
            let mut line = String::new();
            let mut limited_reader = reader.take((crate::ipc::MAX_CONTROL_MESSAGE_BYTES + 1) as u64);
            limited_reader
                .read_line(&mut line)
                .await
                .context("failed to read priv response")?;
            if line.len() > crate::ipc::MAX_CONTROL_MESSAGE_BYTES {
                return Err(anyhow!(
                    "priv response too large (>{} bytes)",
                    crate::ipc::MAX_CONTROL_MESSAGE_BYTES
                ));
            }
            if line.trim().is_empty() {
                return Err(anyhow!("empty response from priv-helper"));
            }
            serde_json::from_str(line.trim()).context("failed to decode priv response")
        };

        timeout(Duration::from_secs(2), op)
            .await
            .with_context(|| format!("priv-helper request timeout via {}", privd_socket.display()))?
    }
}

pub(super) async fn wait_for_privd_ready(privd_socket: &Path, startup_timeout_secs: u64) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(startup_timeout_secs);
    loop {
        if ping_privd(privd_socket).await.is_ok() {
            info!(socket = %privd_socket.display(), "priv-helper is ready");
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "priv-helper did not become ready within {}s ({})",
                startup_timeout_secs,
                privd_socket.display()
            ));
        }
        sleep(Duration::from_millis(500)).await;
    }
}

async fn ping_privd(privd_socket: &Path) -> Result<()> {
    let resp = forward_to_privd(privd_socket, ControlRequest::Ping).await?;
    if !resp.ok {
        return Err(anyhow!("priv-helper ping rejected: {}", resp.message));
    }
    Ok(())
}

pub(super) async fn monitor_privd_health(
    privd_socket: PathBuf,
    interval_secs: u64,
    failure_threshold: u32,
    shutdown_rx: &mut tokio::sync::broadcast::Receiver<()>,
    fatal_tx: mpsc::Sender<String>,
) {
    let mut failures: u32 = 0;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => return,
            _ = sleep(Duration::from_secs(interval_secs)) => {}
        }

        match ping_privd(&privd_socket).await {
            Ok(()) => {
                if failures > 0 {
                    info!(
                        socket = %privd_socket.display(),
                        previous_failures = failures,
                        "priv-helper health recovered"
                    );
                }
                failures = 0;
            }
            Err(err) => {
                failures += 1;
                warn!(
                    socket = %privd_socket.display(),
                    failures,
                    threshold = failure_threshold,
                    error = %err,
                    "priv-helper health check failed"
                );
                if failures >= failure_threshold {
                    match attempt_privd_recovery(&privd_socket).await {
                        Ok(()) => {
                            info!(
                                socket = %privd_socket.display(),
                                "priv-helper recovery succeeded"
                            );
                            failures = 0;
                            continue;
                        }
                        Err(recover_err) => {
                            warn!(
                                socket = %privd_socket.display(),
                                error = %recover_err,
                                "priv-helper recovery failed"
                            );
                        }
                    }
                    let _ = fatal_tx
                        .try_send(format!(
                            "priv-helper unreachable after {} failures: {}",
                            failures, err
                        ));
                    return;
                }
            }
        }
    }
}

async fn attempt_privd_recovery(privd_socket: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let status = Command::new("launchctl")
            .args(["kickstart", "-k", "system/club.motofans.rustpm-privd"])
            .status()
            .await
            .context("failed to execute launchctl kickstart for priv-helper")?;
        if !status.success() {
            return Err(anyhow!(
                "launchctl kickstart failed for priv-helper with status {}",
                status
            ));
        }
        wait_for_privd_ready(privd_socket, 15).await?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "rustpm-privd.exe"])
            .status()
            .await;

        let start_status = Command::new("sc")
            .args(["start", "rustpm-privd"])
            .status()
            .await
            .context("failed to execute sc start rustpm-privd")?;

        if !start_status.success() {
            return Err(anyhow!(
                "sc start rustpm-privd failed with status {}",
                start_status
            ));
        }

        wait_for_privd_ready(privd_socket, 15).await?;
        Ok(())
    }
}
