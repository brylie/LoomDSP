//! The runtime's slider (ui-layer.md, section 5, rule 16; section 6,
//! rule 10; section 7, rule 30).
//!
//! A slider shows a position from 0 to 1 along its travel. The position is
//! the control's travel, not the value it sets: the app maps one to the
//! other, and keeps the position, as it keeps a field's text. The slider
//! keeps no value of its own; the app's wins at the next layout.
//!
//! What moves it reaches the app as [`App::slid`](crate::App::slid), with
//! a [`SlidePhase`], on the next turn:
//!
//! - **A drag** is one Start, its Moves, and one End at the release. A
//!   press on the track jumps there, then drags. Shift while dragging is
//!   ten times finer, from where Shift went down.
//! - **A double-click** resets it to its default, as one Start, Move, and
//!   End.
//! - **Keys** follow the WAI-ARIA slider pattern. A key and a repeat within
//!   [`KEY_GROUP`] of the last join one group: Start and Move on its first
//!   key, Move on each after, and End when the group closes, 500 ms after
//!   its last key, when focus leaves, or on a pointer press.
//! - **A screen reader's Increment and Decrement** move it by its key step,
//!   each as one Start, Move, and End.
//!
//! The wheel never moves it (rule 16): scrolling over a control never
//! changes its value.

use loom_ui_core::{Key, Modifiers, Point, Rect, UiId};
use std::time::{Duration, Instant};

/// The size of a slider's round thumb, in logical pixels (rule 16).
pub(crate) const THUMB: f32 = 14.0;

/// The thickness of a slider's track, in logical pixels (rule 16).
pub(crate) const TRACK: f32 = 4.0;

/// How long after its last key a group of keys on a slider closes, and how
/// close a key must come to the last to join it (panels.md, Keyboard
/// without a mouse).
pub(crate) const KEY_GROUP: Duration = Duration::from_millis(500);

/// How soon a second press must follow the first to be a double-click, as
/// on tiles (`loom-canvas`, `input.rs`).
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// How far a second press may land from the first and still be a
/// double-click, in logical pixels, as on tiles.
const DOUBLE_CLICK_SLOP: f32 = 4.0;

/// How much finer a drag with Shift held is (panels.md, Interaction).
const FINE_DRAG: f32 = 0.1;

/// How a drag moves a slider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drag {
    /// To any position.
    Continuous,
    /// To the nearest whole number of this step, from 0, as for a
    /// whole-number parameter.
    Step(f32),
}

/// Which part of a slider's track is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// From the minimum to the thumb.
    FromMin,
    /// From the middle to the thumb, for a range centred on 0, such as a
    /// modulation's depth.
    FromMiddle,
}

/// What a slider shows and how keys and a drag move it, for
/// [`Element::slider`](crate::Element::slider).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slider {
    position: f32,
    key_step: f32,
    fine_key_step: f32,
    page_step: f32,
    drag: Drag,
    default: Option<f32>,
    fill: Fill,
}

impl Slider {
    /// A slider at `position`, from 0 to 1, clamped there. Its arrow keys
    /// move 1% of the travel, 0.1% with Shift, and Page Up and Page Down
    /// 10%, as on the tile knob for a continuous parameter (section 6,
    /// rule 10). A drag is continuous, and it has no default to reset to
    /// until [`Slider::default`] gives one.
    pub fn new(position: f32) -> Self {
        Self {
            position: unit(position),
            key_step: 0.01,
            fine_key_step: 0.001,
            page_step: 0.1,
            drag: Drag::Continuous,
            default: None,
            fill: Fill::FromMin,
        }
    }

    /// How far an arrow key, and a screen reader's Increment and
    /// Decrement, move it, as a share of the travel: one value's share for
    /// a whole-number parameter.
    #[must_use]
    pub fn key_step(mut self, step: f32) -> Self {
        self.key_step = step.abs();
        self
    }

    /// How far an arrow key with Shift moves it, as a share of the travel.
    #[must_use]
    pub fn fine_key_step(mut self, step: f32) -> Self {
        self.fine_key_step = step.abs();
        self
    }

