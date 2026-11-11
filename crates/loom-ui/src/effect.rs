//! What the app asks for after an update: effects, as plain values.
//!
//! `update` applies one message and returns [`Effects`], a list of
//! [`Effect`] values. An effect is not a future and runs nothing by
//! itself. The runtime, the routing target, or the driver carries it out
//! after `update` returns, so a test can read every effect, answer it, or
//! refuse it (docs/design/ui-layer.md, section 2, rules 2 and 3).
//!
//! An effect that has an answer, such as a dialog's result, holds a
//! [`Reply`] that turns the answer into one of the app's messages. That
//! message waits for the next turn: an effect never calls `update` again
//! in the same turn (rule 5).
//!
//! # Who carries out each effect
//!
//! | Effect | Carried out by |
//! |---|---|
//! | [`Effect::Redraw`], [`Effect::AfterDraw`], [`Effect::Quit`] | The runtime, the same under every driver |
//! | [`Effect::Focus`], [`Effect::ScrollIntoView`] | The routing target that holds the element |
//! | The rest | The driver: a window shell, or the headless driver |

use crate::waker::Waker;
use loom_ui_core::UiId;
use std::fmt;
use std::path::PathBuf;

/// The stable names of the effects, used in debug output and in the panic
/// text of a driver that cannot carry one out. Tests may match on them.
pub mod names {
    /// [`super::Effect::Focus`].
    pub const FOCUS: &str = "focus";
    /// [`super::Effect::ScrollIntoView`].
    pub const SCROLL_INTO_VIEW: &str = "scroll into view";
    /// [`super::Effect::ClipboardWrite`].
    pub const CLIPBOARD_WRITE: &str = "clipboard write";
    /// [`super::Effect::FileDialog`].
    pub const FILE_DIALOG: &str = "file dialog";
    /// [`super::Effect::Ask`].
    pub const ASK: &str = "question";
    /// [`super::Effect::Work`].
    pub const WORK: &str = "background work";
    /// [`super::Effect::Capture`].
    pub const CAPTURE: &str = "window capture";
    /// [`super::Effect::AfterDraw`].
    pub const AFTER_DRAW: &str = "after draw";
    /// [`super::Effect::Redraw`].
    pub const REDRAW: &str = "redraw";
    /// [`super::Effect::Quit`].
    pub const QUIT: &str = "quit";
}

/// Turns the answer to an effect into one of the app's messages.
pub struct Reply<T, M>(Box<dyn FnOnce(T) -> M + Send>);

impl<T, M> Reply<T, M> {
    /// A reply that makes its message with `make`.
    pub fn new(make: impl FnOnce(T) -> M + Send + 'static) -> Self {
        Self(Box::new(make))
    }

    /// The message for `answer`.
    pub fn give(self, answer: T) -> M {
        (self.0)(answer)
    }
}

impl<T, M> fmt::Debug for Reply<T, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Reply")
    }
}

/// Work to run off the UI thread. It gets a [`Waker`] and may send any
/// number of messages through it: one result, or a batch at a time as it
/// goes.
pub struct Job<M>(Box<Work<M>>);

/// The work inside a [`Job`].
type Work<M> = dyn FnOnce(&Waker<M>) + Send;

impl<M> Job<M> {
    /// A job that runs `work`.
    pub fn new(work: impl FnOnce(&Waker<M>) + Send + 'static) -> Self {
        Self(Box::new(work))
    }

    /// Runs the job to its end, sending its messages through `waker`.
    pub fn run(self, waker: &Waker<M>) {
        (self.0)(waker);
    }
}

impl<M> fmt::Debug for Job<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Job")
    }
}

/// Which element a focus effect moves the keyboard focus to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusTarget {
    /// The element with this UI ID. It may name an inner part of a custom
    /// element, which then focuses that part.
    Element(UiId),
    /// The next element in the focus order.
    Next,
    /// The previous element in the focus order.
    Previous,
}

/// What happens to a text field's selection when it takes focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Selection {
    /// The selection and caret stay as the field keeps them.
    #[default]
    Keep,
    /// All of its text is selected, ready to be typed over.
    All,
}

/// Whether a file dialog picks a file to open or a place to save one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    /// Pick an existing file.
    Open,
    /// Pick a place and a name for a new or replaced file.
    Save,
}

/// One kind of file a dialog offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFilter {
    /// What the user reads, such as "LoomDSP patch".
    pub name: String,
    /// The extensions it covers, without the dot.
    pub extensions: Vec<String>,
}

