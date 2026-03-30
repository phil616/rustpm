#[derive(Debug, Clone)]
pub struct WebSecurityPolicy {
    pub session_ttl_secs: u64,
    pub login_block_threshold: u32,
    pub login_block_secs: u64,
    pub login_failure_ttl_secs: u64,
    pub max_web_sessions: usize,
    pub max_login_failure_keys: usize,
}

impl Default for WebSecurityPolicy {
    fn default() -> Self {
        Self {
            session_ttl_secs: 30 * 60,
            login_block_threshold: 5,
            login_block_secs: 60,
            login_failure_ttl_secs: 30 * 60,
            max_web_sessions: 4096,
            max_login_failure_keys: 4096,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManagementSurfacePolicy {
    pub allow_web_daemon_config_write: bool,
    pub allow_web_program_config_write: bool,
    pub allow_web_rescan: bool,
    pub allow_web_service_control: bool,
}

impl Default for ManagementSurfacePolicy {
    fn default() -> Self {
        Self {
            allow_web_daemon_config_write: true,
            allow_web_program_config_write: false,
            allow_web_rescan: true,
            allow_web_service_control: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimePolicies {
    pub web_security: WebSecurityPolicy,
    pub management_surface: ManagementSurfacePolicy,
}

pub trait RuntimePolicyProvider {
    fn provider_name(&self) -> &'static str;
    fn policies(&self) -> RuntimePolicies;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultRuntimePolicyProvider;

impl RuntimePolicyProvider for DefaultRuntimePolicyProvider {
    fn provider_name(&self) -> &'static str {
        "default-v1"
    }

    fn policies(&self) -> RuntimePolicies {
        RuntimePolicies::default()
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimePolicies;

    #[test]
    fn management_surface_defaults_are_explicit() {
        let p = RuntimePolicies::default();
        assert!(p.management_surface.allow_web_daemon_config_write);
        assert!(!p.management_surface.allow_web_program_config_write);
        assert!(p.management_surface.allow_web_service_control);
        assert!(p.management_surface.allow_web_rescan);
    }
}
