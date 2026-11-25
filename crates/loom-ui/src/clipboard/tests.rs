//! Tests for the clipboard thread, over a fake backend, so they run in CI
//! with no display.

use super::{
    BACKEND_READ_LIMIT, Backend, Clipboard, Desktop, MAX_STUCK_READS, Native, READ_NOW_LIMIT, READS_BEFORE_RETRY,
    Reader, System, WAYLAND_READ_LIMIT, answer, text_of,
};
use crate::effect::ClipboardText;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A clipboard in memory, shared with the test, that can refuse. Every
/// clone is a handle onto the same text, as two handles onto the system's
/// clipboard are.
#[derive(Clone, Default)]
struct Fake {
    text: Arc<Mutex<Option<String>>>,
    refuse: Arc<Mutex<bool>>,
}

impl Backend for Fake {
    fn get(&mut self) -> ClipboardText {
        if *self.refuse.lock().unwrap() {
            return ClipboardText::Refused;
        }
        ClipboardText::from_text(self.text.lock().unwrap().clone())
    }

    fn set(&mut self, text: String) {
        *self.text.lock().unwrap() = Some(text);
    }
}

/// How long a test waits for the thread before it fails.
const WAIT: Duration = Duration::from_secs(5);

/// A read, answered on the clipboard's thread.
fn read(clipboard: &Clipboard) -> ClipboardText {
    let (sender, answer) = mpsc::channel();
    clipboard.read(move |text| {
        let _ = sender.send(text);
    });
    answer.recv_timeout(WAIT).expect("the thread answers")
}

/// A write and then a read run in order on the one thread: the read finds
/// what was written. An empty clipboard reads `Empty`.
#[test]
fn a_write_then_a_read_run_in_order() {
    let fake = Fake::default();
    let clipboard = Clipboard::new(move || fake.clone());
    assert_eq!(read(&clipboard), ClipboardText::Empty);
    clipboard.write("osc".to_owned());
    assert_eq!(read(&clipboard), ClipboardText::Text("osc".to_owned()));
}

/// A read the backend cannot complete answers `Refused`, never empty.
#[test]
fn a_refused_read_is_not_empty() {
    let fake = Fake::default();
    *fake.refuse.lock().unwrap() = true;
    let clipboard = Clipboard::new(move || fake.clone());
    clipboard.write("osc".to_owned());
    assert_eq!(read(&clipboard), ClipboardText::Refused);
}

/// Clones share one thread and one clipboard, and a synchronous read, for
/// a toolkit bridge, waits for the same thread.
#[test]
fn clones_share_the_clipboard() {
    let fake = Fake::default();
    let clipboard = Clipboard::new(move || fake.clone());
    let other = clipboard.clone();
    other.write("filter".to_owned());
    assert_eq!(clipboard.read_now(), ClipboardText::Text("filter".to_owned()));
}

/// The text a read finds: `from_text` makes an empty or missing text
/// `Empty`, and `text` gives it back only for `Text`.
#[test]
fn clipboard_text_from_and_to_text() {
    assert_eq!(ClipboardText::from_text(None), ClipboardText::Empty);
    assert_eq!(ClipboardText::from_text(Some(String::new())), ClipboardText::Empty);
    assert_eq!(ClipboardText::from_text(Some("a".into())).text(), Some("a"));
    assert_eq!(ClipboardText::Refused.text(), None);
}

/// The real system clipboard: a write then a read. It needs a display on
/// Linux, so it is ignored in CI and run by hand.
#[test]
#[ignore = "uses the real system clipboard; run by hand"]
fn the_system_clipboard_holds_text() {
    let clipboard = Clipboard::system();
    clipboard.write("loom clipboard test".to_owned());
    assert_eq!(
        clipboard.read_now(),
        ClipboardText::Text("loom clipboard test".to_owned())
    );
}

/// A backend whose first read blocks until the test lets it go, as a
/// clipboard another app holds would.
struct Stuck {
    release: mpsc::Receiver<()>,
    first: bool,
}

impl Backend for Stuck {
    fn get(&mut self) -> ClipboardText {
        if std::mem::take(&mut self.first) {
            let _ = self.release.recv();
        }
        ClipboardText::Text("late".to_owned())
    }

    fn set(&mut self, _text: String) {}
}

