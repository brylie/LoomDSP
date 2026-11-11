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
use std::time::{Duration, Instant};

/// How long an arrow group stays open after its key comes up, for another
/// quick press to join it (`docs/design/undo.md`).
pub(crate) const ARROW_GROUP: Duration = Duration::from_millis(500);

/// The undo group of arrow-key edits on the canvas. The app owns it, with the
/// gesture it opens: an open group always has an open gesture. It lives in
/// the patch session, so replacing the patch closes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ArrowGroup {
    /// No group is open.
    #[default]
    Closed,
    /// An arrow key is down. Its repeats join the group.
    Held,
    /// The last arrow key came up. An arrow press before `due` joins the
    /// group; the group ends at `due`.
    Released {
        /// When the group ends if no arrow comes.
        due: Instant,
    },
}

/// What happens to an arrow group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrowEvent {
    /// An arrow key is about to edit.
    Step(Instant),
    /// An arrow key was pressed while the group was released.
    Pressed(Instant),
    /// An arrow key came up.
    Released(Instant),
    /// The canvas ends the group: another key, a pointer press, keys
    /// leaving the canvas, or focus loss.
    End,
    /// A redraw at this time passed the deadline the canvas was given.
    Due(Instant),
    /// The app closes a released group before another action: Undo, Redo,
    /// an inspector edit. A held key keeps its group.
    Close,
}

/// What the app does with the gesture for an arrow event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrowEffect {
    /// Nothing.
    Nothing,
    /// End the group's gesture: its edits become one step.
    End,
    /// Open a gesture for a new group.
    Start,
    /// End the old group's gesture, then open one for a new group.
    EndAndStart,
}

impl ArrowEffect {
    /// True if the effect ends the open gesture.
    pub(crate) fn ends(self) -> bool {
        matches!(self, Self::End | Self::EndAndStart)
    }

    /// True if the effect opens a gesture.
    pub(crate) fn starts(self) -> bool {
        matches!(self, Self::Start | Self::EndAndStart)
    }
}

impl ArrowGroup {
    /// The group after `event`, and what the app does with the gesture.
    /// A pure function, so every transition is tested without a clock.
    pub(crate) fn on(self, event: ArrowEvent) -> (Self, ArrowEffect) {
        use ArrowEffect as Do;
        use ArrowEvent as E;
        match (self, event) {
            (Self::Closed, E::Step(_)) => (Self::Held, Do::Start),
            (Self::Closed, _) => (Self::Closed, Do::Nothing),
            (Self::Held, E::Released(at)) => (Self::Released { due: at + ARROW_GROUP }, Do::Nothing),
            (Self::Held, E::End) => (Self::Closed, Do::End),
            (Self::Held, _) => (Self::Held, Do::Nothing),
            (Self::Released { due }, E::Pressed(at)) if at < due => (Self::Held, Do::Nothing),
            (Self::Released { .. }, E::Pressed(_)) => (Self::Closed, Do::End),
            (Self::Released { due }, E::Step(at)) if at < due => (Self::Held, Do::Nothing),
            (Self::Released { .. }, E::Step(_)) => (Self::Held, Do::EndAndStart),
            (Self::Released { .. }, E::Released(at)) => (Self::Released { due: at + ARROW_GROUP }, Do::Nothing),
            (Self::Released { due }, E::Due(at)) if at >= due => (Self::Closed, Do::End),
            (Self::Released { due }, E::Due(_)) => (Self::Released { due }, Do::Nothing),
            (Self::Released { .. }, E::End | E::Close) => (Self::Closed, Do::End),
        }
    }
}

