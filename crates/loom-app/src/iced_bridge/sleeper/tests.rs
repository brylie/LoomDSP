//! Tests for the window's sleeper: one thread for every deadline, woken
//! once per deadline, and ended by dropping it.

use super::*;
use std::sync::mpsc;
use std::thread::ThreadId;
use std::time::Duration;

/// A sleeper whose wake-ups say which thread made them.
fn sleeper() -> (Sleeper, mpsc::Receiver<ThreadId>) {
    let (sender, wakes) = mpsc::channel();
    let sleeper = Sleeper::start(move || {
        let _ = sender.send(std::thread::current().id());
    });
    (sleeper, wakes)
}

/// Ten deadlines, each set as the one before came, wake ten times, all
/// from the same thread: a moving deadline starts no new thread.
#[test]
fn changing_the_deadline_ten_times_starts_no_new_thread() {
    let (sleeper, wakes) = sleeper();
    let mut threads = Vec::new();
    for _ in 0..10 {
        sleeper.set(Some(Instant::now() + Duration::from_millis(1)));
        threads.push(
            wakes
                .recv_timeout(Duration::from_secs(10))
                .expect("the deadline woke it"),
        );
    }
    threads.dedup();
    assert_eq!(threads.len(), 1, "one thread woke every time: {threads:?}");
    assert_ne!(threads[0], std::thread::current().id());
}

/// A new time replaces the old one before it comes, and `None` cancels:
/// only the time set last wakes, and only once.
#[test]
fn only_the_latest_time_wakes_and_only_once() {
    let (sleeper, wakes) = sleeper();
    sleeper.set(Some(Instant::now() + Duration::from_secs(3600)));
    sleeper.set(Some(Instant::now()));
    wakes
        .recv_timeout(Duration::from_secs(10))
        .expect("the nearer time replaced the far one");
    assert!(
        wakes.recv_timeout(Duration::from_millis(50)).is_err(),
        "once per deadline"
    );
    sleeper.set(Some(Instant::now() + Duration::from_millis(20)));
    sleeper.set(None);
    assert!(
        wakes.recv_timeout(Duration::from_millis(100)).is_err(),
        "a cancelled time wakes nothing"
    );
}

/// Dropping the sleeper ends its thread, which drops the wake function.
#[test]
fn dropping_it_ends_the_thread() {
    let (sleeper, wakes) = sleeper();
    drop(sleeper);
    assert_eq!(
        wakes.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected),
        "the thread ended and dropped its wake"
    );
}
