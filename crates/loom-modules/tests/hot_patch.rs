//! Measurements for #203: does a new signal path join a running patch
//! without a cold start? Each test measures one case from the issue and
//! checks the result against a click threshold. Run with `--nocapture` to
//! see the numbers the stereo spec quotes.
//!
//! The error signal is the difference between what the engine plays and
//! the ideal continuation: what the patch would play if the new path had
//! been running all along. Its peak, in dBFS, is how loud the glitch is.
//!
//! The unit delay tests check rule 5 of `docs/design/live-patching.md`:
//! a new delay starts with its source's last value, so the output is
//! bit-identical to the ideal. The stereo copy tests measure a cold copy 1,
//! which rule 6 replaces once copies are built (#23). Background:
//! `docs/learn/hot-patching.md`.

mod common;

use common::*;
use loom_core::{EngineConfig, InputRef, ModuleId, OutputSafety, create_engine};
use loom_modules::filter::SVF;
use loom_test::{Harness, Signal};

/// An error below this peak is buried under the signal it rides on. About
/// -60 dBFS.
const INAUDIBLE: f32 = 1e-3;

/// An error above this peak, arriving in one sample, is heard as a click
/// over a full-scale signal. About -40 dBFS.
const CLICK: f32 = 1e-2;

fn db(value: f32) -> f32 {
    20.0 * value.max(1e-12).log10()
}

fn peak(error: &[f32]) -> f32 {
    error.iter().fold(0.0, |peak, e| peak.max(e.abs()))
}

/// The largest change from one sample to the next. A click is a large step.
fn largest_step(signal: &[f32]) -> f32 {
    signal.windows(2).fold(0.0, |step, w| step.max((w[1] - w[0]).abs()))
}

/// How long the error stays above [`INAUDIBLE`], in milliseconds.
fn settle_ms(error: &[f32]) -> f32 {
    let last = error.iter().rposition(|e| e.abs() > INAUDIBLE).map_or(0, |i| i + 1);
    last as f32 * 1_000.0 / SAMPLE_RATE
}

// --- Case 1: the right-hand copy of a module that turns stereo -----------

/// A test input with two partials, so the filter has something to shape.
fn music(n: usize) -> f32 {
    let t = n as f32 / SAMPLE_RATE;
    0.5 * (std::f32::consts::TAU * 110.0 * t).sin() + 0.25 * (std::f32::consts::TAU * 440.0 * t).sin()
}

/// What changes on the right side when a running SVF turns stereo at frame
/// `join`, with the same signal on both sides. Copy 0 has run all along.
/// Copy 1 is created as rule 13 says: from the registry, reset, then given
/// the current parameter values. So its first frame is frame `join`.
///
/// Before the join the right side plays copy 0 (a mono cable plays on both
/// sides). The ideal right side after the join is copy 0 again, because the
/// two sides carry the same signal. Returns the error of a cold copy 1.
fn svf_cold_error(output: &str, cutoff: f32, resonance: f32, join: usize, frames: usize) -> Vec<f32> {
    let harness = |offset: usize, length: usize| {
        Harness::new(&SVF)
            .frames(length)
            .param("cutoff", cutoff)
            .param("resonance", resonance)
            .input("in", Signal::from_fn(move |n| music(n + offset)))
    };
    let copy0 = harness(0, frames).render();
    let copy1 = harness(join, frames - join).render();
    let left = &copy0.output(output)[join..];
    copy1.output(output).iter().zip(left).map(|(r, l)| r - l).collect()
}

/// The right side's error when it crossfades from copy 0's output to copy 1
/// over `fade` frames, instead of switching at once.
fn crossfaded(error: &[f32], fade: usize) -> Vec<f32> {
    error
        .iter()
        .enumerate()
        .map(|(i, e)| e * ((i + 1) as f32 / fade as f32).min(1.0))
        .collect()
}

