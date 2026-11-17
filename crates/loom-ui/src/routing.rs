//! Where input and focus go: the routing target.
//!
//! (The module is `routing`, not `target`, because the repository ignores
//! every folder named `target`, Cargo's build folder.)
//!
//! The runtime hands element events, focus, and scrolling to a [`Target`],
//! and asks it whether the window needs drawing. During phases 2 and 3 of
//! the move off Iced, the target is the bridge that holds Iced's views. In
//! phase 3 it becomes this crate's own element tree. The runtime does not
//! change between them.

use crate::effect::{FocusTarget, Selection, names};
use loom_ui_core::{Event, Point, UiId};
use std::time::Instant;

/// What the target did with an input event, which says whether the
/// runtime's layers below it hear it (ui-layer.md, section 5, rule 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routed {
    /// The target used it, so nothing below hears it.
    Taken,
    /// The target did not use it, so the runtime offers it to its layers
    /// below the target.
    Passed,
    /// The target decides after this turn: a toolkit that routes its own
    /// events later. For each pointer event it then leaves unused, the
    /// driver hands it on with [`Panels::pointer_below`](crate::Panels::pointer_below).
    Later,
}

/// What holds the elements: it takes their events and their focus.
pub trait Target<M> {
    /// Routes one element event, adding any messages it gives to `out`,
    /// and says what it did with it.
    fn input(&mut self, event: &Event, pointer: Option<Point>, now: Instant, out: &mut Vec<M>) -> Routed;

    /// The runtime's elements took `event` before it reached the target:
    /// Tab moving among their stops. A toolkit that hears the window's
    /// events before the runtime does drops it, so one owner acts on a key
    /// (section 6, rule 1). The default does nothing.
    fn kept(&mut self, event: &Event) {
        let _ = event;
    }

    /// Moves the keyboard focus.
    fn focus(&mut self, target: &FocusTarget, select: Selection);

    /// Lets go of the keyboard focus because one of the runtime's own
    /// elements takes it (section 6, rule 1). It must be done before the
    /// next input event reaches the target, so a key never reaches two
    /// owners. A toolkit that applies focus changes after it has routed its
    /// events overrides this to apply the release first.
    fn release_focus(&mut self) {
        self.focus(&FocusTarget::Nothing, Selection::Keep);
    }

    /// Scrolls so the element with `id` is in view.
    fn scroll_into_view(&mut self, id: &UiId);

    /// The element with keyboard focus, if any.
    fn focused(&self) -> Option<UiId>;

    /// True when something the target draws changed in this turn. Asked
    /// once per turn, after the updates; `updates` is how many ran.
    fn needs_redraw(&mut self, updates: usize) -> bool;
}

/// A target with no elements, for an app that draws nothing of its own.
/// A focus or scroll effect sent to it is a mistake, so it panics, naming
/// the effect.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoTarget;

impl<M> Target<M> for NoTarget {
    fn input(&mut self, _event: &Event, _pointer: Option<Point>, _now: Instant, _out: &mut Vec<M>) -> Routed {
        Routed::Passed
    }

    fn focus(&mut self, _target: &FocusTarget, _select: Selection) {
        panic!("{}: there are no elements to {}", NO_ELEMENTS, names::FOCUS);
    }

    fn scroll_into_view(&mut self, _id: &UiId) {
        panic!("{}: there are no elements to {}", NO_ELEMENTS, names::SCROLL_INTO_VIEW);
    }

    fn focused(&self) -> Option<UiId> {
        None
    }

    fn needs_redraw(&mut self, _updates: usize) -> bool {
        false
    }
}

/// The start of [`NoTarget`]'s panic text.
pub const NO_ELEMENTS: &str = "no target";

#[cfg(test)]
mod tests;
