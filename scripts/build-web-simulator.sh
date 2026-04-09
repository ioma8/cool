#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

cd "$ROOT_DIR"
wasm-pack build web-simulator --target web --release --out-dir "$DIST_DIR/pkg" --out-name web_simulator
cp web/index.html "$DIST_DIR/index.html"
cp web/app.js "$DIST_DIR/app.js"

echo "Web simulator built in $DIST_DIR"
