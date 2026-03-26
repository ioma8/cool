//! SSD1677 E-Ink Display Driver for Xteink X4 (800x480)
//!
//! Precise port of the C EInkDisplay.cpp driver.
//! 
//! Key SPI behavior (matching C exactly):
//! - sendCommand: DC=LOW, CS toggle per byte
//! - sendData(byte): DC=HIGH, CS toggle per byte
//! - sendData(buffer): DC=HIGH, CS stays LOW for entire transfer

use esp_hal::delay::Delay;

// ============================================================================
// Display Constants
// ============================================================================

// Physical display dimensions (hardware)
const PHYSICAL_WIDTH: u16 = 800;
const PHYSICAL_HEIGHT: u16 = 480;
pub const DISPLAY_WIDTH_BYTES: u16 = PHYSICAL_WIDTH / 8; // 100
pub const BUFFER_SIZE: usize = (DISPLAY_WIDTH_BYTES as usize) * (PHYSICAL_HEIGHT as usize); // 48000

// Logical display dimensions (after 90° CCW rotation)
// User sees portrait: 480 wide × 800 tall
pub const DISPLAY_WIDTH: u16 = 480;
pub const DISPLAY_HEIGHT: u16 = 800;

// ============================================================================
// SSD1677 Commands (from C: EInkDisplay.cpp lines 8-41)
// ============================================================================

// Initialization and reset
const CMD_SOFT_RESET: u8 = 0x12;
const CMD_BOOSTER_SOFT_START: u8 = 0x0C;
const CMD_DRIVER_OUTPUT_CONTROL: u8 = 0x01;
const CMD_BORDER_WAVEFORM: u8 = 0x3C;
const CMD_TEMP_SENSOR_CONTROL: u8 = 0x18;

// RAM and buffer management
const CMD_DATA_ENTRY_MODE: u8 = 0x11;
const CMD_SET_RAM_X_RANGE: u8 = 0x44;
const CMD_SET_RAM_Y_RANGE: u8 = 0x45;
const CMD_SET_RAM_X_COUNTER: u8 = 0x4E;
const CMD_SET_RAM_Y_COUNTER: u8 = 0x4F;
const CMD_WRITE_RAM_BW: u8 = 0x24;
const CMD_WRITE_RAM_RED: u8 = 0x26;
const CMD_AUTO_WRITE_BW_RAM: u8 = 0x46;
const CMD_AUTO_WRITE_RED_RAM: u8 = 0x47;

// Display update and refresh
const CMD_DISPLAY_UPDATE_CTRL1: u8 = 0x21;
const CMD_DISPLAY_UPDATE_CTRL2: u8 = 0x22;
const CMD_MASTER_ACTIVATION: u8 = 0x20;
const CMD_WRITE_TEMP: u8 = 0x1A;

// Power management
const CMD_DEEP_SLEEP: u8 = 0x10;

// ============================================================================
// Control Constants (from C: lines 30-31)
// ============================================================================

const CTRL1_NORMAL: u8 = 0x00;
const CTRL1_BYPASS_RED: u8 = 0x40;
const DATA_ENTRY_X_INC_Y_DEC: u8 = 0x01;
const TEMP_SENSOR_INTERNAL: u8 = 0x80;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RefreshMode {
    Full,
    Half,
    Fast,
}

// ============================================================================
// Font Data (5x7 bitmap font)
// ============================================================================

