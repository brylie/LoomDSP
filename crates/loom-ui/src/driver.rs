//! What a driver does for the runtime.
//!
//! A driver feeds the runtime [`DriverEvent`](crate::DriverEvent)s,
//! draws when a turn's [`Next`](crate::Next) says to, and carries out the
//! effects that reach outside the app: the clipboard, dialogs, background
//! work, capturing the window, and ending.
//! The window shell is one driver and the headless driver is another. The
//! runtime names no window toolkit, so a third, such as a plugin host's
//! window, needs no change to it (ui-layer.md, section 2, rule 9).
//!
//! Every answer comes back as a message through the [`Waker`], for the
//! next turn. A driver never calls `update`.

use crate::app::WindowSpec;
use crate::effect::{Choice, FileRequest, Image, Job, NoGpu, Question, Reply};
use crate::waker::{Wake, Waker};
use loom_ui_core::UiId;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// One paste answer: the field that asked, the focus's move count when it
/// asked, and the clipboard's text.
pub(crate) type PasteAnswer = (UiId, u64, Option<String>);

/// Where a driver answers a text field's paste: the clipboard's text goes
/// back to the runtime, which puts it in that field on a later turn if the
/// field still has focus (ui-layer.md, section 8, rule 2). Nothing waits
/// on the system: the driver may answer at once or from another thread.
pub struct Paste {
    field: UiId,
    moves: u64,
    answers: Sender<PasteAnswer>,
    wake: Wake,
}

impl Paste {
    /// A paste for the field `field`, asked when the focus had moved
    /// `moves` times, answered on `answers`, waking the loop with `wake`.
    pub(crate) fn new(field: UiId, moves: u64, answers: Sender<PasteAnswer>, wake: Wake) -> Self {
        Self {
            field,
            moves,
            answers,
            wake,
        }
    }

    /// Answers with the clipboard's `text`, or `None` when it holds none,
    /// and wakes the loop. A runtime that is gone takes nothing.
    pub fn give(self, text: Option<String>) {
        if self.answers.send((self.field, self.moves, text)).is_ok() {
            (self.wake)();
        }
    }
}

impl std::fmt::Debug for Paste {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Paste")
            .field("field", &self.field)
            .finish_non_exhaustive()
    }
}

/// What the runtime asks of the thing that runs it.
pub trait Driver<M> {
    /// Apply the window's title and limits. Called only when they changed.
    fn window(&mut self, spec: &WindowSpec);

    /// Put `text` on the clipboard.
    fn clipboard_write(&mut self, text: String);

    /// Read the clipboard, and send the reply's message through `waker`.
    fn clipboard_read(&mut self, reply: Reply<Option<String>, M>, waker: &Waker<M>);

    /// Read the clipboard for a text field's paste, and answer `paste`,
    /// now or later. The default answers that the clipboard is empty, for
    /// a driver with no clipboard.
    fn clipboard_paste(&mut self, paste: Paste) {
        paste.give(None);
    }

    /// Show a file dialog, and send the reply's message through `waker`.
    fn file_dialog(&mut self, request: FileRequest, reply: Reply<Option<PathBuf>, M>, waker: &Waker<M>);

    /// Ask a question, and send the reply's message through `waker`.
    fn ask(&mut self, question: Question, reply: Reply<Choice, M>, waker: &Waker<M>);

    /// Draw the next frame offscreen, and send the reply's message through
    /// `waker`.
    fn capture(&mut self, reply: Reply<Result<Image, NoGpu>, M>, waker: &Waker<M>);

    /// Run `job` off the UI thread. Its messages go through `waker`.
    fn work(&mut self, job: Job<M>, waker: &Waker<M>);

    /// The app asked to quit: end the loop.
    fn exit(&mut self);

    /// Hand the screen reader the accessibility tree (the
    /// [`Accessibility`](crate::Effect::Accessibility) effect). A driver
    /// with no screen reader drops it.
    fn accessibility(&mut self, whole: accesskit::TreeUpdate, changes: Option<accesskit::TreeUpdate>) {
        let _ = (whole, changes);
    }
}
