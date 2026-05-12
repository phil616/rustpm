use std::path::PathBuf;

use anyhow::Result;

#[cfg(windows)]
pub mod windows;

pub trait PlatformServiceControl: Send + Sync {
    fn install_service(&self, config_path: &PathBuf) -> Result<()>;
    fn uninstall_service(&self) -> Result<()>;
    fn start_service(&self) -> Result<()>;
    fn stop_service(&self) -> Result<()>;
    fn service_exists(&self) -> bool;
}

pub trait PlatformIpcChannel: Send + Sync {
    fn create_server(&self, path: &PathBuf) -> Result<()>;
    fn connect_client(&self, path: &PathBuf) -> Result<()>;
    fn get_platform_name(&self) -> &'static str;
}

pub trait PlatformProcessManager: Send + Sync {
    fn create_process_group(&self) -> Result<u64>;
    fn assign_to_group(&self, pid: u32, group_id: u64) -> Result<()>;
    fn terminate_group(&self, group_id: u64, exit_code: u32) -> Result<()>;
    fn spawn_as_user(&self, cmd: &str, args: &[String], user: &str, cwd: Option<&PathBuf>) -> Result<u32>;
}

pub fn get_platform_service_control() -> &'static dyn PlatformServiceControl {
    #[cfg(windows)]
    {
        use windows::service_adapters::WindowsServiceControl;
        static CONTROL: WindowsServiceControl = WindowsServiceControl;
        &CONTROL
    }
    #[cfg(unix)]
    {
        &unix::UnixServiceControl
    }
}

pub fn get_platform_ipc_channel() -> &'static dyn PlatformIpcChannel {
    #[cfg(windows)]
    {
        static CHANNEL: windows::ipc::WindowsNamedPipeChannel = windows::ipc::WindowsNamedPipeChannel;
        &CHANNEL
    }
    #[cfg(unix)]
    {
        &unix::UnixSocketChannel
    }
}

pub fn get_platform_process_manager() -> &'static dyn PlatformProcessManager {
    #[cfg(windows)]
    {
        static MANAGER: windows::process::WindowsProcessManager = windows::process::WindowsProcessManager;
        &MANAGER
    }
    #[cfg(unix)]
    {
        &unix::UnixProcessManager
    }
}

#[cfg(unix)]
mod unix {
    use super::*;

    pub struct UnixServiceControl;

    impl PlatformServiceControl for UnixServiceControl {
        fn install_service(&self, _config_path: &PathBuf) -> Result<()> {
            Err(anyhow::anyhow!("Unix platform uses launchd, not service installation"))
        }

        fn uninstall_service(&self) -> Result<()> {
            Err(anyhow::anyhow!("Unix platform uses launchd, not service installation"))
        }

        fn start_service(&self) -> Result<()> {
            Err(anyhow::anyhow!("Unix platform uses launchd"))
        }

        fn stop_service(&self) -> Result<()> {
            Err(anyhow::anyhow!("Unix platform uses launchd"))
        }

        fn service_exists(&self) -> bool {
            false
        }
    }

    pub struct UnixSocketChannel;

    impl PlatformIpcChannel for UnixSocketChannel {
        fn create_server(&self, _path: &PathBuf) -> Result<()> {
            Ok(())
        }

        fn connect_client(&self, _path: &PathBuf) -> Result<()> {
            Ok(())
        }

        fn get_platform_name(&self) -> &'static str {
            "unix_socket"
        }
    }

    pub struct UnixProcessManager;

    impl PlatformProcessManager for UnixProcessManager {
        fn create_process_group(&self) -> Result<u64> {
            Ok(0)
        }

        fn assign_to_group(&self, _pid: u32, _group_id: u64) -> Result<()> {
            Ok(())
        }

        fn terminate_group(&self, _group_id: u64, _exit_code: u32) -> Result<()> {
            Ok(())
        }

        fn spawn_as_user(&self, _cmd: &str, _args: &[String], _user: &str, _cwd: Option<&PathBuf>) -> Result<u32> {
            Err(anyhow::anyhow!("Unix user spawning not implemented via platform abstraction"))
        }
    }
}
