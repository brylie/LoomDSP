//! winit's events as the runtime's.
//!
//! Plain functions, so a test can check each without a window. winit's
//! `KeyEvent` has a private field, so a test cannot build one: [`key`]
//! takes the parts of it that it reads, and the shell hands them over.
//!
//! Positions and pixel scroll distances come from winit in physical pixels
//! and go to the runtime in logical ones, divided by the window's scale.

use crate::event::{DriverEvent, WindowEvent};
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, ScrollDelta, Size};
use std::time::Duration;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};

/// What the shell keeps between events: where the pointer is, and which
/// modifiers are held. winit reports each only when it changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Held {
    /// Where the pointer is, in logical pixels, or `None` while it is
    /// outside the window.
    pub pointer: Option<Point>,
    /// The modifiers held now.
    pub modifiers: Modifiers,
}

impl Default for Held {
    fn default() -> Self {
        Self {
            pointer: None,
            modifiers: Modifiers::NONE,
        }
    }
}

/// The runtime's events for one winit window event, in order. A key press
/// gives none here: the shell converts it with [`key`], since a test cannot
/// build winit's key event. `RedrawRequested` gives none either: the
/// redraw rule decides what it means (`pacing.rs`).
pub(crate) fn driver_events(event: &winit::event::WindowEvent, scale: f32, held: &mut Held) -> Vec<DriverEvent> {
    use winit::event::WindowEvent as W;
    match event {
        W::CursorMoved { position, .. } => {
            held.pointer = Some(point(*position, scale));
            vec![pointer_event(PointerEvent::Moved, held)]
        }
        W::CursorEntered { .. } => vec![pointer_event(PointerEvent::EnteredWindow, held)],
        W::CursorLeft { .. } => {
            held.pointer = None;
            vec![pointer_event(PointerEvent::LeftWindow, held)]
        }
        W::MouseInput { state, button, .. } => {
            let button = self::button(*button);
            let event = match state {
                ElementState::Pressed => PointerEvent::Pressed(button),
                ElementState::Released => PointerEvent::Released(button),
            };
            vec![pointer_event(event, held)]
        }
        W::MouseWheel { delta, .. } => vec![pointer_event(PointerEvent::Scrolled(scroll(*delta, scale)), held)],
        W::ModifiersChanged(modifiers) => {
            held.modifiers = self::modifiers(modifiers.state());
            vec![input(Event::Key(KeyEvent::ModifiersChanged(held.modifiers)), held)]
        }
        W::Focused(true) => vec![DriverEvent::Window(WindowEvent::Focused(true))],
        // The runtime tells the elements itself when the window loses the
        // keys, so only the window event goes.
        W::Focused(false) => vec![DriverEvent::Window(WindowEvent::Focused(false))],
        W::Resized(size) => vec![DriverEvent::Window(WindowEvent::Resized(logical(*size, scale)))],
        W::ScaleFactorChanged { scale_factor, .. } => {
            vec![DriverEvent::Window(WindowEvent::ScaleChanged(*scale_factor as f32))]
        }
        W::CloseRequested => vec![DriverEvent::Window(WindowEvent::CloseRequested)],
        W::DroppedFile(path) => vec![DriverEvent::Window(WindowEvent::FileDropped(path.clone()))],
        _ => Vec::new(),
    }
}

/// An element event, with the pointer where it is held.
fn input(event: Event, held: &Held) -> DriverEvent {
    DriverEvent::Input {
        event,
        pointer: held.pointer,
    }
}

/// A pointer event, with the pointer where it is held.
fn pointer_event(event: PointerEvent, held: &Held) -> DriverEvent {
    input(Event::Pointer(event), held)
}

/// A key press or release. `key` is the key with no modifier applied, as
/// [`Key::Character`] promises (winit gives it through
/// `KeyEventExtModifierSupplement::key_without_modifiers`), and `text` is
/// what the press types. A press winit made up for a window that gained
/// focus is not a press the user made, and gives `None`.
pub(crate) fn key(
    key: &WinitKey,
    state: ElementState,
    repeat: bool,
    text: Option<&str>,
    synthetic: bool,
    modifiers: Modifiers,
) -> Option<Event> {
    if synthetic {
        return None;
    }
    let key = core_key(key);
    Some(Event::Key(match state {
        ElementState::Pressed => KeyEvent::Pressed {
            key,
            modifiers,
            repeat,
            text: text.map(str::to_string),
        },
        ElementState::Released => KeyEvent::Released { key },
    }))
}

/// winit's key as ours. A key that types exactly one character is
/// [`Key::Character`]; one that types more, or that has no use here, is
/// [`Key::Other`], and its text still reaches the element in the press.
pub(crate) fn core_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Named(named) => match named {
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::Enter => Key::Enter,
            NamedKey::Escape => Key::Escape,
            NamedKey::Space => Key::Space,
            NamedKey::Tab => Key::Tab,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Delete => Key::Delete,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::ContextMenu => Key::ContextMenu,
            NamedKey::F10 => Key::F10,
            NamedKey::Shift
            | NamedKey::Control
            | NamedKey::Alt
            | NamedKey::AltGraph
            | NamedKey::Meta
            | NamedKey::Super => Key::Modifier,
            _ => Key::Other,
        },
        WinitKey::Character(text) => {
            let mut characters = text.chars();
            match (characters.next(), characters.next()) {
                (Some(only), None) => Key::Character(only),
                _ => Key::Other,
            }
        }
        WinitKey::Unidentified(_) | WinitKey::Dead(_) => Key::Other,
    }
}

/// winit's pointer button as ours.
pub(crate) fn button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Left,
        MouseButton::Right => PointerButton::Right,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => PointerButton::Other,
    }
}

/// A scroll, in lines as the wheel gives them, or in logical pixels.
pub(crate) fn scroll(delta: MouseScrollDelta, scale: f32) -> ScrollDelta {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x, y },
        MouseScrollDelta::PixelDelta(position) => ScrollDelta::Pixels {
            x: position.x as f32 / scale,
            y: position.y as f32 / scale,
        },
    }
}

/// A position in physical pixels, in logical ones.
pub(crate) fn point(position: PhysicalPosition<f64>, scale: f32) -> Point {
    Point::new(position.x as f32 / scale, position.y as f32 / scale)
}

/// How long one frame lasts on a display of `millihertz`, as winit gives
/// a monitor's rate. A display that gives no rate, or zero, keeps
/// [`WindowState::FRAME`](crate::event::WindowState::FRAME).
pub(crate) fn frame_period(millihertz: Option<u32>) -> Duration {
    match millihertz {
        Some(rate) if rate > 0 => Duration::from_secs_f64(1000.0 / f64::from(rate)),
        _ => crate::event::WindowState::FRAME,
    }
}

/// A size in physical pixels, in logical ones.
pub(crate) fn logical(size: PhysicalSize<u32>, scale: f32) -> Size {
    Size::new(size.width as f32 / scale, size.height as f32 / scale)
}

/// winit's held modifiers as ours. The logo key is `meta`: Command on
/// macOS, the Windows key elsewhere.
pub(crate) fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        control: state.control_key(),
        alt: state.alt_key(),
        meta: state.super_key(),
    }
}

#[cfg(test)]
mod tests;
