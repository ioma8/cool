//! Hardware Abstraction Layer for Xteink X4
//!
//! Button input handling using ADC for multiple buttons and digital for power button
//! USB connection detection via GPIO20 (UART0_RXD)

use esp_hal::rtc_cntl::{reset_reason, wakeup_cause, SocResetReason};
use esp_hal::system::{Cpu, SleepSource};

// GPIO20 (UART0_RXD) is used to detect USB connection
// When USB is connected, this pin reads HIGH
#[allow(dead_code)]
pub const USB_DETECT_GPIO: u8 = 20;

/// Wakeup reason - distinguishes between different boot scenarios
/// This is important for proper behavior after flashing vs normal power-on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeupReason {
    /// User pressed the power button (normal boot)
    PowerButton,
    /// Device was just flashed via USB (proceed to boot normally)
    AfterFlash,
    /// USB power caused a cold boot (should go back to sleep)
    AfterUSBPower,
    /// Other/unknown reason
    Other,
}

/// Determine the wakeup reason based on reset reason, wakeup cause, and USB state
/// This matches the logic from the C codebase (HalGPIO.cpp)
pub fn get_wakeup_reason(usb_connected: bool) -> WakeupReason {
    let wake_cause = wakeup_cause();
    let reset = reset_reason(Cpu::ProCpu);
    
    // Match the C logic from HalGPIO::getWakeupReason()
    match (wake_cause, reset, usb_connected) {
        // Power button press (cold boot without USB, or GPIO wakeup from deep sleep with USB)
        (SleepSource::Undefined, Some(SocResetReason::ChipPowerOn), false) => WakeupReason::PowerButton,
        (SleepSource::Gpio, Some(SocResetReason::CoreDeepSleep), true) => WakeupReason::PowerButton,
        
        // After flash - undefined wakeup with no reset reason and USB connected
        // ESP_RST_UNKNOWN in C corresponds to None (unknown) reset reason
        (SleepSource::Undefined, None, true) => WakeupReason::AfterFlash,
        
        // USB power caused cold boot
        (SleepSource::Undefined, Some(SocResetReason::ChipPowerOn), true) => WakeupReason::AfterUSBPower,
        
        // Everything else
        _ => WakeupReason::Other,
    }
}

// ADC thresholds for button detection (from C reference)
// Button ADC values (recorded from real devices):
// BACK: ~3512, CONFIRM: ~2694, LEFT: ~1493, RIGHT: ~5, UP: ~2242, DOWN: ~5
// These ranges are midpoints between values
#[allow(dead_code)]
const ADC_NO_BUTTON: i32 = 3800;
#[allow(dead_code)]
const ADC_RANGES_1: [i32; 5] = [ADC_NO_BUTTON, 3100, 2090, 750, i32::MIN]; // BACK, CONFIRM, LEFT, RIGHT
#[allow(dead_code)]
const ADC_RANGES_2: [i32; 3] = [ADC_NO_BUTTON, 1120, i32::MIN]; // UP, DOWN

/// Button indices
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
pub enum Button {
    Back = 0,
    Confirm = 1,
    Left = 2,
    Right = 3,
    Up = 4,
    Down = 5,
    Power = 6,
}

/// Button state - tracks which buttons are currently pressed
#[derive(Debug, Clone, Copy, Default)]
pub struct ButtonState {
    pub state: u8,
}

impl ButtonState {
    /// Check if a specific button is pressed
    #[allow(dead_code)]
    pub fn is_pressed(&self, button: Button) -> bool {
        (self.state & (1 << button as u8)) != 0
    }
    
    /// Check if any button is pressed
    pub fn any_pressed(&self) -> bool {
        self.state != 0
    }
    
    /// Create a new button state with a button pressed
    pub fn with_button(mut self, button: Button) -> Self {
        self.state |= 1 << button as u8;
        self
    }
}

/// Determine which button is pressed based on ADC value for ADC pin 1
/// Returns button index or None if no button pressed
#[allow(dead_code)]
fn get_button_from_adc_1(adc_value: i32) -> Option<Button> {
    for i in 0..4 {
        if ADC_RANGES_1[i + 1] < adc_value && adc_value <= ADC_RANGES_1[i] {
            return match i {
                0 => Some(Button::Back),
                1 => Some(Button::Confirm),
                2 => Some(Button::Left),
                3 => Some(Button::Right),
                _ => None,
            };
        }
    }
    None
}

/// Determine which button is pressed based on ADC value for ADC pin 2
/// Returns button index or None if no button pressed
#[allow(dead_code)]
fn get_button_from_adc_2(adc_value: i32) -> Option<Button> {
    for i in 0..2 {
        if ADC_RANGES_2[i + 1] < adc_value && adc_value <= ADC_RANGES_2[i] {
            return match i {
                0 => Some(Button::Up),
                1 => Some(Button::Down),
                _ => None,
            };
        }
    }
    None
}