/// A synchronous read never freezes the window: with the clipboard stuck it
/// answers `Refused` within its limit, and once the clipboard answers again
/// the next read works. The late answer goes nowhere.
#[test]
fn a_stuck_clipboard_does_not_freeze_a_synchronous_read() {
    let (release, released) = mpsc::channel();
    let mut stuck = Some(Stuck {
        release: released,
        first: true,
    });
    let clipboard = Clipboard::new(move || {
        stuck.take().unwrap_or_else(|| Stuck {
            release: mpsc::channel().1,
            first: false,
        })
    });
    let started = std::time::Instant::now();
    assert_eq!(clipboard.read_now(), ClipboardText::Refused);
    let waited = started.elapsed();
    assert!(waited >= READ_NOW_LIMIT, "it waited its limit: {waited:?}");
    assert!(waited < READ_NOW_LIMIT * 8, "and not much more: {waited:?}");
    release.send(()).unwrap();
    assert_eq!(read(&clipboard), ClipboardText::Text("late".to_owned()));
}

/// What `arboard` gives becomes a `ClipboardText`: text is text, no text is
/// empty, and any other failure, such as a clipboard another app holds, is
/// refused, so the app says it could not read.
#[test]
fn arboards_answers_map_to_clipboard_text() {
    assert_eq!(answer(Ok("osc".into())), ClipboardText::Text("osc".into()));
    assert_eq!(answer(Ok(String::new())), ClipboardText::Empty);
    assert_eq!(answer(Err(arboard::Error::ContentNotAvailable)), ClipboardText::Empty);
    assert_eq!(answer(Err(arboard::Error::ClipboardOccupied)), ClipboardText::Refused);
    assert_eq!(
        answer(Err(arboard::Error::ClipboardNotSupported)),
        ClipboardText::Refused
    );
}

/// A backend that fails hard on its first read, as a panic in the system's
/// clipboard code would, then works.
struct Broken {
    first: bool,
}

impl Backend for Broken {
    fn get(&mut self) -> ClipboardText {
        assert!(!std::mem::take(&mut self.first), "the read fails hard, on purpose");
        ClipboardText::Text("again".to_owned())
    }

    fn set(&mut self, _text: String) {}
}

/// A read whose backend fails hard answers `Refused` at once, and the
/// clipboard's thread goes on with a backend opened again.
#[test]
fn a_backend_that_fails_hard_answers_refused_and_the_thread_goes_on() {
    let mut first = true;
    let clipboard = Clipboard::new(move || Broken {
        first: std::mem::take(&mut first),
    });
    assert_eq!(read(&clipboard), ClipboardText::Refused);
    assert_eq!(read(&clipboard), ClipboardText::Text("again".to_owned()));
}

/// One clipboard in memory, shared by every handle onto it, which records
/// what each handle does and can hold a read, as an owner that stops
/// answering would.
#[derive(Default)]
struct Shared {
    text: Mutex<Option<String>>,
    log: Mutex<Vec<String>>,
    holds: Mutex<std::collections::VecDeque<mpsc::Receiver<()>>>,
    opened: AtomicUsize,
}

impl Shared {
    /// What the handles did, in order.
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

/// One handle onto [`Shared`], numbered in the order it was opened.
struct Handle {
    id: usize,
    shared: Arc<Shared>,
}

impl Backend for Handle {
    fn get(&mut self) -> ClipboardText {
        self.shared.log.lock().unwrap().push(format!("read {}", self.id));
        let hold = self.shared.holds.lock().unwrap().pop_front();
        if let Some(hold) = hold {
            let _ = hold.recv();
        }
        ClipboardText::from_text(self.shared.text.lock().unwrap().clone())
    }

