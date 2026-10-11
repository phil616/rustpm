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
    // Detached Windows programs cannot receive console control events.
    // This backend explicitly uses force-stop mode; do not pretend a grace
    // interval after TerminateJobObject is a graceful shutdown.
    let _ = stop_grace_secs;
    running.child.terminate_tree().with_context(|| format!("failed to terminate job for {}", service_name))?;
    running.child.wait().await.with_context(|| format!("wait failed after terminating {}", service_name))?;
    info!(service = service_name, "Windows process tree force-stopped");
    Ok(())
}

#[cfg(windows)]
#[allow(dead_code)]
pub(super) fn send_group_signal_impl(_service_name: &str, _pgid: i32, _sig: ()) -> Result<()> {
    Ok(())
}