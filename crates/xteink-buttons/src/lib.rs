#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

pub const ADC_MAX: u16 = 4095;
pub const ADC_NO_BUTTON: i32 = 3800;

// Recorded on real devices (same ladder method as legacy C firmware):
// BACK CONF LEFT RGHT   UP DOWN
// 3597 2760 1530    6 2300    6
// 3470 2666 1480    6 2222    5
// 3470 2655 1470    3 2205    3
// Averages:
// BACK CONF LEFT RGHT   UP DOWN
// 3512 2694 1493    5 2242    5
// Midpoint bands are used below to keep the mapping tolerant across devices.
const ADC_RANGES_1: [i32; 5] = [ADC_NO_BUTTON, 3100, 2090, 750, -1];
const ADC_RANGES_2: [i32; 3] = [ADC_NO_BUTTON, 1120, -1];

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonState {
    pub state: u8,
}

impl ButtonState {
    pub fn is_pressed(&self, button: Button) -> bool {
        (self.state & (1 << button as u8)) != 0
    }

    pub fn any_pressed(&self) -> bool {
        self.state != 0
    }

    pub fn with_button(mut self, button: Button) -> Self {
        self.state |= 1 << button as u8;
        self
    }

    pub fn first_pressed(&self) -> Option<Button> {
        if self.is_pressed(Button::Back) {
            return Some(Button::Back);
        }
        if self.is_pressed(Button::Confirm) {
            return Some(Button::Confirm);
        }
        if self.is_pressed(Button::Left) {
            return Some(Button::Left);
        }
        if self.is_pressed(Button::Right) {
            return Some(Button::Right);
        }
        if self.is_pressed(Button::Up) {
            return Some(Button::Up);
        }
        if self.is_pressed(Button::Down) {
            return Some(Button::Down);
        }
        if self.is_pressed(Button::Power) {
            return Some(Button::Power);
        }
        None
    }
}

pub fn get_button_from_adc_1(adc_value: u16) -> Option<Button> {
    let value = adc_value as i32;
    map_from_ranges(&ADC_RANGES_1, value)
}

pub fn get_button_from_adc_2(adc_value: u16) -> Option<Button> {
    let value = adc_value as i32;
    map_from_ranges(&ADC_RANGES_2, value)
}

fn map_from_ranges(ranges: &[i32], value: i32) -> Option<Button> {
    for i in 0..(ranges.len() - 1) {
        if ranges[i + 1] < value && value <= ranges[i] {
            return match (ranges.len(), i) {
                (5, 0) => Some(Button::Left),
                (5, 1) => Some(Button::Confirm),
                (5, 2) => Some(Button::Right),
                (5, 3) => Some(Button::Back),
                (3, 0) => Some(Button::Down),
                (3, 1) => Some(Button::Up),
                _ => None,
            };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpio1_thresholds_map_to_expected_buttons() {
        assert_eq!(get_button_from_adc_1(3500), Some(Button::Left));
        assert_eq!(get_button_from_adc_1(2700), Some(Button::Confirm));
        assert_eq!(get_button_from_adc_1(1500), Some(Button::Back));
        assert_eq!(get_button_from_adc_1(500), Some(Button::Right));
    }

    #[test]
    fn gpio2_thresholds_map_to_expected_buttons() {
        assert_eq!(get_button_from_adc_2(2200), Some(Button::Up));
        assert_eq!(get_button_from_adc_2(500), Some(Button::Down));
    }

    #[test]
    fn adc_low_extreme_values_map_to_expected_endpoints() {
        assert_eq!(get_button_from_adc_1(0), Some(Button::Right));
        assert_eq!(get_button_from_adc_2(0), Some(Button::Down));
    }

    #[test]
    fn button_state_tracks_and_prioritizes_pressed_buttons() {
        let state = ButtonState::default()
            .with_button(Button::Power)
            .with_button(Button::Confirm);

        assert!(state.is_pressed(Button::Power));
        assert!(state.is_pressed(Button::Confirm));
        assert_eq!(state.first_pressed(), Some(Button::Confirm));
    }

    #[test]
    fn no_button_thresholds_return_none() {
        assert_eq!(get_button_from_adc_1(4095), None);
        assert_eq!(get_button_from_adc_2(4095), None);
    }

    #[test]
    fn high_adc_band_is_not_a_front_button_press() {
        assert_eq!(get_button_from_adc_1(3866), None);
        assert_eq!(get_button_from_adc_2(3866), None);
    }
}
