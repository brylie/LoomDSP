//! The system clipboard, on a thread of its own (ui-layer.md, section 8,
//! rule 2).
//!
//! One thread does every read and write: Windows opens its clipboard on one
//! thread at a time. It keeps two handles. The writing one never leaves the
//! thread, because on Linux the app that copied serves the text while it
//! keeps its clipboard alive, so a read that goes wrong must never take our
//! copy with it. The reading one is lent to a helper thread for each read,
//! so a read that never finishes holds only itself (see `Reader`). The
//! thread is the only place a system call may block, so a driver never
//! waits on it: a write sends and goes on, and a read answers later,
//! through a closure.
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
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::Duration;

/// How long [`Clipboard::read_now`] waits for the clipboard's thread before
/// it answers [`ClipboardText::Refused`]. *Why:* it runs on the UI thread,
/// for a toolkit whose reads are synchronous, and a clipboard another app
/// holds on Windows, an X11 owner that does not answer, or a job ahead of it
/// must not freeze the window. A quarter of a second is long enough for a
/// clipboard that answers at all, and short enough to feel like a key press
/// that did not work, which the status message then explains.
pub const READ_NOW_LIMIT: Duration = Duration::from_millis(250);

/// How long the clipboard's thread waits for one read from its backend
/// before it answers [`ClipboardText::Refused`] and goes on to the next job.
/// *Why:* an owner that never answers must not hold every later read and
/// write. On Wayland, `arboard` reads a transfer
/// with no limit of its own; on X11 it waits 4 seconds, then answers as if
/// the clipboard were empty. A second is shorter than that, so a clipboard
/// that does not answer reads as refused, never empty, and is long enough
/// for one that answers at all.
pub const BACKEND_READ_LIMIT: Duration = Duration::from_secs(1);

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
    /// A clipboard thread over the backends `open` makes: one to write
    /// through, and one to read through, opened again only after a read
    /// failed hard. The thread ends when the last handle goes.
    pub fn new<B: Backend>(open: impl FnMut() -> B + Send + 'static) -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let mut open = open;
        // A failed spawn leaves the queue dropped, so every read answers
        // `Refused` and every write is lost: the app still never waits.
        let _ = std::thread::Builder::new()
            .name("loom-ui clipboard".to_owned())
            .spawn(move || {
                let mut writer: Option<B> = None;
                let mut reader = Reader::default();
                for job in queue {
                    match job {
                        Job::Write(text) => writer.get_or_insert_with(&mut open).set(text),
                        Job::Read(answer) => answer(reader.read(&mut open, BACKEND_READ_LIMIT)),
                    }
                }
            });
        Self { jobs }
    }

    /// The system clipboard, one for the process, made on first use.
    pub fn system() -> Self {
        static SYSTEM: OnceLock<Clipboard> = OnceLock::new();
        SYSTEM
            .get_or_init(|| Self::new(System::<arboard::Clipboard>::default))
            .clone()
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
        let (sender, answer) = mpsc::channel();
        self.read(move |text| {
            let _ = sender.send(text);
        });
        answer.recv_timeout(READ_NOW_LIMIT).unwrap_or(ClipboardText::Refused)
    }
}

/// A native clipboard handle: `arboard`'s, or a fake in a test, so the
/// opening and opening again of [`System`] is tested with no display.
trait Native: Sized + Send + 'static {
    /// Opens a handle.
    fn open() -> Result<Self, arboard::Error>;
    /// The clipboard's text.
    fn get_text(&mut self) -> Result<String, arboard::Error>;
    /// Puts `text` on the clipboard.
    fn set_text(&mut self, text: String) -> Result<(), arboard::Error>;
}

impl Native for arboard::Clipboard {
    fn open() -> Result<Self, arboard::Error> {
        Self::new()
    }

    fn get_text(&mut self) -> Result<String, arboard::Error> {
        Self::get_text(self)
    }

    fn set_text(&mut self, text: String) -> Result<(), arboard::Error> {
        // Linux: never waits (no `SetExtLinux::wait`), so a quit never
        // pauses; the desktop's clipboard manager keeps the text, if it runs.
        Self::set_text(self, text)
    }
}

/// The system's clipboard through `arboard`, opened on first use and opened
/// again after a write failed, so a clipboard another app held only refuses
/// that read or loses that write.
struct System<N = arboard::Clipboard> {
    clipboard: Option<N>,
}

impl<N> Default for System<N> {
    fn default() -> Self {
        Self { clipboard: None }
    }
}

impl<N: Native> System<N> {
    /// The clipboard, opened if it is not yet.
    fn open(&mut self) -> Option<&mut N> {
        if self.clipboard.is_none() {
            self.clipboard = N::open().ok();
        }
        self.clipboard.as_mut()
    }
}

