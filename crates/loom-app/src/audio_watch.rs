//! The audio watcher: reads the audio callback stats off the UI thread, and
//! wakes the UI only when something it shows or acts on changes (#90).
//!
//! In Iced every message redraws the window. A timer that sent the UI a
//! message twice a second, to keep the stats bar fresh, drew two frames a
//! second in an idle patch (`docs/design/panels.md`, Redraw). This thread
//! reads the same numbers and sends a message only when the stats bar would
//! show something new, or when the audio callbacks stop
//! (`docs/design/device-changes.md`, Audio device loss).

use crate::alerts::{Dropouts, Severity};
use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_core::EngineStats;
use loom_host_desktop::{CallbackStats, StatsSnapshot};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// How often the thread reads the stats: four times a second, the rate the
/// app checks for a stalled device.
pub const INTERVAL: Duration = Duration::from_millis(250);

/// The load is read every second read, so its number changes at most twice
/// a second, as with the old timer. Dropouts show at once.
const LOAD_EVERY: u32 = 2;

/// What the UI hears from the watcher.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// What the stats bar shows changed: the load, its maximum, the dropout
    /// count, the dropout alarm, or the telemetry drops. `dropouts` is the
    /// count, and `at` the time the watcher read it, so the UI's alarm
    /// ends at the same time as the watcher's.
    Stats {
        /// Callbacks that overran, plus stream errors.
        dropouts: u64,
        /// When the watcher read the count.
        at: Instant,
        /// Peak callback load in this tick, for the warning colour.
        peak: f64,
        /// Highest tick peak in the last ten seconds.
        max: f64,
    },
    /// The callbacks did not advance since the last read, or the stream
    /// reported an error. The UI checks for device loss.
    Stalled {
        /// Callbacks so far.
        callbacks: u64,
        /// When the watcher last saw the count grow.
        advanced: Instant,
    },
}

/// What the stats bar shows of the audio stats. Two reads with the same
/// `Shown` draw the same bar.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    /// The load text, as the bar shows it.
    load: String,
    severity: Severity,
    dropouts: u64,
    alarm: bool,
    telemetry_dropped: u64,
}

/// The watcher's state between reads. Plain data with the time passed in,
/// so tests drive it with a fake clock.
pub struct Watcher {
    callbacks: u64,
    advanced: Instant,
    dropouts: Dropouts,
    shown: Option<Shown>,
    reads: u32,
    peaks: VecDeque<(Instant, f64)>,
    current_peak: f64,
    current_max: f64,
}

impl Watcher {
    /// A watcher that has seen no callbacks yet, started at `now`.
    pub fn new(now: Instant) -> Self {
        Self {
            callbacks: 0,
            advanced: now,
            dropouts: Dropouts::default(),
            shown: None,
            reads: 0,
            peaks: VecDeque::new(),
            current_peak: 0.0,
            current_max: 0.0,
        }
    }

    /// Takes one read of the stats and returns what the UI must hear.
    /// `telemetry_dropped` is the engine's count of dropped telemetry.
    #[cfg(test)]
    pub fn read(&mut self, stats: &StatsSnapshot, telemetry_dropped: u64, now: Instant) -> Vec<Event> {
        self.read_peak(stats, stats.last_load, telemetry_dropped, now)
    }

