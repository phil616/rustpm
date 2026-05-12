pub mod service;
pub mod ipc;
pub mod process;

pub mod service_adapters {
    use std::path::PathBuf;
    use anyhow::Result;
    use crate::platform::PlatformServiceControl;

    pub struct WindowsServiceControl;

    impl PlatformServiceControl for WindowsServiceControl {
        fn install_service(&self, config_path: &PathBuf) -> Result<()> {
            super::service::install_service(config_path)
        }

        fn uninstall_service(&self) -> Result<()> {
            super::service::uninstall_service()
        }

        fn start_service(&self) -> Result<()> {
            super::service::start_service()
        }

        fn stop_service(&self) -> Result<()> {
            super::service::stop_service()
        }

        fn service_exists(&self) -> bool {
            super::service::service_exists()
        }
    }
}

pub use service_adapters::WindowsServiceControl;

pub fn get_current_exe_path() -> std::result::Result<std::path::PathBuf, anyhow::Error> {
    std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("failed to get current exe path: {}", e))
}

pub fn get_service_name() -> &'static str {
    "rustpm"
}

pub fn get_service_display_name() -> &'static str {
    "rustpm"
}