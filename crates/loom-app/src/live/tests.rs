use super::*;
use loom_core::{EngineConfig, EngineParts, ModuleId, create_engine};
use std::time::Duration;

/// The sine example: a free-running sine on module 3, silent envelope.
fn sine_patch() -> EngineParts {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/patches/sine.ron");
    crate::patch_file::load(path)
        .unwrap()
        .load_into(&mut parts.controller)
        .unwrap();
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
    assert_eq!(live.meters.len(), 5);
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

    trace.update(960, Trigger::Auto, Slope::Rising);
    assert!(trace.triggered);
    assert_eq!(
        trace.trace.len(),
        962,
        "20 ms at 48 kHz, plus two samples so the line reaches past both edges"
    );
    assert_eq!(trace.range, 1.0);
    assert!(
        trace.trace[0] <= 0.0 && trace.trace[1] >= 0.0,
        "starts on a rising edge"
    );

    // Removing a module drops the meters on its cables.
    parts.controller.remove_module(ModuleId(3)).unwrap();
    live.sync(&mut parts.controller, &BTreeSet::new());
    assert_eq!(live.meters.len(), 3);
    assert_eq!(parts.controller.probes().count(), 3);

    live.set_activity(false);
    live.sync(&mut parts.controller, &BTreeSet::new());
    assert!(!live.has_probes());
    assert_eq!(parts.controller.probes().count(), 0);
}

#[test]
fn flat_trace_runs_free() {
    let mut trace = ScopeTrace::new(48_000.0);
    trace.update(100, Trigger::Auto, Slope::Rising);
    assert!(trace.trace.is_empty() && !trace.triggered);
    trace.history.push(0, &[0.25; 500]);
    trace.update(100, Trigger::Auto, Slope::Rising);
    assert!(!trace.triggered);
    assert_eq!(trace.trace.len(), 101);
    assert_eq!(trace.range, 0.5);
}

/// A long window keeps a one-sample peak when reduced for the GPU.
#[test]
fn long_scope_window_keeps_short_peak() {
    let mut scope = ScopeTrace::new(48_000.0);
    scope.trace = vec![0.0; loom_canvas::MAX_TRACE_POINTS * 4];
    scope.trace[2_003] = 1.0;
    scope.span = scope.trace.len() as f32;
    let trace = scope.canvas_trace(true);
    assert!(trace.points.len() <= loom_canvas::MAX_TRACE_POINTS);
    assert!(trace.points.iter().any(|point| point[1] == 1.0));
    assert!(trace.points.windows(2).all(|pair| pair[0][0] <= pair[1][0]));
}

/// A reduced triggered trace still reaches past both edges of the frame
/// (`docs/design/panels.md`, Drawing the scope trace, The line reaches past
/// both edges). The window is a step: the sample before the trigger, a dip
/// in the first group that is lower still, then a constant level, so the
/// first group's lowest and highest lie inside the frame, and so does the
/// constant last group's only point. The first and last samples are kept,
/// within the point limit.
#[test]
fn a_reduced_trace_reaches_both_edges() {
    let mut scope = ScopeTrace::new(48_000.0);
    // One sample past a whole number of groups, so the constant last group
    // holds two samples and its one point lies inside the frame.
    let n = loom_canvas::MAX_TRACE_POINTS * 4 + 1;
    scope.trace = vec![1.0; n];
    scope.trace[0] = -0.5;
    scope.trace[3] = -1.0;
    scope.offset = 0.5;
    scope.span = (n - 2) as f32;
    let trace = scope.canvas_trace(true);
    assert!(trace.points.len() <= loom_canvas::MAX_TRACE_POINTS);
    assert!(trace.points[0][0] <= 0.0, "first x: {}", trace.points[0][0]);
    let last = trace.points.last().unwrap();
    assert!(last[0] >= 1.0, "last x: {}", last[0]);
    assert!(trace.points.windows(2).all(|pair| pair[0][0] < pair[1][0]));
    assert!(trace.points.iter().any(|p| p[1] == -1.0), "the dip still shows");

    // A NaN at either end is skipped: the next sample with a value is kept.
    scope.trace[0] = f32::NAN;
    scope.trace[n - 1] = f32::NAN;
    let trace = scope.canvas_trace(true);
    assert!(all_finite(&trace));
    assert_eq!(trace.points[0][0], 0.5 / scope.span, "the first value");
    assert_eq!(
        trace.points.last().unwrap()[0],
        (n as f32 - 2.5) / scope.span,
        "the last value"
    );
}

