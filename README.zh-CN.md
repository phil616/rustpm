# rustpm

[English](README.md) | [简体中文](README.zh-CN.md)

默认文档语言为英文；以下为简体中文对照版，命令与配置键保持不变。

`rustpm` 是一个强调可预测运维、明确权限边界、并兼顾日常效率的进程管理器。

本文档面向用户：按平台说明安装、配置、运行管理、诊断与卸载。

## 为什么开发 rustpm

`rustpm` 在跨平台范围内提供一致的服务管理工作流，同时对接各操作系统的原生服务引导机制。

## 项目目标

- [x] 跨平台运行时：macOS（`launchd` 引导）+ Windows（SCM 服务）。
- [x] 提供服务粒度控制面（`rustpmctl` + Web 控制台）并落实权限边界。
- [x] 配置模型参考 `supervisord` 风格并保持实用兼容性，降低迁移学习成本。
- [x] Windows 服务支持（基于 SCM 集成）。
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
chmod +x scripts/build_pkg.sh packaging/preinstall packaging/postinstall
./scripts/build_pkg.sh
```

产物：
- `dist/rustpm-0.1.5-unsigned.pkg`

方案 B：安装预构建 pkg

如果你已经拿到预构建 pkg（例如来自 CI 产物或其他机器构建结果），本机不需要 Rust 环境。

安装命令：

```bash
sudo installer -pkg dist/rustpm-0.1.5-unsigned.pkg -target /
```

安装默认行为：
- `/usr/local/etc/rustpm/programs.d/example.conf` 以注释模板形式安装。
- 默认不启用任何真实业务服务。
- `/usr/local/bin/rustpmctl` 软链接指向 `../lib/rustpm/rustpmctl`；若已有其他同名入口，安装会终止，避免覆盖。卸载脚本仅删除目标匹配的软链接。
- 安全默认值：
  - `/usr/local/etc/rustpm/rustpmd.conf` 权限 `600`（仅 root）
  - `/usr/local/etc/rustpm/programs.d` 权限 `700`（仅 root）
  - `programs.d/*.conf` 权限 `600`（仅 root）

### Windows（已支持）

rustpm 作为 Windows Service 运行，由 Windows Service Control Manager 管理。

前置条件：
- Windows 10/11 或 Windows Server 2016+
- 管理员权限（服务安装需要）

MSI 安装（推荐）：

构建需要 WiX Toolset v5；构建脚本会自动下载匹配版本的 Util 扩展（首次构建需要联网）。服务故障恢复通过该扩展配置，绕过 Windows Installer 原生故障恢复配置的已知问题。

```powershell
# 构建 MSI（需要 WiX Toolset）
.\scripts\build_msi.ps1

# 双击 dist\rustpm-0.1.5-x64.msi 即可安装
# Windows 会自动弹出 UAC 提权窗口。

# 或命令行（必须在管理员 PowerShell 中运行）：
msiexec /i dist\rustpm-0.1.5-x64.msi

# 静默安装（必须在管理员 PowerShell 中运行）：
msiexec /i dist\rustpm-0.1.5-x64.msi /quiet

# 卸载：
msiexec /x dist\rustpm-0.1.5-x64.msi
```

MSI 将专用 `cli` 目录（默认 `C:\Program Files\rustpm\cli`）追加到系统 PATH。该目录包含安装包提供的 `rustpmctl.exe` 副本，原有 CLI 和守护进程位置保留。MSI 在修复、升级和卸载时管理此 PATH 项。手工配置 PATH 时请使用原有安装目录，避免使用安装包专用的 `cli` 目录：卸载时 MSI 无法区分手工添加的相同项。

安装后关闭并重新打开 PowerShell/cmd。必要时完全退出并重启 Windows Terminal；新标签页仍可能继承父进程的旧环境。

从源码构建（手动注册服务）：

```powershell
# 克隆并构建
cargo build --release

# 安装为 Windows Service
.\scripts\build_msi.ps1
# 安装生成的 MSI，以注册两个 Windows 服务。
```

服务管理：

```powershell
# 启动服务
Start-Service -Name rustpm

# 查看状态
Get-Service -Name rustpm

# 停止服务
Stop-Service -Name rustpm

# 卸载请通过控制面板（程序和功能）
# 或运行 MSI 卸载命令：
# msiexec /x C:\path\to\rustpm-<version>-x64.msi
```

### 验证 CLI 命令发现

在新终端的任意目录运行 `rustpmctl --help`，再使用所需权限运行 `rustpmctl list`（Windows 使用管理员终端，macOS 使用 `sudo rustpmctl list`）。PATH 仅负责查找命令，不会授予服务控制权限或绕过命名管道/socket 的访问检查。

Windows 排查：

```powershell
Get-Command rustpmctl -All
where.exe rustpmctl
[Environment]::GetEnvironmentVariable("Path", "Machine")
# 找不到命令或 PATH 前面存在其他同名命令时：
& "C:\Program Files\rustpm\rustpmctl.exe" --help
```

macOS 排查：

```bash
command -v rustpmctl
ls -l /usr/local/bin/rustpmctl
echo "$PATH"
/usr/local/lib/rustpm/rustpmctl --help
```

自定义 shell 或 `sudo` 环境可能不包含 `/usr/local/bin`，此时使用完整路径。pkg 不修改 shell 配置文件。

### GitHub Releases（Windows amd64）

[发布工作流](.github/workflows/release.yml) 只构建 Windows amd64 MSI，
并上传到对应的 GitHub Release。`v0.1.5` 的产物为 `rustpm-0.1.5-x64.msi`。

提交并推送工作流及打包修改后，推送版本标签即可触发：

```bash
git tag v0.1.5
git push origin v0.1.5
```

CI 在临时检出目录中使用标签统一 Cargo 包版本、MSI 内部版本和注册表版本，依赖版本保持锁定。
如需重跑已有标签，可打开 **Actions → Release Windows MSI → Run workflow**，
默认标签为 `v0.1.5`，选择 `main` 分支可用最新工作流修复重跑已有标签。
标签指向的提交必须包含上述打包修改。
无需额外配置仓库 Secret，工作流使用具备 `contents: write` 权限的 `GITHUB_TOKEN` 发布。

### OpenWrt（规划中）

安装流程暂未发布。

## 配置

托管程序环境变量：[Environment 与 EnvironmentFile 使用说明](docs/environment.zh-CN.md)。

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

如果 macOS 的 `directory` 或 `command` 某一段包含空格，必须给该段加引号，例如：

```ini
directory = "/Users/alice/My Service"
command = "/Users/alice/My Service/bin/example" -c "/Users/alice/My Service/config/example.config"
```

支持键：
- `directory`
- `command`
- `environment` / `Environment`
- `environment_file` / `EnvironmentFile`
- `user`：
  - `root` 或 `Administrator` 视为 `privilege=elevated`
  - 当 `privilege=normal` 时，可指定本机非 root 用户；在 Unix 上 rustpm 会按该用户身份拉起服务
- `privilege`（`normal` 或 `elevated`）
- `autostart`（默认 true；false 时等待手动启动）
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
enabled = false
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

Web 默认关闭。启用前请配置私有密码哈希。
详细安全与 API 行为请查看：[docs/security-api.zh-CN.md](docs/security-api.zh-CN.md)。

### Windows（已支持）

`rustpm` 配置语法是平台无关的；相同的 INI 格式在 macOS 和 Windows 上均可工作。

默认位置：
- 守护配置：`C:\Program Files\rustpm\config\rustpmd.conf`
- 服务配置：`C:\Program Files\rustpm\config\programs.d\`
- 日志目录：`C:\ProgramData\rustpm\logs\`
- IPC 管道：`\\.\pipe\rustpmd`
- Privd 管道：`\\.\pipe\rustpm-privd`

服务配置格式（`programs.d\*.conf`）：

```ini
[program:example]
directory = C:\Users\admin\example
command = C:\Users\admin\example\bin\example.exe -c C:\Users\admin\example\config.ini
# elevated 任务使用 privd 服务身份，不设置 user。
privilege = elevated
startsecs = 3
autorestart = true
redirect_stderr = true
stdout_logfile_maxbytes = 50MB
stdout_logfile_backups = 10
stdout_logfile = C:\ProgramData\rustpm\logs\%(program_name)s.log
stderr_logfile = C:\ProgramData\rustpm\logs\%(program_name)s-error.log
```

如果 Windows 的命令路径或参数里包含空格（例如 `C:\Program Files\...`），
必须给该段加引号，例如：

```ini
directory = "C:/Program Files/My Service"
command = "C:/Program Files/My Service/myservice.exe" --config "C:/Program Files/My Service/config.ini"
```

支持的键（与 macOS 相同）：
- `directory`
- `command`
- `environment` / `Environment`
- `environment_file` / `EnvironmentFile`
- `user`：
  - 普通任务默认使用 LocalService（medium 完整性），也支持 NetworkService。
  - 任意账户登录需要凭据，目前明确拒绝；elevated 任务应省略 user。
- `privilege`（`normal` 或 `elevated`）
- `autostart`（默认 true；false 时等待手动启动）
- `startsecs`
- `autorestart`
- `redirect_stderr`
- `stdout_logfile_maxbytes`
- `stdout_logfile_backups`
- `stdout_logfile`
- `stderr_logfile`

守护配置格式（`rustpmd.conf`）：

```ini
config_dir = C:\Program Files\rustpm\config\programs.d
socket = \\.\pipe\rustpmd
privd_socket = \\.\pipe\rustpm-privd

[web]
enabled = false
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

默认密码为 `password`（生产环境建议立即修改）。

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

### Windows（已支持）

使用 `rustpmctl` 进行服务控制：

```powershell
# 列出所有服务
rustpmctl.exe list

# 重新扫描发现新服务
rustpmctl.exe rescan

# 显示响应码
rustpmctl.exe codes

# 显示运行契约
rustpmctl.exe contract

# JSON 输出
rustpmctl.exe --json list

# 配置校验
rustpmctl.exe check-config --config "C:\Program Files\rustpm\config\rustpmd.conf"

# ping 守护进程
rustpmctl.exe ping

# 服务操作
rustpmctl.exe status myservice
rustpmctl.exe start myservice
rustpmctl.exe stop myservice
rustpmctl.exe restart myservice
rustpmctl.exe reload myservice

# 带配置校验的服务操作
rustpmctl.exe restart myservice --check-config --config "C:\Program Files\rustpm\config\rustpmd.conf"
```

注意：
- `rescan`：重新扫描 `programs.d` 发现新增服务。
- `reload <service>`：仅重载该服务所属的配置文件。
- `rustpmctl` 通过命名管道（`\\.\pipe\rustpmd`）与守护进程通信。

创建托管服务：

```powershell
# 创建服务配置文件
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
stdout_logfile = C:\ProgramData\rustpm\logs\myservice.log
stderr_logfile = C:\ProgramData\rustpm\logs\myservice-error.log
"@

# 重启服务以加载新配置
rustpmctl.exe rescan
rustpmctl.exe restart myservice --check-config --config "C:\Program Files\rustpm\config\rustpmd.conf"
```

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

### Windows（已支持）

查看服务状态：

```powershell
# 服务状态
Get-Service -Name rustpm

# 详细服务信息
sc query rustpm

# 事件日志
Get-EventLog -LogName Application -Source rustpm -Newest 50
```

服务/运行日志：

```powershell
# 查看最近日志条目
Get-Content -Tail 200 "C:\ProgramData\rustpm\logs\myservice.log"
Get-Content -Tail 200 "C:\ProgramData\rustpm\logs\myservice-error.log"
Get-Content -Tail 200 "C:\ProgramData\rustpm\logs\rustpm.log"
Get-Content -Tail 200 "C:\ProgramData\rustpm\logs\rustpm-privd.log"
```

调试模式（前台运行，用于故障排除）：

```powershell
# 前台运行（不作为服务）
rustpmd.exe run --config "C:\Program Files\rustpm\config\rustpmd.conf"
```

服务恢复：
- Windows Service Control Manager 会在故障时自动重启 rustpm。
- 可通过 `sc failure` 命令配置恢复操作。

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

### Windows（已支持）

打开 **控制面板 → 程序和功能**，找到 **rustpm**，点击**卸载**。

如果有 MSI 文件，也可运行：

```powershell
msiexec /x C:\path\to\rustpm-<version>-x64.msi
```

这将：
- 停止并删除 Windows Service
- 不删除配置文件或日志（如需请手动清理）

手动清理（如需要）：

```powershell
# 删除配置目录
Remove-Item -Recurse -Force "C:\Program Files\rustpm\config"

# 删除日志目录
Remove-Item -Recurse -Force "C:\ProgramData\rustpm\logs"

# 删除安装目录
Remove-Item -Recurse -Force "C:\Program Files\rustpm"
```

### OpenWrt（规划中）

卸载流程暂未发布。

Windows 后台任务目前使用 Job Object 强制停止整个进程树，stop_grace_secs 不提供优雅退出。配置解析失败时 rescan 会保留已有任务。修改 privilege 需先删除配置并成功 rescan，再以新权限添加。Windows CLI 控制需管理员权限。
