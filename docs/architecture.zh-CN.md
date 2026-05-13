# rustpm 架构与目录布局

## 动机

`rustpm` 是一个跨平台进程管理器，对接各平台原生服务引导机制，提供友好的编排与控制层。

## 角色分工（平台无关）

- **原生引导**（平台相关）：macOS 使用 `launchd`，Windows 使用 SCM。
- `rustpmd`：服务编排与控制面。
- `rustpm-privd`：受控高权限操作。

各平台适配器保持相同的 `rustpmd` / `rustpm-privd` 职责边界和控制语义（`rescan`、`reload <service>`、`restart <service>`）。

## 各平台目录布局

### macOS

- 二进制：`/usr/local/lib/rustpm/rustpmd`
- 特权助手：`/usr/local/lib/rustpm/rustpm-privd`
- 控制 CLI：`/usr/local/lib/rustpm/rustpmctl`
- 守护配置：`/usr/local/etc/rustpm/rustpmd.conf`
- 服务配置目录：`/usr/local/etc/rustpm/programs.d`
- 引导：`/Library/LaunchDaemons/club.motofans.rustpm.plist`
- 控制 socket：`/usr/local/var/run/rustpm/rustpmd.sock`
- 日志：`/usr/local/var/log/rustpm.log`、`/usr/local/var/log/rustpm.err.log`

### Windows

- 二进制：`C:\Program Files\rustpm\rustpmd.exe`
- 特权助手：`C:\Program Files\rustpm\rustpm-privd.exe`
- 控制 CLI：`C:\Program Files\rustpm\rustpmctl.exe`
- 守护配置：`C:\Program Files\rustpm\config\rustpmd.conf`
- 服务配置目录：`C:\Program Files\rustpm\config\programs.d`
- 引导：Windows 服务（SCM）自动启动
- 控制管道：`\\.\pipe\rustpmd`
- 日志：`C:\ProgramData\rustpm\logs\`

### OpenWrt（规划中）

目录路径与服务集成点暂未发布。