impl<N: Native> Backend for System<N> {
    fn get(&mut self) -> ClipboardText {
        let Some(clipboard) = self.open() else {
            return ClipboardText::Refused;
        };
        answer(clipboard.get_text())
    }

    fn set(&mut self, text: String) {
        if let Some(clipboard) = self.open()
            && clipboard.set_text(text).is_err()
        {
            self.clipboard = None;
        }
    }
}

/// How many reads answer `Refused` at once while the reading handle is out
/// with a stuck read, before the next one gives that read up and tries
/// again with a new handle. *Why:* a clipboard that was stuck may answer
/// again, such as after the app that held it quits, and the user must not
/// be refused for good. It counts reads, not time, because the crate never
/// reads the clock.
pub const READS_BEFORE_RETRY: usize = 3;

/// How many reads may be stuck at once. A stuck read cannot be cancelled,
/// so each keeps its thread and handle until it ends; past this many, every
/// read answers `Refused` until one does. *Why:* a clipboard that stays
/// stuck must not gather one blocked thread per paste.
pub const MAX_STUCK_READS: usize = 2;

/// The handle the clipboard's thread reads through, apart from the one it
/// writes through, so a stuck read never takes the writing one, which
/// holds our copy on Linux.
///
/// Each read runs on a helper thread that holds the handle. A read that
/// does not finish in time leaves it out with its helper. While it is out,
/// reads answer `Refused` at once, with no new thread, until it comes back
/// or [`READS_BEFORE_RETRY`] of them have, when the next read gives the
/// stuck one up and opens a new handle, if fewer than [`MAX_STUCK_READS`]
/// are stuck.
struct Reader<B> {
    /// Here, or not opened yet, or lost to a read that failed hard or was
    /// given up.
    ready: Option<B>,
    /// Reads that did not finish in time, oldest first: each answer comes
    /// back on its channel, with its handle.
    stuck: Vec<Receiver<(ClipboardText, B)>>,
    /// Reads refused at once since the last read reached a backend.
    refused: usize,
}

impl<B> Default for Reader<B> {
    fn default() -> Self {
        Self {
            ready: None,
            stuck: Vec::new(),
            refused: 0,
        }
    }
}

impl<B: Backend> Reader<B> {
    /// One read, waiting at most `limit` for its helper. `Refused` when the
    /// handle is still out, the read is late, or it failed hard.
    fn read(&mut self, open: &mut impl FnMut() -> B, limit: Duration) -> ClipboardText {
        self.collect_late();
        // While the handle is out, refuse at once, until enough reads have
        // that this one gives the stuck read up: it keeps its thread and
        // handle until it ends, and this read goes on with a new handle.
        let out = self.ready.is_none() && !self.stuck.is_empty();
        if out && (self.refused < READS_BEFORE_RETRY || self.stuck.len() >= MAX_STUCK_READS) {
            self.refused += 1;
            return ClipboardText::Refused;
        }
        self.refused = 0;
        let mut backend = self.ready.take().unwrap_or_else(open);
        let (sender, answer) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("loom-ui clipboard read".to_owned())
            .spawn(move || {
                let text = backend.get();
                let _ = sender.send((text, backend));
            });
        if spawned.is_err() {
            return ClipboardText::Refused;
        }
        match answer.recv_timeout(limit) {
            Ok((text, backend)) => {
                self.ready = Some(backend);
                text
            }
            Err(RecvTimeoutError::Timeout) => {
                self.stuck.push(answer);
                ClipboardText::Refused
            }
            // It failed hard: the next read opens another handle.
            Err(RecvTimeoutError::Disconnected) => ClipboardText::Refused,
        }
    }

    /// Takes in the stuck reads that ended. Their answers are stale; the
    /// first good handle is kept to read through, and the rest go.
    fn collect_late(&mut self) {
        let ready = &mut self.ready;
        self.stuck.retain(|late| match late.try_recv() {
            Ok((_, backend)) => {
                if ready.is_none() {
                    *ready = Some(backend);
                }
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
    }
}

/// What `arboard`'s read gave, as a [`ClipboardText`]: no text on the
/// clipboard is empty, and every other failure, such as a clipboard another
/// app holds on Windows, is refused, never empty.
fn answer(read: Result<String, arboard::Error>) -> ClipboardText {
    match read {
        Ok(text) => ClipboardText::from_text(Some(text)),
        Err(arboard::Error::ContentNotAvailable) => ClipboardText::Empty,
        Err(_) => ClipboardText::Refused,
    }
}

#[cfg(test)]
mod tests;
