# rustpm

[English](README.md) | [简体中文](README.zh-CN.md)

`rustpm` is a process manager focused on predictable operations, explicit privilege boundaries, and low-friction daily control.

This document is user-facing: installation, configuration, operations, diagnostics, and uninstall guidance by platform.

## Why develop rustpm

The motivation for developing `rustpm` comes from a practical issue: `launchctl` is powerful but difficult for daily service operations.

`rustpm` keeps Apple-native bootstrap behavior while providing a cleaner service-management workflow for everyday use.

## Project goals

- [x] Stable macOS runtime based on `launchd` bootstrap + `rustpm` orchestration.
- [x] Service-level control plane (`rustpmctl` + Web console) with explicit privilege boundary.
- [x] Config model aligned with existing operator habits through `supervisord`-style syntax compatibility.
- [ ] Windows platform support (design target).
- [ ] OpenWrt platform support (design target).

## Public docs

- Architecture and layout: [docs/architecture.md](docs/architecture.md)
- Security model and API notes: [docs/security-api.md](docs/security-api.md)

## Installation

### macOS (supported)

Option A: build unsigned pkg from source (self-use)

Rust toolchain is required for source build.
Recommended installation source:
- <https://forge.rust-lang.org/infra/other-installation-methods.html>

Build:

```bash
chmod +x scripts/build_pkg.sh packaging/postinstall
./scripts/build_pkg.sh
```

Generated package:
- `dist/rustpm-0.1.0-unsigned.pkg`

Option B: install a prebuilt pkg

If you already have a prebuilt pkg (from CI/artifacts or from another machine), local Rust is not required.

Install:

```bash
sudo installer -pkg dist/rustpm-0.1.0-unsigned.pkg -target /
```

Installer defaults:
- `/usr/local/etc/rustpm/programs.d/example.conf` is installed as a commented template.
- No real business service is enabled by default.
- Security defaults:
  - `/usr/local/etc/rustpm/rustpmd.conf` mode `600` (root-only)
  - `/usr/local/etc/rustpm/programs.d` mode `700` (root-only)
  - `programs.d/*.conf` mode `600` (root-only)

### Windows (planned)

Installation workflow is not released yet.

### OpenWrt (planned)

Installation workflow is not released yet.

## Configuration

### macOS (supported)

`rustpm` configuration syntax references `supervisord` conventions and keeps practical compatibility to reduce migration and onboarding cost.

Because this compatibility already covers common operational workflows, there is currently no plan to provide dedicated support for mainstream distributions that already ship `supervisord` packages.

Program config format (`programs.d/*.conf`):

```ini
[program:example]
directory = /usr/local/example
command = /usr/local/bin/example -c /usr/local/etc/example/example.config
user = root
privilege = elevated
startsecs = 3
autorestart = true
redirect_stderr = true
stdout_logfile_maxbytes = 50MB
stdout_logfile_backups = 10
stdout_logfile = /usr/local/var/log/%(program_name)s.log
stderr_logfile = /usr/local/var/log/%(program_name)s-error.log
```

Supported keys:
- `directory`
- `command`
- `user`:
  - `root` or `Administrator` is treated as `privilege=elevated`
  - for `privilege=normal`, a local non-root account can be specified and rustpm will spawn the service with that user identity on Unix
- `privilege` (`normal` or `elevated`)
- `startsecs`
- `autorestart`
- `redirect_stderr`
- `stdout_logfile_maxbytes`
- `stdout_logfile_backups`
- `stdout_logfile`
- `stderr_logfile`

Daemon config format (`rustpmd.conf`):

```ini
config_dir = /usr/local/etc/rustpm/programs.d
socket = /usr/local/var/run/rustpm/rustpmd.sock
privd_socket = /usr/local/var/run/rustpm/rustpm-privd.sock

[web]
enabled = true
bind = 127.0.0.1
port = 18765
expose = false
password_hash = argon2id:<phc>

[dependency]
require_privd = true
startup_timeout_secs = 30
healthcheck_interval_secs = 5
failure_threshold = 3
service_start_delay_secs = 30
service_start_spin_interval_secs = 1
```

Web is enabled by default, with default password `password` (recommended to change immediately in production).
Detailed security and API behavior: [docs/security-api.md](docs/security-api.md).

### Windows (planned)

Configuration format and file locations are not released yet.

### OpenWrt (planned)

Configuration format and file locations are not released yet.

## Operations

### macOS (supported)

Runtime commands:

```bash
sudo /usr/local/lib/rustpm/rustpmd check-config --config /usr/local/etc/rustpm/rustpmd.conf --config-dir /usr/local/etc/rustpm/programs.d
sudo /usr/local/lib/rustpm/rustpmd run --validate-config --config /usr/local/etc/rustpm/rustpmd.conf --config-dir /usr/local/etc/rustpm/programs.d
sudo launchctl print system/club.motofans.rustpm
sudo launchctl kickstart -k system/club.motofans.rustpm
sudo launchctl bootout system/club.motofans.rustpm
```

Service control with `rustpmctl`:

