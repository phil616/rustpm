# rustpm 安全与 API 说明

本文档面向运维使用者，汇总公开的安全模型与 API 行为。

## 安全模型

- Web 鉴权使用 `password_hash`（`argon2id:<phc>` 格式）。
- 不接受明文 `password` 配置。
- `/api/*` 请求必须鉴权。
- 会话使用短期 HttpOnly Cookie。
- 默认仅绑定回环地址；对外暴露需显式开启。
- `programs.d` 下服务配置仅通过文件系统 + sudo 管理，Web/API 不提供文件编辑能力。

## API 范围

典型接口分组：
- 元信息（`/api/meta/*`）
- 鉴权（`/api/web/login`、`/api/web/logout`、`/api/web/password`）
- 服务控制（`/api/services/*`）
- 日志（`/api/logs/*`）

## 错误契约

- API 失败返回非 2xx 状态码。
- 失败响应包含稳定机读 `code` 与可读 `message`。
- 版本不匹配返回结构化错误。
