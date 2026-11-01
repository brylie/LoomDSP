//! When and where hover text shows (panels.md, Labels and hover text).
//!
//! Kept apart from the widget, so the timing and placement rules can be
//! tested without a window.

use crate::scene::PortRef;
use iced::{Point, Rectangle, Size};
use std::time::{Duration, Instant};

/// How long the pointer or keyboard focus must rest on one port or control
/// before its hover text shows. *Why:* while wiring, the pointer passes over
/// many ports, and text that flashed up under every one would get in the way.
pub const HOVER_DELAY: Duration = Duration::from_millis(500);

/// Text size of hover text, in screen pixels, at any zoom. *Why:* at middle
/// zoom tile text is too small to read, and hover text is what the user
/// needs there.
pub const HOVER_TEXT_PX: f32 = 12.0;

/// Widest hover text, in screen pixels, padding included.
pub const HOVER_MAX_WIDTH: f32 = 260.0;

/// Padding inside the hover text box, in screen pixels.
pub const HOVER_PADDING: f32 = 6.0;

/// Space between the hover text and what it describes, and between it and
/// the canvas edge, in screen pixels.
const HOVER_GAP: f32 = 6.0;

/// What hover text is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverTarget {
    /// A port.
    Port(PortRef),
    /// A control on a panel: module ID and widget index.
    Control(u32, usize),
    /// A tile's header, by module ID: its family and description.
    Title(u32),
}

/// Times hover text: it shows once the same target has rested for
/// [`HOVER_DELAY`], and hides at once when the target changes.
#[derive(Debug, Default)]
pub struct HoverTimer {
    /// The current target, and when it started resting.
    target: Option<(HoverTarget, Instant)>,
    shown: bool,
}

impl HoverTimer {
    /// Sets the current target. A different target hides the text and
    /// starts the wait again; the same target changes nothing. Returns true
    /// if text that was shown is now hidden.
    pub fn rest(&mut self, target: Option<HoverTarget>, now: Instant) -> bool {
        if self.target.map(|(current, _)| current) == target {
            return false;
        }
        self.target = target.map(|target| (target, now));
        std::mem::take(&mut self.shown)
    }

    /// Hides the text and starts the wait again on the same target, for a
    /// pan or zoom. Returns true if text that was shown is now hidden.
    pub fn restart(&mut self, now: Instant) -> bool {
        if let Some((_, since)) = &mut self.target {
            *since = now;
        }
        std::mem::take(&mut self.shown)
    }

    /// When the waiting text is due, or `None` if nothing is waiting. A
    /// shown text is not waiting, so it asks for no more frames.
    pub fn due(&self) -> Option<Instant> {
        match self.target {
            Some((_, since)) if !self.shown => Some(since + HOVER_DELAY),
            _ => None,
        }
    }

    /// Shows the text if it is due at `now`. Returns true if it just
    /// appeared.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.due().is_some_and(|due| now >= due) {
            self.shown = true;
            return true;
        }
        false
    }

    /// The target whose text is showing.
    pub fn shown(&self) -> Option<HoverTarget> {
        self.target.filter(|_| self.shown).map(|(target, _)| target)
    }
}

/// Widest a line of hover text may be, in screen pixels, on a canvas
/// `canvas_width` wide. Usually [`HOVER_MAX_WIDTH`] less padding, but less
/// on a narrow canvas, so the box always fits beside the inspector and text
/// wraps instead of being clipped.
pub fn text_width(canvas_width: f32) -> f32 {
    let room = HOVER_MAX_WIDTH.min(canvas_width - 2.0 * HOVER_GAP);
    (room - 2.0 * HOVER_PADDING).max(1.0)
}

/// Where a hover text box of `size` goes: centred above `anchor`, or below
/// it when there is no room above, and always inside `bounds`. All in
/// screen coordinates. Returns the box's top-left corner.
pub fn place(size: Size, anchor: Rectangle, bounds: Rectangle) -> Point {
    let above = anchor.y - HOVER_GAP - size.height;
    let y = if above >= bounds.y + HOVER_GAP {
        above
    } else {
        anchor.y + anchor.height + HOVER_GAP
    };
    let x = anchor.center_x() - size.width * 0.5;
    let clamp = |value: f32, low: f32, high: f32| value.min(high).max(low);
    Point::new(
        clamp(
            x,
            bounds.x + HOVER_GAP,
            bounds.x + bounds.width - HOVER_GAP - size.width,
        ),
        clamp(
            y,
            bounds.y + HOVER_GAP,
            bounds.y + bounds.height - HOVER_GAP - size.height,
        ),
    )
}

#[cfg(test)]
mod tests;
