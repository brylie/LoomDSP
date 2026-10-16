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
mod tests {
    use super::*;

    const PORT: HoverTarget = HoverTarget::Port(PortRef {
        module: 1,
        output: false,
        index: 0,
    });
    const KNOB: HoverTarget = HoverTarget::Control(1, 0);

    #[test]
    fn shows_after_resting_for_the_delay() {
        let start = Instant::now();
        let mut timer = HoverTimer::default();
        assert!(!timer.rest(Some(PORT), start));
        assert_eq!(timer.due(), Some(start + HOVER_DELAY));
        assert!(!timer.tick(start + HOVER_DELAY - Duration::from_millis(1)));
        assert_eq!(timer.shown(), None);
        assert!(timer.tick(start + HOVER_DELAY));
        assert_eq!(timer.shown(), Some(PORT));
    }

    #[test]
    fn a_shown_text_asks_for_no_more_frames() {
        let start = Instant::now();
        let mut timer = HoverTimer::default();
        timer.rest(Some(PORT), start);
        timer.tick(start + HOVER_DELAY);
        assert_eq!(timer.due(), None);
        // Resting on the same target, or redrawing, changes nothing.
        assert!(!timer.rest(Some(PORT), start + HOVER_DELAY * 4));
        assert!(!timer.tick(start + HOVER_DELAY * 4));
        assert_eq!(timer.shown(), Some(PORT));
    }

    #[test]
    fn leaving_hides_at_once_and_a_new_target_waits_again() {
        let start = Instant::now();
        let mut timer = HoverTimer::default();
        timer.rest(Some(PORT), start);
        timer.tick(start + HOVER_DELAY);
        let later = start + HOVER_DELAY * 2;
        assert!(timer.rest(Some(KNOB), later), "hides the port's text");
        assert_eq!(timer.shown(), None);
        assert_eq!(timer.due(), Some(later + HOVER_DELAY));
        assert!(!timer.rest(None, later), "nothing was shown");
        assert_eq!(timer.due(), None);
    }

    #[test]
    fn moving_within_a_target_does_not_restart_the_wait() {
        let start = Instant::now();
        let mut timer = HoverTimer::default();
        timer.rest(Some(PORT), start);
        timer.rest(Some(PORT), start + Duration::from_millis(300));
        assert!(timer.tick(start + HOVER_DELAY));
    }

    #[test]
    fn pan_and_zoom_hide_and_wait_again() {
        let start = Instant::now();
        let mut timer = HoverTimer::default();
        timer.rest(Some(KNOB), start);
        timer.tick(start + HOVER_DELAY);
        let later = start + HOVER_DELAY * 3;
        assert!(timer.restart(later));
        assert_eq!(timer.shown(), None);
        assert_eq!(timer.due(), Some(later + HOVER_DELAY));
    }

    #[test]
    fn hover_text_stays_inside_the_canvas_at_every_edge() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(800.0, 600.0));
        let size = Size::new(HOVER_MAX_WIDTH, 60.0);
        let inside = |at: Point| {
            at.x >= bounds.x
                && at.y >= bounds.y
                && at.x + size.width <= bounds.x + bounds.width
                && at.y + size.height <= bounds.y + bounds.height
        };
        let port = |x: f32, y: f32| Rectangle::new(Point::new(x - 4.0, y - 4.0), Size::new(8.0, 8.0));
        for anchor in [
            port(12.0, 300.0),  // left edge
            port(808.0, 300.0), // right edge
            port(400.0, 22.0),  // top edge
            port(400.0, 618.0), // bottom edge
            port(12.0, 22.0),   // corner
        ] {
            let at = place(size, anchor, bounds);
            assert!(inside(at), "{anchor:?} -> {at:?}");
        }
    }

    #[test]
    fn a_narrow_canvas_narrows_the_text_and_the_box_fits() {
        assert_eq!(text_width(1_000.0), HOVER_MAX_WIDTH - 2.0 * HOVER_PADDING);
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(150.0, 400.0));
        let width = text_width(bounds.width) + 2.0 * HOVER_PADDING;
        assert!(width <= bounds.width - 2.0 * HOVER_GAP, "{width}");
        let size = Size::new(width, 80.0);
        let anchor = Rectangle::new(Point::new(140.0, 200.0), Size::new(8.0, 8.0));
        let at = place(size, anchor, bounds);
        assert!(at.x >= 0.0 && at.x + size.width <= bounds.width, "{at:?}");
        // Even a canvas too small for padding leaves a positive width.
        assert!(text_width(4.0) > 0.0);
    }

    #[test]
    fn hover_text_sits_above_and_goes_below_near_the_top() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
        let size = Size::new(100.0, 40.0);
        let middle = Rectangle::new(Point::new(395.0, 295.0), Size::new(10.0, 10.0));
        let at = place(size, middle, bounds);
        assert_eq!(at, Point::new(350.0, 295.0 - HOVER_GAP - 40.0));
        let top = Rectangle::new(Point::new(395.0, 10.0), Size::new(10.0, 10.0));
        assert_eq!(place(size, top, bounds).y, 20.0 + HOVER_GAP);
    }
}