    fn set(&mut self, text: String) {
        self.shared.log.lock().unwrap().push(format!("write {}", self.id));
        *self.shared.text.lock().unwrap() = Some(text);
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shared.log.lock().unwrap().push(format!("drop {}", self.id));
    }
}

/// Holds the next read that reaches a backend until the returned sender
/// sends or goes.
fn hold_next(shared: &Shared) -> mpsc::Sender<()> {
    let (release, hold) = mpsc::channel();
    shared.holds.lock().unwrap().push_back(hold);
    release
}

/// A clipboard over `shared`, whose first read is held until the returned
/// sender sends or goes.
fn held(shared: &Arc<Shared>) -> (Clipboard, mpsc::Sender<()>) {
    let release = hold_next(shared);
    let opener = Arc::clone(shared);
    let clipboard = Clipboard::new(move || Handle {
        id: opener.opened.fetch_add(1, Ordering::SeqCst),
        shared: Arc::clone(&opener),
    });
    (clipboard, release)
}

/// A read until it answers text, for a held read that was just let go and
/// whose handle comes back a moment later.
fn read_until_text(clipboard: &Clipboard) -> ClipboardText {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let text = read(clipboard);
        if text != ClipboardText::Refused || std::time::Instant::now() > deadline {
            return text;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A stuck read never takes our copy: the handle that wrote is not the one
/// the read holds, so it does every write, is never dropped, and the text
/// stays. A later read may get the old reader back or open a new one, by
/// how soon the released read ends; either is right, so this test allows
/// both (reuse has its own test).
#[test]
fn a_stuck_read_keeps_the_handle_that_wrote() {
    let shared = Arc::new(Shared::default());
    let (clipboard, release) = held(&shared);
    clipboard.write("osc".to_owned());
    assert_eq!(
        read(&clipboard),
        ClipboardText::Refused,
        "within the limit, {BACKEND_READ_LIMIT:?}"
    );
    assert_eq!(
        shared.log(),
        ["write 0", "read 1"],
        "the read is held on its own handle"
    );
    clipboard.write("filter".to_owned());
    release.send(()).unwrap();
    assert_eq!(read_until_text(&clipboard), ClipboardText::Text("filter".to_owned()));
    let log = shared.log();
    let writes: Vec<_> = log.iter().filter(|entry| entry.starts_with("write")).collect();
    assert_eq!(writes, ["write 0", "write 0"], "every write through the writer");
    assert!(!log.iter().any(|entry| entry == "drop 0"), "the writer stays: {log:?}");
}

/// A late read hands its handle back: once it ends, the next read goes
/// through the same handle, and nothing new opens. Driven on the reader
/// itself, which waits for the late answer to arrive before it reads, so
/// the test does not race the helper.
#[test]
fn a_late_read_hands_its_handle_back() {
    let shared = Arc::new(Shared::default());
    let release = hold_next(&shared);
    let opener = Arc::clone(&shared);
    let mut open = move || Handle {
        id: opener.opened.fetch_add(1, Ordering::SeqCst),
        shared: Arc::clone(&opener),
    };
    let mut reader = Reader::default();
    assert_eq!(
        reader.read(&mut open, Duration::from_millis(1)),
        ClipboardText::Refused,
        "held, so late"
    );
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    while reader.ready.is_none() {
        assert!(std::time::Instant::now() < deadline, "the late read never ended");
        reader.collect_late();
        std::thread::yield_now();
    }
    *shared.text.lock().unwrap() = Some("osc".to_owned());
    assert_eq!(reader.read(&mut open, WAIT), ClipboardText::Text("osc".to_owned()));
    assert_eq!(shared.opened.load(Ordering::SeqCst), 1, "the same handle, used again");
    assert!(reader.stuck.is_empty());
}

/// While a read is stuck, the next reads answer `Refused` at once, with no
/// new helper or handle, and a write still goes through.
#[test]
fn reads_against_a_stuck_clipboard_hold_one_helper() {
    let shared = Arc::new(Shared::default());
    let (clipboard, _release) = held(&shared);
    assert_eq!(read(&clipboard), ClipboardText::Refused);
    let started = std::time::Instant::now();
    assert_eq!(read(&clipboard), ClipboardText::Refused);
    assert_eq!(read(&clipboard), ClipboardText::Refused);
    let waited = started.elapsed();
    assert!(waited < BACKEND_READ_LIMIT, "at once, with no wait: {waited:?}");
    clipboard.write("later".to_owned());
    assert_eq!(clipboard.read_now(), ClipboardText::Refused, "still held");
    assert_eq!(shared.log(), ["read 0", "write 1"], "one read reached a backend");
    assert_eq!(shared.opened.load(Ordering::SeqCst), 2, "a reader and a writer");
    assert_eq!(*shared.text.lock().unwrap(), Some("later".to_owned()));
}

/// The reads that reached a backend, in order.
fn reads(shared: &Shared) -> usize {
    shared.log().iter().filter(|entry| entry.starts_with("read")).count()
}

/// A read stuck for good is given up: after the reads that answer
/// `Refused` at once, the next one opens a new handle and reads what is on
/// the clipboard now.
#[test]
fn a_read_stuck_for_good_is_given_up_and_a_later_read_works() {
    let shared = Arc::new(Shared::default());
    let (clipboard, _never) = held(&shared);
    assert_eq!(read(&clipboard), ClipboardText::Refused, "stuck");
    for _ in 0..READS_BEFORE_RETRY {
        assert_eq!(read(&clipboard), ClipboardText::Refused, "at once, while it is out");
    }
    clipboard.write("osc".to_owned());
    assert_eq!(read(&clipboard), ClipboardText::Text("osc".to_owned()));
    assert_eq!(reads(&shared), 2, "the stuck read, then the one that worked");
    assert_eq!(
        shared.opened.load(Ordering::SeqCst),
        3,
        "the stuck reader, the writer, a new reader"
    );
}

/// No more than [`MAX_STUCK_READS`] reads are stuck at once: past that,
/// every read answers `Refused` at once and reaches no backend.
#[test]
fn no_more_than_the_limit_of_reads_are_stuck_at_once() {
    let shared = Arc::new(Shared::default());
    let (clipboard, _never) = held(&shared);
    let _more: Vec<_> = (1..MAX_STUCK_READS).map(|_| hold_next(&shared)).collect();
    for _ in 0..MAX_STUCK_READS {
        assert_eq!(read(&clipboard), ClipboardText::Refused, "stuck");
        for _ in 0..READS_BEFORE_RETRY {
            assert_eq!(read(&clipboard), ClipboardText::Refused);
        }
    }
    let started = std::time::Instant::now();
    for _ in 0..5 {
        assert_eq!(read(&clipboard), ClipboardText::Refused);
    }
    let waited = started.elapsed();
    assert!(waited < BACKEND_READ_LIMIT, "at once, with no new helper: {waited:?}");
    assert_eq!(reads(&shared), MAX_STUCK_READS, "one helper each, no more");
}

/// With every slot taken, reads come back once a stuck read ends: its
/// handle is taken in and the next read finds the clipboard as it is now.
/// While both are stuck, the cap opens no new helper, so retrying here
/// does not race.
#[test]
fn reads_recover_once_a_stuck_read_ends() {
    let shared = Arc::new(Shared::default());
    let (clipboard, first) = held(&shared);
    let _second = hold_next(&shared);
    for _ in 0..MAX_STUCK_READS {
        assert_eq!(read(&clipboard), ClipboardText::Refused, "stuck");
        for _ in 0..READS_BEFORE_RETRY {
            assert_eq!(read(&clipboard), ClipboardText::Refused);
        }
    }
    assert_eq!(read(&clipboard), ClipboardText::Refused, "both slots are taken");
    clipboard.write("filter".to_owned());
    first.send(()).unwrap();
    assert_eq!(read_until_text(&clipboard), ClipboardText::Text("filter".to_owned()));
    assert_eq!(
        reads(&shared),
        MAX_STUCK_READS + 1,
        "the two stuck, then the one that worked"
    );
}

/// What the next opens of [`Scripted`] do, in order: `true` opens, `false`
/// fails. Only one test uses it, so no other test shares the script.
static OPENS: Mutex<Vec<bool>> = Mutex::new(Vec::new());

/// A native handle that opens as [`OPENS`] says, and whose writes fail
/// while `fail_writes` is set.
struct Scripted {
    text: String,
    fail_writes: bool,
}

impl Native for Scripted {
    fn open() -> Result<Self, arboard::Error> {
        let opens = OPENS.lock().unwrap().remove(0);
        if !opens {
            return Err(arboard::Error::ClipboardNotSupported);
        }
        Ok(Self {
            text: String::new(),
            fail_writes: false,
        })
    }

    fn get_text(&mut self) -> ClipboardText {
        if self.text.is_empty() {
            return answer(Err(arboard::Error::ContentNotAvailable));
        }
        answer(Ok(self.text.clone()))
    }

    fn set_text(&mut self, text: String) -> Result<(), arboard::Error> {
        if self.fail_writes {
            return Err(arboard::Error::ClipboardOccupied);
        }
        self.text = text;
        Ok(())
    }
}

/// The system backend opens on first use, refuses a read while it cannot
/// open, and opens again after a write failed, as a clipboard another app
/// held on Windows would.
#[test]
fn the_system_backend_opens_again_after_a_failed_write() {
    *OPENS.lock().unwrap() = vec![false, true, true];
    let mut system = System::<Scripted>::default();
    assert_eq!(system.get(), ClipboardText::Refused, "it could not open");
    assert_eq!(system.get(), ClipboardText::Empty, "opened, with nothing on it");
    system.set("osc".to_owned());
    assert_eq!(system.get(), ClipboardText::Text("osc".to_owned()));
    system.clipboard.as_mut().unwrap().fail_writes = true;
    system.set("filter".to_owned());
    assert!(system.clipboard.is_none(), "a failed write lets the handle go");
    assert_eq!(system.get(), ClipboardText::Empty, "and the next use opens another");
    assert!(OPENS.lock().unwrap().is_empty(), "three opens, as scripted");
}

/// What a pipe read gave becomes a `ClipboardText`: UTF-8 is text, no bytes
/// are empty, and bytes that are not UTF-8, or a read that could not
/// finish, are refused, never empty.
#[test]
fn pipe_reads_map_to_clipboard_text() {
    assert_eq!(text_of(Some(b"osc".to_vec())), ClipboardText::Text("osc".into()));
    assert_eq!(text_of(Some(Vec::new())), ClipboardText::Empty);
    assert_eq!(text_of(Some(vec![0xff, 0xfe])), ClipboardText::Refused);
    assert_eq!(text_of(None), ClipboardText::Refused);
}

/// A Wayland read gives up before the clipboard thread stops waiting for
/// it, so it answers by itself and is never left stuck.
#[test]
fn a_wayland_read_ends_before_the_thread_stops_waiting() {
    assert!(WAYLAND_READ_LIMIT < BACKEND_READ_LIMIT);
}

/// A backend that waits a while and then refuses, as a Wayland read that
/// reached its deadline does.
struct Bounded {
    wait: Duration,
    reads: Arc<AtomicUsize>,
}

impl Backend for Bounded {
    fn get(&mut self) -> ClipboardText {
        self.reads.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.wait);
        ClipboardText::Refused
    }

    fn set(&mut self, _: String) {}
}

/// A read that ends by itself within the thread's limit leaves nothing
/// stuck: its handle comes back, and the next read reaches the backend at
/// once instead of being refused unasked.
#[test]
fn a_read_that_ends_in_time_leaves_nothing_stuck() {
    let reads = Arc::new(AtomicUsize::new(0));
    let opens = Arc::new(AtomicUsize::new(0));
    let mut reader = Reader::default();
    let mut open = {
        let (reads, opens) = (reads.clone(), opens.clone());
        move || {
            opens.fetch_add(1, Ordering::SeqCst);
            Bounded {
                wait: Duration::from_millis(50),
                reads: reads.clone(),
            }
        }
    };
    for _ in 0..READS_BEFORE_RETRY + 2 {
        assert_eq!(reader.read(&mut open, WAIT), ClipboardText::Refused);
    }
    assert!(reader.stuck.is_empty(), "no read was left stuck");
    assert_eq!(
        reads.load(Ordering::SeqCst),
        READS_BEFORE_RETRY + 2,
        "every read reached the backend"
    );
    assert_eq!(opens.load(Ordering::SeqCst), 1, "through the one handle");
}

/// A native handle that always opens, and holds its text in memory.
#[derive(Default)]
struct Memory {
    text: String,
}

impl Native for Memory {
    fn open() -> Result<Self, arboard::Error> {
        Ok(Self::default())
    }

    fn get_text(&mut self) -> ClipboardText {
        ClipboardText::from_text(Some(self.text.clone()))
    }

    fn set_text(&mut self, text: String) -> Result<(), arboard::Error> {
        self.text = text;
        Ok(())
    }
}

/// The desktop's clipboard writes through its `arboard` handle, and reads
/// through it too when it does not read Wayland itself.
#[test]
fn the_desktop_clipboard_reads_and_writes_through_arboard() {
    let mut desktop = Desktop::<Memory>::open().unwrap();
    // On a Wayland desktop the read would be the real clipboard's.
    #[cfg(target_os = "linux")]
    {
        desktop.wayland = false;
    }
    assert_eq!(desktop.get_text(), ClipboardText::Empty);
    desktop.set_text("osc".to_owned()).unwrap();
    assert_eq!(desktop.get_text(), ClipboardText::Text("osc".to_owned()));
}
