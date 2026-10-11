# v0.1.3

- Unify package, CLI, installer, workflow and documentation versions at 0.1.3.
- Fix Windows service registration, startup readiness, stop handling and recovery settings.
- Manage Windows child processes with owned handles and Job Objects; stopping a service also terminates descendants.
- Preserve environment configuration support in the native Windows process launcher.
- Fix reload/rescan races, lock ordering, stale metadata and shutdown handling of control connections.
- Reject malformed rescans without removing running services. Changing a service's privilege requires removing it, rescanning successfully, then adding it again.
- Disable the Web console by default. Windows normal services support LocalService/NetworkService; elevated services use the privileged daemon identity and omit an explicit user.
- Add Windows/Linux regression checks and Windows MSI build verification.

The MSI is unsigned. SCM installation and account switching still require testing on the target machine with administrator privileges.
