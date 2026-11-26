//! Live patching rule 9 (FR-GRAPH-05): a cable change fades over 5 ms.
//!
//! When a schedule swap changes what feeds an input, the input holds the
//! last value it read and moves in a straight line to the new signal over
//! 5 ms. Gate, trigger, and phase inputs switch on the swap frame. Nothing
//! fades before the engine has played its first block. An Add with offset
//! 0 is an exact pass-through, so its output shows what its input reads.
//! Spec: `docs/design/live-patching.md`, rule 9.

use crate::common;

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

/// Rule 9's second criterion: gate and trigger inputs switch on the swap
/// frame, with no ramp. A phase input switches too, and what it drives
/// fades instead: see
/// [`a_phase_cable_plugged_moved_or_pulled_fades_what_the_sine_feeds`].
#[test]
fn gate_and_trigger_inputs_switch_on_the_swap_frame() {
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

/// What a module plays for its first input and the input under test.
type Apply = fn(f32, f32) -> f32;

/// The inputs that read 1 with no cable (Amp `level`, Multiply `b`, Divide
/// `b`) read that 1 from their port default, so a cable plugged in fades
/// from 1 and a cable pulled fades back to 1 (live patching spec, rule 9;
/// decision M1 of the #21 plan). If the 1 were made in code, a plug would
/// start its fade from 0: a drop to silence on a VCA, and a burst far over
/// full scale on a Divide. Every sample is checked against the module
/// applied to the faded input, at every block size.
#[test]
fn inputs_that_read_1_unplugged_fade_from_1() {
    let a = 0.25;
    let plugged = 0.5;
    let modules: [(&str, &str, Apply); 3] = [
        ("loom.level.amp", "level", |a, level| a * level),
        ("loom.math.mul", "b", |a, b| a * b),
        ("loom.math.div", "b", |a, b| a / b),
    ];
    for (kind, port, apply) in modules {
        for block in BLOCKS {
            let mut parts = engine();
            let c = &mut parts.controller;
            let module = c.add_module(kind).unwrap();
            let out = c.add_module("loom.io.audio-out").unwrap();
            let first = c.graph().node(module).unwrap().descriptor.inputs[0].name;
            c.connect_named(module, "out", out, "in").unwrap();
            let signal = constant(&mut parts, a);
            let value = constant(&mut parts, plugged);
            parts.controller.connect_named(signal, "out", module, first).unwrap();
            parts.controller.commit().unwrap();
            let played = render_changing(&mut parts, 1_000, 500, block, |parts| {
                parts.controller.connect_named(value, "out", module, port).unwrap();
            });
            assert_eq!(played[499], apply(a, 1.0), "{kind} {port}: the unplugged value");
            assert_ne!(played[999], played[499], "{kind} {port}: the cable changes the sound");
            for k in 0..500 {
                let expected = apply(a, faded(1.0, plugged, k));
                assert!(
                    (played[500 + k] - expected).abs() <= 1e-6,
                    "{kind} {port}, plugged, frame {k}, block {block}: {} != {expected}",
                    played[500 + k]
                );
            }
            let input = parts.controller.input_ref(module, port).unwrap();
            let pulled = render_changing(&mut parts, 1_000, 500, block, |parts| {
                parts.controller.disconnect(input).unwrap();
            });
            for k in 0..500 {
                let expected = apply(a, faded(plugged, 1.0, k));
                assert!(
                    (pulled[500 + k] - expected).abs() <= 1e-6,
                    "{kind} {port}, pulled, frame {k}, block {block}: {} != {expected}",
                    pulled[500 + k]
                );
            }
        }
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

/// A Divide's B moved across 0 divides by small numbers for a moment, a
/// burst of up to a few milliseconds (rule 9, What still steps). Output
/// safety holds it under the ceiling, the Divide's own output stays
/// finite, and from 5 ms on it plays exactly `a / b` again.
#[test]
fn a_divide_moved_across_zero_stays_under_the_ceiling() {
    let at = 1_000;
    let config = EngineConfig {
        output_safety: OutputSafety::LowLatency,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let c = &mut parts.controller;
    let divide = c.add_module("loom.math.div").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(divide, "out", out, "in").unwrap();
    let source = sine(&mut parts);
    let plus = constant(&mut parts, 1.0);
    let minus = constant(&mut parts, -1.0);
    parts.controller.connect_named(source, "out", divide, "a").unwrap();
    parts.controller.connect_named(plus, "out", divide, "b").unwrap();
    let probe = parts.controller.output_ref(divide, "out").unwrap();
    parts
        .controller
        .add_probe(probe, loom_core::ProbeKind::MeterAndScope)
        .unwrap();
    parts.controller.commit().unwrap();
    let played = render_changing(&mut parts, 2_000, at, 64, |parts| {
        parts.controller.connect_named(minus, "out", divide, "b").unwrap();
    });
    assert!(played.iter().all(|x| x.abs() <= loom_core::safety::CEILING + 1e-6));
    let divided: Vec<f32> = parts
        .telemetry
        .scope()
        .flat_map(|chunk| chunk.samples().to_vec())
        .collect();
    assert!(
        divided.iter().all(|x| x.is_finite()),
        "the Divide holds its output finite"
    );

    // With safety off, the output is `-a` again from the end of the fade.
    let mut plain = engine();
    let c = &mut plain.controller;
    let divide = c.add_module("loom.math.div").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(divide, "out", out, "in").unwrap();
    let source = sine(&mut plain);
    let plus = constant(&mut plain, 1.0);
    let minus = constant(&mut plain, -1.0);
    plain.controller.connect_named(source, "out", divide, "a").unwrap();
    plain.controller.connect_named(plus, "out", divide, "b").unwrap();
    plain.controller.commit().unwrap();
    let before = render(&mut engine_with_sine(), 2_000, 64, &[]).0;
    let played = render_changing(&mut plain, 2_000, at, 64, |parts| {
        parts.controller.connect_named(minus, "out", divide, "b").unwrap();
    });
    for n in at + fade_length()..2_000 {
        assert_eq!(played[n], -before[n], "frame {n}: back to -a");
    }
}

/// A sine through a window to the output, with output safety off: what a
/// Divide's `a` reads in [`a_divide_moved_across_zero_stays_under_the_ceiling`].
fn engine_with_sine() -> EngineParts {
    let mut parts = engine();
    let source = sine(&mut parts);
    let seen = window(&mut parts);
    parts.controller.connect_named(source, "out", seen, "a").unwrap();
    parts.controller.commit().unwrap();
    parts
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
    /// in from 0 (rule B). Member 1 is a sine, so a fade of the whole cable
    /// would show as a mix with its held value.
    #[test]
    fn a_layer_adding_a_member_switches_and_fades_the_member_in() {
        let plain = render(&mut engine_with_sine(), 1_000, 64, &[]).0;
        for block in BLOCKS {
            let mut parts = engine();
            let first = sine(&mut parts);
            let seen = window(&mut parts);
            parts.controller.connect_named(first, "out", seen, "a").unwrap();
            let input = parts.controller.input_ref(seen, "a").unwrap();
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
                // The engine clips only the final output, to -1 to 1.
                let expected = (plain[500 + k] + faded(0.0, 0.5, k)).clamp(-1.0, 1.0);
                assert!(
                    (out[500 + k] - expected).abs() <= 1e-6,
                    "frame {k}, block {block}: {} != {expected}",
                    out[500 + k]
                );
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

/// An assignment onto a module that is new in this swap, as a paste or an
/// undo makes, applies from the first frame: no ramp (rule 9, Exceptions).
#[test]
fn an_assignment_onto_a_new_module_applies_at_once() {
    let mut parts = engine();
    let source = constant(&mut parts, 1.0);
    parts.controller.commit().unwrap();
    render(&mut parts, 512, 64, &[]);
    let seen = window(&mut parts);
    parts
        .controller
        .add_assignment_named(source, "out", seen, "offset", 0.25)
        .unwrap();
    assert_eq!(parts.controller.commit().unwrap().cable_fades, 0);
    let (out, _) = render(&mut parts, 512, 64, &[]);
    let param = loom_modules::math::ADD.param_index("offset").unwrap();
    let spec = parts.controller.graph().node(seen).unwrap().spec(param).unwrap();
    let modulated = spec.denormalize(spec.normalize(0.0) + 0.25);
    // Past the output's own fade-in, which a new output still has.
    assert!((out[500] - modulated).abs() <= 1e-6, "{} != {modulated}", out[500]);
}

/// An assignment from a gate source onto a continuous parameter ramps, as
/// any assignment does: the target is a value, not a gate (rule 9).
#[test]
fn an_assignment_from_a_gate_ramps() {
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
    render(&mut parts, 512, 64, &[]);
    parts
        .controller
        .add_assignment_named(gate, "out", seen, "offset", 0.25)
        .unwrap();
    assert_eq!(
        parts.controller.commit().unwrap().cable_fades,
        1,
        "the gate's contribution ramps in"
    );
}

// Assignment changes (#762): the old setting blends into the new one over
// 5 ms while the source plays (rule 9, Turned off or on). The source is a
// sine of about 200 Hz, played on the right side, so each test knows the
// sample `x_k` the engine read on every frame. The left side plays an Add
// whose offset the sine modulates, so it shows the contribution.

/// One setting of an assignment, as the test works it out from the rule.
#[derive(Clone, Copy, Debug)]
struct Setting {
    depth: f32,
    enabled: bool,
    polarity: Option<loom_core::Polarity>,
}

impl Setting {
    /// On, following the source, at `depth`.
    fn on(depth: f32) -> Self {
        Self {
            depth,
            enabled: true,
            polarity: None,
        }
    }

    /// What it makes of a bipolar source sample: the depth times the
    /// sample after the polarity's remap, or 0 when off.
    fn of(self, x: f64) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        let x = match self.polarity {
            Some(loom_core::Polarity::Unipolar) => (x + 1.0) / 2.0,
            _ => x,
        };
        f64::from(self.depth) * x
    }
}

/// The contribution the rule gives on each frame, for a setting `first`
/// and changes `(frame, setting)` in frame order. Each blend runs from
/// where the last one was on the frame of the change to the new setting.
struct Blends {
    first: Setting,
    changes: Vec<(usize, Setting)>,
}

impl Blends {
    /// The contribution on frame `k` for source sample `x`.
    fn at(&self, k: usize, x: f64) -> f64 {
        self.upto(self.changes.len(), k, x)
    }

    /// The contribution on frame `k` with only the first `n` changes.
    fn upto(&self, n: usize, k: usize, x: f64) -> f64 {
        let Some(&(start, setting)) = n.checked_sub(1).map(|i| &self.changes[i]) else {
            return self.first.of(x);
        };
        if k < start {
            return self.upto(n - 1, k, x);
        }
        let t = ((k - start) as f64 / fade_length() as f64).min(1.0);
        self.upto(n - 1, start, x) * (1.0 - t) + setting.of(x) * t
    }
}

/// A sine of about 200 Hz on the right side, assigned at `depth` to the
/// offset of an Add on the left side. Returns the assignment.
fn blend_patch(parts: &mut EngineParts, depth: f32) -> loom_core::AssignmentId {
    blend_patch_on(parts, depth, 0.0)
}

/// [`blend_patch`] with the Add's offset at `base`. An offset of 0 comes
/// back exactly through the parameter's 0 to 1 position; most others do
/// not, so a fill that adds even 0 changes the base bits.
fn blend_patch_on(parts: &mut EngineParts, depth: f32, base: f32) -> loom_core::AssignmentId {
    let c = &mut parts.controller;
    let phase = c.add_module("loom.phase.generator").unwrap();
    // 12 · log2(200 / 261.63) semitones below middle C.
    c.set_param_named(phase, "tune", -4.65).unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    c.connect_named(phase, "phase", sine, "phase").unwrap();
    let seen = c.add_module("loom.math.add").unwrap();
    c.set_param_named(seen, "offset", base).unwrap();
    let join = c.add_module("loom.mix.join").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(seen, "out", join, "left").unwrap();
    c.connect_named(sine, "out", join, "right").unwrap();
    c.connect_named(join, "out", out, "in").unwrap();
    let id = c.add_assignment_named(sine, "out", seen, "offset", depth).unwrap();
    c.commit().unwrap();
    id
}

/// The Add offset a contribution gives, through the parameter's 0 to 1
/// position, as fills add it.
fn offset_of(contribution: f64) -> f32 {
    let param = loom_modules::math::ADD.param_index("offset").unwrap();
    let mut parts = engine();
    let seen = window(&mut parts);
    let spec = parts.controller.graph().node(seen).unwrap().spec(param).unwrap();
    spec.denormalize(spec.normalize(0.0) + contribution as f32)
}

/// A change made between blocks, just before its frame.
type Change = Box<dyn FnOnce(&mut loom_core::Controller)>;

/// Renders `frames` frames in buffers of up to `block`, making each change
/// just before its frame. Returns both sides.
fn render_with_changes(
    parts: &mut EngineParts,
    frames: usize,
    block: usize,
    changes: Vec<(usize, Change)>,
) -> (Vec<f32>, Vec<f32>) {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut changes = changes.into_iter().peekable();
    let mut position = 0;
    while position < frames {
        while let Some((_, change)) = changes.next_if(|(at, _)| *at == position) {
            change(&mut parts.controller);
        }
        let until = changes.peek().map_or(frames, |(at, _)| *at);
        let n = block.min(until - position);
        parts
            .engine
            .process(&mut left[position..position + n], &mut right[position..position + n]);
        position += n;
    }
    (left, right)
}

/// Checks every frame from `from` on against the blends, within 1e-6.
fn check_blends(name: &str, block: usize, (left, right): &(Vec<f32>, Vec<f32>), from: usize, blends: &Blends) {
    assert!(right[from..].iter().any(|x| x.abs() > 0.9), "{name}: the source moves");
    for k in from..left.len() {
        let expected = offset_of(blends.at(k, f64::from(right[k])));
        assert!(
            (left[k] - expected).abs() <= 1e-6,
            "{name}, frame {k}, block {block}: {} != {expected}",
            left[k]
        );
    }
}

/// A change that sets assignment `id`'s depth.
fn set_depth(id: loom_core::AssignmentId, depth: f32) -> Change {
    Box::new(move |c| {
        c.set_assignment_depth(id, depth).unwrap();
    })
}

/// Rule 9's #762 criteria: turning an assignment off fades the moving
/// modulation out, `depth · x_k · (1 − k/N)`, and from frame N on it adds
/// exactly nothing; on again fades it in from 0. A depth change and a
/// polarity flip blend `old(x_k)` into `new(x_k)`. Each at block sizes 1,
/// 77, and 512.
#[test]
fn turning_an_assignment_off_and_on_and_changing_it_blends_the_setting() {
    let frames = 3_000;
    let (off_at, on_at) = (1_000, 2_000);
    let depth = 0.4;
    for block in BLOCKS {
        // Off, then on.
        let mut parts = engine();
        let id = blend_patch(&mut parts, depth);
        let played = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![
                (off_at, Box::new(move |c| c.set_assignment_enabled(id, false).unwrap())),
                (on_at, Box::new(move |c| c.set_assignment_enabled(id, true).unwrap())),
            ],
        );
        let off = Setting {
            enabled: false,
            ..Setting::on(depth)
        };
        let blends = Blends {
            first: Setting::on(depth),
            changes: vec![(off_at, off), (on_at, Setting::on(depth))],
        };
        check_blends("off and on", block, &played, off_at - 10, &blends);
        assert!(
            played.0[off_at + fade_length()..on_at].iter().all(|&v| v == 0.0),
            "off adds exactly nothing, block {block}"
        );

        // A depth change and a polarity flip.
        let mut parts = engine();
        let id = blend_patch(&mut parts, depth);
        let played = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![
                (off_at, set_depth(id, -0.15)),
                (
                    on_at,
                    Box::new(move |c| {
                        c.set_assignment_polarity(id, Some(loom_core::Polarity::Unipolar))
                            .unwrap()
                    }),
                ),
            ],
        );
        let unipolar = Setting {
            polarity: Some(loom_core::Polarity::Unipolar),
            ..Setting::on(-0.15)
        };
        let blends = Blends {
            first: Setting::on(depth),
            changes: vec![(off_at, Setting::on(-0.15)), (on_at, unipolar)],
        };
        check_blends("depth and polarity", block, &played, off_at - 10, &blends);
    }
}

/// A drag: five depth changes 800 frames apart, each of which ends before
/// the next, and five 100 frames apart, each of which starts from a
/// running blend. Both match the exact sequence of blends (rule 9, A
/// change while a blend runs).
#[test]
fn a_depth_drag_glides_through_the_exact_sequence_of_blends() {
    let depths = [0.3, -0.2, 0.45, 0.1, -0.35];
    for gap in [800, 100] {
        for block in BLOCKS {
            let mut parts = engine();
            let id = blend_patch(&mut parts, 0.25);
            let start = 1_000;
            let changes: Vec<(usize, Change)> = depths
                .iter()
                .enumerate()
                .map(|(i, &depth)| {
                    let change = set_depth(id, depth);
                    (start + i * gap, change)
                })
                .collect();
            let frames = start + depths.len() * gap + 1_000;
            let played = render_with_changes(&mut parts, frames, block, changes);
            let blends = Blends {
                first: Setting::on(0.25),
                changes: depths
                    .iter()
                    .enumerate()
                    .map(|(i, &depth)| (start + i * gap, Setting::on(depth)))
                    .collect(),
            };
            check_blends(&format!("gap {gap}"), block, &played, start - 10, &blends);
        }
    }
}

/// A batch that ends where it began (A, then B, then A) and the same
/// values sent twice are bit-identical to sending nothing, and a change
/// before the first block is bit-identical to a patch loaded with the new
/// setting (rule 9, How rule 9 handles assignment changes).
#[test]
fn changes_that_change_nothing_heard_are_bit_identical() {
    let frames = 2_000;
    for block in BLOCKS {
        let untouched = {
            let mut parts = engine();
            blend_patch(&mut parts, 0.4);
            render_with_changes(&mut parts, frames, block, vec![])
        };
        let mut parts = engine();
        let id = blend_patch(&mut parts, 0.4);
        let there_and_back = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![(
                1_000,
                Box::new(move |c| {
                    c.set_assignment_depth(id, 0.1).unwrap();
                    c.set_assignment_enabled(id, false).unwrap();
                    c.set_assignment_polarity(id, Some(loom_core::Polarity::Unipolar))
                        .unwrap();
                    c.set_assignment_polarity(id, None).unwrap();
                    c.set_assignment_enabled(id, true).unwrap();
                    c.set_assignment_depth(id, 0.4).unwrap();
                }),
            )],
        );
        assert_eq!(there_and_back, untouched, "A, B, A, block {block}");
        let mut parts = engine();
        let id = blend_patch(&mut parts, 0.4);
        let twice = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![(
                1_000,
                Box::new(move |c| {
                    c.set_assignment_depth(id, 0.4).unwrap();
                    c.set_assignment_depth(id, 0.4).unwrap();
                }),
            )],
        );
        assert_eq!(twice, untouched, "the same values twice, block {block}");

        let loaded = {
            let mut parts = engine();
            blend_patch(&mut parts, 0.1);
            render_with_changes(&mut parts, frames, block, vec![])
        };
        let mut parts = engine();
        let id = blend_patch(&mut parts, 0.4);
        parts.controller.set_assignment_depth(id, 0.1).unwrap();
        let before_play = render_with_changes(&mut parts, frames, block, vec![]);
        assert_eq!(before_play, loaded, "a change before the first block, block {block}");
    }
}

/// Two settings that map the source alike start no blend, so they are
/// bit-identical to no command (rule 9, How rule 9 handles assignment
/// changes: maps are compared, not settings). The sine is bipolar, so
/// forcing it bipolar changes nothing. An assignment that is off, given
/// depth 0 and then turned on at depth 0, adds nothing all along. Its Add
/// sits at an offset of 0.1, which the parameter's position does not give
/// back exactly, so a blend of nothing into nothing would still show.
#[test]
fn settings_that_map_alike_are_bit_identical() {
    let frames = 2_000;
    let (off_at, depth_at, on_at) = (500, 1_000, 1_500);
    for block in BLOCKS {
        let untouched = {
            let mut parts = engine();
            blend_patch(&mut parts, 0.4);
            render_with_changes(&mut parts, frames, block, vec![])
        };
        let mut parts = engine();
        let id = blend_patch(&mut parts, 0.4);
        let own_polarity = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![(
                1_000,
                Box::new(move |c| {
                    c.set_assignment_polarity(id, Some(loom_core::Polarity::Bipolar))
                        .unwrap();
                }),
            )],
        );
        assert_eq!(own_polarity, untouched, "the source's own polarity, block {block}");

        let only_off = {
            let mut parts = engine();
            let id = blend_patch_on(&mut parts, 0.4, 0.1);
            render_with_changes(
                &mut parts,
                frames,
                block,
                vec![(off_at, Box::new(move |c| c.set_assignment_enabled(id, false).unwrap()))],
            )
        };
        let mut parts = engine();
        let id = blend_patch_on(&mut parts, 0.4, 0.1);
        let off_then_depth_0 = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![
                (off_at, Box::new(move |c| c.set_assignment_enabled(id, false).unwrap())),
                (depth_at, set_depth(id, 0.0)),
                (on_at, Box::new(move |c| c.set_assignment_enabled(id, true).unwrap())),
            ],
        );
        assert!(
            only_off.0[off_at + fade_length()..].iter().all(|&v| v == 0.1),
            "the base plays, block {block}"
        );
        assert_eq!(off_then_depth_0, only_off, "off, then depth 0, then on, block {block}");
    }
}

