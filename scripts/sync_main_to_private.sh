#!/usr/bin/env bash
set -euo pipefail

MAIN_BRANCH="${1:-main}"
PRIVATE_BRANCH="${2:-private}"
REMOTE="${3:-origin}"

GOV_FILES=(
  "AGENTS.md"
  "ACCEPTANCE.md"
  "refactory.conf"
  "chinese-version/AGENTS.md"
)

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "error: not inside a git repository" >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "error: tracked working tree is not clean. Commit or stash first." >&2
  exit 1
fi

START_BRANCH="$(git rev-parse --abbrev-ref HEAD)"

restore_start_branch() {
  git switch "${START_BRANCH}" >/dev/null 2>&1 || true
}
trap restore_start_branch EXIT

git fetch "${REMOTE}" "${MAIN_BRANCH}" "${PRIVATE_BRANCH}" >/dev/null

if ! git show-ref --verify --quiet "refs/heads/${MAIN_BRANCH}"; then
  git branch --track "${MAIN_BRANCH}" "${REMOTE}/${MAIN_BRANCH}" >/dev/null
fi
if ! git show-ref --verify --quiet "refs/heads/${PRIVATE_BRANCH}"; then
  git branch --track "${PRIVATE_BRANCH}" "${REMOTE}/${PRIVATE_BRANCH}" >/dev/null
fi

git switch "${MAIN_BRANCH}" >/dev/null
git pull --ff-only "${REMOTE}" "${MAIN_BRANCH}" >/dev/null
MAIN_HEAD="$(git rev-parse --verify "${MAIN_BRANCH}")"

git switch "${PRIVATE_BRANCH}" >/dev/null
git pull --ff-only "${REMOTE}" "${PRIVATE_BRANCH}" >/dev/null
PRIVATE_BASE="$(git rev-parse --verify "${PRIVATE_BRANCH}")"

echo "Syncing tracked files from ${MAIN_BRANCH} into ${PRIVATE_BRANCH}..."
git checkout "${MAIN_BRANCH}" -- .

while IFS= read -r path; do
  [[ -n "${path}" ]] || continue
  if ! git cat-file -e "${MAIN_BRANCH}:${path}" 2>/dev/null; then
    rm -f "${path}"
    git rm --cached --ignore-unmatch -- "${path}" >/dev/null 2>&1 || true
  fi
done < <(git ls-tree -r --name-only "${PRIVATE_BASE}")

for file in "${GOV_FILES[@]}"; do
  if git cat-file -e "${PRIVATE_BASE}:${file}" 2>/dev/null; then
    git show "${PRIVATE_BASE}:${file}" >"${file}"
    git add -f -- "${file}"
  else
    rm -f "${file}"
    git rm --cached --ignore-unmatch -- "${file}" >/dev/null 2>&1 || true
  fi
done

if ! git diff --quiet || ! git diff --cached --quiet; then
  git add -A
  git commit -m "chore: sync ${MAIN_BRANCH} to ${PRIVATE_BRANCH} (${MAIN_HEAD:0:7})"
else
  echo "No changes to commit after sync."
fi

git push "${REMOTE}" "${PRIVATE_BRANCH}"
echo "Sync complete: ${MAIN_BRANCH} -> ${PRIVATE_BRANCH}"
