//! The telemetry thread: drains the engine's meter and scope queues off the
//! UI thread, and wakes the UI only when something it draws changes
//! (`docs/design/panels.md`, How levels reach a meter, and Drawing the
//! scope trace).
//!
//! Every message to the app redraws the window, so draining telemetry on a
//! UI timer would draw frames even when every meter is silent.

use crate::live::{Level, Levels, ScopeTrace, Trigger};
use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_core::{MeterReading, ProbeId, ScopeChunk, Telemetry};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How often the thread drains the queues: about 30 times a second.
pub const INTERVAL: Duration = Duration::from_millis(33);

/// How often it drains them while a scope is on screen: about 60 times a
/// second, so a moving trace animates at the frame rate (NFR-PERF-04).
pub const SCOPE_INTERVAL: Duration = Duration::from_millis(16);

/// Above this peak (-80 dB) an output is carrying a signal
/// (`docs/design/panels.md`, Signal activity on ports).
pub const ACTIVITY_THRESHOLD: f32 = 0.0001;

/// How often activity is read: 5 times a second.
pub const ACTIVITY_INTERVAL: Duration = Duration::from_millis(200);

/// Reads with nothing above the threshold before a port is silent: 400 ms.
const SILENT_READS: u8 = 2;

/// What the UI draws from telemetry, so the thread reads only that.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WatchList {
    /// Meter probes whose levels are drawn.
    pub meters: HashSet<ProbeId>,
    /// Meter probes whose activity (carrying or silent) is shown.
    pub activity: HashSet<ProbeId>,
    /// Scope probes on screen, with the trace each asks for.
    pub scopes: HashMap<ProbeId, ScopeWant>,
    /// True when scopes draw full traces (close zoom), false for a flat
    /// line at their level (middle zoom).
    pub full_traces: bool,
}

/// What one scope on screen shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScopeWant {
    /// Samples across the frame: the module's `window` at the sample rate.
    pub window: usize,
    /// Where the trace starts.
    pub trigger: Trigger,
}

/// What the thread found that the UI draws.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Update {
    /// Meter levels that changed by at least one of 256 steps.
    pub levels: Vec<(ProbeId, Level)>,
    /// Scope traces that changed: only what is drawn, so a still or silent
    /// signal sends none.
    pub traces: Vec<(ProbeId, loom_canvas::Trace)>,
    /// Activity probes whose state changed: true for carrying.
    pub activity: Vec<(ProbeId, bool)>,
}

impl Update {
    /// True when nothing changed.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty() && self.traces.is_empty() && self.activity.is_empty()
    }
}

/// One output's activity: carrying at once, silent after
/// [`SILENT_READS`] reads with nothing.
#[derive(Debug, Default, Clone, Copy)]
struct Activity {
    /// Highest peak since the last read. A meter reading's peak covers
    /// every frame of its block, so a one-sample trigger counts.
    peak: f32,
    carrying: bool,
    quiet_reads: u8,
}

/// Telemetry turned into what the UI draws, and what it last sent, so only
/// changes are sent. The thread runs one; `--frames` runs one on the UI
/// thread. Plain data with the time passed in, so tests drive it.
#[derive(Default)]
pub struct Watcher {
    levels: Levels,
    shown_levels: HashMap<ProbeId, (u8, u8)>,
    scopes: HashMap<ProbeId, ScopeTrace>,
    shown_traces: HashMap<ProbeId, loom_canvas::Trace>,
    activity: HashMap<ProbeId, Activity>,
    shown_activity: HashMap<ProbeId, bool>,
    last_activity_read: Option<Instant>,
}

impl Watcher {
    /// Takes the readings and chunks that arrived and returns what changed
    /// on screen. Call [`Watcher::sent`] once the UI has the update. A
    /// scope's samples are kept whether or not its trace is sent, so it
    /// can find a trigger in the last second.
    pub fn take(
        &mut self,
        watch: &WatchList,
        readings: &[MeterReading],
        chunks: impl IntoIterator<Item = ScopeChunk>,
        sample_rate: f32,
        now: Instant,
    ) -> Update {
        self.levels.update(readings, now);
        let current: Vec<(ProbeId, Level)> = self
            .levels
            .all()
            .filter(|(probe, _)| watch.meters.contains(probe))
            .collect();
        self.shown_levels
            .retain(|probe, _| current.iter().any(|(p, _)| p == probe));
        let levels = changed(&self.shown_levels, &current);

        self.scopes.retain(|probe, _| watch.scopes.contains_key(probe));
        self.shown_traces.retain(|probe, _| watch.scopes.contains_key(probe));
        for chunk in chunks {
            if watch.scopes.contains_key(&chunk.probe) {
                self.scopes
                    .entry(chunk.probe)
                    .or_insert_with(|| ScopeTrace::new(sample_rate))
                    .push(&chunk);
            }
        }
        let mut traces = Vec::new();
        for (probe, want) in &watch.scopes {
            let Some(scope) = self.scopes.get_mut(probe) else {
                continue;
            };
            scope.update(want.window, want.trigger);
            let trace = scope.canvas_trace(watch.full_traces);
            if self.shown_traces.get(probe) != Some(&trace) {
                traces.push((*probe, trace));
            }
        }
        let activity = self.take_activity(watch, readings, now);
        Update {
            levels,
            traces,
            activity,
        }
    }

