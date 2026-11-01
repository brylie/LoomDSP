//! Engine telemetry in the app (FR-VIS-04).
//!
//! [`Live`] keeps meter probes in step with the graph and with the meters on
//! screen ([`Live::sync`], called before every commit). [`Levels`] turns the
//! meter readings into falling levels on the telemetry thread (see
//! `monitor.rs`). [`ScopeTrace`] turns the samples of a scope probe into a
//! stable, triggered trace with a fitting vertical range, and the points the
//! canvas draws in the scope's tile (#62). See `docs/design/telemetry.md`
//! and `docs/design/panels.md`.

#[cfg(test)]
use loom_core::Telemetry;
use loom_core::{Controller, MeterReading, OutputRef, ProbeId, ProbeKind, ScopeChunk};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::Instant;

/// Levels from -60 dB to 0 dB map to 0..1.
const FLOOR_DB: f32 = -60.0;

/// How fast a level falls, in level units per second. Levels rise at once
/// and fall over about a second, so short notes stay visible.
const RELEASE_PER_SECOND: f32 = 1.0;

/// Vertical ranges a scope picks from. Half-steps keep the trace large
/// without the range jumping on every frame.
const RANGES: [f32; 10] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0];

/// Meter probes, kept in step with the graph and with the meters on screen.
/// Lives on the UI thread, beside the `Controller`.
#[derive(Default)]
pub struct Live {
    activity: bool,
    meters: BTreeMap<OutputRef, ProbeId>,
    /// Scope probes, for the scopes on screen.
    scopes: BTreeMap<OutputRef, ProbeId>,
}

impl Live {
    /// No probes until something asks for them, and activity off.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Probes as the app starts them: activity on, the default
    /// (`docs/design/panels.md`, Signal activity on ports).
    pub fn showing_activity() -> Self {
        Self {
            activity: true,
            ..Self::default()
        }
    }

    /// Turns metering of every connected output on or off. Call
    /// [`Live::sync`] and commit after.
    pub fn set_activity(&mut self, on: bool) {
        self.activity = on;
    }

    /// The meter probe on an output, if it has one.
    pub fn probe(&self, output: OutputRef) -> Option<ProbeId> {
        self.meters.get(&output).copied()
    }

    /// The meter probes that show activity: those on outputs with a cable,
    /// while activity is on.
    pub fn activity_probes(&self) -> impl Iterator<Item = ProbeId> + '_ {
        let on = self.activity;
        self.meters.values().copied().filter(move |_| on)
    }

    /// The output a meter probe reads.
    pub fn output_of(&self, probe: ProbeId) -> Option<OutputRef> {
        self.meters.iter().find(|(_, p)| **p == probe).map(|(&o, _)| o)
    }

    /// The scope probe on an output, if it has one.
    pub fn scope_probe(&self, output: OutputRef) -> Option<ProbeId> {
        self.scopes.get(&output).copied()
    }

    /// The probes this `Live` still owns: meters, then scopes.
    pub fn probes(&self) -> impl Iterator<Item = ProbeId> + '_ {
        self.meters.values().chain(self.scopes.values()).copied()
    }

    /// True if any probe is active, so telemetry may arrive.
    #[cfg(test)]
    pub fn has_probes(&self) -> bool {
        !self.meters.is_empty() || !self.scopes.is_empty()
    }

    /// Brings the scope probes in line with the graph and with `wanted`,
    /// the outputs of scopes on screen. Call it before
    /// [`Controller::commit`], as [`Live::sync`].
    pub fn sync_scopes(&mut self, controller: &mut Controller, wanted: &BTreeSet<OutputRef>) {
        keep_probes(&mut self.scopes, controller, wanted, ProbeKind::Scope);
    }

    /// Brings the probes in line with the graph and with `wanted`, the
    /// outputs of meters on screen. Call it before [`Controller::commit`],
    /// so the changes take effect together.
    ///
    /// Probes whose module was removed are forgotten. With activity on,
    /// every output that has a cable gets a meter too.
    pub fn sync(&mut self, controller: &mut Controller, wanted: &BTreeSet<OutputRef>) {
        let mut wanted = wanted.clone();
        if self.activity {
            wanted.extend(controller.graph().cables().map(|(from, _)| from));
        }
        keep_probes(&mut self.meters, controller, &wanted, ProbeKind::Meter);
    }
}

