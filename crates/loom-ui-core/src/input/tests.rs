//! Tests for the input types, each against the rule its doc comment states.

use super::*;

/// One modifier held, by name, to keep tests short.
fn held(shift: bool, control: bool, alt: bool, meta: bool) -> Modifiers {
    Modifiers {
        shift,
        control,
        alt,
        meta,
    }
}

/// `NONE` and the default hold nothing, and `none` says so.
#[test]
fn no_modifiers_is_the_default() {
    assert_eq!(Modifiers::default(), Modifiers::NONE);
    assert!(Modifiers::NONE.none());
}

/// Any one modifier held makes `none` false.
#[test]
fn any_held_modifier_is_not_none() {
    for one in [
        held(true, false, false, false),
        held(false, true, false, false),
        held(false, false, true, false),
        held(false, false, false, true),
    ] {
        assert!(!one.none(), "{one:?}");
    }
}

/// On a Mac the shortcut key is Command, and Control alone is not one.
#[test]
fn the_shortcut_key_on_a_mac_is_command() {
    let command = held(false, false, false, true);
    let control = held(false, true, false, false);
    assert!(command.shortcut_on(Platform::Mac));
    assert!(!control.shortcut_on(Platform::Mac));
}

/// Elsewhere the shortcut key is Control, and the system key alone is not
/// one.
#[test]
fn the_shortcut_key_elsewhere_is_control() {
    let command = held(false, false, false, true);
    let control = held(false, true, false, false);
    assert!(control.shortcut_on(Platform::Other));
    assert!(!command.shortcut_on(Platform::Other));
}

/// Other modifiers held beside the shortcut key do not change the answer,
/// and Shift or Alt alone is never the shortcut key.
#[test]
fn other_modifiers_do_not_change_the_shortcut_answer() {
    for platform in [Platform::Mac, Platform::Other] {
        assert!(held(true, true, true, true).shortcut_on(platform));
        assert!(!held(true, false, true, false).shortcut_on(platform));
        assert!(!Modifiers::NONE.shortcut_on(platform));
    }
}

/// `shortcut` gives the answer for the platform the program was built for.
#[test]
fn shortcut_uses_the_current_platform() {
    let both = [held(false, true, false, false), held(false, false, false, true)];
    for modifiers in both {
        assert_eq!(modifiers.shortcut(), modifiers.shortcut_on(Platform::current()));
    }
}

/// The current platform is a Mac exactly when the program is built for
/// macOS.
#[test]
fn the_current_platform_follows_the_build_target() {
    let expected = if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Other
    };
    assert_eq!(Platform::current(), expected);
}

/// Character keys compare by their exact character, so `a` and `A` are
/// different keys.
#[test]
fn character_keys_compare_by_exact_character() {
    assert_eq!(Key::Character('a'), Key::Character('a'));
    assert_ne!(Key::Character('a'), Key::Character('A'));
    assert_ne!(Key::Character('a'), Key::Other);
}

/// A key press keeps its text beside its key, so a key with no name of its
/// own still types.
#[test]
fn a_press_carries_its_text_beside_its_key() {
    let press = KeyEvent::Pressed {
        key: Key::Other,
        modifiers: Modifiers::NONE,
        repeat: false,
        text: Some("ß".to_string()),
    };
    let KeyEvent::Pressed { key, text, .. } = &press else {
        panic!("a press");
    };
    assert_eq!(*key, Key::Other);
    assert_eq!(text.as_deref(), Some("ß"));
}

/// Leaving the window and being hidden are different facts: one is an
/// event, and the other is the absent position beside any event.
#[test]
fn leaving_the_window_is_its_own_event() {
    assert_ne!(PointerEvent::LeftWindow, PointerEvent::Moved);
    assert_ne!(PointerEvent::LeftWindow, PointerEvent::EnteredWindow);
}

/// Events compare by what they hold.
#[test]
fn events_compare_by_content() {
    let lines = Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: 1.0 }));
    let pixels = Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Pixels { x: 0.0, y: 1.0 }));
    assert_eq!(lines.clone(), lines);
    assert_ne!(lines, pixels);
    assert_ne!(
        Event::Pointer(PointerEvent::Pressed(PointerButton::Left)),
        Event::Pointer(PointerEvent::Pressed(PointerButton::Right))
    );
    assert_ne!(Event::Redraw, Event::WindowUnfocused);
}