    /// Activity (`docs/design/panels.md`, Signal activity on ports): a port
    /// is carrying as soon as a reading peaks above -80 dB, and silent
    /// after two reads, 200 ms apart, with nothing.
    fn take_activity(&mut self, watch: &WatchList, readings: &[MeterReading], now: Instant) -> Vec<(ProbeId, bool)> {
        self.activity.retain(|probe, _| watch.activity.contains(probe));
        self.shown_activity.retain(|probe, _| watch.activity.contains(probe));
        for reading in readings {
            if watch.activity.contains(&reading.probe) {
                let a = self.activity.entry(reading.probe).or_default();
                a.peak = a.peak.max(reading.peak.abs());
            }
        }
        let due = self
            .last_activity_read
            .is_none_or(|last| now.duration_since(last) >= ACTIVITY_INTERVAL);
        if due {
            self.last_activity_read = Some(now);
        }
        for a in self.activity.values_mut() {
            if a.peak > ACTIVITY_THRESHOLD {
                a.carrying = true;
                a.quiet_reads = 0;
            } else if due {
                a.quiet_reads = a.quiet_reads.saturating_add(1);
                if a.quiet_reads >= SILENT_READS {
                    a.carrying = false;
                }
            }
            if due || a.peak > ACTIVITY_THRESHOLD {
                a.peak = 0.0;
            }
        }
        self.activity
            .iter()
            .filter(|(probe, a)| self.shown_activity.get(probe).copied().unwrap_or(false) != a.carrying)
            .map(|(&probe, a)| (probe, a.carrying))
            .collect()
    }

    /// Notes that the UI has `update`, so it is not sent again.
    pub fn sent(&mut self, update: &Update) {
        for (probe, level) in &update.levels {
            self.shown_levels.insert(*probe, steps(*level));
        }
        for (probe, trace) in &update.traces {
            self.shown_traces.insert(*probe, trace.clone());
        }
        for (probe, carrying) in &update.activity {
            self.shown_activity.insert(*probe, *carrying);
        }
    }
}

/// Hands the engine's `Telemetry` to the telemetry thread, once, and tells
/// the thread what the UI draws.
#[derive(Clone)]
pub struct Monitor {
    telemetry: Arc<Mutex<Option<Telemetry>>>,
    watch: Arc<Mutex<WatchList>>,
    sample_rate: f32,
    generation: u64,
}

impl Monitor {
    /// `generation` counts the engines the app has built. Opening a patch
    /// builds a new one, and the subscription must restart to take its
    /// `Telemetry`. `sample_rate` is the engine's, for scope windows.
    pub fn new(telemetry: Telemetry, sample_rate: f32, generation: u64) -> Self {
        Self {
            telemetry: Arc::new(Mutex::new(Some(telemetry))),
            watch: Arc::new(Mutex::new(WatchList::default())),
            sample_rate,
            generation,
        }
    }

    /// Tells the thread what the UI draws now.
    pub fn watch(&self, list: WatchList) {
        *self.watch.lock().unwrap_or_else(PoisonError::into_inner) = list;
    }

