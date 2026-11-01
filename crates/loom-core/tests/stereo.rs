//! The engine runs stereo (ADR-0020, `docs/design/stereo.md`): per-channel
//! copies, downmixes, `Stereo` ports, and per-channel delay state.

mod stereo_modules;

use loom_core::{EngineParts, ModuleId};
use stereo_modules::*;

/// The block sizes the module test harness uses.
const BLOCKS: [usize; 3] = [1, 77, 512];

fn add(parts: &mut EngineParts, kind: &str) -> ModuleId {
    parts.controller.add_module(kind).unwrap()
}

fn connect(parts: &mut EngineParts, from: ModuleId, output: &str, to: ModuleId, input: &str) {
    parts.controller.connect_named(from, output, to, input).unwrap();
}

/// Checks each side of a copied module matches a mono run of that side.
#[test]
fn each_side_of_a_copied_module_matches_a_mono_run_of_that_side() {
    const FRAMES: usize = 2000;
    for block in BLOCKS {
        let mut parts = engine(512);
        let ramp = add(&mut parts, "test.ramp");
        let lag = add(&mut parts, "test.lag");
        let split = add(&mut parts, "test.split");
        let channels = add(&mut parts, "test.split");
        connect(&mut parts, ramp, "out", lag, "in");
        connect(&mut parts, lag, "out", split, "in");
        connect(&mut parts, lag, "channel", channels, "in");
        let mut capture = Capture::new(
            &mut parts,
            &[
                (split, "left"),
                (split, "right"),
                (channels, "left"),
                (channels, "right"),
            ],
        );
        let report = parts.controller.commit().unwrap();
        assert!(report.runs_per_channel(lag));
        assert!(!report.runs_per_channel(split));
        capture.render(&mut parts, FRAMES, block);

        let (mut left, mut right) = (0.0, 0.0);
        for n in 0..FRAMES {
            left = lag_step(left, ramp_left(n), 0.25);
            right = lag_step(right, ramp_right(n), 0.25);
            assert_eq!(capture.get(0)[n], left, "left, frame {n}, block {block}");
            assert_eq!(capture.get(1)[n], right, "right, frame {n}, block {block}");
        }
        // `ProcessContext::channel` tells each copy its side.
        assert!(capture.get(2).iter().all(|&x| x == 0.0));
        assert!(capture.get(3).iter().all(|&x| x == 1.0));
    }
}

/// Checks downmixes follow the input signal type.
#[test]
fn downmixes_follow_the_input_signal_type() {
    let mut parts = engine(64);
    let level = add(&mut parts, "test.pair");
    let gate = add(&mut parts, "test.pair");
    let phase = add(&mut parts, "test.pair");
    let read = add(&mut parts, "test.read");
    let set = |parts: &mut EngineParts, module: ModuleId, left: f32, right: f32| {
        parts.controller.set_param_named(module, "left", left).unwrap();
        parts.controller.set_param_named(module, "right", right).unwrap();
    };
    set(&mut parts, level, 0.2, 0.4);
    set(&mut parts, gate, 1.0, 0.0);
    set(&mut parts, phase, 0.9, 0.1);
    connect(&mut parts, level, "out", read, "level");
    connect(&mut parts, gate, "out", read, "gate");
    connect(&mut parts, phase, "out", read, "phase");
    let mut capture = Capture::new(&mut parts, &[(read, "level"), (read, "gate"), (read, "phase")]);
    let report = parts.controller.commit().unwrap();
    assert!(!report.runs_per_channel(read));
    capture.render(&mut parts, 64, 64);

    // Audio, modulation, and pitch: the mean.
    let mean = (0.2_f32 + 0.4) * 0.5;
    assert!(capture.get(0).iter().all(|&x| x == mean), "{:?}", &capture.get(0)[..4]);
    // A gate high on one side only stays high (the mean would be 0.5, low).
    assert!(capture.get(1).iter().all(|&x| x == 1.0));
    // A phase across the wrap: 0.9 and 0.1 meet at 0.0, not 0.5.
    assert!(capture.get(2).iter().all(|&x| x == 0.0), "{:?}", &capture.get(2)[..4]);

    // Exactly half a cycle apart: the fixed tie rule gives 0.0, every run.
    set(&mut parts, phase, 0.25, 0.75);
    capture.render(&mut parts, 64, 64);
    assert!(
        capture.get(2)[64..].iter().all(|&x| x == 0.0),
        "{:?}",
        &capture.get(2)[64..68]
    );
}

