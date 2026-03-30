use super::*;
#[path = "rustpmd_services_sync.rs"]
mod rustpmd_services_sync;
#[path = "rustpmd_services_view.rs"]
mod rustpmd_services_view;
#[path = "rustpmd_services_control.rs"]
mod rustpmd_services_control;
#[path = "rustpmd_service_actor.rs"]
mod rustpmd_service_actor;
#[path = "rustpmd_service_logs.rs"]
mod rustpmd_service_logs;
#[path = "rustpmd_process_group.rs"]
mod rustpmd_process_group;

pub(super) struct ServicesFacade;

impl ServicesFacade {
    pub(super) async fn control_service(
        handles: &HandleMap,
        state: &SharedState,
        service_sources: &ServiceSourceMap,
        service_meta: &ServiceMetaMap,
        service: &str,
        cmd: ServiceCommand,
    ) -> ControlResponse {
        control_service(handles, state, service_sources, service_meta, service, cmd).await
    }

    pub(super) async fn sync_services_from_config(
        role: DaemonRole,
        daemon_config_path: &Path,
        socket_path: &Path,
        handles: &HandleMap,
        state: &SharedState,
        elevated: &ElevatedSet,
        service_sources: &ServiceSourceMap,
        service_meta: &ServiceMetaMap,
        startup_policy: ServiceStartupPolicy,
    ) -> Result<RescanSummary> {
        sync_services_from_config(
            role,
            daemon_config_path,
            socket_path,
            handles,
            state,
            elevated,
            service_sources,
            service_meta,
            startup_policy,
        )
        .await
    }

    pub(super) async fn reload_single_service_from_file(
        role: DaemonRole,
        service: &str,
        handles: &HandleMap,
        state: &SharedState,
        service_sources: &ServiceSourceMap,
        service_meta: &ServiceMetaMap,
        startup_policy: ServiceStartupPolicy,
    ) -> ControlResponse {
        reload_single_service_from_file(
            role,
            service,
            handles,
            state,
            service_sources,
            service_meta,
            startup_policy,
        )
        .await
    }

    pub(super) async fn list_views(
        state: &SharedState,
        service_sources: &ServiceSourceMap,
        service_meta: &ServiceMetaMap,
    ) -> Vec<ServiceView> {
        list_views(state, service_sources, service_meta).await
    }

    pub(super) async fn get_view(
        state: &SharedState,
        service_sources: &ServiceSourceMap,
        service_meta: &ServiceMetaMap,
        service: &str,
    ) -> Option<ServiceView> {
        get_view(state, service_sources, service_meta, service).await
    }
}

async fn manage_service_actor(
    svc: ServiceConfig,
    rx: mpsc::Receiver<ServiceCommand>,
    state: SharedState,
    service_meta: ServiceMetaMap,
    startup_policy: ServiceStartupPolicy,
) -> Result<()> {
    rustpmd_service_actor::manage_service_actor_impl(svc, rx, state, service_meta, startup_policy)
        .await
}

async fn spawn_service(svc: &ServiceConfig) -> Result<StartedService> {
    let mut cmd = Command::new(&svc.command);
    cmd.args(&svc.args).stdin(Stdio::null());
    let log_bindings = apply_stdio(svc, &mut cmd)?;
    if let Some(dir) = &svc.working_dir {
        cmd.current_dir(dir);
    }
    #[cfg(unix)]
    let run_identity = resolve_run_identity(svc)?;

    #[cfg(unix)]
    {
        let maybe_identity = run_identity;
        // SAFETY: pre_exec runs in child process before exec; closure only performs process attribute setup.
        unsafe {
            cmd.pre_exec(move || {
                setpgid(Pid::from_raw(0), Pid::from_raw(0))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                if let Some(identity) = &maybe_identity {
                    setgid(identity.gid)
                        .map_err(|e| std::io::Error::other(e.to_string()))?;
                    let base_group = i32::try_from(identity.gid.as_raw())
                        .map_err(|_| std::io::Error::other("gid out of range for initgroups"))?;
                    let rc = nix::libc::initgroups(identity.username.as_ptr(), base_group);
                    if rc != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    setuid(identity.uid)
                        .map_err(|e| std::io::Error::other(e.to_string()))?;
                }
                Ok(())
            });
        }
    }

    let child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn service {}", svc.name))?;
    let pid_u32 = child
        .id()
        .ok_or_else(|| anyhow!("service {} started without pid", svc.name))?;
    let pgid = i32::try_from(pid_u32).with_context(|| format!("pid overflow for {}", svc.name))?;

    Ok(StartedService {
        child: RunningChild {
            child,
            pgid,
            pid_u32,
        },
        log_bindings,
    })
}

#[cfg(unix)]
#[derive(Clone)]
struct RunIdentity {
    uid: Uid,
    gid: Gid,
    username: CString,
}