const FONT_5X7: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // Space
    [0x00, 0x00, 0x5F, 0x00, 0x00], // !
    [0x00, 0x07, 0x00, 0x07, 0x00], // "
    [0x14, 0x7F, 0x14, 0x7F, 0x14], // #
    [0x24, 0x2A, 0x7F, 0x2A, 0x12], // $
    [0x23, 0x13, 0x08, 0x64, 0x62], // %
    [0x36, 0x49, 0x55, 0x22, 0x50], // &
    [0x00, 0x05, 0x03, 0x00, 0x00], // '
    [0x00, 0x1C, 0x22, 0x41, 0x00], // (
    [0x00, 0x41, 0x22, 0x1C, 0x00], // )
    [0x08, 0x2A, 0x1C, 0x2A, 0x08], // *
    [0x08, 0x08, 0x3E, 0x08, 0x08], // +
    [0x00, 0x50, 0x30, 0x00, 0x00], // ,
    [0x08, 0x08, 0x08, 0x08, 0x08], // -
    [0x00, 0x60, 0x60, 0x00, 0x00], // .
    [0x20, 0x10, 0x08, 0x04, 0x02], // /
    [0x3E, 0x51, 0x49, 0x45, 0x3E], // 0
    [0x00, 0x42, 0x7F, 0x40, 0x00], // 1
    [0x42, 0x61, 0x51, 0x49, 0x46], // 2
    [0x21, 0x41, 0x45, 0x4B, 0x31], // 3
    [0x18, 0x14, 0x12, 0x7F, 0x10], // 4
    [0x27, 0x45, 0x45, 0x45, 0x39], // 5
    [0x3C, 0x4A, 0x49, 0x49, 0x30], // 6
    [0x01, 0x71, 0x09, 0x05, 0x03], // 7
    [0x36, 0x49, 0x49, 0x49, 0x36], // 8
    [0x06, 0x49, 0x49, 0x29, 0x1E], // 9
    [0x00, 0x36, 0x36, 0x00, 0x00], // :
    [0x00, 0x56, 0x36, 0x00, 0x00], // ;
    [0x08, 0x14, 0x22, 0x41, 0x00], // <
    [0x14, 0x14, 0x14, 0x14, 0x14], // =
    [0x00, 0x41, 0x22, 0x14, 0x08], // >
    [0x02, 0x01, 0x51, 0x09, 0x06], // ?
    [0x32, 0x49, 0x79, 0x41, 0x3E], // @
    [0x7E, 0x11, 0x11, 0x11, 0x7E], // A
    [0x7F, 0x49, 0x49, 0x49, 0x36], // B
    [0x3E, 0x41, 0x41, 0x41, 0x22], // C
    [0x7F, 0x41, 0x41, 0x22, 0x1C], // D
    [0x7F, 0x49, 0x49, 0x49, 0x41], // E
    [0x7F, 0x09, 0x09, 0x09, 0x01], // F
    [0x3E, 0x41, 0x49, 0x49, 0x7A], // G
    [0x7F, 0x08, 0x08, 0x08, 0x7F], // H
    [0x00, 0x41, 0x7F, 0x41, 0x00], // I
    [0x20, 0x40, 0x41, 0x3F, 0x01], // J
    [0x7F, 0x08, 0x14, 0x22, 0x41], // K
    [0x7F, 0x40, 0x40, 0x40, 0x40], // L
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], // M
    [0x7F, 0x04, 0x08, 0x10, 0x7F], // N
    [0x3E, 0x41, 0x41, 0x41, 0x3E], // O
    [0x7F, 0x09, 0x09, 0x09, 0x06], // P
    [0x3E, 0x41, 0x51, 0x21, 0x5E], // Q
    [0x7F, 0x09, 0x19, 0x29, 0x46], // R
    [0x46, 0x49, 0x49, 0x49, 0x31], // S
    [0x01, 0x01, 0x7F, 0x01, 0x01], // T
    [0x3F, 0x40, 0x40, 0x40, 0x3F], // U
    [0x1F, 0x20, 0x40, 0x20, 0x1F], // V
    [0x3F, 0x40, 0x38, 0x40, 0x3F], // W
    [0x63, 0x14, 0x08, 0x14, 0x63], // X
    [0x07, 0x08, 0x70, 0x08, 0x07], // Y
    [0x61, 0x51, 0x49, 0x45, 0x43], // Z
    [0x00, 0x7F, 0x41, 0x41, 0x00], // [
    [0x02, 0x04, 0x08, 0x10, 0x20], // backslash
    [0x00, 0x41, 0x41, 0x7F, 0x00], // ]
    [0x04, 0x02, 0x01, 0x02, 0x04], // ^
    [0x40, 0x40, 0x40, 0x40, 0x40], // _
    [0x00, 0x01, 0x02, 0x04, 0x00], // `
    [0x20, 0x54, 0x54, 0x54, 0x78], // a
    [0x7F, 0x48, 0x44, 0x44, 0x38], // b
    [0x38, 0x44, 0x44, 0x44, 0x20], // c
    [0x38, 0x44, 0x44, 0x48, 0x7F], // d
    [0x38, 0x54, 0x54, 0x54, 0x18], // e
    [0x08, 0x7E, 0x09, 0x01, 0x02], // f
    [0x0C, 0x52, 0x52, 0x52, 0x3E], // g
    [0x7F, 0x08, 0x04, 0x04, 0x78], // h
    [0x00, 0x44, 0x7D, 0x40, 0x00], // i
    [0x20, 0x40, 0x44, 0x3D, 0x00], // j
    [0x7F, 0x10, 0x28, 0x44, 0x00], // k
    [0x00, 0x41, 0x7F, 0x40, 0x00], // l
    [0x7C, 0x04, 0x18, 0x04, 0x78], // m
    [0x7C, 0x08, 0x04, 0x04, 0x78], // n
    [0x38, 0x44, 0x44, 0x44, 0x38], // o
    [0x7C, 0x14, 0x14, 0x14, 0x08], // p
    [0x08, 0x14, 0x14, 0x18, 0x7C], // q
    [0x7C, 0x08, 0x04, 0x04, 0x08], // r
    [0x48, 0x54, 0x54, 0x54, 0x20], // s
    [0x04, 0x3F, 0x44, 0x40, 0x20], // t
    [0x3C, 0x40, 0x40, 0x20, 0x7C], // u
    [0x1C, 0x20, 0x40, 0x20, 0x1C], // v
    [0x3C, 0x40, 0x30, 0x40, 0x3C], // w
    [0x44, 0x28, 0x10, 0x28, 0x44], // x
    [0x0C, 0x50, 0x50, 0x50, 0x3C], // y
    [0x44, 0x64, 0x54, 0x4C, 0x44], // z
    [0x00, 0x08, 0x36, 0x41, 0x00], // {
    [0x00, 0x00, 0x7F, 0x00, 0x00], // |
    [0x00, 0x41, 0x36, 0x08, 0x00], // }
    [0x10, 0x08, 0x08, 0x10, 0x08], // ~
];

