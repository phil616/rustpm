#!/bin/sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

. "$SCRIPT_DIR/dev-shell.sh" >/dev/null

if [ "${1:-}" = "install" ]; then
  echo "error: global or local 'cargo install' is forbidden in this project policy" >&2
  exit 1
fi

exec cargo "$@"