#[cfg(unix)]
fn resolve_run_identity(svc: &ServiceConfig) -> Result<Option<RunIdentity>> {
    if svc.privilege == PrivilegeMode::Elevated {
        return Ok(None);
    }
    let Some(user) = svc.user.as_ref() else {
        return Ok(None);
    };
    let user = user.trim();
    if user.is_empty() {
        return Err(anyhow!("service '{}' user cannot be empty", svc.name));
    }
    let resolved = User::from_name(user)
        .with_context(|| format!("service '{}' failed to resolve user '{}'", svc.name, user))?
        .ok_or_else(|| anyhow!("service '{}' user '{}' does not exist", svc.name, user))?;
    let username = CString::new(resolved.name.clone())
        .with_context(|| format!("service '{}' user contains invalid NUL byte", svc.name))?;
    Ok(Some(RunIdentity {
        uid: resolved.uid,
        gid: resolved.gid,
        username,
    }))
}

fn apply_stdio(svc: &ServiceConfig, cmd: &mut Command) -> Result<ServiceLogBindings> {
    rustpmd_service_logs::apply_stdio_for_service(svc, cmd)
}

pub(super) fn ensure_log_file(path: &Path) -> Result<()> {
    rustpmd_service_logs::ensure_log_file_impl(path)
}

pub(super) fn canonicalize_allowed_log_path(path: &Path) -> Result<PathBuf> {
    rustpmd_service_logs::canonicalize_allowed_log_path_impl(path)
}

#[cfg(unix)]
pub(super) fn current_log_identity(path: &Path) -> Result<FileIdentity> {
    rustpmd_service_logs::current_log_identity_impl(path)
}

#[cfg(not(unix))]
pub(super) fn current_log_identity(path: &Path) -> Result<FileIdentity> {
    rustpmd_service_logs::current_log_identity_impl(path)
}

async fn terminate_process_group(
    service_name: &str,
    running: &mut RunningChild,
    stop_grace_secs: u64,
) -> Result<()> {
    rustpmd_process_group::terminate_process_group_impl(service_name, running, stop_grace_secs).await
}

async fn control_service(
    handles: &HandleMap,
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
    service: &str,
    cmd: ServiceCommand,
) -> ControlResponse {
    rustpmd_services_control::control_service_impl(
        handles,
        state,
        service_sources,
        service_meta,
        service,
        cmd,
    )
    .await
}

async fn register_service_actor(
    svc: ServiceConfig,
    handles: &HandleMap,
    state: &SharedState,
    service_meta: &ServiceMetaMap,
    startup_policy: ServiceStartupPolicy,
) {
    rustpmd_services_control::register_service_actor_impl(
        svc,
        handles,
        state,
        service_meta,
        startup_policy,
    )
    .await
}

async fn sync_services_from_config(
    role: DaemonRole,
    daemon_config_path: &Path,
    socket_path: &Path,
    handles: &HandleMap,
    state: &SharedState,
    elevated: &ElevatedSet,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
    startup_policy: ServiceStartupPolicy,
) -> Result<RescanSummary> {
    rustpmd_services_sync::sync_services_from_config_impl(
        role,
        daemon_config_path,
        socket_path,
        handles,
        state,
        elevated,
        service_sources,
        service_meta,
        startup_policy,
    )
    .await
}

async fn reload_single_service_from_file(
    role: DaemonRole,
    service: &str,
    handles: &HandleMap,
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
    startup_policy: ServiceStartupPolicy,
) -> ControlResponse {
    rustpmd_services_sync::reload_single_service_from_file_impl(
        role,
        service,
        handles,
        state,
        service_sources,
        service_meta,
        startup_policy,
    )
    .await
}

async fn list_views(
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
) -> Vec<ServiceView> {
    rustpmd_services_view::list_views_impl(state, service_sources, service_meta).await
}

async fn get_view(
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
    service: &str,
) -> Option<ServiceView> {
    rustpmd_services_view::get_view_impl(state, service_sources, service_meta, service).await
}

async fn update_state(state: &SharedState, service: &str, rec: RuntimeRecord) {
    rustpmd_services_control::update_state_impl(state, service, rec).await
}

async fn update_runtime_log_bindings(
    service_meta: &ServiceMetaMap,
    service_name: &str,
    bindings: &ServiceLogBindings,
) {
    rustpmd_services_control::update_runtime_log_bindings_impl(service_meta, service_name, bindings)
        .await
}

#[cfg(test)]
mod boundary_tests {
    #[test]
    fn services_module_stays_facade_oriented() {
        let src = include_str!("rustpmd_services.rs");
        let forbidden_reload = ["reload failed", "source file missing"].join(": ");
        let forbidden_orphan = ["orphaned", "source config removed"].join(": ");
        let forbidden_stdout = ["failed to open", "stdout_logfile"].join(" ");
        assert!(!src.contains(&forbidden_reload));
        assert!(!src.contains(&forbidden_orphan));
        assert!(!src.contains(&forbidden_stdout));
    }

    #[test]
    fn services_module_exposes_facade_not_control_free_functions() {
        let src = include_str!("rustpmd_services.rs");
        assert!(!src.contains("\npub(super) async fn control_service("));
        assert!(!src.contains("\npub(super) async fn register_service_actor("));
        assert!(!src.contains("\npub(super) async fn sync_services_from_config("));
        assert!(!src.contains("\npub(super) async fn reload_single_service_from_file("));
        assert!(!src.contains("\npub(super) async fn update_state("));
        assert!(!src.contains("\npub(super) async fn update_runtime_log_bindings("));
        assert!(src.contains("pub(super) struct ServicesFacade;"));
    }
}
