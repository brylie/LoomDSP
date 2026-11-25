//! Tests for the Wayland read's answers, over the system's own pipes and
//! each error the compositor can give, so they run in CI with no
//! compositor.

use super::{answer, arboard_uses_wayland};
use crate::effect::ClipboardText;
use std::io::{PipeReader, Write, pipe};
use std::time::Duration;
use wl_clipboard_rs::paste::Error;

/// The deadline the tests read with.
const LIMIT: Duration = Duration::from_millis(200);

/// The compositor's answer with a pipe, as `get_contents` gives it.
fn offered(pipe: PipeReader) -> Result<(PipeReader, String), Error> {
    Ok((pipe, "text/plain;charset=utf-8".to_owned()))
}

/// Text the owner writes and then closes on is the clipboard's text.
#[test]
fn text_from_the_owner_is_text() {
    let (reader, mut writer) = pipe().unwrap();
    writer.write_all(b"loom.osc.sine").unwrap();
    drop(writer);
    assert_eq!(
        answer(offered(reader), LIMIT),
        ClipboardText::Text("loom.osc.sine".to_owned())
    );
}

/// An owner that never writes is refused at the deadline, never empty.
#[test]
fn a_silent_owner_is_refused() {
    let (reader, _writer) = pipe().unwrap();
    assert_eq!(answer(offered(reader), LIMIT), ClipboardText::Refused);
}

/// An owner that writes nothing and closes gives an empty clipboard.
#[test]
fn an_owner_that_sends_nothing_is_empty() {
    let (reader, writer) = pipe().unwrap();
    drop(writer);
    assert_eq!(answer(offered(reader), LIMIT), ClipboardText::Empty);
}

/// An empty clipboard, or one with no text, is empty, as `arboard` answers
/// it; every other error, such as no seat, is refused.
#[test]
fn the_compositors_errors_map_as_arboards_do() {
    let none = |error| answer(Err::<(PipeReader, String), _>(error), LIMIT);
    assert_eq!(none(Error::ClipboardEmpty), ClipboardText::Empty);
    assert_eq!(none(Error::NoMimeType), ClipboardText::Empty);
    assert_eq!(none(Error::NoSeats), ClipboardText::Refused);
}

/// With no Wayland display, as in CI, `arboard` uses X11, and so do our
/// reads. With one, the check asks the compositor, and the test only runs
/// it.
#[test]
fn with_no_wayland_display_reads_use_arboard() {
    let uses = arboard_uses_wayland();
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        assert!(!uses);
    }
}
