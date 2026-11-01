use super::*;

/// The shortcut is `.` with the command key, and not a repeat.
#[test]
fn the_key_is_command_period() {
    let press = |key: &str, modifiers, repeat| {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character(key.into()),
            modified_key: Key::Character(key.into()),
            physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat,
        })
    };
    let id = iced::window::Id::unique();
    let captured = iced::event::Status::Captured;
    let command = keyboard::Modifiers::COMMAND;
    assert!(matches!(
        key(press(".", command, false), captured, id),
        Some(Message::Panic)
    ));
    assert!(key(press(".", command, true), captured, id).is_none(), "no repeat");
    assert!(key(press(".", keyboard::Modifiers::empty(), false), captured, id).is_none());
    assert!(key(press(",", command, false), captured, id).is_none());
}