/// Removes the probes in `probes` that are not wanted or whose module is
/// gone, and adds one of `kind` for each wanted output that has none.
fn keep_probes(
    probes: &mut BTreeMap<OutputRef, ProbeId>,
    controller: &mut Controller,
    wanted: &BTreeSet<OutputRef>,
    kind: ProbeKind,
) {
    let stale: Vec<OutputRef> = probes
        .iter()
        .filter(|(output, probe)| !wanted.contains(output) || controller.probe(**probe).is_none())
        .map(|(&output, _)| output)
        .collect();
    for output in stale {
        if let Some(probe) = probes.remove(&output) {
            // Already gone if its module was removed.
            let _ = controller.remove_probe(probe);
        }
    }
    for &output in wanted {
        if probes.contains_key(&output) || controller.graph().node(output.module).is_none() {
            continue;
        }
        if let Ok(probe) = controller.add_probe(output, kind) {
            probes.insert(output, probe);
        }
    }
}

/// The level of one meter probe.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Level {
    /// RMS level, 0..1.
    pub rms: f32,
    /// Peak level, 0..1. Never below `rms`.
    pub peak: f32,
    /// Normalised RMS and peak of the right channel, if stereo.
    pub right: Option<(f32, f32)>,
}

#[derive(Default)]
struct Tracked {
    /// Highest peak since the last drain.
    peak: f32,
    /// Sum of squares and frame count since the last drain. A block can be
    /// shorter than one cycle of the signal, so the RMS is taken over every
    /// frame since the last drain, not as the loudest block.
    squares: f64,
    frames: u64,
    right_peak: f32,
    right_squares: f64,
    right_frames: u64,
    stereo: bool,
    level: Level,
    /// When the last reading arrived.
    heard: Option<Instant>,
}

/// Meter levels from the telemetry queues. Lives on the telemetry thread.
#[derive(Default)]
pub struct Levels {
    meters: HashMap<ProbeId, Tracked>,
    last: Option<Instant>,
}

impl Levels {
    /// The level of a probe, if any reading has arrived for it.
    #[cfg(test)]
    pub fn get(&self, probe: ProbeId) -> Option<Level> {
        self.meters.get(&probe).map(|m| m.level)
    }

    /// Every probe's level.
    pub fn all(&self) -> impl Iterator<Item = (ProbeId, Level)> + '_ {
        self.meters.iter().map(|(&p, m)| (p, m.level))
    }

    /// Takes all waiting telemetry and updates the levels, for tests. The
    /// telemetry thread drains the queues itself (`monitor.rs`).
    #[cfg(test)]
    pub fn drain(&mut self, telemetry: &mut Telemetry, now: Instant) {
        let readings: Vec<MeterReading> = telemetry.meters().collect();
        telemetry.scope().for_each(drop);
        self.update(&readings, now);
    }

    /// Takes meter readings and updates the levels. Levels fall with the
    /// time since the last call, so the call rate does not change them. A
    /// probe that has been silent at level 0 for two seconds is forgotten.
    pub fn update(&mut self, readings: &[MeterReading], now: Instant) {
        for reading in readings {
            let meter = self.meters.entry(reading.probe).or_default();
            meter.peak = meter.peak.max(reading.peak);
            meter.squares += f64::from(reading.rms).powi(2) * f64::from(reading.frames);
            meter.frames += u64::from(reading.frames);
            meter.stereo = reading.right.is_some();
            if let Some((peak, rms)) = reading.right {
                meter.right_peak = meter.right_peak.max(peak);
                meter.right_squares += f64::from(rms).powi(2) * f64::from(reading.frames);
                meter.right_frames += u64::from(reading.frames);
            }
            meter.heard = Some(now);
        }

        let dt = self.last.map_or(0.0, |last| (now - last).as_secs_f32()).min(0.25);
        self.last = Some(now);
        for meter in self.meters.values_mut() {
            let window_rms = if meter.frames > 0 {
                (meter.squares / meter.frames as f64).sqrt() as f32
            } else {
                0.0
            };
            let rms = decay(meter.level.rms, normalise(window_rms), dt);
            let peak = decay(meter.level.peak, normalise(meter.peak), dt).max(rms);
            let right = meter.stereo.then(|| {
                let previous = meter.level.right.unwrap_or_default();
                let window = if meter.right_frames > 0 {
                    (meter.right_squares / meter.right_frames as f64).sqrt() as f32
                } else {
                    0.0
                };
                let rms = decay(previous.0, normalise(window), dt);
                let peak = decay(previous.1, normalise(meter.right_peak), dt).max(rms);
                (rms, peak)
            });
            meter.level = Level { rms, peak, right };
            meter.peak = 0.0;
            meter.squares = 0.0;
            meter.frames = 0;
            meter.right_peak = 0.0;
            meter.right_squares = 0.0;
            meter.right_frames = 0;
        }
        self.meters.retain(|_, m| {
            m.level.peak > 0.0
                || m.level.right.is_some_and(|(_, peak)| peak > 0.0)
                || m.heard
                    .is_some_and(|heard| now - heard < std::time::Duration::from_secs(2))
        });
    }
}

