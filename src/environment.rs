//! Per-service environment overrides. Values never pass through a shell.
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

#[derive(Clone, Default, Deserialize)]
pub struct EnvironmentConfig {
    #[serde(default)]
    variables: BTreeMap<String, String>,
    #[serde(default)]
    files: Vec<EnvironmentFile>,
    // Keep this independent of ServiceConfig::privilege: privd rewrites that
    // field when it takes ownership of an elevated service.
    #[serde(skip)]
    require_root_owned: bool,
}

impl fmt::Debug for EnvironmentConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentConfig")
            .field("variable_count", &self.variables.len())
            .field("file_count", &self.files.len())
            .field("require_root_owned", &self.require_root_owned)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize)]
struct EnvironmentFile {
    path: PathBuf,
    optional: bool,
}

/// The original line number is retained for errors without exposing values.
pub(crate) type EnvironmentDirective = (String, String, usize);

impl EnvironmentConfig {
    pub(crate) fn parse(
        source: &Path,
        directives: &[EnvironmentDirective],
        elevated: bool,
    ) -> Result<Self> {
        let mut config = Self {
            require_root_owned: elevated,
            ..Self::default()
        };
        let base = std::path::absolute(source)?;
        let base = base.parent().expect("absolute config path has a parent");
        for (key, value, line) in directives {
            let result = (|| -> Result<()> {
                if key == "environment" {
                    if value.is_empty() {
                        config.variables.clear();
                    } else {
                        for assignment in split_words(value)? {
                            let (name, value) = assignment
                                .split_once('=')
                                .ok_or_else(|| anyhow!("expected NAME=value assignment"))?;
                            insert_variable(&mut config.variables, name, value)?;
                        }
                    }
                } else if value.is_empty() {
                    config.files.clear();
                } else {
                    for path in split_words(value)? {
                        let (optional, path) = match path.strip_prefix('-') {
                            Some(path) => (true, path),
                            None => (false, path.as_str()),
                        };
                        if path.is_empty() || path.contains('\0') {
                            bail!("expected a nonempty environment file path");
                        }
                        config.files.push(EnvironmentFile {
                            path: base.join(path),
                            optional,
                        });
                    }
                }
                Ok(())
            })();
            result.map_err(|err| {
                anyhow!("{}:{}: invalid {}: {}", source.display(), line, key, err)
            })?;
        }
        Ok(config)
    }

    /// Read on every spawn, including automatic restarts. Do not cache file values.
    pub fn resolve(&self) -> Result<BTreeMap<String, String>> {
        let mut variables = BTreeMap::new();
        for (name, value) in &self.variables {
            insert_variable(&mut variables, name, value)?;
        }
        for entry in &self.files {
            // Reject directories/devices/FIFOs before opening (opening a FIFO
            // can block). Validate the opened handle again below.
            match std::fs::metadata(&entry.path) {
                Ok(meta) if !meta.is_file() => bail!(
                    "environment file is not a regular file: {}",
                    entry.path.display()
                ),
                Err(err) if entry.optional && err.kind() == std::io::ErrorKind::NotFound => {
                    continue
                }
                Err(err) => {
                    return Err(anyhow!(
                        "cannot stat environment file {}: {}",
                        entry.path.display(),
                        err
                    ))
                }
                Ok(_) => {}
            }
            let mut file = match File::open(&entry.path) {
                Ok(file) => file,
                Err(err) if entry.optional && err.kind() == std::io::ErrorKind::NotFound => {
                    continue
                }
                Err(err) => {
                    return Err(anyhow!(
                        "cannot open environment file {}: {}",
                        entry.path.display(),
                        err
                    ))
                }
            };
            let metadata = file.metadata().with_context(|| {
                format!("cannot stat environment file {}", entry.path.display())
            })?;
            if !metadata.is_file() {
                bail!(
                    "environment file is not a regular file: {}",
                    entry.path.display()
                );
            }
            #[cfg(unix)]
            if self.require_root_owned {
                use std::os::unix::fs::MetadataExt;
                if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                    bail!("elevated environment file must be root-owned and not group/other writable: {}", entry.path.display());
                }
            }
            let mut raw = String::new();
            file.read_to_string(&mut raw).with_context(|| {
                format!(
                    "cannot read UTF-8 environment file {}",
                    entry.path.display()
                )
            })?;
            parse_file(&raw, &entry.path, &mut variables)?;
        }
        Ok(variables)
    }
}