/// Checks a mono cable into a stereo input reads the same on both sides.
#[test]
fn a_mono_cable_into_a_stereo_input_reads_the_same_on_both_sides() {
    let mut parts = engine(64);
    let mono = add(&mut parts, "test.mono-ramp");
    let split = add(&mut parts, "test.split");
    connect(&mut parts, mono, "out", split, "in");
    let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
    let report = parts.controller.commit().unwrap();
    assert!(report.stereo_outputs.is_empty());
    capture.render(&mut parts, 300, 64);
    for n in 0..300 {
        assert_eq!(capture.get(0)[n], ramp_left(n));
        assert_eq!(capture.get(1)[n], ramp_left(n));
    }
}

/// Unknown modules stay silent when a stereo cable reaches their saved ports.
#[test]
fn stereo_cables_through_an_unknown_module_stay_silent() {
    let mut parts = engine(64);
    let source = add(&mut parts, "test.ramp");
    let unknown = parts
        .controller
        .add_unknown(loom_core::UnknownModule {
            kind: "future.effect@1".into(),
            label: None,
            params: Default::default(),
            ranges: Default::default(),
            inputs: vec!["in".into()],
            outputs: vec!["out".into()],
            parameters: Vec::new(),
        })
        .unwrap();
    let split = add(&mut parts, "test.split");
    connect(&mut parts, source, "out", unknown, "in");
    connect(&mut parts, unknown, "out", split, "in");
    let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
    let report = parts.controller.commit().unwrap();
    assert!(!report.runs_per_channel(unknown));
    capture.render(&mut parts, 128, 64);
    assert!(capture.get(0).iter().all(|&sample| sample == 0.0));
    assert!(capture.get(1).iter().all(|&sample| sample == 0.0));
}

/// `y[n] = x[n] + y[n - 1] / 2` on each side, through a loop.
fn check_stereo_loop(with_delay_module: bool) {
    const FRAMES: usize = 1500;
    for block in BLOCKS {
        let mut parts = engine(512);
        let ramp = add(&mut parts, "test.ramp");
        let sum = add(&mut parts, "test.add");
        let split = add(&mut parts, "test.split");
        connect(&mut parts, ramp, "out", sum, "a");
        if with_delay_module {
            let delay = add(&mut parts, "test.delay1");
            connect(&mut parts, sum, "out", delay, "in");
            connect(&mut parts, delay, "out", sum, "b");
        } else {
            connect(&mut parts, sum, "out", sum, "b");
        }
        connect(&mut parts, sum, "out", split, "in");
        let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
        let report = parts.controller.commit().unwrap();
        assert!(report.runs_per_channel(sum));
        assert_eq!(report.loops.len(), 1);
        assert_eq!(report.inserted_delays().count(), usize::from(!with_delay_module));
        capture.render(&mut parts, FRAMES, block);

        let (mut left, mut right) = (0.0_f32, 0.0_f32);
        for n in 0..FRAMES {
            left = ramp_left(n) + 0.5 * left;
            right = ramp_right(n) + 0.5 * right;
            assert_eq!(capture.get(0)[n], left, "left, frame {n}, block {block}");
            assert_eq!(capture.get(1)[n], right, "right, frame {n}, block {block}");
        }
    }
}

/// Checks a stereo loop through a unit delay keeps each side apart.
#[test]
fn a_stereo_loop_through_a_unit_delay_keeps_each_side_apart() {
    check_stereo_loop(true);
}

/// Checks a stereo loop with an inserted delay keeps each side apart.
#[test]
fn a_stereo_loop_with_an_inserted_delay_keeps_each_side_apart() {
    check_stereo_loop(false);
}

