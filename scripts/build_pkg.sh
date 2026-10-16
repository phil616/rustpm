#!/bin/sh
set -eu

# Build unsigned pkg for self-hosted/internal usage.
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT_DIR/dist"
STAGE_DIR="$ROOT_DIR/target/pkg-root"
SCRIPTS_DIR="$ROOT_DIR/packaging"
BUILD_SCRIPT="$ROOT_DIR/scripts/cargo-local.sh"
PACKAGE_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)"
VERSION="${VERSION:-$PACKAGE_VERSION}"
if [ -z "$PACKAGE_VERSION" ] || [ "$VERSION" != "$PACKAGE_VERSION" ]; then
  echo "Package version must match Cargo.toml ($PACKAGE_VERSION)" >&2
  exit 1
fi
ID="club.motofans.rustpm"
PKG_PATH="$OUT_DIR/rustpm-${VERSION}-unsigned.pkg"

# Always rebuild release binaries before packaging to avoid stale artifacts.
"$BUILD_SCRIPT" build --release --quiet

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/usr/local/lib/rustpm"
mkdir -p "$STAGE_DIR/usr/local/bin"
mkdir -p "$STAGE_DIR/usr/local/etc/rustpm/programs.d"
mkdir -p "$STAGE_DIR/Library/LaunchDaemons"
mkdir -p "$OUT_DIR"

cp "$ROOT_DIR/target/release/rustpmd" "$STAGE_DIR/usr/local/lib/rustpm/rustpmd"
cp "$ROOT_DIR/target/release/rustpm-privd" "$STAGE_DIR/usr/local/lib/rustpm/rustpm-privd"
cp "$ROOT_DIR/target/release/rustpmctl" "$STAGE_DIR/usr/local/lib/rustpm/rustpmctl"
ln -s ../lib/rustpm/rustpmctl "$STAGE_DIR/usr/local/bin/rustpmctl"
cp "$ROOT_DIR/scripts/uninstall_rustpm.sh" "$STAGE_DIR/usr/local/lib/rustpm/uninstall_rustpm.sh"
cp "$ROOT_DIR/config/macos/rustpmd.conf" "$STAGE_DIR/usr/local/lib/rustpm/rustpmd.example.conf"
cp "$ROOT_DIR/config/macos/programs.d/example.conf" "$STAGE_DIR/usr/local/lib/rustpm/example.conf"
cp "$ROOT_DIR/packaging/club.motofans.rustpm.plist" "$STAGE_DIR/Library/LaunchDaemons/club.motofans.rustpm.plist"
cp "$ROOT_DIR/packaging/club.motofans.rustpm-privd.plist" "$STAGE_DIR/Library/LaunchDaemons/club.motofans.rustpm-privd.plist"
chmod 755 "$STAGE_DIR/usr/local/lib/rustpm/uninstall_rustpm.sh"
chmod 755 "$SCRIPTS_DIR/preinstall" "$SCRIPTS_DIR/postinstall"

pkgbuild \
  --root "$STAGE_DIR" \
  --identifier "$ID" \
  --version "$VERSION" \
  --scripts "$SCRIPTS_DIR" \
  "$PKG_PATH"

echo "Created: $PKG_PATH"
