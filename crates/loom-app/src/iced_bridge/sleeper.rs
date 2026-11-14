//! One thread per window that wakes the loop at the runtime's next
//! deadline (ui-layer.md, section 2, rule 8).
//!
//! Iced wakes for an event, a subscription's message, or a frame, never at
//! a time we choose. So the bridge keeps one long-lived thread for that.
//! It waits on a condition variable for the deadline the last turn gave;
//! once that time comes it calls the runtime's wake function, and the next
//! turn finds the deadline due. Setting a new deadline only changes the
//! time it waits for: the thread stays, so a deadline that moves every
//! turn (a frame at display rate, a timer) starts no thread.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Instant;

/// What the thread and the bridge share.
#[derive(Debug, Default)]
struct State {
    /// The time to wake at, or `None` to wait for one.
    at: Option<Instant>,
    /// Set when the bridge drops the sleeper.
    closed: bool,
}

/// The bridge's end of the sleeper. Dropping it ends the thread.
#[derive(Debug)]
pub(crate) struct Sleeper {
    shared: Arc<(Mutex<State>, Condvar)>,
}

impl Sleeper {
    /// Starts the thread. It calls `wake` once for each deadline it
    /// reaches.
    pub(crate) fn start(wake: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let thread = shared.clone();
        std::thread::spawn(move || sleep(&thread, &wake));
        Self { shared }
    }

    /// Wakes at `at`, in place of any time set before; `None` wakes at no
    /// time. A time already past wakes at once.
    pub(crate) fn set(&self, at: Option<Instant>) {
        let (state, changed) = &*self.shared;
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.at != at {
            state.at = at;
            changed.notify_one();
        }
    }
}

impl Drop for Sleeper {
    /// Ends the thread on its next look, and returns at once.
    fn drop(&mut self) {
        let (state, changed) = &*self.shared;
        state.lock().unwrap_or_else(PoisonError::into_inner).closed = true;
        changed.notify_one();
    }
}

/// The thread: waits for a time, waits until it comes, wakes the loop, and
/// waits for the next one.
fn sleep(shared: &(Mutex<State>, Condvar), wake: &impl Fn()) {
    let (state, changed) = shared;
    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if guard.closed {
            return;
        }
        match guard.at {
            None => guard = changed.wait(guard).unwrap_or_else(PoisonError::into_inner),
            Some(at) => {
                let now = Instant::now();
                if now >= at {
                    // Once per deadline: the next turn sets the next one.
                    guard.at = None;
                    drop(guard);
                    wake();
                    guard = state.lock().unwrap_or_else(PoisonError::into_inner);
                } else {
                    // Wakes early when the time changes, and looks again.
                    guard = changed
                        .wait_timeout(guard, at - now)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
