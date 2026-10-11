use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::platform::PlatformProcessManager;

pub struct WindowsProcessManager;

fn groups() -> &'static std::sync::Mutex<std::collections::HashMap<u64, super::managed_process::OwnedHandle>> {
    static GROUPS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u64, super::managed_process::OwnedHandle>>> = std::sync::OnceLock::new();
    GROUPS.get_or_init(Default::default)
}

impl PlatformProcessManager for WindowsProcessManager {
    fn create_process_group(&self) -> Result<u64> {
        let job = super::managed_process::create_job()?;
        let id = job.0 as u64;
        groups().lock().map_err(|_| anyhow!("job registry poisoned"))?.insert(id, job);
        Ok(id)
    }

    fn assign_to_group(&self, pid: u32, group_id: u64) -> Result<()> {
        use windows::Win32::System::{Threading::*, JobObjects::AssignProcessToJobObject};
        let guard = groups().lock().map_err(|_| anyhow!("job registry poisoned"))?;
        let job = guard.get(&group_id).ok_or_else(|| anyhow!("unknown process group"))?;
        unsafe {
            let process = super::managed_process::OwnedHandle(OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)?.0 as usize);
            AssignProcessToJobObject(job.raw(), process.raw())?;
        }
        Ok(())
    }

    fn terminate_group(&self, group_id: u64, exit_code: u32) -> Result<()> {
        let job = groups().lock().map_err(|_| anyhow!("job registry poisoned"))?.remove(&group_id)
            .ok_or_else(|| anyhow!("unknown process group"))?;
        unsafe { windows::Win32::System::JobObjects::TerminateJobObject(job.raw(), exit_code)?; }
        Ok(())
    }

    fn spawn_as_user(&self, cmd: &str, args: &[String], _user: &str, cwd: Option<&PathBuf>) -> Result<u32> {
        let _ = (cmd, args, cwd);
        Err(anyhow!("PID-only spawn cannot safely own a process tree or switch credentials; use spawn_managed"))
    }
}

pub fn get_current_process_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentProcessId() }
}

pub fn get_current_thread_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
}