/// A swap during a blend carries it on with no step, bit-identical to no
/// swap. A swap and a change in the same batch blend from what played,
/// whichever comes first in the batch.
#[test]
fn a_swap_during_or_with_a_blend_carries_it_on() {
    let frames = 2_000;
    for block in BLOCKS {
        let alone = {
            let mut parts = engine();
            let id = blend_patch(&mut parts, 0.4);
            render_with_changes(&mut parts, frames, block, vec![(1_000, set_depth(id, 0.1))])
        };
        let unrelated = |c: &mut loom_core::Controller| {
            c.add_module("loom.osc.noise").unwrap();
            c.commit().unwrap();
        };
        let mut parts = engine();
        let id = blend_patch(&mut parts, 0.4);
        let during = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![(1_000, set_depth(id, 0.1)), (1_100, Box::new(unrelated))],
        );
        assert_eq!(during, alone, "a swap during the blend, block {block}");
        for change_first in [false, true] {
            let mut parts = engine();
            let id = blend_patch(&mut parts, 0.4);
            let together = render_with_changes(
                &mut parts,
                frames,
                block,
                vec![(
                    1_000,
                    Box::new(move |c| {
                        if change_first {
                            c.set_assignment_depth(id, 0.1).unwrap();
                            unrelated(c);
                        } else {
                            unrelated(c);
                            c.set_assignment_depth(id, 0.1).unwrap();
                        }
                    }),
                )],
            );
            assert_eq!(
                together, alone,
                "a swap and a change in one batch, change first: {change_first}, block {block}"
            );
        }
    }
}

