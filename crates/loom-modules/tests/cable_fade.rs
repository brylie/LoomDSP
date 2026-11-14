//! Live patching rule 9 (FR-GRAPH-05): a cable change fades over 5 ms.
//!
//! When a schedule swap changes what feeds an input, the input holds the
//! last value it read and moves in a straight line to the new signal over
//! 5 ms. Gate, trigger, and phase inputs switch on the swap frame. Nothing
//! fades before the engine has played its first block. An Add with offset
//! 0 is an exact pass-through, so its output shows what its input reads.
//! Spec: `docs/design/live-patching.md`, rule 9.

mod common;

use common::*;
use loom_core::cable_fade::CABLE_FADE_SECONDS;
use loom_core::{EngineConfig, EngineParts, ModuleId, OutputSafety, create_engine};

/// Frames a fade takes at the test rate: 240.
fn fade_length() -> usize {
    (SAMPLE_RATE * CABLE_FADE_SECONDS).round() as usize
}

/// What an input reads `k` frames after its cable changed, from `held` to
/// `signal`: the engine's arithmetic.
fn faded(held: f32, signal: f32, k: usize) -> f32 {
    let length = fade_length();
    if k == 0 {
        held
    } else if k >= length {
        signal
    } else {
        let t = k as f32 * (1.0 / length as f32);
        (held * (1.0 - t) + signal * t).clamp(-f32::MAX, f32::MAX)
    }
}

/// Host buffer sizes. The edit lands on the same frame at each.
const BLOCKS: [usize; 3] = [1, 77, 512];

/// An engine with output safety off, so the output is what the patch plays.
fn engine() -> EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    create_engine(loom_modules::registry(), config)
}

/// Renders `frames` frames in buffers of up to `block`, making `change`
/// and committing just before frame `at`.
fn render_changing(
    parts: &mut EngineParts,
    frames: usize,
    at: usize,
    block: usize,
    change: impl FnOnce(&mut EngineParts),
) -> Vec<f32> {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut run = |parts: &mut EngineParts, from: usize, to: usize| {
        let mut position = from;
        while position < to {
            let n = block.min(to - position);
            parts
                .engine
                .process(&mut left[position..position + n], &mut right[position..position + n]);
            position += n;
        }
    };
    run(parts, 0, at);
    change(parts);
    parts.controller.commit().unwrap();
    run(parts, at, frames);
    left
}

/// A sine at middle C, from a Phase module into a Sine module.
fn sine(parts: &mut EngineParts) -> ModuleId {
    let c = &mut parts.controller;
    let phase = c.add_module("loom.phase.generator").unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    c.connect_named(phase, "phase", sine, "phase").unwrap();
    sine
}

/// A Constant module at `value`.
fn constant(parts: &mut EngineParts, value: f32) -> ModuleId {
    let c = &mut parts.controller;
    let module = c.add_module("loom.math.constant").unwrap();
    c.set_param_named(module, "value", value).unwrap();
    module
}

/// An Add whose output goes to the audio output: a window on its `a` input.
fn window(parts: &mut EngineParts) -> ModuleId {
    let c = &mut parts.controller;
    let add = c.add_module("loom.math.add").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(add, "out", out, "in").unwrap();
    add
}

/// The largest change from one sample to the next.
fn largest_step(signal: &[f32]) -> f32 {
    signal.windows(2).fold(0.0, |step, w| step.max((w[1] - w[0]).abs()))
}

/// Rule 9's first criterion: a running sine into a filter, its cable
/// pulled, and the filter's input ramps from the last value to 0 over
/// 5 ms, the same at every block size. A window on the same cable shows
/// the input exactly, sample by sample. The filter's highpass shows that
/// the step a pull would make is gone.
#[test]
fn pulling_a_sine_from_a_filter_ramps_its_input_to_zero() {
    let frames = 4_800;
    let at = 2_400;
    let mut renders = Vec::new();
    for block in BLOCKS {
        let mut parts = engine();
        let source = sine(&mut parts);
        let seen = window(&mut parts);
        let c = &mut parts.controller;
        c.connect_named(source, "out", seen, "a").unwrap();
        c.commit().unwrap();
        let input = c.input_ref(seen, "a").unwrap();
        let out = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 1);
        let held = out[at - 1];
        assert!(held.abs() > 0.1, "pulled while the sine is away from 0");
        for k in 0..frames - at {
            assert_eq!(
                out[at + k],
                faded(held, 0.0, k),
                "frame {k} after the pull, block {block}"
            );
        }
        renders.push(out);
    }
    assert!(renders.windows(2).all(|w| w[0] == w[1]), "the same at every block size");

    // The filter, on its highpass: with no fade, a pull makes the highpass
    // step by about the held input, because it passes a step whole. With
    // the fade it passes only the corner where the sine's slope meets the
    // ramp's, a small fraction of that. The render is the same at every
    // block size. The held input is the window's, from the same sine.
    let held = renders[0][at - 1].abs();
    let mut filtered = Vec::new();
    for block in BLOCKS {
        let mut parts = engine();
        let source = sine(&mut parts);
        let c = &mut parts.controller;
        let svf = c.add_module("loom.filter.svf").unwrap();
        let audio = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(source, "out", svf, "in").unwrap();
        c.connect_named(svf, "highpass", audio, "in").unwrap();
        c.commit().unwrap();
        let input = c.input_ref(svf, "in").unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        let across = largest_step(&played[at - 1..]);
        assert!(across < held / 4.0, "block {block}: {across} against a {held} step");
        filtered.push(played);
    }
    assert!(
        filtered.windows(2).all(|w| w[0] == w[1]),
        "the filter, the same at every block size"
    );
}