#[test]
fn a_cold_right_copy_clicks_and_a_crossfade_only_hides_it() {
    let join = 4_800;
    let frames = join + 9_600;
    let fade = (0.005 * SAMPLE_RATE) as usize;
    println!("SVF copy 1 joining at 100 ms, same signal on both sides");
    println!("output    cutoff  res   | cold: peak  step    settle | 5 ms fade: peak  step    settle");
    // The SVF is built with a cutoff of 1000 Hz and a resonance of 0.2, and
    // `set_param` glides from there over 10 ms. So the 1000 Hz row shows
    // the filter's own memory alone, and the others add the glide.
    for (output, cutoff, resonance) in [
        ("lowpass", 200.0, 0.2),
        ("lowpass", 1_000.0, 0.2),
        ("lowpass", 2_000.0, 0.2),
        ("lowpass", 500.0, 0.9),
        ("highpass", 200.0, 0.2),
        ("highpass", 2_000.0, 0.2),
        ("bandpass", 500.0, 0.9),
    ] {
        let cold = svf_cold_error(output, cutoff, resonance, join, frames);
        let faded = crossfaded(&cold, fade);
        println!(
            "{output:9} {cutoff:6} {resonance:4}  | {:6.1} dB {:6.1} dB {:5.1} ms | {:6.1} dB {:6.1} dB {:5.1} ms",
            db(peak(&cold)),
            db(largest_step(&cold)),
            settle_ms(&cold),
            db(peak(&faded)),
            db(largest_step(&faded)),
            settle_ms(&faded),
        );
        // A cold copy is always off by more than the click threshold.
        assert!(peak(&cold) > CLICK, "{output} {cutoff} {resonance}");
        // A crossfade scales the error down while it fades, but cannot
        // shorten it. When copy 1 takes longer to settle than the fade,
        // the right side is still wrong after the fade.
        if settle_ms(&cold) > 6.0 {
            assert!(settle_ms(&faded) > 5.0, "{output} {cutoff} {resonance}");
            assert!(peak(&faded[fade..]) > INAUDIBLE, "{output} {cutoff} {resonance}");
        }
    }
}

#[test]
fn a_cold_highpass_copy_steps_in_one_sample() {
    // The highpass output passes the input straight through at first, so a
    // cold copy steps by the size of the input at the join. A crossfade
    // turns that step into a ramp.
    let join = 4_800;
    let cold = svf_cold_error("highpass", 200.0, 0.2, join, join + 4_800);
    let faded = crossfaded(&cold, 240);
    assert!(cold[0].abs() > 0.1, "step {}", cold[0]);
    assert!(largest_step(&faded) < CLICK, "{}", largest_step(&faded));
}

#[test]
fn a_right_copy_with_copied_state_matches_the_left_exactly() {
    // The state-copy option. The SVF's state is its two integrators and its
    // parameter smoothing. Taking copy 0's state at the join is the same as
    // copy 1 having run all along, so it is bit-identical to copy 0. The
    // harness has no way to copy a module's state, so this checks the claim
    // the option rests on: a second instance fed the same input from the
    // start gives the same output, bit for bit (hard rule 3).
    let frames = 9_600;
    let render = || {
        Harness::new(&SVF)
            .frames(frames)
            .param("cutoff", 500.0)
            .param("resonance", 0.9)
            .input("in", Signal::from_fn(music))
            .render()
    };
    assert_eq!(render().output("lowpass"), render().output("lowpass"));
}

// --- Cases 2 and 3: unit delays the compiler inserts ---------------------

fn engine() -> loom_core::EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    create_engine(loom_modules::registry(), config)
}

fn input(parts: &loom_core::EngineParts, module: ModuleId, name: &str) -> InputRef {
    parts.controller.input_ref(module, name).unwrap()
}

/// Host buffer sizes the delay tests run at. The edit lands on the same
/// frame at each, so the output must be the same (hard rule 2).
const BLOCKS: [usize; 4] = [1, 64, 77, 512];

