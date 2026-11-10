//! Tests that a value keeps every field on its way to Iced and back.

use super::*;

/// A point keeps both coordinates each way.
#[test]
fn a_point_keeps_its_coordinates() {
    let ours = Point::new(1.5, -2.5);
    let theirs = ours.to_iced();
    assert_eq!((theirs.x, theirs.y), (1.5, -2.5));
    assert_eq!(theirs.to_core(), ours);
}

/// A vector keeps both parts each way.
#[test]
fn a_vector_keeps_its_parts() {
    let ours = Vector::new(-3.0, 4.25);
    let theirs = ours.to_iced();
    assert_eq!((theirs.x, theirs.y), (-3.0, 4.25));
    assert_eq!(theirs.to_core(), ours);
}

/// A size keeps its width and height each way, and does not swap them.
#[test]
fn a_size_keeps_its_width_and_height() {
    let ours = Size::new(640.0, 480.0);
    let theirs = ours.to_iced();
    assert_eq!((theirs.width, theirs.height), (640.0, 480.0));
    assert_eq!(theirs.to_core(), ours);
}

/// A rectangle keeps its corner and its size each way.
#[test]
fn a_rect_keeps_its_corner_and_size() {
    let ours = Rect::new(Point::new(10.0, 20.0), Size::new(30.0, 40.0));
    let theirs = ours.to_iced();
    assert_eq!(
        (theirs.x, theirs.y, theirs.width, theirs.height),
        (10.0, 20.0, 30.0, 40.0)
    );
    assert_eq!(theirs.to_core(), ours);
}

/// A colour keeps its four channels each way, in the same order.
#[test]
fn a_colour_keeps_its_channels() {
    let ours = Color::rgba(0.1, 0.2, 0.3, 0.4);
    let theirs = ours.to_iced();
    assert_eq!((theirs.r, theirs.g, theirs.b, theirs.a), (0.1, 0.2, 0.3, 0.4));
    assert_eq!(theirs.to_core(), ours);
}

use iced::keyboard::key::Named;
use iced::{keyboard, mouse, window};
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, PointerButton, PointerEvent, ScrollDelta};
use std::time::Instant;

/// An Iced key press with the fields the conversion does not read filled in.
fn press(key: keyboard::Key, text: Option<&str>, modifiers: keyboard::Modifiers, repeat: bool) -> iced::Event {
    iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers,
        text: text.map(Into::into),
        repeat,
    })
}

/// An Iced key release.
fn release(key: keyboard::Key) -> iced::Event {
    iced::Event::Keyboard(keyboard::Event::KeyReleased {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
    })
}

/// An Iced key for a string of characters.
fn characters(text: &str) -> keyboard::Key {
    keyboard::Key::Character(text.into())
}