    /// How far Page Up and Page Down move it, as a share of the travel.
    #[must_use]
    pub fn page_step(mut self, step: f32) -> Self {
        self.page_step = step.abs();
        self
    }

    /// How a drag moves it.
    #[must_use]
    pub fn drag(mut self, drag: Drag) -> Self {
        self.drag = drag;
        self
    }

    /// The position a double-click resets it to, the default's place on
    /// the travel.
    #[must_use]
    pub fn default(mut self, position: f32) -> Self {
        self.default = Some(unit(position));
        self
    }

    /// Fills its track from the middle, for a range centred on 0.
    #[must_use]
    pub fn centred(mut self) -> Self {
        self.fill = Fill::FromMiddle;
        self
    }

    /// The position it shows, from 0 to 1.
    pub fn position(&self) -> f32 {
        self.position
    }

    /// Which part of its track is filled.
    pub(crate) fn fill(&self) -> Fill {
        self.fill
    }

    /// The position a double-click resets it to, if it has a default.
    pub(crate) fn reset_to(&self) -> Option<f32> {
        self.default
    }

    /// Where `key` with `modifiers` moves it from `from`, by the WAI-ARIA
    /// slider pattern (section 6, rule 10): Right and Up up by its key
    /// step, Left and Down down, both by the fine step with Shift; Home and
    /// End to the ends, Page Up and Page Down by the page step. `None` for
    /// a key it does not take, or with another modifier.
    pub(crate) fn after_key(&self, from: f32, key: Key, modifiers: Modifiers) -> Option<f32> {
        let shift_only = modifiers.shift && !(modifiers.control || modifiers.alt || modifiers.meta);
        let arrow = if modifiers.none() {
            self.key_step
        } else if shift_only {
            self.fine_key_step
        } else {
            return None;
        };
        let to = match key {
            Key::Right | Key::Up => from + arrow,
            Key::Left | Key::Down => from - arrow,
            Key::Home if modifiers.none() => 0.0,
            Key::End if modifiers.none() => 1.0,
            Key::PageUp if modifiers.none() => from + self.page_step,
            Key::PageDown if modifiers.none() => from - self.page_step,
            _ => return None,
        };
        Some(unit(to))
    }

    /// Where a screen reader's Increment (`up`) or Decrement moves it from
    /// its position: one key step, as an arrow key does (section 7, rule
    /// 5).
    pub(crate) fn after_step(&self, up: bool) -> f32 {
        unit(if up {
            self.position + self.key_step
        } else {
            self.position - self.key_step
        })
    }

    /// `position` as a drag leaves it: on its step, if it has one, and
    /// from 0 to 1.
    fn snapped(&self, position: f32) -> f32 {
        match self.drag {
            Drag::Step(step) if step > 0.0 => unit((position / step).round() * step),
            _ => unit(position),
        }
    }
}

/// `position` clamped from 0 to 1, with NaN as 0.
fn unit(position: f32) -> f32 {
    if position.is_nan() {
        0.0
    } else {
        position.clamp(0.0, 1.0)
    }
}

/// Where a slider's thumb can go in `bounds`: the left end of its centre's
/// path, and the path's length. The thumb stays inside the bounds at both
/// ends.
pub(crate) fn travel(bounds: Rect) -> (f32, f32) {
    let left = bounds.x + THUMB / 2.0;
    (left, (bounds.width - THUMB).max(0.0))
}

/// The centre of the thumb of a slider at `position` in `bounds`.
pub(crate) fn thumb_centre(bounds: Rect, position: f32) -> Point {
    let (left, length) = travel(bounds);
    Point::new(left + position * length, bounds.y + bounds.height / 2.0)
}

/// The position under `x` on a slider in `bounds`.
fn position_at(bounds: Rect, x: f32) -> f32 {
    let (left, length) = travel(bounds);
    if length <= 0.0 {
        return 0.0;
    }
    unit((x - left) / length)
}

/// Where a drag, a reset, or a key is in its gesture, as
/// [`App::slid`](crate::App::slid) reports it. Each report carries the
/// position the slider should show now: Start the one the gesture starts
/// from, Move each new one, End the last. The app opens one undo step at
/// Start and closes it at End.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlidePhase {
    /// A drag, a reset, or a group of keys began.
    Start,
    /// It moved.
    Move,
    /// It ended: the release, the end of a reset, or a group of keys
    /// closing.
    End,
}

