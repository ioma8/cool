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

// ADC thresholds for button detection (from C reference InputManager.cpp)
// Button ADC values (recorded from real devices):
// BACK: ~3512, CONFIRM: ~2694, LEFT: ~1493, RIGHT: ~5, UP: ~2242, DOWN: ~5
// These ranges are midpoints between values
// C code uses analogSetAttenuation(ADC_11db) for full range
pub const ADC_NO_BUTTON: u16 = 3800;
const ADC_RANGES_1: [u16; 5] = [ADC_NO_BUTTON, 3100, 2090, 750, 0]; // BACK, CONFIRM, LEFT, RIGHT
const ADC_RANGES_2: [u16; 3] = [ADC_NO_BUTTON, 1120, 0]; // UP, DOWN

/// Button indices (matching C BTN_* constants from HalGPIO.h)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Button {
    Back = 0,
    Confirm = 1,
    Left = 2,
    Right = 3,
    Up = 4,
    Down = 5,
    Power = 6,
}

impl Button {
    /// Get button name as string
    pub fn name(&self) -> &'static str {
        match self {
            Button::Back => "Back",
            Button::Confirm => "Confirm",
            Button::Left => "Left",
            Button::Right => "Right",
            Button::Up => "Up",
            Button::Down => "Down",
            Button::Power => "Power",
        }
    }
}

/// Button state - tracks which buttons are currently pressed
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonState {
    pub state: u8,
}

impl ButtonState {
    /// Check if a specific button is pressed
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
    
    /// Get the first pressed button (if any)
    pub fn first_pressed(&self) -> Option<Button> {
        if self.is_pressed(Button::Back) { return Some(Button::Back); }
        if self.is_pressed(Button::Confirm) { return Some(Button::Confirm); }
        if self.is_pressed(Button::Left) { return Some(Button::Left); }
        if self.is_pressed(Button::Right) { return Some(Button::Right); }
        if self.is_pressed(Button::Up) { return Some(Button::Up); }
        if self.is_pressed(Button::Down) { return Some(Button::Down); }
        if self.is_pressed(Button::Power) { return Some(Button::Power); }
        None
    }
}

/// Determine which button is pressed based on ADC value for ADC pin 1
/// Returns button or None if no button pressed
/// Uses range detection: ranges[i+1] < adcValue <= ranges[i]
pub fn get_button_from_adc_1(adc_value: u16) -> Option<Button> {
    // Check if above threshold (no button)
    if adc_value > ADC_NO_BUTTON {
        return None;
    }
    
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
/// Returns button or None if no button pressed
pub fn get_button_from_adc_2(adc_value: u16) -> Option<Button> {
    // Check if above threshold (no button)
    if adc_value > ADC_NO_BUTTON {
        return None;
    }
    
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
    
    #[test]
    fn test_button_first_pressed() {
        let state = ButtonState::default()
            .with_button(Button::Confirm)
            .with_button(Button::Up);
        // First pressed should be Confirm (lower index)
        assert_eq!(state.first_pressed(), Some(Button::Confirm));
    }
    
    #[test]
    fn test_button_name() {
        assert_eq!(Button::Back.name(), "Back");
        assert_eq!(Button::Confirm.name(), "Confirm");
        assert_eq!(Button::Left.name(), "Left");
        assert_eq!(Button::Right.name(), "Right");
        assert_eq!(Button::Up.name(), "Up");
        assert_eq!(Button::Down.name(), "Down");
        assert_eq!(Button::Power.name(), "Power");
    }
}
