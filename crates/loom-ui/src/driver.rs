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
use crate::waker::Waker;
use std::path::PathBuf;

/// What the runtime asks of the thing that runs it.
pub trait Driver<M> {
    /// Apply the window's title and limits. Called only when they changed.
    fn window(&mut self, spec: &WindowSpec);

    /// Put `text` on the clipboard.
    fn clipboard_write(&mut self, text: String);

    /// Read the clipboard, and send the reply's message through `waker`.
    fn clipboard_read(&mut self, reply: Reply<Option<String>, M>, waker: &Waker<M>);

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
}
