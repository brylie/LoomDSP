//! Where input and focus go: the routing target.
//!
//! The runtime hands element events, focus, and scrolling to a [`Target`],
//! and asks it whether the window needs drawing. During phases 2 and 3 of
//! the move off Iced, the target is the bridge that holds Iced's views. In
//! phase 3 it becomes this crate's own element tree. The runtime does not
//! change between them.

use crate::effect::{FocusTarget, Selection, names};
use loom_ui_core::{Event, Point, UiId};
use std::time::Instant;

/// What holds the elements: it takes their events and their focus.
pub trait Target<M> {
    /// Routes one element event, adding any messages it gives to `out`.
    fn input(&mut self, event: &Event, pointer: Option<Point>, now: Instant, out: &mut Vec<M>);

    /// Moves the keyboard focus.
    fn focus(&mut self, target: &FocusTarget, select: Selection);

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
    fn input(&mut self, _event: &Event, _pointer: Option<Point>, _now: Instant, _out: &mut Vec<M>) {}

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
