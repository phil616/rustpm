use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{ConnectInfo, Path as AxPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};

use crate::{
    ipc::{ControlRequest, ResponseCode},
    web_api::log_response_http,
};

use super::{
    canonicalize_allowed_log_path, control_runtime_from_state, current_log_identity, process_request,
    rustpmd_auth::unauthorized_json, AppState, LogApiError, LogBinding, LogQuery,
    LogSecurityPolicy, NativeLogSecurityPolicy, RUSTPM_MAIN_ERR_LOG, RUSTPM_MAIN_LOG, RUSTPM_PRIVD_ERR_LOG,
    RUSTPM_PRIVD_LOG,
};

fn read_log_tail(path: &str, lines: usize) -> Result<String> {
    const CHUNK: usize = 8192;
    const MAX_TAIL_BYTES: usize = 2 * 1024 * 1024;
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open log file: {}", path))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("failed to stat log file: {}", path))?
        .len();
    if file_len == 0 {
        return Ok(String::new());
    }

    let mut pos = file_len;
    let mut line_count: usize = 0;
    let mut collected_bytes: usize = 0;
    let mut chunks: Vec<Vec<u8>> = Vec::new();

    while pos > 0 && line_count <= lines && collected_bytes < MAX_TAIL_BYTES {
        let read_len = usize::try_from(std::cmp::min(CHUNK as u64, pos)).unwrap_or(CHUNK);
        let remaining_budget = MAX_TAIL_BYTES.saturating_sub(collected_bytes);
        let read_len = read_len.min(remaining_budget);
        if read_len == 0 {
            break;
        }
        pos -= read_len as u64;
        file.seek(SeekFrom::Start(pos))
            .with_context(|| format!("failed to seek log file: {}", path))?;
        let mut buf = vec![0_u8; read_len];
        file.read_exact(&mut buf)
            .with_context(|| format!("failed to read log file chunk: {}", path))?;
        line_count += buf.iter().filter(|b| **b == b'\n').count();
        collected_bytes += buf.len();
        chunks.push(buf);
    }

    let total_len: usize = chunks.iter().map(Vec::len).sum();
    let mut bytes = Vec::with_capacity(total_len);
    for chunk in chunks.iter().rev() {
        bytes.extend_from_slice(chunk);
    }
    let content = String::from_utf8_lossy(&bytes).to_string();
    let total = content.lines().count();
    let skip = total.saturating_sub(lines);
    Ok(content.lines().skip(skip).collect::<Vec<_>>().join("\n"))
}

fn normalize_lines(lines: Option<usize>) -> usize {
    let lines = lines.unwrap_or(200);
    lines.clamp(1, 2000)
}

fn validate_rustpm_log_access(path: &str) -> Result<PathBuf> {
    NativeLogSecurityPolicy::validate_rustpm_log_path(Path::new(path))
}

fn validate_service_log_access(service: &str, stream: &str, binding: &LogBinding) -> Result<PathBuf> {
    NativeLogSecurityPolicy::validate_service_binding(service, stream, binding)
}

