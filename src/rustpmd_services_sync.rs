use super::*;
use crate::config::{
    load_program_service_entries, load_program_services_from_file, load_runtime_config_for_run,
    ProgramParsePolicy,
};

pub(super) async fn sync_services_from_config_impl(
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
    let runtime =
        load_runtime_config_for_run(daemon_config_path, None, Some(socket_path.to_path_buf()))?;
    let (entries, _) =
        load_program_service_entries(&runtime.config_dir, false, ProgramParsePolicy::v1())?;
    let mut desired_elevated = HashSet::new();
    let mut summary = RescanSummary::default();
    let mut desired_managed = HashSet::new();
    for entry in entries {
        let mut svc = entry.service;
        match role {
            DaemonRole::Primary => {
                if svc.privilege == PrivilegeMode::Elevated {
                    desired_elevated.insert(svc.name.clone());
                    continue;
                }
            }
            DaemonRole::PrivilegedHelper => {
                if svc.privilege != PrivilegeMode::Elevated {
                    continue;
                }
                svc.privilege = PrivilegeMode::Normal;
            }
        }
        desired_managed.insert(svc.name.clone());
        let existed = {
            let guard = handles.read().await;
            guard.contains_key(&svc.name)
        };
        if !existed {
            register_service_actor(
                svc.clone(),
                handles,
                state,
                service_meta,
                startup_policy.clone(),
            )
            .await;
            summary.added += 1;
        }
        let mut changed = false;
        {
            let mut src_guard = service_sources.write().await;
            let prev = src_guard.get(&svc.name);
            if prev != Some(&entry.source_path) {
                src_guard.insert(svc.name.clone(), entry.source_path.clone());
                changed = true;
            }
        }
        {
            let mut meta_guard = service_meta.write().await;
            let new_stdout = svc.stdout_logfile.as_ref().map(|v| v.display().to_string());
            let new_stderr = svc.stderr_logfile.as_ref().map(|v| v.display().to_string());
            let entry = meta_guard
                .entry(svc.name.clone())
                .or_insert_with(ServiceMeta::default);
            if entry.stdout_logfile != new_stdout {
                entry.stdout_logfile = new_stdout;
                entry.stdout_binding = None;
                changed = true;
            }
            if entry.stderr_logfile != new_stderr {
                entry.stderr_logfile = new_stderr;
                entry.stderr_binding = None;
                changed = true;
            }
        }
        if changed && existed {
            summary.updated += 1;
        }
    }

    let existing_services: Vec<String> = {
        let guard = handles.read().await;
        guard.keys().cloned().collect()
    };
    for service_name in existing_services {
        if desired_managed.contains(&service_name) {
            continue;
        }
        summary.removed += 1;
        {
            let mut source_guard = service_sources.write().await;
            source_guard.remove(&service_name);
        }
        {
            let mut meta_guard = service_meta.write().await;
            if let Some(meta) = meta_guard.get_mut(&service_name) {
                meta.stdout_logfile = None;
                meta.stderr_logfile = None;
                meta.stdout_binding = None;
                meta.stderr_binding = None;
            }
        }
        {
            let mut state_guard = state.write().await;
            if let Some(rec) = state_guard.get_mut(&service_name) {
                rec.message =
                    "orphaned: source config removed, service kept until explicit stop/restart"
                        .to_string();
            }
        }
    }
    if role == DaemonRole::Primary {
        let mut eg = elevated.write().await;
        *eg = desired_elevated;
    }
    if summary.added > 0 || summary.updated > 0 || summary.removed > 0 {
        info!(
            added = summary.added,
            updated = summary.updated,
            removed = summary.removed,
            "manual config rescan applied service delta"
        );
    }
    Ok(summary)
}

