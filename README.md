# Cool

Rust `no_std` e-reader firmware for the Xteink X4 device (ESP32-C3 + SSD1677 e-ink display).
The repo now also includes a native desktop simulator that reuses the same shared framebuffer and Bookerly text rendering path as the device.
The current renderer preserves 4-shade grayscale antialiasing for text and shares the same grayscale framebuffer model across hardware, the desktop simulator, and the web simulator.

## Features

- **Bare-metal Rust** — No OS, no heap allocation, fully `no_std`
- **SSD1677 e-ink driver** — 800×480 display with 90° rotation (480×800 portrait)
- **All 7 buttons mapped** — ADC-based detection for 6 front/side buttons + digital power button
- **Deep sleep** — 5-minute idle timeout, wake on button press
- **USB serial** — Native USB-JTAG for flashing and debug console

## Hardware

| Component | Details |
|-----------|---------|
| MCU | ESP32-C3 (RISC-V) |
| Display | SSD1677 e-ink 800×480 |
| SPI | SCLK=GPIO8, MOSI=GPIO10, MISO=GPIO7 |
| Control | CS=GPIO21, DC=GPIO4, RST=GPIO5, BUSY=GPIO6 |
| Buttons | ADC1=GPIO1 (Back/Confirm/Left/Right), ADC2=GPIO2 (Up/Down), Power=GPIO3 |

## Button Mapping

The firmware uses calibrated raw ADC thresholds from `xteink-buttons`. The front/side button ADC reads are taken with 12 dB attenuation, so these values are board-specific raw readings rather than normalized voltages.

| Button | Pin | Method | Raw ADC Range |
|--------|-----|--------|-----------|
| Back | GPIO1 | ADC | `10360 < value <= 11480` |
| Confirm | GPIO1 | ADC | `11540 <= value <= 11740` or `9880 < value <= 10360` |
| Left | GPIO1 | ADC | `8480 < value <= 9880` |
| Right | GPIO1 | ADC | `value <= 8480` |
| Up | GPIO2 | ADC | `17280 < value <= 19380` |
| Down | GPIO2 | ADC | `value <= 17280` |
| Power | GPIO3 | Digital | Active LOW |

## Workspace

```
.
├── crates/
│   ├── xteink-browser/  # Paged browser state machine for SD directory navigation
│   ├── xteink-buttons/  # ADC threshold mapping and button-state logic
│   ├── xteink-app/      # Shared browse/reader session orchestration
│   ├── xteink-controller/ # Host-testable app/controller state transitions
│   ├── xteink-display/  # SSD1677 framebuffer, text layout, render helpers
│   ├── xteink-epub/     # EPUB and ZIP parsing for embedded use
│   ├── xteink-render/   # Shared framebuffer, Bookerly text, and EPUB page rendering
│   ├── xteink-fs/       # SD filesystem access and reader/browser orchestration
│   ├── xteink-input/    # Debouncing and button event tracking
│   ├── xteink-power/    # Wakeup classification and idle-timeout policy
│   └── xteink-sdspi/    # SD-over-SPI transport and protocol layer
├── docs/                # Hardware behavior documentation
├── firmware/            # ESP32-C3 application crate using esp-hal
└── simulator/           # Native desktop simulator binary and host-backed SD root
```

## Architecture

```
firmware/src/
└── main.rs      # Entry point, wiring of esp-hal peripherals to workspace crates
```

Custom logic now lives in dedicated crates:

- `xteink-display` drives the e-ink panel and rendering pipeline
- `xteink-render` owns the shared grayscale framebuffer, font rendering, wrapping, and EPUB page composition
- `xteink-app` owns shared browse/reader session flow for non-hardware runtimes
- `xteink-fs` and `xteink-sdspi` provide SD-backed browsing and file access
- `xteink-epub` parses EPUB content for on-device rendering
- `xteink-buttons` and `xteink-input` handle raw input mapping and button events
- `xteink-browser` and `xteink-power` provide UI navigation and sleep policy
- `xteink-controller` owns host-testable browse/reader state transitions used by firmware

For a short architecture summary, see `docs/PROJECT_OVERVIEW.md`.

## Building

```bash
# Install Rust nightly and ESP32 target
rustup install nightly
rustup component add rust-src --toolchain nightly

# Build
cargo +nightly build --release
```

## Testing

Host and embedded verification are now explicit instead of being forced by workspace-wide Cargo config:

```bash
bash scripts/run-tests-host.sh
bash scripts/check-firmware.sh
bash scripts/run-tests-embedded-fs.sh
```

For a full host-side workspace test run:

```bash
cargo test --workspace --target aarch64-apple-darwin
```

## Simulator

The desktop simulator renders the same shared framebuffer model the device uses and reads books from `simulator/sdcard/`.
The simulator now maps the shared 4-shade framebuffer directly to desktop pixels so grayscale antialiasing is visible during host testing.

```bash
bash scripts/run-simulator.sh
```

Keyboard mapping:

- `Left` / `Right` / `Up` / `Down`: navigation
- `Enter`: open selected entry
- `Backspace`: go to parent / back
- `Escape`: power

## Web simulator

A wasm/web version of the simulator is available and reuses the same shared app/session + render + fs reader pipeline.

- framebuffer is rendered into an HTML `<canvas>`
- grayscale shades are rendered directly in the canvas output
- button footer maps to device-style actions
- EPUB uploads persist to browser `localStorage`
- cache sidecars are written under `/.cool` in the simulated browser-backed SD filesystem

Build a deployable static bundle:

```bash
bash scripts/build-web-simulator.sh
```

Output is placed in `dist/` and can be hosted as static files.

## EPUB Cache And Pagination

EPUB reading now uses a byte-offset cache model. Pagination, resume, chapter jumps, and footer progress all operate on offsets into a fully cached `content.txt`, not on speculative total page counts.

Cache files for each EPUB are written under `/.cool/<book>/`:

- `content.txt`: full linearized UTF-8 text stream used for page rendering
- `meta.txt`: cache validity fields plus final `content_length`
- `chapters.idx`: `CHP1` header followed by chapter records of `u64 start_offset`, `u16 title_len`, and UTF-8 title bytes
- `progress.bin`: three little-endian `u64` values in order `previous`, `current`, `next`

Reader behavior:

- cold open builds the full cache before later pages are read from it
- current reader position is the start byte offset of the rendered page
- next page starts at the previous render's `next` offset
- previous page prefers the saved `previous` offset and otherwise rescans from `0` or a chapter boundary
- chapter jumps use `chapters.idx` offsets and can also show cached chapter titles

Chapter metadata behavior:

- chapter titles prefer EPUB nav/TOC labels
- if a chapter has no nav title, cache build falls back to the first visible text near that chapter offset in `content.txt`
- titles are truncated to the first 64 characters before they are stored

Footer progress behavior:

- first page always shows `0%`
- otherwise progress is `floor(current_page_start_offset * 100 / content_length)`
- non-terminal pages are clamped to `99%`
- `100%` is shown only for the terminal page at EOF

## Flashing

```bash
# Build and flash via USB
bash scripts/build-and-flash.sh
```

## License

MIT
