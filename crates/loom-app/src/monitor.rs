//! The telemetry thread: drains the engine's meter and scope queues off the
//! UI thread, and wakes the UI only when something it draws changes
//! (`docs/design/panels.md`, How levels reach a meter, and Drawing the
//! scope trace).
//!
//! Every message to the app redraws the window, so draining telemetry on a
//! UI timer would draw frames even when every meter is silent.

use crate::clip::{MeterWidget, PeakNews, Peaks};
use crate::history::LevelHistory;
use crate::live::{Level, Levels, ScopeTrace, Slope, Trigger};
use crate::spectrum::SpectrumAnalyser;
use crate::watcher::{Outlet, Watched};
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
    /// Each meter on screen, by module and widget, with the probe on its
    /// output and its own count of the UI's resets of its clip light and
    /// peak hold, so the thread reports that meter's peaks again from there.
    /// Two meters on one output share the probe and keep their own counts.
    pub peak_resets: HashMap<MeterWidget, (ProbeId, u64)>,
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
    /// The scope's size as drawn, in physical pixels, so a trace is sent
    /// only when its line moves by more than half a pixel
    /// (`docs/design/panels.md`, Drawing the scope trace, The drawn size).
    /// `None` until the app knows it: then any change is sent. Two scopes
    /// on one probe share one want, with each side the larger of the two.
    pub drawn: Option<DrawnSize>,
}

/// A display's size on screen in physical pixels: whole pixels, each side
/// at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawnSize {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

impl DrawnSize {
    /// A size from a width and height in physical pixels, each rounded to a
    /// whole pixel and at least 1. Rounding keeps a small zoom step from
    /// changing the watch list when no side moved by a pixel.
    pub fn new(width: f32, height: f32) -> Self {
        let side = |x: f32| if x.is_finite() { x.round().max(1.0) as u32 } else { 1 };
        Self {
            width: side(width),
            height: side(height),
        }
    }

    /// Each side the larger of the two, for two scopes on one probe.
    pub fn max(self, other: Self) -> Self {
        Self {
            width: self.width.max(other.width),
            height: self.height.max(other.height),
        }
    }
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
    /// Scope traces whose line moved by more than half a pixel as drawn, or
    /// whose level changed at the middle zoom ([`scope_moved`]): a still or
    /// silent signal, or a steady smooth wave, sends none.
    pub traces: Vec<(ProbeId, loom_canvas::Trace)>,
    /// Level histories that moved: a still or silent one sends none.
    pub histories: Vec<(ProbeId, loom_canvas::Trace)>,
    /// Spectra that moved, apart from `traces`, because a scope and a spectrum
    /// on one output share a probe.
    pub spectra: Vec<(ProbeId, loom_canvas::Trace)>,
    /// XY traces that changed: a still or silent one sends none.
    pub xy: Vec<(ProbeId, loom_canvas::Trace)>,
    /// Raw peaks that could change a clip light or a peak hold, by meter.
    pub peaks: Vec<(MeterWidget, PeakNews)>,
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
        self.peaks.note(&watch.peak_resets, readings);
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
            if scope_moved(self.shown_traces.get(probe), &trace, want.drawn) {
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
    /// What the UI draws, which the thread waits on, so a change drains
    /// early.
    watch: Watched<WatchList>,
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
            watch: Watched::default(),
            sample_rate,
            generation,
        }
    }

    /// Tells the thread what the UI draws now. A list that differs wakes it,
    /// so it drains at once; an equal one changes nothing.
    pub fn watch(&self, list: WatchList) {
        self.watch.set(list);
    }

    /// What the UI draws, as last told.
    pub fn watched(&self) -> WatchList {
        self.watch.get().0
    }

    /// The watch list the thread waits on.
    pub(crate) fn list(&self) -> &Watched<WatchList> {
        &self.watch
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
    /// `lights::OnScreen::read` reads the lights. For tests, which have no
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

/// How far a scope's line must move before the UI hears of it: half a
/// pixel as drawn, which nobody can see.
const SCOPE_TOLERANCE: f32 = 0.5;

/// The most segments of the other line tried for one point of a scope
/// trace, the window included. A point that reaches it counts as moved, so
/// the check never stalls the telemetry thread, which also carries the meters
/// and the clip news (`docs/design/panels.md`, Drawing the scope trace,
/// Bounded cost).
pub const SCOPE_SEGMENT_CAP: usize = 64;

/// True when a scope's `trace` differs on screen from what the UI has
/// (`docs/design/panels.md`, Drawing the scope trace, What counts as moved).
///
/// Nothing shown yet counts as moved. With no drawn size known, any change
/// counts. When both traces have no points (the middle zoom, where only the
/// level is drawn), any change of the level's 256 steps counts. When one has
/// points and the other has none, it moved. Otherwise only the points are
/// compared, so a level change alone sends nothing: a point of either trace
/// inside the frame (x from 0 to 1) that lies more than half a pixel, at the
/// drawn size, from the other trace's whole line has moved. Points outside
/// the frame still shape the line. A scope trace has no `second` line.
pub fn scope_moved(shown: Option<&loom_canvas::Trace>, trace: &loom_canvas::Trace, drawn: Option<DrawnSize>) -> bool {
    let Some(shown) = shown else {
        return true;
    };
    let Some(size) = drawn else {
        return shown != trace;
    };
    match (shown.points.is_empty(), trace.points.is_empty()) {
        (true, true) => shown.level != trace.level,
        (false, false) => line_check(&shown.points, &trace.points, size).moved,
        _ => true,
    }
}

/// What a check of two scope lines found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCheck {
    /// True when a point lies more than half a pixel from the other line,
    /// or reached [`SCOPE_SEGMENT_CAP`].
    pub moved: bool,
    /// The segment tests it took. The check stops at the first point that
    /// moved.
    pub tests: usize,
}

