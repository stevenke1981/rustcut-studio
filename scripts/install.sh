#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
SOURCE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="$SOURCE_DIR/bin"

if [[ ! -x "$BIN_DIR/rustcut-server" ]]; then
  printf 'Run this installer from an extracted release package. Missing %s\n' "$BIN_DIR/rustcut-server" >&2
  exit 1
fi

install -d "$PREFIX/bin"
for binary in rustcut-cli rustcut-server rustcut-mcp; do
  install -m 0755 "$BIN_DIR/$binary" "$PREFIX/bin/$binary"
done
printf 'Installed RustCut binaries to %s/bin\n' "$PREFIX"
printf 'Start with: RUSTCUT_DATA_DIR=./data rustcut-server\n'