/// A mono feedback delay keeps its held sample when the loop becomes stereo.
#[test]
fn feedback_delays_keep_the_mono_state_when_the_loop_becomes_stereo() {
    for (explicit_delay, block) in [false, true]
        .into_iter()
        .flat_map(|explicit| BLOCKS.map(|block| (explicit, block)))
    {
        let mut parts = engine(512);
        let mono = add(&mut parts, "test.mono-ramp");
        let pair = add(&mut parts, "test.pair");
        let sum = add(&mut parts, "test.add");
        let split = add(&mut parts, "test.split");
        parts.controller.set_param_named(pair, "left", 0.25).unwrap();
        parts.controller.set_param_named(pair, "right", 0.25).unwrap();
        connect(&mut parts, mono, "out", sum, "a");
        if explicit_delay {
            let delay = add(&mut parts, "test.delay1");
            connect(&mut parts, sum, "out", delay, "in");
            connect(&mut parts, delay, "out", sum, "b");
        } else {
            connect(&mut parts, sum, "out", sum, "b");
        }
        connect(&mut parts, sum, "out", split, "in");
        let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
        parts.controller.commit().unwrap();
        capture.render(&mut parts, 128, block);
        assert_ne!(capture.get(0)[127], 0.0);
        connect(&mut parts, pair, "out", sum, "a");
        parts.controller.commit().unwrap();
        capture.render(&mut parts, 64, block);
        assert_eq!(
            &capture.get(0)[128..],
            &capture.get(1)[128..],
            "explicit delay: {explicit_delay}, block: {block}"
        );
    }
}

/// Stereo entering only a downmix input leaves the feedback loop mono.
#[test]
fn downmixed_stereo_input_in_a_mono_feedback_loop() {
    for block in BLOCKS {
        let mut parts = engine(512);
        let pair = add(&mut parts, "test.pair");
        let read = add(&mut parts, "test.read");
        let add = add(&mut parts, "test.add");
        parts.controller.set_param_named(pair, "left", 0.2).unwrap();
        parts.controller.set_param_named(pair, "right", 0.4).unwrap();
        connect(&mut parts, pair, "out", read, "level");
        connect(&mut parts, read, "level", add, "a");
        connect(&mut parts, add, "out", read, "gate");
        let mut capture = Capture::new(&mut parts, &[(read, "level"), (add, "out")]);
        let report = parts.controller.commit().unwrap();
        assert_eq!(report.loops.len(), 1);
        assert!(!report.runs_per_channel(read));
        assert!(!report.runs_per_channel(add));
        capture.render(&mut parts, 512, block);
        let mean = (0.2_f32 + 0.4) * 0.5;
        assert!(capture.get(0).iter().all(|&sample| sample == mean));
        assert!(capture.get(1)[1..].iter().all(|&sample| sample == mean));
    }
}

/// Checks a stereo source modulating a module that runs as one gives the mean.
#[test]
fn a_stereo_source_modulating_a_module_that_runs_as_one_gives_the_mean() {
    let mut parts = engine(64);
    let pair = add(&mut parts, "test.pair");
    let read = add(&mut parts, "test.read");
    parts.controller.set_param_named(pair, "left", 0.2).unwrap();
    parts.controller.set_param_named(pair, "right", 0.4).unwrap();
    parts
        .controller
        .add_assignment_named(pair, "out", read, "p", 1.0)
        .unwrap();
    let mut capture = Capture::new(&mut parts, &[(read, "p")]);
    let report = parts.controller.commit().unwrap();
    // An assignment never makes a module run per channel (rule 17).
    assert!(!report.runs_per_channel(read));
    capture.render(&mut parts, 64, 64);
    assert!(
        capture.get(0).iter().all(|&x| (x - 0.3).abs() < 1e-6),
        "{:?}",
        &capture.get(0)[..4]
    );
}

/// Checks each copy reads its own side of a stereo modulation source.
#[test]
fn each_copy_reads_its_own_side_of_a_stereo_modulation_source() {
    let mut parts = engine(64);
    let ramp = add(&mut parts, "test.ramp");
    let lag = add(&mut parts, "test.lag");
    let pair = add(&mut parts, "test.pair");
    let split = add(&mut parts, "test.split");
    parts.controller.set_param_named(pair, "left", 0.1).unwrap();
    parts.controller.set_param_named(pair, "right", 0.5).unwrap();
    connect(&mut parts, ramp, "out", lag, "in");
    connect(&mut parts, lag, "coef", split, "in");
    parts
        .controller
        .add_assignment_named(pair, "out", lag, "coef", 1.0)
        .unwrap();
    let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
    parts.controller.commit().unwrap();
    capture.render(&mut parts, 64, 64);
    // Base 0.25 plus each side of the source.
    assert!(
        capture.get(0).iter().all(|&x| (x - 0.35).abs() < 1e-6),
        "{:?}",
        &capture.get(0)[..4]
    );
    assert!(
        capture.get(1).iter().all(|&x| (x - 0.75).abs() < 1e-6),
        "{:?}",
        &capture.get(1)[..4]
    );
}

