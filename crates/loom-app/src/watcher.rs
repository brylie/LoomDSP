//! Watcher threads that wake the loop through the runtime's waker
//! (ui-layer.md, section 2, rule 7).
//!
//! A watcher is a thread that reads something the app cannot be told about
//! (the audio stats, the MIDI ports, a socket) and sends a message when it
//! changes. The app keeps its [`Watcher`] handle in its state: the thread
//! runs as long as the handle lives, and dropping the handle stops it. The
//! drop sets a flag and returns at once; the thread sees the flag on its
//! next check and ends, so the UI thread never waits on a thread stuck in a
//! MIDI or audio call.
//!
//! A watcher sends through an [`Outlet`], which holds a few messages at
//! most. A message is in flight from when the thread sends it until the app
//! takes it, at the top of `App::apply`. A [`Ledger`] per kind of message
//! records who sent each one, so taking it frees only its sender, also
//! when a newer watcher of that kind runs by then. While as many
//! as the outlet holds are in flight, a send is refused as full, and the
//! thread sends fresh values next time. So a busy UI never meets a backlog
//! of stale meter readings, as with the bounded channels Iced's
//! subscriptions used.
//!
//! What a watcher reads can change while it runs, such as the lights on
//! screen after a pan. The app hands it a [`Watched`] value, sets the new
//! value there, and the thread waits on it ([`Outlet::wait`]), so a change
//! wakes it at once and starts no new thread (`docs/design/panels.md`,
//! Redraw, A pan or zoom does not disturb a light).

use crate::Message;
use loom_ui::Waker;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// A value the app sets and a watcher thread reads, with a version number
/// that moves on each change. It outlives any one watcher: hiding every
/// light and showing one again starts a new watcher on the same value, so
/// it has no closed flag of its own. Each watcher's own handle says when it
/// stops (`Flow.closed`).
///
/// The values compared are finite, so an equal value never looks like a
/// change: loading a patch clamps or replaces a parameter that is not
/// (`loom-patch/src/flatten.rs`). Every lock is held only to clone or
/// replace the value, never across a read of the engine or a wait.
#[derive(Debug, Default)]
pub(crate) struct Watched<T>(Arc<(Mutex<Held<T>>, Condvar)>);

/// What a [`Watched`] keeps under its lock.
#[derive(Debug, Default)]
pub(crate) struct Held<T> {
    value: T,
    /// Moves by one on each change.
    version: u64,
}

impl<T> Clone for Watched<T> {
    /// Another handle on the same value.
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Clone + PartialEq> Watched<T> {
    /// Replaces the value when `value` differs, moves the version, and wakes
    /// a waiting thread. An equal value changes nothing, not even the
    /// version.
    pub(crate) fn set(&self, value: T) {
        let (lock, changed) = &*self.0;
        let mut held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if held.value != value {
            held.value = value;
            held.version += 1;
            changed.notify_all();
        }
    }

    /// The value, cloned, with the version it had.
    pub(crate) fn get(&self) -> (T, u64) {
        let held = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        (held.value.clone(), held.version)
    }
}

impl<T> Watched<T> {
    /// How many handles share the value, for a test to see that a thread
    /// let go of its own.
    #[cfg(test)]
    pub(crate) fn handles(&self) -> usize {
        Arc::strong_count(&self.0)
    }
}

/// A watched value as its watcher's handle sees it: something to wake when
/// the handle drops, whatever the value's type.
trait Wake: Send + Sync {
    /// Wakes a thread waiting on the value, under the value's lock.
    fn wake(&self);
}

impl<T: Send> Wake for (Mutex<Held<T>>, Condvar) {
    fn wake(&self) {
        // Locked, so a thread between its check and its wait cannot miss it.
        let _held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        self.1.notify_all();
    }
}

/// What a watcher and its handle share.
#[derive(Debug)]
struct Flow {
    /// Set when the handle drops.
    closed: AtomicBool,
    /// Messages sent and not yet taken.
    in_flight: AtomicUsize,
    /// The most that may be in flight at once.
    capacity: usize,
    /// Wakes a blocking send when the app takes a message or the handle
    /// drops.
    room: (Mutex<()>, Condvar),
}

impl Flow {
    fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            closed: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            capacity: capacity.max(1),
            room: (Mutex::new(()), Condvar::new()),
        })
    }

    /// One of its messages was taken: room for another. Never below zero.
    fn release(&self) {
        let _ = self
            .in_flight
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        self.notify();
    }

    /// Wakes a blocking send, to look again.
    fn notify(&self) {
        let _guard = self.room.0.lock().unwrap_or_else(PoisonError::into_inner);
        self.room.1.notify_all();
    }
}

