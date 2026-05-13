# rustpm Architecture and Layout

## Motivation

`rustpm` is a cross-platform process manager that integrates with each OS-native service bootstrap mechanism, adding an operator-friendly orchestration and control layer.

## Role split (platform-agnostic)

- **Native bootstrap** (platform-specific): `launchd` on macOS, SCM on Windows.
- `rustpmd`: service orchestration and control plane.
- `rustpm-privd`: constrained elevated operations.

Each platform adapter preserves the same `rustpmd` / `rustpm-privd` responsibility boundary and control semantics (`rescan`, `reload <service>`, `restart <service>`).

## Platform-specific layout

### macOS

- Binary: `/usr/local/lib/rustpm/rustpmd`
- Priv helper: `/usr/local/lib/rustpm/rustpm-privd`
- Control CLI: `/usr/local/lib/rustpm/rustpmctl`
- Daemon config: `/usr/local/etc/rustpm/rustpmd.conf`
- Program config dir: `/usr/local/etc/rustpm/programs.d`
- Bootstrap: `/Library/LaunchDaemons/club.motofans.rustpm.plist`
- Control socket: `/usr/local/var/run/rustpm/rustpmd.sock`
- Logs: `/usr/local/var/log/rustpm.log`, `/usr/local/var/log/rustpm.err.log`

### Windows

- Binary: `C:\Program Files\rustpm\rustpmd.exe`
- Priv helper: `C:\Program Files\rustpm\rustpm-privd.exe`
- Control CLI: `C:\Program Files\rustpm\rustpmctl.exe`
- Daemon config: `C:\Program Files\rustpm\config\rustpmd.conf`
- Program config dir: `C:\Program Files\rustpm\config\programs.d`
- Bootstrap: Windows Service (SCM) with auto-start
- Control socket: `\\.\pipe\rustpmd`
- Logs: `C:\ProgramData\rustpm\logs\`

### OpenWrt (planned)

Layout paths and service integration points are not released yet.