/// Recent samples of one probe, in order, with gaps detected.
struct History {
    samples: VecDeque<f32>,
    capacity: usize,
    /// Engine frame of the next expected sample.
    next: Option<u64>,
}

impl History {
    fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
            next: None,
        }
    }

    /// Appends samples that start at engine frame `frame`. If chunks were
    /// lost in between, the old samples are dropped: splicing across a gap
    /// would draw a step that is not in the signal.
    fn push(&mut self, frame: u64, samples: &[f32]) {
        if self.next.is_some_and(|next| next != frame) {
            self.samples.clear();
        }
        self.next = Some(frame + samples.len() as u64);
        self.samples.extend(samples);
        let excess = self.samples.len().saturating_sub(self.capacity);
        self.samples.drain(..excess);
    }
}

/// Where a scope's trace starts: the level a rising edge must cross
/// (`docs/design/panels.md`, Scope module, Trigger).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trigger {
    /// Halfway between the lowest and highest sample in the history, so
    /// signals with an offset trigger too.
    Auto,
    /// A fixed level. If no edge crosses it, the trace runs free.
    Level(f32),
}

impl Trigger {
    /// The trigger a scope module's `auto` and `level` parameters ask for.
    pub fn from_params(auto: f32, level: f32) -> Self {
        if auto >= 0.5 { Self::Auto } else { Self::Level(level) }
    }
}

/// A triggered oscilloscope trace for one scope probe (FR-VIS-02).
///
/// Push every [`ScopeChunk`] of the probe, then call [`ScopeTrace::update`]
/// once per frame with the window to show and the trigger.
pub struct ScopeTrace {
    history: History,
    /// Samples to draw: one window plus one sample.
    pub trace: Vec<f32>,
    /// How far the trigger lies past `trace[0]`, as a fraction of a sample.
    /// Shift the drawing left by this much, so the wave stays still even when
    /// its period is not a whole number of samples.
    pub offset: f32,
    /// Samples across the full width.
    pub span: f32,
    /// Value at the top edge. The bottom edge is `-range`.
    pub range: f32,
    /// True if the last update found a trigger. Otherwise the trace is the
    /// latest window ("free run").
    pub triggered: bool,
}

impl ScopeTrace {
    /// A trace that keeps one second of history at this sample rate: enough
    /// for long windows and some room to look back for a trigger.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            history: History::new((sample_rate as usize).max(8192)),
            trace: Vec::new(),
            offset: 0.0,
            span: 1.0,
            range: 1.0,
            triggered: false,
        }
    }

    /// Adds a chunk from the probe.
    pub fn push(&mut self, chunk: &ScopeChunk) {
        self.history.push(chunk.frame, chunk.samples());
    }

    /// Fills the trace with `window` samples from the latest trigger.
    pub fn update(&mut self, window: usize, trigger: Trigger) {
        let window = window.max(1);
        let samples = self.history.samples.make_contiguous();
        self.span = window as f32;
        self.trace.clear();
        if samples.is_empty() {
            self.triggered = false;
            return;
        }
        match find_trigger(samples, window, trigger) {
            Some(position) => {
                let start = position.floor() as usize;
                self.offset = position - start as f32;
                self.trace
                    .extend_from_slice(&samples[start..(start + window + 1).min(samples.len())]);
                self.triggered = true;
            }
            None => {
                // Free run: the latest window.
                let start = samples.len().saturating_sub(window + 1);
                self.offset = 0.0;
                self.trace.extend_from_slice(&samples[start..]);
                self.triggered = false;
            }
        }
        let peak = self.trace.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        self.range = pick_range(self.range, peak);
    }

    /// What the canvas draws of the last [`ScopeTrace::update`]
    /// (`docs/design/panels.md`, Drawing the scope trace). With `full`
    /// (close zoom), the points across the frame, shifted left by
    /// [`ScopeTrace::offset`] so a periodic wave holds still. Otherwise
    /// (middle zoom) only the level, for a flat line, in 256 steps so a
    /// level that looks the same is the same.
    ///
    /// More than [`loom_canvas::MAX_TRACE_POINTS`] samples are reduced by
    /// groups, keeping the lowest and highest sample of each in time order,
    /// so a one-sample trigger always shows (ADR-0007).
    pub fn canvas_trace(&self, full: bool) -> loom_canvas::Trace {
        let peak = self.trace.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let level = (normalise(peak) * 255.0).round() / 255.0;
        if !full {
            return loom_canvas::Trace {
                points: Vec::new(),
                level,
            };
        }
        let point = |i: usize| {
            let x = (i as f32 - self.offset) / self.span;
            [x, (self.trace[i] / self.range).clamp(-1.0, 1.0)]
        };
        let n = self.trace.len();
        let max = loom_canvas::MAX_TRACE_POINTS;
        let points = if n <= max {
            (0..n).map(point).collect()
        } else {
            let group = n.div_ceil(max / 2);
            let mut points = Vec::with_capacity(max);
            for start in (0..n).step_by(group) {
                let end = (start + group).min(n);
                let (low, high) = (start..end).fold((start, start), |(lo, hi), i| {
                    let v = self.trace[i];
                    (
                        if v < self.trace[lo] { i } else { lo },
                        if v > self.trace[hi] { i } else { hi },
                    )
                });
                points.push(point(low.min(high)));
                if low != high {
                    points.push(point(low.max(high)));
                }
            }
            points
        };
        loom_canvas::Trace { points, level }
    }
}