/// Why a send did not go.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SendError {
    /// As many as the outlet holds are in flight: send fresh values later.
    Full,
    /// The handle dropped, or the runtime is gone: the thread should end.
    Closed,
}

impl SendError {
    /// True when the outlet was full, not closed.
    pub(crate) fn is_full(&self) -> bool {
        *self == Self::Full
    }
}

/// The thread's end of a watcher: sends values as the app's messages.
pub(crate) struct Outlet<T> {
    flow: Arc<Flow>,
    sink: Box<dyn Fn(T) -> bool + Send>,
}

impl<T> Outlet<T> {
    /// True once the handle dropped, so the thread should end.
    pub(crate) fn is_closed(&self) -> bool {
        self.flow.closed.load(Ordering::SeqCst)
    }

    /// Waits up to `timeout` while `watched` is still at the version `seen`
    /// and the handle has not dropped, both checked under the value's lock.
    /// With nothing seen yet it returns at once. So a change made while the
    /// thread was busy is never lost: the version moved, and the next wait
    /// returns at once. The thread checks [`Outlet::is_closed`] after it.
    pub(crate) fn wait<V>(&self, watched: &Watched<V>, seen: Option<u64>, timeout: Duration) {
        let (lock, changed) = &*watched.0;
        let held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = changed
            .wait_timeout_while(held, timeout, |held| Some(held.version) == seen && !self.is_closed())
            .unwrap_or_else(PoisonError::into_inner);
    }

    /// Takes a place for one message, or says why there is none.
    fn reserve(&self) -> Result<(), SendError> {
        if self.is_closed() {
            return Err(SendError::Closed);
        }
        let flow = &self.flow;
        let mut now = flow.in_flight.load(Ordering::SeqCst);
        loop {
            if now >= flow.capacity {
                return Err(SendError::Full);
            }
            match flow
                .in_flight
                .compare_exchange(now, now + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return Ok(()),
                Err(seen) => now = seen,
            }
        }
    }

    /// Hands `value` over, in the place just reserved.
    fn deliver(&self, value: T) -> Result<(), SendError> {
        if (self.sink)(value) {
            Ok(())
        } else {
            self.flow.closed.store(true, Ordering::SeqCst);
            Err(SendError::Closed)
        }
    }

    /// Sends `value` unless the outlet is full or closed.
    pub(crate) fn try_send(&self, value: T) -> Result<(), SendError> {
        self.reserve()?;
        self.deliver(value)
    }

    /// Sends `value`, waiting while the outlet is full. False once closed.
    /// For a watcher whose every message must arrive, such as a control
    /// command.
    pub(crate) fn send(&self, value: T) -> bool {
        loop {
            match self.reserve() {
                Ok(()) => return self.deliver(value).is_ok(),
                Err(SendError::Closed) => return false,
                Err(SendError::Full) => {
                    // Room may come between the check and the wait, so the
                    // wait is short and the check runs again.
                    let guard = self.flow.room.0.lock().unwrap_or_else(PoisonError::into_inner);
                    let _ = self.flow.room.1.wait_timeout(guard, Duration::from_millis(50));
                }
            }
        }
    }
}

