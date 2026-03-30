use super::*;

pub(super) async fn control_service_impl(
    handles: &HandleMap,
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
    service: &str,
    cmd: ServiceCommand,
) -> ControlResponse {
    let tx_opt = {
        let guard = handles.read().await;
        guard.get(service).cloned()
    };
    let Some(tx) = tx_opt else {
        return ControlResponse::err(
            ResponseCode::UnknownService,
            format!("unknown service: {}", service),
        );
    };

    if tx.send(cmd).await.is_err() {
        return ControlResponse::err(
            ResponseCode::DependencyUnavailable,
            format!("service actor unavailable: {}", service),
        );
    }

    sleep(Duration::from_millis(30)).await;
    let service_view = get_view(state, service_sources, service_meta, service).await;
    ControlResponse {
        ok: true,
        message: format!("command sent: {}", service),
        code: ResponseCode::Ok,
        service: service_view,
        services: None,
        runtime_contract: None,
    }
}

pub(super) async fn register_service_actor_impl(
    svc: ServiceConfig,
    handles: &HandleMap,
    state: &SharedState,
    service_meta: &ServiceMetaMap,
    startup_policy: ServiceStartupPolicy,
) {
    let name = svc.name.clone();
    {
        let mut guard = handles.write().await;
        if let Some(old) = guard.remove(&name) {
            let _ = old.send(ServiceCommand::Shutdown).await;
        }
    }

    let (tx, rx) = mpsc::channel::<ServiceCommand>(16);
    {
        let mut guard = handles.write().await;
        guard.insert(name.clone(), tx);
    }
    let state_clone = Arc::clone(state);
    let meta_clone = Arc::clone(service_meta);
    let startup_policy_clone = startup_policy.clone();
    tokio::spawn(async move {
        if let Err(err) =
            manage_service_actor(svc, rx, state_clone, meta_clone, startup_policy_clone).await
        {
            error!(service = %name, error = %err, "service actor crashed");
        }
    });
}

pub(super) async fn update_state_impl(
    state: &SharedState,
    service: &str,
    rec: RuntimeRecord,
) {
    let mut guard = state.write().await;
    guard.insert(service.to_string(), rec);
}

pub(super) async fn update_runtime_log_bindings_impl(
    service_meta: &ServiceMetaMap,
    service_name: &str,
    bindings: &ServiceLogBindings,
) {
    let mut guard = service_meta.write().await;
    if let Some(meta) = guard.get_mut(service_name) {
        meta.stdout_binding = bindings.stdout.clone();
        meta.stderr_binding = bindings.stderr.clone();
        if let Some(binding) = &bindings.stdout {
            meta.stdout_logfile = Some(binding.canonical_path.clone());
        }
        if let Some(binding) = &bindings.stderr {
            meta.stderr_logfile = Some(binding.canonical_path.clone());
        }
    }
}