/// Undo of a settings change reaches the engine as a removal and a
/// restore with the same id and other settings, and a commit: the setting
/// blends, as for the change itself. A change 100 frames later, during the
/// blend the swap started, blends on from where it is.
#[test]
fn a_restored_assignment_with_new_settings_fades_the_setting() {
    let frames = 2_000;
    let at = 1_000;
    for block in BLOCKS {
        let mut parts = engine();
        let id = blend_patch(&mut parts, 0.4);
        let played = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![
                (
                    at,
                    Box::new(move |c| {
                        let mut assignment = *c.graph().assignment(id).unwrap();
                        c.remove_assignment(id).unwrap();
                        assignment.depth = -0.2;
                        assignment.enabled = true;
                        c.restore_assignment(id, assignment).unwrap();
                        c.commit().unwrap();
                    }),
                ),
                (at + 100, set_depth(id, 0.3)),
            ],
        );
        assert_eq!(parts.controller.last_report().cable_fades, 0, "no planned fade");
        let blends = Blends {
            first: Setting::on(0.4),
            changes: vec![(at, Setting::on(-0.2)), (at + 100, Setting::on(0.3))],
        };
        check_blends("restored", block, &played, at - 10, &blends);
    }
}

/// A target that turns stereo during a blend: the new right copy takes the
/// left side's ramp, so with the same signal on both sides the right side
/// plays the left side's contribution on every frame.
#[test]
fn a_target_that_turns_stereo_during_a_blend_blends_both_sides() {
    let frames = 2_400;
    for block in BLOCKS {
        let mut parts = engine();
        let source = sine(&mut parts);
        let c = &mut parts.controller;
        let lfo_phase = c.add_module("loom.phase.generator").unwrap();
        c.set_param_named(lfo_phase, "octave", -3.0).unwrap();
        let lfo = c.add_module("loom.osc.sine").unwrap();
        c.connect_named(lfo_phase, "phase", lfo, "phase").unwrap();
        let svf = c.add_module("loom.filter.svf").unwrap();
        let join = c.add_module("loom.mix.join").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(source, "out", svf, "in").unwrap();
        c.connect_named(source, "out", join, "left").unwrap();
        c.connect_named(source, "out", join, "right").unwrap();
        c.connect_named(svf, "lowpass", out, "in").unwrap();
        let id = c.add_assignment_named(lfo, "out", svf, "cutoff", 0.3).unwrap();
        c.commit().unwrap();
        let (left, right) = render_with_changes(
            &mut parts,
            frames,
            block,
            vec![
                (1_000, set_depth(id, -0.2)),
                (
                    1_100,
                    Box::new(move |c| {
                        c.connect_named(join, "out", svf, "in").unwrap();
                        c.commit().unwrap();
                    }),
                ),
            ],
        );
        assert!(parts.controller.last_report().runs_per_channel(svf));
        assert_eq!(left, right, "block {block}");
    }
}

