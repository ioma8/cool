#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${EMBEDDED_TARGET:-riscv32imc-unknown-none-elf}"
RUSTFLAGS="${EMBEDDED_RUSTFLAGS_EXTRA:--Zbuild-std=core}"

cd "$ROOT_DIR"

echo "Using embedded target: $TARGET"
echo "Using build flags: $RUSTFLAGS"
echo "Running tests for xteink-fs..."

cargo test -p xteink-fs --target "$TARGET" "$RUSTFLAGS"

echo
echo "Embedded fs tests completed."
