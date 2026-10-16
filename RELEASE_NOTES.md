# v0.1.5

- Windows MSI now appends a dedicated, package-owned `cli` directory to machine PATH, making `rustpmctl` available by name in new terminals. The original CLI and service binary locations remain available.
- macOS pkg now includes `/usr/local/bin/rustpmctl` as a relative symlink. Installation refuses to overwrite an unrelated entry, and uninstall removes only the expected symlink.
- Add packaging regression tests, compiled MSI table checks, and English/Chinese command discovery diagnostics.
- Related issue: #1. The issue remains open pending installation and lifecycle testing.

After Windows installation, fully close and reopen PowerShell/cmd or Windows Terminal before running `rustpmctl --help`. Service-control operations still require the appropriate permissions. Do not manually add the package-owned `cli` directory to PATH; use the original installation directory for manual entries.

Validation: packaging regression tests and ShellCheck passed; the Windows MSI built successfully and its CLI/PATH tables were verified. Clean-machine installation, upgrades, repair, uninstall and rollback still require validation. The MSI is unsigned. The release asset is the Windows x64 MSI; macOS pkg can be built from source.
