use super::*;

#[cfg(unix)]
pub(super) async fn terminate_process_group_impl(
    service_name: &str,
    running: &mut RunningChild,
    stop_grace_secs: u64,
) -> Result<()> {
    use nix::sys::signal::Signal;
    use nix::unistd::Pid;

    send_group_signal_impl(service_name, running.pgid, Signal::SIGTERM)?;
    let grace = Duration::from_secs(stop_grace_secs);

    match timeout(grace, running.child.wait()).await {
        Ok(wait_result) => {
            let _ = wait_result.with_context(|| format!("wait failed during stop for {}", service_name))?;
            info!(service = service_name, "service terminated gracefully");
        }
        Err(_) => {
            warn!(service = service_name, grace_secs = stop_grace_secs, "grace timeout, sending SIGKILL");
            send_group_signal_impl(service_name, running.pgid, Signal::SIGKILL)?;
            let _ = running
                .child
                .wait()
                .await
                .with_context(|| format!("wait failed after kill for {}", service_name))?;
        }
    }

    Ok(())
}

#[cfg(unix)]
pub(super) fn send_group_signal_impl(service_name: &str, pgid: i32, sig: nix::sys::signal::Signal) -> Result<()> {
    use nix::errno::Errno;
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    match killpg(Pid::from_raw(pgid), sig) {
        Ok(()) => Ok(()),
        Err(Errno::ESRCH) => {
            warn!(service = service_name, pgid, ?sig, "process group not found");
            Ok(())
        }
        Err(err) => Err(anyhow!("failed to send {:?} to {}: {}", sig, service_name, err)),
    }
}

#[cfg(windows)]
pub(super) async fn terminate_process_group_impl(
    service_name: &str,
    running: &mut RunningChild,
    stop_grace_secs: u64,
) -> Result<()> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::TerminateJobObject;

    let job = running.job_handle;
    if job != 0 {
        unsafe {
            TerminateJobObject(HANDLE(job as *mut std::ffi::c_void), 1).ok();
        }
    }

    let grace = Duration::from_secs(stop_grace_secs);
    match timeout(grace, running.child.wait()).await {
        Ok(wait_result) => {
            let _ = wait_result.with_context(|| format!("wait failed during stop for {}", service_name))?;
            info!(service = service_name, "service terminated");
        }
        Err(_) => {
            warn!(service = service_name, grace_secs = stop_grace_secs, "grace timeout, force killing");
            if let Err(e) = running.child.kill().await {
                warn!(service = service_name, error = %e, "force kill failed");
            }
            let _ = running
                .child
                .wait()
                .await
                .with_context(|| format!("wait failed after kill for {}", service_name))?;
        }
    }

    Ok(())
}

#[cfg(windows)]
#[allow(dead_code)]
pub(super) fn send_group_signal_impl(_service_name: &str, _pgid: i32, _sig: ()) -> Result<()> {
    Ok(())
}