/// Plugging a cable fades the input in from what it read, and moving one
/// fades from the old signal's last value to the new signal.
#[test]
fn plugging_and_moving_a_cable_fade_from_the_held_value() {
    let frames = 2_000;
    let at = 1_000;
    for block in BLOCKS {
        let mut parts = engine();
        let seen = window(&mut parts);
        let low = constant(&mut parts, 0.25);
        let high = constant(&mut parts, 0.75);
        let c = &mut parts.controller;
        c.connect_named(low, "out", seen, "a").unwrap();
        c.commit().unwrap();
        let out = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(high, "out", seen, "a").unwrap();
        });
        for k in 0..frames - at {
            assert_eq!(out[at + k], faded(0.25, 0.75, k), "moved, frame {k}, block {block}");
        }

        let mut parts = engine();
        let seen = window(&mut parts);
        let high = constant(&mut parts, 0.75);
        parts.controller.commit().unwrap();
        let out = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(high, "out", seen, "a").unwrap();
        });
        for k in 0..frames - at {
            assert_eq!(out[at + k], faded(0.0, 0.75, k), "plugged, frame {k}, block {block}");
        }
    }
}

/// Rule 9's second criterion: gate, trigger, and phase inputs switch on
/// the swap frame, with no ramp.
#[test]
fn gate_trigger_and_phase_inputs_switch_on_the_swap_frame() {
    let frames = 1_000;
    let at = 500;
    for block in BLOCKS {
        // A gate: an Or of one input passes it through.
        let mut parts = engine();
        let c = &mut parts.controller;
        let logic = c.add_module("loom.logic.logic").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(logic, "out", out, "in").unwrap();
        c.set_param_named(logic, "op", 1.0).unwrap();
        let low = constant(&mut parts, 0.0);
        let high = constant(&mut parts, 1.0);
        parts.controller.connect_named(low, "out", logic, "in1").unwrap();
        parts.controller.commit().unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(high, "out", logic, "in1").unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 0, "a gate input switches");
        assert_eq!(played[at - 1], 0.0);
        assert!(
            played[at..].iter().all(|&s| s == 1.0),
            "high from the swap frame, block {block}"
        );

        // A trigger: a phase generator's reset counts the rising edge on the
        // swap frame, not 2.5 ms later when a ramp would cross 0.5.
        let mut parts = engine();
        let c = &mut parts.controller;
        let phase = c.add_module("loom.phase.generator").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(phase, "phase", out, "in").unwrap();
        let low = constant(&mut parts, 0.0);
        let high = constant(&mut parts, 1.0);
        parts.controller.connect_named(low, "out", phase, "reset").unwrap();
        parts.controller.commit().unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(high, "out", phase, "reset").unwrap();
        });
        assert_eq!(
            parts.controller.last_report().cable_fades,
            0,
            "a trigger input switches"
        );
        assert!(
            played[at] < played[at - 1],
            "the phase resets on the swap frame, block {block}"
        );

        // A phase: a sine reads its new phase from the swap frame.
        let mut parts = engine();
        let c = &mut parts.controller;
        let sine = c.add_module("loom.osc.sine").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(sine, "out", out, "in").unwrap();
        let start = constant(&mut parts, 0.0);
        let quarter = constant(&mut parts, 0.25);
        parts.controller.connect_named(start, "out", sine, "phase").unwrap();
        parts.controller.commit().unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(quarter, "out", sine, "phase").unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 0, "a phase input switches");
        let mut ideal = engine();
        let c = &mut ideal.controller;
        let sine = c.add_module("loom.osc.sine").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(sine, "out", out, "in").unwrap();
        let quarter = constant(&mut ideal, 0.25);
        ideal.controller.connect_named(quarter, "out", sine, "phase").unwrap();
        ideal.controller.commit().unwrap();
        let (ideal, _) = render(&mut ideal, frames, block, &[]);
        assert_eq!(
            played[at..],
            ideal[at..],
            "the new phase from the swap frame, block {block}"
        );
    }
}

/// Rule 9's third criterion: an added assignment ramps its contribution in
/// over 5 ms, a removed one ramps out, and a moved source fades from the
/// old contribution to the new one.
#[test]
fn assignments_ramp_their_contribution_in_and_out() {
    let frames = 2_000;
    let at = 1_000;
    let depth = 0.25;
    let offset_index = loom_modules::math::ADD.param_index("offset").unwrap();
    let spec = {
        let mut parts = engine();
        let seen = window(&mut parts);
        parts.controller.graph().node(seen).unwrap().spec(offset_index).unwrap()
    };
    // The offset an assignment of `contribution` gives, through the
    // parameter's 0 to 1 position, as fills add it.
    let offset = |contribution: f32| spec.denormalize(spec.normalize(0.0) + contribution);
    for block in BLOCKS {
        // Added.
        let mut parts = engine();
        let seen = window(&mut parts);
        let source = constant(&mut parts, 1.0);
        parts.controller.commit().unwrap();
        let mut id = None;
        let out = render_changing(&mut parts, frames, at, block, |parts| {
            id = Some(
                parts
                    .controller
                    .add_assignment_named(source, "out", seen, "offset", depth)
                    .unwrap(),
            );
        });
        assert_eq!(parts.controller.last_report().cable_fades, 1);
        assert_eq!(out[at - 1], 0.0);
        for k in 1..frames - at {
            let expected = offset(faded(0.0, depth, k));
            assert!(
                (out[at + k] - expected).abs() <= 1e-6,
                "added, frame {k}, block {block}: {} != {expected}",
                out[at + k]
            );
        }

        assert!(id.is_some());

        // Removed: the last contribution ramps out to the base value.
        let mut parts = engine();
        let seen = window(&mut parts);
        let source = constant(&mut parts, 1.0);
        let id = parts
            .controller
            .add_assignment_named(source, "out", seen, "offset", depth)
            .unwrap();
        parts.controller.commit().unwrap();
        let out = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.remove_assignment(id).unwrap();
        });
        assert!((out[at - 1] - offset(depth)).abs() <= 1e-6);
        for k in 1..frames - at {
            let expected = offset(faded(depth, 0.0, k));
            assert!(
                (out[at + k] - expected).abs() <= 1e-6,
                "removed, frame {k}, block {block}: {} != {expected}",
                out[at + k]
            );
        }
        assert_eq!(out[frames - 1], 0.0, "the base value once the ramp ends");

        // Moved to another source.
        let mut parts = engine();
        let seen = window(&mut parts);
        let one = constant(&mut parts, 1.0);
        let minus = constant(&mut parts, -1.0);
        let id = parts
            .controller
            .add_assignment_named(one, "out", seen, "offset", depth)
            .unwrap();
        parts.controller.commit().unwrap();
        let param = offset_index;
        let to = parts.controller.output_ref(minus, "out").unwrap();
        let out = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.retarget_assignment(id, to, seen, param).unwrap();
        });
        for k in 1..frames - at {
            let expected = offset(faded(depth, -depth, k));
            assert!(
                (out[at + k] - expected).abs() <= 1e-6,
                "moved, frame {k}, block {block}: {} != {expected}",
                out[at + k]
            );
        }
    }
}