/// Every key the canvas has a use for becomes its own `Key`.
#[test]
fn each_named_key_the_canvas_uses_has_its_own_key() {
    let pairs = [
        (Named::ArrowUp, Key::Up),
        (Named::ArrowDown, Key::Down),
        (Named::ArrowLeft, Key::Left),
        (Named::ArrowRight, Key::Right),
        (Named::Enter, Key::Enter),
        (Named::Escape, Key::Escape),
        (Named::Space, Key::Space),
        (Named::Tab, Key::Tab),
        (Named::Backspace, Key::Backspace),
        (Named::Delete, Key::Delete),
        (Named::Home, Key::Home),
        (Named::End, Key::End),
    ];
    for (named, key) in pairs {
        assert_eq!((&keyboard::Key::Named(named)).to_core(), key, "{named:?}");
    }
    // No two of them share a key.
    for (i, (_, a)) in pairs.iter().enumerate() {
        for (_, b) in &pairs[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

/// Every modifier key, on either side, is the one `Key::Modifier`.
#[test]
fn every_modifier_key_is_the_modifier_key() {
    for named in [
        Named::Shift,
        Named::Control,
        Named::Alt,
        Named::AltGraph,
        Named::Meta,
        Named::Super,
    ] {
        assert_eq!((&keyboard::Key::Named(named)).to_core(), Key::Modifier, "{named:?}");
    }
}

/// A named key the canvas does not use, and a key the system cannot name,
/// are `Key::Other`.
#[test]
fn a_key_with_no_use_is_other() {
    assert_eq!((&keyboard::Key::Named(Named::F5)).to_core(), Key::Other);
    assert_eq!((&keyboard::Key::Named(Named::PageDown)).to_core(), Key::Other);
    assert_eq!((&keyboard::Key::Unidentified).to_core(), Key::Other);
}

/// A key that types one character keeps that character and its case.
#[test]
fn a_one_character_key_keeps_its_character_and_case() {
    for (text, character) in [
        ("f", 'f'),
        ("F", 'F'),
        ("0", '0'),
        ("a", 'a'),
        ("v", 'v'),
        ("V", 'V'),
        ("ß", 'ß'),
    ] {
        assert_eq!((&characters(text)).to_core(), Key::Character(character), "{text}");
    }
}

/// A key that types no character or more than one is `Key::Other`, and its
/// text still arrives in the press.
#[test]
fn a_key_that_types_more_than_one_character_is_other_and_keeps_its_text() {
    assert_eq!((&characters("ab")).to_core(), Key::Other);
    assert_eq!((&characters("")).to_core(), Key::Other);
    let event = event_to_core(&press(
        characters("ab"),
        Some("ab"),
        keyboard::Modifiers::empty(),
        false,
    ));
    assert_eq!(
        event,
        Some(Event::Key(KeyEvent::Pressed {
            key: Key::Other,
            modifiers: Modifiers::NONE,
            repeat: false,
            text: Some("ab".to_string()),
        }))
    );
}

/// Each modifier maps to its own field, alone and together.
#[test]
fn each_modifier_maps_to_its_own_field() {
    let none = Modifiers::NONE;
    assert_eq!(keyboard::Modifiers::empty().to_core(), none);
    assert_eq!(keyboard::Modifiers::SHIFT.to_core(), Modifiers { shift: true, ..none });
    assert_eq!(keyboard::Modifiers::CTRL.to_core(), Modifiers { control: true, ..none });
    assert_eq!(keyboard::Modifiers::ALT.to_core(), Modifiers { alt: true, ..none });
    assert_eq!(keyboard::Modifiers::LOGO.to_core(), Modifiers { meta: true, ..none });
    assert_eq!(
        (keyboard::Modifiers::SHIFT | keyboard::Modifiers::ALT).to_core(),
        Modifiers {
            shift: true,
            alt: true,
            ..none
        }
    );
    assert_eq!(
        keyboard::Modifiers::all().to_core(),
        Modifiers {
            shift: true,
            control: true,
            alt: true,
            meta: true,
        }
    );
}

/// A key press keeps its key, modifiers, repeat flag, and text.
#[test]
fn a_key_press_keeps_every_part() {
    let event = event_to_core(&press(characters("A"), Some("A"), keyboard::Modifiers::SHIFT, true));
    assert_eq!(
        event,
        Some(Event::Key(KeyEvent::Pressed {
            key: Key::Character('A'),
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            repeat: true,
            text: Some("A".to_string()),
        }))
    );
    let no_text = event_to_core(&press(
        keyboard::Key::Named(Named::Enter),
        None,
        keyboard::Modifiers::empty(),
        false,
    ));
    assert_eq!(
        no_text,
        Some(Event::Key(KeyEvent::Pressed {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            repeat: false,
            text: None,
        }))
    );
}

/// A key release and a change of modifiers convert too.
#[test]
fn a_release_and_a_modifier_change_convert() {
    assert_eq!(
        event_to_core(&release(keyboard::Key::Named(Named::Space))),
        Some(Event::Key(KeyEvent::Released { key: Key::Space }))
    );
    assert_eq!(
        event_to_core(&iced::Event::Keyboard(keyboard::Event::ModifiersChanged(
            keyboard::Modifiers::CTRL
        ))),
        Some(Event::Key(KeyEvent::ModifiersChanged(Modifiers {
            control: true,
            ..Modifiers::NONE
        })))
    );
}

/// The three buttons the canvas tells apart keep their names, and every
/// other button is `Other`.
#[test]
fn each_button_converts() {
    assert_eq!(mouse::Button::Left.to_core(), PointerButton::Left);
    assert_eq!(mouse::Button::Right.to_core(), PointerButton::Right);
    assert_eq!(mouse::Button::Middle.to_core(), PointerButton::Middle);
    for other in [mouse::Button::Back, mouse::Button::Forward, mouse::Button::Other(7)] {
        assert_eq!(other.to_core(), PointerButton::Other, "{other:?}");
    }
}

/// Each pointer event converts, and a move drops its position, which
/// travels beside the event.
#[test]
fn each_pointer_event_converts() {
    let pointer = |event: mouse::Event| event_to_core(&iced::Event::Mouse(event));
    assert_eq!(
        pointer(mouse::Event::CursorMoved {
            position: iced::Point::new(3.0, 4.0)
        }),
        Some(Event::Pointer(PointerEvent::Moved))
    );
    assert_eq!(
        pointer(mouse::Event::ButtonPressed(mouse::Button::Right)),
        Some(Event::Pointer(PointerEvent::Pressed(PointerButton::Right)))
    );
    assert_eq!(
        pointer(mouse::Event::ButtonReleased(mouse::Button::Middle)),
        Some(Event::Pointer(PointerEvent::Released(PointerButton::Middle)))
    );
    assert_eq!(
        pointer(mouse::Event::CursorLeft),
        Some(Event::Pointer(PointerEvent::LeftWindow))
    );
    assert_eq!(
        pointer(mouse::Event::CursorEntered),
        Some(Event::Pointer(PointerEvent::EnteredWindow))
    );
}

/// Both scroll units keep their unit, both axes, and their signs.
#[test]
fn both_scroll_units_convert() {
    let scroll = |delta| event_to_core(&iced::Event::Mouse(mouse::Event::WheelScrolled { delta }));
    assert_eq!(
        scroll(mouse::ScrollDelta::Lines { x: 1.0, y: -2.0 }),
        Some(Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines {
            x: 1.0,
            y: -2.0
        })))
    );
    assert_eq!(
        scroll(mouse::ScrollDelta::Pixels { x: -3.5, y: 4.5 }),
        Some(Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Pixels {
            x: -3.5,
            y: 4.5
        })))
    );
}

