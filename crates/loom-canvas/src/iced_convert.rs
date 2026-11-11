//! Conversions between `loom-ui-core` values and Iced's.
//!
//! The canvas and the app keep geometry and colours in `loom-ui-core` types.
//! A value changes type only where it crosses into or out of an Iced call,
//! and it does so through this one module. That gives the move away from
//! Iced a single place to delete (ADR-0041,
//! `docs/dev/leaving-iced-plan.md`, step 1.1).
//!
//! These are extension traits and not `From` impls. Rust allows a `From`
//! impl only in a crate that owns one of the two types, and this crate owns
//! neither. `loom-ui-core` must not own them either, since it depends on no
//! toolkit.

use iced::{keyboard, mouse, window};
use loom_ui_core::{
    Color, Event, Key, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, Rect, ScrollDelta, Size, Vector,
};

/// Turns a `loom-ui-core` value into the Iced value with the same fields.
pub trait ToIced {
    /// The Iced type this value becomes.
    type Iced;

    /// The same value as an Iced type, field for field.
    fn to_iced(self) -> Self::Iced;
}

/// Turns an Iced value into the `loom-ui-core` value with the same fields.
pub trait ToCore {
    /// The `loom-ui-core` type this value becomes.
    type Core;

    /// The same value as a `loom-ui-core` type, field for field.
    fn to_core(self) -> Self::Core;
}

impl ToIced for Point {
    type Iced = iced::Point;

    fn to_iced(self) -> iced::Point {
        iced::Point { x: self.x, y: self.y }
    }
}

impl ToCore for iced::Point {
    type Core = Point;

    fn to_core(self) -> Point {
        Point { x: self.x, y: self.y }
    }
}

impl ToIced for Vector {
    type Iced = iced::Vector;

    fn to_iced(self) -> iced::Vector {
        iced::Vector { x: self.x, y: self.y }
    }
}

impl ToCore for iced::Vector {
    type Core = Vector;

    fn to_core(self) -> Vector {
        Vector { x: self.x, y: self.y }
    }
}

impl ToIced for Size {
    type Iced = iced::Size;

    fn to_iced(self) -> iced::Size {
        iced::Size {
            width: self.width,
            height: self.height,
        }
    }
}

impl ToCore for iced::Size {
    type Core = Size;

    fn to_core(self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }
}

impl ToIced for Rect {
    type Iced = iced::Rectangle;

