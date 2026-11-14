use super::*;
use winit::event::{DeviceId, WindowEvent as W};
use winit::keyboard::SmolStr;

/// The events for `event` at scale 2, from a fresh state.
fn at_scale_2(event: W) -> (Vec<DriverEvent>, Held) {
    let mut held = Held::default();
    let events = driver_events(&event, 2.0, &mut held);
    (events, held)
}

/// A pointer move is in logical pixels, and the shell keeps where it is.
#[test]
fn a_pointer_move_is_in_logical_pixels_and_is_kept() {
    let (events, held) = at_scale_2(W::CursorMoved {
        device_id: DeviceId::dummy(),
        position: PhysicalPosition::new(200.0, 100.0),
    });
    let at = Some(Point::new(100.0, 50.0));
    assert_eq!(
        events,
        [DriverEvent::Input {
            event: Event::Pointer(PointerEvent::Moved),
            pointer: at,
        }]
    );
    assert_eq!(held.pointer, at);
}

/// A press and a release carry the pointer where it was last seen, and
/// leaving the window forgets it.
#[test]
fn buttons_carry_the_pointer_and_leaving_forgets_it() {
    let mut held = Held {
        pointer: Some(Point::new(4.0, 5.0)),
        modifiers: Modifiers::NONE,
    };
    for (state, expected) in [
        (ElementState::Pressed, PointerEvent::Pressed(PointerButton::Right)),
        (ElementState::Released, PointerEvent::Released(PointerButton::Right)),
    ] {
        let event = W::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button: MouseButton::Right,
        };
        assert_eq!(
            driver_events(&event, 1.0, &mut held),
            [DriverEvent::Input {
                event: Event::Pointer(expected),
                pointer: Some(Point::new(4.0, 5.0)),
            }]
        );
    }
    let left = driver_events(
        &W::CursorLeft {
            device_id: DeviceId::dummy(),
        },
        1.0,
        &mut held,
    );
    assert_eq!(
        left,
        [DriverEvent::Input {
            event: Event::Pointer(PointerEvent::LeftWindow),
            pointer: None,
        }]
    );
    assert_eq!(held.pointer, None);
    let entered = driver_events(
        &W::CursorEntered {
            device_id: DeviceId::dummy(),
        },
        1.0,
        &mut held,
    );
    assert_eq!(
        entered,
        [DriverEvent::Input {
            event: Event::Pointer(PointerEvent::EnteredWindow),
            pointer: None,
        }]
    );
}

/// Every pointer button has ours; the side buttons share one.
#[test]
fn every_button_has_ours() {
    assert_eq!(button(MouseButton::Left), PointerButton::Left);
    assert_eq!(button(MouseButton::Right), PointerButton::Right);
    assert_eq!(button(MouseButton::Middle), PointerButton::Middle);
    for side in [MouseButton::Back, MouseButton::Forward, MouseButton::Other(7)] {
        assert_eq!(button(side), PointerButton::Other);
    }
}

/// A wheel scrolls in lines, as given; a touch pad in logical pixels.
#[test]
fn a_scroll_is_in_lines_or_logical_pixels() {
    assert_eq!(
        scroll(MouseScrollDelta::LineDelta(0.0, 1.0), 2.0),
        ScrollDelta::Lines { x: 0.0, y: 1.0 }
    );
    assert_eq!(
        scroll(MouseScrollDelta::PixelDelta(PhysicalPosition::new(10.0, -20.0)), 2.0),
        ScrollDelta::Pixels { x: 5.0, y: -10.0 }
    );
    let (events, _) = at_scale_2(W::MouseWheel {
        device_id: DeviceId::dummy(),
        delta: MouseScrollDelta::LineDelta(0.0, -2.0),
        phase: winit::event::TouchPhase::Moved,
    });
    assert_eq!(
        events,
        [DriverEvent::Input {
            event: Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: -2.0 })),
            pointer: None,
        }]
    );
}

/// The window's own events go to the runtime as window events, a resize in
/// logical pixels.
#[test]
fn window_events_go_as_window_events() {
    let cases = [
        (
            W::Resized(PhysicalSize::new(800, 600)),
            WindowEvent::Resized(Size::new(400.0, 300.0)),
        ),
        (W::Focused(true), WindowEvent::Focused(true)),
        (W::CloseRequested, WindowEvent::CloseRequested),
        (
            W::DroppedFile("patch.ron".into()),
            WindowEvent::FileDropped("patch.ron".into()),
        ),
    ];
    for (event, expected) in cases {
        assert_eq!(at_scale_2(event).0, [DriverEvent::Window(expected)]);
    }
}

/// Losing focus is one window event. The runtime tells the elements, so
/// the shell must not send `WindowUnfocused` too, or they would hear it
/// twice.
#[test]
fn losing_focus_is_one_window_event() {
    assert_eq!(
        at_scale_2(W::Focused(false)).0,
        [DriverEvent::Window(WindowEvent::Focused(false))]
    );
}