/// Renders `frames` frames in host buffers of up to `block` frames, calling
/// `change` and committing once, just before frame `at`. The buffer before
/// the edit is cut short so the edit lands exactly on `at`.
fn render_changing(
    parts: &mut loom_core::EngineParts,
    frames: usize,
    at: usize,
    block: usize,
    change: impl FnOnce(&mut loom_core::EngineParts),
) -> Vec<f32> {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut run = |parts: &mut loom_core::EngineParts, from: usize, to: usize| {
        let mut position = from;
        while position < to {
            let n = block.min(to - position);
            let range = position..position + n;
            parts.engine.process(&mut left[range.clone()], &mut right[range]);
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
fn sine(parts: &mut loom_core::EngineParts) -> ModuleId {
    let c = &mut parts.controller;
    let phase = c.add_module("loom.phase.generator").unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    c.connect_named(phase, "phase", sine, "phase").unwrap();
    sine
}

#[test]
fn a_new_loop_through_a_fallback_input_does_not_drop_out() {
    // Case 2. A Multiply scales a sine by its B input, which reads 1 while
    // nothing is plugged in. At the change, B is patched from a loop:
    // B = 1 + 0.3 * out, one sample late. The compiler inserts the delay.
    //
    // The ideal is the same patch with an explicit Unit Delay that was
    // already running (its input plugged, its output not yet), so the
    // delay holds the value from the frame before. Only the last cable is
    // plugged at the change.
    //
    // A new delay that started at 0 made B read 0 for one sample, where it
    // read 1 before and about 1 after: a one-sample dropout, -6 dB. Rule 5
    // starts it with the source's last value instead.
    let frames = 9_600;
    let at = 4_800;
    let build = |explicit: bool| {
        let mut parts = engine();
        // The Multiply gets the lowest module ID, so the compiler puts its
        // delay on the Multiply's B input (the lowest ID with the fewest
        // inputs in the loop).
        let vca = parts.controller.add_module("loom.math.mul").unwrap();
        // Half scale, so the output stays below the final clip.
        parts.controller.set_param_named(vca, "gain", 0.5).unwrap();
        let source = sine(&mut parts);
        let c = &mut parts.controller;
        let scale = c.add_module("loom.math.mul").unwrap();
        c.set_param_named(scale, "gain", 0.3).unwrap();
        let bias = c.add_module("loom.math.add").unwrap();
        c.set_param_named(bias, "offset", 1.0).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(source, "out", vca, "a").unwrap();
        c.connect_named(vca, "out", scale, "a").unwrap();
        c.connect_named(scale, "out", bias, "a").unwrap();
        c.connect_named(vca, "out", out, "left").unwrap();
        let delay = explicit.then(|| {
            let delay = c.add_module("loom.time.delay1").unwrap();
            c.connect_named(bias, "out", delay, "in").unwrap();
            delay
        });
        c.commit().unwrap();
        (parts, vca, bias, delay)
    };

    for block in BLOCKS {
        let (mut parts, vca, bias, _) = build(false);
        let b = input(&parts, vca, "b");
        let inserted = render_changing(&mut parts, frames, at, block, |parts| {
            let from = parts.controller.output_ref(bias, "out").unwrap();
            parts.controller.connect(from, b).unwrap();
        });
        let report = parts.controller.last_report();
        assert_eq!(report.loops.len(), 1);
        assert_eq!(report.loops[0].inserted_delays[0].1, b, "the delay is on B");

        let (mut parts, vca, _, delay) = build(true);
        let b = input(&parts, vca, "b");
        let ideal = render_changing(&mut parts, frames, at, block, |parts| {
            let from = parts.controller.output_ref(delay.unwrap(), "out").unwrap();
            parts.controller.connect(from, b).unwrap();
        });

        assert!(peak(&ideal[at..]) > 0.1, "the patch makes sound");
        assert_eq!(inserted, ideal, "block size {block}");
    }
}

#[test]
fn a_new_loop_through_an_input_that_reads_zero_matches_a_running_delay() {
    // Case 2, mild form. Same loop, but closed through an Add's B input,
    // which reads 0 while nothing is plugged in. A delay that started at 0
    // acted as if the cable were plugged one sample later, with no extra
    // step. With rule 5 it is exact, as with a delay already running.
    let frames = 9_600;
    let at = 4_800;
    let build = |explicit: bool| {
        let mut parts = engine();
        let mix = parts.controller.add_module("loom.math.add").unwrap();
        let source = sine(&mut parts);
        let c = &mut parts.controller;
        let scale = c.add_module("loom.math.mul").unwrap();
        c.set_param_named(scale, "gain", 0.5).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(source, "out", mix, "a").unwrap();
        c.connect_named(mix, "out", scale, "a").unwrap();
        c.connect_named(mix, "out", out, "left").unwrap();
        let delay = explicit.then(|| {
            let delay = c.add_module("loom.time.delay1").unwrap();
            c.connect_named(scale, "out", delay, "in").unwrap();
            delay
        });
        c.commit().unwrap();
        (parts, mix, scale, delay)
    };

    for block in BLOCKS {
        let (mut parts, mix, scale, _) = build(false);
        let b = input(&parts, mix, "b");
        let inserted = render_changing(&mut parts, frames, at, block, |parts| {
            let from = parts.controller.output_ref(scale, "out").unwrap();
            parts.controller.connect(from, b).unwrap();
        });
        assert_eq!(parts.controller.last_report().loops[0].inserted_delays[0].1, b);

        let (mut parts, mix, _, delay) = build(true);
        let b = input(&parts, mix, "b");
        let ideal = render_changing(&mut parts, frames, at, block, |parts| {
            let from = parts.controller.output_ref(delay.unwrap(), "out").unwrap();
            parts.controller.connect(from, b).unwrap();
        });

        assert_eq!(inserted, ideal, "block size {block}");
    }
}

#[test]
fn a_delay_that_moves_to_another_cable_does_not_click() {
    // Case 3. A running loop: Add B -> Multiply (0.5) -> back into Add B.
    // The compiler delays the Add's B input. Then a cable is added that
    // changes nothing you can hear (it goes through a Multiply set to 0),
    // but makes a second loop. The compiler now delays a different cable,
    // the Multiply's input, and the old delay is gone. A new delay that
    // started at 0 dropped the feedback for one sample: -17 dB. Rule 5
    // starts it with the Add's last output, which is exactly what it would
    // hold had it always been there.
    //
    // The two placements give the same recurrence, out = in + 0.5 * out
    // one sample earlier, so the ideal output is the patch with no change.
    let frames = 9_600;
    let at = 4_800;
    let build = || {
        let mut parts = engine();
        let c = &mut parts.controller;
        let mix = c.add_module("loom.math.add").unwrap();
        let feedback = c.add_module("loom.math.mul").unwrap();
        let front = c.add_module("loom.math.add").unwrap();
        let silent = c.add_module("loom.math.mul").unwrap();
        c.set_param_named(feedback, "gain", 0.5).unwrap();
        c.set_param_named(silent, "gain", 0.0).unwrap();
        let source = sine(&mut parts);
        let c = &mut parts.controller;
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(source, "out", front, "b").unwrap();
        c.connect_named(silent, "out", front, "a").unwrap();
        c.connect_named(front, "out", mix, "a").unwrap();
        c.connect_named(mix, "out", feedback, "a").unwrap();
        c.connect_named(feedback, "out", mix, "b").unwrap();
        c.connect_named(mix, "out", out, "left").unwrap();
        c.commit().unwrap();
        (parts, mix, feedback, silent)
    };

    for block in BLOCKS {
        let (mut parts, ..) = build();
        let ideal = render_changing(&mut parts, frames, at, block, |_| {});

        let (mut parts, mix, feedback, silent) = build();
        let before = parts.controller.last_report().loops[0].inserted_delays.clone();
        assert_eq!(before[0].1, input(&parts, mix, "b"), "delayed at the Add first");
        let changed = render_changing(&mut parts, frames, at, block, |parts| {
            let from = parts.controller.output_ref(feedback, "out").unwrap();
            let to = parts.controller.input_ref(silent, "a").unwrap();
            parts.controller.connect(from, to).unwrap();
        });
        let after = parts.controller.last_report().loops[0].inserted_delays.clone();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].1, input(&parts, feedback, "a"), "the delay moved");

        assert!(peak(&ideal[at..]) > 0.1, "the patch makes sound");
        assert_eq!(changed, ideal, "block size {block}");
    }
}

#[test]
fn an_unrelated_edit_keeps_a_running_loop_bit_identical() {
    // The other side of case 3: a delay that stays on the same input keeps
    // its state across swaps, because its key is that input. Adding an
    // unrelated module recompiles without a click.
    let frames = 9_600;
    let at = 4_800;
    let build = || {
        let mut parts = engine();
        let c = &mut parts.controller;
        let mix = c.add_module("loom.math.add").unwrap();
        let feedback = c.add_module("loom.math.mul").unwrap();
        c.set_param_named(feedback, "gain", 0.5).unwrap();
        let source = sine(&mut parts);
        let c = &mut parts.controller;
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(source, "out", mix, "a").unwrap();
        c.connect_named(mix, "out", feedback, "a").unwrap();
        c.connect_named(feedback, "out", mix, "b").unwrap();
        c.connect_named(mix, "out", out, "left").unwrap();
        c.commit().unwrap();
        parts
    };
    let ideal = render(&mut build(), frames, 64, &[]).0;
    let changed = render_changing(&mut build(), frames, at, 64, |parts| {
        parts.controller.add_module("loom.osc.noise").unwrap();
    });
    assert_eq!(changed, ideal);
}
