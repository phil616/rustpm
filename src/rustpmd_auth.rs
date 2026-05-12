use super::*;
use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use crate::auth::verify_password;
use crate::web_api::to_api_control_response;
use std::path::PathBuf;

pub(super) async fn unauthorized_json(
    headers: &HeaderMap,
    state: &AppState,
    client_ip: Option<IpAddr>,
) -> Option<axum::response::Response> {
    if is_authorized(headers, state, client_ip).await {
        return None;
    }
    Some(
        (
            StatusCode::UNAUTHORIZED,
            Json(to_api_control_response(
                ControlResponse::err(ResponseCode::Unauthorized, "unauthorized"),
            )),
        )
            .into_response(),
    )
}

async fn is_authorized(headers: &HeaderMap, state: &AppState, client_ip: Option<IpAddr>) -> bool {
    if has_valid_session(headers, state, client_ip).await {
        return true;
    }
    let password = crate::web_auth_utils::extract_password_from_headers(headers);
    if password.is_empty() {
        return false;
    }
    let guard = state.web_auth.read().await;
    verify_password(&password, &guard.password_hash)
}

async fn has_valid_session(
    headers: &HeaderMap,
    state: &AppState,
    client_ip: Option<IpAddr>,
) -> bool {
    let Some(session_id) =
        crate::web_auth_utils::extract_session_id(headers, SESSION_COOKIE_NAME)
    else {
        return false;
    };
    let now = crate::web_auth_utils::now_epoch_secs();
    let expected_ua = crate::web_auth_utils::user_agent_hash(headers);
    let expected_ip_prefix = crate::web_auth_utils::client_ip_prefix(client_ip);
    let mut guard = state.sessions.write().await;
    super::rustpmd_security_state::prune_sessions(&mut guard, now, &state.policies.web_security);
    let Some(session) = guard.get(&session_id) else {
        return false;
    };
    if session.user_agent_hash != expected_ua {
        return false;
    }
    if session.ip_prefix != expected_ip_prefix {
        return false;
    }
    true
}

pub(super) fn persist_web_password_hash(config_path: &Path, new_hash: &str) -> Result<()> {
    let mut content = if config_path.exists() {
        fs::read_to_string(config_path)
            .with_context(|| format!("failed to read daemon config {}", config_path.display()))?
    } else {
        String::new()
    };
    if content.is_empty() {
        content.push_str("[web]\n");
        content.push_str(&format!("password_hash = {}\n", new_hash));
        fs::write(config_path, content)
            .with_context(|| format!("failed to write daemon config {}", config_path.display()))?;
        return Ok(());
    }

    let mut output = Vec::new();
    let mut in_web = false;
    let mut web_section_found = false;
    let mut password_hash_written = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_web && !password_hash_written {
                output.push(format!("password_hash = {}", new_hash));
                password_hash_written = true;
            }
            in_web = trimmed[1..trimmed.len() - 1]
                .trim()
                .eq_ignore_ascii_case("web");
            if in_web {
                web_section_found = true;
            }
            output.push(line.to_string());
            continue;
        }

        if let Some((k, _)) = trimmed.split_once('=') {
            let key = k.trim().to_ascii_lowercase();
            if in_web && key == "password_hash" {
                if !password_hash_written {
                    output.push(format!("password_hash = {}", new_hash));
                    password_hash_written = true;
                }
                continue;
            }
        }

        output.push(line.to_string());
    }

    if in_web && !password_hash_written {
        output.push(format!("password_hash = {}", new_hash));
        password_hash_written = true;
    }

    if !web_section_found {
        if !output.is_empty() && output.last().map(|l| !l.is_empty()).unwrap_or(false) {
            output.push(String::new());
        }
        output.push("[web]".to_string());
        output.push(format!("password_hash = {}", new_hash));
        password_hash_written = true;
    }

    if !password_hash_written {
        if !output.is_empty() && output.last().map(|l| !l.is_empty()).unwrap_or(false) {
            output.push(String::new());
        }
        output.push("[web]".to_string());
        output.push(format!("password_hash = {}", new_hash));
    }

    let mut serialized = output.join("\n");
    if !serialized.ends_with('\n') {
        serialized.push('\n');
    }
    fs::write(config_path, serialized)
        .with_context(|| format!("failed to write daemon config {}", config_path.display()))
}

