use serde::{Deserialize, Serialize};

pub const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ControlRequest {
    Ping,
    RuntimeContract,
    Rescan,
    Reload { service: String },
    List,
    Status { service: String },
    Start { service: String },
    Stop { service: String },
    Restart { service: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Starting,
    Running,
    Backoff,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseCode {
    Ok,
    InvalidRequest,
    UnknownService,
    DependencyUnavailable,
    Unauthorized,
    InvalidCredentials,
    Forbidden,
    RateLimited,
    Conflict,
    InternalError,
}

impl ResponseCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResponseCode::Ok => "ok",
            ResponseCode::InvalidRequest => "invalid_request",
            ResponseCode::UnknownService => "unknown_service",
            ResponseCode::DependencyUnavailable => "dependency_unavailable",
            ResponseCode::Unauthorized => "unauthorized",
            ResponseCode::InvalidCredentials => "invalid_credentials",
            ResponseCode::Forbidden => "forbidden",
            ResponseCode::RateLimited => "rate_limited",
            ResponseCode::Conflict => "conflict",
            ResponseCode::InternalError => "internal_error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceView {
    pub name: String,
    pub state: ServiceState,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_logfile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_logfile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeContractView {
    pub config_schema: String,
    pub api_schema: String,
    pub policy_provider: String,
    pub web_session_ttl_secs: u64,
    pub web_max_sessions: usize,
    pub web_program_config_write: bool,
    pub web_service_control: bool,
    pub web_rescan: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlResponse {
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

impl ControlResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            code: ResponseCode::Ok,
            service: None,
            services: None,
            runtime_contract: None,
        }
    }

    pub fn err(code: ResponseCode, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            code,
            service: None,
            services: None,
            runtime_contract: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlRequest, ControlResponse, ResponseCode, MAX_CONTROL_MESSAGE_BYTES};

    #[test]
    fn response_code_serialization_is_stable() {
        let v = serde_json::to_string(&ResponseCode::DependencyUnavailable).expect("serialize");
        assert_eq!(v, "\"dependency_unavailable\"");
    }

    #[test]
    fn control_response_contains_code_field() {
        let resp = ControlResponse::err(ResponseCode::InvalidRequest, "bad");
        let value = serde_json::to_value(resp).expect("to value");
        assert_eq!(value["code"], "invalid_request");
    }

    #[test]
    fn max_control_message_bytes_is_reasonable() {
        assert!(MAX_CONTROL_MESSAGE_BYTES >= 4096);
        assert!(MAX_CONTROL_MESSAGE_BYTES <= 1024 * 1024);
    }

    #[test]
    fn runtime_contract_request_wire_name_is_stable() {
        let v = serde_json::to_string(&ControlRequest::RuntimeContract).expect("serialize");
        assert_eq!(v, r#"{"action":"runtime_contract"}"#);
    }
}
