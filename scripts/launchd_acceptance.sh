#!/usr/bin/env bash
set -euo pipefail

RUSTPM_LABEL="club.motofans.rustpm"
PRIVD_LABEL="club.motofans.rustpm-privd"
RUSTPMCTL_BIN="/usr/local/lib/rustpm/rustpmctl"
SOCKET="/usr/local/var/run/rustpm/rustpmd.sock"
DAEMON_CONF="/usr/local/etc/rustpm/rustpmd.conf"

REQUIRE_ROOT=1
TIMEOUT_SECS=60
POLL_INTERVAL=2
PING_RETRY_MAX=20
PING_RETRY_INTERVAL=1

usage() {
  cat <<'USAGE'
Usage:
  sudo ./scripts/launchd_acceptance.sh [--timeout <seconds>] [--no-root-check]

Checks after installation:
  1) launchd has loaded rustpm and rustpm-privd
  2) rustpmctl ping succeeds
  3) fault injection:
     - kill rustpmd, verify auto-recovery and ping
     - kill rustpm-privd, verify auto-recovery and ping
  4) web auth config is hash-only (password_hash present, no plain password)
USAGE
}

log() {
  printf '[launchd-acceptance] %s\n' "$*"
}

die() {
  printf '[launchd-acceptance] ERROR: %s\n' "$*" >&2
  exit 1
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --timeout)
        shift
        [[ $# -gt 0 ]] || die "--timeout needs a value"
        TIMEOUT_SECS="$1"
        ;;
      --no-root-check)
        REQUIRE_ROOT=0
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown arg: $1"
        ;;
    esac
    shift
  done
}

