#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SDCARD_DIR="${1:-$ROOT_DIR/simulator/sdcard}"

mkdir -p "$SDCARD_DIR"

rm -r "$SDCARD_DIR/.cool" || true

cd "$ROOT_DIR"
cargo run -p simulator -- "$SDCARD_DIR"