/// A redraw request gives no event here: the redraw rule decides.
#[test]
fn a_redraw_request_gives_no_event() {
    assert!(at_scale_2(W::RedrawRequested).0.is_empty());
}

/// A change of modifiers is kept, and sent as an element event.
#[test]
fn a_change_of_modifiers_is_kept_and_sent() {
    let state = ModifiersState::SHIFT | ModifiersState::SUPER;
    let (events, held) = at_scale_2(W::ModifiersChanged(state.into()));
    let expected = Modifiers {
        shift: true,
        meta: true,
        ..Modifiers::NONE
    };
    assert_eq!(held.modifiers, expected);
    assert_eq!(
        events,
        [DriverEvent::Input {
            event: Event::Key(KeyEvent::ModifiersChanged(expected)),
            pointer: None,
        }]
    );
    assert_eq!(
        modifiers(ModifiersState::CONTROL | ModifiersState::ALT),
        Modifiers {
            control: true,
            alt: true,
            ..Modifiers::NONE
        }
    );
}

/// Every named key the elements use has ours, the modifier keys share
/// one, and the rest are other.
#[test]
fn named_keys_map_to_ours() {
    let cases = [
        (NamedKey::ArrowUp, Key::Up),
        (NamedKey::ArrowDown, Key::Down),
        (NamedKey::ArrowLeft, Key::Left),
        (NamedKey::ArrowRight, Key::Right),
        (NamedKey::Enter, Key::Enter),
        (NamedKey::Escape, Key::Escape),
        (NamedKey::Space, Key::Space),
        (NamedKey::Tab, Key::Tab),
        (NamedKey::Backspace, Key::Backspace),
        (NamedKey::Delete, Key::Delete),
        (NamedKey::Home, Key::Home),
        (NamedKey::End, Key::End),
        (NamedKey::ContextMenu, Key::ContextMenu),
        (NamedKey::F10, Key::F10),
        (NamedKey::Shift, Key::Modifier),
        (NamedKey::Control, Key::Modifier),
        (NamedKey::Alt, Key::Modifier),
        (NamedKey::AltGraph, Key::Modifier),
        (NamedKey::Meta, Key::Modifier),
        (NamedKey::Super, Key::Modifier),
        (NamedKey::F1, Key::Other),
        (NamedKey::PageDown, Key::Other),
    ];
    for (named, expected) in cases {
        assert_eq!(core_key(&WinitKey::Named(named)), expected, "{named:?}");
    }
}

/// A key that types one character is that character; one that types
/// more, a dead key, or an unknown one, is other.
#[test]
fn character_keys_are_one_character() {
    assert_eq!(core_key(&WinitKey::Character(SmolStr::new("z"))), Key::Character('z'));
    assert_eq!(core_key(&WinitKey::Character(SmolStr::new("ab"))), Key::Other);
    assert_eq!(core_key(&WinitKey::Dead(Some('`'))), Key::Other);
    assert_eq!(
        core_key(&WinitKey::Unidentified(winit::keyboard::NativeKey::Unidentified)),
        Key::Other
    );
}

/// A press carries its modifiers, repeat, and text; a release only its
/// key; a press winit made up on focus gives nothing.
#[test]
fn presses_and_releases_carry_what_they_should() {
    let z = WinitKey::Character(SmolStr::new("z"));
    let meta = Modifiers {
        meta: true,
        ..Modifiers::NONE
    };
    assert_eq!(
        key(&z, ElementState::Pressed, true, Some("z"), false, meta),
        Some(Event::Key(KeyEvent::Pressed {
            key: Key::Character('z'),
            modifiers: meta,
            repeat: true,
            text: Some("z".into()),
        }))
    );
    assert_eq!(
        key(&z, ElementState::Released, false, None, false, meta),
        Some(Event::Key(KeyEvent::Released {
            key: Key::Character('z')
        }))
    );
    assert_eq!(key(&z, ElementState::Pressed, false, Some("z"), true, meta), None);
}

/// Positions and sizes divide by the scale.
#[test]
fn positions_and_sizes_divide_by_the_scale() {
    assert_eq!(point(PhysicalPosition::new(3.0, 9.0), 1.5), Point::new(2.0, 6.0));
    assert_eq!(logical(PhysicalSize::new(300, 150), 1.5), Size::new(200.0, 100.0));
}

/// A display's rate in millihertz gives its frame period; a display with
/// no rate, or zero, keeps the 16 ms default.
#[test]
fn a_displays_rate_gives_its_frame() {
    assert_eq!(frame_period(Some(60_000)), Duration::from_secs_f64(1.0 / 60.0));
    assert_eq!(frame_period(Some(120_000)), Duration::from_secs_f64(1.0 / 120.0));
    assert_eq!(frame_period(None), crate::event::WindowState::FRAME);
    assert_eq!(frame_period(Some(0)), crate::event::WindowState::FRAME);
}