/// Checks a parameter change reaches both copies.
#[test]
fn a_parameter_change_reaches_both_copies() {
    let mut parts = engine(64);
    let ramp = add(&mut parts, "test.ramp");
    let lag = add(&mut parts, "test.lag");
    let split = add(&mut parts, "test.split");
    connect(&mut parts, ramp, "out", lag, "in");
    connect(&mut parts, lag, "coef", split, "in");
    let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
    parts.controller.commit().unwrap();
    capture.render(&mut parts, 64, 64);
    parts.controller.set_param_named(lag, "coef", 0.6).unwrap();
    capture.render(&mut parts, 64, 64);
    for side in 0..2 {
        assert!(capture.get(side)[..64].iter().all(|&x| x == 0.25));
        assert!(capture.get(side)[64..].iter().all(|&x| x == 0.6));
    }
}

/// Checks a new copy starts with the current parameters.
#[test]
fn a_new_copy_starts_with_the_current_parameters() {
    let mut parts = engine(64);
    let ramp = add(&mut parts, "test.ramp");
    let lag = add(&mut parts, "test.lag");
    let split = add(&mut parts, "test.split");
    parts.controller.set_param_named(lag, "coef", 0.7).unwrap();
    connect(&mut parts, lag, "coef", split, "in");
    parts.controller.commit().unwrap();
    // The lag turns stereo after its parameter was set.
    connect(&mut parts, ramp, "out", lag, "in");
    let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
    parts.controller.commit().unwrap();
    capture.render(&mut parts, 64, 64);
    assert!(capture.get(0).iter().all(|&x| x == 0.7));
    assert!(capture.get(1).iter().all(|&x| x == 0.7));
}

/// Checks stereo reaches both sides of the audio output.
#[test]
fn stereo_reaches_both_sides_of_the_audio_output() {
    let mut parts = engine(64);
    let pair = add(&mut parts, "test.pair");
    let out = add(&mut parts, "test.out");
    parts.controller.set_param_named(pair, "left", 0.2).unwrap();
    parts.controller.set_param_named(pair, "right", 0.4).unwrap();
    connect(&mut parts, pair, "out", out, "in");
    let mut capture = Capture::new(&mut parts, &[]);
    parts.controller.commit().unwrap();
    // Audio fades in over 10 ms (480 frames) even with safety off, so
    // check after it.
    let (left, right) = capture.render(&mut parts, 1024, 64);
    assert!(left[512..].iter().all(|&x| x == 0.2));
    assert!(right[512..].iter().all(|&x| x == 0.4));
}

/// Checks copies are created and retired with the width.
#[test]
fn copies_are_created_and_retired_with_the_width() {
    let mut parts = engine(64);
    let ramp = add(&mut parts, "test.ramp");
    let first = add(&mut parts, "test.lag");
    let second = add(&mut parts, "test.lag");
    connect(&mut parts, first, "out", second, "in");
    parts.controller.commit().unwrap();
    let free = parts.controller.free_module_slots();

    connect(&mut parts, ramp, "out", first, "in");
    let report = parts.controller.commit().unwrap();
    assert!(report.runs_per_channel(first) && report.runs_per_channel(second));
    assert_eq!(parts.controller.free_module_slots(), free - 2);
    let mut left = [0.0; 64];
    let mut right = [0.0; 64];
    parts.engine.process(&mut left, &mut right);

    let input = parts.controller.input_ref(first, "in").unwrap();
    parts.controller.disconnect(input).unwrap();
    let report = parts.controller.commit().unwrap();
    assert!(report.per_channel.is_empty());
    assert_eq!(parts.controller.free_module_slots(), free);
    parts.engine.process(&mut left, &mut right);

    // Removing a module that runs per channel frees both of its slots.
    connect(&mut parts, ramp, "out", first, "in");
    parts.controller.commit().unwrap();
    parts.engine.process(&mut left, &mut right);
    parts.controller.remove_module(first).unwrap();
    parts.controller.remove_module(second).unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(parts.controller.free_module_slots(), free + 2);
}

