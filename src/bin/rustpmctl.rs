use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use rustpm::config::{default_daemon_config_path, load_runtime_config};
use rustpm::ipc::{ControlRequest, ControlResponse, ServiceView};
use rustpm::web_api::response_code_docs;
use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

#[derive(Parser, Debug)]
#[command(name = "rustpmctl", about = "Control rustpmd managed services")]
struct Cli {
    /// Unix socket path for rustpmd control channel.
    #[arg(long, default_value = "/usr/local/var/run/rustpm/rustpmd.sock")]
    socket: PathBuf,
    /// Print machine-readable JSON output.
    #[arg(long, global = true, default_value_t = false)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Ping,
    Contract,
    Rescan,
    Reload {
        service: String,
        /// Validate config before sending reload.
        #[arg(long, default_value_t = false)]
        check_config: bool,
        /// Daemon config path used when --check-config is enabled.
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Program config directory override used when --check-config is enabled.
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },
    /// Validate rustpm config files without changing runtime state.
    CheckConfig {
        /// Daemon config path (default: /usr/local/etc/rustpm/rustpmd.conf).
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Program config directory override.
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },
    /// Print response-code dictionary for automation and troubleshooting.
    Codes,
    List,
    Status { service: String },
    Start { service: String },
    Stop { service: String },
    Restart {
        service: String,
        /// Validate config before sending restart command.
        #[arg(long, default_value_t = false)]
        check_config: bool,
        /// Daemon config path used when --check-config is enabled.
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Program config directory override used when --check-config is enabled.
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let req = match cli.command {
        Commands::CheckConfig { config, config_dir } => {
            validate_config_files(config, config_dir, cli.json)?;
            return Ok(());
        }
        Commands::Codes => {
            print_response_codes(cli.json)?;
            return Ok(());
        }
        Commands::Ping => ControlRequest::Ping,
        Commands::Contract => ControlRequest::RuntimeContract,
        Commands::Rescan => ControlRequest::Rescan,
        Commands::Reload {
            service,
            check_config,
            config,
            config_dir,
        } => {
            if check_config {
                validate_config_files(config, config_dir, cli.json)?;
            }
            ControlRequest::Reload { service }
        }
        Commands::List => ControlRequest::List,
        Commands::Status { service } => ControlRequest::Status { service },
        Commands::Start { service } => ControlRequest::Start { service },
        Commands::Stop { service } => ControlRequest::Stop { service },
        Commands::Restart {
            service,
            check_config,
            config,
            config_dir,
        } => {
            if check_config {
                validate_config_files(config, config_dir, cli.json)?;
            }
            ControlRequest::Restart { service }
        }
    };

    let resp = send_request(&cli.socket, req).await?;
    print_response(&resp, cli.json)?;

    if resp.ok {
        Ok(())
    } else {
        Err(anyhow!(resp.message))
    }
}

async fn send_request(socket: &PathBuf, req: ControlRequest) -> Result<ControlResponse> {
    let stream = UnixStream::connect(socket)
        .await
        .with_context(|| format!("failed to connect socket {}", socket.display()))?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut payload = serde_json::to_string(&req).context("failed to encode request")?;
    payload.push('\n');
    writer
        .write_all(payload.as_bytes())
        .await
        .context("failed to send request")?;

    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .context("failed to read response")?;
    if line.trim().is_empty() {
        return Err(anyhow!("empty response from rustpmd"));
    }

    let resp = serde_json::from_str::<ControlResponse>(line.trim()).context("failed to decode response")?;
    Ok(resp)
}

fn print_response(resp: &ControlResponse, as_json: bool) -> Result<()> {
    if as_json {
        let payload =
            serde_json::to_string_pretty(resp).context("failed to serialize JSON response")?;
        println!("{payload}");
        return Ok(());
    }
    if let Some(services) = &resp.services {
        for svc in services {
            print_service_line(svc);
        }
    } else if let Some(contract) = &resp.runtime_contract {
        println!(
            "config_schema={}\tapi_schema={}\tpolicy_provider={}\tweb_session_ttl_secs={}\tweb_max_sessions={}\tweb_program_config_write={}\tweb_service_control={}\tweb_rescan={}",
            contract.config_schema,
            contract.api_schema,
            contract.policy_provider,
            contract.web_session_ttl_secs,
            contract.web_max_sessions,
            contract.web_program_config_write,
            contract.web_service_control,
            contract.web_rescan
        );
    } else if let Some(service) = &resp.service {
        print_service_line(service);
    } else {
        println!("{}: {}", resp.code.as_str(), resp.message);
    }
    Ok(())
}

fn print_service_line(svc: &ServiceView) {
    let source = svc
        .source_file
        .as_deref()
        .map(|v| format!("\tsource={}", v))
        .unwrap_or_default();
    println!(
        "{}\tstate={:?}\tpid={}\trestarts={}\tmsg={}{}",
        svc.name,
        svc.state,
        svc.pid.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
        svc.restarts,
        svc.message,
        source
    );
}

#[derive(Debug, Serialize)]
struct CheckConfigOutput {
    ok: bool,
    config: String,
    config_dir: String,
    services: usize,
    warnings: usize,
}

fn validate_config_files(config: Option<PathBuf>, config_dir: Option<PathBuf>, as_json: bool) -> Result<()> {
    let cfg_path = config.unwrap_or_else(default_daemon_config_path);
    let runtime = load_runtime_config(&cfg_path, config_dir, None)?;
    if as_json {
        let output = CheckConfigOutput {
            ok: true,
            config: cfg_path.display().to_string(),
            config_dir: runtime.config_dir.display().to_string(),
            services: runtime.services.len(),
            warnings: runtime.program_warnings.len(),
        };
        let payload =
            serde_json::to_string_pretty(&output).context("failed to serialize JSON output")?;
        println!("{payload}");
    } else {
        println!(
            "config is valid: config={}, config_dir={}, services={}",
            cfg_path.display(),
            runtime.config_dir.display(),
            runtime.services.len()
        );
    }
    Ok(())
}

fn print_response_codes(as_json: bool) -> Result<()> {
    let codes = response_code_docs();
    if as_json {
        let payload =
            serde_json::to_string_pretty(&codes).context("failed to serialize JSON output")?;
        println!("{payload}");
        return Ok(());
    }
    for item in codes {
        println!(
            "{}\thttp={}\tretryable={}\tdesc={}",
            item.code.as_str(),
            item.http_status,
            item.retryable,
            item.description
        );
    }
    Ok(())
}
