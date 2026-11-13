//! The telemetry thread: drains the engine's meter and scope queues off the
//! UI thread, and wakes the UI only when something it draws changes
//! (`docs/design/panels.md`, How levels reach a meter, and Drawing the
//! scope trace).
//!
//! Every message to the app redraws the window, so draining telemetry on a
//! UI timer would draw frames even when every meter is silent.

use crate::clip::{PeakNews, Peaks};
use crate::history::LevelHistory;
use crate::live::{Level, Levels, ScopeTrace, Slope, Trigger};
use crate::spectrum::SpectrumAnalyser;
use crate::watcher::Outlet;
use crate::xy::XyTrace;
use loom_core::{MeterReading, PairChunk, ProbeId, ScopeChunk, Telemetry};
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

/// How far a level history's column must move before the UI hears of it:
/// half a dB of the 60 dB the frame shows, which spans 2 in a trace. Every
/// message redraws the window, so an exact comparison would wake it for
/// rounding noise on a steady tone (panel-widgets.md, Level history).
const HISTORY_TOLERANCE: f32 = 2.0 * 0.5 / 60.0;

/// The same for a spectrum: half a dB of the 90 dB its frame shows.
const SPECTRUM_TOLERANCE: f32 = 2.0 * 0.5 / 90.0;

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
    /// Meter probes whose level history is drawn, with the seconds it shows.
    pub histories: HashMap<ProbeId, f32>,
    /// Scope probes whose spectrum is drawn, with how each draws.
    pub spectra: HashMap<ProbeId, SpectrumWant>,
    /// Paired probes whose XY scope is drawn, with how each draws.
    pub xy: HashMap<ProbeId, XyWant>,
    /// Counts the UI's resets of clip lights and peak holds, by meter probe, so
    /// the thread reports that probe's peaks again from there.
    pub peak_resets: HashMap<ProbeId, u64>,
    /// True when scopes draw full traces (close zoom), false for a flat
    /// line at their level (middle zoom).
    pub full_traces: bool,
}

/// What one scope on screen shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScopeWant {
    /// Samples across the frame: the module's `window` at the sample rate.
    pub window: usize,
    /// The level where the trace starts.
    pub trigger: Trigger,
    /// The edge that starts it.
    pub slope: Slope,
}

/// What one spectrum on screen draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpectrumWant {
    /// True to tilt the drawing by +4.5 dB per octave.
    pub tilt: bool,
    /// True to draw the peak hold line.
    pub hold: bool,
}

/// What one XY scope on screen draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XyWant {
    /// Frames of history: the module's `time` at the sample rate.
    pub frames: usize,
    /// The value at each edge of the frame.
    pub range: f32,
    /// True to turn the trace by 45 degrees.
    pub rotate: bool,
}

/// What the thread found that the UI draws.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Update {
    /// Meter levels that changed by at least one of 256 steps.
    pub levels: Vec<(ProbeId, Level)>,
    /// Scope traces that changed: only what is drawn, so a still or silent
    /// signal sends none.
    pub traces: Vec<(ProbeId, loom_canvas::Trace)>,
    /// Level histories that moved: a still or silent one sends none.
    pub histories: Vec<(ProbeId, loom_canvas::Trace)>,
    /// Spectra that moved, apart from `traces`, because a scope and a spectrum
    /// on one output share a probe.
    pub spectra: Vec<(ProbeId, loom_canvas::Trace)>,
    /// XY traces that changed: a still or silent one sends none.
    pub xy: Vec<(ProbeId, loom_canvas::Trace)>,
    /// Raw peaks that could change a clip light or a peak hold.
    pub peaks: Vec<(ProbeId, PeakNews)>,
    /// Activity probes whose state changed: true for carrying.
    pub activity: Vec<(ProbeId, bool)>,
}