/// The app's end of a watcher: it lives in the app's state, and dropping it
/// stops the thread.
pub(crate) struct Watcher {
    flow: Arc<Flow>,
    /// What the watcher reads, as a hash: a new key restarts it.
    key: u64,
    /// The value its thread waits on, if any, woken when the handle drops.
    watched: Option<Arc<dyn Wake>>,
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watcher")
            .field("flow", &self.flow)
            .field("key", &self.key)
            .field("waits_on_a_value", &self.watched.is_some())
            .finish()
    }
}

impl Watcher {
    /// Starts `run` on a thread with an outlet that holds `capacity`
    /// messages and sends each value as `wrap` makes it, through `waker`.
    /// Each send is noted in `ledger`, the one for this kind of message.
    pub(crate) fn start<T: 'static>(
        ledger: &Ledger,
        key: u64,
        capacity: usize,
        waker: Waker<Message>,
        wrap: fn(T) -> Message,
        run: impl FnOnce(Outlet<T>) + Send + 'static,
    ) -> Self {
        Self::spawn(ledger, key, capacity, waker, wrap, None, run)
    }

    /// Starts a watcher as [`Watcher::start`] does, for a thread that waits
    /// on `watched` ([`Outlet::wait`]). The handle registers the value, so
    /// its drop wakes that wait and the thread ends at once.
    pub(crate) fn start_on<T: 'static, V: Send + 'static>(
        ledger: &Ledger,
        key: u64,
        capacity: usize,
        waker: Waker<Message>,
        wrap: fn(T) -> Message,
        watched: &Watched<V>,
        run: impl FnOnce(Outlet<T>) + Send + 'static,
    ) -> Self {
        let wake: Arc<dyn Wake> = watched.0.clone();
        Self::spawn(ledger, key, capacity, waker, wrap, Some(wake), run)
    }

    /// Starts the thread, for [`Watcher::start`] and [`Watcher::start_on`].
    fn spawn<T: 'static>(
        ledger: &Ledger,
        key: u64,
        capacity: usize,
        waker: Waker<Message>,
        wrap: fn(T) -> Message,
        watched: Option<Arc<dyn Wake>>,
        run: impl FnOnce(Outlet<T>) + Send + 'static,
    ) -> Self {
        let flow = Flow::new(capacity);
        let sender = flow.clone();
        let ledger = ledger.clone();
        let outlet = Outlet {
            flow: flow.clone(),
            // Noted and queued under one lock, so the ledger keeps the
            // queue's order for this kind of message.
            sink: Box::new(move |value| {
                let mut sent = ledger.0.lock().unwrap_or_else(PoisonError::into_inner);
                sent.push_back(sender.clone());
                let queued = waker.send(wrap(value));
                if !queued {
                    sent.pop_back();
                }
                queued
            }),
        };
        std::thread::spawn(move || run(outlet));
        Self { flow, key, watched }
    }

    /// The key it was started with.
    pub(crate) fn key(&self) -> u64 {
        self.key
    }

    /// Messages sent and not yet taken, for a test.
    #[cfg(test)]
    pub(crate) fn in_flight(&self) -> usize {
        self.flow.in_flight.load(Ordering::SeqCst)
    }
}

/// Who sent each message of one kind still in the queue, oldest first.
/// The app takes messages in queue order, so the oldest entry is the
/// watcher whose message it took: only that watcher gets room again. A
/// message a dropped watcher sent before a new engine's watcher started
/// then frees nothing on the new one. One ledger per kind of message,
/// kept on the `App`, since it outlives each watcher.
///
/// It rests on one assumption: every queued message of a watched kind
/// reaches `App::apply`, in queue order, and nothing filters or drops one
/// before `watched_message_taken` runs first there. Routing passes these
/// messages through unchanged, and the runtime applies its queue in order;
/// only a quit stops early, when no watcher matters any more. A change that
/// drops one before `apply` would shift the ledger by one.
#[derive(Debug, Clone, Default)]
pub(crate) struct Ledger(Arc<Mutex<std::collections::VecDeque<Arc<Flow>>>>);