/// Checks two scope lines against each other both ways, at `size`: first
/// the points of `a` against the line of `b`, then the other way round. Both
/// must run left to right in x and have only finite points, as
/// [`ScopeTrace::canvas_trace`] gives them.
pub fn line_check(a: &[[f32; 2]], b: &[[f32; 2]], size: DrawnSize) -> LineCheck {
    // A NaN would never lie near any line, so every read would count as
    // moved. `ScopeTrace::canvas_trace` gives only finite points.
    debug_assert!(
        a.iter().chain(b).flatten().all(|v| v.is_finite()),
        "a scope trace has only finite points"
    );
    let mut tests = 0;
    let moved = points_off_line(a, b, size, &mut tests) || points_off_line(b, a, size, &mut tests);
    LineCheck { moved, tests }
}

/// True when a point of `points` inside the frame lies more than
/// [`SCOPE_TOLERANCE`] pixels from the line through `line`, or reaches the
/// cap. Adds each segment test to `tests`.
///
/// For each point the search keeps a window on the line: the segments whose
/// x comes within the tolerance of the point's. Both lines run left to right,
/// so the window only moves forward, and the whole check is close to linear.
/// Inside the window it tries the segment under the point first, then its
/// neighbours outward, since on a line that held still the nearest segment is
/// usually there.
fn points_off_line(points: &[[f32; 2]], line: &[[f32; 2]], size: DrawnSize, tests: &mut usize) -> bool {
    if line.is_empty() {
        return points.iter().any(|p| (0.0..=1.0).contains(&p[0]));
    }
    let (width, height) = (size.width as f32, size.height as f32);
    // x from 0 to 1 across the width, y from -1 to 1 up the height.
    let pixels = |p: [f32; 2]| [p[0] * width, p[1] * height * 0.5];
    let last = line.len() - 1;
    // Segment j joins point j and point j + 1. A line of one point is one
    // segment of no length.
    let segments = last.max(1);
    let segment = |j: usize| (pixels(line[j]), pixels(line[(j + 1).min(last)]));
    // The first segment whose right end reaches x - tolerance, the first
    // whose right end reaches x, and the first whose left end lies past
    // x + tolerance.
    let (mut low, mut under, mut high) = (0, 0, 0);
    for &point in points {
        if !(0.0..=1.0).contains(&point[0]) {
            continue;
        }
        let p = pixels(point);
        while low < segments && segment(low).1[0] < p[0] - SCOPE_TOLERANCE {
            low += 1;
        }
        under = under.max(low);
        while under < segments && segment(under).1[0] < p[0] {
            under += 1;
        }
        high = high.max(under);
        while high < segments && segment(high).0[0] <= p[0] + SCOPE_TOLERANCE {
            high += 1;
        }
        if low >= high {
            // No part of the line comes near this point's x.
            return true;
        }
        let start = under.clamp(low, high - 1);
        let mut tried = 0;
        let mut near = false;
        'search: for step in 0.. {
            let above = start + step;
            let below = start.checked_sub(step).filter(|&j| step > 0 && j >= low);
            if above >= high && below.is_none() {
                break;
            }
            for j in [Some(above).filter(|&j| j < high), below].into_iter().flatten() {
                if tried == SCOPE_SEGMENT_CAP {
                    break 'search;
                }
                tried += 1;
                *tests += 1;
                let (from, to) = segment(j);
                if distance_to_segment(p, from, to) <= SCOPE_TOLERANCE {
                    near = true;
                    break 'search;
                }
            }
        }
        if !near {
            return true;
        }
    }
    false
}

/// The distance in pixels from `p` to the segment from `a` to `b`.
fn distance_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
    (x * x + y * y).sqrt()
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
/// scope, a spectrum, or an XY scope is on screen, and yields what changed on
/// screen. It waits on the watch list for up to that interval, so a change
/// to what the UI draws drains at once. Each turn goes: wait, check closed,
/// read the list, drain, compare, and send. The thread stops when the
/// handle drops.
pub fn run(monitor: &Monitor, tx: Outlet<Update>) {
    let telemetry = monitor.telemetry.lock().ok().and_then(|mut t| t.take());
    let Some(mut telemetry) = telemetry else {
        return;
    };
    let mut watcher = Watcher::default();
    // Nothing seen yet, so the first wait returns at once.
    let mut seen = None;
    let mut interval = INTERVAL;
    loop {
        tx.wait(&monitor.watch, seen, interval);
        if tx.is_closed() {
            break;
        }
        let (watch, version) = monitor.watch.get();
        seen = Some(version);
        interval = if watch.scopes.is_empty() && watch.spectra.is_empty() && watch.xy.is_empty() {
            INTERVAL
        } else {
            SCOPE_INTERVAL
        };
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
                // The UI has not taken the last one yet: try again next
                // time, with fresh values.
                Err(e) if e.is_full() => {}
                Err(_) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests;
