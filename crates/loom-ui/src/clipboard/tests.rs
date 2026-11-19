//! Tests for the clipboard thread, over a fake backend, so they run in CI
//! with no display.

use super::{Backend, Clipboard, READ_NOW_LIMIT, answer};
use crate::effect::ClipboardText;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A clipboard in memory, shared with the test, that can refuse.
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
    let clipboard = Clipboard::new(Fake::default());
    assert_eq!(read(&clipboard), ClipboardText::Empty);
    clipboard.write("osc".to_owned());
    assert_eq!(read(&clipboard), ClipboardText::Text("osc".to_owned()));
}

/// A read the backend cannot complete answers `Refused`, never empty.
#[test]
fn a_refused_read_is_not_empty() {
    let fake = Fake::default();
    *fake.refuse.lock().unwrap() = true;
    let clipboard = Clipboard::new(fake);
    clipboard.write("osc".to_owned());
    assert_eq!(read(&clipboard), ClipboardText::Refused);
}

/// Clones share one thread and one clipboard, and a synchronous read, for
/// a toolkit bridge, waits for the same thread.
#[test]
fn clones_share_the_clipboard() {
    let clipboard = Clipboard::new(Fake::default());
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
    let clipboard = Clipboard::new(Stuck {
        release: released,
        first: true,
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

/// A backend that fails hard, ending its thread on the first read.
struct Broken;

impl Backend for Broken {
    fn get(&mut self) -> ClipboardText {
        panic!("the clipboard's thread ends here, on purpose");
    }

    fn set(&mut self, _text: String) {}
}

/// With its thread gone, a read answers `Refused` at once, never waits,
/// and a write is lost quietly: the app goes on.
#[test]
fn a_dead_clipboard_thread_answers_refused() {
    let clipboard = Clipboard::new(Broken);
    // The first read ends the thread; its answer is never given.
    let (sender, answer) = mpsc::channel();
    clipboard.read(move |text| {
        let _ = sender.send(text);
    });
    assert!(answer.recv_timeout(WAIT).is_err(), "the thread ended before answering");
    clipboard.write("lost".to_owned());
    assert_eq!(read(&clipboard), ClipboardText::Refused);
}
