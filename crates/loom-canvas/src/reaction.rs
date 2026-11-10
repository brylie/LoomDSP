//! What the canvas answers to one input event, in our own terms.
//!
//! The input handlers fill a [`Reaction`] and know nothing of the toolkit
//! that drives them. The driver reads it afterwards: it passes the events
//! on to the app, marks the input as used, and schedules the redraw.

use crate::CanvasEvent;
use std::time::Instant;

/// What one input event led to.
///
/// Three rules hold, whatever order the handlers ran in:
///
/// - `events` keeps the order the canvas reported them in.
/// - `taken` is true once any handler took the input, and stays true.
/// - `redraw` is the soonest redraw any handler asked for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reaction {
    /// What the canvas reports to the app, oldest first.
    pub events: Vec<CanvasEvent>,
    /// True when the canvas used the input, so nothing behind it should
    /// act on it too.
    pub taken: bool,
    /// When the canvas next needs to be drawn or woken.
    pub redraw: Redraw,
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

    /// Asks for a redraw at the next frame.
    pub(crate) fn redraw(&mut self) {
        self.redraw = Redraw::Now;
    }

    /// Asks to be woken and redrawn at `time`, unless something sooner is
    /// already asked for.
    pub(crate) fn redraw_at(&mut self, time: Instant) {
        self.redraw = self.redraw.sooner(Redraw::At(time));
    }
}

/// When the canvas next needs a redraw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Redraw {
    /// Nothing changed: no redraw is needed.
    #[default]
    No,
    /// Wake and redraw at this time, for a deadline such as hover text
    /// that is due.
    At(Instant),
    /// Redraw at the next frame.
    Now,
}

impl Redraw {
    /// The sooner of two requests: `Now` before any time, the earlier of
    /// two times, and any request before `No`.
    #[must_use]
    pub fn sooner(self, other: Self) -> Self {
        match (self, other) {
            (Self::Now, _) | (_, Self::Now) => Self::Now,
            (Self::At(a), Self::At(b)) => Self::At(a.min(b)),
            (Self::At(time), Self::No) | (Self::No, Self::At(time)) => Self::At(time),
            (Self::No, Self::No) => Self::No,
        }
    }
}

/// The pointer shape the canvas asks for, by what it tells the user.
///
/// It holds only the shapes the canvas uses. The driver maps each to what
/// its window system calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// The plain arrow.
    #[default]
    Default,
    /// An open hand: this can be picked up and moved.
    Grab,
    /// A closed hand: something is being moved.
    Grabbing,
    /// A pointing hand: a press here does something.
    Pointer,
    /// A cross: a press here picks an exact place.
    Crosshair,
    /// A two-way arrow, left and right: a drag sideways changes a value.
    ResizeHorizontal,
    /// A two-way arrow, up and down: a drag up or down changes a value.
    ResizeVertical,
    /// A slanted two-way arrow: a drag resizes from a corner.
    ResizeDiagonal,
}

#[cfg(test)]
mod tests;
