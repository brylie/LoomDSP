//! When the shell asks for a frame, and how long it may sleep.
//!
//! The rule, with no window in it, so a test can check it (ui-layer.md,
//! section 2, rule 10, and section 3, rule 6):
//!
//! - **Nothing asked, nothing drawn.** A frame is asked for only when a
//!   turn says to draw, the layers ask for one, or the runtime animates.
//!   Otherwise the loop sleeps until an event, a deadline, or a wake-up.
//! - **Many asks make one frame.** Turns that say to draw before the frame
//!   comes are drawn together.
//! - **A frame nobody asked for is the system's.** A resize or an uncovered
//!   window makes the system ask; that frame is sent to the runtime as
//!   `RedrawRequested`. A frame the shell asked for is drawn without it,
//!   so the shell cannot turn its own request into a loop of draws.
//! - **A frame that fails asks for no other.** A frame that was not shown
//!   asks for no next one, even while the runtime animates, so a lost
//!   surface or a window with no size cannot draw in a loop. A lost
//!   surface is set up again and tried once more; after that, and for a
//!   window with no size, the next event's turn asks again.
//! - **The earliest time wins.** The runtime's deadline and the time the
//!   layers want a frame at (a caret's blink) set when the loop wakes.

use crate::runtime::Next;
use std::time::Instant;

/// When the layers want a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Redraw {
    /// Not on their own account.
    #[default]
    No,
    /// As soon as the window can draw one.
    NextFrame,
    /// At this moment: a caret that blinks, say.
    At(Instant),
}

/// How the loop waits after it has done what is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wait {
    /// Until an event or a wake-up comes.
    Sleep,
    /// Until this moment, or an event before it.
    Until(Instant),
    /// Not at all: messages wait, so another turn runs at once.
    Poll,
}

/// What came of drawing a frame into the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drawn {
    /// The frame is shown.
    Shown,
    /// The surface was out of date or lost, and is set up again: the
    /// frame can be drawn again at once.
    Retry,
    /// No frame, and drawing again now would not help: the window has no
    /// size (it waits for a resize), or the surface failed another way.
    Skipped,
}

/// What a frame that begins is, for the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    /// The shell asked for it: draw it with no event for the runtime.
    Asked,
    /// One animation frame: send the runtime `Frame`, then draw.
    Animation,
    /// The system asked for it: send the runtime `RedrawRequested`, then
    /// draw.
    System,
}

/// What came due at a moment the loop woke for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Due {
    /// The runtime's deadline: run a `DeadlineReached` turn.
    pub deadline: bool,
    /// The layers' time for a frame: ask for one.
    pub redraw: bool,
}

/// The redraw rule's state.
#[derive(Debug, Default)]
pub(crate) struct Pacing {
    /// A frame is asked for and has not begun.
    asked: bool,
    /// The runtime animates: a frame after each frame.
    frames: bool,
    /// Messages wait for another turn.
    again: bool,
    /// The runtime's next deadline.
    deadline: Option<Instant>,
    /// When the layers want their next frame.
    layers_at: Option<Instant>,
    /// The last frame found the surface lost and was tried again, so the
    /// next that does waits for an event.
    retried: bool,
}

impl Pacing {
    /// Takes a turn's result. Returns true when the shell must ask the
    /// window for a frame now.
    pub fn turn(&mut self, next: &Next) -> bool {
        self.frames = next.frames;
        self.again = next.again;
        self.deadline = next.deadline;
        self.ask(next.draw || next.frames)
    }

    /// Takes the result of the turn that begins a frame: its `Frame` or
    /// `RedrawRequested`. That turn's draw is the frame being drawn, and
    /// an animation's next frame is asked for once this one ends, so it
    /// asks for nothing now. An ask here would stand until the next
    /// frame began, and stop [`Pacing::frame_ended`] asking for it.
    pub fn frame_turn(&mut self, next: &Next) {
        self.frames = next.frames;
        self.again = next.again;
        self.deadline = next.deadline;
    }

    /// Takes what the layers want. Returns true when the shell must ask
    /// the window for a frame now.
    pub fn layers(&mut self, redraw: Redraw) -> bool {
        match redraw {
            Redraw::No => false,
            Redraw::NextFrame => self.ask(true),
            Redraw::At(at) => {
                self.layers_at = Some(self.layers_at.map_or(at, |before| before.min(at)));
                false
            }
        }
    }

    /// Asks for a frame when `wanted`. Returns true only for the first ask
    /// before the frame begins, so many asks make one frame.
    fn ask(&mut self, wanted: bool) -> bool {
        if wanted && !self.asked {
            self.asked = true;
            return true;
        }
        false
    }

    /// The window has a frame to draw: says what it is, and clears the ask.
    pub fn frame_begins(&mut self) -> FrameKind {
        let kind = if self.frames {
            FrameKind::Animation
        } else if self.asked {
            FrameKind::Asked
        } else {
            FrameKind::System
        };
        self.asked = false;
        kind
    }

    /// The frame ended as `drawn` says. Returns true when the shell must
    /// ask for the next one at once: after a shown frame while the
    /// runtime animates, and once after a lost surface. A frame that was
    /// not shown otherwise asks for none, animation or not.
    pub fn frame_ended(&mut self, drawn: Drawn) -> bool {
        match drawn {
            Drawn::Shown => {
                self.retried = false;
                self.ask(self.frames)
            }
            Drawn::Retry if !self.retried => {
                self.retried = true;
                self.ask(true)
            }
            Drawn::Retry | Drawn::Skipped => false,
        }
    }

    /// The loop woke at `now`: what came due. A time that came due is
    /// cleared, and the layers' time asks for a frame.
    pub fn due(&mut self, now: Instant) -> Due {
        let deadline = self.deadline.is_some_and(|at| at <= now);
        let redraw = self.layers_at.is_some_and(|at| at <= now);
        if redraw {
            self.layers_at = None;
        }
        Due {
            deadline,
            redraw: redraw && self.ask(true),
        }
    }

    /// How the loop waits now.
    pub fn wait(&self) -> Wait {
        if self.again {
            return Wait::Poll;
        }
        let earliest = match (self.deadline, self.layers_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match earliest {
            Some(at) => Wait::Until(at),
            None => Wait::Sleep,
        }
    }
}

#[cfg(test)]
mod tests;
