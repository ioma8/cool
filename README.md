# Cool

Rust `no_std` e-reader firmware for the Xteink X4 device (ESP32-C3 + SSD1677 e-ink display).

## Features

- **Bare-metal Rust** — No OS, no heap allocation, fully `no_std`
- **SSD1677 e-ink driver** — 800×480 display with 90° rotation (480×800 portrait)
- **Deep sleep** — 5-minute idle timeout, wake on button press
- **USB serial** — Native USB-JTAG for flashing and debug console

## Hardware

| Component | Details |
|-----------|---------|
| MCU | ESP32-C3 (RISC-V) |
| Display | SSD1677 e-ink 800×480 |
| SPI | SCLK=GPIO8, MOSI=GPIO10, MISO=GPIO7 |
| Control | CS=GPIO21, DC=GPIO4, RST=GPIO5, BUSY=GPIO6 |
| Buttons | ADC1=GPIO1, ADC2=GPIO2, Power=GPIO3 |

## Architecture

```
src/
├── main.rs      # Entry point, init, main loop
├── display.rs   # SSD1677 e-ink driver (SPI, rotation, text rendering)
└── hal.rs       # Button handling, wakeup reason detection
```

The display driver is a precise port of the C `EInkDisplay.cpp` from the original SDK, with:
- Command/data SPI transactions matching Arduino timing
- Framebuffer with 90° CCW coordinate transformation
- Full/fast refresh modes

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