fn reading(probe: u32, frames: u32, peak: f32, rms: f32) -> MeterReading {
    MeterReading {
        probe: ProbeId(probe),
        frame: 0,
        frames,
        peak,
        rms,
        right: None,
    }
}

/// Issue #92, as drawn: Level Meters on the subtractive example, one
/// after the VCA and one on the filter's output. The telemetry thread's
/// 33 ms clock is simulated, so the test takes no real time.
#[test]
fn meters_on_the_subtractive_patch_rise_fall_and_change_colour() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0));
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/patches/subtractive.ron");
    crate::patch_file::load(path)
        .unwrap()
        .load_into(&mut parts.controller)
        .unwrap();
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
    let position = find_trigger(&samples, 480, Trigger::Auto, Slope::Rising).unwrap();
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
        let position = find_trigger(samples, 960, Trigger::Auto, Slope::Rising).unwrap();
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
    let position = find_trigger(&samples, 480, Trigger::Auto, Slope::Rising).unwrap();
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
    let position = find_trigger(&samples, 600, Trigger::Auto, Slope::Rising).unwrap();
    assert!((199.0..201.0).contains(&position), "position {position}");
}

#[test]
fn trigger_at_a_fixed_level() {
    // A sine from -1 to 1, triggered at 0.5: the trace starts where the
    // rising wave crosses 0.5, not at the midpoint.
    let samples = sine(1000.0, 48_000.0, 4800, 0.0);
    let position = find_trigger(&samples, 480, Trigger::Level(0.5), Slope::Rising).unwrap();
    let i = position.floor() as usize;
    let value = samples[i] + (samples[i + 1] - samples[i]) * (position - i as f32);
    assert!((value - 0.5).abs() < 1e-3, "trigger at {value}");
    assert!(samples[i + 1] > samples[i], "rising edge");
}

#[test]
fn fixed_level_near_the_bottom_still_triggers() {
    // At -0.95 a tenth of the swing would arm at -1.15, below the
    // lowest sample, so no edge would ever count.
    let samples = sine(1000.0, 48_000.0, 4800, 0.0);
    let position = find_trigger(&samples, 480, Trigger::Level(-0.95), Slope::Rising).unwrap();
    let i = position.floor() as usize;
    let value = samples[i] + (samples[i + 1] - samples[i]) * (position - i as f32);
    assert!((value + 0.95).abs() < 1e-3, "trigger at {value}");
    assert!(samples[i + 1] > samples[i], "rising edge");
}

