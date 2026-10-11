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
    let _update_guard = SERVICE_UPDATES.lock().await;
    let runtime =
        load_runtime_config_for_run(daemon_config_path, None, Some(socket_path.to_path_buf()))?;
    // A malformed edit must not be interpreted as deletion of a running task.
    let strict_rescan = !handles.read().await.is_empty();
    let (entries, _) =
        load_program_service_entries(&runtime.config_dir, strict_rescan, ProgramParsePolicy::v1())?;
    if role == DaemonRole::Primary {
        let local_names: HashSet<String> = handles.read().await.keys().cloned().collect();
        let previous_elevated = elevated.read().await.clone();
        // A one-phase rescan cannot atomically move ownership between daemons.
        // Require explicit removal first instead of briefly running both copies.
        for entry in &entries {
            let svc = &entry.service;
            if (svc.privilege == PrivilegeMode::Elevated && local_names.contains(&svc.name))
                || (svc.privilege == PrivilegeMode::Normal && previous_elevated.contains(&svc.name)) {
                return Err(anyhow!("service '{}' privilege ownership changed; remove it from configuration and rescan successfully before adding it under the new privilege", svc.name));
            }
        }
    }
    let mut desired_elevated = HashSet::new();
    let mut summary = RescanSummary::default();
    let mut desired_managed = HashSet::new();
    for entry in entries {
        let svc = entry.service;
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
            }
        }
        desired_managed.insert(svc.name.clone());
        let existed = {
            let guard = handles.read().await;
            guard.get(&svc.name).is_some_and(|tx| !tx.is_closed())
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
        // Removal from configuration unregisters the task and waits for its tree.
        if let Some(tx) = handles.write().await.remove(&service_name) {
            let _ = tx.send(ServiceCommand::Shutdown).await;
            tx.closed().await;
        }
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
                rec.state = ServiceState::Stopped;
                rec.pid = None;
                rec.message = "unregistered: source config removed".to_string();
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
    let _update_guard = SERVICE_UPDATES.lock().await;
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
    for entry in parsed_entries {
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
    // Receiver closure confirms that the actor has returned and released its process tree.
    old_tx.closed().await;

    register_service_actor(svc, handles, state, service_meta, startup_policy).await;
    control_service(
        handles,
        state,
        service_sources,
        service_meta,
        &service_name,
        // A new autostart actor may already have launched; Start is idempotent.
        ServiceCommand::Start,
    )
    .await
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reloading_one_service_preserves_sibling_metadata() {
        let runtime = rustpmd_state::test_control_runtime();
        let file = std::env::temp_dir().join(format!("rustpm-reload-{}-{}.conf", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let cmd = format!("{}\\System32\\cmd.exe", std::env::var("SystemRoot").unwrap());
        std::fs::write(&file, format!("[program:a]\ncommand={cmd}\nautostart=false\n[program:b]\ncommand={cmd}\nautostart=false\n")).unwrap();
        runtime.service_sources.write().await.insert("a".into(), file.clone());
        runtime.service_sources.write().await.insert("b".into(), file.clone());
        runtime.service_meta.write().await.insert("b".into(), ServiceMeta {
            stdout_logfile: Some("running-b.log".into()), ..ServiceMeta::default()
        });
        let (tx, mut rx) = mpsc::channel(16);
        runtime.handles.write().await.insert("a".into(), tx);
        let old_actor = tokio::spawn(async move {
            assert!(matches!(rx.recv().await, Some(ServiceCommand::Shutdown)));
        });
        let result = reload_single_service_from_file_impl(runtime.role, "a", &runtime.handles, &runtime.state,
            &runtime.service_sources, &runtime.service_meta, runtime.startup_policy.clone()).await;
        old_actor.await.unwrap();
        assert!(result.ok, "{}", result.message);
        assert_eq!(runtime.service_meta.read().await.get("b").unwrap().stdout_logfile.as_deref(), Some("running-b.log"));
        let tx = runtime.handles.read().await.get("a").unwrap().clone();
        tx.send(ServiceCommand::Shutdown).await.unwrap();
        tx.closed().await;
        std::fs::remove_file(file).unwrap();
    }

    #[tokio::test]
    async fn invalid_or_changed_ownership_rescan_preserves_existing_actor() {
        let mut runtime = rustpmd_state::test_control_runtime();
        let dir = std::env::temp_dir().join(format!("rustpm-rescan-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let programs = dir.join("programs");
        std::fs::create_dir_all(&programs).unwrap();
        runtime.daemon_config_path = dir.join("daemon.conf");
        std::fs::write(&runtime.daemon_config_path, format!("config_dir={}\n", programs.display())).unwrap();
        std::fs::write(programs.join("broken.conf"), "[program:existing]\ninvalid line\n").unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        runtime.handles.write().await.insert("existing".into(), tx);
        let result = sync_services_from_config_impl(runtime.role, &runtime.daemon_config_path, &runtime.socket_path,
            &runtime.handles, &runtime.state, &runtime.elevated, &runtime.service_sources, &runtime.service_meta,
            runtime.startup_policy.clone()).await;
        assert!(result.is_err());
        assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
        assert!(runtime.handles.read().await.contains_key("existing"));
        let cmd = format!("{}\\System32\\cmd.exe", std::env::var("SystemRoot").unwrap());
        std::fs::write(programs.join("broken.conf"), format!("[program:existing]\ncommand={cmd}\nprivilege=elevated\n")).unwrap();
        let result = sync_services_from_config_impl(runtime.role, &runtime.daemon_config_path, &runtime.socket_path,
            &runtime.handles, &runtime.state, &runtime.elevated, &runtime.service_sources, &runtime.service_meta,
            runtime.startup_policy.clone()).await;
        assert!(result.unwrap_err().to_string().contains("privilege ownership changed"));
        assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
        std::fs::remove_file(programs.join("broken.conf")).unwrap();
        std::fs::remove_file(&runtime.daemon_config_path).unwrap();
        std::fs::remove_dir(programs).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
