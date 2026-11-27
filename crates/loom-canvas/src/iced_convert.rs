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
    Code, Color, Event, Key, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, Rect, ScrollDelta, Size, Vector,
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

impl ToIced for Modifiers {
    type Iced = keyboard::Modifiers;

    /// The same keys held, as Iced's: the system key is Iced's logo key.
    fn to_iced(self) -> keyboard::Modifiers {
        let mut iced = keyboard::Modifiers::empty();
        iced.set(keyboard::Modifiers::SHIFT, self.shift);
        iced.set(keyboard::Modifiers::CTRL, self.control);
        iced.set(keyboard::Modifiers::ALT, self.alt);
        iced.set(keyboard::Modifiers::LOGO, self.meta);
        iced
    }
}

impl ToIced for Key {
    type Iced = keyboard::Key;

    /// The key as Iced names it. [`Key::Modifier`] becomes Shift, since
    /// nothing reads which modifier it was, and [`Key::Other`] becomes
    /// Iced's unidentified key.
    fn to_iced(self) -> keyboard::Key {
        use keyboard::key::Named;
        let named = match self {
            Key::Up => Named::ArrowUp,
            Key::Down => Named::ArrowDown,
            Key::Left => Named::ArrowLeft,
            Key::Right => Named::ArrowRight,
            Key::Enter => Named::Enter,
            Key::Escape => Named::Escape,
            Key::Space => Named::Space,
            Key::Tab => Named::Tab,
            Key::Backspace => Named::Backspace,
            Key::Delete => Named::Delete,
            Key::Home => Named::Home,
            Key::End => Named::End,
            Key::PageUp => Named::PageUp,
            Key::PageDown => Named::PageDown,
            Key::ContextMenu => Named::ContextMenu,
            Key::F1 => Named::F1,
            Key::F2 => Named::F2,
            Key::F10 => Named::F10,
            Key::Modifier => Named::Shift,
            Key::Character(c) => return keyboard::Key::Character(c.to_string().into()),
            Key::Other => return keyboard::Key::Unidentified,
        };
        keyboard::Key::Named(named)
    }
}

impl ToIced for Code {
    type Iced = keyboard::key::Physical;

    /// The place as Iced names it, or Iced's unidentified place for
    /// [`Code::Other`].
    fn to_iced(self) -> keyboard::key::Physical {
        use keyboard::key::{Code as IcedCode, NativeCode, Physical};
        match self {
            Code::BracketLeft => Physical::Code(IcedCode::BracketLeft),
            Code::BracketRight => Physical::Code(IcedCode::BracketRight),
            Code::Other => Physical::Unidentified(NativeCode::Unidentified),
        }
    }
}

/// A key event of ours as Iced's, for a key the control channel or a test
/// makes up and Iced must still hear until the window leaves Iced (plan
/// step P4). `modified_key` is the key itself, as a key typed with no
/// modifier that changes it gives. A release carries no modifiers in ours,
/// so it gets `released_with`.
#[must_use]
pub fn key_event_to_iced(event: &KeyEvent, released_with: Modifiers) -> keyboard::Event {
    match event {
        KeyEvent::Pressed {
            key,
            code,
            modifiers,
            repeat,
            text,
        } => keyboard::Event::KeyPressed {
            key: key.to_iced(),
            modified_key: key.to_iced(),
            physical_key: code.to_iced(),
            location: keyboard::Location::Standard,
            modifiers: modifiers.to_iced(),
            text: text.as_deref().map(Into::into),
            repeat: *repeat,
        },
        KeyEvent::Released { key, code } => keyboard::Event::KeyReleased {
            key: key.to_iced(),
            modified_key: key.to_iced(),
            physical_key: code.to_iced(),
            location: keyboard::Location::Standard,
            modifiers: released_with.to_iced(),
        },
        KeyEvent::ModifiersChanged(modifiers) => keyboard::Event::ModifiersChanged(modifiers.to_iced()),
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
                Named::PageUp => Key::PageUp,
                Named::PageDown => Key::PageDown,
                Named::ContextMenu => Key::ContextMenu,
                Named::F1 => Key::F1,
                Named::F2 => Key::F2,
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

impl ToCore for keyboard::key::Physical {
    type Core = Code;

    /// Where Iced says the key sits, as ours. Iced's key codes name the
    /// same places as the W3C codes [`Code`] uses.
    fn to_core(self) -> Code {
        use keyboard::key::{Code as IcedCode, Physical};
        match self {
            Physical::Code(IcedCode::BracketLeft) => Code::BracketLeft,
            Physical::Code(IcedCode::BracketRight) => Code::BracketRight,
            _ => Code::Other,
        }
    }
}

impl ToIced for loom_ui_core::Cursor {
    type Iced = mouse::Interaction;

    fn to_iced(self) -> mouse::Interaction {
        use loom_ui_core::Cursor;
        match self {
            Cursor::Default => mouse::Interaction::default(),
            Cursor::Grab => mouse::Interaction::Grab,
            Cursor::Grabbing => mouse::Interaction::Grabbing,
            Cursor::Pointer => mouse::Interaction::Pointer,
            Cursor::Crosshair => mouse::Interaction::Crosshair,
            Cursor::Text => mouse::Interaction::Text,
            Cursor::NotAllowed => mouse::Interaction::NotAllowed,
            Cursor::ResizeHorizontal => mouse::Interaction::ResizingHorizontally,
            Cursor::ResizeVertical => mouse::Interaction::ResizingVertically,
            Cursor::ResizeDiagonal => mouse::Interaction::ResizingDiagonallyDown,
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
                physical_key,
                modifiers,
                repeat,
                text,
                ..
            } => KeyEvent::Pressed {
                key: key.to_core(),
                code: physical_key.to_core(),
                modifiers: modifiers.to_core(),
                repeat: *repeat,
                text: text.as_ref().map(ToString::to_string),
            },
            keyboard::Event::KeyReleased { key, physical_key, .. } => KeyEvent::Released {
                key: key.to_core(),
                code: physical_key.to_core(),
            },
            keyboard::Event::ModifiersChanged(modifiers) => KeyEvent::ModifiersChanged(modifiers.to_core()),
        }),
        iced::Event::Window(window::Event::RedrawRequested(_)) => Event::Redraw,
        iced::Event::Window(window::Event::Unfocused) => Event::WindowUnfocused,
        iced::Event::InputMethod(event) => Event::Ime(match event {
            iced::advanced::input_method::Event::Opened => loom_ui_core::ImeEvent::Enabled,
            iced::advanced::input_method::Event::Preedit(text, cursor) => loom_ui_core::ImeEvent::Preedit {
                text: text.clone(),
                cursor: cursor.as_ref().map(|range| (range.start, range.end)),
            },
            iced::advanced::input_method::Event::Commit(text) => loom_ui_core::ImeEvent::Commit(text.clone()),
            iced::advanced::input_method::Event::Closed => loom_ui_core::ImeEvent::Disabled,
        }),
        iced::Event::Window(_) | iced::Event::Touch(_) => return None,
    })
}

#[cfg(test)]
mod tests;
