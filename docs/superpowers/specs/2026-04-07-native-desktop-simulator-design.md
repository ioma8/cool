# Native desktop simulator design

## Goal

Add a native desktop simulator that renders the exact monochrome framebuffer the device would show, uses the same Bookerly font and pagination/rendering path as the firmware, reads EPUBs from a host directory that acts as the SD card, and maps keyboard keys to the device buttons for a fast local development loop.

## Why this shape

The current workspace cleanly separates controller, EPUB parsing, button decoding, and much of the rendering logic, but the final rendering API is still coupled to the SSD1677 transport type in `xteink-display`. A desktop simulator needs to reuse the software rendering path, not reimplement it, otherwise the simulator will diverge from the real device and stop being trustworthy.

The right move is to split reusable software rendering from panel transport:

- shared software renderer: framebuffer layout, pixel operations, Bookerly glyph drawing, wrapped text layout, EPUB pagination/rendering into the framebuffer
- hardware transport: SSD1677 SPI/reset/busy sequencing and display refresh scheduling
- simulator presenter: a desktop window that displays the same 1-bit framebuffer without changing render semantics

## Scope

In scope:

- extract a reusable software-renderer crate from `xteink-display`
- keep framebuffer format and rendering output identical to the device path
- add a desktop simulator binary crate
- add host-directory-backed storage for browsing and EPUB opens
- add keyboard-to-button mapping
- add one-command startup script for the simulator
- document the workflow

Out of scope for this pass:

- QEMU
- exact e-ink waveform timing emulation
- simulated deep sleep / wake wakeup hardware behavior
- a fake analog button ladder or ESP HAL emulation
- image-backed SD media

## Current constraints observed

- `firmware/src/main.rs` owns UI integration flow, browser drawing helpers, display refresh scheduling, and ESP-specific boot/input setup.
- `xteink-controller` already contains the core browse/reader state machine and is host-testable.
- `xteink-display` already contains the software rendering logic needed by both targets, but it is embedded inside `SSD1677Display`.
- `xteink-fs` is `embedded`-feature-gated and coupled to `SdFilesystem`, which is appropriate for hardware but not reusable by a native simulator.

## Proposed architecture

### 1. New shared renderer crate

Create a new crate, tentatively `crates/xteink-render`, responsible for:

- owning the framebuffer in the same layout used today
- exposing display constants (`DISPLAY_WIDTH`, `DISPLAY_HEIGHT`, buffer size)
- drawing pixels and glyphs
- wrapped text rendering
- cached text pagination
- EPUB rendering into the framebuffer using `xteink-epub`

This crate becomes the single source of truth for:

- Bookerly font rendering
- text wrapping
- page boundaries
- monochrome framebuffer layout

The current `xteink-display` crate is reduced to:

- SSD1677 panel transport
- refresh modes and busy handling
- copying a provided framebuffer to the panel

### 2. Shared app integration crate

Create a small crate, tentatively `crates/xteink-app`, for the integration logic now embedded in firmware:

- current path + loaded directory page state
- browser screen rendering helper
- error/loading overlay rendering helper
- controller command dispatch against abstract storage/rendering ports

This avoids duplicating browse/reader orchestration in firmware and simulator.

### 3. Desktop simulator crate

Create a desktop binary crate, tentatively `simulator/`, responsible for:

- opening a native window
- presenting the framebuffer at a readable scale
- preserving 1-bit black/white output without anti-aliased reinterpretation
- polling keyboard input and mapping it to `xteink_buttons::Button`
- reading directory entries and files from `simulator/sdcard/`
- running the same controller/app loop as the firmware, but synchronously in a desktop event loop

Recommended keyboard mapping:

- `LeftArrow` -> `Left`
- `RightArrow` -> `Right`
- `UpArrow` -> `Up`
- `DownArrow` -> `Down`
- `Enter` -> `Confirm`
- `Backspace` -> `Back`
- `Escape` -> `Power`

### 4. Simulator storage adapter

Create a host-backed storage adapter that mirrors the device-visible semantics:

- root path `/` maps to `simulator/sdcard/`
- directories and entries are listed with the same entry kinds
- EPUB opens read from host files
- cache artifacts can live under `simulator/sdcard/.cool/` to preserve current cache behavior

This adapter should be shaped around the needs of the shared app/render layer, not around `esp-hal`.

## Module/file decomposition

### Shared renderer extraction

- `crates/xteink-render/Cargo.toml`
- `crates/xteink-render/src/lib.rs`
- `crates/xteink-render/src/bookerly.rs`
- `crates/xteink-render/src/pagination.rs`
- `crates/xteink-render/src/text.rs`

`xteink-display` then keeps only transport-specific code and depends on `xteink-render`.

### Shared app integration

- `crates/xteink-app/Cargo.toml`
- `crates/xteink-app/src/lib.rs`
- `crates/xteink-app/src/browser.rs`
- `crates/xteink-app/src/session.rs`

### Desktop simulator

- `simulator/Cargo.toml`
- `simulator/src/main.rs`
- `simulator/src/window.rs`
- `simulator/src/input.rs`
- `simulator/src/storage.rs`
- `simulator/src/runtime.rs`

### Scripts/docs

- `scripts/run-simulator.sh`
- `README.md`
- `docs/PROJECT_OVERVIEW.md`

## Data flow

### Browser flow

1. Simulator starts with host directory `simulator/sdcard/`.
2. Storage adapter lists `/`.
3. Shared app layer loads the first page into controller state.
4. Shared renderer draws the browser into the framebuffer.
5. Window presenter displays the framebuffer.
6. Keyboard input becomes `Button`.
7. Controller emits a command.
8. Shared app layer executes the command and redraws as needed.

### Reader flow

1. User confirms an EPUB entry.
2. Shared app layer asks storage for the EPUB source.
3. Shared renderer renders page `0` using the same font/layout code as firmware.
4. Framebuffer is displayed unchanged by the presenter.
5. Page turn buttons invoke the same controller transitions and renderer page calls as firmware.

## Fidelity rules

The simulator is only useful if it matches device rendering, so these are non-negotiable:

- same Bookerly glyph bitmaps
- same framebuffer bit layout
- same `DISPLAY_WIDTH` / `DISPLAY_HEIGHT`
- same wrapping and line-height behavior
- same EPUB text extraction and pagination code
- no grayscale smoothing or host-font substitution

The desktop window may scale pixels up for readability, but each framebuffer pixel must map to a crisp black or white block.

## Error handling

- Missing `simulator/sdcard/`: show a rendered error screen in the simulator window instead of crashing after startup.
- EPUB read/parse failure: reuse the same error-screen path the firmware uses.
- Invalid cache artifacts: preserve current cache-miss rebuild behavior.
- Unmapped keypresses: ignore.

## Testing strategy

### Test-first targets

- renderer extraction preserves framebuffer output for browser text and wrapped text cases
- shared app layer emits the same controller-driven command effects as firmware today
- host storage adapter lists directories and opens EPUB files with stable path mapping
- keyboard mapping produces the expected button values

### Verification tiers

- unit tests for renderer helpers and app/session logic
- integration tests for host storage adapter against fixture directories/files
- `cargo check` after each change iteration
- final host run command that opens the simulator successfully

## Migration strategy

1. Extract software-rendering primitives into `xteink-render` without changing behavior.
2. Update `xteink-display` to become a transport wrapper over a shared framebuffer.
3. Extract shared app/session logic from firmware into `xteink-app`.
4. Add the desktop simulator using those shared crates.
5. Add script and docs.

This order minimizes risk because each step preserves behavior while creating one new seam at a time.
