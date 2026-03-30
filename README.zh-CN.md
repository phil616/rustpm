# rustpm

[English](README.md) | [简体中文](README.zh-CN.md)

默认文档语言为英文；以下为简体中文对照版，命令与配置键保持不变。

`rustpm` 是一个强调可预测运维、明确权限边界、并兼顾日常效率的进程管理器。

本文档面向用户：按平台说明安装、配置、运行管理、诊断与卸载。

## 为什么开发 rustpm

开发 `rustpm` 的动机来自一个实际问题：`launchctl` 很强大，但日常服务管理使用门槛较高。

`rustpm` 在保持 Apple 原生引导方式的同时，提供了更适合日常管理的服务编排流程。

## 项目目标

- [x] 在 macOS 上提供基于 `launchd` 引导 + `rustpm` 编排的稳定运行时。
- [x] 提供服务粒度控制面（`rustpmctl` + Web 控制台）并落实权限边界。
- [x] 配置模型参考 `supervisord` 风格并保持实用兼容性，降低迁移学习成本。
- [ ] 支持 Windows 平台（设计目标）。
- [ ] 支持 OpenWrt 平台（设计目标）。

## 公开文档

- 架构与目录布局：[docs/architecture.zh-CN.md](docs/architecture.zh-CN.md)
- 安全模型与 API 说明：[docs/security-api.zh-CN.md](docs/security-api.zh-CN.md)

## 安装

### macOS（已支持）

方案 A：从源码构建未签名 pkg（自用）

源码构建需要 Rust 工具链。
建议从以下官方地址选择安装方式：
- <https://forge.rust-lang.org/infra/other-installation-methods.html>

构建：

```bash
chmod +x scripts/build_pkg.sh packaging/postinstall
./scripts/build_pkg.sh
```

产物：
- `dist/rustpm-0.1.0-unsigned.pkg`

方案 B：安装预构建 pkg

如果你已经拿到预构建 pkg（例如来自 CI 产物或其他机器构建结果），本机不需要 Rust 环境。

安装命令：

```bash
sudo installer -pkg dist/rustpm-0.1.0-unsigned.pkg -target /
```

安装默认行为：
- `/usr/local/etc/rustpm/programs.d/example.conf` 以注释模板形式安装。
- 默认不启用任何真实业务服务。
- 安全默认值：
  - `/usr/local/etc/rustpm/rustpmd.conf` 权限 `600`（仅 root）
  - `/usr/local/etc/rustpm/programs.d` 权限 `700`（仅 root）
  - `programs.d/*.conf` 权限 `600`（仅 root）

### Windows（规划中）

安装流程暂未发布。

### OpenWrt（规划中）

安装流程暂未发布。

## 配置

### macOS（已支持）

`rustpm` 的配置语法参考了 `supervisord` 约定，并保留了实用兼容性，以降低学习与迁移成本。

也正因为这一点，当前没有计划专门支持那些已经提供 `supervisord` 包的主流发行版场景。

服务配置格式（`programs.d/*.conf`）：

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

支持键：
- `directory`
- `command`
- `user`：
  - `root` 或 `Administrator` 视为 `privilege=elevated`
  - 当 `privilege=normal` 时，可指定本机非 root 用户；在 Unix 上 rustpm 会按该用户身份拉起服务
- `privilege`（`normal` 或 `elevated`）
- `startsecs`
- `autorestart`
- `redirect_stderr`
- `stdout_logfile_maxbytes`
- `stdout_logfile_backups`
- `stdout_logfile`
- `stderr_logfile`

守护配置格式（`rustpmd.conf`）：

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

Web 默认开启，默认密码为 `password`（生产环境建议立即修改）。
详细安全与 API 行为请查看：[docs/security-api.zh-CN.md](docs/security-api.zh-CN.md)。

### Windows（规划中）

配置格式与文件位置暂未发布。

### OpenWrt（规划中）

配置格式与文件位置暂未发布。

## 运行管理

### macOS（已支持）

运行命令：

```bash
sudo /usr/local/lib/rustpm/rustpmd check-config --config /usr/local/etc/rustpm/rustpmd.conf --config-dir /usr/local/etc/rustpm/programs.d
sudo /usr/local/lib/rustpm/rustpmd run --validate-config --config /usr/local/etc/rustpm/rustpmd.conf --config-dir /usr/local/etc/rustpm/programs.d
sudo launchctl print system/club.motofans.rustpm
sudo launchctl kickstart -k system/club.motofans.rustpm
sudo launchctl bootout system/club.motofans.rustpm
```

使用 `rustpmctl` 管理服务：

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

说明：
- `rescan`：重新扫描 `programs.d` 发现新增服务；不会自动重启已有服务。
- `reload <service>`：仅重读该服务对应来源文件，做冲突检查后只重载该服务。
- 不提供全局 `reload`；全局发现请使用 `rescan`。

新增受管服务：

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

### Windows（规划中）

运行管理命令集暂未发布。

### OpenWrt（规划中）

运行管理命令集暂未发布。

## 诊断

### macOS（已支持）

若 `postinstall` 阶段 launchd 启动失败，pkg 安装仍会完成。
此时请手动启动服务：

```bash
sudo launchctl bootstrap system /Library/LaunchDaemons/club.motofans.rustpm-privd.plist
sudo launchctl kickstart -k system/club.motofans.rustpm-privd
sudo launchctl bootstrap system /Library/LaunchDaemons/club.motofans.rustpm.plist
sudo launchctl kickstart -k system/club.motofans.rustpm
```

服务/运行日志：

```bash
sudo tail -n 200 /usr/local/var/log/easytier.log
sudo tail -n 200 /usr/local/var/log/easytier-error.log
sudo tail -n 200 /usr/local/var/log/rustpm.err.log
sudo tail -n 200 /usr/local/var/log/rustpm-privd.err.log
```

### Windows（规划中）

诊断流程暂未发布。

### OpenWrt（规划中）

诊断流程暂未发布。

## 卸载

### macOS（已支持）

推荐（交互式，带删除摘要确认）：

```bash
sudo /usr/local/lib/rustpm/uninstall_rustpm.sh
```

仅预览（dry-run）：

```bash
sudo /usr/local/lib/rustpm/uninstall_rustpm.sh --dry-run
```

非交互执行：

```bash
sudo /usr/local/lib/rustpm/uninstall_rustpm.sh --yes
```

卸载脚本会删除：
- launchd 服务：`club.motofans.rustpm`、`club.motofans.rustpm-privd`
- `/Library/LaunchDaemons` 下相关 plist 文件
- `/usr/local/lib/rustpm`
- `/usr/local/etc/rustpm`
- `/usr/local/etc/rustpm/programs.d`
- `/usr/local/etc/rustpm/programs.d/*.conf`
- `/usr/local/var/run/rustpm`
- `/usr/local/var/log` 下 rustpm 相关日志文件

### Windows（规划中）

卸载流程暂未发布。

### OpenWrt（规划中）

卸载流程暂未发布。
