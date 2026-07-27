#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export RUSTCUT_DATA_DIR="${RUSTCUT_DATA_DIR:-$ROOT/data}"
exec "$ROOT/bin/rustcut-server" "$@"
