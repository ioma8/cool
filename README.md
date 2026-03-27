# Cool

Rust `no_std` e-reader firmware for the Xteink X4 device (ESP32-C3 + SSD1677 e-ink display).

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
│   ├── xteink-display/  # SSD1677 framebuffer, text layout, render helpers
│   ├── xteink-epub/     # EPUB and ZIP parsing for embedded use
│   ├── xteink-fs/       # SD filesystem access and reader/browser orchestration
│   ├── xteink-input/    # Debouncing and button event tracking
│   ├── xteink-power/    # Wakeup classification and idle-timeout policy
│   └── xteink-sdspi/    # SD-over-SPI transport and protocol layer
├── docs/                # Hardware behavior documentation
└── firmware/            # ESP32-C3 application crate using esp-hal
```

## Architecture

```
firmware/src/
└── main.rs      # Entry point, wiring of esp-hal peripherals to workspace crates
```

Custom logic now lives in dedicated crates:

- `xteink-display` drives the e-ink panel and rendering pipeline
- `xteink-fs` and `xteink-sdspi` provide SD-backed browsing and file access
- `xteink-epub` parses EPUB content for on-device rendering
- `xteink-buttons` and `xteink-input` handle raw input mapping and button events
- `xteink-browser` and `xteink-power` provide UI navigation and sleep policy

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

The workspace defaults to the embedded `riscv32imc-unknown-none-elf` target. Host-side parser tests for the EPUB crate should be run explicitly on the host target:

```bash
cargo test -p xteink-epub --target aarch64-apple-darwin -Zbuild-std=std,panic_abort
```

## Flashing

```bash
# Install espflash
cargo install espflash

# Flash via USB
espflash flash --monitor target/riscv32imc-unknown-none-elf/release/xteink-reader
```

## License

MIT
