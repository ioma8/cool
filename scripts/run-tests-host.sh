#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${HOST_TARGET:-aarch64-apple-darwin}"
RUSTFLAGS="${RUSTFLAGS_EXTRA:--Zbuild-std=std,panic_abort}"

CRATES=(
  xteink-buttons
  xteink-power
  xteink-display
  xteink-input
  xteink-browser
  xteink-epub
  xteink-sdspi
)

cd "$ROOT_DIR"

echo "Using host target: $TARGET"
echo "Using build flags: $RUSTFLAGS"

for crate in "${CRATES[@]}"; do
  echo
  echo "Running tests for $crate..."
  cargo test -p "$crate" --target "$TARGET" "$RUSTFLAGS"
done

echo
echo "All host crate tests completed."
