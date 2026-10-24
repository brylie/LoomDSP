//! Engine telemetry in the app (FR-VIS-04).
//!
//! [`Live`] keeps meter probes in step with the graph and with the meters on
//! screen ([`Live::sync`], called before every commit). [`Levels`] turns the
//! meter readings into falling levels on the telemetry thread (see
//! `monitor.rs`). [`ScopeTrace`] turns the samples of a scope probe into a
//! stable, triggered trace with a fitting vertical range; the scope module
//! does not draw it yet (#62). See `docs/design/telemetry.md` and `docs/design/panels.md`.

// Parts wait for the scope module: only tests call them.
#![cfg_attr(not(test), allow(dead_code))]

use loom_core::{Controller, MeterReading, OutputRef, ProbeId, ProbeKind, ScopeChunk, Telemetry};
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
}

impl Live {
    /// No probes until something asks for them.
    pub fn new() -> Self {
        Self::default()
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

    /// True if any probe is active, so telemetry may arrive.
    pub fn has_probes(&self) -> bool {
        !self.meters.is_empty()
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
        let stale: Vec<OutputRef> = self
            .meters
            .iter()
            .filter(|(output, probe)| !wanted.contains(output) || controller.probe(**probe).is_none())
            .map(|(&output, _)| output)
            .collect();
        for output in stale {
            if let Some(probe) = self.meters.remove(&output) {
                // Already gone if its module was removed.
                let _ = controller.remove_probe(probe);
            }
        }
        for output in wanted {
            if self.meters.contains_key(&output) || controller.graph().node(output.module).is_none() {
                continue;
            }
            if let Ok(probe) = controller.add_probe(output, ProbeKind::Meter) {
                self.meters.insert(output, probe);
            }
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
    pub fn get(&self, probe: ProbeId) -> Option<Level> {
        self.meters.get(&probe).map(|m| m.level)
    }

    /// Every probe's level.
    pub fn all(&self) -> impl Iterator<Item = (ProbeId, Level)> + '_ {
        self.meters.iter().map(|(&p, m)| (p, m.level))
    }

    /// Takes all waiting telemetry and updates the levels. Levels fall with
    /// the time since the last call, so the call rate does not change them.
    /// A probe that has been silent at level 0 for two seconds is forgotten.
    pub fn drain(&mut self, telemetry: &mut Telemetry, now: Instant) {
        let readings: Vec<MeterReading> = telemetry.meters().collect();
        // No scope displays yet. Empty the queue anyway, so a stray chunk
        // can never fill it.
        telemetry.scope().for_each(drop);
        self.update(&readings, now);
    }

    /// Takes meter readings and updates the levels, as [`Levels::drain`].
    pub fn update(&mut self, readings: &[MeterReading], now: Instant) {
        for reading in readings {
            let meter = self.meters.entry(reading.probe).or_default();
            meter.peak = meter.peak.max(reading.peak);
            meter.squares += f64::from(reading.rms).powi(2) * f64::from(reading.frames);
            meter.frames += u64::from(reading.frames);
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
            meter.level = Level { rms, peak };
            meter.peak = 0.0;
            meter.squares = 0.0;
            meter.frames = 0;
        }
        self.meters.retain(|_, m| {
            m.level.peak > 0.0
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
}

/// Peak to level: -60 dB and below is 0, 0 dB (1.0, or 10 V) and above is 1.
fn normalise(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
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
    let arm = level - swing * 0.1;

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
mod tests {
    use super::*;
    use loom_core::{EngineConfig, EngineParts, ModuleId, create_engine};
    use loom_patch::Patch;
    use std::time::Duration;

    /// The sine example: a free-running sine on module 3, silent envelope.
    fn sine_patch() -> EngineParts {
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/patches/sine.ron");
        Patch::load(path).unwrap().load_into(&mut parts.controller).unwrap();
        parts
    }

    fn sine(frequency: f32, sample_rate: f32, len: usize, phase: f32) -> Vec<f32> {
        (0..len)
            .map(|i| (std::f32::consts::TAU * (frequency * i as f32 / sample_rate + phase)).sin())
            .collect()
    }

    #[test]
    fn no_probes_until_something_asks() {
        let mut parts = sine_patch();
        let mut live = Live::new();
        live.sync(&mut parts.controller, &BTreeSet::new());
        assert!(!live.has_probes());
        assert_eq!(parts.controller.probes().count(), 0);
    }

    #[test]
    fn meters_on_screen_get_probes_and_lose_them_off_screen() {
        let mut parts = sine_patch();
        let mut live = Live::new();
        let sine = parts.controller.output_ref(ModuleId(3), "out").unwrap();
        live.sync(&mut parts.controller, &BTreeSet::from([sine]));
        assert!(live.probe(sine).is_some());
        assert_eq!(parts.controller.probes().count(), 1);
        // Off screen: the probe goes.
        live.sync(&mut parts.controller, &BTreeSet::new());
        assert!(live.probe(sine).is_none());
        assert_eq!(parts.controller.probes().count(), 0);
    }

    #[test]
    fn engine_telemetry_reaches_levels_and_traces() {
        let mut parts = sine_patch();
        let mut live = Live::new();
        live.set_activity(true);
        live.sync(&mut parts.controller, &BTreeSet::new());
        // One meter per output with a cable.
        assert_eq!(live.meters.len(), 6);
        let sine = parts.controller.output_ref(ModuleId(3), "out").unwrap();
        let env = parts.controller.output_ref(ModuleId(4), "out").unwrap();
        let probe = parts.controller.add_probe(sine, ProbeKind::Scope).unwrap();
        parts.controller.commit().unwrap();

        let mut trace = ScopeTrace::new(48_000.0);
        let mut levels = Levels::default();
        let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
        let start = Instant::now();
        for i in 0..20 {
            parts.engine.process(&mut left, &mut right);
            for chunk in parts.telemetry.scope() {
                assert_eq!(chunk.probe, probe);
                trace.push(&chunk);
            }
            levels.drain(&mut parts.telemetry, start + Duration::from_millis(80 * i));
        }
        let level = |output| live.probe(output).and_then(|p| levels.get(p)).unwrap_or_default();
        // With no note held, the sine runs at about 8 Hz, at full level.
        assert!(level(sine).peak > 0.9, "the sine is at full level");
        assert!(
            level(sine).rms > 0.8 && level(sine).rms <= level(sine).peak,
            "{:?}",
            level(sine)
        );
        assert_eq!(level(env), Level::default(), "no note, so the envelope is silent");

        trace.update(960, Trigger::Auto);
        assert!(trace.triggered);
        assert_eq!(trace.trace.len(), 961, "20 ms at 48 kHz, plus one sample");
        assert_eq!(trace.range, 1.0);
        assert!(
            trace.trace[0] <= 0.0 && trace.trace[1] >= 0.0,
            "starts on a rising edge"
        );

        // Removing a module drops the meters on its cables.
        parts.controller.remove_module(ModuleId(3)).unwrap();
        live.sync(&mut parts.controller, &BTreeSet::new());
        assert_eq!(live.meters.len(), 4);
        assert_eq!(parts.controller.probes().count(), 4);

        live.set_activity(false);
        live.sync(&mut parts.controller, &BTreeSet::new());
        assert!(!live.has_probes());
        assert_eq!(parts.controller.probes().count(), 0);
    }

    #[test]
    fn flat_trace_runs_free() {
        let mut trace = ScopeTrace::new(48_000.0);
        trace.update(100, Trigger::Auto);
        assert!(trace.trace.is_empty() && !trace.triggered);
        trace.history.push(0, &[0.25; 500]);
        trace.update(100, Trigger::Auto);
        assert!(!trace.triggered);
        assert_eq!(trace.trace.len(), 101);
        assert_eq!(trace.range, 0.5);
    }

    fn reading(probe: u32, frames: u32, peak: f32, rms: f32) -> MeterReading {
        MeterReading {
            probe: ProbeId(probe),
            frame: 0,
            frames,
            peak,
            rms,
        }
    }

    /// Issue #92, as drawn: Level Meters on the subtractive example, one
    /// after the VCA and one on the filter's output. The telemetry thread's
    /// 33 ms clock is simulated, so the test takes no real time.
    #[test]
    fn meters_on_the_subtractive_patch_rise_fall_and_change_colour() {
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0));
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/patches/subtractive.ron");
        Patch::load(path).unwrap().load_into(&mut parts.controller).unwrap();
        let mut level_meter = |module: u32, output: &str| {
            let meter = parts.controller.add_module("loom.display.meter").unwrap();
            parts
                .controller
                .connect_named(ModuleId(module), output, meter, "in")
                .unwrap();
            parts.controller.output_ref(meter, "out").unwrap()
        };
        let vca = level_meter(8, "out");
        let filter = level_meter(4, "lowpass");
        let mut live = Live::new();
        live.sync(&mut parts.controller, &BTreeSet::from([vca, filter]));
        parts.controller.commit().unwrap();

        // 1600 frames is 33.3 ms at 48 kHz: one telemetry tick per buffer.
        // A note is held from 0.5 s to 1.5 s.
        const TICK: usize = 1600;
        let (mut left, mut right) = (vec![0.0; TICK], vec![0.0; TICK]);
        let start = Instant::now();
        let mut levels = Levels::default();
        let mut shown = Vec::new();
        for tick in 0..90 {
            let seconds = (tick * TICK) as f32 / 48_000.0;
            if tick == 15 {
                assert!(parts.midi.send(loom_core::MidiEvent::note_on(0, 0, 57, 100)));
            }
            if tick == 45 {
                assert!(parts.midi.send(loom_core::MidiEvent::note_off(0, 0, 57)));
            }
            parts.engine.process(&mut left, &mut right);
            levels.drain(&mut parts.telemetry, start + Duration::from_secs_f32(seconds));
            let level = |output| live.probe(output).and_then(|p| levels.get(p)).unwrap_or_default();
            shown.push((seconds, level(vca), level(filter)));
        }
        let at = |seconds: f32| *shown.iter().rfind(|(s, _, _)| *s <= seconds).unwrap();

        // Colour bands from panels.md, Meter widget: green below -12 dB,
        // amber up to -3 dB.
        let db = |db: f32| normalise(10f32.powf(db / 20.0));
        let green = |level: f32| level > 0.0 && level < db(-12.0);
        let amber = |level: f32| (db(-12.0)..db(-3.0)).contains(&level);

        // After the VCA: empty before the note, green while it is held,
        // falling after it, and empty again within a second.
        assert_eq!(at(0.45).1, Level::default(), "empty before the note");
        let held = at(1.45).1;
        assert!(green(held.rms), "green while held: {held:?}");
        let falling = at(1.9).1;
        assert!(0.0 < falling.rms && falling.rms < held.rms, "falling: {falling:?}");
        assert_eq!(at(2.6).1, Level::default(), "empty a second after the note");

        // On the filter's output the saw drones with no note, so the meter
        // stays up the whole time, and louder: amber, not green.
        for seconds in [0.45, 1.45, 2.6] {
            let drone = at(seconds).2;
            assert!(amber(drone.rms), "amber at {seconds} s: {drone:?}");
        }
    }

    #[test]
    fn rms_is_taken_over_every_frame_since_the_last_update() {
        let mut levels = Levels::default();
        // One loud block and one silent block of the same length: the RMS
        // of both together is 1 / sqrt(2), -3 dB, not the loudest block.
        levels.update(&[reading(1, 128, 1.0, 1.0), reading(1, 128, 0.0, 0.0)], Instant::now());
        let level = levels.get(ProbeId(1)).unwrap();
        assert!((level.rms - 0.95).abs() < 1e-3, "{level:?}");
        assert_eq!(level.peak, 1.0);
    }

    #[test]
    fn silent_probes_are_forgotten() {
        let start = Instant::now();
        let mut levels = Levels::default();
        levels.update(&[reading(1, 128, 0.5, 0.3)], start);
        // It falls to 0 within a second, then is forgotten two seconds after
        // its last reading.
        for ms in (100..=1_500).step_by(100) {
            levels.update(&[], start + Duration::from_millis(ms));
        }
        assert_eq!(levels.get(ProbeId(1)), Some(Level::default()));
        levels.update(&[], start + Duration::from_millis(2_100));
        assert_eq!(levels.get(ProbeId(1)), None);
    }

    #[test]
    fn levels_map_decibels() {
        assert_eq!(normalise(0.0), 0.0);
        assert_eq!(normalise(1.0), 1.0);
        assert_eq!(normalise(10.0), 1.0);
        assert!((normalise(0.001) - 0.0).abs() < 1e-6);
        assert!((normalise(0.031_622_8) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn levels_rise_at_once_and_fall_slowly() {
        assert_eq!(decay(0.0, 0.8, 0.016), 0.8);
        let fallen = decay(0.8, 0.0, 0.1);
        assert!((fallen - 0.7).abs() < 1e-6);
        assert_eq!(decay(0.0005, 0.0, 0.0), 0.0);
    }

    #[test]
    fn trigger_finds_rising_zero_crossing() {
        // A sine starting at a quarter cycle: first rising crossing at one
        // period, 48 samples in.
        let samples = sine(1000.0, 48_000.0, 4800, 0.25);
        let position = find_trigger(&samples, 480, Trigger::Auto).unwrap();
        let start = position.floor() as usize;
        let fraction = position - start as f32;
        let level = samples[start] + (samples[start + 1] - samples[start]) * fraction;
        assert!(level.abs() < 1e-3, "trigger level {level}");
        assert!(samples[start + 1] > samples[start], "rising edge");
        // The latest edge with a full window after it.
        assert!(position as usize + 480 < samples.len());
        assert!(position as usize + 480 + 48 >= samples.len() - 1);
    }

    #[test]
    fn trigger_is_stable_across_frames() {
        // A period that is not a whole number of samples. Successive frames
        // add 800 samples (60 fps at 48 kHz); the wave under the trigger
        // must start at the same phase every time.
        let all = sine(440.0, 48_000.0, 48_000, 0.1);
        let mut phases = Vec::new();
        for end in (8000..all.len()).step_by(800) {
            let samples = &all[end - 8000..end];
            let position = find_trigger(samples, 960, Trigger::Auto).unwrap();
            let absolute = (end - 8000) as f32 + position;
            phases.push((absolute * 440.0 / 48_000.0 + 0.1).fract());
        }
        for phase in &phases {
            // Phases near 0 and near 1 are the same point of the cycle.
            let d = phase - phases[0];
            assert!((d - d.round()).abs() < 1e-3, "phases {phases:?}");
        }
    }

    #[test]
    fn trigger_works_with_offset() {
        // An envelope-like signal from 2.0 to 3.0.
        let samples: Vec<f32> = sine(100.0, 48_000.0, 4800, 0.0).iter().map(|x| 2.5 + 0.5 * x).collect();
        let position = find_trigger(&samples, 480, Trigger::Auto).unwrap();
        let i = position.floor() as usize;
        let value = samples[i] + (samples[i + 1] - samples[i]) * (position - i as f32);
        assert!((value - 2.5).abs() < 1e-3, "trigger at {value}");
        assert!(samples[i + 1] > samples[i], "rising edge");
    }

    #[test]
    fn trigger_ignores_noise_on_the_edge() {
        // A slow rise through the level with a wiggle just above it: the
        // trigger is the first crossing, not the wiggle.
        let mut samples: Vec<f32> = (0..400).map(|i| -1.0 + i as f32 / 200.0).collect();
        // Around sample 200 the ramp crosses 0. Add a dip back just below 0.
        samples[205] = -0.01;
        samples.extend(std::iter::repeat_n(1.0, 300));
        samples.extend((0..400).map(|i| 1.0 - i as f32 / 200.0));
        let position = find_trigger(&samples, 600, Trigger::Auto).unwrap();
        assert!((199.0..201.0).contains(&position), "position {position}");
    }

    #[test]
    fn trigger_at_a_fixed_level() {
        // A sine from -1 to 1, triggered at 0.5: the trace starts where the
        // rising wave crosses 0.5, not at the midpoint.
        let samples = sine(1000.0, 48_000.0, 4800, 0.0);
        let position = find_trigger(&samples, 480, Trigger::Level(0.5)).unwrap();
        let i = position.floor() as usize;
        let value = samples[i] + (samples[i + 1] - samples[i]) * (position - i as f32);
        assert!((value - 0.5).abs() < 1e-3, "trigger at {value}");
        assert!(samples[i + 1] > samples[i], "rising edge");
    }

    #[test]
    fn fixed_level_out_of_reach_runs_free() {
        // The sine never reaches 2.0, so there is no trigger, and the trace
        // shows the latest window.
        let samples = sine(1000.0, 48_000.0, 4800, 0.0);
        assert_eq!(find_trigger(&samples, 480, Trigger::Level(2.0)), None);
        let mut trace = ScopeTrace::new(48_000.0);
        trace.history.push(0, &samples);
        trace.update(480, Trigger::Level(2.0));
        assert!(!trace.triggered);
        assert_eq!(trace.trace.as_slice(), &samples[samples.len() - 481..]);
    }

    #[test]
    fn trigger_follows_the_scope_parameters() {
        assert_eq!(Trigger::from_params(1.0, 0.3), Trigger::Auto);
        assert_eq!(Trigger::from_params(0.0, 0.3), Trigger::Level(0.3));
    }

    #[test]
    fn flat_signal_does_not_trigger() {
        assert_eq!(find_trigger(&[0.5; 4096], 480, Trigger::Auto), None);
        assert_eq!(find_trigger(&[0.5; 10], 480, Trigger::Auto), None);
    }

    #[test]
    fn history_drops_samples_across_gaps() {
        let mut history = History::new(8);
        history.push(0, &[1.0, 2.0]);
        history.push(2, &[3.0]);
        assert_eq!(history.samples, [1.0, 2.0, 3.0]);
        // Frame 3 was lost.
        history.push(4, &[5.0]);
        assert_eq!(history.samples, [5.0]);
        history.push(5, &[0.0; 10]);
        assert_eq!(history.samples.len(), 8);
    }

    #[test]
    fn range_grows_at_once_and_shrinks_with_margin() {
        assert_eq!(pick_range(1.0, 1.5), 2.0);
        assert_eq!(pick_range(1.0, 0.6), 1.0);
        assert_eq!(pick_range(1.0, 0.3), 0.5);
        assert_eq!(pick_range(1.0, 50.0), 10.0);
    }
}
