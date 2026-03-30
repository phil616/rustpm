#!/bin/sh
set -eu

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"

export CARGO_HOME="$REPO_DIR/.cargo-home"
export RUSTUP_HOME="$REPO_DIR/.rustup-home"
export CARGO_TARGET_DIR="$REPO_DIR/target"
export PATH="$CARGO_HOME/bin:$PATH"

mkdir -p "$CARGO_HOME" "$RUSTUP_HOME" "$CARGO_TARGET_DIR"

echo "Local Rust env enabled for: $REPO_DIR"
echo "CARGO_HOME=$CARGO_HOME"
echo "RUSTUP_HOME=$RUSTUP_HOME"
echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