/// A redraw request and the window losing focus convert. The redraw's time
/// is not part of the event: the caller passes it beside the event.
#[test]
fn the_two_window_events_the_canvas_uses_convert() {
    let window = |event: window::Event| event_to_core(&iced::Event::Window(event));
    assert_eq!(
        window(window::Event::RedrawRequested(Instant::now())),
        Some(Event::Redraw)
    );
    assert_eq!(window(window::Event::Unfocused), Some(Event::WindowUnfocused));
}

/// Window events the canvas has no use for have no conversion.
#[test]
fn other_window_events_have_no_conversion() {
    for event in [
        window::Event::Focused,
        window::Event::Closed,
        window::Event::CloseRequested,
        window::Event::Moved(iced::Point::ORIGIN),
        window::Event::Resized(iced::Size::new(800.0, 600.0)),
        window::Event::Rescaled(2.0),
        window::Event::FilesHoveredLeft,
        window::Event::FileDropped("a.ron".into()),
    ] {
        assert_eq!(event_to_core(&iced::Event::Window(event.clone())), None, "{event:?}");
    }
}

/// Each cursor shape maps to its own Iced cursor, and the default shape to
/// Iced's default.
#[test]
fn each_cursor_shape_has_its_own_iced_cursor() {
    use crate::CursorShape;
    let shapes = [
        CursorShape::Default,
        CursorShape::Grab,
        CursorShape::Grabbing,
        CursorShape::Pointer,
        CursorShape::Crosshair,
        CursorShape::ResizeHorizontal,
        CursorShape::ResizeVertical,
        CursorShape::ResizeDiagonal,
    ];
    assert_eq!(CursorShape::Default.to_iced(), mouse::Interaction::default());
    for (i, a) in shapes.iter().enumerate() {
        for b in &shapes[i + 1..] {
            assert_ne!(a.to_iced(), b.to_iced(), "{a:?} and {b:?}");
        }
    }
}