/// An assignment on a whole-number parameter switches at the next buffer
/// when it is turned off (rule 9, Exceptions).
#[test]
fn turning_off_an_assignment_on_a_whole_number_switches() {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    for block in BLOCKS {
        let mut registry = loom_core::Registry::new();
        loom_modules::register_all(&mut registry).unwrap();
        loom_modules::test_modules::register(&mut registry).unwrap();
        let mut parts = create_engine(std::sync::Arc::new(registry), config);
        let source = constant(&mut parts, 1.0);
        let c = &mut parts.controller;
        let steps = c.add_module("test.steps").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(steps, "out", out, "in").unwrap();
        let id = c.add_assignment_named(source, "out", steps, "steps", 0.5).unwrap();
        c.commit().unwrap();
        let (played, _) = render_with_changes(
            &mut parts,
            1_000,
            block,
            vec![(500, Box::new(move |c| c.set_assignment_enabled(id, false).unwrap()))],
        );
        assert!(played[499] > 0.0, "modulated before, block {block}");
        assert!(played[500..].iter().all(|&v| v == 0.0), "off at once, block {block}");
    }
}

/// A panic during a blend ends it on the silent frame: from there on the
/// patch plays exactly as one whose depth changed long before the panic.
/// The depth changes 300 frames into the panic's 480-frame fade out, so
/// the silent frame comes 180 frames into the 240-frame blend.
#[test]
fn a_panic_ends_a_setting_blend() {
    let (panic_at, change_at, silent) = (1_000, 1_300, 1_480);
    for block in BLOCKS {
        let mut parts = engine();
        let id = blend_patch(&mut parts, 0.4);
        let during = render_with_changes(
            &mut parts,
            2_400,
            block,
            vec![
                (panic_at, Box::new(|c| c.panic().unwrap())),
                (change_at, set_depth(id, 0.1)),
            ],
        );
        let mut parts = engine();
        let id = blend_patch(&mut parts, 0.4);
        let before = render_with_changes(
            &mut parts,
            2_400,
            block,
            vec![(500, set_depth(id, 0.1)), (panic_at, Box::new(|c| c.panic().unwrap()))],
        );
        assert_ne!(during.0[..silent], before.0[..silent], "the blend ran before the panic");
        assert_eq!(during.0[silent..], before.0[silent..], "left, block {block}");
        assert_eq!(during.1[silent..], before.1[silent..], "right, block {block}");
    }
}

