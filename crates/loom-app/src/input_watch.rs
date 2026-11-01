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
mod tests {
    use super::*;

    /// A removed or replaced system input is reported even without a callback error.
    #[test]
    fn watcher_reports_a_missing_or_changed_input_device() {
        use iced::futures::StreamExt;
        use std::sync::atomic::AtomicUsize;

        static LIST_CALLS: AtomicUsize = AtomicUsize::new(0);
        fn missing() -> Result<crate::devices::Inputs, String> {
            LIST_CALLS.fetch_add(1, Ordering::Relaxed);
            Ok(crate::devices::Inputs {
                default: Some("Other".into()),
                devices: vec!["Other".into()],
            })
        }
        fn changed_default() -> Result<crate::devices::Inputs, String> {
            LIST_CALLS.fetch_add(1, Ordering::Relaxed);
            Ok(crate::devices::Inputs {
                default: Some("Other".into()),
                devices: vec!["Test input".into(), "Other".into()],
            })
        }
        fn unchanged() -> Result<crate::devices::Inputs, String> {
            LIST_CALLS.fetch_add(1, Ordering::Relaxed);
            Ok(crate::devices::Inputs {
                default: Some("Test input".into()),
                devices: vec!["Test input".into(), "Other".into()],
            })
        }
        for (list, system_default, should_fail) in [
            (missing as fn() -> _, false, true),
            (changed_default, true, true),
            (unchanged, true, false),
        ] {
            LIST_CALLS.store(0, Ordering::Relaxed);
            let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
            let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
            let watch = Watch {
                status: writer.status(),
                stats: parts.controller.stats().clone(),
                tracker: Arc::new(Tracker::new((0, 0))),
                device: "Test input".into(),
                system_default,
                list: Some(list),
                permission_denied: None,
                active: Vec::new(),
                displays: parts.controller.display_reader(),
            };
            let (send, receive) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let mut events = Box::pin(stream(&watch));
                iced::futures::executor::block_on(async {
                    while let Some(event) = events.next().await {
                        if event.failed {
                            let _ = send.send(());
                            break;
                        }
                    }
                });
            });
            let status = writer.status();
            let deadline = Instant::now() + Duration::from_secs(if should_fail { 5 } else { 2 });
            let mut failed = false;
            while Instant::now() < deadline {
                // Keep callbacks flowing so only the device list can fail the stream.
                writer.callback_received();
                if receive.recv_timeout(Duration::from_millis(50)).is_ok() {
                    failed = true;
                    break;
                }
            }
            let remained_live = status.is_live();
            if !failed {
                // Stop the watcher cleanly after the control case or a timeout.
                status.fail();
                let _ = receive.recv_timeout(Duration::from_secs(2));
            }
            worker.join().unwrap();
            assert!(LIST_CALLS.load(Ordering::Relaxed) > 0, "the device list was checked");
            assert_eq!(failed, should_fail, "the list determines stream failure");
            assert_eq!(remained_live, !should_fail);
        }
    }

    /// The watcher detects permission denied after a stream started.
    #[test]
    fn watcher_reports_a_late_permission_denial() {
        use iced::futures::StreamExt;
        let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
        let watch = Watch {
            status: writer.status(),
            stats: parts.controller.stats().clone(),
            tracker: Arc::new(Tracker::new((0, 0))),
            device: "Test input".into(),
            system_default: false,
            list: None,
            permission_denied: Some(|| true),
            active: Vec::new(),
            displays: parts.controller.display_reader(),
        };
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut events = Box::pin(stream(&watch));
            iced::futures::executor::block_on(async {
                while let Some(event) = events.next().await {
                    if event.failed {
                        let _ = send.send(());
                        break;
                    }
                }
            });
        });
        receive
            .recv_timeout(Duration::from_secs(4))
            .expect("permission denial was reported");
        assert!(!writer.status().is_live());
    }

    /// A replacement watcher keeps the original warning deadline.
    #[test]
    fn drift_warning_survives_a_watcher_restart() {
        let start = Instant::now();
        let tracker = Arc::new(Tracker::new((0, 0)));
        assert!(tracker.read((1, 0), false, false, start).show);
        let replacement = tracker.clone();
        assert!(
            replacement
                .read((1, 0), false, false, start + Duration::from_secs(9))
                .show
        );
        assert!(
            !replacement
                .read((1, 0), false, false, start + Duration::from_secs(10))
                .show
        );
        assert!(
            replacement
                .read((1, 1), false, false, start + Duration::from_secs(11))
                .show
        );
    }

    /// A restarted stream sends its current warning state on its first read.
    #[test]
    fn restarted_stream_reports_the_current_warning() {
        use iced::futures::StreamExt;
        for (age, shown) in [(2, true), (12, false)] {
            let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
            let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
            let tracker = Arc::new(Tracker::new((0, 0)));
            tracker.read((1, 0), false, false, Instant::now() - Duration::from_secs(age));
            parts.controller.stats().input_underruns.store(1, Ordering::Relaxed);
            let watch = Watch {
                status: writer.status(),
                stats: parts.controller.stats().clone(),
                tracker,
                device: "Test input".into(),
                system_default: false,
                list: None,
                permission_denied: None,
                active: Vec::new(),
                displays: parts.controller.display_reader(),
            };
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut events = Box::pin(stream(&watch));
                let event = iced::futures::executor::block_on(events.next());
                let _ = send.send(event);
            });
            let event = receive
                .recv_timeout(Duration::from_secs(3))
                .expect("watcher sends its first state")
                .unwrap();
            assert_eq!((event.underruns, event.overruns, event.show), (1, 0, shown));
        }
    }

    /// An older read from a retiring watcher cannot roll counters back.
    #[test]
    fn stale_watcher_reads_do_not_restart_the_warning() {
        let start = Instant::now();
        let tracker = Tracker::new((0, 0));
        assert!(tracker.read((2, 1), false, false, start).show);
        let stale = tracker.read((1, 1), false, false, start + Duration::from_secs(1));
        assert_eq!((stale.underruns, stale.overruns), (2, 1));
        assert!(!tracker.read((2, 1), false, false, start + Duration::from_secs(10)).show);
    }

    /// A correction is shown for ten seconds; a failed stream is reported.
    #[test]
    fn correction_warning_expires_without_repeated_updates() {
        let start = Instant::now();
        let mut state = State::new((0, 0));
        assert!(!state.read((0, 0), false, false, start).show);
        assert!(state.read((1, 0), false, true, start).show);
        assert!(state.read((1, 0), false, true, start + Duration::from_secs(9)).show);
        let expired = state.read((1, 0), true, false, start + Duration::from_secs(10));
        assert!(!expired.show);
        assert!(expired.failed);
    }
}
