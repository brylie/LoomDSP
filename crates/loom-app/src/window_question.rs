//! Questions only the window's widgets can answer, until phase 3.
//!
//! Iced keeps focus and layout inside its widgets, so three things the app
//! needs come from the window, not from the app's own state: which part has
//! focus, where a text or a named part is, and where every named part is.
//! The app asks with [`App::ask_window`]; the bridge (`iced_bridge.rs`)
//! takes the questions after the turn, asks Iced, and hands each answer
//! back as a message, which arrives on a later turn, in the order asked.
//! The `Ui` harness answers them the same way on its own view.
//!
//! These are not effects (ui-layer.md, section 2, "Not effects"): the
//! runtime never carries them out. They go in phase 3, when the layout and
//! the focus are the app's own.

use crate::control::Found;
use crate::places::Places;
use crate::{App, Message};
use loom_ui_core::UiId;

/// A reply: makes the message for an answer.
pub(crate) type Reply<T> = Box<dyn FnOnce(T) -> Message + Send>;

/// One question for the window.
pub(crate) enum WindowQuestion {
    /// Which part has focus. A focused widget with no field ID reads as
    /// `iced_bridge::unnamed_field`, so `Some` means a text field has the
    /// keys.
    Focus(Reply<Option<UiId>>),
    /// The canvas, the first text widget that reads `text`, and every named
    /// part, for the control channel's `locate` and `screenshot`.
    Find {
        /// The text to look for.
        text: String,
        /// The reply.
        reply: Reply<Found>,
    },
    /// Where every named part is, for the accessibility tree.
    Places(Reply<Places>),
}

impl WindowQuestion {
    /// Asks which part has focus.
    pub(crate) fn focus(reply: impl FnOnce(Option<UiId>) -> Message + Send + 'static) -> Self {
        Self::Focus(Box::new(reply))
    }

    /// Asks where `text` and the named parts are.
    pub(crate) fn find(text: String, reply: impl FnOnce(Found) -> Message + Send + 'static) -> Self {
        Self::Find {
            text,
            reply: Box::new(reply),
        }
    }

    /// Asks where the named parts are.
    pub(crate) fn places(reply: impl FnOnce(Places) -> Message + Send + 'static) -> Self {
        Self::Places(Box::new(reply))
    }
}

impl std::fmt::Debug for WindowQuestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Focus(_) => write!(f, "Focus"),
            Self::Find { text, .. } => write!(f, "Find({text:?})"),
            Self::Places(_) => write!(f, "Places"),
        }
    }
}

impl App {
    /// Asks the window `question`, after this turn.
    pub(crate) fn ask_window(&self, question: WindowQuestion) {
        self.window_questions.borrow_mut().push(question);
    }

    /// The questions asked since the last call, in the order asked.
    pub(crate) fn take_window_questions(&self) -> Vec<WindowQuestion> {
        std::mem::take(&mut *self.window_questions.borrow_mut())
    }
}

#[cfg(test)]
mod tests;
