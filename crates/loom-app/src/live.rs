//! Live views fed by engine telemetry: signal activity on cables and ports
//! (FR-VIS-01) and oscilloscopes on probed outputs (FR-VIS-02).
//!
//! The app keeps probes in step with the graph: a meter on every output that
//! has a cable while activity is on, and a scope on each output the user
//! clicked. Once per UI frame it drains the telemetry queues, turns meter
//! readings into levels for the canvas, and finds a trigger point in each
//! scope's recent samples. See `docs/design/telemetry.md`.

use iced::Color;
use loom_canvas::{PortLevels, PortRef};
use loom_core::{Controller, LoomError, OutputRef, ProbeId, ProbeKind, Telemetry};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::Instant;

/// At most this many scopes are open. Opening another closes the oldest.
/// Four is the number the canvas must hold 60 fps with (ADR-0010).
pub const MAX_SCOPES: usize = 4;

/// Time windows a scope can show, in seconds.
pub const WINDOWS: [f32; 4] = [0.005, 0.02, 0.1, 0.5];

/// Window a new scope starts with: 20 ms shows a few cycles of a bass note.
const DEFAULT_WINDOW: usize = 1;

/// Levels from -60 dB to 0 dB (1.0, or 10 V) map to 0..1 on the canvas.
const FLOOR_DB: f32 = -60.0;

/// How fast a level falls, in canvas units per second. The meters rise at
/// once and fall over about a second, so short notes stay visible.
const RELEASE_PER_SECOND: f32 = 1.0;

/// Smallest change in a level that is worth a redraw.
const LEVEL_EPSILON: f32 = 0.002;

/// Vertical ranges a scope picks from. Half-steps keep the trace large
/// without the range jumping on every frame.
const RANGES: [f32; 10] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0];

/// A meter on one output, for signal activity.
struct Meter {
    probe: ProbeId,
    /// Highest peak since the last frame.
    peak: f32,
    /// Displayed level, 0..1.
    level: f32,
}

/// One open oscilloscope.
pub struct ScopeView {
    /// The output being watched.
    pub output: OutputRef,
    probe: ProbeId,
    /// "Module #id: port".
    pub title: String,
    /// The port's signal colour.
    pub colour: Color,
    /// Index into [`WINDOWS`].
    pub window: usize,
    history: History,
    /// Samples to draw: one window plus one sample.
    pub trace: Vec<f32>,
    /// How far the trigger lies past `trace[0]`, as a fraction of a sample.
    pub offset: f32,
    /// Samples across the full width.
    pub span: f32,
    /// Value at the top edge.
    pub range: f32,
    /// True if the last frame found a trigger. Otherwise the scope shows the
    /// latest samples as they come ("free run").
    pub triggered: bool,
}

impl ScopeView {
    /// Label for the time window, such as "20 ms".
    pub fn window_label(&self) -> String {
        format!("{} ms", WINDOWS[self.window] * 1000.0)
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

/// Telemetry state for the app.
pub struct Live {
    sample_rate: f32,
    activity: bool,
    meters: BTreeMap<OutputRef, Meter>,
    by_probe: HashMap<ProbeId, OutputRef>,
    scopes: Vec<ScopeView>,
    /// Ports with an open scope, for the canvas.
    scope_ports: Vec<PortRef>,
    levels: PortLevels,
    /// True if the last frame changed something on screen.
    changing: bool,
    last: Option<Instant>,
}

fn port_ref(output: OutputRef) -> PortRef {
    PortRef {
        module: output.module.0,
        output: true,
        index: output.port,
    }
}

impl Live {
    /// Live views for an engine at this sample rate. Activity starts on.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            activity: true,
            meters: BTreeMap::new(),
            by_probe: HashMap::new(),
            scopes: Vec::new(),
            scope_ports: Vec::new(),
            levels: PortLevels::default(),
            changing: false,
            last: None,
        }
    }

    /// True if cables and ports show signal activity.
    pub fn activity(&self) -> bool {
        self.activity
    }

    /// Turns signal activity on or off. Call [`Live::sync`] and commit after.
    pub fn set_activity(&mut self, on: bool) {
        self.activity = on;
    }

    /// Levels for the canvas.
    pub fn levels(&self) -> &PortLevels {
        &self.levels
    }

    /// Open scopes, oldest first.
    pub fn scopes(&self) -> &[ScopeView] {
        &self.scopes
    }

    /// Output ports with an open scope.
    pub fn scope_ports(&self) -> &[PortRef] {
        &self.scope_ports
    }

    /// True if the views move: a scope is open or a level is changing. The
    /// app then redraws every frame.
    pub fn animating(&self) -> bool {
        !self.scopes.is_empty() || self.changing
    }

    /// True if any probe is active, so telemetry may arrive.
    pub fn has_probes(&self) -> bool {
        !self.by_probe.is_empty()
    }