    /// What the UI draws, as last told.
    pub fn watched(&self) -> WatchList {
        self.watch.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The engine's sample rate.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Takes the meter readings and scope chunks waiting now, on the
    /// caller's thread. For `--frames`, which runs the engine with no
    /// telemetry thread.
    pub fn take_telemetry(&self) -> (Vec<MeterReading>, Vec<ScopeChunk>) {
        let mut telemetry = self.telemetry.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(telemetry) = telemetry.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        let chunks = telemetry.scope().collect();
        (telemetry.meters().collect(), chunks)
    }

    /// Takes the meter readings waiting now, and drops the scope chunks.
    #[cfg(test)]
    pub fn take_readings(&self) -> Vec<MeterReading> {
        self.take_telemetry().0
    }
}

#[cfg(test)]
impl Monitor {
    /// Reads the meter queues once and turns the readings into levels, as
    /// `lights::Watch::read` reads the lights. For tests, which have no
    /// telemetry thread. One update, so the levels do not fall.
    pub fn read(&self) -> Vec<(ProbeId, Level)> {
        let mut levels = Levels::default();
        levels.update(&self.take_readings(), Instant::now());
        levels.all().collect()
    }
}

impl Hash for Monitor {
    // One monitor per engine: the subscription restarts with a new engine.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-app telemetry monitor".hash(state);
        self.generation.hash(state);
    }
}

/// A level as drawn: 256 steps for RMS and peak.
fn steps(level: Level) -> (u8, u8) {
    let step = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    (step(level.rms), step(level.peak))
}

/// The levels that differ from `shown` by at least one step.
pub fn changed(shown: &HashMap<ProbeId, (u8, u8)>, levels: &[(ProbeId, Level)]) -> Vec<(ProbeId, Level)> {
    levels
        .iter()
        .filter(|(probe, level)| shown.get(probe).copied().unwrap_or((0, 0)) != steps(*level))
        .copied()
        .collect()
}

