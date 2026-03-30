use super::*;

pub(super) async fn list_views_impl(
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
) -> Vec<ServiceView> {
    let guard = state.read().await;
    let sources = service_sources.read().await;
    let meta = service_meta.read().await;

    let mut out: Vec<ServiceView> = guard
        .iter()
        .map(|(name, rec)| to_view(name, rec, sources.get(name), meta.get(name)))
        .collect();

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub(super) async fn get_view_impl(
    state: &SharedState,
    service_sources: &ServiceSourceMap,
    service_meta: &ServiceMetaMap,
    service: &str,
) -> Option<ServiceView> {
    let guard = state.read().await;
    let sources = service_sources.read().await;
    let meta = service_meta.read().await;

    guard
        .get(service)
        .map(|rec| to_view(service, rec, sources.get(service), meta.get(service)))
}

fn to_view(
    name: &str,
    rec: &RuntimeRecord,
    source: Option<&PathBuf>,
    meta: Option<&ServiceMeta>,
) -> ServiceView {
    ServiceView {
        name: name.to_string(),
        state: rec.state.clone(),
        pid: rec.pid,
        restarts: rec.restarts,
        message: rec.message.clone(),
        source_file: source.map(|p| p.display().to_string()),
        stdout_logfile: meta.and_then(|m| m.stdout_logfile.clone()),
        stderr_logfile: meta.and_then(|m| m.stderr_logfile.clone()),
    }
}
