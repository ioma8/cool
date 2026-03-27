#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

pub use xteink_buttons::{Button, ButtonState};

#[derive(Debug, Clone)]
pub struct InputManager {
    current_state: ButtonState,
    pending_state: ButtonState,
    pending_since_ms: u32,
    pressed_events: ButtonState,
    released_events: ButtonState,
    last_state: ButtonState,
    button_press_start: u32,
    button_press_finish: u32,
}

impl InputManager {
    pub const DEBOUNCE_DELAY_MS: u32 = 1;

    pub fn new() -> Self {
        Self {
            current_state: ButtonState::default(),
            pending_state: ButtonState::default(),
            pending_since_ms: 0,
            pressed_events: ButtonState::default(),
            released_events: ButtonState::default(),
            last_state: ButtonState::default(),
            button_press_start: 0,
            button_press_finish: 0,
        }
    }

    pub const fn debounce_delay_ms() -> u32 {
        Self::DEBOUNCE_DELAY_MS
    }

    pub fn update(&mut self, state: ButtonState, now_ms: u32) {
        self.pressed_events = ButtonState::default();
        self.released_events = ButtonState::default();

        if state.state != self.last_state.state {
            if (state.state & self.last_state.state) == 0 {
                self.pending_since_ms = now_ms;
            }
            self.last_state = state;
            self.pending_state = state;
        }

        let stable = now_ms.saturating_sub(self.pending_since_ms) >= Self::DEBOUNCE_DELAY_MS;
        if stable && self.pending_state.state != self.current_state.state {
            self.pressed_events = ButtonState {
                state: self.pending_state.state & !self.current_state.state,
            };
            self.released_events = ButtonState {
                state: self.current_state.state & !self.pending_state.state,
            };

            if self.pressed_events.any_pressed() && !self.current_state.any_pressed() {
                self.button_press_start = now_ms;
            }

            if self.released_events.any_pressed() && !state.any_pressed() {
                self.button_press_finish = now_ms;
            }

            self.current_state = self.pending_state;
        }
    }

    pub fn is_pressed(&self, button: Button) -> bool {
        self.current_state.is_pressed(button)
    }

    pub fn was_pressed(&self, button: Button) -> bool {
        self.pressed_events.is_pressed(button)
    }

    pub fn was_released(&self, button: Button) -> bool {
        self.released_events.is_pressed(button)
    }

    pub fn was_any_pressed(&self) -> bool {
        self.pressed_events.any_pressed()
    }

    pub fn was_any_released(&self) -> bool {
        self.released_events.any_pressed()
    }

    pub fn held_time(&self, now_ms: u32) -> u32 {
        if self.current_state.any_pressed() {
            now_ms.saturating_sub(self.button_press_start)
        } else {
            self.button_press_finish.saturating_sub(self.button_press_start)
        }
    }

    pub fn state(&self) -> ButtonState {
        self.current_state
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}