/// Drains the queues every [`INTERVAL`], or [`SCOPE_INTERVAL`] while a
/// scope is on screen, and yields what changed on screen. The thread stops
/// when the app closes.
pub fn stream(monitor: &Monitor) -> impl Stream<Item = Update> + use<> {
    let (mut tx, rx) = mpsc::channel(1);
    let telemetry = monitor.telemetry.lock().ok().and_then(|mut t| t.take());
    let monitor = monitor.clone();
    if let Some(mut telemetry) = telemetry {
        std::thread::spawn(move || {
            let mut watcher = Watcher::default();
            while !tx.is_closed() {
                let watch = monitor.watched();
                let readings: Vec<MeterReading> = telemetry.meters().collect();
                let update = watcher.take(
                    &watch,
                    &readings,
                    telemetry.scope(),
                    monitor.sample_rate,
                    Instant::now(),
                );
                if !update.is_empty() {
                    match tx.try_send(update.clone()) {
                        Ok(()) => watcher.sent(&update),
                        // The UI has not taken the last one yet: try again
                        // next time, with fresh values.
                        Err(e) if e.is_full() => {}
                        Err(_) => break,
                    }
                }
                std::thread::sleep(if watch.scopes.is_empty() {
                    INTERVAL
                } else {
                    SCOPE_INTERVAL
                });
            }
        });
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_visible_changes_are_sent() {
        let probe = ProbeId(1);
        let mut shown = HashMap::new();
        let silent = [(probe, Level::default())];
        // A new probe at level 0 looks the same as no meter: nothing to send.
        assert!(changed(&shown, &silent).is_empty());
        let loud = [(probe, Level { rms: 0.5, peak: 0.8 })];
        assert_eq!(changed(&shown, &loud).len(), 1);
        shown.insert(probe, steps(loud[0].1));
        // Less than one step of 256: not drawn, not sent.
        let close = [(
            probe,
            Level {
                rms: 0.501,
                peak: 0.799,
            },
        )];
        assert!(changed(&shown, &close).is_empty());
    }

    fn chunk(probe: ProbeId, frame: u64, value: impl Fn(u64) -> f32) -> ScopeChunk {
        let mut samples = [0.0; loom_core::SCOPE_CHUNK];
        for (i, s) in samples.iter_mut().enumerate() {
            *s = value(frame + i as u64);
        }
        ScopeChunk {
            probe,
            frame,
            len: loom_core::SCOPE_CHUNK as u32,
            samples,
        }
    }

    fn scope_watch(probe: ProbeId, full: bool) -> WatchList {
        WatchList {
            scopes: HashMap::from([(
                probe,
                ScopeWant {
                    window: 480,
                    trigger: Trigger::Auto,
                },
            )]),
            full_traces: full,
            ..WatchList::default()
        }
    }

    /// Feeds the watcher 16 ms of a signal per call, as the thread does at
    /// 60 fps with 48 kHz audio, and returns how many updates carried a
    /// trace.
    fn traces_sent(watch: &WatchList, value: impl Fn(u64) -> f32 + Copy, calls: u64) -> usize {
        let probe = *watch.scopes.keys().next().unwrap();
        let mut watcher = Watcher::default();
        let start = Instant::now();
        let mut sent = 0;
        let chunk_frames = loom_core::SCOPE_CHUNK as u64;
        let per_call = 12; // 768 frames, 16 ms
        for call in 0..calls {
            let chunks: Vec<ScopeChunk> = (0..per_call)
                .map(|i| chunk(probe, (call * per_call + i) * chunk_frames, value))
                .collect();
            let update = watcher.take(watch, &[], chunks, 48_000.0, start + SCOPE_INTERVAL * call as u32);
            if !update.traces.is_empty() {
                sent += 1;
            }
            watcher.sent(&update);
        }
        sent
    }

    /// A silent or constant scope sends one trace, then none: nothing on
    /// screen changes (panels.md, Drawing the scope trace, Redraw).
    #[test]
    fn a_still_signal_sends_no_traces() {
        let watch = scope_watch(ProbeId(3), true);
        assert_eq!(traces_sent(&watch, |_| 0.0, 30), 1);
        assert_eq!(traces_sent(&watch, |_| 0.5, 30), 1);
    }

    /// A moving signal sends a trace each time it changes. At the middle
    /// zoom only the level is drawn, so a steady wave sends one.
    #[test]
    fn a_moving_signal_sends_traces_close_up_and_only_its_level_further_out() {
        let saw = |frame: u64| (std::f32::consts::TAU * 440.0 * frame as f32 / 48_000.0).sin();
        assert!(traces_sent(&scope_watch(ProbeId(3), true), saw, 30) > 20);
        assert_eq!(traces_sent(&scope_watch(ProbeId(3), false), saw, 30), 1);
    }

    /// panels.md, Signal activity on ports: carrying within one read of a
    /// signal above -80 dB, a one-sample trigger counts, and silent 400 ms
    /// after it stops. Each change is sent once.
    #[test]
    fn activity_turns_on_at_once_and_off_after_two_quiet_reads() {
        let probe = ProbeId(4);
        let watch = WatchList {
            activity: HashSet::from([probe]),
            ..WatchList::default()
        };
        let mut watcher = Watcher::default();
        let start = Instant::now();
        let mut changes = Vec::new();
        // The thread drains every 33 ms. One reading at 99 ms holds a
        // one-sample trigger: its block's peak is 1.0. Then silence.
        for tick in 0..30u32 {
            let ms = tick * 33;
            let peak = if ms == 99 { 1.0 } else { 0.0 };
            let reading = MeterReading {
                probe,
                frame: 0,
                frames: 128,
                peak,
                rms: peak / 11.3,
            };
            let now = start + Duration::from_millis(u64::from(ms));
            let update = watcher.take(&watch, &[reading], [], 48_000.0, now);
            for &(_, carrying) in &update.activity {
                changes.push((ms, carrying));
            }
            watcher.sent(&update);
        }
        assert_eq!(changes.len(), 2, "{changes:?}");
        assert_eq!(changes[0], (99, true), "carrying at once");
        let off = changes[1];
        assert!(!off.1);
        assert!(
            (99 + 300..=99 + 500).contains(&off.0),
            "silent about 400 ms later: {changes:?}"
        );
        // Below -80 dB is silent.
        let mut watcher = Watcher::default();
        let quiet = MeterReading {
            probe,
            frame: 0,
            frames: 128,
            peak: 0.000_05,
            rms: 0.0,
        };
        assert!(watcher.take(&watch, &[quiet], [], 48_000.0, start).is_empty());
    }

    /// Readings of probes the UI does not draw as meters are not sent: an
    /// activity probe must not redraw the window.
    #[test]
    fn only_drawn_meters_are_sent() {
        let mut watcher = Watcher::default();
        let reading = |probe| MeterReading {
            probe: ProbeId(probe),
            frame: 0,
            frames: 128,
            peak: 0.5,
            rms: 0.3,
        };
        let watch = WatchList {
            meters: HashSet::from([ProbeId(1)]),
            ..WatchList::default()
        };
        let update = watcher.take(&watch, &[reading(1), reading(2)], [], 48_000.0, Instant::now());
        assert_eq!(update.levels.len(), 1);
        assert_eq!(update.levels[0].0, ProbeId(1));
        watcher.sent(&update);
        let again = watcher.take(&watch, &[reading(1), reading(2)], [], 48_000.0, Instant::now());
        assert!(again.is_empty(), "{again:?}");
    }
}
