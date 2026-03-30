use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    Json,
};

use crate::{
    auth::{hash_password, verify_password},
    ipc::{ControlResponse, ResponseCode},
    web_api::{control_error_http, control_ok_http, to_api_control_response},
};

use super::{
    rustpmd_auth::{ensure_web_config_write_allowed, persist_web_password_hash, unauthorized_json},
    rustpmd_security_state, AppState,
    ChangePasswordRequest, LoginRequest, SessionState, SESSION_COOKIE_NAME,
};

const WEB_CONSOLE_HTML: &str = include_str!("web_console.html");

pub(super) async fn web_console(
    State(_state): State<AppState>,
    _headers: HeaderMap,
) -> impl IntoResponse {
    Html(WEB_CONSOLE_HTML.to_string()).into_response()
}

pub(super) async fn api_change_password(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<ChangePasswordRequest>,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    let current_password = payload.current_password.trim().to_string();
    if current_password.is_empty() {
        return control_error_http(
            StatusCode::BAD_REQUEST,
            ResponseCode::InvalidRequest,
            "current password is required",
        );
    }
    let new_password = payload.new_password.trim().to_string();
    if new_password.len() < 8 {
        return control_error_http(
            StatusCode::BAD_REQUEST,
            ResponseCode::InvalidRequest,
            "new password must be at least 8 characters",
        );
    }

    {
        let guard = state.web_auth.read().await;
        if !verify_password(&current_password, &guard.password_hash) {
            return control_error_http(
                StatusCode::UNAUTHORIZED,
                ResponseCode::InvalidCredentials,
                "current password is incorrect",
            );
        }
    }

    let new_hash = match hash_password(&new_password) {
        Ok(v) => v,
        Err(err) => {
            return control_error_http(
                StatusCode::INTERNAL_SERVER_ERROR,
                ResponseCode::InternalError,
                format!("failed to hash password: {}", err),
            )
        }
    };
    let config_path = {
        let guard = state.web_auth.read().await;
        guard.daemon_config_path.clone()
    };
    if let Err(err) = ensure_web_config_write_allowed(&state, &config_path) {
        return control_error_http(
            StatusCode::FORBIDDEN,
            ResponseCode::Forbidden,
            format!("web config write denied: {}", err),
        );
    }

    if let Err(err) = persist_web_password_hash(&config_path, &new_hash) {
        return control_error_http(
            StatusCode::INTERNAL_SERVER_ERROR,
            ResponseCode::InternalError,
            format!("failed to persist password: {}", err),
        );
    }

    {
        let mut guard = state.web_auth.write().await;
        guard.password_hash = new_hash;
    }

    control_ok_http(StatusCode::OK, "password updated")
}

pub(super) async fn api_login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    let web_policy = state.policies.web_security.clone();
    let ua_hash = crate::web_auth_utils::user_agent_hash(&headers);
    let ip_prefix = crate::web_auth_utils::client_ip_prefix(Some(addr.ip()));
    let limit_key = format!("{}|{}", ip_prefix.clone().unwrap_or_default(), ua_hash);
    let now = crate::web_auth_utils::now_epoch_secs();
    {
        let mut failures = state.login_failures.write().await;
        rustpmd_security_state::prune_login_failures(&mut failures, now, &web_policy);
        if let Some(entry) = failures.get_mut(&limit_key) {
            if entry.blocked_until > now {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(to_api_control_response(ControlResponse::err(
                        ResponseCode::RateLimited,
                        "too many failed login attempts, retry later",
                    ))),
                )
                    .into_response();
            }
            if entry.blocked_until > 0 {
                entry.blocked_until = 0;
                entry.count = 0;
                entry.last_updated_at = now;
            }
        }
    }

    let password = payload.password.trim().to_string();
    if password.is_empty() {
        return control_error_http(
            StatusCode::BAD_REQUEST,
            ResponseCode::InvalidRequest,
            "password is required",
        );
    }

    let valid = {
        let guard = state.web_auth.read().await;
        verify_password(&password, &guard.password_hash)
    };
    if !valid {
        let mut failures = state.login_failures.write().await;
        rustpmd_security_state::prune_login_failures(&mut failures, now, &web_policy);
        rustpmd_security_state::mark_login_failure(&mut failures, &limit_key, now, &web_policy);
        return (
            StatusCode::UNAUTHORIZED,
            Json(to_api_control_response(ControlResponse::err(
                ResponseCode::Unauthorized,
                "unauthorized",
            ))),
        )
            .into_response();
    }

    {
        let mut failures = state.login_failures.write().await;
        rustpmd_security_state::clear_login_failure(&mut failures, &limit_key);
    }

    let session_id = crate::web_auth_utils::new_session_id();
    let expire_at = crate::web_auth_utils::now_epoch_secs() + web_policy.session_ttl_secs;
    {
        let mut guard = state.sessions.write().await;
        rustpmd_security_state::prune_sessions(
            &mut guard,
            crate::web_auth_utils::now_epoch_secs(),
            &web_policy,
        );
        guard.insert(
            session_id.clone(),
            SessionState {
                expires_at: expire_at,
                user_agent_hash: ua_hash,
                ip_prefix,
            },
        );
    }

    let secure_flag = crate::web_auth_utils::infer_secure_cookie(&headers);
    let cookie = if secure_flag {
        format!(
            "{}={}; HttpOnly; Path=/; SameSite=Strict; Max-Age={}; Secure",
            SESSION_COOKIE_NAME, session_id, web_policy.session_ttl_secs
        )
    } else {
        format!(
            "{}={}; HttpOnly; Path=/; SameSite=Strict; Max-Age={}",
            SESSION_COOKIE_NAME, session_id, web_policy.session_ttl_secs
        )
    };

    {
        let guard = state.web_auth.read().await;
        if verify_password(crate::auth::DEFAULT_WEB_PASSWORD, &guard.password_hash) {
            return (
                StatusCode::OK,
                [(header::SET_COOKIE, cookie)],
                Json(to_api_control_response(ControlResponse::ok(
                    "login ok (default password in use, please change immediately)",
                ))),
            )
                .into_response();
        }
    }

    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(to_api_control_response(ControlResponse::ok("login ok"))),
    )
        .into_response()
}

pub(super) async fn api_logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(session_id) = crate::web_auth_utils::extract_session_id(&headers, SESSION_COOKIE_NAME) {
        let mut guard = state.sessions.write().await;
        guard.remove(&session_id);
    }
    let clear_cookie = format!(
        "{}=; HttpOnly; Path=/; SameSite=Strict; Max-Age=0",
        SESSION_COOKIE_NAME
    );

    (
        StatusCode::OK,
        [(header::SET_COOKIE, clear_cookie)],
        Json(to_api_control_response(ControlResponse::ok("logout ok"))),
    )
        .into_response()
}

#[cfg(test)]
mod web_console_tests {
    use super::WEB_CONSOLE_HTML;

    #[test]
    fn web_console_contains_required_sections() {
        assert!(WEB_CONSOLE_HTML.contains("id=\"loginPanel\""));
        assert!(WEB_CONSOLE_HTML.contains("id=\"rows\""));
        assert!(WEB_CONSOLE_HTML.contains("/api/services"));
        assert!(WEB_CONSOLE_HTML.contains("/api/web/login"));
        assert!(WEB_CONSOLE_HTML.contains("/api/meta/runtime-contract"));
        assert!(!WEB_CONSOLE_HTML.contains("tr.innerHTML"));
    }
}
