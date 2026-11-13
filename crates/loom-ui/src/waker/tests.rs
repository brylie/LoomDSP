use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

/// A waker and the queue it fills, with a count of wake calls.
fn counted() -> (Waker<u32>, mpsc::Receiver<u32>, Arc<AtomicUsize>) {
    let (queue, receiver) = mpsc::channel();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let wake: Wake = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    (Waker::new(queue, wake), receiver, wakes)
}

/// Section 2, rule 7: a message sent from another thread is queued in
/// order, and the loop is woken once per message.
#[test]
fn a_waker_queues_in_order_from_another_thread_and_wakes_once_each() {
    let (waker, receiver, wakes) = counted();
    let sender = waker.clone();
    std::thread::spawn(move || {
        for message in 1..=3 {
            assert!(sender.send(message));
        }
    })
    .join()
    .expect("the sending thread ends");
    assert_eq!(receiver.try_iter().collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
}

/// A waker whose runtime is gone says so and does not wake anything.
#[test]
fn a_waker_whose_runtime_is_gone_returns_false() {
    let (waker, receiver, wakes) = counted();
    drop(receiver);
    assert!(!waker.send(7));
    assert_eq!(wakes.load(Ordering::SeqCst), 0);
    assert_eq!(format!("{waker:?}"), "Waker");
}

/// A batch from `send_all` is queued whole and in order, with no message
/// from another thread inside it, and wakes the loop once.
#[test]
fn a_batch_stays_together_while_another_thread_sends() {
    let (waker, receiver, wakes) = counted();
    let other = waker.clone();
    let spam = std::thread::spawn(move || {
        for _ in 0..2000 {
            other.send(0);
        }
    });
    for _ in 0..50 {
        assert!(waker.send_all([1, 2, 3]));
    }
    spam.join().expect("the other thread ends");
    let queued: Vec<u32> = receiver.try_iter().collect();
    let batches: Vec<&[u32]> = queued
        .iter()
        .enumerate()
        .filter(|&(_, &m)| m == 1)
        .map(|(at, _)| &queued[at..at + 3])
        .collect();
    assert_eq!(batches.len(), 50);
    assert!(batches.iter().all(|batch| *batch == [1, 2, 3]), "a batch was split");
    assert_eq!(queued.len(), 2000 + 150);
    assert_eq!(wakes.load(Ordering::SeqCst), 2000 + 50, "one wake per send or batch");
}