    /// Reads a tick peak taken atomically from all audio callbacks.
    pub fn read_peak(&mut self, stats: &StatsSnapshot, peak: f64, telemetry_dropped: u64, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        if stats.is_dead || stats.callbacks == self.callbacks {
            events.push(Event::Stalled {
                callbacks: stats.callbacks,
                advanced: self.advanced,
            });
        } else {
            self.callbacks = stats.callbacks;
            self.advanced = now;
        }

        let dropouts = stats.overruns + stats.stream_errors;
        self.dropouts.update(dropouts, now);
        self.reads = self.reads.wrapping_add(1);
        if self.reads.is_multiple_of(LOAD_EVERY) {
            self.current_peak = peak;
            self.peaks.push_back((now, peak));
            while self
                .peaks
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at) >= Duration::from_secs(10))
            {
                self.peaks.pop_front();
            }
            self.current_max = self.peaks.iter().map(|(_, value)| *value).fold(0.0, f64::max);
        }
        let load = match &self.shown {
            Some(old) if !self.reads.is_multiple_of(LOAD_EVERY) => old.load.clone(),
            _ => crate::snapshot::load_text(stats.last_load, self.current_max),
        };
        let shown = Shown {
            load,
            severity: crate::alerts::load_severity(self.current_peak),
            dropouts,
            alarm: self.dropouts.severity(now) == Severity::Alarm,
            telemetry_dropped,
        };
        // The first read only learns what the bar shows: the UI drew it
        // when it started.
        if self.shown.as_ref().is_some_and(|old| *old != shown) {
            events.push(Event::Stats {
                dropouts,
                at: now,
                peak: self.current_peak,
                max: self.current_max,
            });
        }
        self.shown = Some(shown);
        events
    }
}

/// The stats to watch: one audio stream's and the engine's. A new stream
/// has new stats, so the subscription restarts with it.
#[derive(Clone)]
pub struct Watch {
    /// The audio stream's callback stats.
    pub stats: Arc<CallbackStats>,
    /// The engine's stats, for dropped telemetry.
    pub engine: Arc<EngineStats>,
}

impl Hash for Watch {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-app audio watch".hash(state);
        (Arc::as_ptr(&self.stats) as usize).hash(state);
        (Arc::as_ptr(&self.engine) as usize).hash(state);
    }
}

