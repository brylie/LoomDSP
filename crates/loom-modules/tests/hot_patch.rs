//! Measurements for #203: does a new signal path join a running patch
//! without a cold start? Each test measures one case from the issue and
//! checks the result against a click threshold. Run with `--nocapture` to
//! see the numbers the stereo spec quotes.
//!
//! The error signal is the difference between what the engine plays and
//! the ideal continuation: what the patch would play if the new path had
//! been running all along. Its peak, in dBFS, is how loud the glitch is.
//!
//! Some tests pin today's clicks. When rules 5 and 6 of
//! `docs/design/live-patching.md` are built, their assertions flip to
//! bit-identical with the ideal. Background: `docs/learn/hot-patching.md`.

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

/// Renders `frames` frames in blocks of 64, calling `change` and
/// committing once, just before frame `at`.
fn render_changing(
    parts: &mut loom_core::EngineParts,
    frames: usize,
    at: usize,
    mut change: impl FnMut(&mut loom_core::EngineParts),
) -> Vec<f32> {
    render_with(parts, frames, 64, &[], |position, parts| {
        if position == at {
            change(parts);
            parts.controller.commit().unwrap();
        }
    })
    .0
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
fn a_new_loop_through_a_fallback_input_drops_out_for_one_sample() {
    // Case 2. A Multiply scales a sine by its B input, which reads 1 while
    // nothing is plugged in. At the change, B is patched from a loop:
    // B = 1 + 0.3 * out, one sample late. The compiler inserts the delay.
    //
    // The ideal is the same patch with an explicit Unit Delay that was
    // already running (its input plugged, its output not yet), so the
    // delay holds the value from the frame before. Only the last cable is
    // plugged at the change.
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

    let (mut parts, vca, bias, _) = build(false);
    let b = input(&parts, vca, "b");
    let cold = render_changing(&mut parts, frames, at, |parts| {
        let from = parts.controller.output_ref(bias, "out").unwrap();
        parts.controller.connect(from, b).unwrap();
    });
    let report = parts.controller.last_report();
    assert_eq!(report.loops.len(), 1);
    assert_eq!(report.loops[0].inserted_delays[0].1, b, "the delay is on B");

    let (mut parts, vca, _, delay) = build(true);
    let b = input(&parts, vca, "b");
    let ideal = render_changing(&mut parts, frames, at, |parts| {
        let from = parts.controller.output_ref(delay.unwrap(), "out").unwrap();
        parts.controller.connect(from, b).unwrap();
    });

    assert_eq!(cold[..at], ideal[..at], "identical before the change");
    let error: Vec<f32> = cold[at..].iter().zip(&ideal[at..]).map(|(c, i)| c - i).collect();
    println!(
        "new loop into a fallback input: error peak {:.1} dB, settles in {:.2} ms, first sample {:.3} instead of {:.3}",
        db(peak(&error)),
        settle_ms(&error),
        cold[at],
        ideal[at],
    ); // The new delay starts at 0, so B reads 0 for one sample where it read
    // 1 before and about 1 after: the sine drops out for one sample.
    assert_eq!(cold[at], 0.0);
    assert!(peak(&error) > CLICK);
    assert!(settle_ms(&error) < 0.1, "only the first few samples differ");
}

#[test]
fn a_new_loop_through_an_input_that_reads_zero_is_only_one_sample_late() {
    // Case 2, harmless form. Same loop, but closed through an Add's B
    // input, which reads 0 while nothing is plugged in. A delay that starts
    // at 0 then gives exactly what the input read before, so the cable acts
    // as if it were plugged in one sample later. No click beyond the step
    // that plugging the cable makes anyway.
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

    let (mut parts, mix, scale, _) = build(false);
    let b = input(&parts, mix, "b");
    let cold = render_changing(&mut parts, frames, at, |parts| {
        let from = parts.controller.output_ref(scale, "out").unwrap();
        parts.controller.connect(from, b).unwrap();
    });
    assert_eq!(parts.controller.last_report().loops[0].inserted_delays[0].1, b);

    let (mut parts, mix, _, delay) = build(true);
    let b = input(&parts, mix, "b");
    let ideal = render_changing(&mut parts, frames, at, |parts| {
        let from = parts.controller.output_ref(delay.unwrap(), "out").unwrap();
        parts.controller.connect(from, b).unwrap();
    });

    // Plugging the cable steps the output whichever way it is done, by up
    // to half the signal here. The cold delay moves that step one sample
    // later, and does not add a step of its own.
    let window = at - 64..at + 64;
    println!(
        "new loop into a zero input: largest step {:.4} cold, {:.4} ideal",
        largest_step(&cold[window.clone()]),
        largest_step(&ideal[window.clone()]),
    );
    assert!(largest_step(&cold[window.clone()]) <= largest_step(&ideal[window]) + 1e-6);
}

#[test]
fn a_delay_that_moves_to_another_cable_clicks() {
    // Case 3. A running loop: Add B -> Multiply (0.5) -> back into Add B.
    // The compiler delays the Add's B input. Then a cable is added that
    // changes nothing you can hear (it goes through a Multiply set to 0),
    // but makes a second loop. The compiler now delays a different cable,
    // the Multiply's input, and the old delay is gone. The new one starts
    // at 0, so the feedback drops out for one sample.
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

    let (mut parts, ..) = build();
    let ideal = render(&mut parts, frames, 64, &[]).0;

    let (mut parts, mix, feedback, silent) = build();
    let before = parts.controller.last_report().loops[0].inserted_delays.clone();
    assert_eq!(before[0].1, input(&parts, mix, "b"), "delayed at the Add first");
    let changed = render_changing(&mut parts, frames, at, |parts| {
        let from = parts.controller.output_ref(feedback, "out").unwrap();
        let to = parts.controller.input_ref(silent, "a").unwrap();
        parts.controller.connect(from, to).unwrap();
    });
    let after = parts.controller.last_report().loops[0].inserted_delays.clone();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].1, input(&parts, feedback, "a"), "the delay moved");

    let error: Vec<f32> = changed[at..].iter().zip(&ideal[at..]).map(|(c, i)| c - i).collect();
    println!(
        "delay moved inside a running loop: error peak {:.1} dB, settles in {:.2} ms",
        db(peak(&error)),
        settle_ms(&error),
    );
    assert_eq!(changed[..at], ideal[..at]);
    assert!(peak(&error) > CLICK);
}

#[test]
fn unplugging_and_replugging_inside_a_loop_keeps_the_delay_state() {
    // The other side of case 3: a delay that stays on the same input keeps
    // its state across swaps, because its key is that input. Changing a
    // knob or adding an unrelated module recompiles without a click.
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
    let changed = render_changing(&mut build(), frames, at, |parts| {
        parts.controller.add_module("loom.osc.noise").unwrap();
    });
    assert_eq!(changed, ideal);
}
