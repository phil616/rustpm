use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "rustpm-privd", about = "Privileged helper daemon for rustpm")]
struct Cli {
    /// Daemon config path (default: /usr/local/etc/rustpm/rustpmd.conf).
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Program config directory (default: /usr/local/etc/rustpm/programs.d).
    #[arg(long)]
    config_dir: Option<PathBuf>,
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
    use rustpm::config::{default_daemon_config_path, load_runtime_config_envelope_for_run, WebConfig};
    use rustpm::rustpmd::{self, DaemonRole};

    tracing_subscriber::fmt()
        .with_env_filter("rustpm=info")
        .compact()
        .init();

    let cli = Cli::parse();
    let cfg_path = cli.config.unwrap_or_else(default_daemon_config_path);
    let envelope = load_runtime_config_envelope_for_run(&cfg_path, cli.config_dir, None)?;
    let runtime = envelope.runtime;
    for warning in &runtime.program_warnings {
        tracing::warn!(warning = %warning, "program config skipped in run mode");
    }

    info!(
        service_count = runtime.services.len(),
        socket = %runtime.privd_socket_path.display(),
        "rustpm-privd starting"
    );

    let mut web_disabled = WebConfig::default();
    web_disabled.enabled = false;

    rustpmd::run(
        runtime.services,
        &runtime.privd_socket_path,
        &runtime.config_dir,
        &web_disabled,
        &runtime.privd_socket_path,
        None,
        &cfg_path,
        envelope.schema_version,
        DaemonRole::PrivilegedHelper,
    )
    .await
}

#[cfg(windows)]
fn main() -> Result<()> {
    use rustpm::config::{default_daemon_config_path, load_runtime_config_envelope_for_run, WebConfig};
    use rustpm::platform::windows::service;
    use rustpm::rustpmd::{self, DaemonRole};

    tracing_subscriber::fmt()
        .with_env_filter("rustpm=info")
        .compact()
        .init();

    let cli = Cli::parse();
    let cfg_path = cli.config.unwrap_or_else(default_daemon_config_path);
    let envelope = load_runtime_config_envelope_for_run(&cfg_path, cli.config_dir, None)?;
    let runtime = envelope.runtime;
    for warning in &runtime.program_warnings {
        tracing::warn!(warning = %warning, "program config skipped in run mode");
    }

    let services = runtime.services;
    let privd_socket = runtime.privd_socket_path;
    let cfg_dir = runtime.config_dir;
    let schema_ver = envelope.schema_version;

    service::run_as_service("rustpm-privd", move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let mut web_disabled = WebConfig::default();
            web_disabled.enabled = false;
            rustpmd::run(
                services,
                &privd_socket,
                &cfg_dir,
                &web_disabled,
                &privd_socket,
                None,
                &cfg_path,
                schema_ver,
                DaemonRole::PrivilegedHelper,
            )
            .await
        })
    })?;

    Ok(())
}