pub(super) fn ensure_web_config_write_allowed(state: &AppState, target_path: &Path) -> Result<()> {
    let policy = &state.policies.management_surface;
    if !policy.allow_web_daemon_config_write {
        return Err(anyhow!("web write to daemon config is disabled by policy"));
    }
    if !target_path.is_absolute() {
        return Err(anyhow!(
            "web config write target must be absolute: {}",
            target_path.display()
        ));
    }

    let daemon_cfg = resolve_for_policy(state.daemon_config_path.as_path())?;
    let target = resolve_for_policy(target_path)?;
    if daemon_cfg != target {
        return Err(anyhow!(
            "web config write target mismatch: expected {}, got {}",
            daemon_cfg.display(),
            target.display()
        ));
    }

    if !policy.allow_web_program_config_write {
        let config_dir = resolve_for_policy(state.config_dir.as_path())?;
        if target.starts_with(config_dir) {
            return Err(anyhow!(
                "web write to programs config dir is disabled by policy: {}",
                state.config_dir.display()
            ));
        }
    }
    Ok(())
}

fn resolve_for_policy(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path)
            .with_context(|| format!("failed to resolve path for policy: {}", path.display()));
    }
    let parent = path.parent().ok_or_else(|| {
        anyhow!(
            "path has no parent for policy resolution: {}",
            path.display()
        )
    })?;
    let parent_resolved = fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to resolve parent path for policy: {}",
            parent.display()
        )
    })?;
    let name = path.file_name().ok_or_else(|| {
        anyhow!(
            "path has no file name for policy resolution: {}",
            path.display()
        )
    })?;
    Ok(parent_resolved.join(name))
}

#[cfg(test)]
mod tests {
    use super::ensure_web_config_write_allowed;
    use crate::{
        config::ConfigSchemaVersion,
        runtime_contract::RuntimeContract,
        runtime_policy::RuntimePolicies,
    };
    use std::{
        collections::{HashMap, HashSet},
        fs,
        path::PathBuf,
        sync::Arc,
    };
    use tokio::sync::RwLock;

    fn build_state(config_dir: &str, daemon_cfg: &str) -> super::AppState {
        super::AppState {
            handles: Arc::new(RwLock::new(HashMap::new())),
            state: Arc::new(RwLock::new(HashMap::new())),
            web_auth: Arc::new(RwLock::new(super::WebAuthState {
                password_hash: crate::auth::default_password_hash(),
                daemon_config_path: PathBuf::from(daemon_cfg),
            })),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            login_failures: Arc::new(RwLock::new(HashMap::new())),
            elevated: Arc::new(RwLock::new(HashSet::new())),
            privd_socket: PathBuf::from("/tmp/rustpm-privd.sock"),
            daemon_config_path: PathBuf::from(daemon_cfg),
            config_dir: PathBuf::from(config_dir),
            socket_path: PathBuf::from("/tmp/rustpmd.sock"),
            role: super::DaemonRole::Primary,
            service_sources: Arc::new(RwLock::new(HashMap::new())),
            service_meta: Arc::new(RwLock::new(HashMap::new())),
            policies: RuntimePolicies::default(),
            contract: RuntimeContract::new(ConfigSchemaVersion::V1, "test"),
            startup_policy: super::ServiceStartupPolicy {
                first_start_not_before: tokio::time::Instant::now(),
                spin_interval: std::time::Duration::from_secs(0),
                spin_gate: Arc::new(tokio::sync::Mutex::new(tokio::time::Instant::now())),
            },
        }
    }

    #[test]
    fn deny_web_write_into_programs_dir() {
        let base = PathBuf::from("/tmp/rustpm-policy-basic-deny");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        fs::write(base.join("rustpmd.conf"), "[web]\n").expect("write daemon");
        fs::write(base.join("programs.d/easytier.conf"), "[program:easytier]\n").expect("write program");
        let state = build_state(
            base.join("programs.d").to_string_lossy().as_ref(),
            base.join("rustpmd.conf").to_string_lossy().as_ref(),
        );
        let result = ensure_web_config_write_allowed(
            &state,
            base.join("programs.d/easytier.conf").as_path(),
        );
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn allow_web_write_to_daemon_config() {
        let base = std::env::temp_dir().join("rustpm-policy-basic-allow");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        fs::write(base.join("rustpmd.conf"), "[web]\n").expect("write daemon");
        let state = build_state(
            base.join("programs.d").to_string_lossy().as_ref(),
            base.join("rustpmd.conf").to_string_lossy().as_ref(),
        );
        let result = ensure_web_config_write_allowed(
            &state,
            base.join("rustpmd.conf").as_path(),
        );
        assert!(result.is_ok());
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_programs_dir_is_rejected() {
        use std::os::unix::fs::symlink;

        let base = PathBuf::from("/tmp/rustpm-policy-symlink-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        fs::write(base.join("programs.d/evil.conf"), "x").expect("write");
        let daemon_link = base.join("rustpmd.conf");
        let _ = fs::remove_file(&daemon_link);
        symlink(base.join("programs.d/evil.conf"), &daemon_link).expect("symlink");

        let state = build_state(
            base.join("programs.d").to_string_lossy().as_ref(),
            daemon_link.to_string_lossy().as_ref(),
        );
        let result = ensure_web_config_write_allowed(&state, daemon_link.as_path());
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&base);
    }
}
