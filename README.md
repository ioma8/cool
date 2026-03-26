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

| Button | Pin | Method | ADC Range |
|--------|-----|--------|-----------|
| Back | GPIO1 | ADC | 3100-3800 |
| Confirm | GPIO1 | ADC | 2090-3100 |
| Left | GPIO1 | ADC | 750-2090 |
| Right | GPIO1 | ADC | 0-750 |
| Up | GPIO2 | ADC | 1120-3800 |
| Down | GPIO2 | ADC | 0-1120 |
| Power | GPIO3 | Digital | Active LOW |

## Workspace

```
.
├── crates/
│   ├── xteink-buttons/  # ADC threshold mapping and button-state logic
│   ├── xteink-display/  # SSD1677 framebuffer and generic embedded-hal driver
│   └── xteink-power/    # Wakeup classification and idle-timeout policy
├── docs/                # Hardware behavior documentation
└── firmware/            # ESP32-C3 application crate using esp-hal
```

## Architecture

```
firmware/src/
└── main.rs      # Entry point, wiring of esp-hal peripherals to workspace crates
```

Custom logic now lives in dedicated crates:

- `xteink-display` contains the SSD1677 driver and framebuffer logic
- `xteink-buttons` contains ADC-to-button mapping and button state handling
- `xteink-power` contains wakeup classification and awake-timeout policy

## Building

```bash
# Install Rust nightly and ESP32 target
rustup install nightly
rustup component add rust-src --toolchain nightly

# Build
cargo +nightly build --release
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
