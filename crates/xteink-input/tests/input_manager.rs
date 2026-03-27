use xteink_buttons::Button as RawButton;
use xteink_input::{
    browse_command_from_buttons, ButtonNavigator, ButtonState, BrowseCommand, InputManager,
    MappedInputManager,
};

#[test]
fn debounce_emits_press_and_release_edges() {
    let mut input = InputManager::new();

    input.update(ButtonState::default().with_button(RawButton::Left), 0);
    input.update(
        ButtonState::default().with_button(RawButton::Left),
        InputManager::debounce_delay_ms() - 1,
    );
    assert!(!input.was_pressed(RawButton::Left));
    assert!(!input.was_released(RawButton::Left));

    input.update(
        ButtonState::default().with_button(RawButton::Left),
        InputManager::debounce_delay_ms() + 1,
    );
    assert!(input.was_pressed(RawButton::Left));
    assert!(input.is_pressed(RawButton::Left));

    input.update(ButtonState::default(), InputManager::debounce_delay_ms() + 2);
    input.update(ButtonState::default(), InputManager::debounce_delay_ms() * 2 + 3);
    assert!(input.was_released(RawButton::Left));
    assert!(!input.is_pressed(RawButton::Left));
}

#[test]
fn raw_input_manager_tracks_held_time() {
    let mut input = InputManager::new();

    input.update(ButtonState::default().with_button(RawButton::Power), 10);
    input.update(ButtonState::default().with_button(RawButton::Power), 16);

    assert!(input.is_pressed(RawButton::Power));
    assert_eq!(input.held_time(22), 6);
}

#[test]
fn input_manager_handles_multiple_raw_buttons() {
    let mut manager = InputManager::new();
    manager.update(ButtonState::default().with_button(RawButton::Back), 0);
    manager.update(
        ButtonState::default()
            .with_button(RawButton::Back)
            .with_button(RawButton::Power),
        InputManager::debounce_delay_ms() + 1,
    );

    assert!(manager.is_pressed(RawButton::Back));
    assert!(manager.is_pressed(RawButton::Power));
    assert_eq!(manager.state(), ButtonState::default().with_button(RawButton::Back).with_button(RawButton::Power));
}

#[test]
fn button_navigator_pages_by_screenfuls_and_wraps() {
    assert_eq!(ButtonNavigator::next_page_index(0, 10, 4), 4);
    assert_eq!(ButtonNavigator::next_page_index(8, 10, 4), 0);
    assert_eq!(ButtonNavigator::previous_page_index(4, 10, 4), 0);
    assert_eq!(ButtonNavigator::previous_page_index(0, 10, 4), 8);
}

#[test]
fn browse_mode_confirm_opens_selected_and_back_goes_up() {
    let confirm = ButtonState::default().with_button(RawButton::Confirm);
    assert_eq!(browse_command_from_buttons(confirm, false), BrowseCommand::OpenSelected);

    let back = ButtonState::default().with_button(RawButton::Back);
    assert_eq!(browse_command_from_buttons(back, false), BrowseCommand::GoUp);
    assert_eq!(browse_command_from_buttons(back, true), BrowseCommand::None);
}

#[test]
fn mapped_manager_is_identity_for_raw_inputs() {
    let mut mapped = MappedInputManager::new();
    mapped.update(
        ButtonState::default()
            .with_button(RawButton::Up)
            .with_button(RawButton::Down),
        InputManager::debounce_delay_ms() + 1,
    );
    mapped.update(
        ButtonState::default()
            .with_button(RawButton::Up)
            .with_button(RawButton::Down),
        InputManager::debounce_delay_ms() * 2 + 2,
    );

    assert!(mapped.is_pressed(RawButton::Up));
    assert!(mapped.is_pressed(RawButton::Down));
}