async fn resolve_service_log_binding(
    state: &AppState,
    service: &str,
    stderr: bool,
) -> std::result::Result<LogBinding, LogApiError> {
    let runtime = control_runtime_from_state(state);
    let status_resp = process_request(
        ControlRequest::Status {
            service: service.to_string(),
        },
        &runtime,
    )
    .await;
    if !status_resp.ok {
        return Err(LogApiError {
            code: status_resp.code,
            message: status_resp.message,
        });
    }
    let Some(view) = status_resp.service else {
        return Err(LogApiError {
            code: ResponseCode::InternalError,
            message: "missing service view".to_string(),
        });
    };
    let configured_path = if stderr {
        view.stderr_logfile
    } else {
        view.stdout_logfile
    }
    .ok_or_else(|| LogApiError {
        code: ResponseCode::InvalidRequest,
        message: format!(
            "log is not configured for service '{}' ({})",
            service,
            if stderr { "stderr" } else { "stdout" }
        ),
    })?;
    let binding = {
        let guard = state.service_meta.read().await;
        guard.get(service).and_then(|meta| {
            if stderr {
                meta.stderr_binding.clone()
            } else {
                meta.stdout_binding.clone()
            }
        })
    };
    let canonical = canonicalize_allowed_log_path(Path::new(&configured_path)).map_err(|err| LogApiError {
        code: ResponseCode::InvalidRequest,
        message: err.to_string(),
    })?;
    let canonical_str = canonical.display().to_string();
    let current_identity = current_log_identity(&canonical).map_err(|err| LogApiError {
        code: ResponseCode::InvalidRequest,
        message: err.to_string(),
    })?;

    if let Some(bound) = binding {
        if bound.canonical_path != canonical_str {
            return Err(LogApiError {
                code: ResponseCode::Forbidden,
                message: format!(
                    "log access denied for service '{}': mapped path changed, run hard-restart",
                    service
                ),
            });
        }
        return Ok(bound);
    }

    let new_binding = LogBinding {
        canonical_path: canonical_str,
        identity: current_identity,
    };
    {
        let mut guard = state.service_meta.write().await;
        let meta = guard
            .entry(service.to_string())
            .or_insert_with(super::ServiceMeta::default);
        if stderr {
            meta.stderr_logfile = Some(configured_path);
            meta.stderr_binding = Some(new_binding.clone());
        } else {
            meta.stdout_logfile = Some(configured_path);
            meta.stdout_binding = Some(new_binding.clone());
        }
    }
    Ok(new_binding)
}

pub(super) async fn api_service_stdout_log(
    AxPath(service): AxPath<String>,
    Query(query): Query<LogQuery>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    let lines = normalize_lines(query.lines);
    let service_name = service.clone();
    let binding = match resolve_service_log_binding(&state, &service, false).await {
        Ok(path) => path,
        Err(err) => {
            let code = err.code;
            let msg = err.message;
            let status = crate::web_api::status_for_code(&code);
            return log_response_http(status, code, msg, "", "");
        }
    };
    let canonical = match validate_service_log_access(&service_name, "stdout", &binding) {
        Ok(path) => path,
        Err(err) => {
            return log_response_http(
                StatusCode::FORBIDDEN,
                ResponseCode::Forbidden,
                err.to_string(),
                binding.canonical_path.clone(),
                "",
            )
        }
    };
    let canonical_str = canonical.display().to_string();
    match read_log_tail(&canonical_str, lines) {
        Ok(content) => log_response_http(StatusCode::OK, ResponseCode::Ok, "ok", canonical_str, content),
        Err(err) => log_response_http(
            StatusCode::BAD_REQUEST,
            ResponseCode::InvalidRequest,
            err.to_string(),
            canonical_str,
            "",
        ),
    }
}

pub(super) async fn api_service_stderr_log(
    AxPath(service): AxPath<String>,
    Query(query): Query<LogQuery>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    let lines = normalize_lines(query.lines);
    let service_name = service.clone();
    let binding = match resolve_service_log_binding(&state, &service, true).await {
        Ok(path) => path,
        Err(err) => {
            let code = err.code;
            let msg = err.message;
            let status = crate::web_api::status_for_code(&code);
            return log_response_http(status, code, msg, "", "");
        }
    };
    let canonical = match validate_service_log_access(&service_name, "stderr", &binding) {
        Ok(path) => path,
        Err(err) => {
            return log_response_http(
                StatusCode::FORBIDDEN,
                ResponseCode::Forbidden,
                err.to_string(),
                binding.canonical_path.clone(),
                "",
            )
        }
    };
    let canonical_str = canonical.display().to_string();
    match read_log_tail(&canonical_str, lines) {
        Ok(content) => log_response_http(StatusCode::OK, ResponseCode::Ok, "ok", canonical_str, content),
        Err(err) => log_response_http(
            StatusCode::BAD_REQUEST,
            ResponseCode::InvalidRequest,
            err.to_string(),
            canonical_str,
            "",
        ),
    }
}

