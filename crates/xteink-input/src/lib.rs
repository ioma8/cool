#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

pub use xteink_buttons::{Button, ButtonState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseCommand {
    None,
    OpenSelected,
    GoUp,
}

pub fn browse_command_from_buttons(state: ButtonState, at_root: bool) -> BrowseCommand {
    if state.is_pressed(Button::Confirm) {
        return BrowseCommand::OpenSelected;
    }
    if state.is_pressed(Button::Back) && !at_root {
        return BrowseCommand::GoUp;
    }
    BrowseCommand::None
}

#[derive(Debug, Clone)]
pub struct InputManager {
    current_state: ButtonState,
    last_state: ButtonState,
    pressed_events: ButtonState,
    released_events: ButtonState,
    last_debounce_time: u32,
    button_press_start: u32,
    button_press_finish: u32,
}

impl InputManager {
    pub const DEBOUNCE_DELAY_MS: u32 = 5;

    pub fn new() -> Self {
        Self {
            current_state: ButtonState::default(),
            last_state: ButtonState::default(),
            pressed_events: ButtonState::default(),
            released_events: ButtonState::default(),
            last_debounce_time: 0,
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
            self.last_state = state;
            self.last_debounce_time = now_ms;
        }

        if now_ms.saturating_sub(self.last_debounce_time) > Self::DEBOUNCE_DELAY_MS
            && state.state != self.current_state.state
        {
            self.pressed_events = ButtonState {
                state: state.state & !self.current_state.state,
            };
            self.released_events = ButtonState {
                state: self.current_state.state & !state.state,
            };

            if self.pressed_events.any_pressed() && !self.current_state.any_pressed() {
                self.button_press_start = now_ms;
            }

            if self.released_events.any_pressed() && !state.any_pressed() {
                self.button_press_finish = now_ms;
            }

            self.current_state = state;
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

#[derive(Debug, Default, Clone, Copy)]
pub struct ButtonNavigator;

impl ButtonNavigator {
    pub fn next_index(current_index: usize, total_items: usize) -> usize {
        if total_items == 0 {
            return 0;
        }
        (current_index + 1) % total_items
    }

    pub fn previous_index(current_index: usize, total_items: usize) -> usize {
        if total_items == 0 {
            return 0;
        }
        (current_index + total_items - 1) % total_items
    }

    pub fn next_page_index(current_index: usize, total_items: usize, items_per_page: usize) -> usize {
        if total_items == 0 || items_per_page == 0 {
            return 0;
        }
        if total_items <= items_per_page {
            return Self::next_index(current_index, total_items);
        }

        let last_page_index = (total_items - 1) / items_per_page;
        let current_page_index = current_index / items_per_page;
        if current_page_index < last_page_index {
            (current_page_index + 1) * items_per_page
        } else {
            0
        }
    }

    pub fn previous_page_index(current_index: usize, total_items: usize, items_per_page: usize) -> usize {
        if total_items == 0 || items_per_page == 0 {
            return 0;
        }
        if total_items <= items_per_page {
            return Self::previous_index(current_index, total_items);
        }

        let last_page_index = (total_items - 1) / items_per_page;
        let current_page_index = current_index / items_per_page;
        if current_page_index > 0 {
            (current_page_index - 1) * items_per_page
        } else {
            last_page_index * items_per_page
        }
    }
}
