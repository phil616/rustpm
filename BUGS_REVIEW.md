# BUGS.md 核实与修复结果

核实基于当前工作区源码（Cargo.toml 版本 0.1.0），而非清单声称的 v0.1.2。
保留原始 BUGS.md，以下区分代码缺陷、设计行为和新增功能需求。

| 编号 | 核实结果与处理 |
| --- | --- |
| 001、006 | 确认：停止控制码及状态/接受控制常量错误。改为引用 windows crate 的 SDK 常量。 |
| 002 | 确认：调度表缺少空终止项。已补齐，注册和状态更新失败会检查并记录。 |
| 003 | 确认：普通任务继承 SYSTEM，user 被忽略。普通任务现在默认 LocalService，支持显式 LocalService/NetworkService，创建账户令牌并设置 medium 完整性；失败即报错。其他账户需要凭据，目前明确拒绝。高权限任务保留 Elevated 标记，使用 privd 的身份；不能把 Administrator 字符串当作已完成身份切换，需省略 user。任意本地/域账户凭据管理仍未实现。 |
| 004 | 确认：Job 分配错误被忽略且没有句柄释放。现在暂停创建进程，检查 Job 创建、限制设置与分配，再恢复执行。Job 设置关闭即终止，所有句柄由所有权对象释放；父进程退出时也清理子树，等待 Job 活动进程归零后结束 wait。平台分组接口共享同一 Job 创建实现。 |
| 005 | 确认：按映像名全局强杀有误杀风险。删除自定义 taskkill 操作，使用已有的 ServiceControl/Wait=yes 按服务停止并等待；不会按进程名强杀，未加入超时 PID 强杀回退。 |
| 007 | 确认名称传递不一致，但不能仅凭静态代码认定必然启动失败。注册处理器现在使用实际传入的服务名。 |
| 008 | 确认过早 RUNNING。IPC 和启用的 Web 监听预先建立成功后才报告就绪，初始化错误传播；START/STOP_PENDING 定期更新 checkpoint，并避免并发停止后回到 RUNNING。 |
| 009 | 确认 MSI 未配置恢复。两个 ServiceInstall 均加入失败恢复与返回错误时恢复设置，5 秒重启，1 天重置。尚需 WiX 构建与安装后验证。 |
| 010 | 确认：POSIX 分词丢失 Windows 反斜杠。加入 Windows CRT 风格分词和对应的启动参数转义；directory 去除外围引号。覆盖空格、Unicode、空参数、尾部反斜杠和引号。 |
| 011 | 确认：强杀后等待不能称为优雅退出。当前无控制台后台程序明确使用强制停止模式，检查 Job 终止并等待整个树退出；不宣称 stop_grace_secs 在 Windows 有优雅退出效果。应用专用退出协议仍未实现。 |
| 012 | 确认：60ms 无法保证退出。改为等待旧 Actor 接收端关闭（其返回前清理进程树），配置变更全程串行，避免并发 rescan/reload 提前创建替代实例；守护进程退出也等待 Actor。 |
| 013 | 确认：启动等待阻塞控制。首次延迟、错峰等待及 startsecs 窗口均可被停止/关闭命令中断；控制通道关闭时也清理运行中的进程。 |
| 014 | 确认：autostart 被写死。加入允许字段并解析布尔值，默认 true，false 时等待手动 Start。 |
| 015 | 现有 orphan 行为是设计，并非配置删除必然应该停止。明确调整为：rescan 发现任务配置被删除时，停止、等待并注销 Actor，状态注明 unregistered。后续审查修正了把解析错误当作删除的问题：已有任务时，配置解析失败会拒绝 rescan，保留当前任务。未新增保留孤儿进程的 remove 选项。 |
| 016 | 默认 DACL 不等同于已证明任意用户可写或可提权。现在显式设置仅 SYSTEM/管理员可访问的 DACL，拒绝远程客户端，首次监听要求独占管道名；普通 CLI 用户需要提升权限。仍需不同身份的安全集成测试。 |
| 017 | 确认 CLI 没有 install。README 改用实际 MSI 构建/安装流程。 |
| 018 | 确认，不只 README：DEFAULT_LOG_DIR 也错误。代码默认值、Windows 示例和文档统一 ProgramData。 |
| 019 | 确认：默认开启且默认密码公开。Web 默认与发行配置均关闭，文档要求配置私有密码后启用；既有显式开启的配置不会自动改写。 |
| 020、021 | 属于新增功能。现有 startsecs 是存活窗口，不保证业务就绪；此次不新增 depends_on/TCP/HTTP 检查。 |
| 022 | 确认存在未实现抽象。Job 分组接口已实现；不具备持有连接/进程树所有权能力的同步 IPC 与 PID-only 启动接口明确返回错误，实际使用异步 IPC 和 ManagedChild，不再假装成功或忽略身份。 |
| 023 | 确认测试覆盖不足。加入真实 Windows 子进程/Job 测试、配置和延迟取消回归测试，以及 Windows/Ubuntu CI；完整安装、重启、升级、卸载与令牌/DACL 验收仍需测试机执行。 |

