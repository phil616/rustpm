# rustpm 架构与目录布局

## 动机

这个项目起步于一个实际痛点：`launchctl` 很可靠，但在高频日常服务管理中不够顺手。

`rustpm` 的目标是保留系统原生引导，同时提供更友好的编排与控制层。

## macOS 角色分工（当前实现）

- `launchd`：系统级引导与守护恢复。
- `rustpmd`：服务编排与控制面。
- `rustpm-privd`：受控高权限操作。

该模型既遵循 Apple 体系，也保留了日常管理效率。

## 跨平台方向

当前生产实现以 macOS 为主。
未来明确规划支持：

- Windows
- OpenWrt

跨平台设计原则：
- 保持 `rustpmd` / `rustpm-privd` 职责边界稳定。
- 对接各平台原生服务引导机制，而不是替换它。
- 尽量保持服务级控制语义一致（`rescan`、`reload <service>`、`restart <service>`）。

## 目录布局

### macOS（当前）

- 二进制：`/usr/local/lib/rustpm/rustpmd`
- 特权助手：`/usr/local/lib/rustpm/rustpm-privd`
- 控制 CLI：`/usr/local/lib/rustpm/rustpmctl`
- 守护配置：`/usr/local/etc/rustpm/rustpmd.conf`
- 服务配置目录：`/usr/local/etc/rustpm/programs.d`
- LaunchDaemon plist：`/Library/LaunchDaemons/club.motofans.rustpm.plist`
- 控制 socket：`/usr/local/var/run/rustpm/rustpmd.sock`
- 日志：`/usr/local/var/log/rustpm.log`、`/usr/local/var/log/rustpm.err.log`

### Windows（规划中）

目录路径与服务集成点暂未发布。

### OpenWrt（规划中）

目录路径与服务集成点暂未发布。