```bash
sudo /usr/local/lib/rustpm/rustpmctl list
sudo /usr/local/lib/rustpm/rustpmctl rescan
sudo /usr/local/lib/rustpm/rustpmctl codes
sudo /usr/local/lib/rustpm/rustpmctl contract
sudo /usr/local/lib/rustpm/rustpmctl --json codes
sudo /usr/local/lib/rustpm/rustpmctl --json contract
sudo /usr/local/lib/rustpm/rustpmctl check-config --config /usr/local/etc/rustpm/rustpmd.conf --config-dir /usr/local/etc/rustpm/programs.d
sudo /usr/local/lib/rustpm/rustpmctl ping
sudo /usr/local/lib/rustpm/rustpmctl status api
sudo /usr/local/lib/rustpm/rustpmctl stop api
sudo /usr/local/lib/rustpm/rustpmctl start api
sudo /usr/local/lib/rustpm/rustpmctl restart api
sudo /usr/local/lib/rustpm/rustpmctl reload api
sudo /usr/local/lib/rustpm/rustpmctl --json list
sudo /usr/local/lib/rustpm/rustpmctl --json status api
sudo /usr/local/lib/rustpm/rustpmctl restart api --check-config --config /usr/local/etc/rustpm/rustpmd.conf --config-dir /usr/local/etc/rustpm/programs.d
sudo /usr/local/lib/rustpm/rustpmctl reload api --check-config --config /usr/local/etc/rustpm/rustpmd.conf --config-dir /usr/local/etc/rustpm/programs.d
```

Notes:
- `rescan`: rescans `programs.d` to discover newly added services; does not auto-restart existing services.
- `reload <service>`: reloads only the config file that owns `<service>`, runs conflict checks, then reloads that service only.
- There is no global `reload` command. Use `rescan` for global discovery only.

Create a managed service:

```bash
sudo tee /usr/local/etc/rustpm/programs.d/easytier.conf >/dev/null <<'EOF_SERVICE'
[program:easytier]
directory = /Users/ares
command = /usr/local/bin/easytier-core -c /Users/ares/.config/easytier/config.toml
privilege = elevated
startsecs = 3
autorestart = true
redirect_stderr = true
stdout_logfile_maxbytes = 50MB
stdout_logfile_backups = 10
stdout_logfile = /usr/local/var/log/%(program_name)s.log
stderr_logfile = /usr/local/var/log/%(program_name)s-error.log
EOF_SERVICE
```

```bash
sudo /usr/local/lib/rustpm/rustpmctl check-config \
  --config /usr/local/etc/rustpm/rustpmd.conf \
  --config-dir /usr/local/etc/rustpm/programs.d
sudo /usr/local/lib/rustpm/rustpmctl restart easytier \
  --check-config \
  --config /usr/local/etc/rustpm/rustpmd.conf \
  --config-dir /usr/local/etc/rustpm/programs.d
sudo /usr/local/lib/rustpm/rustpmctl rescan
sudo /usr/local/lib/rustpm/rustpmctl status easytier
```

### Windows (planned)

Operational command set is not released yet.

### OpenWrt (planned)

Operational command set is not released yet.

## Diagnostics

### macOS (supported)

If launchd startup fails during `postinstall`, pkg installation still completes.
In that case, start services manually:

```bash
sudo launchctl bootstrap system /Library/LaunchDaemons/club.motofans.rustpm-privd.plist
sudo launchctl kickstart -k system/club.motofans.rustpm-privd
sudo launchctl bootstrap system /Library/LaunchDaemons/club.motofans.rustpm.plist
sudo launchctl kickstart -k system/club.motofans.rustpm
```

Service/runtime logs:

```bash
sudo tail -n 200 /usr/local/var/log/easytier.log
sudo tail -n 200 /usr/local/var/log/easytier-error.log
sudo tail -n 200 /usr/local/var/log/rustpm.err.log
sudo tail -n 200 /usr/local/var/log/rustpm-privd.err.log
```

### Windows (planned)

Diagnostic procedures are not released yet.

### OpenWrt (planned)

Diagnostic procedures are not released yet.

## Uninstall

### macOS (supported)

Recommended (interactive, with deletion summary prompt):

```bash
sudo /usr/local/lib/rustpm/uninstall_rustpm.sh
```

Dry-run preview:

```bash
sudo /usr/local/lib/rustpm/uninstall_rustpm.sh --dry-run
```

Non-interactive:

```bash
sudo /usr/local/lib/rustpm/uninstall_rustpm.sh --yes
```

The uninstall script removes:
- launchd services: `club.motofans.rustpm`, `club.motofans.rustpm-privd`
- launchd plist files under `/Library/LaunchDaemons`
- `/usr/local/lib/rustpm`
- `/usr/local/etc/rustpm`
- `/usr/local/etc/rustpm/programs.d`
- `/usr/local/etc/rustpm/programs.d/*.conf`
- `/usr/local/var/run/rustpm`
- rustpm log files under `/usr/local/var/log`

### Windows (planned)

Uninstall workflow is not released yet.

### OpenWrt (planned)

Uninstall workflow is not released yet.
