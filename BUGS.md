# rustpm 项目缺陷与修复清单

审查对象： [phil616/rustpm](https://github.com/phil616/rustpm) 版本： main 分支，v0.1.2 目标： 修复 Windows 后台进程托管、开机自启、自动重启、权限隔离及配置管理方面的问题。

根据仓库源码复核，建议按照 P0（阻塞性问题）→ P1（重要功能缺陷）→ P2（功能完善与文档修正） 的顺序处理。

以下结论属于静态源码审查，尚未通过 Windows 实机集成测试。确定的代码错误与尚待验证的运行风险会分别说明。

## 一、P0：优先修复的严重缺陷

涉及系统服务可靠性与安全边界

| 编号      | 问题                       | 影响                    |
| ------- | ------------------------ | --------------------- |
| BUG-001 | Windows 服务停止控制码错误        | Windows 正常停止请求无法正确处理  |
| BUG-002 | SCM 服务调度表缺少终止项           | 可能导致服务启动失败或非法内存访问     |
| BUG-003 | Windows 普通权限未真正实现        | 普通业务进程可能以 SYSTEM 权限运行 |
| BUG-004 | Windows Job Object 管理不可靠 | 进程树清理不完整，可能遗留子进程      |
| BUG-005 | MSI 卸载使用全局强制终止           | 可能误杀其他实例及破坏优雅退出       |

### BUG-001：Windows Service 停止控制码定义错误

文件： `src/platform/windows/service.rs`

当前代码：

```
const SERVICE_CONTROL_STOP: u32 = 0x00000013;
```

正确值应为：

```
const SERVICE_CONTROL_STOP: u32 = 0x00000001;
```

该常量同时用于 SCM 控制回调和 `ControlService()`。因此错误可能导致 `Stop-Service rustpm`、`sc stop rustpm` 以及卸载时的正常停止行为失效。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



修复要求： 更正常量，并统一使用可靠的 Windows API 定义，避免重复手写 Win32 常量。

验收： Windows 服务可以通过 SCM 正常停止，业务进程收到停止指令，服务最终进入 `STOPPED`。

### BUG-002：SCM 服务调度表未正确终止

文件： `src/platform/windows/service.rs`

当前仅传入单个 `SERVICE_TABLE_ENTRY`：

```
StartServiceCtrlDispatcherW(&entry);
```

Windows API 要求服务调度表的最后一个元素必须为终止项。

修复要求： 构造包含服务项与空终止项的数组，确保数组在 API 调用期间有效；同时检查 `RegisterServiceCtrlHandlerExW` 和 `SetServiceStatus` 的返回值。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



验收： MSI 安装后，两个 Windows 服务都能通过 SCM 连续启动、停止和重新启动，没有服务启动超时或异常退出。

### BUG-003：`privilege=normal` 未实现真正的 Windows 降权

文件： `src/rustpmd_services.rs`、`src/platform/windows/process.rs`

当前主服务与特权服务都以 `LocalSystem` 启动。业务进程主要通过 `tokio::process::Command::spawn()` 创建。

Windows 分支没有按 `user` 配置切换账户身份；另一个 `spawn_as_user()` 实现也忽略了传入的 `_user` 参数。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub

+2



修复要求：

- `privilege=normal` 不应默认继承 SYSTEM 权限。
- 实现普通账户令牌创建与进程启动。
- `user` 必须真正指定运行身份。
- `privilege=elevated` 采用受控的高权限启动机制。
- 无法完成降权时必须明确报错，禁止静默以 SYSTEM 执行。

验收： 分别启动普通权限与高权限测试程序，通过进程令牌验证实际用户 SID、完整性级别和权限。

### BUG-004：Windows Job Object 进程树控制存在缺陷

文件： `src/platform/windows/process.rs`、`src/rustpmd_services.rs`、`src/rustpmd_process_group.rs`

`WindowsProcessManager` 的创建进程组、分配进程组和终止进程组接口直接返回 `not yet implemented`。

与此同时，实际业务进程启动路径已经尝试创建 Job Object，但分配结果未严格检查，终止时也忽略了部分错误。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub

+2



修复要求： 统一 Windows 进程管理实现，正确创建、绑定和释放 Job Object；检查所有 Win32 API 的返回结果。

验收： 托管进程创建多个子进程后，执行停止或重启操作，进程树中不再残留属于该任务的进程；资源句柄不会随重启次数持续泄漏。

### BUG-005：MSI 卸载逻辑存在误杀风险

文件： `wix/main.wxs`

卸载自定义操作包含：

```
taskkill.exe /F /IM rustpmd.exe
```

这是按映像名称全局强制终止，不限于当前 MSI 所安装的服务实例。

并且脚本在 `sc stop` 后直接进入强制终止流程，没有可靠地等待优雅退出。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



修复要求： 改为 SCM 定位具体服务实例、等待正常停止；仅在超时并确认身份后终止目标 PID。不得按进程映像名称无差别杀进程。

验收： 卸载 rustpm 不影响其他目录下运行的同名程序，托管进程能够完成必要的退出清理。

### BUG-006：Windows Service 状态常量还有多处错误

文件： `src/platform/windows/service.rs`

进一步检查发现，不仅 `SERVICE_CONTROL_STOP` 错误，另外三个状态常量也不正确。

| 常量                        | 当前值          | 正确值          |
| ------------------------- | ------------ | ------------ |
| `SERVICE_STOP_PENDING`    | `0x00000006` | `0x00000003` |
| `SERVICE_ACCEPT_STOP`     | `0x00000020` | `0x00000001` |
| `SERVICE_ACCEPT_SHUTDOWN` | `0x00000040` | `0x00000004` |

这些值会导致服务错误地报告其状态和可接受的控制请求。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub

+1



修复要求： 全面核对 Windows SCM 常量，优先改用 `windows` crate 中的官方 Windows API 类型和常量。

验收： `sc queryex rustpm` 正确显示状态，SCM 可以发出正常停止请求，关机通知能够被处理。

## 二、P1：重要功能与稳定性缺陷

| 编号      | 问题                      | 修复目标                   |
| ------- | ----------------------- | ---------------------- |
| BUG-007 | 服务注册名称硬编码               | 两个 Windows 服务分别使用正确的身份 |
| BUG-008 | 服务过早报告 RUNNING          | 正确反映服务就绪状态             |
| BUG-009 | SCM 故障恢复配置未完整接入 MSI     | 主服务崩溃后可自动恢复            |
| BUG-010 | Windows 命令行路径解析不可靠      | 正确支持反斜杠、空格和引号          |
| BUG-011 | Job Object 停止逻辑无法真正优雅退出 | 先通知退出，超时后强制清理          |
| BUG-012 | `reload` 存在新旧进程重叠风险     | 旧实例完全退出后再启动新实例         |
| BUG-013 | 启动延迟期间不能及时处理控制命令        | 支持即时停止和取消启动            |
| BUG-014 | `autostart` 无法单独配置      | 支持自启与手动启动两种模式          |
| BUG-015 | 删除配置后的服务管理不完整           | 明确处理孤儿进程               |
| BUG-016 | Windows IPC 安全权限没有显式定义  | 确保控制接口的访问权限正确          |

### 修复要求

BUG-007 — Windows Service 名称

`run_as_service()` 接受服务名称参数，但 `service_main()` 注册控制回调时固定使用 `SERVICE_NAME = "rustpm"`，没有使用实际传入的 `rustpm-privd` 名称。

应修复名称传递机制，并验证两个独立服务的控制句柄注册、状态更新及停止行为。对于独立进程服务，Windows 对名称校验有特殊规则，因此这属于实现不一致及兼容性风险，不应未经实测就认定它必然导致启动失败。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub

+2



BUG-008 — 服务状态报告过早

当前 `service_main()` 在执行实际守护进程初始化逻辑之前就调用：

```
set_service_status(SERVICE_RUNNING, 0);
```

应该在必要的 IPC、控制服务器等核心组件成功初始化后报告 `RUNNING`，初始化失败时报告正确的错误状态和退出码。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



BUG-009 — SCM 故障恢复

代码中存在 `configure_service_failure_actions()`，但 MSI 的服务安装定义没有明确配置故障恢复动作，而且安装 MSI 不会执行源码中的 `install_service()`。

应把故障恢复配置纳入实际安装流程，并同时处理进程异常终止以及服务主动报告失败的不同情况。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub

+1



BUG-010 — Windows 路径解析

`src/config.rs` 使用 `shlex::split()` 解析 `command`。这是偏 POSIX 的分词规则，不适合直接解析所有 Windows 命令行格式。

同时，`directory` 使用 `PathBuf::from()` 直接读取字符串，没有移除配置示例建议使用的外围引号。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



应实现 Windows 专用参数解析，并至少测试：

```
command = "C:\Program Files\App\app.exe" --config "D:\My Data\config.yaml"
directory = "C:\Program Files\App"
```

路径、参数、反斜杠和 Unicode 字符均应得到正确处理。

BUG-011 — 优雅停止无效

当前 Windows 实现一进入 `terminate_process_group_impl()`，就先尝试 `TerminateJobObject()`，然后才等待 `stop_grace_secs`。

这意味着成功执行时已经强制结束整个 Job，而不是先让进程自行退出。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



建议把停止逻辑改为：发出适合该程序的正常停止请求 → 等待退出 → 超时后通过 Job Object 强制终止。对无法接收控制台事件的后台进程，可以明确支持强制停止模式。

BUG-012 — 配置重载存在竞争条件

`src/rustpmd_services_sync.rs` 在发送旧 Actor 的 `Shutdown` 后仅等待固定 60ms，就创建新的 Actor。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



这无法保证旧业务进程已经完全退出。

应改为等待旧 Actor 的实际终止确认，并在确认进程树已释放后再创建新实例。

BUG-013 — 启动延迟阻塞控制

`src/rustpmd_service_actor.rs` 在首次启动时直接执行 `sleep()`。

在默认 30 秒启动延迟期间，Actor 不能立即响应队列中的停止或关闭命令。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



建议改为可取消的异步等待，使 `Stop`、`Shutdown` 能中断启动过程。

BUG-014 — 自动启动配置不完整

`ServiceConfig` 已经有 `autostart: bool`，但是 INI 允许字段中没有 `autostart`，并且解析时写死 `autostart: true`。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub

+1



应正式支持：

```
autostart = true
```

以及：

```
autostart = false
```

`autostart=false` 的程序在 rustpm 启动时保持停止状态，但允许通过 CLI 或 Web 手动启动。

BUG-015 — 删除配置后进程仍可能继续运行

目前 `rescan` 发现配置文件被删除后，会把服务标记为 `orphaned`，但不会停止原有进程，也不会完整移除 Actor。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



这属于现有设计行为，但缺少完整的服务移除机制。建议明确支持 `remove` 或 `unregister` 操作，并允许管理员决定是否同时停止进程。

BUG-016 — Windows Named Pipe 权限需要验证

`src/rustpmd_control_socket.rs` 使用默认 `ServerOptions` 创建 Named Pipe，没有显式指定 DACL，也没有在 IPC 层实施调用方身份授权。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub



由于 `rustpm-privd` 具备高权限，这一接口必须明确限制哪些用户可以发送启动、停止、重启等管理指令。

这属于需要安全测试的风险，而不是已证明可利用的提权漏洞。如果普通非授权用户能够操作特权服务，应立即提升为 P0。

## 三、P2：配置、文档及功能完善

| 编号      | 问题                                         | 建议处理                               |
| ------- | ------------------------------------------ | ---------------------------------- |
| BUG-017 | README 使用 `rustpmd.exe install`，实际 CLI 不支持 | 实现子命令或删除错误示例                       |
| BUG-018 | README 和默认示例的日志目录错误                        | 全部统一为 `C:\ProgramData\rustpm\logs` |
| BUG-019 | Web 控制台默认开启且使用公开默认密码                       | 默认关闭 Web，或首次使用强制配置密码               |
| BUG-020 | 不具备真正的业务进程依赖管理                             | 增加 `depends_on` 和就绪检查              |
| BUG-021 | `startsecs` 只能判断存活                         | 增加 TCP/HTTP 健康检查机制                 |
| BUG-022 | Windows 平台抽象中保留未实现的 IPC、Process Stub       | 完成实现或移除无效接口                        |
| BUG-023 | 缺少充分的 Windows 生命周期测试                       | 建立安装、启动、崩溃、重载、升级、卸载测试              |

其中值得特别强调：

BUG-018：日志目录不一致。 `src/rustpmd.rs` 和日志安全策略使用 `C:\ProgramData\rustpm\logs`，而 Windows README 和默认示例仍然使用 `C:\Program Files\rustpm\logs`。这不仅是文档错误，还可能导致业务程序无法启动，因为日志路径会受到安全策略限制。

[image](https://www.google.com/s2/favicons?domain=https://raw.githubusercontent.com\&sz=32)

GitHub

+2



BUG-020 和 BUG-021 属于新增功能需求，而不是已有功能失效。 当前启动间隔只能错开启动时间，`startsecs` 仅表示进程存活足够长的时间。它们不保证数据库、API 等业务服务实际就绪。

## 四、建议的修复顺序

第一组：恢复 Windows SCM 的正确行为

BUG-001、002、006、007、008、009

目标：安装后两个服务能够正常启动、停止和恢复。

第二组：修复进程权限与生命周期

BUG-003、004、005、011、012、016

目标：权限正确、进程树可控，重启和卸载不遗留进程。

第三组：修复实际配置和管理体验

BUG-010、013、014、015、017、018、019

目标：配置文件可靠，手动与自动启动行为明确。

第四组：补充功能和测试

BUG-020、021、022、023

目标：提高依赖服务管理能力和发布质量。

## 五、修复完成后的验收清单

建议在一台 Windows 11 或 Windows Server 测试机上执行完整验收。

Windows 集成测试

0/15 通过

MSI 能正常安装，注册 rustpm 和 rustpm-privd 两个自动启动服务

两个服务能够通过 sc start / sc stop 正常控制

Windows 重启且无人登录时，业务程序正常自动运行

autostart=false 的业务程序不会随系统启动

普通权限进程不会继承 SYSTEM 身份

配置的 Windows 用户身份与实际进程令牌一致

应用异常退出后，rustpm 能正确重新启动它

停止含有子进程的程序时，不会留下孤儿进程

reload 能等待旧实例完全结束后再启动新实例

Windows 路径中包含空格、反斜杠和中文时能够正常启动

日志能够正确写入 ProgramData 并按配置轮转

没有权限的本地用户不能控制特权进程

rustpm 主服务异常终止后能够按 SCM 恢复策略重新启动

升级 MSI 后，原有业务配置得到保留

卸载不会误杀其他同名程序，托管进程和文件的清理行为符合预期

重置 复制测试清单

最终建议： 先完成 Windows SCM 常量与状态管理的修复，再处理权限隔离和进程树问题。尤其不要只修正 `SERVICE_CONTROL_STOP`，而忽略 `SERVICE_ACCEPT_STOP` 和 `SERVICE_STOP_PENDING`，因为三者都会影响系统服务停止行为。完成前两组修复并通过实机测试后，再考虑将 rustpm 用于需要长期稳定运行的客户环境。