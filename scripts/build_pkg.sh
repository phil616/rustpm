#!/bin/sh
set -eu

# Build unsigned pkg for self-hosted/internal usage.
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT_DIR/dist"
STAGE_DIR="$ROOT_DIR/target/pkg-root"
SCRIPTS_DIR="$ROOT_DIR/packaging"
BUILD_SCRIPT="$ROOT_DIR/scripts/cargo-local.sh"
VERSION="${VERSION:-0.1.0}"
ID="club.motofans.rustpm"
PKG_PATH="$OUT_DIR/rustpm-${VERSION}-unsigned.pkg"

# Always rebuild release binaries before packaging to avoid stale artifacts.
"$BUILD_SCRIPT" build --release --quiet

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/usr/local/lib/rustpm"
mkdir -p "$STAGE_DIR/usr/local/etc/rustpm/programs.d"
mkdir -p "$STAGE_DIR/Library/LaunchDaemons"
mkdir -p "$OUT_DIR"

cp "$ROOT_DIR/target/release/rustpmd" "$STAGE_DIR/usr/local/lib/rustpm/rustpmd"
cp "$ROOT_DIR/target/release/rustpm-privd" "$STAGE_DIR/usr/local/lib/rustpm/rustpm-privd"
cp "$ROOT_DIR/target/release/rustpmctl" "$STAGE_DIR/usr/local/lib/rustpm/rustpmctl"
cp "$ROOT_DIR/scripts/uninstall_rustpm.sh" "$STAGE_DIR/usr/local/lib/rustpm/uninstall_rustpm.sh"
cp "$ROOT_DIR/config/rustpmd.example.conf" "$STAGE_DIR/usr/local/lib/rustpm/rustpmd.example.conf"
cp "$ROOT_DIR/config/programs.d/example.conf" "$STAGE_DIR/usr/local/lib/rustpm/example.conf"
cp "$ROOT_DIR/packaging/club.motofans.rustpm.plist" "$STAGE_DIR/Library/LaunchDaemons/club.motofans.rustpm.plist"
cp "$ROOT_DIR/packaging/club.motofans.rustpm-privd.plist" "$STAGE_DIR/Library/LaunchDaemons/club.motofans.rustpm-privd.plist"
chmod 755 "$STAGE_DIR/usr/local/lib/rustpm/uninstall_rustpm.sh"

pkgbuild \
  --root "$STAGE_DIR" \
  --identifier "$ID" \
  --version "$VERSION" \
  --scripts "$SCRIPTS_DIR" \
  "$PKG_PATH"

echo "Created: $PKG_PATH"