pub(super) async fn reload_single_service_from_file_impl(
    role: DaemonRole,
    service: &str,
    handles: &HandleMap,
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
    startup_policy: ServiceStartupPolicy,
) -> ControlResponse {
    let rename_hint =
        " (source changed?): run rescan, then use current service name and hard-restart";
    let source_path = {
        let guard = service_sources.read().await;
        guard.get(service).cloned()
    };
    let Some(source_path) = source_path else {
        return ControlResponse::err(
            ResponseCode::UnknownService,
            format!("unknown service: {}{}", service, rename_hint),
        );
    };

    let parsed_entries = match load_program_services_from_file(&source_path) {
        Ok(entries) => entries,
        Err(err) => {
            let err_msg = err.to_string();
            let is_missing = err_msg.contains("does not exist");
            let msg = if is_missing {
                format!(
                    "reload failed: source file missing for service '{}' at {}. run rescan, then hard-restart with current service name",
                    service,
                    source_path.display()
                )
            } else {
                format!(
                    "reload failed: {}. if source was renamed, run rescan then hard-restart",
                    err_msg
                )
            };
            return ControlResponse::err(ResponseCode::InvalidRequest, msg);
        }
    };

    let mut managed_services = Vec::new();
    for mut entry in parsed_entries {
        match role {
            DaemonRole::Primary => {
                if entry.service.privilege == PrivilegeMode::Elevated {
                    continue;
                }
            }
            DaemonRole::PrivilegedHelper => {
                if entry.service.privilege != PrivilegeMode::Elevated {
                    continue;
                }
                entry.service.privilege = PrivilegeMode::Normal;
            }
        }
        managed_services.push(entry.service);
    }

    if managed_services.is_empty() {
        return ControlResponse::err(
            ResponseCode::InvalidRequest,
            format!(
                "reload failed: no managed service in source file {}",
                source_path.display()
            ),
        );
    }

    let sources_snapshot = service_sources.read().await.clone();
    for svc in &managed_services {
        if let Some(existing_source) = sources_snapshot.get(&svc.name) {
            if existing_source != &source_path {
                return ControlResponse::err(
                    ResponseCode::Conflict,
                    format!(
                        "reload conflict: service '{}' is already mapped to {}",
                        svc.name,
                        existing_source.display()
                    ),
                );
            }
        }
    }

    let Some(target_cfg) = managed_services
        .iter()
        .find(|svc| svc.name == service)
        .cloned()
    else {
        return ControlResponse::err(
            ResponseCode::InvalidRequest,
            format!(
                "reload failed: service '{}' not found in {}. run rescan, then hard-restart with current service name",
                service,
                source_path.display()
            ),
        );
    };

    {
        let mut guard = service_sources.write().await;
        for svc in &managed_services {
            guard.insert(svc.name.clone(), source_path.clone());
        }
    }
    {
        let mut meta_guard = service_meta.write().await;
        for svc in &managed_services {
            meta_guard.insert(
                svc.name.clone(),
                ServiceMeta {
                    stdout_logfile: svc.stdout_logfile.as_ref().map(|v| v.display().to_string()),
                    stderr_logfile: svc.stderr_logfile.as_ref().map(|v| v.display().to_string()),
                    stdout_binding: None,
                    stderr_binding: None,
                },
            );
        }
    }

    restart_service_with_new_config(
        target_cfg,
        handles,
        state,
        service_sources,
        service_meta,
        startup_policy,
    )
    .await
}

async fn restart_service_with_new_config(
    svc: ServiceConfig,
    handles: &HandleMap,
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
    startup_policy: ServiceStartupPolicy,
) -> ControlResponse {
    let service_name = svc.name.clone();
    let old_tx = {
        let mut guard = handles.write().await;
        guard.remove(&service_name)
    };
    let Some(old_tx) = old_tx else {
        return ControlResponse::err(
            ResponseCode::UnknownService,
            format!("unknown service: {}", service_name),
        );
    };

    let _ = old_tx.send(ServiceCommand::Shutdown).await;
    sleep(Duration::from_millis(60)).await;

    register_service_actor(svc, handles, state, service_meta, startup_policy).await;
    control_service(
        handles,
        state,
        service_sources,
        service_meta,
        &service_name,
        ServiceCommand::Restart,
    )
    .await
}
