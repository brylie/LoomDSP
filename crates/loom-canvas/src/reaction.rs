//! What the canvas answers to one input event, in our own terms.
//!
//! The input handlers fill a [`Reaction`] and know nothing of the toolkit
//! that drives them. The driver reads it afterwards: it passes the events
//! on to the app, marks the input as used, and redraws.

use crate::CanvasEvent;

/// What one input event led to.
///
/// Three rules hold, whatever order the handlers ran in:
///
/// - `events` keeps the order the canvas reported them in.
/// - `taken` is true once any handler took the input, and stays true.
/// - `redraw` is true once any handler asked for a redraw, and stays true.
///
/// A reaction says nothing about waking later. When the canvas next needs
/// to wake with no input is a fact of its state, not of one event: ask
/// `CanvasInput::next_deadline`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reaction {
    /// What the canvas reports to the app, oldest first.
    pub events: Vec<CanvasEvent>,
    /// True when the canvas used the input, so nothing behind it should
    /// act on it too.
    pub taken: bool,
    /// True when what the canvas shows changed, so it needs a redraw.
    pub redraw: bool,
}

impl Reaction {
    /// Reports an event to the app, after those already reported.
    pub(crate) fn emit(&mut self, event: CanvasEvent) {
        self.events.push(event);
    }

    /// Marks the input as used.
    pub(crate) fn take(&mut self) {
        self.taken = true;
    }

    /// Asks for a redraw.
    pub(crate) fn redraw(&mut self) {
        self.redraw = true;
    }
}

#[cfg(test)]
mod tests;