/// What a file dialog shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRequest {
    /// Open or save.
    pub action: FileAction,
    /// The dialog's title.
    pub title: String,
    /// The kinds of file offered. Empty offers every file.
    pub filters: Vec<FileFilter>,
    /// The folder the dialog starts in, or `None` to let the system choose.
    pub folder: Option<PathBuf>,
    /// The file name the save dialog starts with.
    pub file_name: Option<String>,
}

/// How serious a question is, which decides its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Information.
    Info,
    /// Something may be lost.
    Warning,
    /// Something went wrong.
    Error,
}

/// A question in a native dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// How serious it is.
    pub level: Level,
    /// The dialog's title.
    pub title: String,
    /// The question itself.
    pub text: String,
    /// The buttons, in the order the platform shows them, by their labels.
    pub buttons: Vec<String>,
}

/// The answer to a [`Question`]: the index of the button pressed, or
/// `None` when the dialog was closed with no button.
pub type Choice = Option<usize>;

/// A captured window: its size in physical pixels and its pixels as
/// 8-bit RGBA, row by row from the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    /// An image, or `None` when `rgba` does not hold exactly four bytes for
    /// each pixel.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let expected = usize::try_from(u64::from(width) * u64::from(height) * 4).ok()?;
        (rgba.len() == expected).then_some(Self { width, height, rgba })
    }

    /// Its width in physical pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Its height in physical pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Its pixels as 8-bit RGBA, row by row from the top.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// The answer to a capture where no GPU adapter can draw the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoGpu;

/// One thing the app asks for after an update.
pub enum Effect<M> {
    /// Move the keyboard focus.
    Focus {
        /// Where to.
        target: FocusTarget,
        /// What happens to a text field's selection.
        select: Selection,
    },
    /// Scroll so the element with this UI ID is in view.
    ScrollIntoView(UiId),
    /// Put this text on the clipboard.
    ClipboardWrite(String),
    /// Show a file dialog. The reply gets the path chosen, or `None`.
    FileDialog {
        /// What the dialog shows.
        request: FileRequest,
        /// Makes the message for the result.
        reply: Reply<Option<PathBuf>, M>,
    },
    /// Ask a question in a native dialog. The reply gets the button.
    Ask {
        /// What the dialog asks.
        question: Question,
        /// Makes the message for the answer.
        reply: Reply<Choice, M>,
    },
    /// Run work off the UI thread.
    Work(Job<M>),
    /// Draw the next frame offscreen and reply with the image.
    Capture(Reply<Result<Image, NoGpu>, M>),
    /// Queue this message once the next frame has been drawn and shown.
    AfterDraw(M),
    /// Draw the window again although no panel model changed: a request
    /// handed to a custom element that it applies when it draws.
    Redraw,
    /// End the loop.
    Quit,
}

impl<M> Effect<M> {
    /// The effect's stable name, from [`names`].
    pub fn name(&self) -> &'static str {
        match self {
            Self::Focus { .. } => names::FOCUS,
            Self::ScrollIntoView(_) => names::SCROLL_INTO_VIEW,
            Self::ClipboardWrite(_) => names::CLIPBOARD_WRITE,
            Self::FileDialog { .. } => names::FILE_DIALOG,
            Self::Ask { .. } => names::ASK,
            Self::Work(_) => names::WORK,
            Self::Capture(_) => names::CAPTURE,
            Self::AfterDraw(_) => names::AFTER_DRAW,
            Self::Redraw => names::REDRAW,
            Self::Quit => names::QUIT,
        }
    }
}

impl<M> fmt::Debug for Effect<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Focus { target, select } => f
                .debug_struct("Focus")
                .field("target", target)
                .field("select", select)
                .finish(),
            Self::ScrollIntoView(id) => f.debug_tuple("ScrollIntoView").field(id).finish(),
            Self::ClipboardWrite(text) => f.debug_tuple("ClipboardWrite").field(text).finish(),
            Self::FileDialog { request, .. } => f.debug_struct("FileDialog").field("request", request).finish(),
            Self::Ask { question, .. } => f.debug_struct("Ask").field("question", question).finish(),
            Self::Work(_) => f.write_str("Work"),
            Self::Capture(_) => f.write_str("Capture"),
            Self::AfterDraw(_) => f.write_str("AfterDraw"),
            Self::Redraw => f.write_str("Redraw"),
            Self::Quit => f.write_str("Quit"),
        }
    }
}

/// The effects one update returns, in the order they run.
pub struct Effects<M> {
    list: Vec<Effect<M>>,
}

impl<M> Effects<M> {
    /// No effects.
    pub fn none() -> Self {
        Self { list: Vec::new() }
    }

    /// One effect.
    pub fn one(effect: Effect<M>) -> Self {
        Self { list: vec![effect] }
    }

