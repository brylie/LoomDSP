//! Input events: what the pointer, the keys, and the window tell an element.
//!
//! These are our own types, shaped by what an element needs to know and not
//! by any toolkit's event list. A driver, such as a window shell or a test
//! harness, turns what it receives into these and hands them over.
//!
//! # The pointer travels beside the event
//!
//! A [`PointerEvent`] does not carry a position. The driver passes the
//! pointer's place beside every event, pointer or not, as an
//! `Option<Point>`. `None` means the element may not see the pointer: it is
//! outside the window, or under something drawn above the element. There are
//! two reasons for this shape:
//!
//! - A key press can depend on where the pointer is, so the position must
//!   reach key events too.
//! - Only the driver knows that something above the element covers the
//!   pointer. The element cannot work that out from positions.
//!
//! # Only what something asks for
//!
//! The types hold what today's elements match on and no more. [`Key`] has
//! one variant for every modifier key and one for every key nothing uses.
//! Events for text composition, touch, dropped files, and resizing are not
//! here yet. Each arrives when an element needs it.

/// One thing that happened, as an element receives it.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The pointer moved, or one of its buttons or its wheel was used.
    Pointer(PointerEvent),
    /// A key went down or up, or the held modifiers changed.
    Key(KeyEvent),
    /// Time has passed and the element may have work to do: advance an
    /// animation, act on a deadline that has passed, or get ready to be
    /// drawn. One variant stands for all three for now. A later runtime may
    /// tell them apart.
    Redraw,
    /// The window stopped being the one that takes the keys.
    WindowUnfocused,
}

/// What the pointer did. Where it is travels beside the event (see the
/// module text).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    /// The pointer moved.
    Moved,
    /// A button went down.
    Pressed(PointerButton),
    /// A button came up.
    Released(PointerButton),
    /// The wheel turned, or two fingers slid on a touch pad.
    Scrolled(ScrollDelta),
    /// The pointer left the window. This is not the same as the pointer
    /// being hidden under something: an element may keep a drag alive while
    /// the pointer is hidden, and end it when the pointer leaves.
    LeftWindow,
    /// The pointer came back into the window.
    EnteredWindow,
}

/// A button on the pointing device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    /// The main button: the left one for a right-handed mouse.
    Left,
    /// The button that opens menus.
    Right,
    /// The middle button, often the wheel pressed down.
    Middle,
    /// Any other button, such as the back and forward buttons on the side
    /// of a mouse. One variant covers them because an element only asks
    /// whether some button went down, for example to give up the keys when
    /// a press lands elsewhere.
    Other,
}

/// How far a scroll went, in the unit the device reports.
///
/// A wheel reports whole steps as lines. A touch pad reports pixels. The
/// element decides how far one line is.
///
/// A positive `y` is a scroll up: the wheel turned away from the user.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// Steps of a wheel.
    Lines {
        /// Steps sideways.
        x: f32,
        /// Steps up, or down when negative.
        y: f32,
    },
    /// A distance in logical pixels.
    Pixels {
        /// Pixels sideways.
        x: f32,
        /// Pixels up, or down when negative.
        y: f32,
    },
}

/// What a key did.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyEvent {
    /// A key went down, or repeats while held.
    Pressed {
        /// Which key.
        key: Key,
        /// The modifier keys held at that moment.
        modifiers: Modifiers,
        /// True when the key was already down and the system repeats it.
        repeat: bool,
        /// The text the press types, if it types any, with every modifier
        /// applied: Shift and `a` give `A` here. This is separate from
        /// `key`, which names the key with no modifier applied. A key that
        /// types more than one character is [`Key::Other`], and its text
        /// still arrives here.
        text: Option<String>,
    },
    /// A key came up.
    Released {
        /// Which key.
        key: Key,
    },
    /// The set of held modifier keys changed.
    ModifiersChanged(Modifiers),
}

/// A key, by what it means and not by where it sits on the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// Enter or Return.
    Enter,
    /// Escape.
    Escape,
    /// The space bar.
    Space,
    /// Tab.
    Tab,
    /// Backspace: deletes to the left.
    Backspace,
    /// Delete: deletes to the right.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// The menu key, which opens the menu of what has focus.
    ContextMenu,
    /// F10. With Shift, it opens a menu as the menu key does.
    F10,
    /// Shift, Control, Alt, or the system key, on either side. One variant
    /// covers them all because nothing yet asks which one it was. Which
    /// ones are held is in [`Modifiers`].
    Modifier,
    /// A key that types exactly one character, such as `a` or `5`. The
    /// character is the one the keyboard layout gives the key with no
    /// modifier applied: Shift, Caps Lock, and Control do not change it, so
    /// the `a` key is `Character('a')` whatever is held. A shortcut can
    /// then match one character. What the press types, such as `A`, is in
    /// the `text` of [`KeyEvent::Pressed`].
    Character(char),
    /// Any other key. It exists because nothing yet uses those keys, not
    /// because they will never have names.
    Other,
}

/// Which modifier keys are held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub control: bool,
    /// Alt, which a Mac keyboard labels Option.
    pub alt: bool,
    /// The system key: Command on a Mac, the Windows key on a PC keyboard.
    pub meta: bool,
}

impl Modifiers {
    /// No modifier held.
    pub const NONE: Self = Self {
        shift: false,
        control: false,
        alt: false,
        meta: false,
    };

    /// True when no modifier is held.
    #[must_use]
    pub const fn none(self) -> bool {
        !(self.shift || self.control || self.alt || self.meta)
    }

    /// Whether the key for shortcuts is held, on the given platform:
    /// Command on a Mac and Control everywhere else. This is each
    /// platform's own convention, so Save is Cmd+S on a Mac and Ctrl+S on
    /// Windows and Linux.
    ///
    /// Control stays its own field, so a caller can still tell Control from
    /// Command on a Mac.
    #[must_use]
    pub const fn shortcut_on(self, platform: Platform) -> bool {
        match platform {
            Platform::Mac => self.meta,
            Platform::Other => self.control,
        }
    }

    /// Whether the key for shortcuts is held, on the platform this program
    /// was built for.
    #[must_use]
    pub const fn shortcut(self) -> bool {
        self.shortcut_on(Platform::current())
    }
}

/// The platforms that differ in which key makes a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS, where shortcuts use Command.
    Mac,
    /// Every other platform, where shortcuts use Control.
    Other,
}

impl Platform {
    /// The platform this program was built for.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }
}

#[cfg(test)]
mod tests;