impl Update {
    /// True when nothing changed.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
            && self.traces.is_empty()
            && self.histories.is_empty()
            && self.spectra.is_empty()
            && self.xy.is_empty()
            && self.peaks.is_empty()
            && self.activity.is_empty()
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
    shown_levels: HashMap<ProbeId, MeterSteps>,
    scopes: HashMap<ProbeId, ScopeTrace>,
    shown_traces: HashMap<ProbeId, loom_canvas::Trace>,
    histories: HashMap<ProbeId, LevelHistory>,
    shown_histories: HashMap<ProbeId, loom_canvas::Trace>,
    spectra: HashMap<ProbeId, SpectrumAnalyser>,
    shown_spectra: HashMap<ProbeId, loom_canvas::Trace>,
    peaks: Peaks,
    xy: HashMap<ProbeId, XyTrace>,
    shown_xy: HashMap<ProbeId, loom_canvas::Trace>,
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
        self.peaks.note(&watch.peak_resets, &watch.meters, readings);
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
        self.spectra.retain(|probe, _| watch.spectra.contains_key(probe));
        self.shown_spectra.retain(|probe, _| watch.spectra.contains_key(probe));
        for chunk in chunks {
            if watch.scopes.contains_key(&chunk.probe) {
                self.scopes
                    .entry(chunk.probe)
                    .or_insert_with(|| ScopeTrace::new(sample_rate))
                    .push(&chunk);
            }
            if watch.spectra.contains_key(&chunk.probe) {
                self.spectra
                    .entry(chunk.probe)
                    .or_insert_with(|| SpectrumAnalyser::new(sample_rate))
                    .push(&chunk);
            }
        }
        let spectra = watch
            .spectra
            .iter()
            .filter_map(|(probe, want)| {
                let trace = self.spectra.get(probe)?.trace(want.tilt, want.hold);
                moved(self.shown_spectra.get(probe), &trace, SPECTRUM_TOLERANCE).then_some((*probe, trace))
            })
            .collect();
        let mut traces = Vec::new();
        for (probe, want) in &watch.scopes {
            let Some(scope) = self.scopes.get_mut(probe) else {
                continue;
            };
            scope.update(want.window, want.trigger, want.slope);
            let trace = scope.canvas_trace(watch.full_traces);
            if self.shown_traces.get(probe) != Some(&trace) {
                traces.push((*probe, trace));
            }
        }
        let histories = self.take_histories(watch, readings, sample_rate);
        let activity = self.take_activity(watch, readings, now);
        Update {
            levels,
            traces,
            histories,
            spectra,
            xy: Vec::new(),
            peaks: self.peaks.news(),
            activity,
        }
    }

    /// Takes the pair chunks that arrived, and returns the XY traces that
    /// changed. Apart from [`Watcher::take`], because chunks of the third queue
    /// are read only here.
    pub fn take_pairs(
        &mut self,
        watch: &WatchList,
        chunks: impl IntoIterator<Item = PairChunk>,
        sample_rate: f32,
    ) -> Vec<(ProbeId, loom_canvas::Trace)> {
        self.xy.retain(|probe, _| watch.xy.contains_key(probe));
        self.shown_xy.retain(|probe, _| watch.xy.contains_key(probe));
        for chunk in chunks {
            if watch.xy.contains_key(&chunk.probe) {
                self.xy
                    .entry(chunk.probe)
                    .or_insert_with(|| XyTrace::new(sample_rate))
                    .push(&chunk);
            }
        }
        let empty = loom_canvas::Trace::default();
        watch
            .xy
            .iter()
            .filter_map(|(probe, want)| {
                let trace = self
                    .xy
                    .get(probe)?
                    .trace(want.frames, want.range, want.rotate, watch.full_traces);
                (self.shown_xy.get(probe).unwrap_or(&empty) != &trace).then_some((*probe, trace))
            })
            .collect()
    }

    /// Files the readings into the level histories on screen, and returns
    /// those that moved. A history that shows another window starts again.
    fn take_histories(
        &mut self,
        watch: &WatchList,
        readings: &[MeterReading],
        sample_rate: f32,
    ) -> Vec<(ProbeId, loom_canvas::Trace)> {
        self.histories.retain(|probe, _| watch.histories.contains_key(probe));
        self.shown_histories
            .retain(|probe, _| watch.histories.contains_key(probe));
        for (probe, &window) in &watch.histories {
            let fresh = self.histories.get(probe).is_none_or(|h| h.window() != window);
            if fresh {
                self.histories.insert(*probe, LevelHistory::new(window, sample_rate));
            }
        }
        for reading in readings {
            if let Some(history) = self.histories.get_mut(&reading.probe) {
                history.push(reading);
            }
        }
        self.histories
            .iter()
            .filter_map(|(probe, history)| {
                let trace = history.trace();
                let shown = self.shown_histories.get(probe);
                moved(shown, &trace, HISTORY_TOLERANCE).then_some((*probe, trace))
            })
            .collect()
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
                a.peak = a
                    .peak
                    .max(reading.peak.abs())
                    .max(reading.right.map_or(0.0, |(peak, _)| peak.abs()));
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
        for (probe, trace) in &update.histories {
            self.shown_histories.insert(*probe, trace.clone());
        }
        for (probe, trace) in &update.spectra {
            self.shown_spectra.insert(*probe, trace.clone());
        }
        self.peaks.sent(&update.peaks);
        for (probe, trace) in &update.xy {
            self.shown_xy.insert(*probe, trace.clone());
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
    /// builds a new one, and the watcher must restart to take its
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

    /// Takes the pair chunks waiting now, on the caller's thread, for
    /// `--frames` as [`Monitor::take_telemetry`] does.
    pub fn take_pairs(&self) -> Vec<PairChunk> {
        let mut telemetry = self.telemetry.lock().unwrap_or_else(PoisonError::into_inner);
        telemetry.as_mut().map_or_else(Vec::new, |t| t.pairs().collect())
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
    // One monitor per engine: the watcher restarts with a new engine.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-app telemetry monitor".hash(state);
        self.generation.hash(state);
    }
}

/// True when `trace` differs from what the UI has by more than `tolerance`
/// in some point, in a trace's vertical units. Nothing shown yet counts as an
/// empty trace, so a display that stays empty sends nothing.
pub fn moved(shown: Option<&loom_canvas::Trace>, trace: &loom_canvas::Trace, tolerance: f32) -> bool {
    let empty = loom_canvas::Trace::default();
    let shown = shown.unwrap_or(&empty);
    let apart = |a: &[[f32; 2]], b: &[[f32; 2]]| {
        a.len() != b.len() || a.iter().zip(b).any(|(a, b)| (a[1] - b[1]).abs() > tolerance)
    };
    apart(&shown.points, &trace.points) || apart(&shown.second, &trace.second)
}

/// The quantised left and optional right meter levels.
pub type MeterSteps = (u8, u8, Option<(u8, u8)>);

/// A level as drawn: 256 steps for RMS and peak.
fn steps(level: Level) -> MeterSteps {
    let step = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    (
        step(level.rms),
        step(level.peak),
        level.right.map(|(rms, peak)| (step(rms), step(peak))),
    )
}

/// The levels that differ from `shown` by at least one step.
pub fn changed(shown: &HashMap<ProbeId, MeterSteps>, levels: &[(ProbeId, Level)]) -> Vec<(ProbeId, Level)> {
    levels
        .iter()
        .filter(|(probe, level)| shown.get(probe).copied().unwrap_or((0, 0, None)) != steps(*level))
        .copied()
        .collect()
}

/// Drains the queues every [`INTERVAL`], or [`SCOPE_INTERVAL`] while a
/// scope or a spectrum is on screen, and yields what changed on screen. The thread stops
/// when the app closes.
pub fn run(monitor: &Monitor, tx: Outlet<Update>) {
    let telemetry = monitor.telemetry.lock().ok().and_then(|mut t| t.take());
    if let Some(mut telemetry) = telemetry {
        {
            let mut watcher = Watcher::default();
            while !tx.is_closed() {
                let watch = monitor.watched();
                let readings: Vec<MeterReading> = telemetry.meters().collect();
                let mut update = watcher.take(
                    &watch,
                    &readings,
                    telemetry.scope(),
                    monitor.sample_rate,
                    Instant::now(),
                );
                update.xy = watcher.take_pairs(&watch, telemetry.pairs(), monitor.sample_rate);
                if !update.is_empty() {
                    match tx.try_send(update.clone()) {
                        Ok(()) => watcher.sent(&update),
                        // The UI has not taken the last one yet: try again
                        // next time, with fresh values.
                        Err(e) if e.is_full() => {}
                        Err(_) => break,
                    }
                }
                std::thread::sleep(
                    if watch.scopes.is_empty() && watch.spectra.is_empty() && watch.xy.is_empty() {
                        INTERVAL
                    } else {
                        SCOPE_INTERVAL
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
