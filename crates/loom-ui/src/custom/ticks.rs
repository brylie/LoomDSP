//! A custom element's ticks: at its deadline and at each animation frame
//! while it animates (ui-layer.md, section 9, rule 4).
//!
//! The runtime never reads the clock. A deadline is worked out from the
//! moments the element was given, and its tick comes with the driver's
//! moment for the turn that woke at it. A tick is not input, so an element
//! under an open overlay ticks too.

use super::{CustomEvent, Schedule};
use crate::app::App;
use crate::draw::Panels;
use loom_ui_core::UiId;
use std::time::Instant;

impl<A: App> Panels<'_, A> {
    /// Gives a deadline tick to each element whose deadline came by `now`.
    /// What they send runs in this turn, after its batch.
    pub(crate) fn tick_deadlines(&mut self, now: Instant) {
        // First, each element a layout placed, moved, or resized hears it
        // once, before any deadline of this turn (section 9, rule 4).
        let placed: Vec<UiId> = self
            .customs
            .elements
            .iter_mut()
            .filter_map(|(id, hosted)| std::mem::take(&mut hosted.placed).then(|| id.clone()))
            .collect();
        for id in placed {
            self.tick(&id, &CustomEvent::Placed);
        }
        self.tick_due(now);
    }

    /// Gives a deadline tick to each element whose deadline came by `now`,
    /// with no placement: at a turn's start, and again after its updates,
    /// for a deadline an update made due. What they send runs in this turn.
    pub(crate) fn tick_due(&mut self, now: Instant) {
        let due: Vec<UiId> = self
            .customs
            .elements
            .iter()
            .filter(|(_, hosted)| {
                hosted
                    .state
                    .as_deref()
                    .and_then(|state| state.next_deadline(self.app))
                    .is_some_and(|at| at <= now)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            self.tick(&id, &CustomEvent::Deadline);
            self.check_moved_on(&id, now);
        }
    }

    /// Checks that the element `id` moved its deadline past `now` in its
    /// tick. One still due would wake the loop at once, every turn: a
    /// debug build panics, and a release build marks it stuck, so the
    /// schedule leaves it out until it gives a later one.
    fn check_moved_on(&mut self, id: &UiId, now: Instant) {
        let Some(hosted) = self.customs.elements.get_mut(id) else {
            return;
        };
        let still = hosted
            .state
            .as_deref()
            .and_then(|state| state.next_deadline(self.app))
            .filter(|at| *at <= now);
        debug_assert!(
            still.is_none(),
            "a custom element's deadline did not move past its tick: clear it or move it on in event"
        );
        hosted.stuck = still;
    }

    /// Gives a frame tick to each element that animates. A frame runs no
    /// update, so it returns what they sent, which a debug build refuses
    /// and a release build runs on the next turn ([`CustomEvent::Frame`]).
    pub(crate) fn tick_frames(&mut self) -> Vec<A::Message> {
        let animating: Vec<UiId> = self
            .customs
            .elements
            .iter()
            .filter(|(_, hosted)| hosted.state.as_deref().is_some_and(|state| state.animating(self.app)))
            .map(|(id, _)| id.clone())
            .collect();
        let before = self.customs.sent.len();
        for id in animating {
            self.tick(&id, &CustomEvent::Frame);
        }
        let sent = self.customs.sent.split_off(before);
        debug_assert!(
            sent.is_empty(),
            "a custom element sent a message from a frame's tick: ask for a deadline instead, \
             and send it from the turn that deadline wakes"
        );
        sent
    }

    /// Gives `tick` to the element `id`, with the pointer where it is.
    fn tick(&mut self, id: &UiId, tick: &CustomEvent) {
        if let Some(moment) = self.custom_moment(id, self.pointer.at) {
            let _ = self.customs.tell(self.app, id, tick, &moment);
        }
    }

    /// Wakes the loop for a turn when a call outside a turn changed what
    /// only a turn tells the driver or the app: an element asked for a
    /// frame, its nodes changed, a layout placed, moved, or resized it, or the
    /// elements' deadline or frames changed, as a state a layout made
    /// does. Transitional, as [`Panels::send_custom_later`] is: from P4
    /// every event reaches an element inside a turn.
    pub(crate) fn wake_if_rescheduled(&mut self) {
        let schedule: Schedule = self.customs.schedule(self.app);
        // An element a layout placed, moved, or resized hears it on the
        // next turn, which this wakes (section 9, rule 4).
        if self.customs.redraw_asked()
            || self.customs.nodes_reported()
            || self.customs.any_placed()
            || schedule != self.customs.planned
        {
            self.waker.nudge();
        }
    }
}

#[cfg(test)]
mod tests;