/// A whole-number or choice parameter switches: a ramp across separate
/// values would add steps.
#[test]
fn an_assignment_onto_a_choice_switches() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let logic = c.add_module("loom.logic.logic").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(logic, "out", out, "in").unwrap();
    let source = constant(&mut parts, 1.0);
    parts.controller.commit().unwrap();
    render(&mut parts, 64, 64, &[]);
    parts
        .controller
        .add_assignment_named(source, "out", logic, "op", 1.0)
        .unwrap();
    assert_eq!(parts.controller.commit().unwrap().cable_fades, 0);
}

/// Rule 9's fourth criterion: nothing fades on the first schedule, nor on a
/// recompile that changes no cable, nor before the engine has played.
#[test]
fn nothing_fades_on_load_or_without_a_cable_change() {
    let mut parts = engine();
    let source = sine(&mut parts);
    let seen = window(&mut parts);
    parts.controller.connect_named(source, "out", seen, "a").unwrap();
    assert_eq!(parts.controller.commit().unwrap().cable_fades, 0, "first schedule");
    let output = parts.controller.output_ref(source, "out").unwrap();
    parts
        .controller
        .add_probe(output, loom_core::ProbeKind::MeterAndScope)
        .unwrap();
    assert_eq!(
        parts.controller.commit().unwrap().cable_fades,
        0,
        "a probe is not a cable"
    );

    // Edits before the first block fade nothing in the engine, so a patch
    // plays the same however many commits it took to load.
    let build = |edit: bool| {
        let mut parts = engine();
        let source = sine(&mut parts);
        let seen = window(&mut parts);
        if edit {
            parts.controller.commit().unwrap();
        }
        parts.controller.connect_named(source, "out", seen, "a").unwrap();
        parts.controller.commit().unwrap();
        render(&mut parts, 1_000, 64, &[]).0
    };
    assert_eq!(build(true), build(false));
}

/// A swap while a fade runs: an unrelated edit lets the fade go on exactly,
/// and a second change starts a new fade from the faded value, with no
/// step.
#[test]
fn a_swap_during_a_fade_continues_it() {
    let frames = 2_000;
    let at = 1_000;
    for block in BLOCKS {
        let build = || {
            let mut parts = engine();
            let seen = window(&mut parts);
            let high = constant(&mut parts, 0.75);
            parts.controller.connect_named(high, "out", seen, "a").unwrap();
            parts.controller.commit().unwrap();
            (parts, seen)
        };
        let (mut parts, seen) = build();
        let input = parts.controller.input_ref(seen, "a").unwrap();
        let once = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });

        let (mut parts, seen) = build();
        let input = parts.controller.input_ref(seen, "a").unwrap();
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        let mut position = 0;
        for (edit, until) in [(0, at), (1, at + 100), (2, frames)] {
            match edit {
                1 => {
                    parts.controller.disconnect(input).unwrap();
                    parts.controller.commit().unwrap();
                }
                2 => {
                    parts.controller.add_module("loom.osc.noise").unwrap();
                    parts.controller.commit().unwrap();
                }
                _ => {}
            }
            while position < until {
                let n = block.min(until - position);
                parts
                    .engine
                    .process(&mut left[position..position + n], &mut right[position..position + n]);
                position += n;
            }
        }
        assert_eq!(left, once, "an unrelated swap keeps the fade, block {block}");

        // A second change 100 frames in starts from the faded value.
        let (mut parts, seen) = build();
        let low = constant(&mut parts, 0.25);
        let input = parts.controller.input_ref(seen, "a").unwrap();
        let first = render_changing(&mut parts, at + 100, at, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        let mut rest = vec![0.0; 1_000];
        let mut right = vec![0.0; 1_000];
        parts.controller.connect_named(low, "out", seen, "a").unwrap();
        parts.controller.commit().unwrap();
        let mut position = 0;
        while position < rest.len() {
            let n = block.min(rest.len() - position);
            parts
                .engine
                .process(&mut rest[position..position + n], &mut right[position..position + n]);
            position += n;
        }
        let held = first[at + 99];
        assert_eq!(rest[0], held, "the new fade starts from the faded value, block {block}");
        for (k, &value) in rest.iter().enumerate() {
            assert_eq!(value, faded(held, 0.25, k));
        }
    }
}

/// Pulling the cable to the audio output fades the output to 0, with output
/// safety off and a fixed value, where nothing else would soften the step.
#[test]
fn pulling_the_output_cable_fades_the_output_out() {
    let frames = 1_000;
    let at = 500;
    for block in BLOCKS {
        let mut parts = engine();
        let c = &mut parts.controller;
        let out = c.add_module("loom.io.audio-out").unwrap();
        let dc = constant(&mut parts, 0.8);
        parts.controller.connect_named(dc, "out", out, "in").unwrap();
        parts.controller.commit().unwrap();
        let input = parts.controller.input_ref(out, "in").unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        assert!(
            largest_step(&played) <= 0.8 / fade_length() as f32 + 1e-6,
            "block {block}"
        );
        for k in 0..frames - at {
            assert_eq!(played[at + k], faded(0.8, 0.0, k));
        }
    }
}

