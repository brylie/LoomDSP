//! Engine behaviour tests using the example patches.

mod common;

use common::*;
use loom_core::{EngineConfig, EngineParts, LoomError, ModuleId, OutputRef, ProbeKind};
use std::sync::atomic::Ordering;

#[test]
fn sine_patch_plays_a_note_and_is_silent_without_one() {
    let mut parts = load("sine.ron");
    let (silent, _) = render(&mut parts, 4_800, 128, &[]);
    assert_eq!(rms(&silent), 0.0);

    let mut parts = load("sine.ron");
    let (left, right) = render(&mut parts, 48_000, 128, &one_note());
    assert!(rms(&left[1_000..20_000]) > 0.05, "note should be audible");
    assert_eq!(left, right, "mono into one side plays on both sides");
    assert!(rms(&left[40_000..]) < 1e-3, "note should have released");
}

#[test]
fn renders_are_bit_identical_across_runs() {
    // NFR-SND-02
    for patch in [
        "sine.ron",
        "subtractive.ron",
        "fm-feedback.ron",
        "plucked-string.ron",
        "modulation.ron",
    ] {
        let a = render(&mut load(patch), 48_000, 256, &one_note());
        let b = render(&mut load(patch), 48_000, 256, &one_note());
        assert_eq!(a, b, "{patch}");
    }
}

#[test]
fn output_does_not_depend_on_host_buffer_size() {
    // FR-GRAPH-04 and the P1 exit criterion: feedback sounds the same at
    // buffer sizes 32 and 1024.
    for patch in [
        "subtractive.ron",
        "fm-feedback.ron",
        "fm-feedback-delay1.ron",
        "plucked-string.ron",
        "modulation.ron",
    ] {
        let small = render(&mut load(patch), 48_000, 32, &one_note());
        let large = render(&mut load(patch), 48_000, 1024, &one_note());
        let odd = render(&mut load(patch), 48_000, 77, &one_note());
        assert_eq!(small, large, "{patch}: 32 vs 1024");
        assert_eq!(small, odd, "{patch}: 32 vs 77");
        assert!(rms(&small.0) > 0.01, "{patch} should make sound");
    }
}

#[test]
fn feedback_fm_recovers_after_extreme_settings() {
    // #121: Multiply gain 10 and Add offset 10 push the loop's pitch past
    // the range of f32 Hz. The sound must come back when the knobs do, and
    // no NaN may reach the host on the way.
    let mut parts = load("fm-feedback.ron");
    let note = [Note {
        note: 57,
        on: 0,
        off: usize::MAX,
    }];
    // Gain first, so the sine swings hard when the offset arrives. Both at
    // once can lock the loop at phase 0, where the sine is 0, and miss the bug.
    let (left, _) = render_with(&mut parts, 84_000, 512, &note, |position, parts| {
        let (gain, offset) = match position {
            24_000..36_000 => (10.0, 0.0),
            36_000..48_000 => (10.0, 10.0),
            _ => (0.08, 0.0),
        };
        parts.controller.set_param(ModuleId(5), 0, gain).unwrap();
        parts.controller.set_param(ModuleId(2), 0, offset).unwrap();
    });
    assert!(
        left.iter().all(|x| x.is_finite()),
        "no NaN or infinity reaches the host"
    );
    assert!(rms(&left[72_000..]) > 0.05, "the tone comes back");

    // After the extremes, the tone is the same periodic wave as before:
    // the loop has no stuck state, only a different phase.
    let before = &left[12_000..24_000];
    let after = &left[72_000..84_000];
    assert!((rms(before) - rms(after)).abs() < 0.01);
}