require_root_if_needed() {
  if [[ "$REQUIRE_ROOT" -eq 1 ]] && [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
    die "please run as root (sudo)"
  fi
}

check_binaries() {
  command -v launchctl >/dev/null 2>&1 || die "launchctl not found"
  [[ -x "$RUSTPMCTL_BIN" ]] || die "rustpmctl missing or not executable: $RUSTPMCTL_BIN"
}

check_web_auth_config() {
  [[ -f "$DAEMON_CONF" ]] || die "daemon config not found: $DAEMON_CONF"
  if ! grep -Eq '^[[:space:]]*password_hash[[:space:]]*=' "$DAEMON_CONF"; then
    die "daemon config missing web password_hash: $DAEMON_CONF"
  fi
  if ! grep -Eq '^[[:space:]]*password_hash[[:space:]]*=[[:space:]]*argon2id:' "$DAEMON_CONF"; then
    die "daemon config password_hash must use argon2id format: $DAEMON_CONF"
  fi
  if grep -Eq '^[[:space:]]*password[[:space:]]*=' "$DAEMON_CONF"; then
    die "plain web password is forbidden, found password= in: $DAEMON_CONF"
  fi
  if grep -Eq '^[[:space:]]*password_hash[[:space:]]*=[[:space:]]*sha256:' "$DAEMON_CONF"; then
    die "sha256 password hash is forbidden, found legacy format in: $DAEMON_CONF"
  fi
  if grep -Eq '^[[:space:]]*token(_hash)?[[:space:]]*=' "$DAEMON_CONF"; then
    die "legacy token naming is forbidden, found token/token_hash in: $DAEMON_CONF"
  fi
  log "web auth config check passed (hash-only)"
}

check_config_path_security() {
  local conf="/usr/local/etc/rustpm/rustpmd.conf"
  local dir="/usr/local/etc/rustpm/programs.d"
  [[ -f "$conf" ]] || die "missing daemon config: $conf"
  [[ -d "$dir" ]] || die "missing programs dir: $dir"

  local owner_conf mode_conf owner_dir mode_dir
  owner_conf="$(stat -f '%Su' "$conf")"
  mode_conf="$(normalize_mode "$(stat -f '%Mp%Lp' "$conf")")"
  owner_dir="$(stat -f '%Su' "$dir")"
  mode_dir="$(normalize_mode "$(stat -f '%Mp%Lp' "$dir")")"
  [[ "$owner_conf" = "root" ]] || die "daemon config owner must be root: $conf"
  [[ "$mode_conf" = "600" ]] || die "daemon config mode must be 600: $conf (current $mode_conf)"
  [[ "$owner_dir" = "root" ]] || die "programs.d owner must be root: $dir"
  [[ "$mode_dir" = "700" ]] || die "programs.d mode must be 700: $dir (current $mode_dir)"
  log "config path security check passed (root-only)"
}

normalize_mode() {
  local raw="$1"
  if [[ ${#raw} -ge 3 ]]; then
    echo "${raw: -3}"
    return 0
  fi
  printf '%03d\n' "${raw:-0}"
}

get_pid() {
  local label="$1"
  local pid
  pid="$(launchctl print "system/${label}" 2>/dev/null | awk -F'= ' '/^[[:space:]]*pid = /{print $2; exit}' | tr -d ';')"
  if [[ -z "$pid" ]]; then
    echo "0"
  else
    echo "$pid"
  fi
}

wait_for_label_loaded() {
  local label="$1"
  local deadline=$((SECONDS + TIMEOUT_SECS))
  while (( SECONDS < deadline )); do
    if launchctl print "system/${label}" >/dev/null 2>&1; then
      local pid
      pid="$(get_pid "$label")"
      if [[ "$pid" =~ ^[0-9]+$ ]] && (( pid > 0 )); then
        log "${label} is loaded, pid=${pid}"
        return 0
      fi
    fi
    sleep "$POLL_INTERVAL"
  done
  return 1
}

wait_for_pid_change() {
  local label="$1"
  local old_pid="$2"
  local deadline=$((SECONDS + TIMEOUT_SECS))
  while (( SECONDS < deadline )); do
    local new_pid
    new_pid="$(get_pid "$label")"
    if [[ "$new_pid" =~ ^[0-9]+$ ]] && (( new_pid > 0 )) && [[ "$new_pid" != "$old_pid" ]]; then
      log "${label} recovered: old_pid=${old_pid}, new_pid=${new_pid}"
      return 0
    fi
    sleep "$POLL_INTERVAL"
  done
  return 1
}

assert_ping() {
  local i=1
  while [ "$i" -le "$PING_RETRY_MAX" ]; do
    if "$RUSTPMCTL_BIN" --socket "$SOCKET" ping >/dev/null 2>&1; then
      log "rustpmctl ping ok"
      return 0
    fi
    if [ "$i" -eq "$PING_RETRY_MAX" ]; then
      break
    fi
    sleep "$PING_RETRY_INTERVAL"
    i=$((i + 1))
  done

  "$RUSTPMCTL_BIN" --socket "$SOCKET" ping >/dev/null
}

inject_fault_and_verify_recovery() {
  local label="$1"
  local old_pid
  old_pid="$(get_pid "$label")"
  [[ "$old_pid" =~ ^[0-9]+$ ]] && (( old_pid > 0 )) || die "${label} has no valid pid before fault injection"

  log "inject fault: kill -9 ${label} pid=${old_pid}"
  launchctl kill SIGKILL "system/${label}" >/dev/null

  wait_for_pid_change "$label" "$old_pid" || die "${label} did not recover in ${TIMEOUT_SECS}s"
  assert_ping
}

main() {
  parse_args "$@"
  require_root_if_needed
  check_binaries
  check_web_auth_config
  check_config_path_security

  log "checking launchd labels"
  wait_for_label_loaded "$RUSTPM_LABEL" || die "${RUSTPM_LABEL} not ready in ${TIMEOUT_SECS}s"
  wait_for_label_loaded "$PRIVD_LABEL" || die "${PRIVD_LABEL} not ready in ${TIMEOUT_SECS}s"

  log "checking control-plane liveness"
  assert_ping

  log "fault injection for rustpmd"
  inject_fault_and_verify_recovery "$RUSTPM_LABEL"

  log "fault injection for rustpm-privd"
  inject_fault_and_verify_recovery "$PRIVD_LABEL"

  log "all checks passed"
}

main "$@"
