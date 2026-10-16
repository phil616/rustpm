#!/bin/sh
set -eu

LABEL="club.motofans.rustpm"
PRIVD_LABEL="club.motofans.rustpm-privd"

YES=0
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage:
  sudo uninstall_rustpm.sh [--yes] [--dry-run]

Options:
  --yes      Skip interactive confirmation.
  --dry-run  Print actions only, do not modify system.
  -h, --help Show this help.
EOF
}

for arg in "$@"; do
  case "$arg" in
    --yes) YES=1 ;;
    --dry-run) DRY_RUN=1 ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown arg: $arg" >&2
      usage
      exit 1
      ;;
  esac
done

if [ "$(id -u)" -ne 0 ]; then
  echo "Please run as root: sudo /usr/local/lib/rustpm/uninstall_rustpm.sh" >&2
  exit 1
fi

echo "This will remove rustpm from this machine."
echo
echo "The script will attempt to remove:"
echo "  - launchd services:"
echo "      system/$LABEL"
echo "      system/$PRIVD_LABEL"
echo "  - plist files:"
echo "      /Library/LaunchDaemons/${LABEL}.plist"
echo "      /Library/LaunchDaemons/${PRIVD_LABEL}.plist"
echo "  - installation directory:"
echo "      /usr/local/lib/rustpm"
echo "  - CLI link (only if it still points to ../lib/rustpm/rustpmctl):"
echo "      /usr/local/bin/rustpmctl"
echo "  - runtime/config directories:"
echo "      /usr/local/etc/rustpm"
echo "      /usr/local/etc/rustpm/programs.d"
echo "      /usr/local/etc/rustpm/programs.d/*.conf"
echo "      /usr/local/var/run/rustpm"
echo "  - log files:"
echo "      /usr/local/var/log/rustpm*.log"
echo "      /usr/local/var/log/rustpm-privd*.log"
echo

if [ "$YES" -ne 1 ]; then
  printf "Continue uninstall? [y/N]: "
  read -r answer
  case "$answer" in
    y|Y|yes|YES) ;;
    *)
      echo "Canceled."
      exit 0
      ;;
  esac
fi

run_cmd() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '[dry-run] %s\n' "$*"
  else
    # shellcheck disable=SC2086
    sh -c "$*"
  fi
}

echo "Stopping and unloading launchd services..."
run_cmd "launchctl bootout system/$LABEL >/dev/null 2>&1 || true"
run_cmd "launchctl disable system/$LABEL >/dev/null 2>&1 || true"
run_cmd "launchctl bootout system/$PRIVD_LABEL >/dev/null 2>&1 || true"
run_cmd "launchctl disable system/$PRIVD_LABEL >/dev/null 2>&1 || true"

echo "Removing files..."
# Check at execution time as well as in dry-run mode. Preserve unrelated files,
# directories and replacement symlinks, including dangling links.
# shellcheck disable=SC2016 # Expanded by run_cmd's sh -c, not by this shell.
run_cmd 'if [ -L /usr/local/bin/rustpmctl ] && [ "$(readlink /usr/local/bin/rustpmctl)" = "../lib/rustpm/rustpmctl" ]; then rm -f /usr/local/bin/rustpmctl; fi'
run_cmd "rm -f /Library/LaunchDaemons/${LABEL}.plist"
run_cmd "rm -f /Library/LaunchDaemons/${PRIVD_LABEL}.plist"
run_cmd "rm -rf /usr/local/lib/rustpm"
run_cmd "rm -rf /usr/local/etc/rustpm"
run_cmd "rm -rf /usr/local/var/run/rustpm"
run_cmd "rm -f /usr/local/var/log/rustpm.log /usr/local/var/log/rustpm.err.log"
run_cmd "rm -f /usr/local/var/log/rustpm-privd.log /usr/local/var/log/rustpm-privd.err.log"

echo "rustpm uninstall completed."
