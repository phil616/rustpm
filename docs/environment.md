# Managed program environment variables

[English](environment.md) | [简体中文](environment.zh-CN.md)

Configure these fields in a `[program:name]` section in `programs.d/*.conf`.
The same rules apply on Windows and Unix, for normal services and services managed by `rustpm-privd`.

```ini
[program:api]
command = /usr/local/bin/my-api
Environment = APP_MODE=production PORT=8080 "APP_TITLE=My API"
Environment = EMPTY= 'LITERAL=$HOME'
EnvironmentFile = api.env
EnvironmentFile = -api.local.env
```

Place `api.env` beside the program configuration file:

```dotenv
# One variable per line
PORT=9090
MESSAGE="hello world"
DATA_DIR=C:\Program Files\My App\data
TOKEN=abc=def
```

The resulting `PORT` is `9090`; an existing `api.local.env` can override it again.

## Fields and precedence

| Field | Meaning |
| --- | --- |
| `environment` / `Environment` | Whitespace-separated `NAME=value` assignments with shell-style single/double quotes and escaping. Quote an entire assignment to include spaces. |
| `environment_file` / `EnvironmentFile` | Whitespace-separated paths, quoted when containing spaces. Relative paths resolve against the program configuration file's directory, independently of `directory`. Absolute paths are also supported. |

Field names are case-insensitive. Both fields may be repeated and accumulate entries in order. An empty field (`EnvironmentFile =`) clears previous entries for that field only, leaving the other field and inherited environment intact. `EMPTY=` sets an empty value, rather than removing a variable.

Precedence is **environment inherited from the spawning daemon → Environment → EnvironmentFile**. Files override inline assignments regardless of the relative positions of these field types. Later assignments and later files win. Variable names are case-insensitive on Windows and case-sensitive on Unix. Overrides affect only that child process; the daemon and other services are unchanged.

Names must match `[A-Za-z_][A-Za-z0-9_]*`; values cannot contain NUL. Without these fields, existing environment inheritance is preserved.

## File syntax

- UTF-8, with optional Windows UTF-8 BOM and CRLF. One `NAME=value` per physical line.
- Empty lines and whole-line comments beginning with `#` or `;` after leading whitespace are ignored.
- The first `=` separates name and value. Whitespace surrounding the name is removed.
- Unquoted values lose surrounding whitespace but preserve internal spaces, backslashes, `#`, `;`, and further `=` characters. Windows paths can be written literally.
- Single/double quoted values preserve interior whitespace and use shell-style quoting/escaping. Quotes are interpreted only when the whole value starts with a quote. Single quotes preserve backslashes.
- No `export NAME=...`, multiline values, continuations, wildcards, or systemd `%` specifiers. No shell execution or expansion of `$VAR`, `${VAR}`, `%VAR%`, `~`, or `$(command)`. Invalid syntax reports the file and line number.

Inline assignments and paths in the program configuration use shell-style tokenization. Prefer forward slashes or single quotes for Windows paths:

```ini
environment = 'APP_DATA=C:\Program Files\My App\data'
environment_file = "C:/ProgramData/My App/api.env" '-C:/ProgramData/My App/local.env'
```

A leading `-` ignores **missing files only**. Permission errors, directories, invalid UTF-8, and malformed content still fail. These deliberate strict rules are not a complete implementation of systemd syntax.

## When changes take effect

- Files are reread immediately before every start, manual restart, and automatic restart. After editing file contents, use `rustpmctl restart api`.
- After editing inline assignments or the file list, use `rustpmctl reload api` to reparse the program configuration and restart the service.
- `rustpmctl check-config --config <daemon-config-path>` checks current file readability and syntax.
- Daemon startup does not preread environment files, allowing them to be created later. A required file failure prevents that service from starting and follows existing failure/backoff handling.
- Reload validates environment files before replacing the running service. A validation failure leaves it running. Files are still reread and checked at spawn if changed between validation and startup.

On Unix, elevated services require root-owned environment files that are not group/other writable. Permissions on the opened file are checked on every start and are never repaired automatically. Windows uses filesystem access permissions. Files are read by the spawning daemon, before changing the child's Unix user identity.

Environment values are not added to status/Web/API responses, parse errors, or configuration Debug output. Set file read permissions for the deployment account; the child and its descendants can still access their supplied environment.
