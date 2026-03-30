# rustpm Security and API Notes

This document summarizes the public security and API behavior for operators.

## Security model

- Web auth uses password verification with `password_hash` (`argon2id:<phc>` format).
- Plain-text `password` config is not accepted.
- `/api/*` requests require authentication.
- Session handling uses short-lived HttpOnly cookies.
- Default bind is loopback; non-local exposure requires explicit opt-in.
- Service config files under `programs.d` are managed by filesystem + sudo; Web/API does not edit these files.

## API surface

Typical endpoint groups:
- meta (`/api/meta/*`)
- auth (`/api/web/login`, `/api/web/logout`, `/api/web/password`)
- service control (`/api/services/*`)
- logs (`/api/logs/*`)

## Error contract

- API failures return non-2xx status.
- Failure payloads contain stable machine-readable `code` and readable `message`.
- Version mismatch returns structured error.
