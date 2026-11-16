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

/// How long a test waits on a value, as a thread waits for its interval.
/// Long enough that a wait that ends early shows it was woken.
const LONG: Duration = Duration::from_secs(10);

/// How soon a woken wait must end. A wake takes far less, so this holds on
/// a busy machine too.
const PROMPT: Duration = Duration::from_millis(100);

/// A changed `set` moves the version, and an equal one leaves it as it was,
/// so a test can tell from the version alone, with no timing.
#[test]
fn only_a_changed_set_moves_the_version() {
    let value = Watched::<u32>::default();
    assert_eq!(value.get(), (0, 0));
    value.set(0);
    assert_eq!(value.get(), (0, 0), "an equal set changes nothing");
    value.set(3);
    assert_eq!(value.get(), (3, 1));
    value.set(3);
    assert_eq!(value.get(), (3, 1));
}

/// A watcher on a watched value, whose thread runs `run` and reports
/// through the channel it returns.
fn watching<R: Send + 'static>(
    value: &Watched<u32>,
    run: impl FnOnce(Outlet<()>, Watched<u32>, mpsc::Sender<R>) + Send + 'static,
) -> (Watcher, mpsc::Receiver<R>) {
    let (waker, _queue) = waker();
    let (report, reports) = mpsc::channel();
    let shared = value.clone();
    let watcher = Watcher::start_on(
        &Ledger::default(),
        1,
        1,
        waker,
        |()| Message::Tick,
        value,
        move |outlet| run(outlet, shared, report),
    );
    (watcher, reports)
}

/// A new watcher has seen no version, so its first wait returns at once,
/// as the light thread reads the table straight away.
#[test]
fn a_first_wait_returns_at_once() {
    let value = Watched::<u32>::default();
    let (_watcher, reports) = watching(&value, |outlet, value, report| {
        let start = std::time::Instant::now();
        outlet.wait(&value, None, LONG);
        let _ = report.send(start.elapsed());
    });
    let waited = reports.recv_timeout(LONG).expect("the wait ended");
    assert!(waited < PROMPT, "waited {waited:?}");
}

/// A `set` wakes a thread waiting on the value at once, long before its
/// timeout.
#[test]
fn a_set_wakes_a_waiting_thread_at_once() {
    let value = Watched::<u32>::default();
    let (ready, waiting) = mpsc::channel();
    let (_watcher, reports) = watching(&value, move |outlet, value, report| {
        let (_, seen) = value.get();
        let _ = ready.send(());
        outlet.wait(&value, Some(seen), LONG);
        let _ = report.send((std::time::Instant::now(), value.get().0));
    });
    waiting.recv_timeout(LONG).expect("the thread is about to wait");
    let set_at = std::time::Instant::now();
    value.set(1);
    let (woke_at, read) = reports.recv_timeout(LONG).expect("the wait ended");
    assert_eq!(read, 1, "it reads the new value");
    assert!(
        woke_at.duration_since(set_at) < PROMPT,
        "woke after {:?}",
        woke_at - set_at
    );
}

/// A `set` made while the thread was busy reading is not lost: the version
/// moved, so its next wait returns at once.
#[test]
fn a_set_made_while_reading_makes_the_next_wait_return_at_once() {
    let value = Watched::<u32>::default();
    let (reading, busy) = mpsc::channel();
    let (release, read_done) = mpsc::channel::<()>();
    let (_watcher, reports) = watching(&value, move |outlet, value, report| {
        let (_, seen) = value.get();
        // Busy, as a thread reading the display table is.
        let _ = reading.send(());
        let _ = read_done.recv();
        let start = std::time::Instant::now();
        outlet.wait(&value, Some(seen), LONG);
        let _ = report.send(start.elapsed());
    });
    busy.recv_timeout(LONG).expect("the thread is reading");
    value.set(1);
    release.send(()).unwrap();
    let waited = reports.recv_timeout(LONG).expect("the wait ended");
    assert!(waited < PROMPT, "waited {waited:?}");
}

/// Dropping the handle wakes a thread waiting on its value, so it ends at
/// once instead of after its timeout. The closed flag is the handle's, so
/// another watcher on the same value goes on waiting.
#[test]
fn a_dropped_handle_ends_the_wait_at_once() {
    let value = Watched::<u32>::default();
    let wait = |outlet: Outlet<()>, value: Watched<u32>, report: mpsc::Sender<_>, ready: mpsc::Sender<()>| {
        let (_, seen) = value.get();
        let _ = ready.send(());
        outlet.wait(&value, Some(seen), LONG);
        let _ = report.send((std::time::Instant::now(), outlet.is_closed()));
    };
    let (ready, waiting) = mpsc::channel();
    let other_ready = ready.clone();
    let (watcher, reports) = watching(&value, move |outlet, value, report| wait(outlet, value, report, ready));
    let (_other, others) = watching(&value, move |outlet, value, report| {
        wait(outlet, value, report, other_ready);
    });
    waiting.recv_timeout(LONG).expect("one is about to wait");
    waiting.recv_timeout(LONG).expect("both are about to wait");
    let dropped_at = std::time::Instant::now();
    drop(watcher);
    let (woke_at, closed) = reports.recv_timeout(LONG).expect("the wait ended");
    assert!(closed, "it was woken by its own drop");
    assert!(
        woke_at.duration_since(dropped_at) < PROMPT,
        "woke after {:?}",
        woke_at - dropped_at
    );
    assert!(others.recv_timeout(PROMPT).is_err(), "the other watcher still waits");
}

/// A handle's debug text says whether its drop wakes a watched value, so a
/// dump of the app's watchers shows which threads end at once on a drop and
/// which wait out their interval.
#[test]
fn a_handle_says_whether_it_waits_on_a_value() {
    let value = Watched::<u32>::default();
    let (on_a_value, _reports) = watching(&value, |_, _, _: mpsc::Sender<()>| {});
    let (waker, _queue) = waker();
    let plain = Watcher::start(&Ledger::default(), 2, 1, waker, |()| Message::Tick, |_| {});
    let text = format!("{on_a_value:?}");
    assert!(
        text.contains("waits_on_a_value: true") && text.contains("key: 1"),
        "{text}"
    );
    assert!(format!("{plain:?}").contains("waits_on_a_value: false"));
}
