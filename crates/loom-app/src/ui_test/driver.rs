//! The `Ui` harness's driver: `loom-ui`'s headless driver, with the fake
//! dialogs in the crate's tests, captures left for the harness to draw,
//! and every accessibility tree kept for a test to read.
//!
//! Effects run in memory: the clipboard is a string, work runs inline, and
//! a dialog with no answer panics, naming it (ui-layer.md, section 2, rules
//! 3 and 4). In the crate's own tests the fake dialogs answer instead.

use crate::Message;
use loom_ui::effect::{Choice, FileRequest, Image, Job, NoGpu, Question, Reply};
use loom_ui::headless::Headless;
use loom_ui::{Driver, Waker, WindowSpec};
use std::path::PathBuf;

/// The harness's driver.
#[derive(Default)]
pub(crate) struct UiDriver {
    /// The clipboard, work, exit, and dialogs with no fake answer.
    pub(crate) headless: Headless,
    /// Capture replies waiting for the harness to draw the window.
    pub(crate) captures: Vec<Reply<Result<Image, NoGpu>, Message>>,
    /// Every whole accessibility tree the app handed the screen reader, in
    /// order, for a test to read what was sent rather than build its own.
    pub(crate) trees: Vec<accesskit::TreeUpdate>,
    /// Incremental updates actually sent after the full tree.
    pub(crate) changes: Vec<accesskit::TreeUpdate>,
}

impl Driver<Message> for UiDriver {
    fn window(&mut self, spec: &WindowSpec) {
        Driver::<Message>::window(&mut self.headless, spec);
    }

    fn clipboard_write(&mut self, text: String) {
        Driver::<Message>::clipboard_write(&mut self.headless, text);
    }

    fn clipboard_read(&mut self, reply: Reply<loom_ui::ClipboardText, Message>, waker: &Waker<Message>) {
        self.headless.clipboard_read(reply, waker);
    }

    fn clipboard_paste(&mut self, paste: loom_ui::Paste) {
        Driver::<Message>::clipboard_paste(&mut self.headless, paste);
    }

    /// Answered from `dialogs::fake` in the crate's tests. In a test under
    /// `tests/` the fakes are off, so the headless driver panics, naming it.
    fn file_dialog(&mut self, request: FileRequest, reply: Reply<Option<PathBuf>, Message>, waker: &Waker<Message>) {
        #[cfg(test)]
        {
            waker.send(reply.give(crate::dialogs::fake::file_answer(&request)));
        }
        #[cfg(not(test))]
        self.headless.file_dialog(request, reply, waker);
    }

    /// As [`UiDriver::file_dialog`], from `dialogs::fake`.
    fn ask(&mut self, question: Question, reply: Reply<Choice, Message>, waker: &Waker<Message>) {
        #[cfg(test)]
        {
            waker.send(reply.give(crate::dialogs::fake::question_answer(&question)));
        }
        #[cfg(not(test))]
        self.headless.ask(question, reply, waker);
    }

    /// Waits for the harness to draw the window and answer.
    fn capture(&mut self, reply: Reply<Result<Image, NoGpu>, Message>, _waker: &Waker<Message>) {
        self.captures.push(reply);
    }

    fn work(&mut self, job: Job<Message>, waker: &Waker<Message>) {
        self.headless.work(job, waker);
    }

    fn exit(&mut self) {
        Driver::<Message>::exit(&mut self.headless);
    }

    /// Records the full tree and the incremental update sent to the adapter.
    fn accessibility(&mut self, whole: accesskit::TreeUpdate, changes: Option<accesskit::TreeUpdate>) {
        self.trees.push(whole);
        self.changes.extend(changes);
    }
}
