//! Watches input stream health and drift counters without an idle UI timer.

use iced::futures::{Stream, channel::mpsc};
use loom_core::{EngineStats, audio_input::InputStatus};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::{Duration, Instant};

/// The input state relevant to the status bar and reconnect logic.
#[derive(Clone, Copy, Debug)]
pub struct Event {
    /// Input underrun corrections since engine start.
    pub underruns: u64,
    /// Input overrun corrections since engine start.
    pub overruns: u64,
    /// Whether the counts grew in the last ten seconds.
    pub show: bool,
    /// Whether the stream failed.
    pub failed: bool,
    /// An Audio In module is receiving frames.
    pub ready: bool,
}

/// One running input and its engine statistics.
#[derive(Clone)]
pub struct Watch {
    /// Stream health.
    pub status: InputStatus,
    /// The engine's counters.
    pub stats: Arc<EngineStats>,
    /// Drift history shared by subscriptions for this input stream.
    pub tracker: Arc<Tracker>,
    /// The device this stream opened.
    pub device: String,
    /// True when it follows the system default.
    pub system_default: bool,
    /// Lists devices on the watcher thread when the host supports it.
    pub list: Option<fn() -> Result<crate::devices::Inputs, String>>,
    /// Checks whether the OS refused microphone access.
    pub permission_denied: Option<fn() -> bool>,
    /// Display cells of Audio In modules, including off-screen ones.
    pub active: Vec<loom_core::DisplaySource>,
    /// Reads the cells without taking trigger values.
    pub displays: loom_core::DisplayReader,
}

impl Hash for Watch {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom input watch".hash(state);
        self.status.identity().hash(state);
        (Arc::as_ptr(&self.stats) as usize).hash(state);
        self.active.hash(state);
    }
}

/// Tracks whether corrections were recent and whether the stream failed.
struct State {
    previous: (u64, u64),
    last_growth: Option<Instant>,
}

impl State {
    /// Starts from the counters already displayed by the engine.
    fn new(previous: (u64, u64)) -> Self {
        Self {
            previous,
            last_growth: None,
        }
    }

    /// Reads the current counters without requiring a UI timer.
    fn read(&mut self, counts: (u64, u64), failed: bool, ready: bool, now: Instant) -> Event {
        if counts.0 > self.previous.0 || counts.1 > self.previous.1 {
            self.last_growth = Some(now);
        }
        // Two generations of the watcher can briefly overlap. An older
        // read must not roll the counters back or restart the alarm later.
        self.previous = (self.previous.0.max(counts.0), self.previous.1.max(counts.1));
        Event {
            underruns: self.previous.0,
            overruns: self.previous.1,
            show: self
                .last_growth
                .is_some_and(|at| now.duration_since(at) < Duration::from_secs(10)),
            failed,
            ready,
        }
    }
}

/// Drift history that survives a watcher restart for the same input stream.
pub struct Tracker(Mutex<State>);

impl Tracker {
    /// Starts tracking from the engine's current counters.
    pub fn new(counts: (u64, u64)) -> Self {
        Self(Mutex::new(State::new(counts)))
    }

    /// Reads counters on the watcher thread, keeping the last growth time.
    fn read(&self, counts: (u64, u64), failed: bool, ready: bool, now: Instant) -> Event {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .read(counts, failed, ready, now)
    }
}

/// Emits only when the counts, warning visibility, or stream health change.
pub fn stream(watch: &Watch) -> impl Stream<Item = Event> + use<> {
    let (mut tx, rx) = mpsc::channel(4);
    let watch = watch.clone();
    std::thread::spawn(move || {
        let counts = || {
            (
                watch.stats.input_underruns.load(Ordering::Relaxed),
                watch.stats.input_overruns.load(Ordering::Relaxed),
            )
        };
        // The first read always reaches the UI, even after a subscription
        // restart, so it can clear a warning that expired while restarting.
        let mut sent = None;
        let mut last_list = Instant::now();
        let mut callbacks = watch.status.callbacks();
        let mut last_callback = Instant::now();
        while !tx.is_closed() {
            let now = Instant::now();
            let current = watch.status.callbacks();
            if current != callbacks {
                callbacks = current;
                last_callback = now;
            } else if watch.list.is_some()
                && now.duration_since(last_callback) > Duration::from_secs(if callbacks == 0 { 10 } else { 1 })
            {
                watch.status.fail();
            }
            if last_list.elapsed() >= Duration::from_secs(1) {
                last_list = Instant::now();
                if watch.permission_denied.is_some_and(|check| check()) {
                    watch.status.fail();
                }
                if let Some(list) = watch.list
                    && let Ok(inputs) = list()
                    && (!inputs.devices.contains(&watch.device)
                        || (watch.system_default && inputs.default.as_deref() != Some(&watch.device)))
                {
                    watch.status.fail();
                }
            }
            let ready = watch.active.iter().any(|source| watch.displays.read(*source) >= 0.5);
            let event = watch.tracker.read(counts(), !watch.status.is_live(), ready, now);
            let next = (event.underruns, event.overruns, event.show, event.failed, event.ready);
            if Some(next) != sent && tx.try_send(event).is_ok() {
                sent = Some(next);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    rx
}

#[cfg(test)]
mod tests;
