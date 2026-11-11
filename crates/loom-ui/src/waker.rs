//! How another thread hands the loop a message and wakes it.
//!
//! A [`Waker`] puts a message in the runtime's queue and then calls the
//! driver's wake function, so a loop that sleeps until something happens
//! wakes for it (docs/design/ui-layer.md, section 2, rule 7). In a window
//! shell the wake function nudges the event loop; in the headless driver
//! it does nothing, because the driver drains the queue itself after each
//! step.
//!
//! A watcher thread keeps a clone for as long as it runs. Its owner keeps
//! the thread's handle in app state, and stopping the watcher is dropping
//! that handle: there is no list of subscriptions to restart by key.

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::Sender;

/// The driver's wake function: called after each message is queued.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Hands the loop a message from any thread, and wakes it.
pub struct Waker<M> {
    queue: Sender<M>,
    wake: Wake,
}

impl<M> Waker<M> {
    /// A waker that puts messages on `queue` and calls `wake` after each.
    pub fn new(queue: Sender<M>, wake: Wake) -> Self {
        Self { queue, wake }
    }

    /// Queues `message` for the next turn and wakes the loop. Returns false
    /// when the runtime is gone, so a watcher can stop.
    pub fn send(&self, message: M) -> bool {
        if self.queue.send(message).is_err() {
            return false;
        }
        (self.wake)();
        true
    }
}

impl<M> Clone for Waker<M> {
    fn clone(&self) -> Self {
        Self {
            queue: self.queue.clone(),
            wake: Arc::clone(&self.wake),
        }
    }
}

impl<M> fmt::Debug for Waker<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Waker")
    }
}

#[cfg(test)]
mod tests;
