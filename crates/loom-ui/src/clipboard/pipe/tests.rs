//! Tests for the deadline read, over the system's own pipes, so they run in
//! CI with no compositor. A held writer that never writes stands in for an
//! app that copied and then stopped.

use super::{Unread, read_with, read_within};
use crate::clipboard::MAX_TEXT;
use std::io::{ErrorKind, Write, pipe};
use std::time::{Duration, Instant};

/// The deadline the tests read with: short, so the suite stays quick.
const LIMIT: Duration = Duration::from_millis(200);

/// How much later than the deadline a read may end and still pass, so a
/// busy CI machine does not fail the test.
const SLACK: Duration = Duration::from_secs(2);

/// Asserts that a read took about its limit: no less, and not much more.
fn ended_at_the_limit(started: Instant) {
    let took = started.elapsed();
    assert!(took >= LIMIT, "it waited its limit: {took:?}");
    assert!(took < LIMIT + SLACK, "and not much more: {took:?}");
}

/// A writer that is held and never writes: the read ends at the deadline,
/// late, as an app that copied and then stopped would leave it.
#[test]
fn a_silent_writer_ends_at_the_deadline() {
    let (mut reader, _writer) = pipe().unwrap();
    let started = Instant::now();
    assert_eq!(read_within(&mut reader, LIMIT, MAX_TEXT), Err(Unread::Late));
    ended_at_the_limit(started);
}

/// Bytes, then a closed writer: the read gives every byte.
#[test]
fn bytes_then_a_close_give_the_bytes() {
    let (mut reader, mut writer) = pipe().unwrap();
    writer.write_all(b"loom.osc.sine").unwrap();
    drop(writer);
    assert_eq!(read_within(&mut reader, LIMIT, MAX_TEXT), Ok(b"loom.osc.sine".to_vec()));
}

/// More bytes than one chunk, written from another thread while the read
/// runs, all come through.
#[test]
fn a_long_text_comes_through_whole() {
    let (mut reader, mut writer) = pipe().unwrap();
    let text: Vec<u8> = (0..300_000).map(|i| b'a' + (i % 26) as u8).collect();
    let sent = text.clone();
    let writing = std::thread::spawn(move || writer.write_all(&sent).unwrap());
    assert_eq!(read_within(&mut reader, SLACK, MAX_TEXT), Ok(text));
    writing.join().unwrap();
}

/// Part of the text, then silence: the read is late, and the part is not
/// given as if it were the whole.
#[test]
fn a_partial_write_then_silence_is_late() {
    let (mut reader, mut writer) = pipe().unwrap();
    writer.write_all(b"loom.").unwrap();
    assert_eq!(read_within(&mut reader, LIMIT, MAX_TEXT), Err(Unread::Late));
}

/// A writer that closes at once: the read gives nothing, which the
/// clipboard answers as empty.
#[test]
fn a_writer_that_closes_at_once_gives_nothing() {
    let (mut reader, writer) = pipe().unwrap();
    drop(writer);
    assert_eq!(read_within(&mut reader, LIMIT, MAX_TEXT), Ok(Vec::new()));
}

/// After a late read, dropping the pipe closes our end: the writer's next
/// write fails, so nothing is left waiting on either side.
#[test]
fn after_a_late_read_the_writer_finds_the_pipe_closed() {
    let (mut reader, mut writer) = pipe().unwrap();
    assert_eq!(read_within(&mut reader, LIMIT, MAX_TEXT), Err(Unread::Late));
    drop(reader);
    let error = writer.write_all(b"late").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::BrokenPipe);
}

/// A writer that sends a byte every few milliseconds, for longer than the
/// deadline: the read still ends at the deadline. A timeout on each wait
/// would never end it.
#[test]
fn a_trickling_writer_ends_at_the_deadline() {
    let (mut reader, mut writer) = pipe().unwrap();
    let trickling = std::thread::spawn(move || {
        while writer.write_all(b".").is_ok() {
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let started = Instant::now();
    assert_eq!(read_within(&mut reader, LIMIT, MAX_TEXT), Err(Unread::Late));
    ended_at_the_limit(started);
    drop(reader);
    trickling.join().unwrap();
}

/// A timer that cannot be made fails the read at once: it never goes on
/// with no deadline.
#[test]
fn a_timer_that_cannot_be_made_fails_the_read() {
    let (mut reader, _writer) = pipe().unwrap();
    let failed = read_with(&mut reader, MAX_TEXT, || {
        Err::<std::io::PipeReader, _>(ErrorKind::Other.into())
    });
    assert_eq!(failed, Err(Unread::Failed));
}

/// A timer the system reports broken fails the read at once. Here the
/// "timer" is the writing end of a pipe whose reading end is gone, which
/// `poll` reports as an error.
#[test]
fn a_broken_timer_fails_the_read() {
    let (mut reader, _writer) = pipe().unwrap();
    let (gone, broken) = pipe().unwrap();
    drop(gone);
    assert_eq!(read_with(&mut reader, MAX_TEXT, || Ok(broken)), Err(Unread::Failed));
}

/// A writer that sends more than the cap, within the deadline: the read
/// stops there, too long, and closes our end, so the writer's next write
/// fails.
#[test]
fn a_text_past_the_cap_is_too_long() {
    let (mut reader, mut writer) = pipe().unwrap();
    let flooding = std::thread::spawn(move || {
        let block = [b'a'; 4096];
        while writer.write_all(&block).is_ok() {}
    });
    assert_eq!(read_within(&mut reader, SLACK, 100_000), Err(Unread::TooLong));
    drop(reader);
    flooding.join().unwrap();
}

/// A text of exactly the cap is read whole: the cap is the most a read
/// takes, not one byte less.
#[test]
fn a_text_of_exactly_the_cap_comes_through() {
    let (mut reader, mut writer) = pipe().unwrap();
    let writing = std::thread::spawn(move || writer.write_all(&[b'a'; 100_000]).unwrap());
    assert_eq!(read_within(&mut reader, SLACK, 100_000), Ok(vec![b'a'; 100_000]));
    writing.join().unwrap();
}
