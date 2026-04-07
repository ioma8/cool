use minifb::{Key, KeyRepeat, Window};
use xteink_buttons::Button;

pub fn map_key(key: Key) -> Option<Button> {
    match key {
        Key::Left => Some(Button::Left),
        Key::Right => Some(Button::Right),
        Key::Up => Some(Button::Up),
        Key::Down => Some(Button::Down),
        Key::Enter => Some(Button::Back),
        Key::Backspace => Some(Button::Confirm),
        Key::Escape => Some(Button::Power),
        _ => None,
    }
}

pub fn pressed_buttons(window: &Window) -> Vec<Button> {
    window
        .get_keys_pressed(KeyRepeat::No)
        .into_iter()
        .filter_map(map_key)
        .collect()
}