#[test]
fn final_output_is_clipped_and_never_non_finite() {
    // ADR-0007: audio is clipped only at the final output, to -1 to 1.
    // NaN and infinity become silence there (#121). With the safety stage
    // off, that is all that happens (ADR-0019).
    let mut parts = load_raw("sine.ron");
    let c = &mut parts.controller;
    let constant = c.add_module("loom.math.constant").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(constant, "out", out, "left").unwrap();
    c.set_param_named(constant, "value", 5.0).unwrap();
    c.commit().unwrap();
    // Constant smooths its value, so look after the ramp.
    let (left, right) = render(&mut parts, 2_048, 128, &[]);
    assert!(
        left[1_024..].iter().chain(&right[1_024..]).all(|&x| x == 1.0),
        "5 clips to 1"
    );

    parts.controller.set_param_named(constant, "value", -5.0).unwrap();
    let (left, _) = render(&mut parts, 2_048, 128, &[]);
    assert!(left[1_024..].iter().all(|&x| x == -1.0), "-5 clips to -1");

    // x = 10 * (x + 1) each sample: past f32's range in about 40 samples.
    let c = &mut parts.controller;
    c.remove_module(constant).unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let mul = c.add_module("loom.math.mul").unwrap();
    c.set_param_named(add, "offset", 1.0).unwrap();
    c.set_param_named(mul, "gain", 10.0).unwrap();
    c.connect_named(add, "out", mul, "a").unwrap();
    c.connect_named(mul, "out", add, "a").unwrap();
    c.connect_named(mul, "out", out, "left").unwrap();
    c.commit().unwrap();
    let (left, _) = render(&mut parts, 256, 128, &[]);
    assert!(left.iter().all(|x| x.is_finite()));
    assert!(left.contains(&1.0), "the loop clips at 1 before it overflows");
    assert!(left[100..].iter().all(|&x| x == 0.0), "infinity becomes silence");
}

#[test]
fn compiler_reports_feedback_loops() {
    let parts = load("subtractive.ron");
    assert!(parts.controller.last_report().loops.is_empty());

    let parts = load("fm-feedback.ron");
    let report = parts.controller.last_report();
    assert_eq!(report.loops.len(), 1);
    let feedback = &report.loops[0];
    assert_eq!(
        feedback.modules,
        vec![ModuleId(2), ModuleId(3), ModuleId(4), ModuleId(5)]
    );
    assert_eq!(feedback.inserted_delays.len(), 1);
    let (from, to) = feedback.inserted_delays[0];
    assert_eq!((from.module, to.module), (ModuleId(5), ModuleId(2)));

    // With an explicit unit delay, the compiler adds nothing.
    let parts = load("fm-feedback-delay1.ron");
    let report = parts.controller.last_report();
    assert_eq!(report.loops.len(), 1);
    assert!(report.loops[0].inserted_delays.is_empty());

    let parts = load("plucked-string.ron");
    let report = parts.controller.last_report();
    assert_eq!(report.loops.len(), 1);
    assert!(report.loops[0].inserted_delays.is_empty());
}

#[test]
fn explicit_and_automatic_delays_sound_the_same() {
    // The compiler breaks the loop at the same cable the user did, so the
    // two patches must render identically.
    let automatic = render(&mut load("fm-feedback.ron"), 48_000, 128, &one_note());
    let explicit = render(&mut load("fm-feedback-delay1.ron"), 48_000, 128, &one_note());
    assert_eq!(automatic, explicit);
}

#[test]
fn self_loop_gets_a_delay() {
    let mut parts = load("sine.ron");
    let c = &mut parts.controller;
    let add = c.add_module("loom.math.add").unwrap();
    c.connect_named(add, "out", add, "a").unwrap();
    let report = c.commit().unwrap().clone();
    assert_eq!(report.loops.len(), 1);
    assert_eq!(report.loops[0].modules, vec![add]);
    assert_eq!(report.loops[0].inserted_delays.len(), 1);
    // An integrator with offset 1 counts up by one per sample.
    c.set_param_named(add, "offset", 1.0).unwrap();
    render(&mut parts, 256, 128, &[]);
}

