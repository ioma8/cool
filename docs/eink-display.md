# E-Ink Display Communication Specification

This document describes the current Rust firmware implementation for the Xteink X4 e-ink display interface in `xteink-display/src/lib.rs` and its initialization in `firmware/src/main.rs`.

## Hardware Interface

- Display controller: SSD1677
- Physical panel resolution: 800 x 480 pixels
- Logical drawing resolution exposed by the driver: 480 x 800 pixels
- Logical orientation: portrait, implemented as a 90 degree counter-clockwise transform into the physical framebuffer
- SPI peripheral: `SPI2`
- SPI clock: 40 MHz
- SPI mode: mode 0

## Pin Mapping

- `SCLK`: GPIO8
- `MOSI`: GPIO10
- `MISO`: GPIO7
- `CS`: GPIO21
- `DC`: GPIO4
- `RST`: GPIO5
- `BUSY`: GPIO6

`MISO` is configured even though the display path is write-only, because the SPI bus is created with all three signal lines.

## Framebuffer Layout

- Buffer size: 48,000 bytes
- Bytes per physical row: 100
- Pixel encoding: `0` bit = black, `1` bit = white
- Initial framebuffer state: all bytes `0xFF`

Logical pixel `(x, y)` is transformed to physical pixel `(px, py)` as follows:

- `px = y`
- `py = 479 - x`

The byte index and bit position are then:

- `idx = py * 100 + (px / 8)`
- `bit = 7 - (px % 8)`

## SPI Transaction Rules

The driver intentionally mirrors the original C implementation timing and chip-select behavior.

### Command write

- `DC` is driven low
- a short spin-loop delay is inserted
- `CS` is driven low
- one command byte is written
- SPI is flushed
- `CS` is driven high

### Single data byte write

- `DC` is driven high
- a short spin-loop delay is inserted
- `CS` is driven low
- one data byte is written
- SPI is flushed
- `CS` is driven high

### Bulk data write

- `DC` is driven high
- a short spin-loop delay is inserted
- `CS` is driven low for the entire transfer
- all bytes are written
- SPI is flushed
- `CS` is driven high

Framebuffer uploads are chunked in 4096-byte writes while keeping `CS` low across the whole RAM transaction.

## Initialization Sequence

After firmware startup, `firmware/src/main.rs` constructs `SSD1677Display` and calls `init()`.

`init()` performs:

1. Hardware reset
2. SSD1677 controller initialization

### Hardware reset

- `RST` high for 20 ms
- `RST` low for 2 ms
- `RST` high for 20 ms

### Controller initialization sequence

The current implementation sends the following commands in order:

1. `0x12` soft reset
2. wait while `BUSY` is high, with a 10 second timeout
3. 10 ms delay
4. `0x18` temperature sensor control, data `0x80`
5. `0x0C` booster soft start, data `0xAE 0xC7 0xC3 0xC0 0x40`
6. `0x01` driver output control, data derived from physical height 480:
   - low byte: `0xDF`
   - high byte: `0x01`
   - third byte: `0x02`
7. `0x3C` border waveform, data `0x01`
8. RAM area setup covering the full 800 x 480 panel

Auto-write clear commands `0x46` and `0x47` are not used in the current implementation.

## RAM Addressing

The driver programs RAM using physical coordinates. Before each full-frame upload it configures:

- data entry mode `0x11` with value `0x01`
- X range with `0x44`
- Y range with `0x45`
- X counter with `0x4E`
- Y counter with `0x4F`

Y addressing is inverted internally:

- `y_internal = 480 - y - h`

For full-screen updates the configured area is:

- `x = 0`
- `y = 0`
- `w = 800`
- `h = 480`

## Refresh Modes

The public API exposes:

- `refresh_full()`
- `refresh_fast()`
- `deep_sleep()`

### Full refresh

- writes the framebuffer to black/white RAM with command `0x24`
- writes the same framebuffer to red RAM with command `0x26`
- sends update control 1 (`0x21`) with `0x40`
- builds update control 2 (`0x22`) flags:
  - if the screen was off, adds `0xC0`
  - adds `0x34` for full refresh
- sends master activation `0x20`
- waits for `BUSY` to go low

### Fast refresh

- writes only black/white RAM with command `0x24`
- sends update control 1 (`0x21`) with `0x00`
- builds update control 2 (`0x22`) flags:
  - if the screen was off, adds `0xC0`
  - adds `0x1C` for fast refresh
- sends master activation `0x20`
- waits for `BUSY` to go low
- after the refresh, writes the same framebuffer into red RAM with `0x26` to keep both controller memories synchronized

### Half refresh

The enum exists and `refresh_display()` has handling for it, but there is no public method calling it in the current firmware.

If used, it would:

- send `0x1A` with value `0x5A`
- use update control 2 flags `0xD4`

## Text Rendering

Text rendering is implemented entirely in software:

- font: built-in 5 x 7 bitmap font
- character advance: 6 pixels
- clipping: drawing stops when the next glyph would exceed logical width 480
- only set bits for black pixels; background remains unchanged unless the framebuffer was explicitly cleared first

## Busy Handling

The driver treats `BUSY = HIGH` as busy.

- it polls once per millisecond
- it aborts waiting after 10,000 ms
- the driver crate itself does not emit timeout logging

## Power-Down Behavior

`deep_sleep()` on the display driver does the following:

1. If the internal `is_screen_on` flag is true:
   - send `0x21` with `0x40`
   - send `0x22` with `0x03`
   - send `0x20`
   - wait for `BUSY` to clear
   - mark the screen as off
2. Send `0x10` with data `0x01`

The framebuffer in MCU memory is not preserved across MCU deep sleep. The visible panel image remains on screen because the panel is not cleared before the controller is put into deep sleep.
