//! Tests for which watchers the app runs.

use crate::Message;
use crate::tests::app;
use crate::watcher::Watcher;
use loom_ui::Waker;
use std::sync::{Arc, mpsc};

/// No watcher runs before the window opens, so a test's app never has a
/// thread sending it messages it did not ask for. Once started, the
/// engine's watchers run, and a new engine starts with none until the
/// next update starts its own.
#[test]
fn watchers_start_with_the_window_and_follow_the_engine() {
    let (sender, _queue) = mpsc::channel();
    let waker = Waker::new(sender, Arc::new(|| {}));
    let mut app = app();
    app.sync_watchers(&waker);
    assert!(
        app.session.engine.watchers.none_running(),
        "not before the window opens"
    );
    let _ = app.send(Message::StartWatching);
    app.sync_watchers(&waker);
    assert!(
        !app.session.engine.watchers.none_running(),
        "the telemetry monitor runs once watching"
    );
    assert!(app.replace(crate::Source::Empty));
    assert!(
        app.session.engine.watchers.none_running(),
        "the old engine's went with it"
    );
    app.sync_watchers(&waker);
    assert!(!app.session.engine.watchers.none_running(), "the new engine's start");
}

/// A message a watcher sent is taken first in `apply`, before anything
/// could return early (every watcher's message also runs while a question
/// is open), so the watcher gets room for its next one.
#[test]
fn applying_a_watched_message_makes_room_for_the_next() {
    let (sender, queue) = mpsc::channel();
    let waker = Waker::new(sender, Arc::new(|| {}));
    let mut app = app();
    // A watcher that sends two readings, the second only once the first
    // has been taken.
    let (sent, results) = mpsc::channel();
    app.session.engine.watchers.lights = Some(Watcher::start(
        &app.watchers.ledgers.lights,
        1,
        1,
        waker,
        Message::Lights,
        move |outlet| {
            let _ = sent.send(outlet.try_send(Vec::new()));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                if outlet.try_send(Vec::new()).is_ok() {
                    let _ = sent.send(Ok(()));
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let _ = sent.send(Err(crate::watcher::SendError::Full));
        },
    ));
    assert_eq!(results.recv().unwrap(), Ok(()));
    let first = queue.recv().unwrap();
    assert!(
        results.recv_timeout(std::time::Duration::from_millis(100)).is_err(),
        "the second waits while the first is in flight"
    );
    let _ = app.send(first);
    assert_eq!(results.recv().unwrap(), Ok(()), "taken, so the next one went");
}

/// A message the old engine's watcher sent, taken after the engine was
/// replaced, frees nothing on the new engine's watcher: its count and its
/// room stay as they were.
#[test]
fn a_stale_message_leaves_the_new_watcher_alone() {
    let (sender, queue) = mpsc::channel();
    let waker = Waker::new(sender, Arc::new(|| {}));
    let mut app = app();
    let lights = |sent: Vec<((u32, usize), f32)>| {
        move |outlet: crate::watcher::Outlet<Vec<((u32, usize), f32)>>| {
            let _ = outlet.try_send(sent);
        }
    };
    app.session.engine.watchers.lights = Some(Watcher::start(
        &app.watchers.ledgers.lights,
        1,
        1,
        waker.clone(),
        Message::Lights,
        lights(vec![((1, 0), 0.5)]),
    ));
    let stale = queue
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the old watcher sent");
    // New, as a player opens another patch: the old engine's watcher goes.
    assert!(app.replace(crate::Source::Empty));
    let new = Watcher::start(
        &app.watchers.ledgers.lights,
        2,
        1,
        waker,
        Message::Lights,
        lights(vec![((2, 0), 0.5)]),
    );
    let fresh = queue
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the new watcher sent");
    assert_eq!(new.in_flight(), 1);
    app.watched_message_taken(&stale);
    assert_eq!(
        new.in_flight(),
        1,
        "the stale message freed the old sender, not the new one"
    );
    app.watched_message_taken(&fresh);
    assert_eq!(new.in_flight(), 0, "its own message frees it");
}