#[test]
fn edits_during_playback_do_not_change_the_sound() {
    // Adding and removing modules that are not in the signal path must not
    // disturb anything that is: no reset state, no glitch (FR-GRAPH-02).
    let reference = render(&mut load("fm-feedback.ron"), 48_000, 128, &one_note());

    let mut parts = load("fm-feedback.ron");
    let mut extra = Vec::new();
    let edited = render_with(&mut parts, 48_000, 128, &one_note(), |position, parts| {
        let c = &mut parts.controller;
        match (position / 128) % 4 {
            0 => {
                let noise = c.add_module("loom.osc.noise").unwrap();
                let gain = c.add_module("loom.math.mul").unwrap();
                c.connect_named(noise, "out", gain, "a").unwrap();
                // Feed it into the loop's adder with zero gain: the topology
                // changes, the sound must not.
                c.set_param_named(gain, "gain", 0.0).unwrap();
                extra.push((noise, gain));
            }
            2 => {
                if let Some((noise, gain)) = extra.pop() {
                    c.remove_module(noise).unwrap();
                    c.remove_module(gain).unwrap();
                }
            }
            _ => {}
        }
        c.commit().unwrap();
    });
    assert_eq!(reference, edited);
    let stats = parts.controller.stats();
    assert_eq!(stats.leaked.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert!(stats.schedule_swaps.load(std::sync::atomic::Ordering::Relaxed) > 100);
}

#[test]
fn unit_delay_state_survives_recompiles() {
    // Recompiling must carry delay state across, or loops would click.
    let reference = render(&mut load("fm-feedback-delay1.ron"), 9_600, 64, &one_note());
    let mut parts = load("fm-feedback-delay1.ron");
    let edited = render_with(&mut parts, 9_600, 64, &one_note(), |_, parts| {
        let c = &mut parts.controller;
        let id = c.add_module("loom.math.constant").unwrap();
        c.commit().unwrap();
        c.remove_module(id).unwrap();
        c.commit().unwrap();
    });
    assert_eq!(reference, edited);
}

#[test]
fn removed_modules_are_returned_for_freeing() {
    let mut parts = load("sine.ron");
    for _ in 0..50 {
        let id = parts.controller.add_module("loom.osc.noise").unwrap();
        parts.controller.commit().unwrap();
        render(&mut parts, 128, 128, &[]);
        parts.controller.remove_module(id).unwrap();
        parts.controller.commit().unwrap();
        render(&mut parts, 128, 128, &[]);
    }
    parts.controller.collect_garbage();
    let stats = parts.controller.stats();
    assert_eq!(stats.leaked.load(std::sync::atomic::Ordering::Relaxed), 0);
}

#[test]
fn scope_probe_reproduces_the_signal_and_meters_match_it() {
    // FR-VIS-04: the scope sees exactly what the port carries, and each meter
    // reading describes one block of it. The safety stage is off, so the
    // output is the port's signal (after the fade-in, which starts before
    // the note).
    let mut parts = load_raw("sine.ron");
    let gain = parts.controller.output_ref(ModuleId(5), "out").unwrap();
    let probe = parts.controller.add_probe(gain, ProbeKind::MeterAndScope).unwrap();
    parts.controller.commit().unwrap();

    let mut scope = Vec::new();
    let mut meters = Vec::new();
    let mut drain = |parts: &mut EngineParts| {
        for chunk in parts.telemetry.scope() {
            assert_eq!(chunk.probe, probe);
            assert_eq!(chunk.frame, scope.len() as u64, "no gaps");
            scope.extend_from_slice(chunk.samples());
        }
        meters.extend(parts.telemetry.meters());
    };
    // 77 frames per buffer: blocks and scope chunks do not line up.
    let note = [Note {
        note: 57,
        on: 1_000,
        off: 24_000,
    }];
    let (left, _) = render_with(&mut parts, 24_000, 77, &note, |_, parts| drain(parts));
    drain(&mut parts);

    assert_eq!(scope, left, "the probed port feeds the output directly");
    let mut frame = 0;
    for reading in &meters {
        assert_eq!(reading.probe, probe);
        assert_eq!(reading.frame, frame);
        let block = &left[frame as usize..frame as usize + reading.frames as usize];
        let peak = block.iter().fold(0.0f32, |p, x| p.max(x.abs()));
        assert_eq!(reading.peak, peak);
        assert!((reading.rms - rms(block)).abs() < 1e-6);
        frame += u64::from(reading.frames);
    }
    assert_eq!(frame, 24_000);
    assert!(meters.iter().any(|m| m.peak > 0.1), "the note is metered");
    let stats = parts.controller.stats();
    assert_eq!(stats.dropped_meters.load(Ordering::Relaxed), 0);
    assert_eq!(stats.dropped_scope.load(Ordering::Relaxed), 0);
}

#[test]
fn probes_inside_feedback_loops_see_every_sample() {
    // Loop members run one sample at a time, into the same buffers.
    let mut parts = load("fm-feedback.ron");
    let probes: Vec<_> = [2, 3, 4, 5]
        .into_iter()
        .map(|m| {
            // Each of these modules has one output.
            let port = OutputRef {
                module: ModuleId(m),
                port: 0,
            };
            parts.controller.add_probe(port, ProbeKind::Scope).unwrap()
        })
        .collect();
    parts.controller.commit().unwrap();
    render(&mut parts, 4_096, 128, &one_note());
    let mut signals = vec![Vec::new(); probes.len()];
    for chunk in parts.telemetry.scope() {
        let i = probes.iter().position(|p| *p == chunk.probe).unwrap();
        signals[i].extend_from_slice(chunk.samples());
    }
    for signal in &signals {
        assert_eq!(signal.len(), 4_096);
        assert!(rms(signal) > 0.0);
    }
    assert_eq!(parts.telemetry.meters().count(), 0, "scope only");
}

#[test]
fn full_telemetry_queues_drop_telemetry_not_audio() {
    // NFR-RT-07: a UI that stops reading loses meters, never sound.
    let reference = render(&mut load("sine.ron"), 9_600, 128, &one_note());

    let config = EngineConfig {
        meter_capacity: 4,
        scope_capacity: 4,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = load_with("sine.ron", config);
    for module in [3, 4, 5] {
        let port = parts.controller.output_ref(ModuleId(module), "out").unwrap();
        parts.controller.add_probe(port, ProbeKind::MeterAndScope).unwrap();
    }
    parts.controller.commit().unwrap();
    let probed = render(&mut parts, 9_600, 128, &one_note());
    assert_eq!(reference, probed, "probes never change the sound");

    let stats = parts.controller.stats();
    assert!(stats.dropped_meters.load(Ordering::Relaxed) > 0);
    assert!(stats.dropped_scope.load(Ordering::Relaxed) > 0);
    // The oldest messages are kept, the newest dropped.
    let meters: Vec<_> = parts.telemetry.meters().collect();
    assert_eq!(meters.len(), 4);
    assert!(meters.iter().all(|m| m.frame < 256));
    let chunks: Vec<_> = parts.telemetry.scope().collect();
    assert_eq!(chunks.len(), 4);
    assert!(chunks.iter().all(|c| c.frame < 128));
}

#[test]
fn removing_a_probe_or_its_module_stops_telemetry() {
    let mut parts = load("sine.ron");
    let c = &mut parts.controller;
    let sine = c.output_ref(ModuleId(3), "out").unwrap();
    let env = c.output_ref(ModuleId(4), "out").unwrap();
    let a = c.add_probe(sine, ProbeKind::Meter).unwrap();
    let b = c.add_probe(env, ProbeKind::Meter).unwrap();
    assert!(c.has_uncommitted_changes(), "probes take effect at commit");
    c.commit().unwrap();
    assert_eq!(c.probe(a), Some((sine, ProbeKind::Meter)));
    render(&mut parts, 128, 128, &[]);
    let probes: Vec<_> = parts.telemetry.meters().map(|m| m.probe).collect();
    assert_eq!(probes, vec![a, b]);

    let c = &mut parts.controller;
    c.remove_probe(a).unwrap();
    assert!(matches!(c.remove_probe(a), Err(LoomError::UnknownProbe(_))));
    c.remove_module(ModuleId(4)).unwrap();
    assert_eq!(c.probes().count(), 0, "removing a module removes its probes");
    c.commit().unwrap();
    render(&mut parts, 128, 128, &[]);
    assert_eq!(parts.telemetry.meters().count(), 0);

    let missing = OutputRef {
        module: ModuleId(3),
        port: 9,
    };
    assert!(parts.controller.add_probe(missing, ProbeKind::Meter).is_err());
}

/// Checks that `loom.time.delay` in a feedback loop without an explicit
/// `loom.time.delay1` gets 1 compiler-inserted unit delay and renders
/// identically across host buffer sizes 1, 32, 77, and 1024.
#[test]
fn delay_in_feedback_loop_gets_an_inserted_unit_delay_and_is_block_invariant() {
    // docs/design/delay.md, rule 10: `loom.time.delay` is a normal DSP module,
    // not a `UnitDelay`. A cable loop without an explicit `loom.time.delay1`
    // gets a 1-sample unit delay inserted by the compiler, so the loop period
    // is `delay_samples + 1`.
    let build = || {
        let config = EngineConfig {
            output_safety: loom_core::OutputSafety::Off,
            ..EngineConfig::with_sample_rate(1_000.0)
        };
        let mut parts = loom_core::create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let trig = c.add_module("loom.phase.trigger").unwrap();
        let add = c.add_module("loom.math.add").unwrap();
        let delay = c.add_module("loom.time.delay").unwrap();
        let mul = c.add_module("loom.math.mul").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        // Default `time` is 0.2 s = 200 samples at 1,000 Hz (already settled).
        // With the inserted 1-sample unit delay on the return cable, echoes
        // are spaced by `200 + 1 = 201` samples.
        c.set_param_named(mul, "gain", 0.5).unwrap();
        c.connect_named(trig, "trigger", add, "a").unwrap();
        c.connect_named(add, "out", delay, "in").unwrap();
        c.connect_named(delay, "out", mul, "a").unwrap();
        c.connect_named(mul, "out", add, "b").unwrap();
        c.connect_named(delay, "out", out, "left").unwrap();
        let report = c.commit().unwrap().clone();
        assert_eq!(report.loops.len(), 1);
        assert_eq!(report.loops[0].inserted_delays.len(), 1);
        parts
    };

    let b1 = render(&mut build(), 1_024, 1, &[]);
    let b32 = render(&mut build(), 1_024, 32, &[]);
    let b77 = render(&mut build(), 1_024, 77, &[]);
    let b1024 = render(&mut build(), 1_024, 1_024, &[]);
    assert_eq!(b1, b32, "1 vs 32");
    assert_eq!(b32, b77, "32 vs 77");
    assert_eq!(b32, b1024, "32 vs 1024");

    // `loom.phase.trigger` fires 1.0 on frame 0 (bar start).
    // First pass through the 200-sample delay line arrives at frame 200 (1.0),
    // second pass after `200 + 1 = 201` samples at frame 401 (0.5),
    // third pass at frame 602 (0.25).
    let left = &b32.0;
    assert!((left[200] - 1.0).abs() < 1e-4, "first echo at 200: {}", left[200]);
    assert!((left[401] - 0.5).abs() < 1e-3, "second echo at 401: {}", left[401]);
    assert!((left[602] - 0.25).abs() < 1e-3, "third echo at 602: {}", left[602]);
}

/// Checks stereo port channel declarations (`PerChannel` / `Downmix`) and
/// verifies that independent left and right delay and all-pass paths through
/// the engine process each channel without crosstalk.
#[test]
fn delay_and_allpass_ports_and_left_right_engine_paths_are_independent() {
    // ADR-0020 and docs/design/delay.md, Stereo: `in` and `out` are
    // `PerChannel`, `time` is `Downmix`.
    use loom_core::Channels;
    for descriptor in [&loom_modules::time::DELAY, &loom_modules::time::ALLPASS] {
        assert_eq!(descriptor.inputs[0].name, "in");
        assert_eq!(descriptor.inputs[0].channels, Channels::PerChannel);
        assert_eq!(descriptor.inputs[1].name, "time");
        assert_eq!(descriptor.inputs[1].channels, Channels::Downmix);
        assert_eq!(descriptor.outputs[0].name, "out");
        assert_eq!(descriptor.outputs[0].channels, Channels::PerChannel);
    }

    // Feed different left (sine) and right (saw) signals through a left and
    // right delay line and all-pass in the engine, and check that each output
    // channel equals that channel's input delayed, with no crosstalk.
    let build_dry_or_wet = |with_delays: bool| {
        let config = EngineConfig {
            output_safety: loom_core::OutputSafety::Off,
            ..EngineConfig::with_sample_rate(1_000.0)
        };
        let mut parts = loom_core::create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;
        let phase_l = c.add_module("loom.phase.generator").unwrap();
        let sine_l = c.add_module("loom.osc.sine").unwrap();
        let phase_r = c.add_module("loom.phase.generator").unwrap();
        let saw_r = c.add_module("loom.osc.saw").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.set_param_named(phase_r, "tune", 7.0).unwrap();
        c.connect_named(phase_l, "phase", sine_l, "phase").unwrap();
        c.connect_named(phase_r, "phase", saw_r, "phase").unwrap();
        if with_delays {
            // At 1,000 Hz, default `Delay.time` is 0.2 s (200 samples) and
            // default `Allpass.time` is 0.01 s (10 samples). With `gain = 0`,
            // the all-pass is a 10-sample delay, so each side is delayed by
            // `200 + 10 = 210` samples.
            let delay_l = c.add_module("loom.time.delay").unwrap();
            let ap_l = c.add_module("loom.time.allpass").unwrap();
            let delay_r = c.add_module("loom.time.delay").unwrap();
            let ap_r = c.add_module("loom.time.allpass").unwrap();
            c.set_param_named(ap_l, "gain", 0.0).unwrap();
            c.set_param_named(ap_r, "gain", 0.0).unwrap();
            c.connect_named(sine_l, "out", delay_l, "in").unwrap();
            c.connect_named(delay_l, "out", ap_l, "in").unwrap();
            c.connect_named(ap_l, "out", out, "left").unwrap();
            c.connect_named(saw_r, "out", delay_r, "in").unwrap();
            c.connect_named(delay_r, "out", ap_r, "in").unwrap();
            c.connect_named(ap_r, "out", out, "right").unwrap();
        } else {
            c.connect_named(sine_l, "out", out, "left").unwrap();
            c.connect_named(saw_r, "out", out, "right").unwrap();
        }
        c.commit().unwrap();
        render(&mut parts, 512, 64, &[])
    };

    let (dry_l, dry_r) = build_dry_or_wet(false);
    let (wet_l, wet_r) = build_dry_or_wet(true);
    assert_ne!(dry_l, dry_r, "left and right sources differ");
    // Wait past the 5 ms gain smoother on `ap_l` / `ap_r` (5 samples at 1 kHz)
    // plus the 210-sample total delay: from frame 220 onward, each channel
    // matches its own dry source shifted by 210 samples.
    for i in 220..512 {
        assert!(
            (wet_l[i] - dry_l[i - 210]).abs() < 1e-4,
            "left frame {i}: got {}, expected {}",
            wet_l[i],
            dry_l[i - 210]
        );
        assert!(
            (wet_r[i] - dry_r[i - 210]).abs() < 1e-4,
            "right frame {i}: got {}, expected {}",
            wet_r[i],
            dry_r[i - 210]
        );
    }
}

/// Checks that the assignments in `examples/patches/modulation.ron` change
/// the sound: the same patch with its `modulation` list removed plays
/// something different, and so does the patch with only the LFO or only
/// the envelope assignment removed, so each one is heard on its own
/// (#150, FR-MOD-01).
#[test]
fn modulation_patch_differs_from_the_same_patch_without_assignments() {
    let config = || EngineConfig {
        output_safety: loom_core::OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let patch = loom_patch::Patch::load(patch_path("modulation.ron")).expect("example patch loads");
    assert_eq!(patch.modulation.len(), 2, "an LFO and an envelope on the cutoff");
    let play = |patch: &loom_patch::Patch| {
        let mut parts = loom_core::create_engine(loom_modules::registry(), config());
        patch.load_into(&mut parts.controller).expect("example patch is valid");
        render(&mut parts, 48_000, 128, &one_note()).0
    };
    let modulated = play(&patch);
    assert!(rms(&modulated[1_000..20_000]) > 0.01, "note should be audible");

    // The patch without the assignments named by `remove`, by source module.
    let without = |remove: &[u32]| {
        let mut changed = patch.clone();
        changed.modulation.retain(|m| !remove.contains(&m.source.0));
        assert_eq!(
            changed.modulation.len(),
            2 - remove.len(),
            "{remove:?} names assignments"
        );
        changed
    };
    for (what, remove) in [("both", &[5, 6][..]), ("the LFO", &[5][..]), ("the envelope", &[6][..])] {
        let other = play(&without(remove));
        let difference: Vec<f32> = modulated.iter().zip(&other).map(|(a, b)| a - b).collect();
        let difference = rms(&difference[1_000..20_000]);
        assert!(
            difference > 0.01,
            "removing {what} should change the sound, difference RMS {difference}"
        );
    }
}

/// Checks that `examples/patches/plucked-string.ron` rings at A3 (220 Hz).
#[test]
fn plucked_string_patch_rings_at_a3() {
    // docs/design/delay.md, Worked example: `plucked-string.ron` is tuned to
    // A3 (220 Hz at 48 kHz) once `time` (212.88 samples), `delay1` (1 sample),
    // and the 3,500 Hz SVF lowpass phase delay (~4.28 samples) are summed.
    let mut parts = load_raw("plucked-string.ron");
    let note = [Note {
        note: 57,
        on: 1_000,
        off: 24_000,
    }];
    let (left, _) = render(&mut parts, 48_000, 128, &note);
    // Measure pitch in the steady decaying tail after the noise burst settles
    // (frames 4,800..24,000 = 0.1 s..0.5 s) by finding the autocorrelation peak
    // around the expected 218-sample period.
    let tail = &left[4_800..24_000];
    let corr = |lag: usize| -> f64 {
        tail.iter()
            .zip(&tail[lag..])
            .map(|(&a, &b)| f64::from(a) * f64::from(b))
            .sum()
    };
    let best_lag = (200..=235)
        .max_by(|&a, &b| corr(a).partial_cmp(&corr(b)).unwrap())
        .unwrap();
    // Parabolic interpolation around `best_lag` for sub-sample period accuracy.
    let y0 = corr(best_lag - 1);
    let y1 = corr(best_lag);
    let y2 = corr(best_lag + 1);
    let delta = 0.5 * (y0 - y2) / (y0 - 2.0 * y1 + y2);
    let period_samples = best_lag as f64 + delta;
    let freq_hz = f64::from(SAMPLE_RATE) / period_samples;
    assert!(
        (freq_hz - 220.0).abs() < 0.5,
        "expected ~220.0 Hz, got {freq_hz:.2} Hz (period {period_samples:.2} samples)"
    );
}

/// Checks that `loom.time.delay` and `loom.time.allpass` stay bounded and
/// block-size invariant when modulated at audio rate through both the `time`
/// input cable and wireless parameter assignments on `time` and `gain`.
#[test]
fn delay_and_allpass_stay_bounded_and_block_invariant_under_audio_rate_modulation() {
    let build = |block_size: usize| -> (Vec<f32>, Vec<f32>) {
        let config = EngineConfig {
            output_safety: loom_core::OutputSafety::Off,
            ..EngineConfig::with_sample_rate(SAMPLE_RATE)
        };
        let mut parts = loom_core::create_engine(loom_modules::registry(), config);
        let c = &mut parts.controller;

        // Audio source: 220 Hz saw oscillator.
        let src_phase = c.add_module("loom.phase.generator").unwrap();
        let src = c.add_module("loom.osc.saw").unwrap();
        c.connect_named(src_phase, "phase", src, "phase").unwrap();

        // Audio-rate modulator (~440 Hz sine) driving both the `time` cable
        // (scaled to ±2 ms) and wireless assignments on `time` and `gain`.
        let mod_phase = c.add_module("loom.phase.generator").unwrap();
        c.set_param_named(mod_phase, "tune", 9.0).unwrap();
        let mod_osc = c.add_module("loom.osc.sine").unwrap();
        let mod_scale = c.add_module("loom.math.mul").unwrap();
        c.set_param_named(mod_scale, "gain", 0.002).unwrap();
        c.connect_named(mod_phase, "phase", mod_osc, "phase").unwrap();
        c.connect_named(mod_osc, "out", mod_scale, "a").unwrap();

        let delay = c.add_module("loom.time.delay").unwrap();
        c.set_param_named(delay, "time", 0.008).unwrap();
        let allpass = c.add_module("loom.time.allpass").unwrap();
        c.set_param_named(allpass, "time", 0.008).unwrap();
        c.set_param_named(allpass, "gain", 0.9).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();

        c.connect_named(src, "out", delay, "in").unwrap();
        c.connect_named(mod_scale, "out", delay, "time").unwrap();
        c.connect_named(src, "out", allpass, "in").unwrap();
        c.connect_named(mod_scale, "out", allpass, "time").unwrap();
        c.connect_named(delay, "out", out, "left").unwrap();
        c.connect_named(allpass, "out", out, "right").unwrap();

        c.add_assignment_named(mod_osc, "out", delay, "time", 0.1).unwrap();
        c.add_assignment_named(mod_osc, "out", allpass, "time", 0.1).unwrap();
        c.add_assignment_named(mod_osc, "out", allpass, "gain", 0.1).unwrap();

        let delay_ref = c.output_ref(delay, "out").unwrap();
        let allpass_ref = c.output_ref(allpass, "out").unwrap();
        let delay_probe = c.add_probe(delay_ref, ProbeKind::Scope).unwrap();
        let allpass_probe = c.add_probe(allpass_ref, ProbeKind::Scope).unwrap();
        c.commit().unwrap();

        let total_frames = 4_096;
        let mut left = vec![0.0f32; total_frames];
        let mut right = vec![0.0f32; total_frames];
        let mut offset = 0;
        let mut delay_unclipped = Vec::with_capacity(total_frames);
        let mut allpass_unclipped = Vec::with_capacity(total_frames);
        while offset < total_frames {
            let n = block_size.min(total_frames - offset);
            parts
                .engine
                .process(&mut left[offset..offset + n], &mut right[offset..offset + n]);
            for chunk in parts.telemetry.scope() {
                if chunk.probe == delay_probe {
                    delay_unclipped.extend_from_slice(chunk.samples());
                } else if chunk.probe == allpass_probe {
                    allpass_unclipped.extend_from_slice(chunk.samples());
                }
            }
            offset += n;
        }
        assert_eq!(delay_unclipped.len(), total_frames);
        assert_eq!(allpass_unclipped.len(), total_frames);
        (delay_unclipped, allpass_unclipped)
    };

    let (ref_delay, ref_allpass) = build(1);
    assert!(ref_delay.iter().all(|x| x.is_finite() && x.abs() <= 1.25 + 1e-5));
    assert!(ref_allpass.iter().all(|x| x.is_finite() && x.abs() < 10.0));
    for block in [32, 77, 512] {
        let (d, ap) = build(block);
        assert_eq!(d, ref_delay, "delay block size {block} differs from 1");
        assert_eq!(ap, ref_allpass, "allpass block size {block} differs from 1");
    }
}