    /// These effects, then `other`'s.
    #[must_use]
    pub fn and(mut self, other: Self) -> Self {
        self.list.extend(other.list);
        self
    }

    /// Adds one effect at the end.
    pub fn push(&mut self, effect: Effect<M>) {
        self.list.push(effect);
    }

    /// True when there are no effects.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// How many effects there are.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// The effects, in order, to read.
    pub fn iter(&self) -> std::slice::Iter<'_, Effect<M>> {
        self.list.iter()
    }

    /// Focus the element with `id`.
    pub fn focus(id: UiId) -> Self {
        Self::one(Effect::Focus {
            target: FocusTarget::Element(id),
            select: Selection::Keep,
        })
    }

    /// Focus the text field with `id`, with all of its text selected.
    pub fn focus_and_select_all(id: UiId) -> Self {
        Self::one(Effect::Focus {
            target: FocusTarget::Element(id),
            select: Selection::All,
        })
    }

    /// Move the focus to the next element, or to the previous one when
    /// `back` is true.
    pub fn focus_step(back: bool) -> Self {
        let target = if back { FocusTarget::Previous } else { FocusTarget::Next };
        Self::one(Effect::Focus {
            target,
            select: Selection::Keep,
        })
    }

    /// Scroll the element with `id` into view.
    pub fn scroll_into_view(id: UiId) -> Self {
        Self::one(Effect::ScrollIntoView(id))
    }

    /// Put `text` on the clipboard.
    pub fn clipboard_write(text: impl Into<String>) -> Self {
        Self::one(Effect::ClipboardWrite(text.into()))
    }

    /// Show a file dialog, and make a message from the path chosen.
    pub fn file_dialog(request: FileRequest, reply: impl FnOnce(Option<PathBuf>) -> M + Send + 'static) -> Self {
        Self::one(Effect::FileDialog {
            request,
            reply: Reply::new(reply),
        })
    }

    /// Ask a question, and make a message from the button pressed.
    pub fn ask(question: Question, reply: impl FnOnce(Choice) -> M + Send + 'static) -> Self {
        Self::one(Effect::Ask {
            question,
            reply: Reply::new(reply),
        })
    }

    /// Run `work` off the UI thread and queue the one message it returns.
    pub fn work(work: impl FnOnce() -> M + Send + 'static) -> Self {
        Self::job(move |waker| {
            waker.send(work());
        })
    }

    /// Run `work` off the UI thread. It sends its messages through the
    /// waker it gets, as many as it likes.
    pub fn job(work: impl FnOnce(&Waker<M>) + Send + 'static) -> Self {
        Self::one(Effect::Work(Job::new(work)))
    }

    /// Capture the next frame, and make a message from the image.
    pub fn capture(reply: impl FnOnce(Result<Image, NoGpu>) -> M + Send + 'static) -> Self {
        Self::one(Effect::Capture(Reply::new(reply)))
    }

    /// Queue `message` once the next frame has been drawn and shown.
    pub fn after_draw(message: M) -> Self {
        Self::one(Effect::AfterDraw(message))
    }

    /// Draw the window again although no panel model changed.
    pub fn redraw() -> Self {
        Self::one(Effect::Redraw)
    }

    /// End the loop.
    pub fn quit() -> Self {
        Self::one(Effect::Quit)
    }
}

impl<M> Default for Effects<M> {
    fn default() -> Self {
        Self::none()
    }
}

impl<M> fmt::Debug for Effects<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.list).finish()
    }
}

impl<M> From<Effect<M>> for Effects<M> {
    fn from(effect: Effect<M>) -> Self {
        Self::one(effect)
    }
}

impl<M> FromIterator<Effect<M>> for Effects<M> {
    fn from_iter<I: IntoIterator<Item = Effect<M>>>(iter: I) -> Self {
        Self {
            list: iter.into_iter().collect(),
        }
    }
}

impl<M> FromIterator<Effects<M>> for Effects<M> {
    fn from_iter<I: IntoIterator<Item = Effects<M>>>(iter: I) -> Self {
        Self {
            list: iter.into_iter().flat_map(|effects| effects.list).collect(),
        }
    }
}

impl<M> Extend<Effect<M>> for Effects<M> {
    fn extend<I: IntoIterator<Item = Effect<M>>>(&mut self, iter: I) {
        self.list.extend(iter);
    }
}

impl<M> IntoIterator for Effects<M> {
    type Item = Effect<M>;
    type IntoIter = std::vec::IntoIter<Effect<M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

impl<'a, M> IntoIterator for &'a Effects<M> {
    type Item = &'a Effect<M>;
    type IntoIter = std::slice::Iter<'a, Effect<M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

#[cfg(test)]
mod tests;