/// One report for the app: the slider, its position, and the phase.
pub(crate) type Slid = (UiId, f32, SlidePhase);

/// A drag on a slider, from its press to its release.
#[derive(Debug, Clone, PartialEq)]
struct Dragging {
    /// The slider.
    id: UiId,
    /// The slider's bounds in the window when the press went down.
    bounds: Rect,
    /// The slider's settings when the press went down.
    slider: Slider,
    /// Where the pointer was, left to right, when the drag last changed
    /// speed: at the press, or at the last move before Shift went down or
    /// up.
    anchor_x: f32,
    /// The position then, before any snapping to a step.
    anchor: f32,
    /// Shift was held then, so the drag is ten times finer.
    fine: bool,
    /// Where the pointer was at the last move, left to right.
    last_x: f32,
    /// The position at the last move, before any snapping to a step.
    raw: f32,
    /// The position the drag last reported, snapped.
    position: f32,
}

/// A group of keys on one slider, until it closes.
#[derive(Debug, Clone, PartialEq)]
struct KeyRun {
    /// The slider.
    id: UiId,
    /// When its last key came.
    last: Instant,
    /// The position its last key reported.
    position: f32,
}

/// The runtime's sliders' state: the drag held, the group of keys open,
/// and the last press, for a double-click.
#[derive(Debug, Default)]
pub(crate) struct Sliding {
    dragging: Option<Dragging>,
    keys: Option<KeyRun>,
    last_press: Option<(UiId, Instant, Point)>,
}

impl Sliding {
    /// True while a drag on a slider is held.
    pub(crate) fn drags(&self) -> bool {
        self.dragging.is_some()
    }

    /// A left press at `point`, at `now`, on the slider `id` with
    /// `slider` in `bounds`, with `shift` held or not. A second press soon
    /// after the first, close to it, resets the slider to its default; any
    /// other press starts a drag, jumping to the press first unless it is
    /// on the thumb. With no moment known (`now` is `None`), a press is
    /// never half of a double-click. Closes any open group of keys first.
    /// Returns what to report.
    pub(crate) fn press(
        &mut self,
        id: &UiId,
        slider: Slider,
        bounds: Rect,
        point: Point,
        shift: bool,
        now: Option<Instant>,
    ) -> Vec<Slid> {
        let mut out = self.close_keys();
        let double = now.is_some_and(|now| {
            self.last_press.as_ref().is_some_and(|(last, at, from)| {
                last == id
                    && now.saturating_duration_since(*at) < DOUBLE_CLICK
                    && from.distance(point) < DOUBLE_CLICK_SLOP
            })
        });
        if double && let Some(reset) = slider.reset_to() {
            self.last_press = None;
            self.dragging = None;
            out.push((id.clone(), slider.position(), SlidePhase::Start));
            out.push((id.clone(), reset, SlidePhase::Move));
            out.push((id.clone(), reset, SlidePhase::End));
            return out;
        }
        self.last_press = now.map(|now| (id.clone(), now, point));
        let from = slider.position();
        out.push((id.clone(), from, SlidePhase::Start));
        let on_thumb = thumb_centre(bounds, from).distance(point) <= THUMB / 2.0;
        let raw = if on_thumb { from } else { position_at(bounds, point.x) };
        let at = slider.snapped(raw);
        if at != from {
            out.push((id.clone(), at, SlidePhase::Move));
        }
        self.dragging = Some(Dragging {
            id: id.clone(),
            bounds,
            slider,
            anchor_x: point.x,
            anchor: raw,
            fine: shift,
            last_x: point.x,
            raw,
            position: at,
        });
        out
    }