/// Peak to level: -60 dB and below is 0, 0 dB (1.0, or 10 V) and above is 1.
pub(crate) fn normalise(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// Level to dB, the inverse of [`normalise`]: 0 is -60 dB, 1 is 0 dB.
pub fn position_db(level: f32) -> f32 {
    FLOOR_DB + level.clamp(0.0, 1.0) * -FLOOR_DB
}

/// Rises at once, falls at [`RELEASE_PER_SECOND`].
fn decay(level: f32, target: f32, dt: f32) -> f32 {
    let fallen = level - RELEASE_PER_SECOND * dt;
    let next = target.max(fallen);
    if next < 0.001 { 0.0 } else { next }
}

/// The latest rising edge through the trigger level that still has a full
/// window after it, as a sample position with a fraction.
///
/// With [`Trigger::Auto`] the level is halfway between the lowest and
/// highest sample in the history, so signals with an offset (envelopes,
/// pitch) trigger too. An edge counts only if the signal was clearly below
/// the level before it (hysteresis of a tenth of the swing in the history),
/// so noise on a slow edge cannot trigger twice. Returns `None` for a flat
/// signal or when no edge is found.
fn find_trigger(samples: &[f32], window: usize, trigger: Trigger) -> Option<f32> {
    let n = samples.len();
    let last_start = n.checked_sub(window + 1)?;
    // Level from the whole history (about a second), so that a slow signal
    // still has a full cycle in it.
    let (low, high) = samples
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    let swing = high - low;
    if swing.is_nan() || swing <= 1e-6 {
        return None;
    }
    let level = match trigger {
        Trigger::Auto => low + swing * 0.5,
        Trigger::Level(level) => level,
    };
    // Hysteresis of a tenth of the swing, but never as deep as the lowest
    // sample: a fixed level near the bottom must still arm.
    let arm = level - (swing * 0.1).min((level - low) * 0.5);

    // Walk back from the newest possible start. A crossing between i - 1 and
    // i is accepted once, walking further back, the signal dips below the
    // arm level. If it rises to the trigger level first, that crossing was
    // a wiggle on the edge, and the search goes on.
    let mut candidate: Option<usize> = None;
    for i in (1..=last_start + 1).rev() {
        let (before, at) = (samples[i - 1], samples[i]);
        if let Some(found) = candidate {
            if at < arm {
                let fraction = (level - samples[found - 1]) / (samples[found] - samples[found - 1]);
                return Some((found - 1) as f32 + fraction);
            }
            if at >= level {
                candidate = None;
            }
        }
        if candidate.is_none() && before < level && at >= level {
            candidate = Some(i);
        }
    }
    None
}

/// The smallest range in [`RANGES`] that fits `peak`. It grows at once but
/// shrinks only when the signal uses less than 40% of the range, so it does
/// not flicker between two steps.
fn pick_range(current: f32, peak: f32) -> f32 {
    let fit = RANGES
        .iter()
        .copied()
        .find(|&r| r >= peak)
        .unwrap_or(RANGES[RANGES.len() - 1]);
    if fit > current || peak < current * 0.4 {
        fit
    } else {
        current
    }
}

#[cfg(test)]
mod tests;