/// A Phase module at `tune` semitones, with a meter on its output so it
/// runs from the first frame even before a cable reaches it (rule 8).
fn probed_phase(parts: &mut EngineParts, tune: f32) -> ModuleId {
    let c = &mut parts.controller;
    let phase = c.add_module("loom.phase.generator").unwrap();
    c.set_param_named(phase, "tune", tune).unwrap();
    let output = c.output_ref(phase, "phase").unwrap();
    c.add_probe(output, loom_core::ProbeKind::Meter).unwrap();
    phase
}

/// A Sine module straight into the audio output.
fn sine_out(parts: &mut EngineParts) -> ModuleId {
    let c = &mut parts.controller;
    let sine = c.add_module("loom.osc.sine").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(sine, "out", out, "in").unwrap();
    sine
}

/// What a Sine makes for `frames` frames, fed from a Phase at `tune`, through
/// a Phase Shift when `shift` is true, or on its own phase for `None`: the
/// new signal after a replug, as if it had been running all along. It is
/// read from a scope on the Sine's output, which shows every sample, so the
/// output's fade-in at the engine's start is not in it.
fn ideal_sine(tune: Option<f32>, shift: bool, frames: usize, block: usize) -> Vec<f32> {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        scope_capacity: 2 * frames,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let sine = sine_out(&mut parts);
    if let Some(tune) = tune {
        let phase = probed_phase(&mut parts, tune);
        let c = &mut parts.controller;
        if shift {
            let shifter = c.add_module("loom.phase.shift").unwrap();
            c.connect_named(phase, "phase", shifter, "phase").unwrap();
            c.connect_named(shifter, "phase", sine, "phase").unwrap();
        } else {
            c.connect_named(phase, "phase", sine, "phase").unwrap();
        }
    }
    let output = parts.controller.output_ref(sine, "out").unwrap();
    parts
        .controller
        .add_probe(output, loom_core::ProbeKind::MeterAndScope)
        .unwrap();
    parts.controller.commit().unwrap();
    render(&mut parts, frames, block, &[]);
    let samples: Vec<f32> = parts
        .telemetry
        .scope()
        .flat_map(|chunk| chunk.samples().to_vec())
        .collect();
    assert_eq!(samples.len(), frames, "the scope kept every sample");
    samples
}