    fn to_iced(self) -> iced::Rectangle {
        iced::Rectangle {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

impl ToCore for iced::Rectangle {
    type Core = Rect;

    fn to_core(self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

impl ToIced for Color {
    type Iced = iced::Color;

    fn to_iced(self) -> iced::Color {
        iced::Color {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }
}

impl ToCore for iced::Color {
    type Core = Color;

    fn to_core(self) -> Color {
        Color {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }
}

impl ToCore for keyboard::Modifiers {
    type Core = Modifiers;

    fn to_core(self) -> Modifiers {
        Modifiers {
            shift: self.shift(),
            control: self.control(),
            alt: self.alt(),
            meta: self.logo(),
        }
    }
}

impl ToCore for mouse::Button {
    type Core = PointerButton;

    fn to_core(self) -> PointerButton {
        match self {
            mouse::Button::Left => PointerButton::Left,
            mouse::Button::Right => PointerButton::Right,
            mouse::Button::Middle => PointerButton::Middle,
            mouse::Button::Back | mouse::Button::Forward | mouse::Button::Other(_) => PointerButton::Other,
        }
    }
}

impl ToCore for mouse::ScrollDelta {
    type Core = ScrollDelta;

    fn to_core(self) -> ScrollDelta {
        match self {
            mouse::ScrollDelta::Lines { x, y } => ScrollDelta::Lines { x, y },
            mouse::ScrollDelta::Pixels { x, y } => ScrollDelta::Pixels { x, y },
        }
    }
}

impl ToCore for &keyboard::Key {
    type Core = Key;

    /// A key that types exactly one character is [`Key::Character`]. A key
    /// that types more, or that has no use here, is [`Key::Other`]; its
    /// text still reaches the element in the press event.
    ///
    /// Give this the press's `key`, never its `modified_key`. On desktop
    /// Iced fills `key` with the key with no modifier applied, which is
    /// what [`Key::Character`] promises, and `modified_key` with the key
    /// after Shift and Caps Lock. [`event_to_core`] reads `key`.
    fn to_core(self) -> Key {
        use keyboard::key::Named;
        match self.as_ref() {
            keyboard::Key::Named(named) => match named {
                Named::ArrowUp => Key::Up,
                Named::ArrowDown => Key::Down,
                Named::ArrowLeft => Key::Left,
                Named::ArrowRight => Key::Right,
                Named::Enter => Key::Enter,
                Named::Escape => Key::Escape,
                Named::Space => Key::Space,
                Named::Tab => Key::Tab,
                Named::Backspace => Key::Backspace,
                Named::Delete => Key::Delete,
                Named::Home => Key::Home,
                Named::End => Key::End,
                Named::ContextMenu => Key::ContextMenu,
                Named::F10 => Key::F10,
                Named::Shift | Named::Control | Named::Alt | Named::AltGraph | Named::Meta | Named::Super => {
                    Key::Modifier
                }
                _ => Key::Other,
            },
            keyboard::Key::Character(text) => {
                let mut characters = text.chars();
                match (characters.next(), characters.next()) {
                    (Some(only), None) => Key::Character(only),
                    _ => Key::Other,
                }
            }
            keyboard::Key::Unidentified => Key::Other,
        }
    }
}

impl ToIced for crate::CursorShape {
    type Iced = mouse::Interaction;

    fn to_iced(self) -> mouse::Interaction {
        use crate::CursorShape;
        match self {
            CursorShape::Default => mouse::Interaction::default(),
            CursorShape::Grab => mouse::Interaction::Grab,
            CursorShape::Grabbing => mouse::Interaction::Grabbing,
            CursorShape::Pointer => mouse::Interaction::Pointer,
            CursorShape::Crosshair => mouse::Interaction::Crosshair,
            CursorShape::ResizeHorizontal => mouse::Interaction::ResizingHorizontally,
            CursorShape::ResizeVertical => mouse::Interaction::ResizingVertically,
            CursorShape::ResizeDiagonal => mouse::Interaction::ResizingDiagonallyDown,
        }
    }
}

/// Turns an Iced event into ours, or `None` for an event with no meaning to
/// our elements yet: touch, text composition, and the window events other
/// than a redraw and losing focus.
///
/// The pointer's place is not part of the result. Iced passes it beside the
/// event as a cursor, and the caller passes it on in the same way.
#[must_use]
pub fn event_to_core(event: &iced::Event) -> Option<Event> {
    Some(match event {
        iced::Event::Mouse(event) => Event::Pointer(match event {
            mouse::Event::CursorMoved { .. } => PointerEvent::Moved,
            mouse::Event::ButtonPressed(button) => PointerEvent::Pressed(button.to_core()),
            mouse::Event::ButtonReleased(button) => PointerEvent::Released(button.to_core()),
            mouse::Event::WheelScrolled { delta } => PointerEvent::Scrolled(delta.to_core()),
            mouse::Event::CursorLeft => PointerEvent::LeftWindow,
            mouse::Event::CursorEntered => PointerEvent::EnteredWindow,
        }),
        iced::Event::Keyboard(event) => Event::Key(match event {
            keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat,
                text,
                ..
            } => KeyEvent::Pressed {
                key: key.to_core(),
                modifiers: modifiers.to_core(),
                repeat: *repeat,
                text: text.as_ref().map(ToString::to_string),
            },
            keyboard::Event::KeyReleased { key, .. } => KeyEvent::Released { key: key.to_core() },
            keyboard::Event::ModifiersChanged(modifiers) => KeyEvent::ModifiersChanged(modifiers.to_core()),
        }),
        iced::Event::Window(window::Event::RedrawRequested(_)) => Event::Redraw,
        iced::Event::Window(window::Event::Unfocused) => Event::WindowUnfocused,
        iced::Event::Window(_) | iced::Event::Touch(_) | iced::Event::InputMethod(_) => return None,
    })
}

#[cfg(test)]
mod tests;