/// Reads the stats every [`INTERVAL`] and yields the events the UI must
/// hear. The thread stops when the subscription ends. The end of the
/// dropout alarm ([`crate::alerts::DROPOUT_ALARM`]) is an event too, so the
/// bar turns back from red without a timer on the UI thread.
pub fn stream(watch: &Watch) -> impl Stream<Item = Event> + use<> {
    let (mut tx, rx) = mpsc::channel(4);
    let watch = watch.clone();
    std::thread::spawn(move || {
        let mut watcher = Watcher::new(Instant::now());
        while !tx.is_closed() {
            let dropped = watch.engine.dropped_meters.load(Ordering::Relaxed)
                + watch.engine.dropped_scope.load(Ordering::Relaxed);
            let peak = if (watcher.reads + 1).is_multiple_of(LOAD_EVERY) {
                watch.stats.take_peak_since_read()
            } else {
                0.0
            };
            for event in watcher.read_peak(&watch.stats.snapshot(), peak, dropped, Instant::now()) {
                match tx.try_send(event) {
                    // The UI is busy: it reads the stats itself when it
                    // draws, and the next read sends again.
                    Ok(()) => {}
                    Err(e) if e.is_full() => {}
                    Err(_) => return,
                }
            }
            std::thread::sleep(INTERVAL);
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(callbacks: u64, load: f64, overruns: u64) -> StatsSnapshot {
        StatsSnapshot {
            callbacks,
            last_load: load,
            max_load: load,
            overruns,
            ..StatsSnapshot::default()
        }
    }

    /// Steady callbacks at a steady load: nothing to send, so an idle
    /// patch draws no frames (#90).
    #[test]
    fn steady_audio_sends_nothing_after_first_load_tick() {
        let start = Instant::now();
        let mut watcher = Watcher::new(start);
        for read in 1..=40u64 {
            let now = start + INTERVAL * read as u32;
            let events = watcher.read(&stats(read * 10, 0.02, 0), 0, now);
            assert_eq!(events.len(), usize::from(read == 2), "read {read}");
        }
    }

    /// A load change of one percent or more shows within two reads, once.
    /// Less than half a percent shows the same text, so nothing is sent.
    #[test]
    fn a_load_change_is_sent_once() {
        let start = Instant::now();
        let mut watcher = Watcher::new(start);
        let mut sent = Vec::new();
        for read in 0..10u32 {
            let load = match read {
                0..2 => 0.02,
                2..4 => 0.021,
                _ => 0.05,
            };
            let now = start + INTERVAL * read;
            sent.extend(watcher.read(&stats(u64::from(read) + 1, load, 0), 0, now));
        }
        assert_eq!(
            sent,
            vec![
                Event::Stats {
                    dropouts: 0,
                    at: start + INTERVAL,
                    peak: 0.02,
                    max: 0.02
                },
                Event::Stats {
                    dropouts: 0,
                    at: start + INTERVAL * 5,
                    peak: 0.05,
                    max: 0.05
                },
            ]
        );
    }

    /// One callback spike colours its tick and leaves the ten-second max.
    #[test]
    fn callback_spike_is_visible_then_expires() {
        let start = Instant::now();
        let mut watcher = Watcher::new(start);
        let low = stats(1, 0.2, 0);
        let _ = watcher.read_peak(&low, 0.2, 0, start);
        let spike = watcher.read_peak(&low, 0.95, 0, start + INTERVAL);
        assert!(spike.iter().any(|event| matches!(
            event,
            Event::Stats {
                peak: 0.95,
                max: 0.95,
                ..
            }
        )));
        let mut expired = false;
        for read in 2..44 {
            let events = watcher.read_peak(&low, 0.2, 0, start + INTERVAL * read);
            expired |= events
                .iter()
                .any(|event| matches!(event, Event::Stats { max: 0.2, .. }));
        }
        assert!(expired, "the max should fall after ten seconds of quiet");
    }

    /// A dropout is sent, and so is the end of its alarm ten seconds later.
    /// Nothing is sent in between.
    #[test]
    fn dropouts_and_the_end_of_their_alarm_are_sent() {
        let start = Instant::now();
        let mut watcher = Watcher::new(start);
        let mut sent = Vec::new();
        for read in 0..60u32 {
            let now = start + INTERVAL * read;
            let overruns = u64::from(read >= 4);
            for event in watcher.read(&stats(u64::from(read) + 1, 0.02, overruns), 0, now) {
                sent.push((read, event));
            }
        }
        let at = |read: u32| start + INTERVAL * read;
        assert_eq!(
            sent,
            vec![
                (
                    1,
                    Event::Stats {
                        dropouts: 0,
                        at: at(1),
                        peak: 0.02,
                        max: 0.02
                    }
                ),
                (
                    4,
                    Event::Stats {
                        dropouts: 1,
                        at: at(4),
                        peak: 0.02,
                        max: 0.02
                    }
                ),
                // Red for ten seconds from read 4: off at read 44.
                (
                    44,
                    Event::Stats {
                        dropouts: 1,
                        at: at(44),
                        peak: 0.02,
                        max: 0.02,
                    }
                ),
            ]
        );
    }

    /// Callbacks that stop advancing are reported on every read, with the
    /// time they last advanced, so the UI times the stall from there.
    #[test]
    fn a_stall_is_reported_with_the_last_advance() {
        let start = Instant::now();
        let mut watcher = Watcher::new(start);
        assert!(watcher.read(&stats(5, 0.02, 0), 0, start).is_empty());
        let later = start + INTERVAL;
        assert_eq!(
            watcher.read(&stats(5, 0.02, 0), 0, later),
            vec![
                Event::Stalled {
                    callbacks: 5,
                    advanced: start
                },
                Event::Stats {
                    dropouts: 0,
                    at: later,
                    peak: 0.02,
                    max: 0.02
                },
            ]
        );
        let dead = StatsSnapshot {
            is_dead: true,
            ..stats(9, 0.02, 0)
        };
        assert!(matches!(
            watcher.read(&dead, 0, later + INTERVAL)[..],
            [Event::Stalled { callbacks: 9, .. }, ..]
        ));
    }
}
