use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use axum::{
    middleware::{self, Next},
    extract::Request,
    response::Response,
    routing::{get, post},
    Router,
};

#[cfg(unix)]
use nix::{
    errno::Errno,
    sys::signal::{killpg, Signal},
    unistd::{setgid, setpgid, setuid, Gid, Pid, Uid, User},
};

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

use tokio::{
    process::{Child, Command},
    sync::{broadcast, mpsc, RwLock},
    time::{sleep, timeout, Instant},
};
use tracing::{error, info, warn};

#[path = "rustpmd_auth.rs"]
mod rustpmd_auth;
#[path = "rustpmd_control_api.rs"]
mod rustpmd_control_api;
#[path = "rustpmd_control_plane.rs"]
mod rustpmd_control_plane;
#[path = "rustpmd_control_socket.rs"]
mod rustpmd_control_socket;
#[path = "rustpmd_logs.rs"]
mod rustpmd_logs;
#[path = "rustpmd_log_security.rs"]
mod rustpmd_log_security;
#[path = "rustpmd_services.rs"]
mod rustpmd_services;
#[path = "rustpmd_security_state.rs"]
mod rustpmd_security_state;
#[path = "rustpmd_state.rs"]
mod rustpmd_state;
#[path = "rustpmd_web_host.rs"]
mod rustpmd_web_host;
#[path = "rustpmd_web_auth_api.rs"]
mod rustpmd_web_auth_api;