/// Button manager
pub struct Buttons<P1, P2, PP> {
    adc_pin1: P1,
    adc_pin2: P2,
    power_pin: PP,
}

impl<P1, P2, PP> Buttons<P1, P2, PP>
where
    P1: embedded_hal::digital::InputPin,
    P2: embedded_hal::digital::InputPin,
    PP: embedded_hal::digital::InputPin,
{
    /// Create a new button manager
    pub fn new(adc_pin1: P1, adc_pin2: P2, power_pin: PP) -> Self {
        Self {
            adc_pin1,
            adc_pin2,
            power_pin,
        }
    }

    /// Read the current button state
    /// Note: For a proper implementation, we would need to use ADC to read the analog values.
    /// For this MVP, we use digital reads which will detect any button press.
    pub fn read_state(&mut self) -> ButtonState {
        let mut state = ButtonState::default();
        
        // Check ADC pins for low (button pressed pulls low typically)
        if self.adc_pin1.is_low().unwrap_or(false) {
            // Some button on ADC1 is pressed - we can't tell which without ADC
            // For MVP, just mark as "any" (we'll use Back as placeholder)
            state = state.with_button(Button::Back);
        }
        
        if self.adc_pin2.is_low().unwrap_or(false) {
            // Some button on ADC2 is pressed
            state = state.with_button(Button::Up);
        }
        
        // Power button is active LOW with pull-up
        if self.power_pin.is_low().unwrap_or(false) {
            state = state.with_button(Button::Power);
        }
        
        state
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_usb_detect_gpio_is_20() {
        assert_eq!(USB_DETECT_GPIO, 20);
    }
    
    #[test]
    fn test_wakeup_reason_variants() {
        // Ensure all variants are distinct
        assert_ne!(WakeupReason::PowerButton, WakeupReason::AfterFlash);
        assert_ne!(WakeupReason::AfterFlash, WakeupReason::AfterUSBPower);
        assert_ne!(WakeupReason::AfterUSBPower, WakeupReason::Other);
    }
    
    #[test]
    fn test_button_state_default_empty() {
        let state = ButtonState::default();
        assert!(!state.any_pressed());
    }
    
    #[test]
    fn test_button_state_with_button() {
        let state = ButtonState::default().with_button(Button::Power);
        assert!(state.is_pressed(Button::Power));
        assert!(!state.is_pressed(Button::Back));
        assert!(state.any_pressed());
    }
    
    #[test]
    fn test_button_state_multiple_buttons() {
        let state = ButtonState::default()
            .with_button(Button::Power)
            .with_button(Button::Confirm);
        assert!(state.is_pressed(Button::Power));
        assert!(state.is_pressed(Button::Confirm));
        assert!(!state.is_pressed(Button::Back));
        assert!(state.any_pressed());
    }
    
    #[test]
    fn test_adc_1_no_button() {
        assert_eq!(get_button_from_adc_1(4000), None);
        assert_eq!(get_button_from_adc_1(3900), None);
    }
    
    #[test]
    fn test_adc_1_back_button() {
        // Back is between 3100 and 3800
        assert_eq!(get_button_from_adc_1(3500), Some(Button::Back));
        assert_eq!(get_button_from_adc_1(3200), Some(Button::Back));
    }
    
    #[test]
    fn test_adc_1_confirm_button() {
        // Confirm is between 2090 and 3100
        assert_eq!(get_button_from_adc_1(2700), Some(Button::Confirm));
        assert_eq!(get_button_from_adc_1(2200), Some(Button::Confirm));
    }
    
    #[test]
    fn test_adc_1_left_button() {
        // Left is between 750 and 2090
        assert_eq!(get_button_from_adc_1(1500), Some(Button::Left));
        assert_eq!(get_button_from_adc_1(1000), Some(Button::Left));
    }
    
    #[test]
    fn test_adc_1_right_button() {
        // Right is below 750
        assert_eq!(get_button_from_adc_1(500), Some(Button::Right));
        assert_eq!(get_button_from_adc_1(5), Some(Button::Right));
    }
    
    #[test]
    fn test_adc_2_no_button() {
        assert_eq!(get_button_from_adc_2(4000), None);
        assert_eq!(get_button_from_adc_2(3900), None);
    }
    
    #[test]
    fn test_adc_2_up_button() {
        // Up is between 1120 and 3800
        assert_eq!(get_button_from_adc_2(2200), Some(Button::Up));
        assert_eq!(get_button_from_adc_2(1500), Some(Button::Up));
    }
    
    #[test]
    fn test_adc_2_down_button() {
        // Down is below 1120
        assert_eq!(get_button_from_adc_2(500), Some(Button::Down));
        assert_eq!(get_button_from_adc_2(5), Some(Button::Down));
    }
}
