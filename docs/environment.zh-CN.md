# 托管程序的环境变量

[English](environment.md) | [简体中文](environment.zh-CN.md)

在 `programs.d/*.conf` 的 `[program:名称]` 中配置，Windows 与 Unix 使用相同规则，普通服务和 `rustpm-privd` 管理的提权服务均支持。

```ini
[program:api]
command = /usr/local/bin/my-api
Environment = APP_MODE=production PORT=8080 "APP_TITLE=My API"
Environment = EMPTY= 'LITERAL=$HOME'
EnvironmentFile = api.env
EnvironmentFile = -api.local.env
```

`api.env` 与该程序配置文件放在同一个目录：

```dotenv
# 一行一个变量
PORT=9090
MESSAGE="hello world"
DATA_DIR=C:\Program Files\My App\data
TOKEN=abc=def
```

此例的 `PORT` 最终为 `9090`；`api.local.env` 存在时，其值还会覆盖 `api.env`。

## 字段与优先级

| 字段 | 作用 |
| --- | --- |
| `environment` / `Environment` | 空白分隔的 `NAME=value`，支持单引号、双引号和 shell 风格的转义；包含空格时可给整个赋值加引号。 |
| `environment_file` / `EnvironmentFile` | 空白分隔的文件路径；包含空格时加引号。支持绝对路径及相对程序配置文件目录的路径，与 `directory` 无关。 |

字段名不区分大小写。这两个字段均可重复；同类条目按书写顺序累积。空字段值（例如 `EnvironmentFile =`）清空此前该字段的条目，不影响另一字段或继承的环境。`EMPTY=` 设置空字符串，不删除变量。

覆盖顺序：**启动该程序的守护进程所继承的环境 → Environment → EnvironmentFile**。文件始终覆盖内联变量，与这两类字段在配置中的相对位置无关；同名变量最后一次赋值生效。Windows 变量名不区分大小写，Unix 区分大小写。所有覆盖只应用于该子进程，不修改守护进程或其他服务的环境。

变量名只允许 `[A-Za-z_][A-Za-z0-9_]*`；值不允许 NUL。无配置时保持原有环境继承行为。

## 环境文件格式

- UTF-8，兼容 Windows UTF-8 BOM 和 CRLF；每个物理行一个 `NAME=value`。
- 忽略空行，以及去除前导空白后以 `#`、`;` 开头的整行注释。
- 以第一个 `=` 分隔；变量名两侧的空白被去掉。
- 未加引号的值去掉首尾空白，保留中间空格、反斜杠、`#`、`;` 和后续 `=`；因此 Windows 路径可以原样填写。
- 单引号或双引号包围的值保留引号内空白，按 shell 风格解析引号和转义。值中的引号只在整个值以引号开始时进行解析。使用单引号可保留反斜杠。
- 不支持 `export NAME=...`、跨行值、续行、通配符或 systemd `%` 占位符。不执行 shell，不展开 `$VAR`、`${VAR}`、`%VAR%`、`~` 或 `$(command)`。语法错误会报出文件路径和行号。

程序配置中的内联赋值和文件路径采用 shell 风格的分词规则；Windows 路径推荐使用正斜杠或单引号：

```ini
environment = 'APP_DATA=C:\Program Files\My App\data'
environment_file = "C:/ProgramData/My App/api.env" '-C:/ProgramData/My App/local.env'
```

`-` 前缀仅忽略**文件不存在**，权限不足、目录、非 UTF-8 或内容格式错误仍会失败。这是有意选择的严格行为，不是 systemd 配置语法的完整兼容实现。

## 修改后何时生效

- 环境文件在每次启动、手动重启和自动重启前重新读取。仅修改文件内容后执行 `rustpmctl restart api` 即可生效。
- 修改内联变量或文件路径列表后执行 `rustpmctl reload api`，重新解析程序配置并重启该服务。
- `rustpmctl check-config --config <守护配置路径>` 会验证当前文件是否可读及语法是否正确。
- 启动守护进程时不预读环境文件，允许它们稍后创建；必需文件读取失败会使该服务启动失败，走现有失败/退避重试机制。
- `reload` 会先验证环境文件，验证失败时不替换正在运行的服务。校验与启动之间若文件被修改，启动时仍会重新读取并校验。

Unix 提权服务的环境文件必须属于 root，且不能被组或其他用户写入；每次启动均检查已打开文件的权限，不自动修改权限。Windows 使用文件系统访问权限。文件由启动该服务的守护进程读取，Unix 下读取发生在切换子进程用户之前。

环境值不加入状态/Web/API 响应，解析错误及配置 Debug 输出也不回显值。请根据部署账户设置环境文件的读取权限；子进程及其后代仍能读取实际传入的环境。
