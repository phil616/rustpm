use super::*;

pub(super) async fn manage_service_actor_impl(
    svc: ServiceConfig,
    mut rx: mpsc::Receiver<ServiceCommand>,
    state: SharedState,
    service_meta: ServiceMetaMap,
    startup_policy: ServiceStartupPolicy,
) -> Result<()> {
    let name = svc.name.clone();
    let mut desired_running = svc.autostart;
    let mut running: Option<RunningChild> = None;
    let mut restart_count = 0_u32;
    let mut backoff_until: Option<Instant> = None;
    let mut first_start_pending = true;

    update_state(
        &state,
        &name,
        RuntimeRecord {
            state: ServiceState::Stopped,
            pid: None,
            restarts: restart_count,
            message: if desired_running {
                "autostart enabled".to_string()
            } else {
                "autostart disabled".to_string()
            },
        },
    )
    .await;

    'actor: loop {
        if !desired_running && running.is_none() {
            match rx.recv().await {
                Some(cmd) => {
                    if let ControlOutcome::Exit =
                        apply_control(cmd, &mut desired_running, &mut backoff_until).await?
                    {
                        update_state(
                            &state,
                            &name,
                            RuntimeRecord {
                                state: ServiceState::Stopped,
                                pid: None,
                                restarts: restart_count,
                                message: "daemon shutdown".to_string(),
                            },
                        )
                        .await;
                        break;
                    }
                }
                None => break,
            }
            continue;
        }

        if desired_running && running.is_none() {
            if first_start_pending {
                let now = Instant::now();
                if startup_policy.first_start_not_before > now {
                    tokio::select! {
                        _ = sleep(startup_policy.first_start_not_before.duration_since(now)) => {}
                        cmd = rx.recv() => {
                            match cmd {
                                Some(c) => {
                                    if let ControlOutcome::Exit = apply_control(c, &mut desired_running, &mut backoff_until).await? { break; }
                                }
                                None => break,
                            }
                            continue;
                        }
                    }
                }
                first_start_pending = false;
            }

            if let Some(until) = backoff_until {
                if Instant::now() < until {
                    update_state(
                        &state,
                        &name,
                        RuntimeRecord {
                            state: ServiceState::Backoff,
                            pid: None,
                            restarts: restart_count,
                            message: "waiting for restart backoff".to_string(),
                        },
                    )
                    .await;

                    tokio::select! {
                        _ = sleep(until.saturating_duration_since(Instant::now())) => {
                            backoff_until = None;
                        }
                        cmd = rx.recv() => {
                            match cmd {
                                Some(c) => {
                                    if let ControlOutcome::Exit =
                                        apply_control(c, &mut desired_running, &mut backoff_until).await?
                                    {
                                        update_state(
                                            &state,
                                            &name,
                                            RuntimeRecord {
                                                state: ServiceState::Stopped,
                                                pid: None,
                                                restarts: restart_count,
                                                message: "daemon shutdown".to_string(),
                                            },
                                        )
                                        .await;
                                        break;
                                    }
                                }
                                None => break,
                            }
                        }
                    }
                    continue;
                }
                backoff_until = None;
            }

            tokio::select! {
                _ = apply_start_spin_guard(&startup_policy) => {}
                cmd = rx.recv() => {
                    match cmd {
                        Some(c) => {
                            if let ControlOutcome::Exit = apply_control(c, &mut desired_running, &mut backoff_until).await? { break; }
                        }
                        None => break,
                    }
                    continue;
                }
            }

            update_state(
                &state,
                &name,
                RuntimeRecord {
                    state: ServiceState::Starting,
                    pid: None,
                    restarts: restart_count,
                    message: "starting service".to_string(),
                },
            )
            .await;

            match spawn_service(&svc).await {
                Ok(mut started) => {
                    update_runtime_log_bindings(&service_meta, &name, &started.log_bindings).await;
                    if svc.start_secs > 0 {
                        let window = Duration::from_secs(svc.start_secs);
                        let deadline = Instant::now() + window;
                        let result = loop {
                            tokio::select! {
                                result = timeout(deadline.saturating_duration_since(Instant::now()), started.child.child.wait()) => break result,
                                cmd = rx.recv() => {
                                    if matches!(cmd, Some(ServiceCommand::Start)) { continue; }
                                    terminate_process_group(&name, &mut started.child, svc.restart.stop_grace_secs).await?;
                                    let Some(cmd) = cmd else { break 'actor; };
                                    if let ControlOutcome::Exit = apply_control(cmd, &mut desired_running, &mut backoff_until).await? { break 'actor; }
                                    update_state(&state, &name, RuntimeRecord::new("startup interrupted by control request")).await;
                                    continue 'actor;
                                }
                            }
                        };
                        match result {
                            Ok(wait_result) => {
                                let status = wait_result.with_context(|| {
                                    format!("wait failed in start window for {}", name)
                                })?;
                                warn!(
                                    service = %name,
                                    code = ?status.code(),
                                    "service exited before startsecs window"
                                );

                                if !svc.restart.enabled {
                                    desired_running = false;
                                    update_state(
                                        &state,
                                        &name,
                                        RuntimeRecord {
                                            state: ServiceState::Failed,
                                            pid: None,
                                            restarts: restart_count,
                                            message: "service failed start and restart disabled".to_string(),
                                        },
                                    )
                                    .await;
                                    continue;
                                }

                                if svc.restart.max_retries > 0 && restart_count >= svc.restart.max_retries {
                                    desired_running = false;
                                    update_state(
                                        &state,
                                        &name,
                                        RuntimeRecord {
                                            state: ServiceState::Failed,
                                            pid: None,
                                            restarts: restart_count,
                                            message: "service failed start and retry limit reached".to_string(),
                                        },
                                    )
                                    .await;
                                    continue;
                                }

                                restart_count += 1;
                                backoff_until =
                                    Some(Instant::now() + Duration::from_secs(svc.restart.backoff_secs));
                                update_state(
                                    &state,
                                    &name,
                                    RuntimeRecord {
                                        state: ServiceState::Backoff,
                                        pid: None,
                                        restarts: restart_count,
                                        message: "service failed start, scheduling restart".to_string(),
                                    },
                                )
                                .await;
                                continue;
                            }
                            Err(_) => {}
                        }
                    }

                    update_state(
                        &state,
                        &name,
                        RuntimeRecord {
                            state: ServiceState::Running,
                            pid: Some(started.child.pid_u32),
                            restarts: restart_count,
                            message: "service running".to_string(),
                        },
                    )
                    .await;
                    running = Some(started.child);
                }
                Err(err) => {
                    let err_detail = format_error_chain(&err);
                    warn!(service = %name, error = %err_detail, "failed to start service");
                    if !svc.restart.enabled {
                        desired_running = false;
                        update_state(
                            &state,
                            &name,
                            RuntimeRecord {
                                state: ServiceState::Failed,
                                pid: None,
                                restarts: restart_count,
                                message: format!(
                                    "start failed and restart disabled: {}",
                                    truncate_message(&err_detail, 220)
                                ),
                            },
                        )
                        .await;
                        continue;
                    }

                    if svc.restart.max_retries > 0 && restart_count >= svc.restart.max_retries {
                        desired_running = false;
                        update_state(
                            &state,
                            &name,
                            RuntimeRecord {
                                state: ServiceState::Failed,
                                pid: None,
                                restarts: restart_count,
                                message: "start failed and retry limit reached".to_string(),
                            },
                        )
                        .await;
                        continue;
                    }

                    restart_count += 1;
                    update_state(
                        &state,
                        &name,
                        RuntimeRecord {
                            state: ServiceState::Backoff,
                            pid: None,
                            restarts: restart_count,
                            message: format!(
                                "start failed, retry in {}s: {}",
                                svc.restart.backoff_secs,
                                truncate_message(&err_detail, 180)
                            ),
                        },
                    )
                    .await;
                    backoff_until = Some(Instant::now() + Duration::from_secs(svc.restart.backoff_secs));
                }
            }
            continue;
        }

        if let Some(running_child) = running.as_mut() {
            tokio::select! {
                status = running_child.child.wait() => {
                    let status = status.with_context(|| format!("wait failed: {}", name))?;
                    running = None;

                    if !desired_running {
                        update_state(&state, &name, RuntimeRecord {
                            state: ServiceState::Stopped,
                            pid: None,
                            restarts: restart_count,
                            message: "service stopped".to_string(),
                        }).await;
                        continue;
                    }

                    if !svc.restart.enabled {
                        desired_running = false;
                        update_state(&state, &name, RuntimeRecord {
                            state: ServiceState::Failed,
                            pid: None,
                            restarts: restart_count,
                            message: format!("service exited ({:?}), restart disabled", status.code()),
                        }).await;
                        continue;
                    }

                    if svc.restart.max_retries > 0 && restart_count >= svc.restart.max_retries {
                        desired_running = false;
                        update_state(&state, &name, RuntimeRecord {
                            state: ServiceState::Failed,
                            pid: None,
                            restarts: restart_count,
                            message: format!("service exited ({:?}), retry limit reached", status.code()),
                        }).await;
                        continue;
                    }

                    restart_count += 1;
                    backoff_until = Some(Instant::now() + Duration::from_secs(svc.restart.backoff_secs));
                    update_state(&state, &name, RuntimeRecord {
                        state: ServiceState::Backoff,
                        pid: None,
                        restarts: restart_count,
                        message: format!("service exited ({:?}), scheduling restart", status.code()),
                    }).await;
                }
                cmd = rx.recv() => {
                    match cmd {
                        Some(ServiceCommand::Start) => {
                            desired_running = true;
                        }
                        Some(ServiceCommand::Stop) => {
                            desired_running = false;
                            terminate_process_group(&name, running_child, svc.restart.stop_grace_secs).await?;
                            running = None;
                            update_state(&state, &name, RuntimeRecord {
                                state: ServiceState::Stopped,
                                pid: None,
                                restarts: restart_count,
                                message: "service stopped by request".to_string(),
                            }).await;
                        }
                        Some(ServiceCommand::Restart) => {
                            desired_running = true;
                            restart_count = 0;
                            backoff_until = None;
                            terminate_process_group(&name, running_child, svc.restart.stop_grace_secs).await?;
                            running = None;
                            update_state(&state, &name, RuntimeRecord {
                                state: ServiceState::Starting,
                                pid: None,
                                restarts: restart_count,
                                message: "restart requested".to_string(),
                            }).await;
                        }
                        Some(ServiceCommand::Shutdown) => {
                            terminate_process_group(&name, running_child, svc.restart.stop_grace_secs).await?;
                            update_state(&state, &name, RuntimeRecord {
                                state: ServiceState::Stopped,
                                pid: None,
                                restarts: restart_count,
                                message: "daemon shutdown".to_string(),
                            }).await;
                            break;
                        }
                        None => {
                            terminate_process_group(&name, running_child, svc.restart.stop_grace_secs).await?;
                            break;
                        },
                    }
                }
            }
        }
    }

    Ok(())
}

fn format_error_chain(err: &anyhow::Error) -> String {
    let mut parts = Vec::new();
    for cause in err.chain() {
        let msg = cause.to_string();
        if parts.last() == Some(&msg) {
            continue;
        }
        parts.push(msg);
    }
    parts.join(": ")
}

fn truncate_message(value: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(value.len().min(max_chars));
    for (i, c) in value.chars().enumerate() {
        if i >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::{format_error_chain, truncate_message};

    #[test]
    fn truncate_message_limits_and_marks() {
        assert_eq!(truncate_message("abcdef", 3), "abc...");
        assert_eq!(truncate_message("abcdef", 6), "abcdef");
    }

    #[test]
    fn format_error_chain_deduplicates_adjacent_messages() {
        let err = anyhow!("inner").context("inner").context("outer");
        let value = format_error_chain(&err);
        assert_eq!(value, "outer: inner");
    }

    #[tokio::test]
    async fn startup_delay_can_be_stopped_and_shutdown_immediately() {
        use super::*;
        let svc = ServiceConfig {
            name: "delayed".into(), command: "must-never-be-launched".into(), args: vec![],
            working_dir: None, autostart: true, start_secs: 0, user: None,
            redirect_stderr: false, stdout_logfile: None, stderr_logfile: None,
            stdout_logfile_maxbytes: None, stdout_logfile_backups: None,
            privilege: PrivilegeMode::Normal, restart: crate::config::RestartConfig::default(),
        };
        let state = Arc::new(RwLock::new(HashMap::new()));
        let meta = Arc::new(RwLock::new(HashMap::new()));
        let policy = ServiceStartupPolicy {
            first_start_not_before: Instant::now() + Duration::from_secs(30),
            spin_interval: Duration::ZERO,
            spin_gate: Arc::new(tokio::sync::Mutex::new(Instant::now())),
        };
        let (tx, rx) = mpsc::channel(16);
        let task = tokio::spawn(manage_service_actor_impl(svc, rx, Arc::clone(&state), meta, policy));
        tx.send(ServiceCommand::Stop).await.unwrap();
        tx.send(ServiceCommand::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), task).await.unwrap().unwrap().unwrap();
        assert!(matches!(state.read().await.get("delayed").unwrap().state, ServiceState::Stopped));
        assert!(tx.is_closed());
    }
}

async fn apply_control(
    cmd: ServiceCommand,
    desired_running: &mut bool,
    backoff_until: &mut Option<Instant>,
) -> Result<ControlOutcome> {
    match cmd {
        ServiceCommand::Start => {
            *desired_running = true;
            Ok(ControlOutcome::Continue)
        }
        ServiceCommand::Stop => {
            *desired_running = false;
            *backoff_until = None;
            Ok(ControlOutcome::Continue)
        }
        ServiceCommand::Restart => {
            *desired_running = true;
            *backoff_until = None;
            Ok(ControlOutcome::Continue)
        }
        ServiceCommand::Shutdown => {
            *desired_running = false;
            *backoff_until = None;
            Ok(ControlOutcome::Exit)
        }
    }
}

async fn apply_start_spin_guard(policy: &ServiceStartupPolicy) {
    if policy.spin_interval.is_zero() {
        return;
    }
    let mut gate = policy.spin_gate.lock().await;
    let now = Instant::now();
    if *gate > now {
        sleep(gate.saturating_duration_since(now)).await;
    }
    *gate = Instant::now() + policy.spin_interval;
}
