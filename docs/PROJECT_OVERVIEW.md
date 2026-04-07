# Project Overview

## Purpose

`cool` is a Rust `no_std` firmware workspace for the Xteink X4 e-reader hardware. The target device is an ESP32-C3 driving an SSD1677 e-ink panel, reading content from SD storage, and rendering EPUB content through a custom embedded pipeline.

## Top-Level Architecture

- `firmware/`: hardware-facing application crate and async entrypoint
- `simulator/`: native desktop runtime with host-backed SD directory and keyboard input
- `crates/xteink-app/`: shared browse/reader session orchestration
- `crates/xteink-controller/`: host-testable app/controller state transitions for browse and reader flow
- `crates/xteink-display/`: SSD1677 display driver, framebuffer, text layout, EPUB rendering helpers
- `crates/xteink-render/`: shared framebuffer, Bookerly glyph rendering, wrapped text, and EPUB page composition
- `crates/xteink-epub/`: heap-conscious EPUB and ZIP parsing
- `crates/xteink-fs/`: SD card initialization, directory paging, cache helpers, and EPUB loading/render orchestration
- `crates/xteink-sdspi/`: SPI transport and SD card protocol implementation
- `crates/xteink-browser/`: paged browser state machine for directory navigation
- `crates/xteink-buttons/`: raw button definitions and ADC threshold mapping
- `crates/xteink-input/`: button debouncing and press/release event tracking
- `crates/xteink-power/`: wakeup classification and idle-to-sleep policy

## Runtime Flow

The main runtime lives in `firmware/src/main.rs`. On boot it:

1. Initializes the ESP32-C3 peripherals.
2. Detects USB power and configures SPI, GPIO, ADC, display, and SD access.
3. Builds the initial browser state for the SD card root directory.
4. Delegates browse/reader state transitions to `xteink-controller`.
5. Renders either the file browser or reader screen to the e-ink display.
6. Processes button input to navigate directories, open EPUB files, turn pages, and exit reading mode.

The firmware uses `embassy`, `esp-hal`, and `heapless`, keeping the design compatible with `no_std` constraints and predictable memory usage.

The simulator reuses the same controller and rendering pipeline, but swaps the hardware layer for host filesystem access, a native window, and keyboard input.

## Key Design Characteristics

- No general-purpose OS and no heap allocation in the runtime path.
- Workspace split into focused crates so hardware, input, browsing, controller logic, storage, and rendering logic are separable.
- Custom EPUB handling instead of depending on a desktop-style reader stack.
- Embedded-first filesystem flow: paged directory listing, entry selection, and on-device page rendering.
- Power-aware behavior through explicit wakeup classification and a five-minute idle sleep policy.

## Current Product Shape

This is not just a display driver demo. The workspace already contains the pieces for a minimal embedded reader:

- storage and directory browsing
- button-driven UI navigation
- EPUB parsing and rendering
- e-ink display refresh management
- deep sleep / wakeup behavior

The firmware crate is the integration layer; the crates under `crates/` hold most of the reusable logic.

## Verification

- Host workspace tests: `cargo test --workspace --target aarch64-apple-darwin`
- Desktop simulator: `bash scripts/run-simulator.sh`
- Host crate loop: `bash scripts/run-tests-host.sh`
- Embedded firmware check: `bash scripts/check-firmware.sh`
- Embedded fs build check: `bash scripts/run-tests-embedded-fs.sh`