impl Ledger {
    /// The app took a message of this kind: its sender has room again.
    /// Nothing when no watcher sent one, as for a message a test built.
    pub(crate) fn taken(&self) {
        let sender = self.0.lock().unwrap_or_else(PoisonError::into_inner).pop_front();
        if let Some(sender) = sender {
            sender.release();
        }
    }
}

impl Drop for Watcher {
    /// Signals the thread and returns: it ends on its next check. A thread
    /// waiting on a watched value is woken, so that check comes at once. The
    /// value's lock and `Flow.room`'s are taken one after the other, never
    /// one inside the other.
    fn drop(&mut self) {
        self.flow.closed.store(true, Ordering::SeqCst);
        if let Some(watched) = &self.watched {
            watched.wake();
        }
        self.flow.notify();
    }
}

/// The hash of `value`, as a watcher's key.
pub(crate) fn key_of(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Keeps `slot` running the watcher for `wanted`: none when it is `None`,
/// and a new one when its key differs from the running one's. `start` makes
/// the watcher for a key.
pub(crate) fn keep<W: std::hash::Hash>(
    slot: &mut Option<Watcher>,
    wanted: Option<W>,
    start: impl FnOnce(u64, W) -> Watcher,
) {
    match wanted {
        None => *slot = None,
        Some(wanted) => {
            let key = key_of(&wanted);
            if slot.as_ref().map(Watcher::key) != Some(key) {
                // The old one stops first, so two never run at once.
                *slot = None;
                *slot = Some(start(key, wanted));
            }
        }
    }
}

/// A test's end of an outlet: what the thread sent, taken one at a time, as
/// the app takes its messages.
#[cfg(test)]
pub(crate) struct Inlet<T> {
    receiver: std::sync::mpsc::Receiver<T>,
    watcher: Watcher,
}

#[cfg(test)]
impl<T> Inlet<T> {
    /// The next value, waiting up to `timeout`, marked taken.
    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Option<T> {
        let value = self.receiver.recv_timeout(timeout).ok()?;
        self.watcher.flow.release();
        Some(value)
    }

    /// The next value, waiting up to two seconds.
    pub(crate) fn recv(&self) -> Option<T> {
        self.recv_timeout(Duration::from_secs(2))
    }

    /// The next value if one waits now, marked taken.
    pub(crate) fn try_recv(&self) -> Option<T> {
        let value = self.receiver.try_recv().ok()?;
        self.watcher.flow.release();
        Some(value)
    }

    /// Stops the thread, as dropping the app's handle does.
    pub(crate) fn close(self) {}
}

/// Runs a watcher's `run` on a thread with an outlet that holds `capacity`
/// messages, for a test to read through the inlet.
#[cfg(test)]
pub(crate) fn run_for_test<T: Send + 'static>(
    capacity: usize,
    run: impl FnOnce(Outlet<T>) + Send + 'static,
) -> Inlet<T> {
    let (outlet, inlet) = channel(capacity);
    std::thread::spawn(move || run(outlet));
    inlet
}

/// An outlet that holds `capacity` messages, and the test's end of it.
#[cfg(test)]
pub(crate) fn channel<T: Send + 'static>(capacity: usize) -> (Outlet<T>, Inlet<T>) {
    let flow = Flow::new(capacity);
    let (sender, receiver) = std::sync::mpsc::channel();
    let outlet = Outlet {
        flow: flow.clone(),
        sink: Box::new(move |value| sender.send(value).is_ok()),
    };
    (
        outlet,
        Inlet {
            receiver,
            watcher: Watcher {
                flow,
                key: 0,
                watched: None,
            },
        },
    )
}

#[cfg(test)]
mod tests;