fn split_words(value: &str) -> Result<Vec<String>> {
    let words =
        shlex::split(value).ok_or_else(|| anyhow!("unbalanced quotes or trailing escape"))?;
    if words.is_empty() {
        bail!("expected an assignment or path");
    }
    Ok(words)
}

fn insert_variable(
    variables: &mut BTreeMap<String, String>,
    name: &str,
    value: &str,
) -> Result<()> {
    let mut chars = name.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        bail!("invalid environment variable name (expected [A-Za-z_][A-Za-z0-9_]*)");
    }
    if value.contains('\0') {
        bail!("environment value contains NUL");
    }
    // Windows treats environment names case-insensitively. Remove earlier
    // spellings so precedence follows configuration order, not map sorting.
    #[cfg(windows)]
    variables.retain(|key, _| !key.eq_ignore_ascii_case(name));
    variables.insert(name.to_string(), value.to_string());
    Ok(())
}

fn parse_file(raw: &str, path: &Path, variables: &mut BTreeMap<String, String>) -> Result<()> {
    // Accept a UTF-8 BOM and CRLF for files authored on Windows.
    for (index, line) in raw.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let result = (|| -> Result<()> {
            let (name, raw_value) = line
                .split_once('=')
                .ok_or_else(|| anyhow!("expected NAME=value assignment"))?;
            let raw_value = raw_value.trim();
            let value = if raw_value.starts_with(['\'', '"']) {
                if raw_value.len() < 2 || !raw_value.ends_with(raw_value.chars().next().unwrap()) {
                    bail!("quoted file value must end with its opening quote");
                }
                let values = split_words(raw_value)?;
                if values.len() != 1 {
                    bail!("quoted file value must be a single string");
                }
                values[0].clone()
            } else {
                raw_value.to_string()
            };
            insert_variable(variables, name.trim(), &value)
        })();
        result.map_err(|err| anyhow!("{}:{}: {}", path.display(), index + 1, err))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn parse(directives: &[(&str, &str)], source: &Path) -> EnvironmentConfig {
        let directives: Vec<_> = directives
            .iter()
            .enumerate()
            .map(|(i, (key, value))| (key.to_string(), value.to_string(), i + 1))
            .collect();
        EnvironmentConfig::parse(source, &directives, false).unwrap()
    }

    #[test]
    fn inline_order_reset_empty_values_and_literal_expansions() {
        let config = parse(
            &[
                ("environment", "OLD=removed"),
                ("environment", ""),
                (
                    "environment",
                    "A=first EMPTY= 'MESSAGE=hello world' 'LITERAL=$HOME $(whoami) %PATH%'",
                ),
                ("environment", "A=last 'WIN=C:\\Program Files\\App'"),
            ],
            Path::new("service.conf"),
        );
        let vars = config.resolve().unwrap();
        assert!(!vars.contains_key("OLD"));
        assert_eq!(vars["A"], "last");
        assert_eq!(vars["EMPTY"], "");
        assert_eq!(vars["MESSAGE"], "hello world");
        assert_eq!(vars["LITERAL"], "$HOME $(whoami) %PATH%");
        assert_eq!(vars["WIN"], "C:\\Program Files\\App");
        assert!(!format!("{config:?}").contains("whoami"));
    }

    #[test]
    fn file_format_preserves_spaces_equals_comments_and_windows_paths() {
        let mut vars = BTreeMap::new();
        parse_file("\u{feff}# comment\r\n; comment\r\nA = one  two = three # literal\r\nEMPTY=\r\nQUOTED=\"  hello 世界  \"\r\nPATH=C:\\tools\\bin\r\nLITERAL=$HOME $(id)\n", Path::new("test.env"), &mut vars).unwrap();
        assert_eq!(vars["A"], "one  two = three # literal");
        assert_eq!(vars["EMPTY"], "");
        assert_eq!(vars["QUOTED"], "  hello 世界  ");
        assert_eq!(vars["PATH"], "C:\\tools\\bin");
        assert_eq!(vars["LITERAL"], "$HOME $(id)");
    }

    #[test]
    fn invalid_values_are_rejected_without_leaking_contents() {
        for bad in [
            "NO_EQUALS",
            "9NAME=secret",
            "BAD-NAME=secret",
            "=secret",
            "TOKEN=secret\0",
            "'TOKEN=secret",
        ] {
            let directives = vec![("environment".into(), bad.into(), 17)];
            let err = EnvironmentConfig::parse(Path::new("svc.conf"), &directives, false)
                .unwrap_err()
                .to_string();
            assert!(err.contains("svc.conf:17"));
            assert!(!err.contains("secret"));
        }
        for bad in [
            "TOKEN='secret",
            "export TOKEN=secret",
            "TOKEN=secret\0",
            "TOKEN=\"secret\" extra",
        ] {
            let err = parse_file(bad, Path::new("bad.env"), &mut BTreeMap::new())
                .unwrap_err()
                .to_string();
            assert!(err.contains("bad.env:1"));
            assert!(!err.contains("secret"));
        }
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("rustpm-env-{}", rand::random::<u64>()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn files_override_inline_in_order_and_are_read_again() {
        let dir = TempDir::new();
        fs::write(dir.0.join("first.env"), "A=first\nB=preserved").unwrap();
        fs::write(dir.0.join("second file.env"), "A=second").unwrap();
        let config = parse(
            &[
                (
                    "environment_file",
                    "first.env 'second file.env' -absent.env",
                ),
                ("environment", "A=inline"),
            ],
            &dir.0.join("service.conf"),
        );
        let vars = config.resolve().unwrap();
        assert_eq!(vars["A"], "second");
        assert_eq!(vars["B"], "preserved");
        fs::write(dir.0.join("second file.env"), "A=updated").unwrap();
        assert_eq!(config.resolve().unwrap()["A"], "updated");
        fs::remove_file(dir.0.join("first.env")).unwrap();
        assert!(config.resolve().is_err());
        let reset = parse(
            &[
                ("environment_file", "missing.env"),
                ("environment_file", ""),
                ("environment", "A=ok"),
            ],
            &dir.0.join("service.conf"),
        );
        assert_eq!(reset.resolve().unwrap()["A"], "ok");
    }

    #[test]
    fn optional_only_ignores_missing_files() {
        let dir = TempDir::new();
        let config = parse(
            &[("environment_file", "-optional.env")],
            &dir.0.join("service.conf"),
        );
        assert!(config.resolve().unwrap().is_empty());
        fs::write(dir.0.join("optional.env"), "INVALID").unwrap();
        assert!(config.resolve().is_err());
        fs::remove_file(dir.0.join("optional.env")).unwrap();
        fs::create_dir(dir.0.join("optional.env")).unwrap();
        assert!(config.resolve().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn elevated_files_must_not_be_writable_by_other_users() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let path = dir.0.join("unsafe.env");
        fs::write(&path, "TOKEN=secret").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        let mut config = parse(
            &[("environment_file", "unsafe.env")],
            &dir.0.join("service.conf"),
        );
        config.require_root_owned = true;
        let err = config.resolve().unwrap_err().to_string();
        assert!(err.contains("root-owned"));
        assert!(!err.contains("secret"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_names_are_case_insensitive_and_last_wins() {
        let config = parse(
            &[("environment", "Path=first PATH=second Path=last")],
            Path::new("service.conf"),
        );
        let vars = config.resolve().unwrap();
        assert_eq!(vars.len(), 1);
        assert_eq!(vars["Path"], "last");
    }
}
