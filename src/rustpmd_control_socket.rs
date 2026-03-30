use super::*;
use tokio::io::AsyncReadExt;

pub(super) fn prepare_socket_path(socket_path: &Path) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create socket parent {}", parent.display()))?;
    }
    if socket_path.exists() {
        std::fs::remove_file(socket_path)
            .with_context(|| format!("failed to remove stale socket {}", socket_path.display()))?;
    }
    Ok(())
}

pub(super) fn set_socket_permissions(socket_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set socket permissions for {}", socket_path.display()))?;
    }
    Ok(())
}

pub(super) fn cleanup_socket(socket_path: &Path) {
    if let Err(err) = std::fs::remove_file(socket_path) {
        warn!(path = %socket_path.display(), error = %err, "failed to cleanup socket");
    }
}

pub(super) async fn run_control_server(
    listener: UnixListener,
    handles: HandleMap,
    state: SharedState,
    elevated: ElevatedSet,
    service_sources: ServiceSourceMap,
    service_meta: ServiceMetaMap,
    privd_socket: PathBuf,
    daemon_config_path: PathBuf,
    socket_path: PathBuf,
    startup_policy: ServiceStartupPolicy,
    policies: crate::runtime_policy::RuntimePolicies,
    contract: crate::runtime_contract::RuntimeContract,
    role: DaemonRole,
    shutdown_rx: &mut broadcast::Receiver<()>,
) -> Result<()> {
    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("failed to accept control connection")?;
                let runtime = ControlRuntime {
                    handles: Arc::clone(&handles),
                    state: Arc::clone(&state),
                    elevated: Arc::clone(&elevated),
                    service_sources: Arc::clone(&service_sources),
                    service_meta: Arc::clone(&service_meta),
                    privd_socket: privd_socket.clone(),
                    daemon_config_path: daemon_config_path.clone(),
                    socket_path: socket_path.clone(),
                    startup_policy: startup_policy.clone(),
                    policies: policies.clone(),
                    contract: contract.clone(),
                    role,
                };
                tokio::spawn(async move {
                    if let Err(err) = handle_control_connection(stream, runtime).await {
                        warn!(error = %err, "control request failed");
                    }
                });
            }
        }
    }
}

async fn handle_control_connection(stream: UnixStream, runtime: ControlRuntime) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let reader = BufReader::new(reader);
    let mut limited_reader = reader.take((crate::ipc::MAX_CONTROL_MESSAGE_BYTES + 1) as u64);
    let mut buf = String::new();
    limited_reader
        .read_line(&mut buf)
        .await
        .context("failed to read control request")?;
    if buf.len() > crate::ipc::MAX_CONTROL_MESSAGE_BYTES {
        return Err(anyhow!(
            "control request too large (>{} bytes)",
            crate::ipc::MAX_CONTROL_MESSAGE_BYTES
        ));
    }
    if buf.trim().is_empty() {
        return Ok(());
    }

    let req: ControlRequest = serde_json::from_str(buf.trim()).context("invalid control request json")?;
    let resp = process_request(req, &runtime).await;
    let payload = serde_json::to_string(&resp).context("failed to encode response")?;
    writer
        .write_all(payload.as_bytes())
        .await
        .context("failed to write response")?;
    writer.write_all(b"\n").await.context("failed to write newline")?;
    Ok(())
}
