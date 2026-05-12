use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::platform::PlatformProcessManager;

pub struct WindowsProcessManager;

impl PlatformProcessManager for WindowsProcessManager {
    fn create_process_group(&self) -> Result<u64> {
        Err(anyhow!("Job Objects not yet implemented"))
    }

    fn assign_to_group(&self, _pid: u32, _group_id: u64) -> Result<()> {
        Err(anyhow!("Job Objects not yet implemented"))
    }

    fn terminate_group(&self, _group_id: u64, _exit_code: u32) -> Result<()> {
        Err(anyhow!("Job Objects not yet implemented"))
    }

    fn spawn_as_user(&self, cmd: &str, args: &[String], _user: &str, cwd: Option<&PathBuf>) -> Result<u32> {
        let mut cmd_line = cmd.to_string();
        for arg in args {
            cmd_line.push(' ');
            cmd_line.push_str(arg);
        }

        let mut startup_info: windows::Win32::System::Threading::STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup_info.cb = std::mem::size_of::<windows::Win32::System::Threading::STARTUPINFOW>() as u32;
        let mut process_info: windows::Win32::System::Threading::PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

        let cmd_wide: Vec<u16> = cmd_line.encode_utf16().chain(std::iter::once(0)).collect();

        let cwd_wide: Option<Vec<u16>> = cwd.map(|p| p.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect());
        let cwd_ptr = cwd_wide
            .as_ref()
            .map(|v| windows::core::PCWSTR(v.as_ptr()))
            .unwrap_or(windows::core::PCWSTR(std::ptr::null()));

        unsafe {
            let result = windows::Win32::System::Threading::CreateProcessW(
                None,
                windows::core::PWSTR(cmd_wide.as_ptr() as *mut u16),
                None,
                None,
                false,
                windows::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP,
                None,
                cwd_ptr,
                &startup_info,
                &mut process_info,
            );

            match result {
                Ok(()) => {
                    let pid = process_info.dwProcessId;
                    let _ = windows::Win32::Foundation::CloseHandle(process_info.hThread);
                    let _ = windows::Win32::Foundation::CloseHandle(process_info.hProcess);
                    Ok(pid)
                }
                Err(e) => Err(anyhow!("failed to create process: {}", e)),
            }
        }
    }
}

pub fn get_current_process_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentProcessId() }
}

pub fn get_current_thread_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
}