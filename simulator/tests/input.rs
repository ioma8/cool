use minifb::Key;
use simulator::input::map_key;
use xteink_buttons::Button;

#[test]
fn arrow_enter_backspace_escape_keys_map_to_device_buttons() {
    assert_eq!(map_key(Key::Left), Some(Button::Left));
    assert_eq!(map_key(Key::Right), Some(Button::Right));
    assert_eq!(map_key(Key::Up), Some(Button::Up));
    assert_eq!(map_key(Key::Down), Some(Button::Down));
    assert_eq!(map_key(Key::Enter), Some(Button::Back));
    assert_eq!(map_key(Key::Backspace), Some(Button::Confirm));
    assert_eq!(map_key(Key::Escape), Some(Button::Power));
}
