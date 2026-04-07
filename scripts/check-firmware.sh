#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${EMBEDDED_TARGET:-riscv32imc-unknown-none-elf}"
BUILD_STD="${EMBEDDED_BUILD_STD:-core}"

cd "$ROOT_DIR"

echo "Using embedded target: $TARGET"
echo "Using build-std: $BUILD_STD"
echo "Checking firmware..."

cargo check -p xteink-reader --features embedded --target "$TARGET" -Zbuild-std="$BUILD_STD"

echo
echo "Firmware check completed."