依据：Microsoft 的 [命名管道访问权限](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)、[LogonUser](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-logonuserw)，WiX 的 [ServiceConfig](https://docs.firegiant.com/wix/schema/wxs/serviceconfig/) 和 [ServiceConfigFailureActions](https://docs.firegiant.com/wix/schema/wxs/serviceconfigfailureactions/)。

验证：Windows 本机 `cargo test --all-targets --locked`，49 项通过（原有 42 项，新增 7 项）；`cargo build --bins --locked` 通过；`git diff --check` 通过；WiX XML 可以解析。
MSI 构建未执行成功：本机没有 WiX，尝试将 WiX 安装到工作区 target 下时发现只有 .NET 运行时、没有 .NET SDK。
未操作本机 SCM、安装/卸载 MSI 或改变系统账户。账户令牌和跨身份 DACL 验收尚未执行；不能将单元测试结果视为完整的 Windows 安装验收。

## 后续代码检查

再次检查配置、权限路由、控制接口和关闭路径，确认并处理了以下问题：

| 问题 | 触发与修复 |
| --- | --- |
| reload/rescan 锁顺序反转 | normal reload 持有 elevated 读锁等待配置锁，rescan 持有配置锁等待 elevated 写锁。路由只读取布尔快照，在后续 await 前释放锁。 |
| reload 错改同文件其他服务元数据 | 单服务 reload 原先重写整个文件中的日志元数据。现在只在替换目标 Actor 时初始化其元数据，保留其他运行任务的日志绑定；rescan 不再用尚未生效的新路径覆盖运行中的日志信息。 |
| 错误配置被当作删除 | 上一轮新增的注销行为会误停解析失败的任务。已有 Actor 时 rescan 使用严格解析，失败发生在任何任务变更前，保持现有 Actor。 |
| 配置入口校验不完整 | rescan 二次读取的 entries 原先只分词，不检查路径、重复服务名等完整约束。底层 entries 加载现在也执行服务校验。 |
| 退出期间仍能启动新 Actor | 控制连接任务原先脱离服务器生命周期，关闭时可能在最终 Actor 快照后新增任务。现在使用 JoinSet 跟踪并等待连接，先停止监听并等待 IPC/HTTP 请求，再关闭 Actor。 |
| 空闲连接无限占用 | IPC 单行读取和响应写入没有期限。加入 5 秒读/写超时，空闲连接不再无限阻塞关闭。 |
| 控制/Web 服务器故障后仍报告运行 | 服务器任务错误原先只在最终退出时查看。现在通过故障通道触发守护进程清理退出；通知采用非阻塞发送，防止并发错误在退出阶段阻塞。 |
| Actor 退出后无法恢复 | rescan 原先只检查映射是否存在，保留已关闭的发送端。现在检查通道是否存活，替换失效 Actor。 |
| 跨守护进程权限迁移可能短暂双开 | 单阶段 rescan 无法原子转移 primary/privd 所有权。拒绝已有服务直接改变 privilege，要求先删除配置并成功 rescan，再按新权限添加。privd rescan 失败不再返回成功，并恢复此前特权路由；整个 rescan 请求串行处理。 |
| reload 后新进程可能再次重启 | 新 Actor 的 autostart 可能先启动，随后原有 Restart 命令又将其停止。改为发送幂等 Start，避免重复启动。 |

新增回归测试覆盖锁顺序、IPC 空闲超时和消息上限、同文件服务日志隔离、错误配置与权限迁移保护。
Clippy 检查完成，仍有既有的接口参数数量、PathBuf 接口和代码风格警告；这些不等同于运行缺陷。

本轮最终验证：`cargo test --all-targets --locked`，54 项通过（新增 5 项）；二进制构建和 `git diff --check` 通过。SCM/MSI、令牌和跨身份 DACL 集成验收限制仍与上一轮相同。
