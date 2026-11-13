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
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// The driver's wake function: called after each message is queued.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Hands the loop a message from any thread, and wakes it.
pub struct Waker<M> {
    queue: Sender<M>,
    wake: Wake,
    /// Shared by every clone, so a batch from [`Waker::send_all`] is queued
    /// whole: no other thread's message lands inside it. Held only while
    /// messages go on the queue, never by the audio thread.
    order: Arc<Mutex<()>>,
}

impl<M> Waker<M> {
    /// A waker that puts messages on `queue` and calls `wake` after each.
    pub fn new(queue: Sender<M>, wake: Wake) -> Self {
        Self {
            queue,
            wake,
            order: Arc::default(),
        }
    }

    /// Queues `message` for the next turn and wakes the loop. Returns false
    /// when the runtime is gone, so a watcher can stop.
    pub fn send(&self, message: M) -> bool {
        self.send_all([message])
    }

    /// Queues `messages` for the next turn together and in order, with no
    /// other sender's message between them, then wakes the loop once.
    /// Returns false when the runtime is gone. For a driver or a thread
    /// that hands the loop several messages that belong together from
    /// outside a turn, such as the messages one input event becomes. Inside
    /// `update`, the next-turn effect does this.
    pub fn send_all(&self, messages: impl IntoIterator<Item = M>) -> bool {
        {
            // A poisoned lock guards nothing but order, so it is used as is.
            let _order = self.order.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            for message in messages {
                if self.queue.send(message).is_err() {
                    return false;
                }
            }
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
            order: Arc::clone(&self.order),
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
