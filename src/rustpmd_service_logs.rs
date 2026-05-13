use super::*;

pub(super) fn apply_stdio_for_service(
    svc: &ServiceConfig,
    cmd: &mut Command,
) -> Result<ServiceLogBindings> {
    let mut bindings = ServiceLogBindings::default();
    if let Some(path) = &svc.stdout_logfile {
        bindings.stdout = Some(ensure_service_log_file(path)?);
    }
    if let Some(path) = &svc.stderr_logfile {
        bindings.stderr = Some(ensure_service_log_file(path)?);
    }

    let stdout = if let Some(path) = &svc.stdout_logfile {
        rotate_log_if_needed(
            path,
            svc.stdout_logfile_maxbytes,
            svc.stdout_logfile_backups.unwrap_or(0),
        )?;
        bindings.stdout = Some(ensure_service_log_file(path)?);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to open stdout_logfile: {}", path.display()))?;
        Stdio::from(file)
    } else {
        Stdio::inherit()
    };

    if svc.redirect_stderr {
        let stderr = if let Some(path) = &svc.stdout_logfile {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("failed to open redirected stderr file: {}", path.display()))?;
            Stdio::from(file)
        } else {
            Stdio::inherit()
        };
        cmd.stdout(stdout).stderr(stderr);
        return Ok(bindings);
    }

    let stderr = if let Some(path) = &svc.stderr_logfile {
        rotate_log_if_needed(
            path,
            svc.stdout_logfile_maxbytes,
            svc.stdout_logfile_backups.unwrap_or(0),
        )?;
        bindings.stderr = Some(ensure_service_log_file(path)?);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to open stderr_logfile: {}", path.display()))?;
        Stdio::from(file)
    } else {
        Stdio::inherit()
    };

    cmd.stdout(stdout).stderr(stderr);
    Ok(bindings)
}

pub(super) fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create log directory: {}", parent.display()))?;
        set_dir_permissions(parent)?;
    }
    Ok(())
}

pub(super) fn ensure_log_file_impl(path: &Path) -> Result<()> {
    ensure_parent_dir(path)?;
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to create log file: {}", path.display()))?;
    set_log_file_permissions(path)?;
    Ok(())
}

pub(super) fn ensure_service_log_file(path: &Path) -> Result<LogBinding> {
    NativeLogSecurityPolicy::prepare_binding(path)
}

pub(super) fn canonicalize_allowed_log_path_impl(path: &Path) -> Result<PathBuf> {
    let fs_policy = platform_fs::default_policy_adapter();
    let root = platform_fs::canonicalize(Path::new(LOG_ROOT))
        .with_context(|| format!("failed to canonicalize log root: {}", LOG_ROOT))?;
    fs_policy.canonicalize_under(&root, path)
}

pub(super) fn set_dir_permissions(path: &Path) -> Result<()> {
    platform_fs::default_policy_adapter()
        .set_file_mode(path, 0o750)
        .with_context(|| format!("failed to set directory permissions: {}", path.display()))?;
    Ok(())
}

pub(super) fn set_log_file_permissions(path: &Path) -> Result<()> {
    platform_fs::default_policy_adapter()
        .set_file_mode(path, 0o640)
        .with_context(|| format!("failed to set log file permissions: {}", path.display()))?;
    Ok(())
}

pub(super) fn current_log_identity_impl(path: &Path) -> Result<FileIdentity> {
    platform_fs::default_policy_adapter().current_file_identity(path)
}

pub(super) fn rotate_log_if_needed(path: &Path, max_bytes: Option<u64>, backups: u32) -> Result<()> {
    let Some(limit) = max_bytes else {
        return Ok(());
    };
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() < limit || backups == 0 {
        return Ok(());
    }

    for idx in (1..=backups).rev() {
        let src = if idx == 1 {
            path.to_path_buf()
        } else {
            PathBuf::from(format!("{}.{}", path.display(), idx - 1))
        };
        let dst = PathBuf::from(format!("{}.{}", path.display(), idx));
        if src.exists() {
            if dst.exists() {
                std::fs::remove_file(&dst)
                    .with_context(|| format!("failed to remove old log backup: {}", dst.display()))?;
            }
            std::fs::rename(&src, &dst).with_context(|| {
                format!(
                    "failed to rotate log from {} to {}",
                    src.display(),
                    dst.display()
                )
            })?;
        }
    }
    Ok(())
}
