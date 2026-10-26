//! The state that turns edits into undo steps (`docs/design/undo.md`).
//!
//! [`Edits`] keeps the patch as it was when an edit or gesture began, the
//! parameters the app set since, whether a gesture is open, and whether
//! recording is off after a failed commit. It owns the [`History`] the
//! steps go into.
//!
//! It holds no UI or audio types. The app hands it a snapshot source (a
//! closure that takes the patch as it is now), whether the controller has
//! changes the engine does not have yet, and the result of each commit. So
//! the rules on gestures and failed commits are tested here, without a
//! window or an audio device.

use crate::undo::{self, History, Snapshot};
use std::collections::BTreeSet;

/// Records edits as undo steps.
#[derive(Default)]
pub(crate) struct Edits {
    /// The steps recorded so far.
    pub(crate) history: History,
    /// The patch before the edit in progress, if there is one.
    pending: Option<Snapshot>,
    /// True between a gesture's start and end. Undo waits for it.
    gesture: bool,
    /// True when the open gesture began on an inspector slider. A slider
    /// sends no start, and arrow keys on it send no end, so the next edit
    /// or an undo closes it too.
    slider_gesture: bool,
    /// True after a commit failed, until the graph is sent. The graph holds
    /// a change the engine does not, so no step is recorded: undo would
    /// send that change along with a later one.
    history_off: bool,
    /// Parameters this app set during the edit in progress. A change on a
    /// mapped parameter is part of the step only if it is listed here.
    touched: BTreeSet<(u32, usize)>,
}

impl Edits {
    /// True between a gesture's start and its end.
    pub(crate) fn gesture_open(&self) -> bool {
        self.gesture
    }

    /// True while the open gesture began on an inspector slider.
    pub(crate) fn slider_gesture_open(&self) -> bool {
        self.slider_gesture
    }

    /// True while no step is recorded, after a failed commit.
    #[cfg(test)]
    pub(crate) fn history_off(&self) -> bool {
        self.history_off
    }

    /// Turns recording off or on, for tests of what a failed commit does
    /// next.
    #[cfg(test)]
    pub(crate) fn set_history_off(&mut self, off: bool) {
        self.history_off = off;
    }

    /// Opens a gesture at a canvas gesture start. The caller closes a
    /// slider gesture first.
    pub(crate) fn start_gesture(&mut self) {
        self.gesture = true;
    }

    /// Opens a gesture at a slider's first change, unless one is open.
    pub(crate) fn start_slider_gesture(&mut self) {
        if !self.gesture {
            self.gesture = true;
            self.slider_gesture = true;
        }
    }

    /// Starts recording an edit: keeps `now()`, the patch as it is now.
    /// Does nothing if an edit is already open, so a gesture's first call
    /// is the one that counts. `uncommitted` is true while the controller
    /// has changes the engine does not have.
    pub(crate) fn begin(&mut self, uncommitted: bool, now: impl FnOnce() -> Snapshot) {
        // Another path (probe sync) may have sent the graph since the failure.
        if !self.gesture {
            self.back_on(uncommitted);
        }
        if self.pending.is_none() && !self.history_off {
            self.pending = Some(now());
            self.touched.clear();
        }
    }

    /// Notes that the app set this parameter during the edit in progress.
    pub(crate) fn touch(&mut self, module: u32, index: usize) {
        self.touched.insert((module, index));
    }

    /// Ends the recording and adds a step, named `name` if given, if the
    /// patch changed since [`Edits::begin`]. A gesture stays open: what it
    /// changes from here on becomes a step of its own.
    pub(crate) fn record(&mut self, name: Option<String>, now: impl FnOnce() -> Snapshot) {
        let Some(before) = self.pending.take() else {
            return;
        };
        if let Some(mut step) = undo::diff(&before, &now(), &self.touched) {
            if let Some(name) = name {
                step.rename(name);
            }
            self.history.push(step);
        }
        self.touched.clear();
    }

    /// Closes the open gesture, of either kind, and records it.
    /// `uncommitted` is as for [`Edits::begin`], after the gesture's last
    /// change.
    pub(crate) fn end_gesture(&mut self, uncommitted: bool, name: Option<String>, now: impl FnOnce() -> Snapshot) {
        self.gesture = false;
        self.slider_gesture = false;
        self.record(name, now);
        self.back_on(uncommitted);
    }

    /// Records steps again once the engine has the whole graph.
    fn back_on(&mut self, uncommitted: bool) {
        if !uncommitted {
            self.history_off = false;
        }
    }

    /// Takes the result of sending the graph to the engine.
    ///
    /// A failed commit leaves a change in the graph that the engine does
    /// not have. Recording it would put a step in the history for
    /// something that does not play, so the edit in progress is dropped,
    /// the history is cleared, and recording stays off until the graph is
    /// sent (undo.md, rule 10). Inside a gesture it stays off until the
    /// gesture's end, so the rest of a cut gesture stays out of the
    /// history as a whole.
    pub(crate) fn committed(&mut self, sent: bool) {
        if sent {
            if !self.gesture {
                self.history_off = false;
            }
        } else {
            self.pending = None;
            self.touched.clear();
            self.history.clear();
            self.history_off = true;
        }
    }
}

#[cfg(test)]
mod tests;