/// Returning a running chain to mono preserves its left copy's state.
#[test]
fn returning_to_mono_keeps_the_left_state() {
    for block in BLOCKS {
        let mut parts = engine(512);
        let pair = add(&mut parts, "test.pair");
        let lag = add(&mut parts, "test.lag");
        let split = add(&mut parts, "test.split");
        parts.controller.set_param_named(pair, "left", 0.6).unwrap();
        parts.controller.set_param_named(pair, "right", -0.8).unwrap();
        connect(&mut parts, pair, "out", lag, "in");
        connect(&mut parts, lag, "out", split, "in");
        let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
        assert!(parts.controller.commit().unwrap().runs_per_channel(lag));
        capture.render(&mut parts, 512, block);
        let old_left = capture.get(0)[511];
        let old_right = capture.get(1)[511];
        assert_ne!(old_left, old_right);

        let input = parts.controller.input_ref(lag, "in").unwrap();
        parts.controller.disconnect(input).unwrap();
        assert!(!parts.controller.commit().unwrap().runs_per_channel(lag));
        capture.render(&mut parts, 128, block);
        let mut expected = old_left;
        for frame in 512..640 {
            expected = lag_step(expected, 0.0, 0.25);
            assert_eq!(capture.get(0)[frame], expected, "left, frame {frame}, block {block}");
            assert_eq!(capture.get(1)[frame], expected, "right, frame {frame}, block {block}");
        }
    }
}

/// Checks turning stereo copies the running state.
#[test]
fn turning_stereo_copies_the_running_state() {
    let mut parts = engine(64);
    let mono = add(&mut parts, "test.mono-ramp");
    let ramp = add(&mut parts, "test.ramp");
    let sum = add(&mut parts, "test.add");
    let lag = add(&mut parts, "test.lag");
    let split = add(&mut parts, "test.split");
    connect(&mut parts, mono, "out", sum, "a");
    connect(&mut parts, sum, "out", lag, "in");
    connect(&mut parts, lag, "out", split, "in");
    let mut capture = Capture::new(&mut parts, &[(split, "left"), (split, "right")]);
    parts.controller.commit().unwrap();
    capture.render(&mut parts, 64, 64);

    // The unconnected stereo source was skipped and starts at frame zero.
    connect(&mut parts, ramp, "out", sum, "b");
    let report = parts.controller.commit().unwrap();
    assert!(report.runs_per_channel(lag));
    capture.render(&mut parts, 64, 64);

    let mut left = 0.0;
    for n in 0..64 {
        left = lag_step(left, ramp_left(n), 0.25);
    }
    let mut right = left;
    for n in 64..128 {
        left = lag_step(left, ramp_left(n) + 0.5 * ramp_left(n - 64), 0.25);
        right = lag_step(right, ramp_left(n) + 0.5 * ramp_right(n - 64), 0.25);
        assert_eq!(capture.get(0)[n], left, "left keeps its state, frame {n}");
        assert_eq!(capture.get(1)[n], right, "right takes the running state, frame {n}");
    }
}

/// Stereo meters report each side and a scope sees the mean, including
/// cancellation of opposite signals.
#[test]
fn stereo_probe_measures_each_side_and_scopes_the_mean() {
    let mut parts = engine(128);
    let pair = add(&mut parts, "test.pair");
    parts.controller.set_param_named(pair, "left", 0.5).unwrap();
    parts.controller.set_param_named(pair, "right", -0.5).unwrap();
    let output = parts.controller.output_ref(pair, "out").unwrap();
    let probe = parts
        .controller
        .add_probe(output, loom_core::ProbeKind::MeterAndScope)
        .unwrap();
    parts.controller.commit().unwrap();
    parts.engine.process(&mut [0.0; 128], &mut [0.0; 128]);
    let reading = parts.telemetry.pop_meter().unwrap();
    assert_eq!(reading.probe, probe);
    assert_eq!((reading.peak, reading.rms), (0.5, 0.5));
    assert_eq!(reading.right, Some((0.5, 0.5)));
    let samples: Vec<_> = parts
        .telemetry
        .scope()
        .flat_map(|chunk| chunk.samples().to_vec())
        .collect();
    assert_eq!(samples.len(), 128);
    assert!(samples.iter().all(|&sample| sample == 0.0));
}
