#![no_std]

pub const ADC_NO_BUTTON: u16 = 3800;
const ADC_RANGES_1: [u16; 5] = [ADC_NO_BUTTON, 3100, 2090, 750, 0];
const ADC_RANGES_2: [u16; 3] = [ADC_NO_BUTTON, 1120, 0];

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

pub fn get_button_from_adc_2(adc_value: u16) -> Option<Button> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpio1_thresholds_map_to_expected_buttons() {
        assert_eq!(get_button_from_adc_1(3500), Some(Button::Back));
        assert_eq!(get_button_from_adc_1(2700), Some(Button::Confirm));
        assert_eq!(get_button_from_adc_1(1500), Some(Button::Left));
        assert_eq!(get_button_from_adc_1(500), Some(Button::Right));
    }

    #[test]
    fn gpio2_thresholds_map_to_expected_buttons() {
        assert_eq!(get_button_from_adc_2(2200), Some(Button::Up));
        assert_eq!(get_button_from_adc_2(500), Some(Button::Down));
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
        assert_eq!(get_button_from_adc_1(4000), None);
        assert_eq!(get_button_from_adc_2(3900), None);
    }
}