/// Checks that from frame `at` on the output ramps from what it played on
/// frame `at - 1` to `new` over 5 ms, exactly, and that there was a jump.
fn check_phase_ramp(name: &str, block: usize, played: &[f32], at: usize, new: &[f32]) {
    let held = played[at - 1];
    assert!(
        (held - new[0]).abs() > 0.1,
        "{name}: the phase replug makes a jump, block {block}"
    );
    for (k, &new) in new.iter().enumerate() {
        assert_eq!(played[at + k], faded(held, new, k), "{name}, frame {k}, block {block}");
    }
}

/// Rule 9's #763 criterion: a running Sine into the audio output, with a
/// phase cable plugged into it, moved, or pulled. The phase switches on the
/// swap frame, so the Sine plays its new phase at once, and the audio
/// output's input ramps from its last value to that over 5 ms: frame k is
/// `held · (1 − k/N) + new · k/N`, and from frame N on exactly `new`. A
/// ramped phase would not give `new`. The same at block sizes 1, 77, and
/// 512.
#[test]
fn a_phase_cable_plugged_moved_or_pulled_fades_what_the_sine_feeds() {
    let frames = 2_000;
    let at = 1_000;
    for block in BLOCKS {
        // Plugged: from the own phase to a Phase at a fifth above.
        let fifth = ideal_sine(Some(7.0), false, frames, block);
        let mut parts = engine();
        let sine = sine_out(&mut parts);
        let other = probed_phase(&mut parts, 7.0);
        parts.controller.commit().unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(other, "phase", sine, "phase").unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 2, "plugged, block {block}");
        check_phase_ramp("plugged", block, &played, at, &fifth[at..]);

        // Moved: from a Phase at middle C to the one a fifth above.
        let mut parts = engine();
        let sine = sine_out(&mut parts);
        let first = probed_phase(&mut parts, 0.0);
        let other = probed_phase(&mut parts, 7.0);
        parts.controller.connect_named(first, "phase", sine, "phase").unwrap();
        parts.controller.commit().unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(other, "phase", sine, "phase").unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 2, "moved, block {block}");
        check_phase_ramp("moved", block, &played, at, &fifth[at..]);

        // Pulled: the Sine goes back to its own phase, which held at 0
        // while the cable was in (oscillators spec, rule 3).
        let own = ideal_sine(None, false, frames - at, block);
        let mut parts = engine();
        let sine = sine_out(&mut parts);
        let other = probed_phase(&mut parts, 7.0);
        parts.controller.connect_named(other, "phase", sine, "phase").unwrap();
        parts.controller.commit().unwrap();
        let input = parts.controller.input_ref(sine, "phase").unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.disconnect(input).unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 2, "pulled, block {block}");
        check_phase_ramp("pulled", block, &played, at, &own);
    }
}