/// Two swaps before one block: nothing is read from buffers that never ran,
/// and what plays stays finite and starts from the held value.
#[test]
fn two_swaps_before_one_block_stay_finite() {
    let mut parts = engine();
    let seen = window(&mut parts);
    let high = constant(&mut parts, 0.75);
    let low = constant(&mut parts, 0.25);
    parts.controller.connect_named(high, "out", seen, "a").unwrap();
    parts.controller.commit().unwrap();
    let (before, _) = render(&mut parts, 512, 512, &[]);
    parts.controller.connect_named(low, "out", seen, "a").unwrap();
    parts.controller.commit().unwrap();
    let input = parts.controller.input_ref(seen, "a").unwrap();
    parts.controller.disconnect(input).unwrap();
    parts.controller.commit().unwrap();
    let (after, _) = render(&mut parts, 512, 512, &[]);
    assert!(after.iter().all(|s| s.is_finite()));
    assert_eq!(after[0], before[511], "the first fade's held value");
    assert!(largest_step(&after) <= 0.75 / fade_length() as f32 + 1e-6);
}

/// The same edit twice gives the same samples (hard rule 3).
#[test]
fn a_faded_edit_is_deterministic() {
    let run = || {
        let mut parts = engine();
        let source = sine(&mut parts);
        let seen = window(&mut parts);
        parts.controller.connect_named(source, "out", seen, "a").unwrap();
        parts.controller.commit().unwrap();
        let input = parts.controller.input_ref(seen, "a").unwrap();
        render_changing(&mut parts, 2_000, 777, 64, |parts| {
            parts.controller.disconnect(input).unwrap();
        })
    };
    assert_eq!(run(), run());
}

/// A mixer's inputs carry any signal, so what fades follows the first
/// source past the mixers: a gate through a Sum switches, into a gate input
/// or not. A Select's member input never fades, because a ramp would walk
/// it through the wrong members (rule 9, The mixers the editor adds).
#[test]
fn mixers_fade_by_what_they_carry() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let gate = c.add_module("loom.logic.logic").unwrap();
    let sum = c.add_module("loom.struct.sum").unwrap();
    let inner = c.add_module("loom.struct.sum").unwrap();
    let seen = c.add_module("loom.math.add").unwrap();
    let logic = c.add_module("loom.logic.logic").unwrap();
    let select = c.add_module("loom.struct.select").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    for (module, port) in [(seen, "out"), (logic, "out"), (select, "out")] {
        let mix = c.add_module("loom.math.add").unwrap();
        c.connect_named(module, port, mix, "a").unwrap();
        c.connect_named(mix, "out", out, "in").unwrap();
    }
    c.connect_named(gate, "out", inner, "in1").unwrap();
    c.connect_named(inner, "out", sum, "in1").unwrap();
    c.commit().unwrap();
    render(&mut parts, 64, 64, &[]);
    let c = &mut parts.controller;
    c.connect_named(sum, "out", seen, "a").unwrap();
    assert_eq!(
        c.commit().unwrap().cable_fades,
        0,
        "a gate through nested Sums switches"
    );
    c.connect_named(sum, "out", logic, "in1").unwrap();
    assert_eq!(c.commit().unwrap().cable_fades, 0, "a gate input switches");
    let member = constant(&mut parts, 2.0);
    parts.controller.connect_named(member, "out", select, "member").unwrap();
    assert_eq!(
        parts.controller.commit().unwrap().cable_fades,
        0,
        "the member never fades"
    );
}

