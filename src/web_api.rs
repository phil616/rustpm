use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use crate::ipc::{ControlResponse, ResponseCode, RuntimeContractView, ServiceView};
use crate::{runtime_contract::RuntimeContract, runtime_policy::RuntimePolicies};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiSchemaVersion {
    V1,
}

pub const API_VERSION_HEADER: &str = "x-rustpm-api-version";
pub const API_VERSION_V1: &str = "v1";

#[derive(Debug, serde::Serialize)]
pub struct ApiControlResponse {
    pub schema: ApiSchemaVersion,
    pub ok: bool,
    pub message: String,
    pub code: ResponseCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services: Option<Vec<ServiceView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_contract: Option<RuntimeContractView>,
}

#[derive(Debug, serde::Serialize)]
pub struct ApiLogResponse {
    pub schema: ApiSchemaVersion,
    pub ok: bool,
    pub message: String,
    pub code: ResponseCode,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResponseCodeDoc {
    pub code: ResponseCode,
    pub http_status: u16,
    pub retryable: bool,
    pub description: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub struct ApiResponseCodeCatalog {
    pub schema: ApiSchemaVersion,
    pub ok: bool,
    pub message: String,
    pub code: ResponseCode,
    pub codes: Vec<ResponseCodeDoc>,
}

#[derive(Debug, serde::Serialize)]
pub struct ApiRuntimeContractResponse {
    pub schema: ApiSchemaVersion,
    pub ok: bool,
    pub message: String,
    pub code: ResponseCode,
    pub config_schema: String,
    pub api_schema: String,
    pub policy_provider: String,
    pub web_session_ttl_secs: u64,
    pub web_max_sessions: usize,
    pub web_program_config_write: bool,
    pub web_service_control: bool,
    pub web_rescan: bool,
}

pub fn status_for_control_response(resp: &ControlResponse) -> StatusCode {
    status_for_code(&resp.code)
}

pub fn status_for_code(code: &ResponseCode) -> StatusCode {
    if code == &ResponseCode::Ok {
        return StatusCode::OK;
    }
    match code {
        ResponseCode::UnknownService => StatusCode::NOT_FOUND,
        ResponseCode::DependencyUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ResponseCode::Unauthorized | ResponseCode::InvalidCredentials => StatusCode::UNAUTHORIZED,
        ResponseCode::Forbidden => StatusCode::FORBIDDEN,
        ResponseCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ResponseCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    }
}

pub fn response_code_docs() -> Vec<ResponseCodeDoc> {
    vec![
        ResponseCodeDoc { code: ResponseCode::Ok, http_status: 200, retryable: false, description: "request completed successfully" },
        ResponseCodeDoc { code: ResponseCode::InvalidRequest, http_status: 400, retryable: false, description: "request parameters or current state are invalid" },
        ResponseCodeDoc { code: ResponseCode::UnknownService, http_status: 404, retryable: false, description: "service name was not found in current runtime mapping" },
        ResponseCodeDoc { code: ResponseCode::DependencyUnavailable, http_status: 503, retryable: true, description: "dependency is unavailable or timed out" },
        ResponseCodeDoc { code: ResponseCode::Unauthorized, http_status: 401, retryable: false, description: "authentication is required or failed" },
        ResponseCodeDoc { code: ResponseCode::InvalidCredentials, http_status: 401, retryable: false, description: "provided credentials are incorrect" },
        ResponseCodeDoc { code: ResponseCode::Forbidden, http_status: 403, retryable: false, description: "access is denied by policy" },
        ResponseCodeDoc { code: ResponseCode::RateLimited, http_status: 429, retryable: true, description: "too many requests, retry later" },
        ResponseCodeDoc { code: ResponseCode::Conflict, http_status: 400, retryable: false, description: "request conflicts with current source mapping or state" },
        ResponseCodeDoc { code: ResponseCode::InternalError, http_status: 500, retryable: true, description: "internal server error" },
    ]
}

pub fn to_api_control_response(resp: ControlResponse) -> ApiControlResponse {
    ApiControlResponse {
        schema: ApiSchemaVersion::V1,
        ok: resp.ok,
        message: resp.message,
        code: resp.code,
        service: resp.service,
        services: resp.services,
        runtime_contract: resp.runtime_contract,
    }
}

pub fn control_response_http(resp: ControlResponse) -> Response {
    let status = status_for_control_response(&resp);
    (status, Json(to_api_control_response(resp))).into_response()
}

pub fn control_error_http(
    status: StatusCode,
    code: ResponseCode,
    message: impl Into<String>,
) -> Response {
    let resp = ControlResponse::err(code, message);
    (status, Json(to_api_control_response(resp))).into_response()
}

pub fn control_ok_http(status: StatusCode, message: impl Into<String>) -> Response {
    let resp = ControlResponse::ok(message);
    (status, Json(to_api_control_response(resp))).into_response()
}

pub fn api_version_guard(headers: &HeaderMap) -> Option<Response> {
    let requested = headers
        .get(API_VERSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_ascii_lowercase());
    let Some(requested) = requested else {
        return None;
    };
    if requested == API_VERSION_V1 {
        return None;
    }
    Some(control_error_http(
        StatusCode::BAD_REQUEST,
        ResponseCode::InvalidRequest,
        format!(
            "unsupported api version '{}', supported: {}",
            requested, API_VERSION_V1
        ),
    ))
}

pub fn log_response_http(
    status: StatusCode,
    code: ResponseCode,
    message: impl Into<String>,
    path: impl Into<String>,
    content: impl Into<String>,
) -> Response {
    (
        status,
        Json(ApiLogResponse {
            schema: ApiSchemaVersion::V1,
            ok: code == ResponseCode::Ok,
            message: message.into(),
            code,
            path: path.into(),
            content: content.into(),
        }),
    )
        .into_response()
}

pub fn response_code_catalog_http() -> Response {
    (
        StatusCode::OK,
        Json(ApiResponseCodeCatalog {
            schema: ApiSchemaVersion::V1,
            ok: true,
            message: "ok".to_string(),
            code: ResponseCode::Ok,
            codes: response_code_docs(),
        }),
    )
        .into_response()
}

pub fn runtime_contract_http(contract: &RuntimeContract, policies: &RuntimePolicies) -> Response {
    (
        StatusCode::OK,
        Json(ApiRuntimeContractResponse {
            schema: ApiSchemaVersion::V1,
            ok: true,
            message: "ok".to_string(),
            code: ResponseCode::Ok,
            config_schema: contract.config_schema.as_str().to_string(),
            api_schema: contract.api_schema.as_str().to_string(),
            policy_provider: contract.policy_provider.to_string(),
            web_session_ttl_secs: policies.web_security.session_ttl_secs,
            web_max_sessions: policies.web_security.max_web_sessions,
            web_program_config_write: policies.management_surface.allow_web_program_config_write,
            web_service_control: policies.management_surface.allow_web_service_control,
            web_rescan: policies.management_surface.allow_web_rescan,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{api_version_guard, response_code_docs, status_for_code, API_VERSION_HEADER};
    use crate::ipc::ResponseCode;
    use axum::http::{HeaderMap, HeaderValue, StatusCode};

    #[test]
    fn response_code_http_mapping_is_stable() {
        assert_eq!(status_for_code(&ResponseCode::Ok), StatusCode::OK);
        assert_eq!(
            status_for_code(&ResponseCode::UnknownService),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status_for_code(&ResponseCode::DependencyUnavailable),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            status_for_code(&ResponseCode::Unauthorized),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status_for_code(&ResponseCode::InternalError),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn api_version_guard_accepts_missing_or_v1() {
        let headers = HeaderMap::new();
        assert!(api_version_guard(&headers).is_none());

        let mut headers = HeaderMap::new();
        headers.insert(API_VERSION_HEADER, HeaderValue::from_static("v1"));
        assert!(api_version_guard(&headers).is_none());
    }

    #[test]
    fn api_version_guard_rejects_unknown_version() {
        let mut headers = HeaderMap::new();
        headers.insert(API_VERSION_HEADER, HeaderValue::from_static("v2"));
        assert!(api_version_guard(&headers).is_some());
    }

    #[test]
    fn response_code_catalog_includes_core_codes() {
        let codes = response_code_docs();
        assert!(codes.iter().any(|c| c.code == ResponseCode::Ok));
        assert!(codes.iter().any(|c| c.code == ResponseCode::UnknownService));
        assert!(codes.iter().any(|c| c.code == ResponseCode::InvalidRequest));
        assert!(codes.iter().any(|c| c.code == ResponseCode::InternalError));
    }
}
