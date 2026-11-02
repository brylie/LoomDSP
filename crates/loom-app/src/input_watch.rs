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

/// How long a stream that has never called back may stay quiet. Some devices
/// take seconds to start.
const FIRST_CALLBACK_LIMIT: Duration = Duration::from_secs(10);
/// How long a running stream may stay quiet.
const CALLBACK_LIMIT: Duration = Duration::from_secs(1);
/// How often the permission and the device list are checked.
const CHECK_EVERY: Duration = Duration::from_secs(1);
/// How often the worker looks at the stream.
const POLL: Duration = Duration::from_millis(250);

/// Why a stream that has not reported an error must still be replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// The system refused microphone access.
    PermissionDenied,
    /// The device this stream opened is no longer listed.
    DeviceMissing,
    /// The stream follows the system default, which is now another device.
    DefaultChanged,
}

/// What one look at the stream decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tick {
    /// Callbacks stopped for longer than the limit.
    stalled: bool,
    /// The permission and the device list should be checked now.
    check_due: bool,
}

/// The watcher's timing decisions, with the time given as an input so tests
/// need no clock and no sleep.
struct Health {
    callbacks: u64,
    last_callback: Instant,
    last_check: Instant,
    /// Whether silence counts as a failure. Hosts that cannot list devices
    /// have no real stream behind them, so their callbacks say nothing.
    detect_stalls: bool,
}

impl Health {
    /// Starts counting from this moment, with the callbacks seen so far.
    fn new(callbacks: u64, now: Instant, detect_stalls: bool) -> Self {
        Self {
            callbacks,
            last_callback: now,
            last_check: now,
            detect_stalls,
        }
    }

    /// Takes the current callback count and decides what is due.
    fn tick(&mut self, callbacks: u64, now: Instant) -> Tick {
        let mut stalled = false;
        if callbacks != self.callbacks {
            self.callbacks = callbacks;
            self.last_callback = now;
        } else if self.detect_stalls {
            let limit = if callbacks == 0 {
                FIRST_CALLBACK_LIMIT
            } else {
                CALLBACK_LIMIT
            };
            stalled = now.duration_since(self.last_callback) > limit;
        }
        let check_due = now.duration_since(self.last_check) >= CHECK_EVERY;
        if check_due {
            self.last_check = now;
        }
        Tick { stalled, check_due }
    }
}

/// Decides from the permission and the device list whether the stream must
/// be replaced. A list that could not be read says nothing about the device:
/// listing can fail for a moment, and that must not interrupt a live input.
fn check_failure(
    denied: bool,
    listed: Option<&Result<crate::devices::Inputs, String>>,
    device: &str,
    system_default: bool,
) -> Option<Failure> {
    if denied {
        return Some(Failure::PermissionDenied);
    }
    let inputs = listed?.as_ref().ok()?;
    if !inputs.devices.iter().any(|name| name == device) {
        Some(Failure::DeviceMissing)
    } else if system_default && inputs.default.as_deref() != Some(device) {
        Some(Failure::DefaultChanged)
    } else {
        None
    }
}

/// Samples the host and the stream until the subscription ends.
fn run(watch: &Watch, mut tx: mpsc::Sender<Event>) {
    let counts = || {
        (
            watch.stats.input_underruns.load(Ordering::Relaxed),
            watch.stats.input_overruns.load(Ordering::Relaxed),
        )
    };
    // The first read always reaches the UI, even after a subscription
    // restart, so it can clear a warning that expired while restarting.
    let mut sent = None;
    let mut health = Health::new(watch.status.callbacks(), Instant::now(), watch.list.is_some());
    while !tx.is_closed() {
        let now = Instant::now();
        let tick = health.tick(watch.status.callbacks(), now);
        if tick.stalled {
            watch.status.fail();
        }
        if tick.check_due {
            let denied = watch.permission_denied.is_some_and(|check| check());
            let listed = watch.list.map(|list| list());
            if check_failure(denied, listed.as_ref(), &watch.device, watch.system_default).is_some() {
                watch.status.fail();
            }
        }
        let ready = watch.active.iter().any(|source| watch.displays.read(*source) >= 0.5);
        let event = watch.tracker.read(counts(), !watch.status.is_live(), ready, now);
        let next = (event.underruns, event.overruns, event.show, event.failed, event.ready);
        if Some(next) != sent && tx.try_send(event).is_ok() {
            sent = Some(next);
        }
        std::thread::sleep(POLL);
    }
}

/// Emits only when the counts, warning visibility, or stream health change.
pub fn stream(watch: &Watch) -> impl Stream<Item = Event> + use<> {
    let (tx, rx) = mpsc::channel(4);
    let watch = watch.clone();
    std::thread::spawn(move || run(&watch, tx));
    rx
}

#[cfg(test)]
mod tests;
