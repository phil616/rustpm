# rustpm

[English](README.md) | [简体中文](README.zh-CN.md)

`rustpm` is a process manager focused on predictable operations, explicit privilege boundaries, and low-friction daily control.

This document is user-facing: installation, configuration, operations, diagnostics, and uninstall guidance by platform.

## Why develop rustpm

`rustpm` provides a consistent service-management workflow across platforms while integrating with each OS-native bootstrap mechanism.

## Project goals

- [x] Cross-platform runtime: macOS (`launchd` bootstrap) + Windows (SCM service).
- [x] Service-level control plane (`rustpmctl` + Web console) with explicit privilege boundary.
- [x] Config model aligned with existing operator habits through `supervisord`-style syntax compatibility.
- [x] Windows Service support via SCM integration.
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

### Windows (supported)

rustpm runs as a Windows Service managed by the Windows Service Control Manager.

Prerequisites:
- Windows 10/11 or Windows Server 2016+
- Administrator privileges (required for service installation)

Install from MSI (recommended):

```powershell
# Build MSI (requires WiX Toolset)
.\scripts\build_msi.ps1

# Double-click dist\rustpm-0.1.0-x64.msi to install
# Windows will show UAC elevation prompt automatically.

# Or command-line (must run from elevated/admin PowerShell):
msiexec /i dist\rustpm-0.1.0-x64.msi

# Silent install (must run from elevated/admin PowerShell):
msiexec /i dist\rustpm-0.1.0-x64.msi /quiet

# Uninstall:
msiexec /x dist\rustpm-0.1.0-x64.msi
```

Build from source (with manual service registration):

```powershell
# Clone and build
cargo build --release

# Install as Windows Service
rustpmd.exe install --config "C:\Program Files\rustpm\config\rustpmd.conf"
```

Service management:

```powershell
# Start the service
Start-Service -Name rustpm

# Check status
Get-Service -Name rustpm

# Stop the service
Stop-Service -Name rustpm

# Uninstall via Control Panel (Programs and Features)
# or run the MSI uninstall:
# msiexec /x C:\path\to\rustpm-<version>-x64.msi
```

### GitHub Releases (Windows amd64)

The [release workflow](.github/workflows/release.yml) builds only the Windows
amd64 MSI and attaches it to the corresponding GitHub Release. For `v0.1.0`,
the asset is `rustpm-0.1.0-x64.msi`.

After committing and pushing the workflow and packaging changes, publish the tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The release tag sets the Cargo package, MSI product, and registry versions in
the temporary CI checkout; dependency versions remain locked. To retry an existing tag, use
**Actions → Release Windows MSI → Run workflow** (default tag: `v0.1.1`).
Select the `main` branch to use the latest workflow fixes for an existing tag.
The tagged commit must include the packaging changes. No additional repository
secret is required; publication uses `GITHUB_TOKEN` with `contents: write`.

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

If a macOS `directory` or `command` segment contains spaces, quote that segment, for example:

```ini
directory = "/Users/alice/My Service"
command = "/Users/alice/My Service/bin/example" -c "/Users/alice/My Service/config/example.config"
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

### Windows (supported)

`rustpm` configuration syntax is platform-agnostic; the same INI format works across macOS and Windows.

Default locations:
- Daemon config: `C:\Program Files\rustpm\config\rustpmd.conf`
- Programs config: `C:\Program Files\rustpm\config\programs.d\`
- Logs: `C:\Program Files\rustpm\logs\`
- IPC pipe: `\\.\pipe\rustpmd`
- Privd pipe: `\\.\pipe\rustpm-privd`

Program config format (`programs.d\*.conf`):

```ini
[program:example]
directory = C:\Users\admin\example
command = C:\Users\admin\example\bin\example.exe -c C:\Users\admin\example\config.ini
user = Administrator
privilege = elevated
startsecs = 3
autorestart = true
redirect_stderr = true
stdout_logfile_maxbytes = 50MB
stdout_logfile_backups = 10
stdout_logfile = C:\Program Files\rustpm\logs\%(program_name)s.log
stderr_logfile = C:\Program Files\rustpm\logs\%(program_name)s-error.log
```

If a Windows command path or argument contains spaces (for example `C:\Program Files\...`),
you must quote that segment, for example:

```ini
directory = "C:/Program Files/My Service"
command = "C:/Program Files/My Service/myservice.exe" --config "C:/Program Files/My Service/config.ini"
```

Supported keys (same as macOS):
- `directory`
- `command`
- `user`:
  - `Administrator` is treated as `privilege=elevated`
  - for `privilege=normal`, a local non-admin account can be specified
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
config_dir = C:\Program Files\rustpm\config\programs.d
socket = \\.\pipe\rustpmd
privd_socket = \\.\pipe\rustpm-privd

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

Default password is `password` (recommended to change immediately in production).

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

### Windows (supported)

Service control with `rustpmctl`:

```powershell
# List all services
rustpmctl.exe list

