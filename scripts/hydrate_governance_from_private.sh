#!/usr/bin/env bash
set -euo pipefail

SOURCE_BRANCH="${1:-private}"

FILES=(
  "AGENTS.md"
  "ACCEPTANCE.md"
  "refactory.conf"
  "chinese-version/AGENTS.md"
)

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "error: not inside a git repository" >&2
  exit 1
fi

EXCLUDE_FILE="$(git rev-parse --git-path info/exclude)"
touch "${EXCLUDE_FILE}"

add_exclude_if_missing() {
  local path="$1"
  if ! grep -Fxq "${path}" "${EXCLUDE_FILE}"; then
    echo "${path}" >>"${EXCLUDE_FILE}"
  fi
}

echo "Hydrating governance files from branch: ${SOURCE_BRANCH}"
for path in "${FILES[@]}"; do
  if git show "${SOURCE_BRANCH}:${path}" >"${path}" 2>/dev/null; then
    add_exclude_if_missing "${path}"
    echo "  restored: ${path}"
  else
    echo "  skipped (not found on ${SOURCE_BRANCH}): ${path}"
  fi
done

echo "Done. Files are restored to working tree and ignored via .git/info/exclude."