/// Rule 9's #763 criterion: Phase into Phase Shift into Sine. When the
/// Phase Shift's phase cable moves, the fade follows the phase cable on:
/// the Sine's phase switches, and the audio output it feeds ramps over
/// 5 ms. A gate input the Sine feeds switches instead, exactly as if the
/// new phase had been there all along.
#[test]
fn a_phase_shift_replug_fades_what_the_sine_after_it_feeds() {
    let frames = 2_000;
    let at = 1_000;
    for block in BLOCKS {
        let new = ideal_sine(Some(7.0), true, frames, block);
        let mut parts = engine();
        let sine = sine_out(&mut parts);
        let first = probed_phase(&mut parts, 0.0);
        let other = probed_phase(&mut parts, 7.0);
        let c = &mut parts.controller;
        let shifter = c.add_module("loom.phase.shift").unwrap();
        c.connect_named(first, "phase", shifter, "phase").unwrap();
        c.connect_named(shifter, "phase", sine, "phase").unwrap();
        c.commit().unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts
                .controller
                .connect_named(other, "phase", shifter, "phase")
                .unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 2, "block {block}");
        check_phase_ramp("through a Phase Shift", block, &played, at, &new[at..]);

        // A gate downstream: the Sine into an Or's input, which passes it
        // on as a gate. It switches.
        let gated = |parts: &mut EngineParts, tune: f32| {
            let phase = probed_phase(parts, tune);
            let c = &mut parts.controller;
            let shifter = c.add_module("loom.phase.shift").unwrap();
            let sine = c.add_module("loom.osc.sine").unwrap();
            let logic = c.add_module("loom.logic.logic").unwrap();
            let out = c.add_module("loom.io.audio-out").unwrap();
            c.set_param_named(logic, "op", 1.0).unwrap();
            c.connect_named(phase, "phase", shifter, "phase").unwrap();
            c.connect_named(shifter, "phase", sine, "phase").unwrap();
            c.connect_named(sine, "out", logic, "in1").unwrap();
            c.connect_named(logic, "out", out, "in").unwrap();
            shifter
        };
        let mut ideal = engine();
        gated(&mut ideal, 7.0);
        ideal.controller.commit().unwrap();
        let (ideal, _) = render(&mut ideal, frames, block, &[]);
        let mut parts = engine();
        let shifter = gated(&mut parts, 0.0);
        let other = probed_phase(&mut parts, 7.0);
        parts.controller.commit().unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts
                .controller
                .connect_named(other, "phase", shifter, "phase")
                .unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 0, "a gate switches");
        assert!(
            ideal[at..].contains(&1.0) && ideal[at..].contains(&0.0),
            "the gate moves"
        );
        assert_eq!(played[at..], ideal[at..], "the gate from the swap frame, block {block}");
    }
}