#[test]
fn fixed_level_out_of_reach_runs_free() {
    // The sine never reaches 2.0, so there is no trigger, and the trace
    // shows the latest window.
    let samples = sine(1000.0, 48_000.0, 4800, 0.0);
    assert_eq!(find_trigger(&samples, 480, Trigger::Level(2.0), Slope::Rising), None);
    let mut trace = ScopeTrace::new(48_000.0);
    trace.history.push(0, &samples);
    trace.update(480, Trigger::Level(2.0), Slope::Rising);
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
    assert_eq!(find_trigger(&[0.5; 4096], 480, Trigger::Auto, Slope::Rising), None);
    assert_eq!(find_trigger(&[0.5; 10], 480, Trigger::Auto, Slope::Rising), None);
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

/// A falling slope starts the trace on a falling edge: the trace goes down
/// from its first sample (`docs/design/panels.md`, Trigger).
#[test]
fn a_falling_slope_starts_on_a_falling_edge() {
    let samples: Vec<f32> = (0..48_000)
        .map(|i| (i as f32 * std::f32::consts::TAU / 480.0).sin())
        .collect();
    let mut trace = ScopeTrace::new(48_000.0);
    trace.history.push(0, &samples);
    trace.update(480, Trigger::Auto, Slope::Falling);
    assert!(trace.triggered);
    assert!(trace.trace[1] < trace.trace[0], "falling at the start");
    assert!(trace.trace[0].abs() < 0.05, "at the level");
    trace.update(480, Trigger::Auto, Slope::Rising);
    assert!(trace.trace[1] > trace.trace[0], "a rising slope still rises");
}

/// A fixed level of 3 triggers on a signal between 2 and 5, on either slope.
/// Cables are never clipped, so a trigger beyond ±1 must work.
#[test]
fn a_fixed_level_of_three_triggers_between_two_and_five() {
    let samples: Vec<f32> = (0..48_000)
        .map(|i| 3.5 + 1.5 * (i as f32 * std::f32::consts::TAU / 480.0).sin())
        .collect();
    for (slope, rising) in [(Slope::Rising, true), (Slope::Falling, false)] {
        let mut trace = ScopeTrace::new(48_000.0);
        trace.history.push(0, &samples);
        trace.update(480, Trigger::Level(3.0), slope);
        assert!(trace.triggered, "{slope:?}");
        assert!((trace.trace[0] - 3.0).abs() < 0.1, "{slope:?} starts at the level");
        assert_eq!(trace.trace[1] > trace.trace[0], rising, "{slope:?}");
    }
}

/// A fixed level near the far end of the signal still arms on a falling edge:
/// the hysteresis never reaches above the highest sample (`panels.md`, Trigger).
#[test]
fn a_falling_edge_near_the_top_still_arms() {
    let samples: Vec<f32> = (0..48_000)
        .map(|i| (i as f32 * std::f32::consts::TAU / 480.0).sin())
        .collect();
    let mut trace = ScopeTrace::new(48_000.0);
    trace.history.push(0, &samples);
    trace.update(480, Trigger::Level(0.99), Slope::Falling);
    assert!(trace.triggered);
}

/// A triggered trace takes `window + 2` samples, so after the shift by its
/// fraction its first point lies at or left of x = 0 and its last at or
/// right of x = 1. A short history has no start that leaves that many, so it
/// runs free (panels.md, Drawing the scope trace, The line reaches past both
/// edges).
#[test]
fn a_triggered_line_reaches_past_both_edges() {
    let samples = sine(261.63, 48_000.0, 4800, 0.0);
    let mut scope = ScopeTrace::new(48_000.0);
    scope.history.push(0, &samples);
    scope.update(240, Trigger::Auto, Slope::Rising);
    assert!(scope.triggered);
    assert_eq!(scope.trace.len(), 242);
    let trace = scope.canvas_trace(true);
    assert!(trace.points[0][0] <= 0.0);
    assert!(trace.points.last().unwrap()[0] >= 1.0);

    // One rising edge between samples 1 and 2. From the sample before it,
    // 241 samples are too few for a window of 240, and 242 are enough.
    let step: Vec<f32> = (0..243).map(|i| if i < 2 { -1.0 } else { 1.0 }).collect();
    assert_eq!(find_trigger(&step[..242], 240, Trigger::Auto, Slope::Rising), None);
    assert_eq!(find_trigger(&step, 240, Trigger::Auto, Slope::Rising), Some(1.5));
}

/// True when every point of `trace` has a finite x and y.
fn all_finite(trace: &loom_canvas::Trace) -> bool {
    trace.points.iter().flatten().all(|v| v.is_finite())
}

/// A NaN sample draws no point, in the full trace and in a reduced one, and
/// the line joins the samples on each side. A group made only of NaN adds
/// no point (panels.md, Drawing the scope trace, Values that are not finite).
#[test]
fn a_nan_in_the_window_draws_no_point() {
    let mut scope = ScopeTrace::new(48_000.0);
    scope.trace = sine(1000.0, 48_000.0, 241, 0.0);
    scope.trace[100] = f32::NAN;
    scope.span = 240.0;
    let trace = scope.canvas_trace(true);
    assert!(all_finite(&trace));
    assert_eq!(trace.points.len(), 240, "one point left out");
    assert!(trace.points.iter().all(|p| (p[0] * 240.0 - 100.0).abs() > 0.5));

    // Reduced in groups of 9: samples 801 to 809 are one group, all NaN.
    let n = loom_canvas::MAX_TRACE_POINTS * 4;
    scope.trace = sine(100.0, 48_000.0, n, 0.0);
    scope.trace[3] = f32::NAN;
    scope.trace[801..810].fill(f32::NAN);
    scope.span = n as f32;
    let trace = scope.canvas_trace(true);
    assert!(all_finite(&trace));
    assert!(trace.points.len() <= loom_canvas::MAX_TRACE_POINTS);
    assert!(
        trace.points.iter().any(|p| p[0] * (n as f32) < 8.0),
        "a group with one NaN keeps its points"
    );
    let in_gap = |p: &&[f32; 2]| (801.0..810.0).contains(&(p[0] * n as f32));
    assert_eq!(
        trace.points.iter().filter(in_gap).count(),
        0,
        "the NaN group adds no point"
    );
    assert!(trace.points.windows(2).all(|pair| pair[0][0] <= pair[1][0]));
}

/// A window of only NaN draws no line, and its level is 0.
#[test]
fn a_window_of_only_nan_draws_no_line() {
    let mut scope = ScopeTrace::new(48_000.0);
    scope.history.push(0, &[f32::NAN; 1000]);
    scope.update(240, Trigger::Auto, Slope::Rising);
    assert!(!scope.triggered);
    let trace = scope.canvas_trace(true);
    assert!(trace.points.is_empty());
    assert_eq!(trace.level, 0.0);
    scope.trace = vec![f32::NAN; loom_canvas::MAX_TRACE_POINTS * 2];
    assert!(scope.canvas_trace(true).points.is_empty(), "reduced too");
}

/// An infinity is pinned to the frame's edge, as any value past the range
/// is, and the level is full. It pushes the range to its largest step.
#[test]
fn an_infinity_is_drawn_at_the_edge() {
    let mut samples = sine(1000.0, 48_000.0, 4800, 0.0);
    samples[4700] = f32::INFINITY;
    samples[4750] = f32::NEG_INFINITY;
    let mut scope = ScopeTrace::new(48_000.0);
    scope.history.push(0, &samples);
    scope.update(240, Trigger::Auto, Slope::Rising);
    assert!(!scope.triggered, "an infinity leaves Auto no level");
    assert_eq!(scope.range, 10.0);
    let trace = scope.canvas_trace(true);
    assert!(all_finite(&trace));
    assert!(trace.points.iter().any(|p| p[1] == 1.0));
    assert!(trace.points.iter().any(|p| p[1] == -1.0));
    assert_eq!(scope.canvas_trace(false).level, 1.0);
}

/// A fixed level with an infinity just before the crossing, on the side the
/// edge starts from: nothing arms, so the trace runs free with an offset of
/// 0, and every point is finite.
#[test]
fn an_infinity_before_a_fixed_level_runs_free_with_finite_points() {
    let mut samples = sine(1000.0, 48_000.0, 4800, 0.0);
    // The last rising crossing of 0.5 with a full window after it.
    let crossing = (1..=4800 - 241)
        .rev()
        .find(|&i| samples[i - 1] < 0.5 && samples[i] >= 0.5)
        .unwrap();
    samples[crossing - 1] = f32::NEG_INFINITY;
    let mut scope = ScopeTrace::new(48_000.0);
    scope.history.push(0, &samples);
    scope.update(240, Trigger::Level(0.5), Slope::Rising);
    assert!(!scope.triggered);
    assert_eq!(scope.offset, 0.0);
    assert!(all_finite(&scope.canvas_trace(true)));
}

/// A fixed level with an infinity on the side the edge rises toward still
/// triggers, here on the very sample after the crossing, and every point is
/// finite.
#[test]
fn an_infinity_past_a_fixed_level_still_triggers() {
    let mut samples = sine(1000.0, 48_000.0, 4800, 0.0);
    let crossing = (1..=4800 - 241)
        .rev()
        .find(|&i| samples[i - 1] < 0.5 && samples[i] >= 0.5)
        .unwrap();
    samples[crossing] = f32::INFINITY;
    let mut scope = ScopeTrace::new(48_000.0);
    scope.history.push(0, &samples);
    scope.update(240, Trigger::Level(0.5), Slope::Rising);
    assert!(scope.triggered);
    assert_eq!(scope.offset, 0.0, "the crossing is at the finite sample");
    let trace = scope.canvas_trace(true);
    assert!(all_finite(&trace));
    assert!(trace.points.iter().any(|p| p[1] == 1.0));
}
