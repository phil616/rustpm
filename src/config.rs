use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
#[cfg(unix)]
use nix::unistd::User;

use crate::auth::{default_password_hash, is_hash_format};
#[cfg(unix)]
use crate::platform_fs;

#[cfg(unix)]
pub const DEFAULT_DAEMON_CONFIG_PATH: &str = "/usr/local/etc/rustpm/rustpmd.conf";
#[cfg(unix)]
pub const DEFAULT_CONFIG_DIR: &str = "/usr/local/etc/rustpm/programs.d";
#[cfg(unix)]
pub const SECURE_CONFIG_ROOT: &str = "/usr/local/etc/rustpm";
#[cfg(unix)]
pub const DEFAULT_SOCKET_PATH: &str = "/usr/local/var/run/rustpm/rustpmd.sock";
#[cfg(unix)]
pub const DEFAULT_PRIVD_SOCKET_PATH: &str = "/usr/local/var/run/rustpm/rustpm-privd.sock";
#[cfg(unix)]
pub const DEFAULT_LOG_DIR: &str = "/usr/local/var/log";

#[cfg(windows)]
pub const DEFAULT_DAEMON_CONFIG_PATH: &str = "C:\\Program Files\\rustpm\\config\\rustpmd.conf";
#[cfg(windows)]
pub const DEFAULT_CONFIG_DIR: &str = "C:\\Program Files\\rustpm\\config\\programs.d";
#[cfg(windows)]
pub const SECURE_CONFIG_ROOT: &str = "C:\\Program Files\\rustpm\\config";
#[cfg(windows)]
pub const DEFAULT_SOCKET_PATH: &str = "\\\\.\\pipe\\rustpmd";
#[cfg(windows)]
pub const DEFAULT_PRIVD_SOCKET_PATH: &str = "\\\\.\\pipe\\rustpm-privd";
#[cfg(windows)]
pub const DEFAULT_LOG_DIR: &str = "C:\\Program Files\\rustpm\\logs";

pub const DEFAULT_WEB_BIND: &str = "127.0.0.1";
pub const DEFAULT_WEB_PORT: u16 = 18765;
pub const CONFIG_SCHEMA_V1: &str = "v1";
const MAX_SERVICE_NAME_LEN: usize = 64;
const ALLOWED_PROGRAM_KEYS: &[&str] = &[
    "directory",
    "command",
    "user",
    "startsecs",
    "autorestart",
    "redirect_stderr",
    "stdout_logfile_maxbytes",
    "stdout_logfile_backups",
    "stdout_logfile",
    "stderr_logfile",
    "privilege",
];

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub socket_path: PathBuf,
    pub privd_socket_path: PathBuf,
    pub config_dir: PathBuf,
    pub web: WebConfig,
    pub dependency: DependencyConfig,
    pub services: Vec<ServiceConfig>,
    pub program_warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSchemaVersion {
    V1,
}

impl ConfigSchemaVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => CONFIG_SCHEMA_V1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeConfigEnvelope {
    pub schema_version: ConfigSchemaVersion,
    pub runtime: RuntimeConfig,
}

pub trait RuntimeConfigLoadPolicy {
    fn strict_program_parse(&self) -> bool;
    fn target_schema(&self) -> ConfigSchemaVersion;
    fn program_parse_policy(&self) -> ProgramParsePolicy;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RunModeLoadPolicy;

impl RuntimeConfigLoadPolicy for RunModeLoadPolicy {
    fn strict_program_parse(&self) -> bool {
        false
    }

    fn target_schema(&self) -> ConfigSchemaVersion {
        ConfigSchemaVersion::V1
    }

