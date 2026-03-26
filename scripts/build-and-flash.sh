#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${TARGET:-riscv32imc-unknown-none-elf}"
BUILD_MODE="${BUILD_MODE:-release}"
CRATE_NAME="${CRATE_NAME:-xteink-reader}"
PACKAGE="${PACKAGE:-xteink-reader}"

cd "$ROOT_DIR"

echo "Building $PACKAGE for $TARGET ($BUILD_MODE)"
if [ "$BUILD_MODE" = "release" ]; then
  cargo build -p "$PACKAGE" --target "$TARGET" --release
  OUT_DIR="$ROOT_DIR/target/$TARGET/release"
else
  cargo build -p "$PACKAGE" --target "$TARGET"
  OUT_DIR="$ROOT_DIR/target/$TARGET/debug"
fi

ELF_PATH="$OUT_DIR/$CRATE_NAME"
if [ ! -f "$ELF_PATH" ] && [ -f "${ELF_PATH}.elf" ]; then
  ELF_PATH="${ELF_PATH}.elf"
fi

if [ ! -f "$ELF_PATH" ]; then
  echo "Could not find flash artifact at $OUT_DIR/$CRATE_NAME{,.elf}"
  echo "Directory listing:"
  ls -1 "$OUT_DIR"
  exit 1
fi

echo "Flashing $ELF_PATH"
espflash flash --monitor "$ELF_PATH" "$@"