/// Rule 9's #763 criterion: an assignment whose source is a module whose
/// phase cable changed ramps from its held contribution to the new one
/// over 5 ms, as a moved source does, at block sizes 1, 77, and 512.
#[test]
fn an_assignment_from_a_replugged_module_ramps_its_contribution() {
    let frames = 2_000;
    let at = 1_000;
    let depth = 0.25;
    for block in BLOCKS {
        let old = ideal_sine(Some(0.0), false, frames, block);
        let new = ideal_sine(Some(7.0), false, frames, block);
        let mut parts = engine();
        let seen = window(&mut parts);
        let first = probed_phase(&mut parts, 0.0);
        let other = probed_phase(&mut parts, 7.0);
        let c = &mut parts.controller;
        let sine = c.add_module("loom.osc.sine").unwrap();
        c.connect_named(first, "phase", sine, "phase").unwrap();
        c.add_assignment_named(sine, "out", seen, "offset", depth).unwrap();
        c.commit().unwrap();
        let played = render_changing(&mut parts, frames, at, block, |parts| {
            parts.controller.connect_named(other, "phase", sine, "phase").unwrap();
        });
        assert_eq!(parts.controller.last_report().cable_fades, 1, "block {block}");
        let held = f64::from(depth * old[at - 1]);
        assert!(
            (held - f64::from(depth * new[at])).abs() > 0.05,
            "the contribution jumps, block {block}"
        );
        for k in 1..frames - at {
            let t = (k as f64 / fade_length() as f64).min(1.0);
            let expected = offset_of(held * (1.0 - t) + f64::from(depth * new[at + k]) * t);
            assert!(
                (played[at + k] - expected).abs() <= 1e-6,
                "frame {k}, block {block}: {} != {expected}",
                played[at + k]
            );
        }
    }
}

/// A 4-voice phase bundle into a Sine, replugged to a mono phase, so the
/// Sine narrows to one voice. Voice 1 of the audio output fades to the new
/// signal, and voices 2 to 4 fade out from their held values. Their fade
/// slots exist only if the phase walk adds the output's input before the
/// compiler lists the voices the old schedule read; without them those
/// voices would stop at once.
#[test]
fn a_phase_bundle_replugged_to_mono_fades_the_voices_out() {
    let mut registry = loom_core::Registry::new();
    loom_modules::register_all(&mut registry).unwrap();
    loom_modules::test_modules::register(&mut registry).unwrap();
    let registry = std::sync::Arc::new(registry);
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    // What a Sine makes of a fixed phase, once the output's fade-in at
    // the engine's start is over.
    let sine_of = |phase: f32| {
        let mut parts = engine();
        let sine = sine_out(&mut parts);
        let value = constant(&mut parts, phase);
        parts.controller.connect_named(value, "out", sine, "phase").unwrap();
        parts.controller.commit().unwrap();
        render(&mut parts, 1_000, 64, &[]).0[999]
    };
    let at = 500;
    for block in BLOCKS {
        let mut parts = create_engine(registry.clone(), config);
        let sine = sine_out(&mut parts);
        let c = &mut parts.controller;
        let voices = c.add_module("test.voices").unwrap();
        c.set_param_named(voices, "voices", 4.0).unwrap();
        c.set_param_named(voices, "value", 0.005).unwrap();
        c.connect_named(voices, "out", sine, "phase").unwrap();
        let mono = constant(&mut parts, 0.2);
        parts.controller.commit().unwrap();
        let played = render_changing(&mut parts, 1_000, at, block, |parts| {
            parts.controller.connect_named(mono, "out", sine, "phase").unwrap();
        });
        // Small phases, so the four voices sum under full scale.
        let held: Vec<f32> = (0..4).map(|voice| sine_of(0.005 + voice as f32 * 0.01)).collect();
        let before: f32 = held.iter().sum();
        assert!(
            (played[at - 1] - before).abs() < 1e-5,
            "four voices sum: {} != {before}, {held:?}",
            played[at - 1]
        );
        let new = sine_of(0.2);
        for k in 0..500 {
            let expected =
                faded(held[0], new, k) + faded(held[1], 0.0, k) + faded(held[2], 0.0, k) + faded(held[3], 0.0, k);
            assert!(
                (played[at + k] - expected).abs() < 1e-6,
                "frame {k}, block {block}: {} != {expected}",
                played[at + k]
            );
        }
    }
}