async fn api_rustpm_log(
    log_path: &'static str,
    query: LogQuery,
    state: AppState,
    headers: HeaderMap,
    addr: SocketAddr,
) -> axum::response::Response {
    if let Some(resp) = unauthorized_json(&headers, &state, Some(addr.ip())).await {
        return resp;
    }
    let lines = normalize_lines(query.lines);
    let canonical = match validate_rustpm_log_access(log_path) {
        Ok(path) => path,
        Err(err) => {
            return log_response_http(
                StatusCode::FORBIDDEN,
                ResponseCode::Forbidden,
                err.to_string(),
                log_path.to_string(),
                "",
            )
        }
    };
    let canonical_str = canonical.display().to_string();
    match read_log_tail(&canonical_str, lines) {
        Ok(content) => log_response_http(StatusCode::OK, ResponseCode::Ok, "ok", canonical_str, content),
        Err(err) => log_response_http(
            StatusCode::BAD_REQUEST,
            ResponseCode::InvalidRequest,
            err.to_string(),
            log_path.to_string(),
            "",
        ),
    }
}

pub(super) async fn api_rustpm_main_log(
    Query(query): Query<LogQuery>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    api_rustpm_log(RUSTPM_MAIN_LOG, query, state, headers, addr).await
}

pub(super) async fn api_rustpm_main_err_log(
    Query(query): Query<LogQuery>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    api_rustpm_log(RUSTPM_MAIN_ERR_LOG, query, state, headers, addr).await
}

pub(super) async fn api_rustpm_privd_log(
    Query(query): Query<LogQuery>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    api_rustpm_log(RUSTPM_PRIVD_LOG, query, state, headers, addr).await
}

pub(super) async fn api_rustpm_privd_err_log(
    Query(query): Query<LogQuery>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    api_rustpm_log(RUSTPM_PRIVD_ERR_LOG, query, state, headers, addr).await
}

#[cfg(test)]
mod tests {
    use super::{normalize_lines, read_log_tail};
    use std::fs;

    #[test]
    fn normalize_lines_uses_default_when_missing() {
        assert_eq!(normalize_lines(None), 200);
    }

    #[test]
    fn normalize_lines_enforces_bounds() {
        assert_eq!(normalize_lines(Some(0)), 1);
        assert_eq!(normalize_lines(Some(1)), 1);
        assert_eq!(normalize_lines(Some(250)), 250);
        assert_eq!(normalize_lines(Some(5000)), 2000);
    }

    #[test]
    fn read_log_tail_has_memory_cap() {
        let path = "/tmp/rustpm-log-tail-memory-cap.log";
        let payload = "x".repeat(3 * 1024 * 1024);
        fs::write(path, payload).expect("write");
        let out = read_log_tail(path, 200).expect("tail");
        fs::remove_file(path).expect("remove");
        assert!(out.len() <= 2 * 1024 * 1024 + 3);
    }
}

#[cfg(unix)]
pub(super) fn verify_log_file_permissions_strict(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .with_context(|| format!("failed to stat log permissions: {}", path.display()))?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o640 {
        return Err(anyhow!(
            "log file permission must be 0640 for {} (current {:o})",
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn verify_log_file_permissions_strict(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn verify_log_file_permissions_relaxed(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .with_context(|| format!("failed to stat log permissions: {}", path.display()))?
        .permissions()
        .mode()
        & 0o777;
    if (mode & 0o022) != 0 {
        return Err(anyhow!(
            "log file must not be group/world writable for {} (current {:o})",
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn verify_log_file_permissions_relaxed(_path: &Path) -> Result<()> {
    Ok(())
}