    /// Brings the probes in line with the graph and the open views. Call it
    /// before [`Controller::commit`], so the changes take effect together.
    ///
    /// Probes whose module was removed are forgotten. With activity on,
    /// every output that has a cable gets a meter.
    pub fn sync(&mut self, controller: &mut Controller) {
        let wanted: BTreeSet<OutputRef> = if self.activity {
            controller.graph().cables().map(|(from, _)| from).collect()
        } else {
            BTreeSet::new()
        };
        let stale: Vec<OutputRef> = self
            .meters
            .iter()
            .filter(|(output, meter)| !wanted.contains(output) || controller.probe(meter.probe).is_none())
            .map(|(&output, _)| output)
            .collect();
        for output in stale {
            if let Some(meter) = self.meters.remove(&output) {
                // Already gone if its module was removed.
                let _ = controller.remove_probe(meter.probe);
                self.by_probe.remove(&meter.probe);
                self.levels.set(output.module.0, output.port, 0.0);
            }
        }
        for output in wanted {
            if self.meters.contains_key(&output) {
                continue;
            }
            if let Ok(probe) = controller.add_probe(output, ProbeKind::Meter) {
                self.by_probe.insert(probe, output);
                self.meters.insert(
                    output,
                    Meter {
                        probe,
                        peak: 0.0,
                        level: 0.0,
                    },
                );
            }
        }

        let mut i = 0;
        while i < self.scopes.len() {
            if controller.probe(self.scopes[i].probe).is_none() {
                self.forget_scope(i);
            } else {
                i += 1;
            }
        }
    }

    /// Opens a scope on an output. Returns false if one is already open
    /// there. Call [`Controller::commit`] after.
    pub fn open_scope(
        &mut self,
        controller: &mut Controller,
        output: OutputRef,
        title: String,
        colour: Color,
    ) -> Result<bool, LoomError> {
        if self.scopes.iter().any(|s| s.output == output) {
            return Ok(false);
        }
        let probe = controller.add_probe(output, ProbeKind::Scope)?;
        if self.scopes.len() >= MAX_SCOPES {
            self.close_scope(controller, 0);
        }
        self.by_probe.insert(probe, output);
        // One second of history: enough for the longest window and some
        // room to look back for a trigger.
        let capacity = (self.sample_rate as usize).max(8192);
        self.scopes.push(ScopeView {
            output,
            probe,
            title,
            colour,
            window: DEFAULT_WINDOW,
            history: History::new(capacity),
            trace: Vec::new(),
            offset: 0.0,
            span: 1.0,
            range: 1.0,
            triggered: false,
        });
        self.scope_ports.push(port_ref(output));
        Ok(true)
    }

    /// Closes a scope by position. Call [`Controller::commit`] after.
    pub fn close_scope(&mut self, controller: &mut Controller, index: usize) {
        if let Some(scope) = self.scopes.get(index) {
            let _ = controller.remove_probe(scope.probe);
            self.forget_scope(index);
        }
    }

    fn forget_scope(&mut self, index: usize) {
        let scope = self.scopes.remove(index);
        self.by_probe.remove(&scope.probe);
        self.scope_ports.retain(|&p| p != port_ref(scope.output));
    }

    /// Steps a scope to its next time window.
    pub fn next_window(&mut self, index: usize) {
        if let Some(scope) = self.scopes.get_mut(index) {
            scope.window = (scope.window + 1) % WINDOWS.len();
        }
    }

    /// Takes all waiting telemetry and updates the views. Call once per UI
    /// frame.
    pub fn drain(&mut self, telemetry: &mut Telemetry, now: Instant) {
        for reading in telemetry.meters() {
            if let Some(meter) = self.by_probe.get(&reading.probe).and_then(|o| self.meters.get_mut(o)) {
                meter.peak = meter.peak.max(reading.peak);
            }
        }
        for chunk in telemetry.scope() {
            if let Some(scope) = self.scopes.iter_mut().find(|s| s.probe == chunk.probe) {
                scope.history.push(chunk.frame, chunk.samples());
            }
        }

        let dt = self.last.map_or(0.0, |last| (now - last).as_secs_f32()).min(0.25);
        self.last = Some(now);
        self.changing = false;
        for (output, meter) in &mut self.meters {
            let level = decay(meter.level, normalise(meter.peak), dt);
            meter.peak = 0.0;
            if (level - meter.level).abs() > LEVEL_EPSILON || (level == 0.0 && meter.level > 0.0) {
                self.changing = true;
            }
            meter.level = level;
            self.levels.set(output.module.0, output.port, level);
        }

        for scope in &mut self.scopes {
            let window = (WINDOWS[scope.window] * self.sample_rate).round().max(2.0) as usize;
            update_trace(scope, window);
        }
    }
}

/// Peak to canvas level: -60 dB and below is 0, 0 dB (1.0) and above is 1.
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

