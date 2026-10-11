use std::path::PathBuf;

use anyhow::Result;

use crate::platform::PlatformIpcChannel;

pub struct WindowsNamedPipeChannel;

impl PlatformIpcChannel for WindowsNamedPipeChannel {
    fn create_server(&self, path: &PathBuf) -> Result<()> {
        tracing::info!(path = %path.display(), "Windows named pipe channel ready (IPC stub)");
        Err(anyhow::anyhow!("synchronous IPC facade is unsupported; use the async control server"))
    }

    fn connect_client(&self, path: &PathBuf) -> Result<()> {
        tracing::info!(path = %path.display(), "Windows named pipe channel ready (IPC stub)");
        Err(anyhow::anyhow!("synchronous IPC facade is unsupported; use ipc::send_request"))
    }

    fn get_platform_name(&self) -> &'static str {
        "windows_named_pipe"
    }
}

#[cfg(windows)]
pub fn create_named_pipe_server(pipe_name: &str) -> Result<()> {
    let _ = pipe_name;
    tracing::debug!(pipe = pipe_name, "named pipe server stub (not fully implemented)");
    Err(anyhow::anyhow!("use the async control server"))
}

#[cfg(windows)]
pub fn connect_to_named_pipe(pipe_name: &str) -> Result<()> {
    let _ = pipe_name;
    tracing::debug!(pipe = pipe_name, "named pipe client stub (not fully implemented)");
    Err(anyhow::anyhow!("use ipc::send_request"))
}

#[cfg(not(windows))]
pub fn create_named_pipe_server(_pipe_name: &str) -> Result<()> {
    Err(anyhow!("named pipe is Windows-only"))
}

#[cfg(not(windows))]
pub fn connect_to_named_pipe(_pipe_name: &str) -> Result<()> {
    Err(anyhow!("named pipe is Windows-only"))
}
