use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub dev: u64,
    pub inode: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub supports_posix_owner: bool,
    pub supports_posix_mode: bool,
    pub identity_source: &'static str,
}

pub trait FsPolicyAdapter: Send + Sync {
    fn capabilities(&self) -> PlatformCapabilities;
    fn canonicalize_under(&self, root: &Path, path: &Path) -> Result<PathBuf>;
    fn current_file_identity(&self, path: &Path) -> Result<FileIdentity>;
    fn verify_file_mode_exact(&self, path: &Path, expected_mode: u32) -> Result<()>;
    fn verify_root_owned(&self, path: &Path) -> Result<()>;
    fn set_owner_root(&self, path: &Path) -> Result<()>;
    fn set_file_mode(&self, path: &Path, mode: u32) -> Result<()>;
    fn running_as_root(&self) -> bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NativeFsPolicyAdapter;

impl FsPolicyAdapter for NativeFsPolicyAdapter {
    fn capabilities(&self) -> PlatformCapabilities {
        capabilities()
    }

    fn canonicalize_under(&self, root: &Path, path: &Path) -> Result<PathBuf> {
        canonicalize_under(root, path)
    }

    fn current_file_identity(&self, path: &Path) -> Result<FileIdentity> {
        current_file_identity(path)
    }

    fn verify_file_mode_exact(&self, path: &Path, expected_mode: u32) -> Result<()> {
        verify_file_mode_exact(path, expected_mode)
    }

    fn verify_root_owned(&self, path: &Path) -> Result<()> {
        verify_root_owned(path)
    }

    fn set_owner_root(&self, path: &Path) -> Result<()> {
        set_owner_root(path)
    }

    fn set_file_mode(&self, path: &Path, mode: u32) -> Result<()> {
        set_file_mode(path, mode)
    }

    fn running_as_root(&self) -> bool {
        running_as_root()
    }
}

pub fn default_policy_adapter() -> &'static dyn FsPolicyAdapter {
    static ADAPTER: NativeFsPolicyAdapter = NativeFsPolicyAdapter;
    &ADAPTER
}

#[cfg(unix)]
pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        supports_posix_owner: true,
        supports_posix_mode: true,
        identity_source: "unix_dev_inode",
    }
}

#[cfg(windows)]
pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        supports_posix_owner: false,
        supports_posix_mode: false,
        identity_source: "windows_volume_file_index",
    }
}

#[cfg(all(not(unix), not(windows)))]
pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        supports_posix_owner: false,
        supports_posix_mode: false,
        identity_source: "portable_path_hash",
    }
}

pub fn canonicalize_under(root: &Path, path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize path: {}", path.display()))?;
    if !canonical.starts_with(root) {
        return Err(anyhow!(
            "log path is outside allowed root {}: {}",
            root.display(),
            canonical.display()
        ));
    }
    Ok(canonical)
}

#[cfg(unix)]
pub fn current_file_identity(path: &Path) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path)
        .with_context(|| format!("failed to stat path: {}", path.display()))?;
    Ok(FileIdentity {
        dev: meta.dev(),
        inode: meta.ino(),
    })
}

#[cfg(windows)]
pub fn current_file_identity(path: &Path) -> Result<FileIdentity> {
    use std::os::windows::fs::MetadataExt;
    let meta = std::fs::metadata(path)
        .with_context(|| format!("failed to stat path: {}", path.display()))?;
    let inode = ((meta.file_index_high() as u64) << 32) | (meta.file_index_low() as u64);
    Ok(FileIdentity {
        dev: meta.volume_serial_number().unwrap_or(0) as u64,
        inode,
    })
}

#[cfg(all(not(unix), not(windows)))]
pub fn current_file_identity(path: &Path) -> Result<FileIdentity> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize path: {}", path.display()))?;
    let mut hash: u64 = 1469598103934665603;
    for b in canonical.to_string_lossy().as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    Ok(FileIdentity {
        dev: 0,
        inode: hash,
    })
}

#[cfg(unix)]
pub fn verify_file_mode_exact(path: &Path, expected_mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("failed to stat permissions: {}", path.display()))?
        .permissions()
        .mode()
        & 0o777;
    if mode != expected_mode {
        return Err(anyhow!(
            "file permission must be {:o} for {} (current {:o})",
            expected_mode,
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn verify_file_mode_exact(_path: &Path, _expected_mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn verify_file_not_group_world_writable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("failed to stat permissions: {}", path.display()))?
        .permissions()
        .mode()
        & 0o777;
    if (mode & 0o022) != 0 {
        return Err(anyhow!(
            "file must not be group/world writable for {} (current {:o})",
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn verify_file_not_group_world_writable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn verify_root_owned(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata(path)
        .with_context(|| format!("failed to stat owner: {}", path.display()))?
        .uid();
    if uid != 0 {
        return Err(anyhow!(
            "secure path must be owned by root: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn verify_root_owned(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn set_owner_root(path: &Path) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("invalid path for chown: {}", path.display()))?;
    // SAFETY: c_path points to a valid, NUL-terminated path buffer.
    let rc = unsafe { nix::libc::chown(c_path.as_ptr(), 0, !0) };
    if rc != 0 {
        return Err(anyhow!(
            "failed to chown path to root: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn set_owner_root(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn set_file_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to chmod {:o}: {}", mode, path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
pub fn set_file_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn running_as_root() -> bool {
    // SAFETY: geteuid has no preconditions and is thread-safe.
    unsafe { nix::libc::geteuid() == 0 }
}

#[cfg(not(unix))]
pub fn running_as_root() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{capabilities, default_policy_adapter, running_as_root};

    #[test]
    fn platform_capabilities_are_non_empty() {
        let caps = capabilities();
        assert!(!caps.identity_source.is_empty());
    }

    #[test]
    fn default_policy_adapter_matches_capabilities() {
        let adapter_caps = default_policy_adapter().capabilities();
        let direct_caps = capabilities();
        assert_eq!(adapter_caps, direct_caps);
    }

    #[test]
    fn default_policy_adapter_matches_root_detection() {
        assert_eq!(default_policy_adapter().running_as_root(), running_as_root());
    }
}