    fn program_parse_policy(&self) -> ProgramParsePolicy {
        ProgramParsePolicy::v1()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CheckModeLoadPolicy;

impl RuntimeConfigLoadPolicy for CheckModeLoadPolicy {
    fn strict_program_parse(&self) -> bool {
        true
    }

    fn target_schema(&self) -> ConfigSchemaVersion {
        ConfigSchemaVersion::V1
    }

    fn program_parse_policy(&self) -> ProgramParsePolicy {
        ProgramParsePolicy::v1()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ProgramParsePolicy {
    pub reject_unknown_keys: bool,
    pub max_service_name_len: usize,
}

impl ProgramParsePolicy {
    pub const fn v1() -> Self {
        Self {
            reject_unknown_keys: true,
            max_service_name_len: MAX_SERVICE_NAME_LEN,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgramServiceEntry {
    pub service: ServiceConfig,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DependencyConfig {
    pub require_privd: bool,
    pub startup_timeout_secs: u64,
    pub healthcheck_interval_secs: u64,
    pub failure_threshold: u32,
    pub service_start_delay_secs: u64,
    pub service_start_spin_interval_secs: u64,
}

impl Default for DependencyConfig {
    fn default() -> Self {
        Self {
            require_privd: true,
            startup_timeout_secs: 30,
            healthcheck_interval_secs: 5,
            failure_threshold: 3,
            service_start_delay_secs: 30,
            service_start_spin_interval_secs: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WebConfig {
    pub enabled: bool,
    pub bind_ip: IpAddr,
    pub port: u16,
    pub expose: bool,
    pub password_hash: String,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind_ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            port: DEFAULT_WEB_PORT,
            expose: false,
            password_hash: default_password_hash(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default)]
    pub restart: RestartConfig,
    #[serde(default = "default_start_secs")]
    pub start_secs: u64,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub redirect_stderr: bool,
    #[serde(default)]
    pub stdout_logfile: Option<PathBuf>,
    #[serde(default)]
    pub stderr_logfile: Option<PathBuf>,
    #[serde(default)]
    pub stdout_logfile_maxbytes: Option<u64>,
    #[serde(default)]
    pub stdout_logfile_backups: Option<u32>,
    #[serde(default)]
    pub privilege: PrivilegeMode,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PrivilegeMode {
    Normal,
    Elevated,
}

impl Default for PrivilegeMode {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RestartConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default = "default_backoff_secs")]
    pub backoff_secs: u64,
    #[serde(default = "default_stop_grace_secs")]
    pub stop_grace_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_backoff_secs() -> u64 {
    2
}

fn default_stop_grace_secs() -> u64 {
    10
}

fn default_start_secs() -> u64 {
    1
}

impl Default for RestartConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 0,
            backoff_secs: default_backoff_secs(),
            stop_grace_secs: default_stop_grace_secs(),
        }
    }
}

pub fn default_daemon_config_path() -> PathBuf {
    PathBuf::from(DEFAULT_DAEMON_CONFIG_PATH)
}

pub fn default_config_dir() -> PathBuf {
    PathBuf::from(DEFAULT_CONFIG_DIR)
}

pub fn default_socket_path() -> PathBuf {
    PathBuf::from(DEFAULT_SOCKET_PATH)
}

pub fn default_privd_socket_path() -> PathBuf {
    PathBuf::from(DEFAULT_PRIVD_SOCKET_PATH)
}

pub fn load_runtime_config(
    daemon_config_path: &Path,
    config_dir_override: Option<PathBuf>,
    socket_override: Option<PathBuf>,
) -> Result<RuntimeConfig> {
    Ok(
        load_runtime_config_envelope(daemon_config_path, config_dir_override, socket_override)?
            .runtime,
    )
}

pub fn load_runtime_config_for_run(
    daemon_config_path: &Path,
    config_dir_override: Option<PathBuf>,
    socket_override: Option<PathBuf>,
) -> Result<RuntimeConfig> {
    Ok(
        load_runtime_config_envelope_for_run(daemon_config_path, config_dir_override, socket_override)?
            .runtime,
    )
}

pub fn load_runtime_config_envelope(
    daemon_config_path: &Path,
    config_dir_override: Option<PathBuf>,
    socket_override: Option<PathBuf>,
) -> Result<RuntimeConfigEnvelope> {
    let policy = CheckModeLoadPolicy;
    load_runtime_config_with_policy(daemon_config_path, config_dir_override, socket_override, &policy)
}

pub fn load_runtime_config_envelope_for_run(
    daemon_config_path: &Path,
    config_dir_override: Option<PathBuf>,
    socket_override: Option<PathBuf>,
) -> Result<RuntimeConfigEnvelope> {
    let policy = RunModeLoadPolicy;
    load_runtime_config_with_policy(daemon_config_path, config_dir_override, socket_override, &policy)
}

fn load_runtime_config_with_policy(
    daemon_config_path: &Path,
    config_dir_override: Option<PathBuf>,
    socket_override: Option<PathBuf>,
    policy: &dyn RuntimeConfigLoadPolicy,
) -> Result<RuntimeConfigEnvelope> {
    let schema_version = policy.target_schema();
    validate_target_schema(schema_version)?;
    let daemon_cfg = load_daemon_file(daemon_config_path)?;
    let config_dir = config_dir_override
        .or(daemon_cfg.config_dir)
        .unwrap_or_else(default_config_dir);
    let socket_path = socket_override
        .or(daemon_cfg.socket)
        .unwrap_or_else(default_socket_path);
    let privd_socket_path = daemon_cfg
        .privd_socket
        .unwrap_or_else(default_privd_socket_path);
    let web = daemon_cfg.web;
    let dependency = daemon_cfg.dependency;

    validate_runtime_config_security(daemon_config_path, &config_dir)?;

    let (entries, program_warnings) = load_program_service_entries(
        &config_dir,
        policy.strict_program_parse(),
        policy.program_parse_policy(),
    )?;
    let services: Vec<ServiceConfig> = entries.into_iter().map(|entry| entry.service).collect();
    validate_services(&services)?;
    validate_web_config(&web)?;
    validate_dependency_config(&dependency)?;

    Ok(RuntimeConfigEnvelope {
        schema_version,
        runtime: RuntimeConfig {
        socket_path,
        privd_socket_path,
        config_dir,
        web,
        dependency,
        services,
        program_warnings,
        },
    })
}

fn validate_target_schema(schema_version: ConfigSchemaVersion) -> Result<()> {
    match schema_version {
        ConfigSchemaVersion::V1 => Ok(()),
    }
}

#[derive(Debug, Default)]
struct DaemonFileConfig {
    config_dir: Option<PathBuf>,
    socket: Option<PathBuf>,
    privd_socket: Option<PathBuf>,
    web: WebConfig,
    dependency: DependencyConfig,
}

fn load_daemon_file(path: &Path) -> Result<DaemonFileConfig> {
    if !path.exists() {
        return Ok(DaemonFileConfig::default());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read daemon config: {}", path.display()))?;
    let sections = parse_ini_like(path, &raw)?;

    let root = sections.get("").cloned().unwrap_or_default();
    let web_sec = sections.get("web").cloned().unwrap_or_default();
    let dep_sec = sections.get("dependency").cloned().unwrap_or_default();

    let config_dir = root.get("config_dir").map(PathBuf::from);
    let socket = root.get("socket").map(PathBuf::from);
    let privd_socket = root.get("privd_socket").map(PathBuf::from);

    let enabled = parse_flag(
        web_sec.get("enabled").or(root.get("web_enabled")),
        true,
        "web.enabled",
    )?;
    let bind_ip = parse_ip(
        web_sec.get("bind").or(root.get("web_bind")),
        DEFAULT_WEB_BIND,
        "web.bind",
    )?;
    let port = parse_u16(
        web_sec.get("port").or(root.get("web_port")),
        DEFAULT_WEB_PORT,
        "web.port",
    )?;
    let expose = parse_flag(
        web_sec.get("expose").or(root.get("web_expose")),
        false,
        "web.expose",
    )?;
    let password_hash = web_sec
        .get("password_hash")
        .or(root.get("web_password_hash"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if web_sec.contains_key("password") || root.contains_key("web_password") {
        return Err(anyhow!(
            "plain web password is not supported; use web.password_hash=argon2id:<phc>"
        ));
    }
    if web_sec.contains_key("token")
        || root.contains_key("web_token")
        || web_sec.contains_key("token_hash")
        || root.contains_key("web_token_hash")
    {
        return Err(anyhow!(
            "token naming is removed; use web.password_hash=argon2id:<phc>"
        ));
    }
    let password_hash = resolve_password_hash(password_hash.as_deref())?;
    let require_privd = parse_flag(
        dep_sec.get("require_privd").or(root.get("require_privd")),
        true,
        "dependency.require_privd",
    )?;
    let startup_timeout_secs = parse_u64_global(
        dep_sec
            .get("startup_timeout_secs")
            .or(root.get("dependency_startup_timeout_secs")),
        30,
        "dependency.startup_timeout_secs",
    )?;
    let healthcheck_interval_secs = parse_u64_global(
        dep_sec
            .get("healthcheck_interval_secs")
            .or(root.get("dependency_healthcheck_interval_secs")),
        5,
        "dependency.healthcheck_interval_secs",
    )?;
    let failure_threshold = parse_u32_global(
        dep_sec
            .get("failure_threshold")
            .or(root.get("dependency_failure_threshold")),
        3,
        "dependency.failure_threshold",
    )?;
    let service_start_delay_secs = parse_u64_global(
        dep_sec
            .get("service_start_delay_secs")
            .or(root.get("dependency_service_start_delay_secs")),
        30,
        "dependency.service_start_delay_secs",
    )?;
    let service_start_spin_interval_secs = parse_u64_global(
        dep_sec
            .get("service_start_spin_interval_secs")
            .or(root.get("dependency_service_start_spin_interval_secs")),
        1,
        "dependency.service_start_spin_interval_secs",
    )?;

    Ok(DaemonFileConfig {
        config_dir,
        socket,
        privd_socket,
        web: WebConfig {
            enabled,
            bind_ip,
            port,
            expose,
            password_hash,
        },
        dependency: DependencyConfig {
            require_privd,
            startup_timeout_secs,
            healthcheck_interval_secs,
            failure_threshold,
            service_start_delay_secs,
            service_start_spin_interval_secs,
        },
    })
}

fn parse_ini_like(path: &Path, raw: &str) -> Result<BTreeMap<String, HashMap<String, String>>> {
    let mut sections: BTreeMap<String, HashMap<String, String>> = BTreeMap::new();
    let mut current = String::new();
    sections.entry(current.clone()).or_default();

    for (idx, line) in raw.lines().enumerate() {
        let lnum = idx + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].trim().to_ascii_lowercase();
            sections.entry(current.clone()).or_default();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(anyhow!(
                "invalid key-value in {}:{}",
                path.display(),
                lnum
            ));
        };
        sections
            .entry(current.clone())
            .or_default()
            .insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    Ok(sections)
}

pub fn load_program_service_entries(
    config_dir: &Path,
    strict_program_parse: bool,
    parse_policy: ProgramParsePolicy,
) -> Result<(Vec<ProgramServiceEntry>, Vec<String>)> {
    if !config_dir.exists() {
        return Err(anyhow!(
            "config_dir does not exist: {}",
            config_dir.display()
        ));
    }
    if !config_dir.is_dir() {
        return Err(anyhow!("config_dir is not a directory: {}", config_dir.display()));
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(config_dir)
        .with_context(|| format!("failed to read config_dir: {}", config_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|v| v.to_str()).unwrap_or_default();
        if ext.eq_ignore_ascii_case("conf") || ext.eq_ignore_ascii_case("ini") {
            files.push(path);
        }
    }
    files.sort();

    let mut services = Vec::new();
    let mut warnings = Vec::new();
    for file in files {
        if let Err(err) = ensure_runtime_program_file_security(&file) {
            if strict_program_parse {
                return Err(err);
            }
            warnings.push(format!(
                "skip insecure program config {}: {}",
                file.display(),
                err
            ));
            continue;
        }
        let (mut parsed, mut parse_warnings) = parse_program_file_entries(&file, parse_policy);
        services.append(&mut parsed);
        warnings.append(&mut parse_warnings);
    }

    if strict_program_parse && !warnings.is_empty() {
        return Err(anyhow!(
            "program config validation failed:\n{}",
            warnings.join("\n")
        ));
    }

    Ok((services, warnings))
}

pub fn load_program_services_from_file(path: &Path) -> Result<Vec<ProgramServiceEntry>> {
    if !path.exists() {
        return Err(anyhow!("program config file does not exist: {}", path.display()));
    }
    if !path.is_file() {
        return Err(anyhow!("program config path is not a file: {}", path.display()));
    }
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or_default();
    if !ext.eq_ignore_ascii_case("conf") && !ext.eq_ignore_ascii_case("ini") {
        return Err(anyhow!(
            "unsupported program config extension for {} (expected .conf/.ini)",
            path.display()
        ));
    }
    ensure_runtime_program_file_security(path)?;
    let (entries, warnings) = parse_program_file_entries(path, ProgramParsePolicy::v1());
    if !warnings.is_empty() {
        return Err(anyhow!(
            "program config validation failed for {}:\n{}",
            path.display(),
            warnings.join("\n")
        ));
    }
    let services: Vec<ServiceConfig> = entries.iter().map(|entry| entry.service.clone()).collect();
    validate_services(&services)?;
    Ok(entries)
}

fn parse_program_file_entries(
    path: &Path,
    parse_policy: ProgramParsePolicy,
) -> (Vec<ProgramServiceEntry>, Vec<String>) {
    let raw = match fs::read_to_string(path)
        .with_context(|| format!("failed to read program file: {}", path.display()))
    {
        Ok(raw) => raw,
        Err(err) => return (Vec::new(), vec![err.to_string()]),
    };

    let mut sections: BTreeMap<String, HashMap<String, String>> = BTreeMap::new();
    let mut current_section: Option<String> = None;
    let mut warnings = Vec::new();

    for (idx, line) in raw.lines().enumerate() {
        let lnum = idx + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            let sec = line[1..line.len() - 1].trim().to_string();
            current_section = Some(sec.clone());
            sections.entry(sec).or_default();
            continue;
        }

        let Some(section_name) = current_section.clone() else {
            warnings.push(format!(
                "invalid line before section in {}:{}",
                path.display(),
                lnum
            ));
            return (Vec::new(), warnings);
        };

        let Some((key, value)) = line.split_once('=') else {
            warnings.push(format!(
                "invalid key-value in {}:{}",
                path.display(),
                lnum
            ));
            return (Vec::new(), warnings);
        };
        sections
            .entry(section_name)
            .or_default()
            .insert(key.trim().to_lowercase(), value.trim().to_string());
    }

    let mut out = Vec::new();
    for (section, kv) in sections {
        let Some(name) = section.strip_prefix("program:") else {
            continue;
        };
        match build_service_config(path, name.trim(), &kv, parse_policy) {
            Ok(svc) => out.push(ProgramServiceEntry {
                service: svc,
                source_path: path.to_path_buf(),
            }),
            Err(err) => warnings.push(err.to_string()),
        }
    }
    (out, warnings)
}

#[cfg(test)]
fn parse_program_file(path: &Path) -> (Vec<ServiceConfig>, Vec<String>) {
    let (entries, warnings) = parse_program_file_entries(path, ProgramParsePolicy::v1());
    (
        entries.into_iter().map(|entry| entry.service).collect(),
        warnings,
    )
}

fn build_service_config(
    source_path: &Path,
    program_name: &str,
    kv: &HashMap<String, String>,
    parse_policy: ProgramParsePolicy,
) -> Result<ServiceConfig> {
    if program_name.is_empty() {
        return Err(anyhow!(
            "program section name cannot be empty in {}",
            source_path.display()
        ));
    }
    validate_service_name(program_name, parse_policy)?;
    ensure_supported_program_keys(program_name, kv, parse_policy)?;

    let command_raw = kv
        .get("command")
        .ok_or_else(|| anyhow!("program '{}' missing command", program_name))?;
    let argv = shlex::split(command_raw)
        .ok_or_else(|| anyhow!("program '{}' command parse failed", program_name))?;
    if argv.is_empty() {
        return Err(anyhow!("program '{}' command is empty", program_name));
    }
    let command = argv[0].clone();
    let args = argv[1..].to_vec();

    let directory = kv.get("directory").map(PathBuf::from);
    let user = kv
        .get("user")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let start_secs = parse_u64(kv.get("startsecs"), 1, "startsecs", program_name)?;
    let autorestart = parse_bool(kv.get("autorestart"), true, "autorestart", program_name)?;
    let redirect_stderr =
        parse_bool(kv.get("redirect_stderr"), false, "redirect_stderr", program_name)?;

    let stdout_logfile = kv
        .get("stdout_logfile")
        .map(|v| expand_program_name(v, program_name))
        .map(PathBuf::from);
    let stderr_logfile = kv
        .get("stderr_logfile")
        .map(|v| expand_program_name(v, program_name))
        .map(PathBuf::from);
    let stdout_logfile_maxbytes =
        parse_bytes_opt(kv.get("stdout_logfile_maxbytes"), program_name)?;
    let stdout_logfile_backups =
        parse_u32_opt(kv.get("stdout_logfile_backups"), program_name, "stdout_logfile_backups")?;
    let mut privilege = match kv.get("privilege").map(|v| v.trim().to_ascii_lowercase()) {
        None => PrivilegeMode::Normal,
        Some(v) if v == "normal" => PrivilegeMode::Normal,
        Some(v) if v == "elevated" => PrivilegeMode::Elevated,
        Some(v) => {
            return Err(anyhow!(
                "program '{}' has invalid privilege: {}",
                program_name,
                v
            ))
        }
    };

    if let Some(u) = user.as_deref() {
        let user_is_elevated = is_elevated_user(u);
        if privilege == PrivilegeMode::Elevated && !user_is_elevated {
            return Err(anyhow!(
                "program '{}' has invalid user '{}' for privilege=elevated; only root or Administrator are allowed",
                program_name,
                u
            ));
        }
        if user_is_elevated {
            privilege = PrivilegeMode::Elevated;
        }
    }

    Ok(ServiceConfig {
        name: program_name.to_string(),
        command,
        args,
        working_dir: directory,
        autostart: true,
        start_secs,
        user,
        redirect_stderr,
        stdout_logfile,
        stderr_logfile,
        stdout_logfile_maxbytes,
        stdout_logfile_backups,
        privilege,
        restart: RestartConfig {
            enabled: autorestart,
            ..RestartConfig::default()
        },
    })
}

fn validate_service_name(name: &str, parse_policy: ProgramParsePolicy) -> Result<()> {
    if name.len() > parse_policy.max_service_name_len {
        return Err(anyhow!(
            "service '{}' name too long (max {})",
            name,
            parse_policy.max_service_name_len
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(anyhow!(
            "service '{}' name contains invalid characters; allowed: [A-Za-z0-9._-]",
            name
        ));
    }
    Ok(())
}

fn ensure_supported_program_keys(
    program_name: &str,
    kv: &HashMap<String, String>,
    parse_policy: ProgramParsePolicy,
) -> Result<()> {
    if !parse_policy.reject_unknown_keys {
        return Ok(());
    }
    let allowed: HashSet<&str> = ALLOWED_PROGRAM_KEYS.iter().copied().collect();
    let mut unknown: Vec<&str> = kv
        .keys()
        .map(String::as_str)
        .filter(|k| !allowed.contains(*k))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    unknown.sort_unstable();
    Err(anyhow!(
        "program '{}' has unsupported keys: {}",
        program_name,
        unknown.join(", ")
    ))
}

fn is_elevated_user(user: &str) -> bool {
    let normalized = user.trim().to_ascii_lowercase();
    normalized == "root" || normalized == "administrator"
}

fn parse_bool(input: Option<&String>, default: bool, key: &str, program: &str) -> Result<bool> {
    let Some(v) = input else {
        return Ok(default);
    };
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(anyhow!(
            "{} has invalid bool for {}: {}",
            program,
            key,
            v
        )),
    }
}

fn parse_flag(input: Option<&String>, default: bool, key: &str) -> Result<bool> {
    let Some(v) = input else {
        return Ok(default);
    };
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(anyhow!("invalid bool for {}: {}", key, v)),
    }
}

fn parse_u16(input: Option<&String>, default: u16, key: &str) -> Result<u16> {
    let Some(v) = input else {
        return Ok(default);
    };
    v.trim()
        .parse::<u16>()
        .with_context(|| format!("invalid number for {}: {}", key, v))
}

fn parse_u64_global(input: Option<&String>, default: u64, key: &str) -> Result<u64> {
    let Some(v) = input else {
        return Ok(default);
    };
    v.trim()
        .parse::<u64>()
        .with_context(|| format!("invalid number for {}: {}", key, v))
}

fn parse_u32_global(input: Option<&String>, default: u32, key: &str) -> Result<u32> {
    let Some(v) = input else {
        return Ok(default);
    };
    v.trim()
        .parse::<u32>()
        .with_context(|| format!("invalid number for {}: {}", key, v))
}

fn parse_ip(input: Option<&String>, default: &str, key: &str) -> Result<IpAddr> {
    let raw = input.map(|v| v.trim()).unwrap_or(default);
    raw.parse::<IpAddr>()
        .with_context(|| format!("invalid IP for {}: {}", key, raw))
}

fn parse_u64(input: Option<&String>, default: u64, key: &str, program: &str) -> Result<u64> {
    let Some(v) = input else {
        return Ok(default);
    };
    v.trim().parse::<u64>().with_context(|| {
        format!(
            "program '{}' has invalid number for {}: {}",
            program, key, v
        )
    })
}

fn parse_u32_opt(input: Option<&String>, program: &str, key: &str) -> Result<Option<u32>> {
    let Some(v) = input else {
        return Ok(None);
    };
    let parsed = v.trim().parse::<u32>().with_context(|| {
        format!(
            "program '{}' has invalid number for {}: {}",
            program, key, v
        )
    })?;
    Ok(Some(parsed))
}

fn parse_bytes_opt(input: Option<&String>, program: &str) -> Result<Option<u64>> {
    let Some(v) = input else {
        return Ok(None);
    };
    parse_bytes(v.trim())
        .with_context(|| format!("program '{}' has invalid stdout_logfile_maxbytes: {}", program, v))
        .map(Some)
}

fn parse_bytes(v: &str) -> Result<u64> {
    let upper = v.trim().to_ascii_uppercase();
    let (num_part, mul) = if let Some(n) = upper.strip_suffix("KB") {
        (n, 1024_u64)
    } else if let Some(n) = upper.strip_suffix("MB") {
        (n, 1024_u64 * 1024)
    } else if let Some(n) = upper.strip_suffix("GB") {
        (n, 1024_u64 * 1024 * 1024)
    } else if let Some(n) = upper.strip_suffix('B') {
        (n, 1_u64)
    } else {
        (upper.as_str(), 1_u64)
    };

    let num = num_part
        .trim()
        .parse::<u64>()
        .with_context(|| format!("invalid byte number: {}", v))?;
    num.checked_mul(mul)
        .ok_or_else(|| anyhow!("byte value overflow: {}", v))
}

fn expand_program_name(template: &str, program_name: &str) -> String {
    template.replace("%(program_name)s", program_name)
}

#[cfg(unix)]
fn validate_runtime_config_security(daemon_config_path: &Path, config_dir: &Path) -> Result<()> {
    if should_enforce_secure_path(daemon_config_path) {
        validate_secure_owner_permissions(daemon_config_path, false)?;
    }
    if should_enforce_secure_path(config_dir) {
        ensure_runtime_program_dir_security(config_dir)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_runtime_config_security(_daemon_config_path: &Path, _config_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn ensure_runtime_program_file_security(path: &Path) -> Result<()> {
    let fs_policy = platform_fs::default_policy_adapter();
    if !should_enforce_secure_path(path) {
        return Ok(());
    }
    if validate_secure_owner_permissions(path, false).is_ok() {
        return Ok(());
    }
    if !fs_policy.running_as_root() {
        return validate_secure_owner_permissions(path, false);
    }

    repair_program_file_security(path)?;
    validate_secure_owner_permissions(path, false)
}

#[cfg(not(unix))]
fn ensure_runtime_program_file_security(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn ensure_runtime_program_dir_security(path: &Path) -> Result<()> {
    let fs_policy = platform_fs::default_policy_adapter();
    if validate_secure_owner_permissions(path, true).is_ok() {
        return Ok(());
    }
    if !fs_policy.running_as_root() {
        return validate_secure_owner_permissions(path, true);
    }
    repair_program_dir_security(path)?;
    validate_secure_owner_permissions(path, true)
}

#[cfg(not(unix))]
#[allow(dead_code)]
fn ensure_runtime_program_dir_security(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn repair_program_file_security(path: &Path) -> Result<()> {
    let fs_policy = platform_fs::default_policy_adapter();
    fs_policy.set_owner_root(path)
        .with_context(|| format!("failed to fix owner for program config: {}", path.display()))?;
    fs_policy.set_file_mode(path, 0o600)
        .with_context(|| format!("failed to fix mode for program config: {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
#[allow(dead_code)]
fn repair_program_file_security(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn repair_program_dir_security(path: &Path) -> Result<()> {
    let fs_policy = platform_fs::default_policy_adapter();
    fs_policy.set_owner_root(path).with_context(|| {
        format!(
            "failed to fix owner for program config dir: {}",
            path.display()
        )
    })?;
    fs_policy.set_file_mode(path, 0o700).with_context(|| {
        format!(
            "failed to fix mode for program config dir: {}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
#[allow(dead_code)]
fn repair_program_dir_security(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn validate_secure_owner_permissions(path: &Path, is_dir: bool) -> Result<()> {
    let fs_policy = platform_fs::default_policy_adapter();
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::metadata(path)
        .with_context(|| format!("failed to stat secure path: {}", path.display()))?;
    if is_dir && !meta.is_dir() {
        return Err(anyhow!("expected directory: {}", path.display()));
    }
    if !is_dir && !meta.is_file() {
        return Err(anyhow!("expected file: {}", path.display()));
    }
    fs_policy.verify_root_owned(path)?;
    let required = if is_dir { 0o700 } else { 0o600 };
    fs_policy.verify_file_mode_exact(path, required)?;
    Ok(())
}

#[cfg(unix)]
fn should_enforce_secure_path(path: &Path) -> bool {
    path.starts_with(Path::new(SECURE_CONFIG_ROOT))
}


fn validate_services(services: &[ServiceConfig]) -> Result<()> {
    let mut names = HashSet::new();
    for service in services {
        if service.name.trim().is_empty() {
            return Err(anyhow!("service name cannot be empty"));
        }
        validate_service_name(&service.name, ProgramParsePolicy::v1())?;
        if !names.insert(service.name.clone()) {
            return Err(anyhow!("duplicated service name: {}", service.name));
        }
        let cmd = Path::new(service.command.trim());
        if !cmd.is_absolute() {
            return Err(anyhow!(
                "service '{}' command must be an absolute path",
                service.name
            ));
        }
        if let Some(dir) = &service.working_dir {
            if !dir.is_absolute() {
                return Err(anyhow!(
                    "service '{}' working_dir must be an absolute path",
                    service.name
                ));
            }
        }
        if service.restart.backoff_secs > 3600 {
            return Err(anyhow!(
                "service '{}' backoff_secs must be <= 3600",
                service.name
            ));
        }
        if service.restart.stop_grace_secs == 0 || service.restart.stop_grace_secs > 3600 {
            return Err(anyhow!(
                "service '{}' stop_grace_secs must be between 1 and 3600",
                service.name
            ));
        }
        if service.start_secs > 3600 {
            return Err(anyhow!(
                "service '{}' startsecs must be <= 3600",
                service.name
            ));
        }
        validate_service_user(service)?;
        validate_service_privilege_path_security(service)?;
    }
    Ok(())
}

#[cfg(unix)]
fn validate_service_user(service: &ServiceConfig) -> Result<()> {
    let Some(user) = service.user.as_ref() else {
        return Ok(());
    };
    if user.trim().is_empty() {
        return Err(anyhow!("service '{}' user cannot be empty", service.name));
    }
    if service.privilege == PrivilegeMode::Elevated {
        if !is_elevated_user(user) {
            return Err(anyhow!(
                "service '{}' has invalid elevated user '{}'; expected root",
                service.name,
                user
            ));
        }
        return Ok(());
    }
    if is_elevated_user(user) {
        return Err(anyhow!(
            "service '{}' has user '{}' but privilege is normal; set privilege=elevated or use a non-root account",
            service.name,
            user
        ));
    }
    let lookup = User::from_name(user)
        .with_context(|| format!("service '{}' failed to resolve user '{}'", service.name, user))?;
    if lookup.is_none() {
        return Err(anyhow!(
            "service '{}' user '{}' does not exist on this host",
            service.name,
            user
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_service_user(_service: &ServiceConfig) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn validate_service_privilege_path_security(service: &ServiceConfig) -> Result<()> {
    match service.privilege {
        PrivilegeMode::Elevated => {
            validate_secure_path_for_elevated(
                &PathBuf::from(&service.command),
                false,
                &service.name,
                "command",
            )?;
        }
        PrivilegeMode::Normal => {
            if let Some(dir) = &service.working_dir {
                validate_non_root_owned_for_normal(dir, &service.name, "working_dir")?;
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_service_privilege_path_security(_service: &ServiceConfig) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn validate_secure_path_for_elevated(
    path: &Path,
    expect_dir: bool,
    service_name: &str,
    field_name: &str,
) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let meta = fs::metadata(path).with_context(|| {
        format!(
            "service '{}' {} does not exist: {}",
            service_name,
            field_name,
            path.display()
        )
    })?;
    if expect_dir && !meta.is_dir() {
        return Err(anyhow!(
            "service '{}' {} must be a directory: {}",
            service_name,
            field_name,
            path.display()
        ));
    }
    if !expect_dir && !meta.is_file() {
        return Err(anyhow!(
            "service '{}' {} must be a file: {}",
            service_name,
            field_name,
            path.display()
        ));
    }
    if meta.uid() != 0 {
        return Err(anyhow!(
            "service '{}' {} must be root-owned for privilege=elevated: {}",
            service_name,
            field_name,
            path.display()
        ));
    }
    let mode = meta.permissions().mode() & 0o777;
    if (mode & 0o022) != 0 {
        return Err(anyhow!(
            "service '{}' {} must not be group/other writable for privilege=elevated: {} (mode {:04o})",
            service_name,
            field_name,
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_non_root_owned_for_normal(
    path: &Path,
    service_name: &str,
    field_name: &str,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let meta = fs::metadata(path).with_context(|| {
        format!(
            "service '{}' {} does not exist: {}",
            service_name,
            field_name,
            path.display()
        )
    })?;
    if !meta.is_dir() {
        return Err(anyhow!(
            "service '{}' {} must be a directory: {}",
            service_name,
            field_name,
            path.display()
        ));
    }
    if meta.uid() == 0 {
        return Err(anyhow!(
            "service '{}' {} must not be root-owned when privilege=normal: {}",
            service_name,
            field_name,
            path.display()
        ));
    }
    Ok(())
}

fn validate_web_config(web: &WebConfig) -> Result<()> {
    if !web.enabled {
        return Ok(());
    }
    if !is_hash_format(&web.password_hash) {
        return Err(anyhow!(
            "web.password_hash must be in format argon2id:<phc>"
        ));
    }
    let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    if web.bind_ip != localhost && !web.expose {
        return Err(anyhow!(
            "web.bind is not 127.0.0.1; set web.expose=true explicitly"
        ));
    }
    Ok(())
}

fn resolve_password_hash(password_hash: Option<&str>) -> Result<String> {
    if let Some(password_hash) = password_hash {
        if !is_hash_format(password_hash) {
            return Err(anyhow!(
                "web.password_hash must be in format argon2id:<phc>"
            ));
        }
        return Ok(password_hash.to_string());
    }
    Ok(default_password_hash())
}

fn validate_dependency_config(dep: &DependencyConfig) -> Result<()> {
    if dep.startup_timeout_secs == 0 || dep.startup_timeout_secs > 600 {
        return Err(anyhow!(
            "dependency.startup_timeout_secs must be between 1 and 600"
        ));
    }
    if dep.healthcheck_interval_secs == 0 || dep.healthcheck_interval_secs > 300 {
        return Err(anyhow!(
            "dependency.healthcheck_interval_secs must be between 1 and 300"
        ));
    }
    if dep.failure_threshold == 0 || dep.failure_threshold > 120 {
        return Err(anyhow!(
            "dependency.failure_threshold must be between 1 and 120"
        ));
    }
    if dep.service_start_delay_secs > 300 {
        return Err(anyhow!(
            "dependency.service_start_delay_secs must be between 0 and 300"
        ));
    }
    if dep.service_start_spin_interval_secs > 30 {
        return Err(anyhow!(
            "dependency.service_start_spin_interval_secs must be between 0 and 30"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        load_runtime_config, load_runtime_config_envelope, load_runtime_config_envelope_for_run,
        parse_bytes, parse_program_file, resolve_password_hash, ConfigSchemaVersion,
    };
    use crate::auth::default_password_hash;
    use std::{fs, path::PathBuf};

    fn test_temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rustpm-test-{}", name))
    }

    fn test_cmd() -> String {
        #[cfg(unix)]
        { "/bin/sleep".into() }
        #[cfg(windows)]
        { "C:/Windows/System32/cmd.exe".into() }
    }

    fn test_cmd_with_arg(arg: &str) -> String {
        #[cfg(unix)]
        { format!("/bin/sleep {}", arg) }
        #[cfg(windows)]
        { format!("{test_cmd}", test_cmd = test_cmd()) }
    }

    #[test]
    fn parse_bytes_units() {
        assert_eq!(parse_bytes("50MB").expect("parse"), 50 * 1024 * 1024);
        assert_eq!(parse_bytes("100kb").expect("parse"), 100 * 1024);
        assert_eq!(parse_bytes("12").expect("parse"), 12);
    }

    #[test]
    fn parse_program_file_works() {
        let cmd = test_cmd_with_arg("30");
        let content = format!(
            "[program:alpha]\ncommand={}\ndirectory={}\n",
            cmd,
            std::env::temp_dir().display()
        );
        let path = test_temp_dir("parse-test.conf");
        fs::write(&path, &content).expect("write");
        let (services, warnings) = parse_program_file(path.as_path());
        fs::remove_file(&path).expect("remove");
        assert!(warnings.is_empty());
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "alpha");
    }

    #[test]
    fn user_root_implies_elevated_privilege() {
        let cmd = test_cmd_with_arg("30");
        let td = std::env::temp_dir().display().to_string();
        let content = format!(
            "[program:svc]\ncommand={}\ndirectory={}\nuser=root\n", cmd, td
        );
        let path = test_temp_dir("parse-root-elevated.conf");
        fs::write(&path, &content).expect("write");
        let (services, warnings) = parse_program_file(path.as_path());
        fs::remove_file(&path).expect("remove");
        assert!(warnings.is_empty());
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].privilege, super::PrivilegeMode::Elevated);
    }

    #[test]
    fn user_administrator_implies_elevated_privilege() {
        let cmd = test_cmd_with_arg("30");
        let td = std::env::temp_dir().display().to_string();
        let content = format!(
            "[program:svc]\ncommand={}\ndirectory={}\nuser=Administrator\n", cmd, td
        );
        let path = test_temp_dir("parse-admin-elevated.conf");
        fs::write(&path, &content).expect("write");
        let (services, warnings) = parse_program_file(path.as_path());
        fs::remove_file(&path).expect("remove");
        assert!(warnings.is_empty());
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].privilege, super::PrivilegeMode::Elevated);
    }

    #[test]
    fn privilege_elevated_with_normal_user_must_fail() {
        let cmd = test_cmd_with_arg("30");
        let td = std::env::temp_dir().display().to_string();
        let content = format!(
            "[program:svc]\ncommand={}\ndirectory={}\nuser=abc\nprivilege=elevated\n", cmd, td
        );
        let path = test_temp_dir("parse-invalid-elevated-user.conf");
        fs::write(&path, &content).expect("write");
        let (services, warnings) = parse_program_file(path.as_path());
        fs::remove_file(&path).expect("remove");
        assert!(services.is_empty());
        assert!(warnings.iter().any(|w| w.contains("invalid user")));
    }

    #[test]
    fn invalid_service_name_must_fail() {
        let cmd = test_cmd_with_arg("30");
        let td = std::env::temp_dir().display().to_string();
        let content = format!(
            "[program:../../evil]\ncommand={}\ndirectory={}\n", cmd, td
        );
        let path = test_temp_dir("parse-invalid-service-name.conf");
        fs::write(&path, &content).expect("write");
        let (services, warnings) = parse_program_file(path.as_path());
        fs::remove_file(&path).expect("remove");
        assert!(services.is_empty());
        assert!(warnings.iter().any(|w| w.contains("invalid characters")));
    }

    #[test]
    fn unsupported_program_key_must_fail() {
        let cmd = test_cmd_with_arg("30");
        let td = std::env::temp_dir().display().to_string();
        let content = format!(
            "[program:svc]\ncommand={}\ndirectory={}\nenvironment=FOO=BAR\n", cmd, td
        );
        let path = test_temp_dir("parse-unsupported-key.conf");
        fs::write(&path, &content).expect("write");
        let (services, warnings) = parse_program_file(path.as_path());
        fs::remove_file(&path).expect("remove");
        assert!(services.is_empty());
        assert!(warnings.iter().any(|w| w.contains("unsupported keys")));
    }

    #[cfg(unix)]
    #[test]
    fn elevated_service_requires_root_owned_command() {
        use crate::platform_fs;
        if platform_fs::default_policy_adapter().running_as_root() {
            return;
        }

        let base = test_temp_dir("elevated-owner");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        fs::write(
            base.join("fake-bin"),
            "#!/bin/sh\necho test\n",
        )
        .expect("write fake bin");
        fs::write(
            base.join("programs.d/app.conf"),
            format!(
                "[program:app]\ncommand={}\ndirectory=/tmp\nprivilege=elevated\n",
                base.join("fake-bin").display()
            ),
        )
        .expect("write program");
        fs::write(
            base.join("rustpmd.conf"),
            format!("config_dir = {}\n", base.join("programs.d").display()),
        )
        .expect("write daemon");

        let err = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None)
            .expect_err("should fail");
        assert!(err.to_string().contains("must be root-owned"));

        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn elevated_service_allows_non_root_owned_working_dir() {
        use crate::platform_fs;
        if platform_fs::default_policy_adapter().running_as_root() {
            return;
        }

        let base = test_temp_dir("elevated-working-dir");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        fs::write(
            base.join("programs.d/app.conf"),
            "[program:app]\ncommand=/bin/sleep 5\ndirectory=/tmp\nprivilege=elevated\n",
        )
        .expect("write program");
        fs::write(
            base.join("rustpmd.conf"),
            format!("config_dir = {}\n", base.join("programs.d").display()),
        )
        .expect("write daemon");

        let runtime = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None);
        assert!(runtime.is_ok());

        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn normal_service_rejects_root_owned_working_dir() {
        let base = test_temp_dir("normal-root-working-dir");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        fs::write(
            base.join("programs.d/app.conf"),
            "[program:app]\ncommand=/bin/sleep 5\ndirectory=/\nprivilege=normal\n",
        )
        .expect("write program");
        fs::write(
            base.join("rustpmd.conf"),
            format!("config_dir = {}\n", base.join("programs.d").display()),
        )
        .expect("write daemon");

        let err = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None)
            .expect_err("should fail");
        assert!(err
            .to_string()
            .contains("must not be root-owned when privilege=normal"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn web_expose_must_be_explicit_for_non_localhost() {
        let base = test_temp_dir("web-expose");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        let wd = base.join("workdir");
        fs::create_dir_all(&wd).expect("mkdir workdir");
        let cmd = test_cmd_with_arg("5");
        fs::write(
            base.join("programs.d/app.conf"),
            format!(
                "[program:app]\ncommand={}\ndirectory={}\n",
                cmd,
                wd.display()
            ),
        )
        .expect("write program");
        fs::write(
            base.join("rustpmd.conf"),
            format!(
                "config_dir = {}\n[web]\nenabled = true\nbind = 0.0.0.0\nport = 18765\npassword_hash = {}\n",
                base.join("programs.d").display(),
                default_password_hash()
            ),
        )
        .expect("write daemon");

        let err = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None)
            .expect_err("should fail");
        assert!(err.to_string().contains("web.expose=true"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn dependency_bounds_are_validated() {
        let base = test_temp_dir("runtime-test-dep");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        let wd = base.join("workdir");
        fs::create_dir_all(&wd).expect("mkdir workdir");
        let cmd = test_cmd_with_arg("5");
        fs::write(
            base.join("programs.d/app.conf"),
            format!(
                "[program:app]\ncommand={}\ndirectory={}\n",
                cmd,
                wd.display()
            ),
        )
        .expect("write program");
        fs::write(
            base.join("rustpmd.conf"),
            format!(
                "config_dir = {}\n[dependency]\nstartup_timeout_secs = 0\n",
                base.join("programs.d").display(),
            ),
        )
        .expect("write daemon");

        let err = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None)
            .expect_err("should fail");
        assert!(
            err.to_string()
                .contains("dependency.startup_timeout_secs must be between 1 and 600")
        );

        fs::write(
            base.join("rustpmd.conf"),
            format!(
                "config_dir = {}\n[dependency]\nservice_start_delay_secs = 301\n",
                base.join("programs.d").display(),
            ),
        )
        .expect("write daemon delay");
        let err = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None)
            .expect_err("should fail");
        assert!(
            err.to_string()
                .contains("dependency.service_start_delay_secs must be between 0 and 300")
        );

        fs::write(
            base.join("rustpmd.conf"),
            format!(
                "config_dir = {}\n[dependency]\nservice_start_spin_interval_secs = 31\n",
                base.join("programs.d").display(),
            ),
        )
        .expect("write daemon spin");
        let err = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None)
            .expect_err("should fail");
        assert!(
            err.to_string()
                .contains("dependency.service_start_spin_interval_secs must be between 0 and 30")
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn empty_program_config_dir_is_allowed() {
        let base = test_temp_dir("empty-programs");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        fs::write(
            base.join("programs.d/example.conf"),
            "# template only, no active program sections\n",
        )
        .expect("write example");
        fs::write(
            base.join("rustpmd.conf"),
            format!("config_dir = {}\n", base.join("programs.d").display()),
        )
        .expect("write daemon");

        let runtime = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None)
            .expect("should parse");
        assert_eq!(runtime.services.len(), 0);
        assert_eq!(runtime.web.password_hash, default_password_hash());
        assert!(runtime.web.enabled);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn config_envelope_uses_v1_schema_for_check_mode() {
        let base = test_temp_dir("schema-check");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        let wd = base.join("workdir");
        fs::create_dir_all(&wd).expect("mkdir workdir");
        let cmd = test_cmd_with_arg("5");
        fs::write(
            base.join("programs.d/app.conf"),
            format!(
                "[program:app]\ncommand={}\ndirectory={}\n",
                cmd,
                wd.display()
            ),
        )
        .expect("write program");
        fs::write(
            base.join("rustpmd.conf"),
            format!(
                "config_dir = {}\n",
                base.join("programs.d").display()
            ),
        )
        .expect("write daemon");
        let envelope =
            load_runtime_config_envelope(base.join("rustpmd.conf").as_path(), None, None).expect("load");
        assert_eq!(envelope.schema_version, ConfigSchemaVersion::V1);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn config_envelope_uses_v1_schema_for_run_mode() {
        let base = test_temp_dir("schema-run");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        let wd = base.join("workdir");
        fs::create_dir_all(&wd).expect("mkdir workdir");
        let cmd = test_cmd_with_arg("5");
        fs::write(
            base.join("programs.d/app.conf"),
            format!(
                "[program:app]\ncommand={}\ndirectory={}\n",
                cmd,
                wd.display()
            ),
        )
        .expect("write program");
        fs::write(
            base.join("rustpmd.conf"),
            format!(
                "config_dir = {}\n",
                base.join("programs.d").display()
            ),
        )
        .expect("write daemon");
        let envelope = load_runtime_config_envelope_for_run(
            base.join("rustpmd.conf").as_path(),
            None,
            None,
        )
        .expect("load");
        assert_eq!(envelope.schema_version, ConfigSchemaVersion::V1);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn web_password_hash_is_required_format() {
        let explicit = default_password_hash();
        let from_hash = resolve_password_hash(Some(&explicit)).expect("hash");
        assert_eq!(from_hash, explicit);
        assert!(resolve_password_hash(Some("password")).is_err());
    }

    #[test]
    fn plain_web_password_is_rejected() {
        let base = test_temp_dir("plain-password");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("programs.d")).expect("mkdir");
        fs::write(
            base.join("programs.d/example.conf"),
            "# template only, no active program sections\n",
        )
        .expect("write example");
        fs::write(
            base.join("rustpmd.conf"),
            format!(
                "config_dir = {}\n[web]\npassword = password123\n",
                base.join("programs.d").display(),
            ),
        )
        .expect("write daemon");

        let err = load_runtime_config(base.join("rustpmd.conf").as_path(), None, None)
            .expect_err("should reject plain password");
        assert!(err.to_string().contains("plain web password is not supported"));

        let _ = fs::remove_dir_all(&base);
    }
}
