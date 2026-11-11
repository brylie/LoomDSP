//! A driver with no window, for tests.
//!
//! [`Headless`] carries out effects in memory: the clipboard is a string,
//! background work runs inline and to its end, and a dialog, a question,
//! or a capture gets the answer a test gave first. With no answer it
//! panics, naming the effect, so no test passes by skipping one
//! (ui-layer.md, section 2, rules 3 and 4). The panic text starts with
//! [`NO_ANSWER`], followed by the effect's name from
//! [`names`]; tests may match on it.
//!
//! [`Harness`] holds a runtime, this driver, and a clock the test moves,
//! and runs turns until no message waits.

use crate::app::{App, WindowSpec};
use crate::driver::Driver;
use crate::effect::{Choice, FileRequest, Image, Job, NoGpu, Question, Reply, names};
use crate::event::{DriverEvent, WindowState};
use crate::runtime::{Next, Runtime};
use crate::target::Target;
use crate::waker::Waker;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The start of the panic text for an effect the test gave no answer for.
pub const NO_ANSWER: &str = "the headless driver has no answer for";

/// The most turns [`Harness`] runs in a row before it decides the queue
/// never empties.
const MOST_TURNS: usize = 10_000;

/// A driver that keeps everything in memory.
#[derive(Debug)]
pub struct Headless {
    clipboard: Option<String>,
    files: VecDeque<Option<PathBuf>>,
    choices: VecDeque<Choice>,
    captures: VecDeque<Result<Image, NoGpu>>,
    file_requests: Vec<FileRequest>,
    questions: Vec<Question>,
    window: Option<WindowSpec>,
    window_changes: usize,
    redraws: usize,
    exited: bool,
}

impl Headless {
    /// A driver with an empty clipboard and no answers.
    pub fn new() -> Self {
        Self {
            clipboard: None,
            files: VecDeque::new(),
            choices: VecDeque::new(),
            captures: VecDeque::new(),
            file_requests: Vec::new(),
            questions: Vec::new(),
            window: None,
            window_changes: 0,
            redraws: 0,
            exited: false,
        }
    }

    /// The answer for the next file dialog.
    pub fn answer_file(&mut self, path: Option<PathBuf>) {
        self.files.push_back(path);
    }

    /// The answer for the next question: the index of the button pressed.
    pub fn answer_question(&mut self, choice: Choice) {
        self.choices.push_back(choice);
    }

    /// The answer for the next capture.
    pub fn answer_capture(&mut self, image: Result<Image, NoGpu>) {
        self.captures.push_back(image);
    }

    /// Sets the clipboard's text.
    pub fn set_clipboard(&mut self, text: Option<String>) {
        self.clipboard = text;
    }

    /// The clipboard's text.
    pub fn clipboard(&self) -> Option<&str> {
        self.clipboard.as_deref()
    }

    /// Every file dialog shown, in order.
    pub fn file_requests(&self) -> &[FileRequest] {
        &self.file_requests
    }

    /// Every question asked, in order.
    pub fn questions(&self) -> &[Question] {
        &self.questions
    }

    /// The window spec last applied.
    pub fn window_spec(&self) -> Option<&WindowSpec> {
        self.window.as_ref()
    }

    /// How many times the runtime passed a changed window spec.
    pub fn window_changes(&self) -> usize {
        self.window_changes
    }

    /// How many draws the runtime asked for.
    pub fn redraws(&self) -> usize {
        self.redraws
    }

    /// True once the app has asked to quit.
    pub fn exited(&self) -> bool {
        self.exited
    }
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

/// Panics for an effect with no answer.
fn no_answer(name: &str) -> ! {
    panic!("{NO_ANSWER} {name}; give one before the effect runs")
}

impl<M> Driver<M> for Headless {
    fn redraw(&mut self) {
        self.redraws += 1;
    }

    fn window(&mut self, spec: &WindowSpec) {
        self.window = Some(spec.clone());
        self.window_changes += 1;
    }

    fn clipboard_write(&mut self, text: String) {
        self.clipboard = Some(text);
    }

