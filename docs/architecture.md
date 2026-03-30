# rustpm Architecture and Layout

## Motivation

The project started from a practical pain point: `launchctl` is reliable but not ergonomic for frequent service operations.

`rustpm` keeps system-native bootstrap while adding an operator-friendly orchestration/control layer.

## macOS role split (current implementation)

- `launchd`: system-level bootstrap and daemon recovery.
- `rustpmd`: service orchestration and control plane.
- `rustpm-privd`: constrained elevated operations.

This model keeps alignment with Apple conventions and still provides efficient daily operations.

## Cross-platform direction

Current production implementation is macOS-first.
Future platform scope is explicitly planned as:

- Windows
- OpenWrt

Cross-platform principles:
- Keep the `rustpmd` / `rustpm-privd` responsibility boundary stable.
- Integrate with each platform's native service bootstrap mechanism rather than replacing it.
- Preserve service-level control semantics (`rescan`, `reload <service>`, `restart <service>`) across platforms.

## Layout

### macOS (current)

- Binary: `/usr/local/lib/rustpm/rustpmd`
- Priv helper: `/usr/local/lib/rustpm/rustpm-privd`
- Control CLI: `/usr/local/lib/rustpm/rustpmctl`
- Daemon config: `/usr/local/etc/rustpm/rustpmd.conf`
- Program config dir: `/usr/local/etc/rustpm/programs.d`
- LaunchDaemon plist: `/Library/LaunchDaemons/club.motofans.rustpm.plist`
- Control socket: `/usr/local/var/run/rustpm/rustpmd.sock`
- Logs: `/usr/local/var/log/rustpm.log`, `/usr/local/var/log/rustpm.err.log`

### Windows (planned)

Layout paths and service integration points are not released yet.

### OpenWrt (planned)

Layout paths and service integration points are not released yet.
