use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rustpm::{
    config::{
        default_daemon_config_path, load_runtime_config_envelope, load_runtime_config_envelope_for_run,
    },
    rustpmd::{self, DaemonRole},
};
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "rustpm", about = "A minimal rustpm process manager for macOS services")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start rustpmd and watch configured services.
    Run {
        /// Daemon config path (default: /usr/local/etc/rustpm/rustpmd.conf).
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Program config directory (default: /usr/local/etc/rustpm/programs.d).
        #[arg(long)]
        config_dir: Option<PathBuf>,
        /// Unix socket path for rustpmctl control channel.
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Validate config and exit without starting daemon.
        #[arg(long, default_value_t = false)]
        validate_config: bool,
    },
    /// Validate config format and values.
    CheckConfig {
        /// Daemon config path (default: /usr/local/etc/rustpm/rustpmd.conf).
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Program config directory (default: /usr/local/etc/rustpm/programs.d).
        #[arg(long)]
        config_dir: Option<PathBuf>,
        /// Unix socket path (default: /usr/local/var/run/rustpm/rustpmd.sock).
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("rustpm=info")
        .compact()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            config,
            config_dir,
            socket,
            validate_config,
        } => {
            let cfg_path = config.unwrap_or_else(default_daemon_config_path);
            let envelope = load_runtime_config_envelope_for_run(&cfg_path, config_dir, socket)?;
            let runtime = envelope.runtime;
            for warning in &runtime.program_warnings {
                tracing::warn!(warning = %warning, "program config skipped in run mode");
            }
            if validate_config {
                info!(
                    service_count = runtime.services.len(),
                    config_schema = envelope.schema_version.as_str(),
                    config = %cfg_path.display(),
                    config_dir = %runtime.config_dir.display(),
                    socket = %runtime.socket_path.display(),
                    privd_socket = %runtime.privd_socket_path.display(),
                    "config is valid (validate-only mode)"
                );
                return Ok(());
            }
            rustpmd::run(
                runtime.services,
                &runtime.socket_path,
                &runtime.config_dir,
                &runtime.web,
                &runtime.privd_socket_path,
                Some(&runtime.dependency),
                &cfg_path,
                envelope.schema_version,
                DaemonRole::Primary,
            )
            .await
        }
        Commands::CheckConfig {
            config,
            config_dir,
            socket,
        } => {
            let cfg_path = config.unwrap_or_else(default_daemon_config_path);
            let envelope = load_runtime_config_envelope(&cfg_path, config_dir, socket)?;
            let runtime = envelope.runtime;
            info!(
                service_count = runtime.services.len(),
                config_schema = envelope.schema_version.as_str(),
                config = %cfg_path.display(),
                config_dir = %runtime.config_dir.display(),
                socket = %runtime.socket_path.display(),
                privd_socket = %runtime.privd_socket_path.display(),
                web_enabled = runtime.web.enabled,
                web_bind = %runtime.web.bind_ip,
                web_port = runtime.web.port,
                require_privd = runtime.dependency.require_privd,
                dep_startup_timeout = runtime.dependency.startup_timeout_secs,
                dep_healthcheck_interval = runtime.dependency.healthcheck_interval_secs,
                dep_failure_threshold = runtime.dependency.failure_threshold,
                service_start_delay_secs = runtime.dependency.service_start_delay_secs,
                service_start_spin_interval_secs = runtime.dependency.service_start_spin_interval_secs,
                "config is valid"
            );
            Ok(())
        }
    }
}