    fn clipboard_read(&mut self, reply: Reply<Option<String>, M>, waker: &Waker<M>) {
        waker.send(reply.give(self.clipboard.clone()));
    }

    fn file_dialog(&mut self, request: FileRequest, reply: Reply<Option<PathBuf>, M>, waker: &Waker<M>) {
        let Some(answer) = self.files.pop_front() else {
            no_answer(names::FILE_DIALOG);
        };
        self.file_requests.push(request);
        waker.send(reply.give(answer));
    }

    fn ask(&mut self, question: Question, reply: Reply<Choice, M>, waker: &Waker<M>) {
        let Some(answer) = self.choices.pop_front() else {
            no_answer(names::ASK);
        };
        self.questions.push(question);
        waker.send(reply.give(answer));
    }

    fn capture(&mut self, reply: Reply<Result<Image, NoGpu>, M>, waker: &Waker<M>) {
        let Some(answer) = self.captures.pop_front() else {
            no_answer(names::CAPTURE);
        };
        waker.send(reply.give(answer));
    }

    fn work(&mut self, job: Job<M>, waker: &Waker<M>) {
        job.run(waker);
    }

    fn exit(&mut self) {
        self.exited = true;
    }
}

/// A runtime driven by [`Headless`], with a clock the test moves.
pub struct Harness<A: App, T> {
    runtime: Runtime<A, T>,
    driver: Headless,
    now: Instant,
    next: Next,
}

impl<A: App, T: Target<A::Message>> Harness<A, T> {
    /// Starts `app` routing to `target`, in a window that starts as
    /// `window`, with the clock at `start`. Runs the first turn, which
    /// reads the app's view, window spec, and deadline.
    pub fn new(app: A, target: T, window: WindowState, start: Instant) -> Self {
        let runtime = Runtime::new(app, target, window, Arc::new(|| {}));
        let mut harness = Self {
            runtime,
            driver: Headless::new(),
            now: start,
            next: Next {
                again: false,
                frames: false,
                deadline: None,
            },
        };
        harness.step(DriverEvent::Woken);
        harness
    }

    /// Runs a turn for `event`, then more turns until no message waits.
    ///
    /// # Panics
    ///
    /// If messages keep coming for more turns than any test needs.
    pub fn step(&mut self, event: DriverEvent) -> Next {
        self.next = self.runtime.handle(event, self.now, &mut self.driver);
        let mut turns = 1;
        while self.next.again {
            assert!(
                turns < MOST_TURNS,
                "the queue never empties: each turn queues another message"
            );
            self.next = self.runtime.handle(DriverEvent::Woken, self.now, &mut self.driver);
            turns += 1;
        }
        self.next
    }

    /// Sends `message` through the runtime's waker, as a watcher thread
    /// would, and runs turns until no message waits.
    pub fn send(&mut self, message: A::Message) -> Next {
        self.runtime.waker().send(message);
        self.step(DriverEvent::Woken)
    }

    /// Moves the clock on by `by`, waking the runtime at each deadline on
    /// the way, as a driver waiting with a timer would.
    pub fn advance(&mut self, by: Duration) -> Next {
        let end = self.now + by;
        let mut wakes = 0;
        while let Some(at) = self.next.deadline.filter(|at| *at <= end) {
            assert!(
                wakes < MOST_TURNS,
                "the deadline never moves on: handling its message must set a later one"
            );
            self.now = self.now.max(at);
            self.step(DriverEvent::DeadlineReached);
            wakes += 1;
        }
        self.now = end;
        self.next
    }

    /// The clock.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// What the last turn asked for.
    pub fn next(&self) -> Next {
        self.next
    }

    /// The runtime.
    pub fn runtime(&self) -> &Runtime<A, T> {
        &self.runtime
    }

    /// The app.
    pub fn app(&self) -> &A {
        self.runtime.app()
    }

    /// The driver, to read.
    pub fn driver(&self) -> &Headless {
        &self.driver
    }

    /// The driver, to give it answers.
    pub fn driver_mut(&mut self) -> &mut Headless {
        &mut self.driver
    }
}

#[cfg(test)]
mod tests;