# Rescan for new services
rustpmctl.exe rescan

# Show response codes
rustpmctl.exe codes

# Show runtime contract
rustpmctl.exe contract

# JSON output
rustpmctl.exe --json list

# Check configuration
rustpmctl.exe check-config --config "C:\Program Files\rustpm\config\rustpmd.conf"

# Ping the daemon
rustpmctl.exe ping

# Service operations
rustpmctl.exe status myservice
rustpmctl.exe start myservice
rustpmctl.exe stop myservice
rustpmctl.exe restart myservice
rustpmctl.exe reload myservice

# Service operations with config check
rustpmctl.exe restart myservice --check-config --config "C:\Program Files\rustpm\config\rustpmd.conf"
```

Notes:
- `rescan`: rescans `programs.d` to discover newly added services.
- `reload <service>`: reloads only the config file that owns `<service>`.
- `rustpmctl` communicates with the service via Named Pipe (`\\.\pipe\rustpmd`).

Create a managed service:

```powershell
# Create service config file
New-Item -ItemType File -Path "C:\Program Files\rustpm\config\programs.d\myservice.conf" -Value @"
[program:myservice]
directory = C:\Users\admin\myservice
command = C:\Users\admin\myservice\myservice.exe
privilege = normal
startsecs = 3
autorestart = true
redirect_stderr = true
stdout_logfile_maxbytes = 50MB
stdout_logfile_backups = 10
stdout_logfile = C:\Program Files\rustpm\logs\myservice.log
stderr_logfile = C:\Program Files\rustpm\logs\myservice-error.log
"@

# Restart the service to pick up new config
rustpmctl.exe rescan
rustpmctl.exe restart myservice --check-config --config "C:\Program Files\rustpm\config\rustpmd.conf"
```

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

### Windows (supported)

Check service status:

```powershell
# Service status
Get-Service -Name rustpm

# Detailed service information
sc query rustpm

# Event logs
Get-EventLog -LogName Application -Source rustpm -Newest 50
```

Service/runtime logs:

```powershell
# View recent log entries
Get-Content -Tail 200 "C:\Program Files\rustpm\logs\myservice.log"
Get-Content -Tail 200 "C:\Program Files\rustpm\logs\myservice-error.log"
Get-Content -Tail 200 "C:\Program Files\rustpm\logs\rustpm.log"
Get-Content -Tail 200 "C:\Program Files\rustpm\logs\rustpm-privd.log"
```

Debug mode (run in foreground for troubleshooting):

```powershell
# Run in foreground (not as service)
rustpmd.exe run --config "C:\Program Files\rustpm\config\rustpmd.conf"
```

Service recovery:
- Windows Service Control Manager automatically restarts rustpm on failure.
- Recovery actions can be configured via `sc failure` command.

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

### Windows (supported)

Open **Control Panel → Programs and Features**, find **rustpm**, and click **Uninstall**.

If you have the MSI file, you can also run:

```powershell
msiexec /x C:\path\to\rustpm-<version>-x64.msi
```

This will:
- Stop and remove the Windows Service
- Not remove config files or logs (manual cleanup if needed)

Manual cleanup (if needed):

```powershell
# Remove config directory
Remove-Item -Recurse -Force "C:\Program Files\rustpm\config"

# Remove logs directory
Remove-Item -Recurse -Force "C:\Program Files\rustpm\logs"

# Remove installation directory
Remove-Item -Recurse -Force "C:\Program Files\rustpm"
```

### OpenWrt (planned)

Uninstall workflow is not released yet.