/// The inputs that read 1 with no cable (Amp `level`, Multiply `b`, Divide
/// `b`) must read that 1 from their port default, or a plugged cable would
/// start its fade from 0: a drop to silence on a VCA, and a burst far over
/// full scale on a Divide (live patching spec, rule 9; decision M1 of the
/// #21 plan). These fail until the normals live in the descriptors (#700).
#[test]
fn inputs_that_read_1_unplugged_fade_from_1() {
    for (kind, port) in [
        ("loom.level.amp", "level"),
        ("loom.math.mul", "b"),
        ("loom.math.div", "b"),
    ] {
        let mut parts = engine();
        let c = &mut parts.controller;
        let module = c.add_module(kind).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        let first = c.graph().node(module).unwrap().descriptor.inputs[0].name;
        c.connect_named(module, "out", out, "in").unwrap();
        let signal = constant(&mut parts, 0.5);
        let two = constant(&mut parts, 2.0);
        parts.controller.connect_named(signal, "out", module, first).unwrap();
        parts.controller.commit().unwrap();
        let played = render_changing(&mut parts, 1_000, 500, 64, |parts| {
            parts.controller.connect_named(two, "out", module, port).unwrap();
        });
        let before = played[499];
        let after = played[999];
        let step = largest_step(&played[499..]);
        assert!(
            step <= (after - before).abs() / fade_length() as f32 + 1e-6,
            "{kind} {port}: the fade starts from the unplugged value, {before}, not from 0 (step {step})"
        );
        // Pulled again: back to the unplugged value, in a straight line.
        let input = parts.controller.input_ref(module, port).unwrap();
        let pulled = render_changing(&mut parts, 1_000, 500, 64, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        let step = largest_step(&pulled[499..]);
        assert!(
            step <= (pulled[999] - pulled[499]).abs() / fade_length() as f32 + 1e-6,
            "{kind} {port}: pulling fades back to the unplugged value (step {step})"
        );
        assert_eq!(pulled[999], before, "{kind} {port}: the unplugged value");
    }
}

/// Renders as [`render_changing`] and returns both sides.
fn render_changing_stereo(
    parts: &mut EngineParts,
    frames: usize,
    at: usize,
    block: usize,
    change: impl FnOnce(&mut EngineParts),
) -> (Vec<f32>, Vec<f32>) {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut run = |parts: &mut EngineParts, from: usize, to: usize| {
        let mut position = from;
        while position < to {
            let n = block.min(to - position);
            parts
                .engine
                .process(&mut left[position..position + n], &mut right[position..position + n]);
            position += n;
        }
    };
    run(parts, 0, at);
    change(parts);
    parts.controller.commit().unwrap();
    run(parts, at, frames);
    (left, right)
}

/// The right side of a stereo input that loses its stereo cable fades to
/// what a mono cable gives, the left signal, instead of jumping to it.
#[test]
fn a_stereo_input_s_right_side_fades_when_its_stereo_cable_goes() {
    let frames = 1_000;
    let at = 500;
    for block in BLOCKS {
        let build = || {
            let mut parts = engine();
            let left = constant(&mut parts, 0.3);
            let right = constant(&mut parts, -0.5);
            let mono = constant(&mut parts, 0.2);
            let c = &mut parts.controller;
            let join = c.add_module("loom.mix.join").unwrap();
            let split = c.add_module("loom.mix.split").unwrap();
            let rejoin = c.add_module("loom.mix.join").unwrap();
            let out = c.add_module("loom.io.audio-out").unwrap();
            c.connect_named(left, "out", join, "left").unwrap();
            c.connect_named(right, "out", join, "right").unwrap();
            c.connect_named(join, "out", split, "in").unwrap();
            c.connect_named(split, "left", rejoin, "left").unwrap();
            c.connect_named(split, "right", rejoin, "right").unwrap();
            c.connect_named(rejoin, "out", out, "in").unwrap();
            c.commit().unwrap();
            (parts, split, mono)
        };
        let (mut parts, split, mono) = build();
        let (left, right) = render_changing_stereo(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(mono, "out", split, "in").unwrap();
        });
        for k in 0..frames - at {
            assert_eq!(left[at + k], faded(0.3, 0.2, k), "left, frame {k}, block {block}");
            assert_eq!(right[at + k], faded(-0.5, 0.2, k), "right, frame {k}, block {block}");
        }
        let (mut parts, split, _) = build();
        let input = parts.controller.input_ref(split, "in").unwrap();
        let (_, right) = render_changing_stereo(&mut parts, frames, at, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        for k in 0..frames - at {
            assert_eq!(
                right[at + k],
                faded(-0.5, 0.0, k),
                "pulled, right, frame {k}, block {block}"
            );
        }
    }
}

/// A stereo source with different sides plugged into a running mono
/// filter: the new right copy takes copy 0's state and its fade, so both
/// sides move with no step larger than the sound on either side allows.
#[test]
fn a_stereo_source_into_a_running_mono_filter_fades_both_sides() {
    let frames = 4_800;
    let at = 2_400;
    for block in BLOCKS {
        let mut parts = engine();
        let source = sine(&mut parts);
        let c = &mut parts.controller;
        let saw = c.add_module("loom.osc.saw").unwrap();
        let join = c.add_module("loom.mix.join").unwrap();
        let svf = c.add_module("loom.filter.svf").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(source, "out", join, "left").unwrap();
        c.connect_named(saw, "out", join, "right").unwrap();
        c.connect_named(source, "out", svf, "in").unwrap();
        c.connect_named(svf, "lowpass", out, "in").unwrap();
        let probe = c.output_ref(join, "out").unwrap();
        c.add_probe(probe, loom_core::ProbeKind::Meter).unwrap();
        c.commit().unwrap();
        let (left, right) = render_changing_stereo(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(join, "out", svf, "in").unwrap();
        });
        assert!(parts.controller.last_report().runs_per_channel(svf));
        let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let limit = |s: &[f32]| (largest_step(s) + 2.0 * peak(s) / fade_length() as f32) * 1.1 + 1e-6;
        for (side, played) in [("left", &left), ("right", &right)] {
            let bound = limit(&played[at - 1_000..at]).max(limit(&played[frames - 1_000..]));
            let across = largest_step(&played[at - 1..at + 2 * fade_length()]);
            assert!(across <= bound, "{side}, block {block}: {across} > {bound}");
        }
    }
}

/// A 4-voice cable into the audio output moved to a mono source: voice 0
/// fades to the new signal, and the voices only the old cable had fade out
/// to silence, instead of stopping on one frame.
#[test]
fn voices_the_output_loses_fade_out() {
    let mut registry = loom_core::Registry::new();
    loom_modules::register_all(&mut registry).unwrap();
    loom_modules::test_modules::register(&mut registry).unwrap();
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(std::sync::Arc::new(registry), config);
    let c = &mut parts.controller;
    let voices = c.add_module("test.voices").unwrap();
    c.set_param_named(voices, "voices", 4.0).unwrap();
    c.set_param_named(voices, "value", 0.1).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(voices, "out", out, "in").unwrap();
    let mono = constant(&mut parts, 0.2);
    parts.controller.commit().unwrap();
    let played = render_changing(&mut parts, 1_000, 500, 64, |parts| {
        parts.controller.connect_named(mono, "out", out, "in").unwrap();
    });
    let before = played[499];
    assert!((before - 0.46).abs() < 1e-5, "four voices sum: {before}");
    for k in 0..500 {
        let expected = faded(0.1, 0.2, k) + faded(0.11, 0.0, k) + faded(0.12, 0.0, k) + faded(0.13, 0.0, k);
        assert!(
            (played[500 + k] - expected).abs() < 1e-6,
            "frame {k}: {} != {expected}",
            played[500 + k]
        );
    }
}

