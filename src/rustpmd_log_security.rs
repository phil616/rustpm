use super::*;

#[derive(Debug, Clone)]
pub(super) struct LogBinding {
    pub(super) canonical_path: String,
    pub(super) identity: FileIdentity,
}

#[derive(Debug, Clone)]
pub(super) struct LogApiError {
    pub(super) code: ResponseCode,
    pub(super) message: String,
}

pub(super) trait LogSecurityPolicy {
    fn prepare_binding(path: &Path) -> Result<LogBinding>;
    fn validate_service_binding(service: &str, stream: &str, binding: &LogBinding) -> Result<PathBuf>;
    fn validate_rustpm_log_path(path: &Path) -> Result<PathBuf>;
}

pub(super) struct NativeLogSecurityPolicy;

impl LogSecurityPolicy for NativeLogSecurityPolicy {
    fn prepare_binding(path: &Path) -> Result<LogBinding> {
        ensure_log_file(path)?;
        let canonical = canonicalize_allowed_log_path(path)?;
        let identity = current_log_identity(&canonical)?;
        Ok(LogBinding {
            canonical_path: canonical.display().to_string(),
            identity,
        })
    }

    fn validate_service_binding(service: &str, stream: &str, binding: &LogBinding) -> Result<PathBuf> {
        let canonical = canonicalize_allowed_log_path(Path::new(&binding.canonical_path))?;
        rustpmd_logs::verify_log_file_permissions_strict(&canonical)?;
        let now = current_log_identity(&canonical)?;
        if now != binding.identity {
            return Err(anyhow!(
                "log access denied for service '{}' {}: file identity changed, run hard-restart",
                service,
                stream
            ));
        }
        Ok(canonical)
    }

    fn validate_rustpm_log_path(path: &Path) -> Result<PathBuf> {
        let canonical = canonicalize_allowed_log_path(path)?;
        rustpmd_logs::verify_log_file_permissions_relaxed(&canonical)?;
        Ok(canonical)
    }
}
