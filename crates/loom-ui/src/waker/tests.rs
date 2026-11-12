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