/// Fills the scope's trace from its history.
fn update_trace(scope: &mut ScopeView, window: usize) {
    let samples = scope.history.samples.make_contiguous();
    scope.span = window as f32;
    scope.trace.clear();
    if samples.is_empty() {
        scope.triggered = false;
        return;
    }
    match find_trigger(samples, window) {
        Some(position) => {
            let start = position.floor() as usize;
            scope.offset = position - start as f32;
            scope
                .trace
                .extend_from_slice(&samples[start..(start + window + 1).min(samples.len())]);
            scope.triggered = true;
        }
        None => {
            // Free run: the latest window.
            let start = samples.len().saturating_sub(window + 1);
            scope.offset = 0.0;
            scope.trace.extend_from_slice(&samples[start..]);
            scope.triggered = false;
        }
    }
    let peak = scope.trace.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    scope.range = pick_range(scope.range, peak);
}

/// The latest rising edge through the middle of the signal that still has a
/// full window after it, as a sample position with a fraction.
///
/// The trigger level is halfway between the lowest and highest sample in
/// the history, so signals with an offset (envelopes, pitch) trigger too. An edge
/// counts only if the signal was clearly below the level before it
/// (hysteresis of a tenth of the swing), so noise on a slow edge cannot
/// trigger twice. Returns `None` for a flat signal or when no edge is found.
pub fn find_trigger(samples: &[f32], window: usize) -> Option<f32> {
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
    let level = low + swing * 0.5;
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

    #[test]
    fn engine_telemetry_reaches_levels_and_scopes() {
        let mut parts = sine_patch();
        let mut live = Live::new(48_000.0);
        live.sync(&mut parts.controller);
        // One meter per output with a cable.
        assert_eq!(live.meters.len(), 6);
        let sine = parts.controller.output_ref(ModuleId(3), "out").unwrap();
        assert!(
            live.open_scope(&mut parts.controller, sine, "Sine".into(), Color::WHITE)
                .unwrap()
        );
        assert!(
            !live
                .open_scope(&mut parts.controller, sine, "Sine".into(), Color::WHITE)
                .unwrap()
        );
        assert_eq!(live.scope_ports().len(), 1);
        parts.controller.commit().unwrap();

        let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
        let start = Instant::now();
        for i in 0..20 {
            parts.engine.process(&mut left, &mut right);
            live.drain(&mut parts.telemetry, start + Duration::from_millis(16 * i));
        }
        assert!(live.levels().get(3, 0) > 0.9, "the sine is at full level");
        assert_eq!(live.levels().get(4, 0), 0.0, "no note, so the envelope is silent");
        let scope = &live.scopes()[0];
        assert!(scope.triggered);
        assert_eq!(scope.trace.len(), 961, "20 ms at 48 kHz, plus one sample");
        assert_eq!(scope.range, 1.0);
        assert!(
            scope.trace[0] <= 0.0 && scope.trace[1] >= 0.0,
            "starts on a rising edge: {:?} offset {}",
            &scope.trace[..3],
            scope.offset
        );

        // Removing the module drops its scope and the meters on its cables.
        parts.controller.remove_module(ModuleId(3)).unwrap();
        live.sync(&mut parts.controller);
        assert!(live.scopes().is_empty());
        assert!(live.scope_ports().is_empty());
        assert_eq!(live.meters.len(), 4);
        assert_eq!(parts.controller.probes().count(), 4);

        live.set_activity(false);
        live.sync(&mut parts.controller);
        assert!(!live.has_probes());
        assert_eq!(parts.controller.probes().count(), 0);
    }

    #[test]
    fn opening_too_many_scopes_closes_the_oldest() {
        let mut parts = sine_patch();
        let mut live = Live::new(48_000.0);
        let outputs: Vec<OutputRef> = [(1, "pitch"), (1, "gate"), (2, "phase"), (3, "out"), (4, "out")]
            .into_iter()
            .map(|(id, port)| parts.controller.output_ref(ModuleId(id), port).unwrap())
            .collect();
        for &output in &outputs {
            live.open_scope(&mut parts.controller, output, String::new(), Color::WHITE)
                .unwrap();
        }
        assert_eq!(live.scopes().len(), MAX_SCOPES);
        assert_eq!(live.scopes()[0].output, outputs[1]);
        assert_eq!(parts.controller.probes().count(), MAX_SCOPES);
        live.close_scope(&mut parts.controller, 0);
        assert_eq!(parts.controller.probes().count(), MAX_SCOPES - 1);
        assert_eq!(live.scope_ports().len(), MAX_SCOPES - 1);
    }

    fn sine(frequency: f32, sample_rate: f32, len: usize, phase: f32) -> Vec<f32> {
        (0..len)
            .map(|i| (std::f32::consts::TAU * (frequency * i as f32 / sample_rate + phase)).sin())
            .collect()
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
        let position = find_trigger(&samples, 480).unwrap();
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
            let position = find_trigger(samples, 960).unwrap();
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
        let position = find_trigger(&samples, 480).unwrap();
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
        let position = find_trigger(&samples, 600).unwrap();
        assert!((199.0..201.0).contains(&position), "position {position}");
    }

    #[test]
    fn flat_signal_does_not_trigger() {
        assert_eq!(find_trigger(&[0.5; 4096], 480), None);
        assert_eq!(find_trigger(&[0.5; 10], 480), None);
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
