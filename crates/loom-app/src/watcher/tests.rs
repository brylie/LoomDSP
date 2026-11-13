//! Tests for watcher threads: the handle stops them, and the outlet holds
//! only as many messages as the app has room for.

use super::*;
use crate::watcher::Ledger;
use std::sync::mpsc;

/// A waker into a plain channel, for a test to read what a watcher sent.
fn waker() -> (Waker<Message>, mpsc::Receiver<Message>) {
    let (sender, receiver) = mpsc::channel();
    (Waker::new(sender, Arc::new(|| {})), receiver)
}

/// Dropping the handle returns while the thread is still busy, and the
/// thread ends at its next check: the drop signals it and never waits.
#[test]
fn a_watcher_stops_when_its_handle_drops() {
    let (waker, _queue) = waker();
    let (release, busy) = mpsc::channel::<()>();
    let (done, finished) = mpsc::channel();
    let watcher = Watcher::start(
        &Ledger::default(),
        1,
        1,
        waker,
        |()| Message::Tick,
        move |outlet| {
            // Busy until the test lets go, as a thread inside a slow call is.
            let _ = busy.recv();
            while !outlet.is_closed() {
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = done.send(());
        },
    );
    drop(watcher);
    assert!(
        finished.try_recv().is_err(),
        "the drop returned while the thread was busy"
    );
    release.send(()).expect("the thread still waits");
    finished
        .recv_timeout(Duration::from_secs(10))
        .expect("the thread ended at its next check");
}

/// A ledger frees only the watcher that sent the message taken, in the
/// order sent, and does nothing for a message no watcher sent.
#[test]
fn a_ledger_frees_only_the_sender() {
    let (waker, queue) = waker();
    let ledger = Ledger::default();
    let first = Watcher::start(
        &ledger,
        1,
        1,
        waker.clone(),
        |()| Message::Tick,
        |outlet| {
            let _ = outlet.try_send(());
        },
    );
    let _ = queue.recv_timeout(Duration::from_secs(2)).expect("the first sent");
    let second = Watcher::start(
        &ledger,
        2,
        1,
        waker,
        |()| Message::Tick,
        |outlet| {
            let _ = outlet.try_send(());
        },
    );
    let _ = queue.recv_timeout(Duration::from_secs(2)).expect("the second sent");
    assert_eq!((first.in_flight(), second.in_flight()), (1, 1));
    ledger.taken();
    assert_eq!(
        (first.in_flight(), second.in_flight()),
        (0, 1),
        "the oldest is the first's"
    );
    ledger.taken();
    ledger.taken();
    assert_eq!((first.in_flight(), second.in_flight()), (0, 0), "never below zero");
}

/// The outlet holds as many messages as its capacity: past that a send is
/// refused as full, and the thread sends fresh values once the app has
/// taken one. So a slow app sees one message per value it takes.
#[test]
fn an_outlet_refuses_past_its_capacity_until_one_is_taken() {
    let (waker, queue) = waker();
    let (sent, results) = mpsc::channel();
    let (go, wait) = mpsc::channel::<()>();
    let ledger = Ledger::default();
    let _watcher = Watcher::start(
        &ledger,
        1,
        1,
        waker,
        |()| Message::Tick,
        move |outlet| {
            let _ = sent.send(outlet.try_send(()));
            let _ = sent.send(outlet.try_send(()));
            let _ = wait.recv();
            let _ = sent.send(outlet.try_send(()));
        },
    );
    assert_eq!(results.recv().unwrap(), Ok(()));
    let full = results.recv().unwrap();
    assert!(full.as_ref().is_err_and(SendError::is_full), "{full:?}");
    assert!(matches!(queue.try_recv(), Ok(Message::Tick)), "the first arrived");
    ledger.taken();
    go.send(()).unwrap();
    assert_eq!(results.recv().unwrap(), Ok(()), "room again once taken");
}

/// A blocking send waits for room and keeps its value, and ends with false
/// once the handle drops.
#[test]
fn a_blocking_send_waits_for_room_and_ends_when_closed() {
    let (waker, queue) = waker();
    let (sent, results) = mpsc::channel();
    let ledger = Ledger::default();
    let watcher = Watcher::start(
        &ledger,
        1,
        1,
        waker,
        |n: u32| Message::Lights(vec![((n, 0), 0.0)]),
        move |outlet| {
            let _ = sent.send(outlet.send(1));
            let _ = sent.send(outlet.send(2));
            let _ = sent.send(outlet.send(3));
        },
    );
    assert!(results.recv().unwrap());
    assert!(
        results.recv_timeout(Duration::from_millis(100)).is_err(),
        "the second waits for room"
    );
    ledger.taken();
    assert!(results.recv().unwrap(), "and goes once there is room");
    let lights: Vec<u32> = queue
        .try_iter()
        .filter_map(|message| match message {
            Message::Lights(lights) => Some(lights[0].0.0),
            _ => None,
        })
        .collect();
    assert_eq!(lights, [1, 2], "each value arrives, in order");
    drop(watcher);
    assert!(!results.recv().unwrap(), "a closed outlet sends nothing");
}

/// A runtime that is gone closes the outlet, so the thread can end.
#[test]
fn a_gone_runtime_closes_the_outlet() {
    let (waker, queue) = waker();
    drop(queue);
    let (sent, results) = mpsc::channel();
    let _watcher = Watcher::start(
        &Ledger::default(),
        1,
        1,
        waker,
        |()| Message::Tick,
        move |outlet| {
            let _ = sent.send((outlet.try_send(()), outlet.is_closed()));
        },
    );
    assert_eq!(results.recv().unwrap(), (Err(SendError::Closed), true));
}

/// `keep` starts a watcher for a new key, keeps one whose key is the same,
/// and stops it when nothing is wanted.
#[test]
fn keep_restarts_only_on_a_new_key() {
    let (waker, _queue) = waker();
    let starts = std::cell::Cell::new(0);
    let start = |key: u64, _: &str| {
        starts.set(starts.get() + 1);
        Watcher::start(&Ledger::default(), key, 1, waker.clone(), |()| Message::Tick, |_| {})
    };
    let mut slot = None;
    keep(&mut slot, Some("a"), start);
    keep(&mut slot, Some("a"), start);
    assert_eq!(starts.get(), 1, "the same key keeps it");
    keep(&mut slot, Some("b"), start);
    assert_eq!(starts.get(), 2, "a new key restarts it");
    assert_eq!(slot.as_ref().map(Watcher::key), Some(key_of(&"b")));
    keep(&mut slot, None::<&str>, start);
    assert!(slot.is_none(), "nothing wanted, nothing runs");
}
