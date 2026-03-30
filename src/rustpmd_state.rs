use super::*;

#[derive(Debug, Clone)]
pub(super) struct RuntimeRecord {
    pub(super) state: ServiceState,
    pub(super) pid: Option<u32>,
    pub(super) restarts: u32,
    pub(super) message: String,
}

impl RuntimeRecord {
    pub(super) fn new(message: &str) -> Self {
        Self {
            state: ServiceState::Stopped,
            pid: None,
            restarts: 0,
            message: message.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum ServiceCommand {
    Start,
    Stop,
    Restart,
    Shutdown,
}

pub(super) enum ControlOutcome {
    Continue,
    Exit,
}

pub(super) struct RunningChild {
    pub(super) child: Child,
    pub(super) pgid: i32,
    pub(super) pid_u32: u32,
}

pub(super) struct StartedService {
    pub(super) child: RunningChild,
    pub(super) log_bindings: ServiceLogBindings,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ServiceLogBindings {
    pub(super) stdout: Option<LogBinding>,
    pub(super) stderr: Option<LogBinding>,
}

#[derive(Clone)]
pub(super) struct ServiceStartupPolicy {
    pub(super) first_start_not_before: Instant,
    pub(super) spin_interval: Duration,
    pub(super) spin_gate: Arc<tokio::sync::Mutex<Instant>>,
}

pub(super) type SharedState = Arc<RwLock<HashMap<String, RuntimeRecord>>>;
pub(super) type HandleMap = Arc<RwLock<HashMap<String, mpsc::Sender<ServiceCommand>>>>;
pub(super) type ElevatedSet = Arc<RwLock<HashSet<String>>>;
pub(super) type ServiceSourceMap = Arc<RwLock<HashMap<String, PathBuf>>>;
pub(super) type ServiceMetaMap = Arc<RwLock<HashMap<String, ServiceMeta>>>;

#[derive(Debug, Clone, Default)]
pub(super) struct ServiceMeta {
    pub(super) stdout_logfile: Option<String>,
    pub(super) stderr_logfile: Option<String>,
    pub(super) stdout_binding: Option<LogBinding>,
    pub(super) stderr_binding: Option<LogBinding>,
}

#[derive(Clone)]
pub(super) struct AppState {
    pub(super) handles: HandleMap,
    pub(super) state: SharedState,
    pub(super) web_auth: Arc<RwLock<WebAuthState>>,
    pub(super) sessions: Arc<RwLock<HashMap<String, SessionState>>>,
    pub(super) login_failures: Arc<RwLock<HashMap<String, LoginFailureState>>>,
    pub(super) elevated: ElevatedSet,
    pub(super) privd_socket: PathBuf,
    pub(super) daemon_config_path: PathBuf,
    pub(super) config_dir: PathBuf,
    pub(super) socket_path: PathBuf,
    pub(super) role: DaemonRole,
    pub(super) service_sources: ServiceSourceMap,
    pub(super) service_meta: ServiceMetaMap,
    pub(super) policies: RuntimePolicies,
    pub(super) contract: RuntimeContract,
    pub(super) startup_policy: ServiceStartupPolicy,
}

#[derive(Clone)]
pub(super) struct ControlRuntime {
    pub(super) handles: HandleMap,
    pub(super) state: SharedState,
    pub(super) elevated: ElevatedSet,
    pub(super) service_sources: ServiceSourceMap,
    pub(super) service_meta: ServiceMetaMap,
    pub(super) privd_socket: PathBuf,
    pub(super) daemon_config_path: PathBuf,
    pub(super) socket_path: PathBuf,
    pub(super) role: DaemonRole,
    pub(super) policies: RuntimePolicies,
    pub(super) contract: RuntimeContract,
    pub(super) startup_policy: ServiceStartupPolicy,
}

pub(super) fn control_runtime_from_state(state: &AppState) -> ControlRuntime {
    ControlRuntime {
        handles: Arc::clone(&state.handles),
        state: Arc::clone(&state.state),
        elevated: Arc::clone(&state.elevated),
        service_sources: Arc::clone(&state.service_sources),
        service_meta: Arc::clone(&state.service_meta),
        privd_socket: state.privd_socket.clone(),
        daemon_config_path: state.daemon_config_path.clone(),
        socket_path: state.socket_path.clone(),
        role: state.role,
        policies: state.policies.clone(),
        contract: state.contract.clone(),
        startup_policy: state.startup_policy.clone(),
    }
}

#[derive(Debug, Clone)]
pub(super) struct WebAuthState {
    pub(super) password_hash: String,
    pub(super) daemon_config_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(super) struct SessionState {
    pub(super) expires_at: u64,
    pub(super) user_agent_hash: String,
    pub(super) ip_prefix: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct LoginFailureState {
    pub(super) count: u32,
    pub(super) blocked_until: u64,
    pub(super) last_updated_at: u64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RescanSummary {
    pub(super) added: usize,
    pub(super) updated: usize,
    pub(super) removed: usize,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct ChangePasswordRequest {
    pub(super) current_password: String,
    pub(super) new_password: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct LoginRequest {
    pub(super) password: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct LogQuery {
    pub(super) lines: Option<usize>,
}
