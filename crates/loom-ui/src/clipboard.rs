//! The system clipboard, on a thread of its own (ui-layer.md, section 8,
//! rule 2).
//!
//! One long-lived clipboard does every read and write, from one thread:
//! Windows opens its clipboard on one thread at a time, and on Linux the app
//! that copied serves the text while it keeps its clipboard alive. The thread
//! is the only place a system call may block, so a driver never waits on it:
//! a write sends and goes on, and a read answers later, through a closure.
//!
//! [`Clipboard::system`] is the one for the process, shared by every driver
//! in it, as several plugin instances in one host are. It reads and writes
//! text only. A read that cannot complete answers
//! [`ClipboardText::Refused`], never empty, so the app can say so.
//!
//! On Linux nothing waits at quit: what was copied stays only if the
//! desktop's clipboard manager took it. On Wayland the compositor's
//! data-control protocol is used where it has one, and X11 through XWayland
//! where it does not.

use crate::effect::ClipboardText;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

/// How long [`Clipboard::read_now`] waits for the clipboard's thread before
/// it answers [`ClipboardText::Refused`]. *Why:* it runs on the UI thread,
/// for a toolkit whose reads are synchronous, and a clipboard another app
/// holds on Windows, an X11 owner that does not answer, or a job ahead of it
/// must not freeze the window. A quarter of a second is long enough for a
/// clipboard that answers at all, and short enough to feel like a key press
/// that did not work, which the status message then explains.
pub const READ_NOW_LIMIT: Duration = Duration::from_millis(250);

/// What the clipboard thread reads and writes through: the system's
/// clipboard, or a fake in a test.
pub trait Backend: Send + 'static {
    /// The clipboard's text, nothing, or a refusal.
    fn get(&mut self) -> ClipboardText;
    /// Puts `text` on the clipboard.
    fn set(&mut self, text: String);
}

/// One job for the thread.
enum Job {
    /// Put this text on the clipboard.
    Write(String),
    /// Read the clipboard, and give the answer to this.
    Read(Box<dyn FnOnce(ClipboardText) + Send>),
}

/// A handle to a clipboard thread. Clones share the thread.
#[derive(Clone)]
pub struct Clipboard {
    jobs: Sender<Job>,
}

impl std::fmt::Debug for Clipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clipboard").finish_non_exhaustive()
    }
}

impl Clipboard {
    /// A clipboard thread over `backend`. The thread ends when the last
    /// handle goes.
    pub fn new(backend: impl Backend) -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let mut backend = backend;
        // A failed spawn leaves the queue dropped, so every read answers
        // `Refused` and every write is lost: the app still never waits.
        let _ = std::thread::Builder::new()
            .name("loom-ui clipboard".to_owned())
            .spawn(move || {
                for job in queue {
                    match job {
                        Job::Write(text) => backend.set(text),
                        Job::Read(answer) => answer(backend.get()),
                    }
                }
            });
        Self { jobs }
    }

    /// The system clipboard, one for the process, made on first use.
    pub fn system() -> Self {
        static SYSTEM: OnceLock<Clipboard> = OnceLock::new();
        SYSTEM.get_or_init(|| Self::new(System::default())).clone()
    }

    /// Puts `text` on the clipboard, and does not wait.
    pub fn write(&self, text: String) {
        let _ = self.jobs.send(Job::Write(text));
    }

    /// Reads the clipboard, and gives the answer to `answer`, on the
    /// clipboard's thread. With the thread gone, it answers `Refused` at
    /// once.
    pub fn read(&self, answer: impl FnOnce(ClipboardText) + Send + 'static) {
        if let Err(mpsc::SendError(Job::Read(answer))) = self.jobs.send(Job::Read(Box::new(answer))) {
            answer(ClipboardText::Refused);
        }
    }

    /// Reads the clipboard and waits for the answer, at most
    /// [`READ_NOW_LIMIT`], then answers `Refused`. Only for a toolkit whose
    /// own clipboard calls are synchronous, as Iced's are while it still
    /// draws (a bridge, gone in phase 4); a driver uses [`read`](Self::read).
    /// An answer that comes later goes to a channel no one holds.
    pub fn read_now(&self) -> ClipboardText {
        self.read_within(READ_NOW_LIMIT)
    }

    /// [`read_now`](Self::read_now), waiting at most `limit`.
    fn read_within(&self, limit: Duration) -> ClipboardText {
        let (sender, answer) = mpsc::channel();
        self.read(move |text| {
            let _ = sender.send(text);
        });
        answer.recv_timeout(limit).unwrap_or(ClipboardText::Refused)
    }
}

/// The system's clipboard through `arboard`, opened on first use and opened
/// again after a failure, so a clipboard another app held only refuses that
/// read.
#[derive(Default)]
struct System {
    clipboard: Option<arboard::Clipboard>,
}

impl System {
    /// The clipboard, opened if it is not yet.
    fn open(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }
}

impl Backend for System {
    fn get(&mut self) -> ClipboardText {
        let Some(clipboard) = self.open() else {
            return ClipboardText::Refused;
        };
        match clipboard.get_text() {
            Ok(text) => ClipboardText::from_text(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => ClipboardText::Empty,
            Err(_) => ClipboardText::Refused,
        }
    }

    fn set(&mut self, text: String) {
        // Linux: never waits (no `SetExtLinux::wait`), so a quit never
        // pauses; the desktop's clipboard manager keeps the text, if it runs.
        if let Some(clipboard) = self.open()
            && clipboard.set_text(text).is_err()
        {
            self.clipboard = None;
        }
    }
}

#[cfg(test)]
mod tests;
