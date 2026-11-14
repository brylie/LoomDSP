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
/// the input exactly, and the filter's output makes no step larger than
/// the steady signal's plus the ramp's.
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

    for block in BLOCKS {
        let mut parts = engine();
        let source = sine(&mut parts);
        let c = &mut parts.controller;
        let svf = c.add_module("loom.filter.svf").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(source, "out", svf, "in").unwrap();
        c.connect_named(svf, "lowpass", out, "in").unwrap();
        c.commit().unwrap();
        let input = c.input_ref(svf, "in").unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        let steady = largest_step(&played[..at]);
        let ramp = played[at - 1].abs() / fade_length() as f32;
        // The filter can lift the ramp a little, so a small margin.
        assert!(
            largest_step(&played[at - 1..]) <= (steady + ramp) * 1.1,
            "block {block}: {} > {}",
            largest_step(&played[at - 1..]),
            steady + ramp
        );
    }
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
    }
}
