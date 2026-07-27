#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' crates/core/Cargo.toml | head -n1)"
if [[ -z "$VERSION" ]]; then
  VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n1)"
fi
VERSION="${VERSION:-0.1.0}"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
NAME="rustcut-studio-${VERSION}-${OS}-${ARCH}"
STAGE="dist/${NAME}"

cargo test --workspace
cargo build --release --workspace
rm -rf "$STAGE"
mkdir -p "$STAGE/bin"

for bin in rustcut-cli rustcut-server rustcut-mcp; do
  cp "target/release/$bin" "$STAGE/bin/"
done
cp -R web config docs deploy scripts "$STAGE/"
cp README.md LICENSE .env.example .mcp.json.example "$STAGE/"

(
  cd dist
  tar -czf "${NAME}.tar.gz" "$NAME"
)
printf 'Created %s\n' "dist/${NAME}.tar.gz"
