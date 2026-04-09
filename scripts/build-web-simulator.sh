#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"

log() {
  printf '[build-web-simulator] %s\n' "$1"
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf '[build-web-simulator] missing required command: %s\n' "$1" >&2
    return 1
  fi
}

run_wasm_pack_build() {
  CARGO_TERM_VERBOSE=true \
    CARGO_TERM_PROGRESS_WHEN=always \
    wasm-pack build web-simulator --target web --release --out-dir "$DIST_DIR/pkg" --out-name web_simulator &
  local pid=$!
  local elapsed=0

  log "wasm-pack pid: $pid"
  log "cargo verbosity: verbose"
  while kill -0 "$pid" >/dev/null 2>&1; do
    sleep 5
    elapsed=$((elapsed + 5))
    log "wasm-pack still running (${elapsed}s elapsed)"
  done

  wait "$pid"
}

log "building web simulator"
log "workspace: $ROOT_DIR"
require_command wasm-pack

log "resetting dist directory: $DIST_DIR"
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

cd "$ROOT_DIR"
log "running wasm-pack build"
run_wasm_pack_build
log "copying static web assets"
cp web/index.html "$DIST_DIR/index.html"
cp web/app.js "$DIST_DIR/app.js"

log "web simulator built in $DIST_DIR"