use crate::{
    auth::verify_password,
    config::{
        ConfigSchemaVersion, DependencyConfig, PrivilegeMode, ServiceConfig, WebConfig,
    },
    ipc::{ControlRequest, ControlResponse, ResponseCode, ServiceState, ServiceView},
    platform_fs::{self, FileIdentity},
    runtime_contract::RuntimeContract,
    runtime_policy::{DefaultRuntimePolicyProvider, RuntimePolicies, RuntimePolicyProvider},
    web_api::api_version_guard,
};
use self::rustpmd_control_plane::{monitor_privd_health, process_request, wait_for_privd_ready};
use self::rustpmd_control_socket::{
    cleanup_socket, prepare_socket_path, run_control_server, set_socket_permissions,
};
use self::rustpmd_services::{
    canonicalize_allowed_log_path, current_log_identity, ensure_log_file, ServicesFacade,
};
use self::rustpmd_web_host::{run_web_server, wait_for_shutdown_signal};
use self::rustpmd_log_security::{LogApiError, LogBinding, LogSecurityPolicy, NativeLogSecurityPolicy};
use self::rustpmd_state::{
    control_runtime_from_state, AppState, ChangePasswordRequest, ControlOutcome, ControlRuntime,
    HandleMap, LoginFailureState, LoginRequest, LogQuery, RescanSummary, RunningChild, RuntimeRecord,
    ServiceCommand, ServiceLogBindings, ServiceMeta, ServiceMetaMap, ServiceSourceMap, ServiceStartupPolicy,
    SessionState, SharedState, StartedService, WebAuthState, ElevatedSet,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonRole {
    Primary,
    PrivilegedHelper,
}


const SESSION_COOKIE_NAME: &str = "rustpm_session";
#[cfg(unix)]
const RUSTPM_MAIN_LOG: &str = "/usr/local/var/log/rustpm.log";
#[cfg(unix)]
const RUSTPM_MAIN_ERR_LOG: &str = "/usr/local/var/log/rustpm.err.log";
#[cfg(unix)]
const RUSTPM_PRIVD_LOG: &str = "/usr/local/var/log/rustpm-privd.log";
#[cfg(unix)]
const RUSTPM_PRIVD_ERR_LOG: &str = "/usr/local/var/log/rustpm-privd.err.log";
#[cfg(unix)]
const LOG_ROOT: &str = "/usr/local/var/log";

#[cfg(windows)]
const RUSTPM_MAIN_LOG: &str = "C:\\ProgramData\\rustpm\\logs\\rustpm.log";
#[cfg(windows)]
const RUSTPM_MAIN_ERR_LOG: &str = "C:\\ProgramData\\rustpm\\logs\\rustpm.err.log";
#[cfg(windows)]
const RUSTPM_PRIVD_LOG: &str = "C:\\ProgramData\\rustpm\\logs\\rustpm-privd.log";
#[cfg(windows)]
const RUSTPM_PRIVD_ERR_LOG: &str = "C:\\ProgramData\\rustpm\\logs\\rustpm-privd.err.log";
#[cfg(windows)]
const LOG_ROOT: &str = "C:\\ProgramData\\rustpm\\logs";

pub async fn run(
    services: Vec<ServiceConfig>,
    socket_path: &Path,
    config_dir: &Path,
    web: &WebConfig,
    privd_socket_path: &Path,
    dependency: Option<&DependencyConfig>,
    daemon_config_path: &Path,
    config_schema: ConfigSchemaVersion,
    role: DaemonRole,
) -> Result<()> {
    let policy_provider = DefaultRuntimePolicyProvider;
    let policies = policy_provider.policies();
    let contract = RuntimeContract::new(config_schema, policy_provider.provider_name());
    let fs_caps = platform_fs::default_policy_adapter().capabilities();
    info!(
        supports_posix_owner = fs_caps.supports_posix_owner,
        supports_posix_mode = fs_caps.supports_posix_mode,
        identity_source = fs_caps.identity_source,
        config_schema = contract.config_schema.as_str(),
        api_schema = contract.api_schema.as_str(),
        policy_provider = contract.policy_provider,
        "platform fs capabilities"
    );

    if let Some(dep) = dependency {
        if dep.require_privd {
            wait_for_privd_ready(privd_socket_path, dep.startup_timeout_secs).await?;
        }
    }

    prepare_socket_path(socket_path)?;
    #[cfg(unix)]
    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind socket {}", socket_path.display()))?;
    set_socket_permissions(socket_path)?;

    let state: SharedState = Arc::new(RwLock::new(HashMap::new()));
    let handles: HandleMap = Arc::new(RwLock::new(HashMap::new()));
    let elevated: ElevatedSet = Arc::new(RwLock::new(HashSet::new()));
    let service_sources: ServiceSourceMap = Arc::new(RwLock::new(HashMap::new()));
    let service_meta: ServiceMetaMap = Arc::new(RwLock::new(HashMap::new()));
    let startup_policy = ServiceStartupPolicy {
        first_start_not_before: Instant::now()
            + Duration::from_secs(dependency.map_or(30, |d| d.service_start_delay_secs)),
        spin_interval: Duration::from_secs(
            dependency.map_or(1, |d| d.service_start_spin_interval_secs),
        ),
        spin_gate: Arc::new(tokio::sync::Mutex::new(Instant::now())),
    };

    {
        let mut guard = state.write().await;
        for svc in &services {
            guard.insert(svc.name.clone(), RuntimeRecord::new("initialized"));
        }
    }

    ServicesFacade::sync_services_from_config(
        role,
        daemon_config_path,
        socket_path,
        &handles,
        &state,
        &elevated,
        &service_sources,
        &service_meta,
        startup_policy.clone(),
    )
    .await?;
    let (control_shutdown_tx, _) = broadcast::channel::<()>(2);
    let (fatal_tx, mut fatal_rx) = mpsc::channel::<String>(1);
    let control_state = Arc::clone(&state);
    let control_handles = Arc::clone(&handles);
    let mut control_shutdown_rx = control_shutdown_tx.subscribe();
    let privd_socket = privd_socket_path.to_path_buf();
    let daemon_config_path_buf = daemon_config_path.to_path_buf();
    let socket_path_buf = socket_path.to_path_buf();

    let elevated_for_control = Arc::clone(&elevated);
    let sources_for_control = Arc::clone(&service_sources);
    let meta_for_control = Arc::clone(&service_meta);
    let contract_for_control = contract.clone();
    let policies_for_control = policies.clone();
    let startup_policy_for_control = startup_policy.clone();
    let control_task = tokio::spawn(async move {
        #[cfg(windows)]
        let pipe_arg = socket_path_buf.clone();
        run_control_server(
            #[cfg(unix)]
            listener,
            #[cfg(windows)]
            pipe_arg,
            control_handles,
            control_state,
            elevated_for_control,
            sources_for_control,
            meta_for_control,
            privd_socket,
            daemon_config_path_buf,
            socket_path_buf,
            startup_policy_for_control,
            policies_for_control,
            contract_for_control,
            role,
            &mut control_shutdown_rx,
        )
        .await
    });

    let mut dependency_task = None;
    if let Some(dep) = dependency {
        if dep.require_privd {
            let mut dep_shutdown_rx = control_shutdown_tx.subscribe();
            let dep_socket = privd_socket_path.to_path_buf();
            let dep_cfg = dep.clone();
            dependency_task = Some(tokio::spawn(async move {
                monitor_privd_health(
                    dep_socket,
                    dep_cfg.healthcheck_interval_secs,
                    dep_cfg.failure_threshold,
                    &mut dep_shutdown_rx,
                    fatal_tx,
                )
                .await;
            }));
        }
    }

    let mut web_task = None;
    if web.enabled {
        if verify_password(crate::auth::DEFAULT_WEB_PASSWORD, &web.password_hash) {
            warn!("web console is using default password; change it immediately");
        }
        let web_state = AppState {
            handles: Arc::clone(&handles),
            state: Arc::clone(&state),
            web_auth: Arc::new(RwLock::new(WebAuthState {
                password_hash: web.password_hash.clone(),
                daemon_config_path: daemon_config_path.to_path_buf(),
            })),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            login_failures: Arc::new(RwLock::new(HashMap::new())),
            elevated: Arc::clone(&elevated),
            privd_socket: privd_socket_path.to_path_buf(),
            daemon_config_path: daemon_config_path.to_path_buf(),
            config_dir: config_dir.to_path_buf(),
            socket_path: socket_path.to_path_buf(),
            role,
            service_sources: Arc::clone(&service_sources),
            service_meta: Arc::clone(&service_meta),
            policies: policies.clone(),
            contract: contract.clone(),
            startup_policy: startup_policy.clone(),
        };
        let web_shutdown_rx = control_shutdown_tx.subscribe();
        let addr = SocketAddr::from((web.bind_ip, web.port));
        web_task = Some(tokio::spawn(async move {
            run_web_server(addr, web_state, web_shutdown_rx).await
        }));
    }

    info!(
        services = handles.read().await.len(),
        socket = %socket_path.display(),
        web_enabled = web.enabled,
        service_start_delay_secs = dependency.map_or(30, |d| d.service_start_delay_secs),
        service_start_spin_interval_secs = dependency.map_or(1, |d| d.service_start_spin_interval_secs),
        "rustpmd started"
    );
    let mut fatal_reason = None;
    tokio::select! {
        _ = wait_for_shutdown_signal() => {
            info!("received shutdown signal");
        }
        maybe_reason = fatal_rx.recv() => {
            if let Some(reason) = maybe_reason {
                error!(reason = %reason, "dependency health check failed");
                fatal_reason = Some(reason);
            }
        }
    }

    let _ = control_shutdown_tx.send(());

    let txs: Vec<mpsc::Sender<ServiceCommand>> = {
        let guard = handles.read().await;
        guard.values().cloned().collect()
    };
    for tx in &txs {
        let _ = tx.send(ServiceCommand::Shutdown).await;
    }
    match control_task.await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => error!(error = %err, "control server failed"),
        Err(err) => error!(error = %err, "control task join failed"),
    }

    if let Some(task) = web_task {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => error!(error = %err, "web server failed"),
            Err(err) => error!(error = %err, "web task join failed"),
        }
    }

    if let Some(task) = dependency_task {
        if let Err(err) = task.await {
            error!(error = %err, "dependency monitor task join failed");
        }
    }

    cleanup_socket(socket_path);
    info!("rustpmd stopped");
    if let Some(reason) = fatal_reason {
        return Err(anyhow!("fatal dependency failure: {}", reason));
    }
    Ok(())
}