    /// The pointer moved to `point` with `shift` held or not, during a
    /// drag: the position moves with it, ten times finer with Shift, from
    /// where Shift last went down or up, which was after the last move.
    /// Returns what to report: nothing with no drag, or when the position
    /// did not change.
    pub(crate) fn moved(&mut self, point: Point, shift: bool) -> Option<Slid> {
        let drag = self.dragging.as_mut()?;
        if shift != drag.fine {
            drag.anchor_x = drag.last_x;
            drag.anchor = drag.raw;
            drag.fine = shift;
        }
        let (_, length) = travel(drag.bounds);
        if length <= 0.0 {
            return None;
        }
        let speed = if drag.fine { FINE_DRAG } else { 1.0 };
        drag.last_x = point.x;
        // Unclamped, so a drag past an end and back comes back with the
        // pointer, not before it.
        drag.raw = drag.anchor + (point.x - drag.anchor_x) / length * speed;
        let to = drag.slider.snapped(drag.raw);
        if to == drag.position {
            return None;
        }
        drag.position = to;
        Some((drag.id.clone(), to, SlidePhase::Move))
    }

    /// The left button came up: the drag ends where it is. Returns its End,
    /// or nothing with no drag.
    pub(crate) fn released(&mut self) -> Option<Slid> {
        let drag = self.dragging.take()?;
        Some((drag.id, drag.position, SlidePhase::End))
    }

    /// A key moved the focused slider `id` from `from` to `to` at `now`.
    /// It joins the open group when that is this slider's and its last key
    /// came less than [`KEY_GROUP`] ago; otherwise the open group closes
    /// and a new one starts. Returns what to report: nothing when the key
    /// moved nothing and no group is open for it.
    pub(crate) fn key(&mut self, id: &UiId, from: f32, to: f32, now: Instant) -> Vec<Slid> {
        let joins = self
            .keys
            .as_ref()
            .is_some_and(|run| &run.id == id && now.saturating_duration_since(run.last) < KEY_GROUP);
        let mut out = Vec::new();
        if joins {
            let run = self.keys.as_mut().expect("the group it joins");
            run.last = now;
            if to != run.position {
                run.position = to;
                out.push((id.clone(), to, SlidePhase::Move));
            }
            return out;
        }
        out.extend(self.close_keys());
        if to == from {
            return out;
        }
        out.push((id.clone(), from, SlidePhase::Start));
        out.push((id.clone(), to, SlidePhase::Move));
        self.keys = Some(KeyRun {
            id: id.clone(),
            last: now,
            position: to,
        });
        out
    }

    /// Where keys on the slider `id` step from: the position its open
    /// group last reported, which the app may not have laid out yet, or
    /// `shown`, the one it shows, with no group open for it.
    pub(crate) fn key_base(&self, id: &UiId, shown: f32) -> f32 {
        self.keys
            .as_ref()
            .filter(|run| &run.id == id)
            .map_or(shown, |run| run.position)
    }

    /// One whole step of the slider `id` from `from` to `to`, as a screen
    /// reader's Increment or Decrement: Start, Move, and End, after closing
    /// any open group of keys. Nothing more when it did not move.
    pub(crate) fn step(&mut self, id: &UiId, from: f32, to: f32) -> Vec<Slid> {
        let mut out = self.close_keys();
        if to != from {
            out.push((id.clone(), from, SlidePhase::Start));
            out.push((id.clone(), to, SlidePhase::Move));
            out.push((id.clone(), to, SlidePhase::End));
        }
        out
    }

    /// Closes the open group of keys: its End, or nothing.
    pub(crate) fn close_keys(&mut self) -> Vec<Slid> {
        self.keys
            .take()
            .map(|run| (run.id, run.position, SlidePhase::End))
            .into_iter()
            .collect()
    }

    /// Closes the open group of keys when its time is up at `now`, or when
    /// `focus` is not its slider any more.
    pub(crate) fn close_keys_if(&mut self, now: Instant, focus: Option<&UiId>) -> Vec<Slid> {
        let over = self
            .keys
            .as_ref()
            .is_some_and(|run| now.saturating_duration_since(run.last) >= KEY_GROUP || focus != Some(&run.id));
        if over { self.close_keys() } else { Vec::new() }
    }

    /// When the open group of keys closes by itself, if one is open.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.keys.as_ref().map(|run| run.last + KEY_GROUP)
    }
}

#[cfg(test)]
mod tests;
