use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Path as AxPath, State},
    http::HeaderMap,
    response::IntoResponse,
};

use crate::{
    ipc::{ControlRequest, ResponseCode},
    web_api::{control_error_http, control_response_http, response_code_catalog_http},
};

use super::{
    control_runtime_from_state, process_request, rustpmd_auth::unauthorized_json, AppState,
};

async fn invoke_control(state: &AppState, request: ControlRequest) -> crate::ipc::ControlResponse {
    let runtime = control_runtime_from_state(state);
    process_request(request, &runtime).await
}

fn ensure_web_control_allowed(state: &AppState) -> Option<axum::response::Response> {
    if state.policies.management_surface.allow_web_service_control {
        return None;
    }
    Some(control_error_http(
        axum::http::StatusCode::FORBIDDEN,
        ResponseCode::Forbidden,
        "web service control is disabled by policy",
    ))
}

fn ensure_web_rescan_allowed(state: &AppState) -> Option<axum::response::Response> {
    if state.policies.management_surface.allow_web_rescan {
        return None;
    }
    Some(control_error_http(
        axum::http::StatusCode::FORBIDDEN,
        ResponseCode::Forbidden,
        "web rescan is disabled by policy",
    ))
}

pub(super) async fn api_list(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    control_response_http(invoke_control(&state, ControlRequest::List).await)
}

pub(super) async fn api_status(
    AxPath(service): AxPath<String>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    control_response_http(invoke_control(&state, ControlRequest::Status { service }).await)
}

pub(super) async fn api_start(
    AxPath(service): AxPath<String>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    if let Some(resp) = ensure_web_control_allowed(&state) {
        return resp;
    }
    control_response_http(invoke_control(&state, ControlRequest::Start { service }).await)
}

pub(super) async fn api_stop(
    AxPath(service): AxPath<String>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    if let Some(resp) = ensure_web_control_allowed(&state) {
        return resp;
    }
    control_response_http(invoke_control(&state, ControlRequest::Stop { service }).await)
}

pub(super) async fn api_restart(
    AxPath(service): AxPath<String>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    if let Some(resp) = ensure_web_control_allowed(&state) {
        return resp;
    }
    if let Some(resp) = ensure_web_rescan_allowed(&state) {
        return resp;
    }

    // Web hard-restart: rescan first to refresh service->source mapping.
    let rescan_resp = invoke_control(&state, ControlRequest::Rescan).await;
    if !rescan_resp.ok {
        return control_response_http(rescan_resp);
    }

    let mut restart_resp = invoke_control(&state, ControlRequest::Restart { service }).await;
    if !restart_resp.ok && restart_resp.code == ResponseCode::UnknownService {
        restart_resp
            .message
            .push_str(" (service may have been renamed; rescan and use the current service name)");
    }
    control_response_http(restart_resp)
}

pub(super) async fn api_rescan_config(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    if let Some(resp) = ensure_web_rescan_allowed(&state) {
        return resp;
    }
    control_response_http(invoke_control(&state, ControlRequest::Rescan).await)
}

pub(super) async fn api_reload_service(
    AxPath(service): AxPath<String>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    if let Some(resp) = ensure_web_control_allowed(&state) {
        return resp;
    }
    control_response_http(invoke_control(&state, ControlRequest::Reload { service }).await)
}

pub(super) async fn api_response_codes(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    response_code_catalog_http()
}

pub(super) async fn api_runtime_contract(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    control_response_http(invoke_control(&state, ControlRequest::RuntimeContract).await)
}
