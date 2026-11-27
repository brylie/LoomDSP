use super::*;

/// The shortcut is `.` with the command key, and not a repeat.
#[test]
fn the_key_is_command_period() {
    let press = |key: char, modifiers, repeat| {
        Event::Key(KeyEvent::Pressed {
            key: Key::Character(key),
            code: loom_ui_core::Code::Other,
            modifiers,
            text: None,
            repeat,
        })
    };
    let command = Modifiers::platform_shortcut();
    assert!(matches!(key(&press('.', command, false), true), Some(Message::Panic)));
    assert!(key(&press('.', command, true), true).is_none(), "no repeat");
    assert!(key(&press('.', Modifiers::NONE, false), true).is_none());
    assert!(key(&press(',', command, false), true).is_none());
}