/// A gate plugged straight into an input typed for modulation fades, by
/// the port's own type (rule 9), and a cable moved from a gate to a value
/// on that input fades too. A gate is a full-scale step there.
#[test]
fn a_gate_into_a_modulation_input_fades() {
    let frames = 1_000;
    let at = 500;
    for block in BLOCKS {
        let mut parts = engine();
        let seen = window(&mut parts);
        let high = constant(&mut parts, 1.0);
        let c = &mut parts.controller;
        let gate = c.add_module("loom.logic.logic").unwrap();
        c.set_param_named(gate, "op", 1.0).unwrap();
        c.connect_named(high, "out", gate, "in1").unwrap();
        let probe = c.output_ref(gate, "out").unwrap();
        c.add_probe(probe, loom_core::ProbeKind::Meter).unwrap();
        c.commit().unwrap();
        let out = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(gate, "out", seen, "a").unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 1, "block {block}");
        for k in 0..frames - at {
            assert_eq!(out[at + k], faded(0.0, 1.0, k), "plugged, frame {k}, block {block}");
        }
        let low = constant(&mut parts, 0.25);
        parts.controller.connect_named(low, "out", seen, "a").unwrap();
        assert_eq!(parts.controller.commit().unwrap().cable_fades, 1, "moved from a gate");
    }
}

/// A Divide's B moved across 0 divides by small numbers for a moment: the
/// output stays finite and, with output safety off, inside full scale,
/// because the engine clips only the final output (rule 9, What still
/// steps).
#[test]
fn a_divide_moved_across_zero_stays_finite() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let divide = c.add_module("loom.math.div").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(divide, "out", out, "in").unwrap();
    let source = sine(&mut parts);
    let plus = constant(&mut parts, 1.0);
    let minus = constant(&mut parts, -1.0);
    parts.controller.connect_named(source, "out", divide, "a").unwrap();
    parts.controller.connect_named(plus, "out", divide, "b").unwrap();
    parts.controller.commit().unwrap();
    let played = render_changing(&mut parts, 2_000, 1_000, 64, |parts| {
        parts.controller.connect_named(minus, "out", divide, "b").unwrap();
    });
    assert!(played.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
}

/// A Unit Delay's input fades like any input; the delay plays the ramp one
/// sample later.
#[test]
fn a_unit_delay_input_fades() {
    for block in BLOCKS {
        let mut parts = engine();
        let c = &mut parts.controller;
        let delay = c.add_module("loom.time.delay1").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(delay, "out", out, "in").unwrap();
        let source = constant(&mut parts, 0.8);
        parts.controller.connect_named(source, "out", delay, "in").unwrap();
        parts.controller.commit().unwrap();
        let input = parts.controller.input_ref(delay, "in").unwrap();
        let played = render_changing(&mut parts, 1_000, 500, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        assert_eq!(played[500], 0.8, "the delay's held sample");
        for k in 0..499 {
            assert_eq!(played[501 + k], faded(0.8, 0.0, k), "frame {k}, block {block}");
        }
    }
}

/// A module that resumes after it was skipped read nothing while paused,
/// so a cable plugged into it as it resumes fades from 0.
#[test]
fn a_resumed_module_fades_from_zero() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let add = c.add_module("loom.math.add").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    let source = constant(&mut parts, 0.5);
    let report = parts.controller.commit().unwrap();
    assert!(report.skipped.contains(&add));
    let played = render_changing(&mut parts, 1_000, 500, 64, |parts| {
        parts.controller.connect_named(source, "out", add, "a").unwrap();
        parts.controller.connect_named(add, "out", out, "in").unwrap();
    });
    for k in 0..500 {
        // Both the Add's input and the output's input fade from 0.
        assert_eq!(played[500 + k], faded(0.0, faded(0.0, 0.5, k), k), "frame {k}");
    }
}