/// The undo group of a screen reader's edits on one control
/// (ui-layer.md, section 7, rule 6). A screen reader sends no key down or
/// key up, so each SetValue, Increment, or Decrement counts as one arrow
/// step let go at once. The app owns the group, with the gesture it opens,
/// beside [`ArrowGroup`]: an open group always has an open gesture.
///
/// It names the control only by the number of its node, so it knows
/// nothing of the patch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ReaderGroup {
    /// No group is open.
    #[default]
    Closed,
    /// An action on `node` is being applied: the step, before its release.
    /// As with a held arrow key, the edit it makes does not close the
    /// group.
    Stepping {
        /// The node of the control the group's edits are on.
        node: u64,
        /// When the action arrived.
        at: Instant,
    },
    /// The last action was on `node`. Another on the same node before `due`
    /// joins the group; the group ends at `due`.
    Open {
        /// The node of the control the group's edits are on.
        node: u64,
        /// When the group ends if no action comes.
        due: Instant,
    },
}

/// What happens to a screen reader's undo group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReaderEvent {
    /// A SetValue, Increment, or Decrement on `node` is about to edit.
    Action {
        /// The node of the control.
        node: u64,
        /// When the action arrived.
        at: Instant,
    },
    /// The action's edit was applied: its release, at once.
    Applied,
    /// A wake at this time passed the deadline the group gave.
    Due(Instant),
    /// Something else happens first: an action that is not a step, a key
    /// or pointer press, focus moving, an inspector edit, Undo, or Redo.
    Close,
}

impl ReaderGroup {
    /// The group after `event`, and what the app does with the gesture.
    /// A pure function, so every transition is tested without a clock.
    pub(crate) fn on(self, event: ReaderEvent) -> (Self, ArrowEffect) {
        use ArrowEffect as Do;
        use ReaderEvent as E;
        match (self, event) {
            (Self::Closed, E::Action { node, at }) => (Self::Stepping { node, at }, Do::Start),
            (Self::Closed, _) => (Self::Closed, Do::Nothing),
            (Self::Stepping { node, at }, E::Applied) => (
                Self::Open {
                    node,
                    due: at + ARROW_GROUP,
                },
                Do::Nothing,
            ),
            (Self::Stepping { .. }, _) => (self, Do::Nothing),
            (Self::Open { node: last, due }, E::Action { node, at }) if node == last && at < due => {
                (Self::Stepping { node, at }, Do::Nothing)
            }
            (Self::Open { .. }, E::Action { node, at }) => (Self::Stepping { node, at }, Do::EndAndStart),
            (Self::Open { due, .. }, E::Due(at)) if at >= due => (Self::Closed, Do::End),
            (Self::Open { .. }, E::Due(_) | E::Applied) => (self, Do::Nothing),
            (Self::Open { .. }, E::Close) => (Self::Closed, Do::End),
        }
    }

    /// When the group next needs a wake to end itself. `None` while no
    /// group is open, so nothing wakes the app for it at rest.
    pub(crate) fn due(self) -> Option<Instant> {
        match self {
            Self::Closed | Self::Stepping { .. } => None,
            Self::Open { due, .. } => Some(due),
        }
    }
}

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
    /// The undo group of arrow-key edits.
    arrow: ArrowGroup,
    /// The undo group of a screen reader's edits.
    reader: ReaderGroup,
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

    /// The undo group of arrow-key edits.
    pub(crate) fn arrow_group(&self) -> ArrowGroup {
        self.arrow
    }

    /// The undo group of a screen reader's edits.
    pub(crate) fn reader_group(&self) -> ReaderGroup {
        self.reader
    }

    /// Sets the screen reader's group, after the app has applied the effect
    /// of an event. See [`ReaderGroup::on`].
    pub(crate) fn set_reader_group(&mut self, group: ReaderGroup) {
        self.reader = group;
    }

    /// Sets the arrow group, after the app has applied the effect of an
    /// event. See [`ArrowGroup::on`].
    pub(crate) fn set_arrow_group(&mut self, group: ArrowGroup) {
        self.arrow = group;
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
        // An open group always has an open gesture, so a later end cannot
        // close a gesture that is not the group's.
        self.arrow = ArrowGroup::Closed;
        self.reader = ReaderGroup::Closed;
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