fn get_char_bitmap(c: u8) -> [u8; 5] {
    if c >= 32 && c < 127 {
        FONT_5X7[(c - 32) as usize]
    } else {
        FONT_5X7[0] // Space for unknown chars
    }
}

// ============================================================================
// Display Driver
// ============================================================================

pub struct SSD1677Display<SPI, CS, DC, RST, BUSY>
where
    SPI: embedded_hal::spi::SpiBus,
    CS: embedded_hal::digital::OutputPin,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    spi: SPI,
    cs: CS,
    dc: DC,
    rst: RST,
    busy: BUSY,
    delay: Delay,
    framebuffer: [u8; BUFFER_SIZE],
    is_screen_on: bool,
}

impl<SPI, CS, DC, RST, BUSY> SSD1677Display<SPI, CS, DC, RST, BUSY>
where
    SPI: embedded_hal::spi::SpiBus,
    CS: embedded_hal::digital::OutputPin,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    pub fn new(spi: SPI, cs: CS, dc: DC, rst: RST, busy: BUSY, delay: Delay) -> Self {
        Self {
            spi,
            cs,
            dc,
            rst,
            busy,
            delay,
            framebuffer: [0xFF; BUFFER_SIZE],
            is_screen_on: false,
        }
    }

    // ========================================================================
    // Low-level SPI - EXACTLY matching C code
    // ========================================================================

    /// C: sendCommand (lines 185-192)
    /// DC=LOW, CS toggle per byte
    fn send_command(&mut self, cmd: u8) {
        let _ = self.dc.set_low();
        // Small delay for DC setup time (matching Arduino timing overhead)
        for _ in 0..10 { core::hint::spin_loop(); }
        let _ = self.cs.set_low();
        let _ = self.spi.write(&[cmd]);
        let _ = self.spi.flush(); // Ensure transfer complete before CS high
        let _ = self.cs.set_high();
    }

    /// C: sendData(uint8_t) (lines 194-201)
    /// DC=HIGH, CS toggle per byte
    fn send_data_byte(&mut self, data: u8) {
        let _ = self.dc.set_high();
        // Small delay for DC setup time
        for _ in 0..10 { core::hint::spin_loop(); }
        let _ = self.cs.set_low();
        let _ = self.spi.write(&[data]);
        let _ = self.spi.flush(); // Ensure transfer complete before CS high
        let _ = self.cs.set_high();
    }

    /// C: sendData(const uint8_t*, uint16_t) (lines 203-210)
    /// DC=HIGH, CS stays LOW for entire transfer
    #[allow(dead_code)]
    fn send_data_bulk(&mut self, data: &[u8]) {
        let _ = self.dc.set_high();
        for _ in 0..10 { core::hint::spin_loop(); }
        let _ = self.cs.set_low();
        let _ = self.spi.write(data);
        let _ = self.spi.flush(); // Ensure transfer complete before CS high
        let _ = self.cs.set_high();
    }

    /// C: waitWhileBusy (lines 212-224)
    /// Wait while BUSY pin is HIGH
    fn wait_while_busy(&mut self, comment: &str) {
        let start = esp_hal::time::Instant::now();
        let initial = self.busy.is_high().unwrap_or(false);
        esp_println::println!("  Waiting for {}: BUSY={}", comment, if initial { "HIGH" } else { "LOW" });
        
        while self.busy.is_high().unwrap_or(false) {
            self.delay.delay_millis(1);
            if start.elapsed().as_millis() > 10000 {
                esp_println::println!("  TIMEOUT: {}", comment);
                break;
            }
        }
        esp_println::println!("  Done: {} ({}ms)", comment, start.elapsed().as_millis());
    }

    // ========================================================================
    // Initialization - C: resetDisplay + initDisplayController
    // ========================================================================

    /// C: resetDisplay (lines 174-183)
    fn reset_display(&mut self) {
        esp_println::println!("  Hardware reset...");
        let _ = self.rst.set_high();
        self.delay.delay_millis(20);
        let _ = self.rst.set_low();
        self.delay.delay_millis(2);
        let _ = self.rst.set_high();
        self.delay.delay_millis(20);
    }

    /// C: initDisplayController (lines 226-271)
    fn init_display_controller(&mut self) {
        esp_println::println!("  Init SSD1677...");

        // Soft reset (C line 232-233)
        self.send_command(CMD_SOFT_RESET);
        self.wait_while_busy("soft reset");
        
        // Add delay after soft reset - controller needs time to fully initialize
        self.delay.delay_millis(10);
        esp_println::println!("    Post-reset delay done");

        // Temperature sensor control (C lines 236-237)
        self.send_command(CMD_TEMP_SENSOR_CONTROL);
        self.send_data_byte(TEMP_SENSOR_INTERNAL);

        // Booster soft-start (C lines 240-245)
        self.send_command(CMD_BOOSTER_SOFT_START);
        self.send_data_byte(0xAE);
        self.send_data_byte(0xC7);
        self.send_data_byte(0xC3);
        self.send_data_byte(0xC0);
        self.send_data_byte(0x40);

        // Driver output control (C lines 248-252) - uses PHYSICAL height (480)
        self.send_command(CMD_DRIVER_OUTPUT_CONTROL);
        self.send_data_byte(((PHYSICAL_HEIGHT - 1) & 0xFF) as u8);        // 479 & 0xFF = 0xDF
        self.send_data_byte((((PHYSICAL_HEIGHT - 1) >> 8) & 0xFF) as u8); // 479 >> 8 = 0x01
        self.send_data_byte(0x02);

        // Border waveform (C lines 255-256)
        self.send_command(CMD_BORDER_WAVEFORM);
        self.send_data_byte(0x01);

        // Set RAM area (C line 259) - uses PHYSICAL dimensions
        self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);

        // Skip auto-write commands for now - they don't seem to work
        // We'll clear by writing 0xFF to both RAMs during display_buffer
        esp_println::println!("  Skipping auto-clear (will clear on first write)");
    }

    /// C: setRamArea (lines 273-306) - uses PHYSICAL coordinates
    fn set_ram_area(&mut self, x: u16, y: u16, w: u16, h: u16) {
        // Reverse Y (C line 277) - uses PHYSICAL height
        let y = PHYSICAL_HEIGHT - y - h;

        // Data entry mode (C lines 280-281)
        self.send_command(CMD_DATA_ENTRY_MODE);
        self.send_data_byte(DATA_ENTRY_X_INC_Y_DEC);

        // X range (C lines 284-288)
        self.send_command(CMD_SET_RAM_X_RANGE);
        self.send_data_byte((x & 0xFF) as u8);
        self.send_data_byte(((x >> 8) & 0xFF) as u8);
        self.send_data_byte(((x + w - 1) & 0xFF) as u8);
        self.send_data_byte((((x + w - 1) >> 8) & 0xFF) as u8);

        // Y range (C lines 291-295)
        self.send_command(CMD_SET_RAM_Y_RANGE);
        self.send_data_byte(((y + h - 1) & 0xFF) as u8);
        self.send_data_byte((((y + h - 1) >> 8) & 0xFF) as u8);
        self.send_data_byte((y & 0xFF) as u8);
        self.send_data_byte(((y >> 8) & 0xFF) as u8);

        // X counter (C lines 298-300)
        self.send_command(CMD_SET_RAM_X_COUNTER);
        self.send_data_byte((x & 0xFF) as u8);
        self.send_data_byte(((x >> 8) & 0xFF) as u8);

        // Y counter (C lines 303-305)
        self.send_command(CMD_SET_RAM_Y_COUNTER);
        self.send_data_byte(((y + h - 1) & 0xFF) as u8);
        self.send_data_byte((((y + h - 1) >> 8) & 0xFF) as u8);
    }

    /// C: writeRamBuffer (lines 378-388)
    #[allow(dead_code)]
    fn write_ram_buffer(&mut self, cmd: u8, data: &[u8]) {
        self.send_command(cmd);
        self.send_data_bulk(data);
    }

    /// C: refreshDisplay (lines 574-628)
    fn refresh_display(&mut self, mode: RefreshMode, turn_off_screen: bool) {
        // CTRL1 (C lines 576-577)
        esp_println::println!("  Sending CTRL1 (0x21)...");
        self.send_command(CMD_DISPLAY_UPDATE_CTRL1);
        let ctrl1 = if mode == RefreshMode::Fast { CTRL1_NORMAL } else { CTRL1_BYPASS_RED };
        self.send_data_byte(ctrl1);
        esp_println::println!("    CTRL1 data: 0x{:02X}", ctrl1);

        // Build display mode (C lines 592-615)
        let mut display_mode: u8 = 0x00;

        if !self.is_screen_on {
            self.is_screen_on = true;
            display_mode |= 0xC0;
            esp_println::println!("    Screen was OFF, adding 0xC0 (CLOCK_ON|ANALOG_ON)");
        }

        if turn_off_screen {
            self.is_screen_on = false;
            display_mode |= 0x03;
        }

        match mode {
            RefreshMode::Full => {
                display_mode |= 0x34;
                esp_println::println!("    FULL refresh: adding 0x34");
            }
            RefreshMode::Half => {
                self.send_command(CMD_WRITE_TEMP);
                self.send_data_byte(0x5A);
                display_mode |= 0xD4;
            }
            RefreshMode::Fast => {
                display_mode |= 0x1C;
            }
        }

        esp_println::println!("  Sending CTRL2 (0x22) with mode=0x{:02X}...", display_mode);
        self.send_command(CMD_DISPLAY_UPDATE_CTRL2);
        self.send_data_byte(display_mode);

        esp_println::println!("  Sending MASTER_ACTIVATION (0x20)...");
        self.send_command(CMD_MASTER_ACTIVATION);
        
        // Check BUSY immediately after activation
        let busy_now = self.busy.is_high().unwrap_or(false);
        esp_println::println!("  BUSY immediately after activation: {}", if busy_now { "HIGH" } else { "LOW" });
        
        // Give it a moment to respond
        self.delay.delay_millis(10);
        let busy_after = self.busy.is_high().unwrap_or(false);
        esp_println::println!("  BUSY after 10ms delay: {}", if busy_after { "HIGH" } else { "LOW" });
        
        self.wait_while_busy("refresh");
    }

    // ========================================================================
    // Public API
    // ========================================================================

    pub fn init(&mut self) {
        esp_println::println!("Initializing display...");
        self.reset_display();
        self.init_display_controller();
        esp_println::println!("Display ready.");
    }

    pub fn clear(&mut self, color: u8) {
        self.framebuffer.fill(color);
    }

    /// Set a pixel using logical coordinates (after 90° CCW rotation)
    /// Logical: (0,0) is top-left of portrait display (480×800)
    /// Physical: maps to the 800×480 framebuffer with rotation
    pub fn set_pixel(&mut self, x: u16, y: u16, black: bool) {
        // Bounds check against logical dimensions
        if x >= DISPLAY_WIDTH || y >= DISPLAY_HEIGHT {
            return;
        }
        
        // 90° counter-clockwise rotation transform:
        // Logical (x, y) in 480×800 → Physical (px, py) in 800×480
        // px = y
        // py = (DISPLAY_WIDTH - 1) - x = 479 - x
        let px = y;
        let py = (DISPLAY_WIDTH - 1) - x;
        
        // Calculate framebuffer index using physical coordinates
        let idx = (py as usize) * (DISPLAY_WIDTH_BYTES as usize) + (px as usize / 8);
        let bit = 7 - (px % 8);
        
        if black {
            self.framebuffer[idx] &= !(1 << bit);
        } else {
            self.framebuffer[idx] |= 1 << bit;
        }
    }

    pub fn draw_text(&mut self, x: u16, y: u16, text: &[u8]) {
        let mut cx = x;
        for &c in text {
            if cx + 6 > DISPLAY_WIDTH { break; }
            let bmp = get_char_bitmap(c);
            for col in 0..5 {
                for row in 0..7 {
                    if (bmp[col] >> row) & 1 == 1 {
                        self.set_pixel(cx + col as u16, y + row as u16, true);
                    }
                }
            }
            cx += 6;
        }
    }

    /// C: displayBuffer (lines 443-486)
    pub fn display_buffer(&mut self, mode: RefreshMode) {
        esp_println::println!("Writing to display...");
        
        // Use PHYSICAL dimensions for RAM area
        self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);

        if mode != RefreshMode::Fast {
            // Full/Half: write both RAMs (C lines 461-462)
            let start = esp_hal::time::Instant::now();
            esp_println::println!("  Writing BW RAM ({} bytes)...", BUFFER_SIZE);
            self.send_command(CMD_WRITE_RAM_BW);
            let _ = self.dc.set_high();
            let _ = self.cs.set_low();
            // Write in chunks to ensure data is sent (matching Arduino SPI.writeBytes behavior)
            for chunk in self.framebuffer.chunks(4096) {
                let _ = self.spi.write(chunk);
            }
            let _ = self.spi.flush();
            let _ = self.cs.set_high();
            esp_println::println!("    BW done ({}ms)", start.elapsed().as_millis());
            
            let start = esp_hal::time::Instant::now();
            esp_println::println!("  Writing RED RAM ({} bytes)...", BUFFER_SIZE);
            self.send_command(CMD_WRITE_RAM_RED);
            let _ = self.dc.set_high();
            let _ = self.cs.set_low();
            for chunk in self.framebuffer.chunks(4096) {
                let _ = self.spi.write(chunk);
            }
            let _ = self.spi.flush();
            let _ = self.cs.set_high();
            esp_println::println!("    RED done ({}ms)", start.elapsed().as_millis());
        } else {
            // Fast: BW only (C line 465)
            self.send_command(CMD_WRITE_RAM_BW);
            let _ = self.dc.set_high();
            let _ = self.cs.set_low();
            for chunk in self.framebuffer.chunks(4096) {
                let _ = self.spi.write(chunk);
            }
            let _ = self.spi.flush();
            let _ = self.cs.set_high();
        }

        self.refresh_display(mode, false);

        // Single buffer mode: sync RED after refresh (C lines 480-485)
        if mode == RefreshMode::Fast {
            self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);
            self.send_command(CMD_WRITE_RAM_RED);
            let _ = self.dc.set_high();
            let _ = self.cs.set_low();
            for chunk in self.framebuffer.chunks(4096) {
                let _ = self.spi.write(chunk);
            }
            let _ = self.spi.flush();
            let _ = self.cs.set_high();
        }
    }

    pub fn refresh_full(&mut self) {
        self.display_buffer(RefreshMode::Full);
    }

    #[allow(dead_code)]
    pub fn refresh_fast(&mut self) {
        self.display_buffer(RefreshMode::Fast);
    }

    /// C: deepSleep (lines 660-684)
    pub fn deep_sleep(&mut self) {
        esp_println::println!("Display deep sleep...");
        
        if self.is_screen_on {
            self.send_command(CMD_DISPLAY_UPDATE_CTRL1);
            self.send_data_byte(CTRL1_BYPASS_RED);

            self.send_command(CMD_DISPLAY_UPDATE_CTRL2);
            self.send_data_byte(0x03);

            self.send_command(CMD_MASTER_ACTIVATION);
            self.wait_while_busy("power down");

            self.is_screen_on = false;
        }

        self.send_command(CMD_DEEP_SLEEP);
        self.send_data_byte(0x01);
    }
}