/// A stereo source with different sides pulled from the audio output: each
/// side fades out from its own value.
#[test]
fn a_stereo_source_pulled_from_the_output_fades_each_side() {
    for block in BLOCKS {
        let mut parts = engine();
        let left = constant(&mut parts, 0.3);
        let right = constant(&mut parts, -0.5);
        let c = &mut parts.controller;
        let join = c.add_module("loom.mix.join").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(left, "out", join, "left").unwrap();
        c.connect_named(right, "out", join, "right").unwrap();
        c.connect_named(join, "out", out, "in").unwrap();
        c.commit().unwrap();
        let input = c.input_ref(out, "in").unwrap();
        let (l, r) = render_changing_stereo(&mut parts, 1_000, 500, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        for k in 0..500 {
            assert_eq!(l[500 + k], faded(0.3, 0.0, k), "left, frame {k}, block {block}");
            assert_eq!(r[500 + k], faded(-0.5, 0.0, k), "right, frame {k}, block {block}");
        }
    }
}

/// A 1-voice cable into the audio output moved to 4 voices: each new voice
/// fades in from 0, so the first frame plays the held value, not four
/// times it (rule 9).
#[test]
fn voices_the_output_gains_fade_in_from_zero() {
    let mut registry = loom_core::Registry::new();
    loom_modules::register_all(&mut registry).unwrap();
    loom_modules::test_modules::register(&mut registry).unwrap();
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(std::sync::Arc::new(registry), config);
    let c = &mut parts.controller;
    let voices = c.add_module("test.voices").unwrap();
    c.set_param_named(voices, "voices", 4.0).unwrap();
    c.set_param_named(voices, "value", 0.1).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    let mono = constant(&mut parts, 0.2);
    parts.controller.connect_named(mono, "out", out, "in").unwrap();
    let probe = parts.controller.output_ref(voices, "out").unwrap();
    parts.controller.add_probe(probe, loom_core::ProbeKind::Meter).unwrap();
    parts.controller.commit().unwrap();
    let played = render_changing(&mut parts, 1_000, 500, 64, |parts| {
        parts.controller.connect_named(voices, "out", out, "in").unwrap();
    });
    assert_eq!(played[500], 0.2, "the held value, not four times it");
    for k in 0..500 {
        let expected = faded(0.2, 0.1, k) + faded(0.0, 0.11, k) + faded(0.0, 0.12, k) + faded(0.0, 0.13, k);
        assert!((played[500 + k] - expected).abs() < 1e-6, "frame {k}");
    }
    assert!(largest_step(&played[499..]) <= (0.46 - 0.2) / fade_length() as f32 + 1e-6);
}

/// The mixers at the graph level, as the flattener wires them (rule 9, The
/// mixers the editor adds).
mod mixers {
    use super::*;

    /// A source, a window on it at the output, and the window's input.
    fn played_through(parts: &mut EngineParts, value: f32) -> (ModuleId, loom_core::InputRef) {
        let source = constant(parts, value);
        let seen = window(parts);
        parts.controller.connect_named(source, "out", seen, "a").unwrap();
        let input = parts.controller.input_ref(seen, "a").unwrap();
        (source, input)
    }

    /// A Layer that adds a member: the cable moves onto a new Sum at gain 1
    /// that holds its old source, so it switches, and the new member fades
    /// in from 0 (rule B).
    #[test]
    fn a_layer_adding_a_member_switches_and_fades_the_member_in() {
        for block in BLOCKS {
            let mut parts = engine();
            let (first, input) = played_through(&mut parts, 0.25);
            let second = constant(&mut parts, 0.5);
            parts.controller.commit().unwrap();
            let out = render_changing(&mut parts, 1_000, 500, block, |parts| {
                let c = &mut parts.controller;
                let sum = c.add_module("loom.struct.sum").unwrap();
                c.connect_named(first, "out", sum, "in1").unwrap();
                c.connect_named(second, "out", sum, "in2").unwrap();
                c.connect(c.output_ref(sum, "out").unwrap(), input).unwrap();
            });
            assert_eq!(
                parts.controller.last_report().cable_fades,
                1,
                "only the new member fades"
            );
            for k in 0..500 {
                assert_eq!(out[500 + k], 0.25 + faded(0.0, 0.5, k), "frame {k}, block {block}");
            }
        }
    }

    /// Layer of one member back to Single, and Select back to Single: the
    /// mixer that leaves played its source alone, so the cable switches and
    /// nothing changes (rule A, leaving side).
    #[test]
    fn a_mixer_that_played_one_source_alone_leaves_with_no_fade() {
        for kind in ["loom.struct.sum", "loom.struct.select"] {
            let mut parts = engine();
            let (source, input) = played_through(&mut parts, 0.25);
            let other = constant(&mut parts, -0.5);
            let c = &mut parts.controller;
            let mixer = c.add_module(kind).unwrap();
            c.connect_named(source, "out", mixer, "in1").unwrap();
            if kind == "loom.struct.select" {
                c.connect_named(other, "out", mixer, "in2").unwrap();
                c.set_param_named(mixer, "members", 2.0).unwrap();
            }
            c.connect(c.output_ref(mixer, "out").unwrap(), input).unwrap();
            c.commit().unwrap();
            let out = render_changing(&mut parts, 1_000, 500, 64, |parts| {
                parts.controller.remove_module(mixer).unwrap();
                let from = parts.controller.output_ref(source, "out").unwrap();
                parts.controller.connect(from, input).unwrap();
            });
            assert_eq!(parts.controller.last_report().cable_fades, 0, "{kind}");
            // From the end of the output's 10 ms fade-in on.
            assert!(out[480..].iter().all(|&v| v == 0.25), "{kind} changes nothing");
        }
    }

    /// A new Select whose Member picks member 3 plays it from its first
    /// frame: the cable moves from that member onto the Select, so it
    /// switches, and the Select's member input is never faded.
    #[test]
    fn a_new_select_plays_its_chosen_member_from_the_first_frame() {
        let mut parts = engine();
        let (third, input) = played_through(&mut parts, 0.75);
        let first = constant(&mut parts, 0.25);
        let second = constant(&mut parts, 0.5);
        parts.controller.commit().unwrap();
        let out = render_changing(&mut parts, 1_000, 500, 64, |parts| {
            let c = &mut parts.controller;
            let select = c.add_module("loom.struct.select").unwrap();
            let member = c.add_module("loom.struct.member").unwrap();
            c.set_param_named(select, "members", 3.0).unwrap();
            c.set_param_named(member, "member", 3.0).unwrap();
            for (source, port) in [(first, "in1"), (second, "in2"), (third, "in3")] {
                c.connect_named(source, "out", select, port).unwrap();
            }
            c.connect_named(member, "out", select, "member").unwrap();
            c.connect(c.output_ref(select, "out").unwrap(), input).unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 0);
        // From the end of the output's 10 ms fade-in on.
        assert!(out[480..].iter().all(|&v| v == 0.75), "member 3 from the first frame");
    }

    /// Changes of mixer that change the sound fade the cable once: two
    /// copies to one (a Sum that played two sources leaves), and Layer to
    /// Select (a Sum leaves and a Select arrives). The new mixer's own
    /// inputs are not faded, so there is no fade into a fade.
    #[test]
    fn a_real_change_of_mixer_fades_once() {
        for to_select in [false, true] {
            let mut parts = engine();
            let (first, input) = played_through(&mut parts, 0.25);
            let second = constant(&mut parts, 0.5);
            let c = &mut parts.controller;
            let sum = c.add_module("loom.struct.sum").unwrap();
            c.set_param_named(sum, "gain", std::f32::consts::FRAC_1_SQRT_2).unwrap();
            c.connect_named(first, "out", sum, "in1").unwrap();
            c.connect_named(second, "out", sum, "in2").unwrap();
            c.connect(c.output_ref(sum, "out").unwrap(), input).unwrap();
            c.commit().unwrap();
            let out = render_changing(&mut parts, 1_000, 500, 64, |parts| {
                let c = &mut parts.controller;
                c.remove_module(sum).unwrap();
                if to_select {
                    let select = c.add_module("loom.struct.select").unwrap();
                    c.set_param_named(select, "members", 2.0).unwrap();
                    c.connect_named(first, "out", select, "in1").unwrap();
                    c.connect_named(second, "out", select, "in2").unwrap();
                    c.connect(c.output_ref(select, "out").unwrap(), input).unwrap();
                } else {
                    c.connect(c.output_ref(first, "out").unwrap(), input).unwrap();
                }
            });
            assert_eq!(parts.controller.last_report().cable_fades, 1, "to select: {to_select}");
            let held = out[499];
            for k in 0..500 {
                assert_eq!(out[500 + k], faded(held, 0.25, k), "frame {k}, to select: {to_select}");
            }
        }
    }
}

/// An assignment moved to another target ramps out on the old parameter
/// and in on the new one, on the same frames.
#[test]
fn an_assignment_moved_to_another_target_ramps_out_and_in() {
    let mut parts = engine();
    let source = constant(&mut parts, 1.0);
    let c = &mut parts.controller;
    let first = c.add_module("loom.math.add").unwrap();
    let second = c.add_module("loom.math.add").unwrap();
    let join = c.add_module("loom.mix.join").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(first, "out", join, "left").unwrap();
    c.connect_named(second, "out", join, "right").unwrap();
    c.connect_named(join, "out", out, "in").unwrap();
    let id = c.add_assignment_named(source, "out", first, "offset", 0.25).unwrap();
    c.commit().unwrap();
    let param = loom_modules::math::ADD.param_index("offset").unwrap();
    let spec = parts.controller.graph().node(first).unwrap().spec(param).unwrap();
    let offset = |contribution: f32| spec.denormalize(spec.normalize(0.0) + contribution);
    let from = parts.controller.output_ref(source, "out").unwrap();
    let (left, right) = render_changing_stereo(&mut parts, 1_000, 500, 64, |parts| {
        parts.controller.retarget_assignment(id, from, second, param).unwrap();
    });
    assert_eq!(
        parts.controller.last_report().cable_fades,
        2,
        "out on one, in on the other"
    );
    for k in 1..500 {
        assert!(
            (left[500 + k] - offset(faded(0.25, 0.0, k))).abs() <= 1e-6,
            "out, frame {k}"
        );
        assert!(
            (right[500 + k] - offset(faded(0.0, 0.25, k))).abs() <= 1e-6,
            "in, frame {k}"
        );
    }
}

/// A removed assignment's ramp goes on across a second swap 2 ms later,
/// exactly as with no second swap (rule 9, A swap while a fade runs).
#[test]
fn a_leaving_contribution_continues_across_a_second_swap() {
    let build = || {
        let mut parts = engine();
        let seen = window(&mut parts);
        let source = constant(&mut parts, 1.0);
        let id = parts
            .controller
            .add_assignment_named(source, "out", seen, "offset", 0.25)
            .unwrap();
        parts.controller.commit().unwrap();
        (parts, id)
    };
    let (mut parts, id) = build();
    let once = render_changing(&mut parts, 1_000, 500, 32, |parts| {
        parts.controller.remove_assignment(id).unwrap();
    });
    let (mut parts, id) = build();
    let first = render_changing(&mut parts, 596, 500, 32, |parts| {
        parts.controller.remove_assignment(id).unwrap();
    });
    let rest = render_changing(&mut parts, 404, 0, 32, |parts| {
        parts.controller.add_module("loom.osc.noise").unwrap();
    });
    assert_eq!([&first[..], &rest[..]].concat(), once);
}

/// A ramp on an assignment that is switched off still ends, so the engine
/// reports the swap's fades as settled.
#[test]
fn a_disabled_assignment_s_ramp_still_ends() {
    let mut parts = engine();
    let seen = window(&mut parts);
    let source = constant(&mut parts, 1.0);
    parts.controller.commit().unwrap();
    render(&mut parts, 64, 64, &[]);
    let id = parts
        .controller
        .add_assignment_named(source, "out", seen, "offset", 0.25)
        .unwrap();
    parts.controller.commit().unwrap();
    parts.controller.set_assignment_enabled(id, false).unwrap();
    render(&mut parts, 512, 64, &[]);
    let stats = parts.controller.stats();
    let settled = stats.cable_fades_settled.load(std::sync::atomic::Ordering::Relaxed);
    let swaps = stats.schedule_swaps.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(settled, swaps, "the last schedule's fades have settled");
    assert_eq!(
        parts.controller.cable_fades_in_flight(),
        0,
        "the controller keeps no fades"
    );
}

/// A panic ends every fade at its silent frame. A cable changed while the
/// panic fades out would still be fading 5 ms later; the panic's reset ends
/// it, and the engine reports the fades settled at once.
#[test]
fn a_panic_ends_every_fade() {
    let mut parts = engine();
    let seen = window(&mut parts);
    let source = constant(&mut parts, 0.8);
    parts.controller.commit().unwrap();
    render(&mut parts, 1_024, 64, &[]);
    parts.controller.panic().unwrap();
    // The panic fades the output out over 10 ms, then resets.
    render(&mut parts, 400, 16, &[]);
    parts.controller.connect_named(source, "out", seen, "a").unwrap();
    parts.controller.commit().unwrap();
    // 160 frames on: past the panic's reset, before the 240-frame fade
    // would have ended by itself.
    render(&mut parts, 160, 16, &[]);
    let stats = parts.controller.stats();
    let settled = stats.cable_fades_settled.load(std::sync::atomic::Ordering::Relaxed);
    let swaps = stats.schedule_swaps.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(settled, swaps, "the panic ended the fade");
}

/// A patch of mixers that share inputs, as a crafted file could place them:
/// twelve Sums, each with all sixteen inputs fed by the next, the last
/// empty. Finding what a cable from the first carries visits each mixer
/// once, so a cable edit compiles at once instead of walking 16^12 paths.
#[test]
fn a_cable_edit_past_many_shared_mixers_compiles_quickly() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let sums: Vec<ModuleId> = (0..12).map(|_| c.add_module("loom.struct.sum").unwrap()).collect();
    for pair in sums.windows(2) {
        for k in 1..=16 {
            c.connect_named(pair[1], "out", pair[0], &format!("in{k}")).unwrap();
        }
    }
    let seen = c.add_module("loom.math.add").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(seen, "out", out, "in").unwrap();
    c.commit().unwrap();
    render(&mut parts, 64, 64, &[]);
    let started = std::time::Instant::now();
    parts.controller.connect_named(sums[0], "out", seen, "a").unwrap();
    parts.controller.commit().unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}
