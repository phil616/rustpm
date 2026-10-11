use super::*;

pub(super) async fn run_web_server(
    listener: tokio::net::TcpListener,
    app_state: AppState,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    let api_router = Router::new()
        .route(
            "/meta/response-codes",
            get(rustpmd_control_api::api_response_codes),
        )
        .route(
            "/meta/runtime-contract",
            get(rustpmd_control_api::api_runtime_contract),
        )
        .route("/web/login", post(rustpmd_web_auth_api::api_login))
        .route("/web/logout", post(rustpmd_web_auth_api::api_logout))
        .route("/web/rescan", post(rustpmd_control_api::api_rescan_config))
        .route("/services", get(rustpmd_control_api::api_list))
        .route("/services/{service}", get(rustpmd_control_api::api_status))
        .route("/services/{service}/start", post(rustpmd_control_api::api_start))
        .route("/services/{service}/stop", post(rustpmd_control_api::api_stop))
        .route(
            "/services/{service}/restart",
            post(rustpmd_control_api::api_restart),
        )
        .route(
            "/services/{service}/reload",
            post(rustpmd_control_api::api_reload_service),
        )
        .route(
            "/logs/services/{service}/stdout",
            get(rustpmd_logs::api_service_stdout_log),
        )
        .route(
            "/logs/services/{service}/stderr",
            get(rustpmd_logs::api_service_stderr_log),
        )
        .route("/logs/rustpm/main", get(rustpmd_logs::api_rustpm_main_log))
        .route(
            "/logs/rustpm/main_err",
            get(rustpmd_logs::api_rustpm_main_err_log),
        )
        .route("/logs/rustpm/privd", get(rustpmd_logs::api_rustpm_privd_log))
        .route(
            "/logs/rustpm/privd_err",
            get(rustpmd_logs::api_rustpm_privd_err_log),
        )
        .route(
            "/web/password",
            post(rustpmd_web_auth_api::api_change_password),
        )
        .route_layer(middleware::from_fn(api_version_middleware));

    let app = Router::new()
        .route("/", get(rustpmd_web_auth_api::web_console))
        .nest("/api", api_router)
        .with_state(app_state);

    let addr = listener.local_addr()?;
    info!(addr = %addr, "web control enabled");

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
        })
        .await
        .context("web server runtime error")
}

async fn api_version_middleware(req: Request, next: Next) -> Response {
    if let Some(resp) = api_version_guard(req.headers()) {
        return resp;
    }
    next.run(req).await
}

#[cfg(unix)]
pub(super) async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(v) => v,
        Err(err) => {
            error!(error = %err, "failed to register SIGINT handler, fallback to ctrl_c");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(v) => v,
        Err(err) => {
            error!(error = %err, "failed to register SIGTERM handler, fallback to ctrl_c");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };

    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }
}

#[cfg(not(unix))]
pub(super) async fn wait_for_shutdown_signal() {
    if let Some(mut rx) = crate::platform::windows::service::service_stop_receiver() {
        if *rx.borrow() {
            return;
        }
        let _ = rx.changed().await;
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}
