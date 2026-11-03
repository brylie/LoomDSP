use super::*;
use loom_core::ModuleConfig;
use loom_core::testing::run;

/// What the module sent over a render.
struct Outputs {
    trigger: Vec<f32>,
    gate: Vec<f32>,
    accent: Vec<f32>,
}

impl Outputs {
    /// The frames where `samples` is high.
    fn high(samples: &[f32]) -> Vec<usize> {
        (0..samples.len()).filter(|&i| samples[i] > 0.5).collect()
    }
}

/// A module with these parameters, as the `Controller` starts one.
fn module(params: &[(usize, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = EUCLID.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 4_096,
    });
    for &(index, value) in params {
        m.set_param(index, value);
    }
    m.reset();
    m
}

/// `module` over the inputs, in one block.
fn process(m: &mut dyn Module, phase: &[f32], clock: &[f32], reset: &[f32]) -> Outputs {
    let (mut trigger, mut gate, mut accent) = (vec![0.0; phase.len()], vec![0.0; phase.len()], vec![0.0; phase.len()]);
    run(
        m,
        48_000.0,
        &[phase, clock, reset],
        &[true, true, true],
        &mut [&mut trigger, &mut gate, &mut accent],
        &[],
    );
    Outputs { trigger, gate, accent }
}

/// A clock with a rising edge every 10 frames, starting at `first`.
fn clock_pulses(frames: usize, first: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            if i >= first && (i - first).is_multiple_of(10) {
                1.0
            } else {
                0.0
            }
        })
        .collect()
}

/// Clock mode with these parameters.
fn clocked(steps: f32, hits: f32, rotation: f32) -> Box<dyn Module> {
    module(&[
        (Euclid::SOURCE, 1.0),
        (Euclid::STEPS, steps),
        (Euclid::HITS, hits),
        (Euclid::ROTATION, rotation),
    ])
}

/// A phase covering `cycles` cycles over `frames` frames.
fn ramp(frames: usize, cycles: f32) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            let x = i as f32 * cycles / frames as f32;
            x - x.floor()
        })
        .collect()
}

/// In Phase mode, 3 in 8 fires on steps 0, 3, and 6, with the accent on the
/// first (spec, acceptance).
#[test]
fn a_phase_plays_the_pattern() {
    let mut m = module(&[(Euclid::STEPS, 8.0), (Euclid::HITS, 3.0)]);
    let silent = vec![0.0; 800];
    let out = process(m.as_mut(), &ramp(800, 1.0), &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0, 300, 600]);
    assert_eq!(Outputs::high(&out.accent), [0]);
    assert_eq!(Outputs::high(&out.gate).len(), 300, "three steps of 100 frames");
}

/// Rotation moves the pattern later, and the accent with it.
#[test]
fn rotation_moves_the_hits_later() {
    let mut m = module(&[(Euclid::STEPS, 8.0), (Euclid::HITS, 3.0), (Euclid::ROTATION, 2.0)]);
    let silent = vec![0.0; 800];
    let out = process(m.as_mut(), &ramp(800, 1.0), &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0, 200, 500]);
    assert_eq!(Outputs::high(&out.accent), [200]);
}

/// A rotation above the steps goes round the pattern.
#[test]
fn a_rotation_above_the_steps_goes_round() {
    let mut m = module(&[(Euclid::STEPS, 8.0), (Euclid::HITS, 3.0), (Euclid::ROTATION, 10.0)]);
    let silent = vec![0.0; 800];
    let out = process(m.as_mut(), &ramp(800, 1.0), &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0, 200, 500]);
}

/// With 0 hits, nothing fires (spec).
#[test]
fn no_hits_is_silence() {
    let mut m = module(&[(Euclid::STEPS, 8.0), (Euclid::HITS, 0.0)]);
    let silent = vec![0.0; 800];
    let out = process(m.as_mut(), &ramp(800, 1.0), &silent, &silent);
    assert!(Outputs::high(&out.trigger).is_empty());
    assert!(Outputs::high(&out.gate).is_empty());
    assert!(Outputs::high(&out.accent).is_empty());
}

/// The gate stays high across hits next to each other, and each still fires.
#[test]
fn the_gate_stays_high_across_adjacent_hits() {
    let mut m = module(&[(Euclid::STEPS, 4.0), (Euclid::HITS, 4.0)]);
    let silent = vec![0.0; 400];
    let out = process(m.as_mut(), &ramp(400, 1.0), &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0, 100, 200, 300]);
    assert_eq!(Outputs::high(&out.gate).len(), 400);
}

/// One step fires once per cycle, because a wrap counts as entering the step
/// (decision in the spec), and a wrap in a longer pattern counts once.
#[test]
fn a_wrap_enters_the_step_once() {
    let mut one = module(&[(Euclid::STEPS, 1.0), (Euclid::HITS, 1.0)]);
    let silent = vec![0.0; 600];
    let out = process(one.as_mut(), &ramp(600, 3.0), &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0, 200, 400]);

    let mut four = module(&[(Euclid::STEPS, 4.0), (Euclid::HITS, 4.0)]);
    let out = process(four.as_mut(), &ramp(600, 3.0), &silent, &silent);
    let entries = Outputs::high(&out.trigger);
    assert_eq!(entries.len(), 12, "{entries:?}");
    assert!(entries.windows(2).all(|pair| pair[1] - pair[0] == 50), "{entries:?}");
}

/// A falling phase enters its steps too, firing from either direction (spec).
#[test]
fn a_falling_phase_fires() {
    let mut m = module(&[(Euclid::STEPS, 4.0), (Euclid::HITS, 4.0)]);
    let falling: Vec<f32> = ramp(400, 1.0).iter().map(|phase| 0.999 - phase).collect();
    let silent = vec![0.0; 400];
    let out = process(m.as_mut(), &falling, &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger).len(), 4);
}

/// A phase outside 0 to 1 is wrapped, so a large one still picks a step.
#[test]
fn a_phase_outside_the_range_is_wrapped() {
    let mut m = module(&[(Euclid::STEPS, 4.0), (Euclid::HITS, 4.0)]);
    let phase = [0.1, 2.3, -0.4, f32::MAX];
    let silent = [0.0; 4];
    let out = process(m.as_mut(), &phase, &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0, 1, 2, 3]);
}

/// A bad phase holds the step, with no false entry when it ends.
#[test]
fn a_bad_phase_holds_the_step() {
    let mut m = module(&[(Euclid::STEPS, 2.0), (Euclid::HITS, 2.0)]);
    let phase = [0.1, f32::NAN, f32::INFINITY, 0.2];
    let silent = [0.0; 4];
    let out = process(m.as_mut(), &phase, &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0]);
    assert_eq!(Outputs::high(&out.gate).len(), 4);
}

/// Changing `hits` mid-step takes effect at the next step (spec, acceptance).
#[test]
fn a_change_of_hits_waits_for_the_next_step() {
    let mut m = module(&[(Euclid::STEPS, 2.0), (Euclid::HITS, 2.0)]);
    let silent = vec![0.0; 100];
    process(m.as_mut(), &ramp(100, 0.25), &silent, &silent);
    m.set_param(Euclid::HITS, 1.0);
    // The rest of step 0, then step 1, which the new pattern leaves empty.
    let rest: Vec<f32> = (100..400).map(|i| i as f32 / 400.0).collect();
    let silent = vec![0.0; rest.len()];
    let out = process(m.as_mut(), &rest, &silent, &silent);
    let gate = Outputs::high(&out.gate);
    assert_eq!(
        gate,
        (0..100).collect::<Vec<_>>(),
        "high until step 0 ends, at frame 200"
    );
}

/// A turn of `steps` also waits: the step ends where the old count said.
#[test]
fn a_change_of_steps_waits_for_the_next_step() {
    let mut m = module(&[(Euclid::STEPS, 2.0), (Euclid::HITS, 2.0)]);
    let silent = vec![0.0; 100];
    process(m.as_mut(), &ramp(100, 0.25), &silent, &silent);
    m.set_param(Euclid::STEPS, 8.0);
    let rest: Vec<f32> = (100..400).map(|i| i as f32 / 400.0).collect();
    let silent = vec![0.0; rest.len()];
    let out = process(m.as_mut(), &rest, &silent, &silent);
    let fired = Outputs::high(&out.trigger);
    assert_eq!(fired.first(), Some(&100), "the first entry is at frame 200: {fired:?}");
}

/// A clock on the first frame enters step 0 (decision in the spec).
#[test]
fn a_clock_on_the_first_frame_plays_step_0() {
    let mut m = clocked(8.0, 3.0, 0.0);
    let clock = clock_pulses(80, 0);
    let silent = vec![0.0; 80];
    let out = process(m.as_mut(), &silent, &clock, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0, 30, 60]);
    assert_eq!(Outputs::high(&out.accent), [0]);
}

/// The first clock long after frame 0, as when a module is added during
/// playback or a clock is patched in later, plays step 0, and nothing plays
/// before it.
#[test]
fn a_late_first_clock_plays_step_0() {
    let mut m = clocked(8.0, 3.0, 0.0);
    let clock = clock_pulses(180, 100);
    let silent = vec![0.0; 180];
    let out = process(m.as_mut(), &silent, &clock, &silent);
    assert_eq!(Outputs::high(&out.trigger), [100, 130, 160]);
    assert_eq!(Outputs::high(&out.accent), [100]);
    assert!(Outputs::high(&out.gate).iter().all(|&frame| frame >= 100));
}

/// A reset ahead of a clock leaves step 0 to that clock, and nothing fires on
/// the reset's frame, so the pattern does not run one clock ahead.
#[test]
fn a_reset_ahead_of_a_clock_leaves_step_0_to_it() {
    let mut m = clocked(4.0, 4.0, 0.0);
    let clock = clock_pulses(60, 0);
    let mut reset = vec![0.0; 60];
    reset[25] = 1.0;
    let silent = vec![0.0; 60];
    let out = process(m.as_mut(), &silent, &clock, &reset);
    assert_eq!(Outputs::high(&out.trigger), [0, 10, 20, 30, 40, 50]);
    assert_eq!(
        Outputs::high(&out.accent),
        [0, 30],
        "the clock after the reset is step 0"
    );
    assert!(!Outputs::high(&out.gate).contains(&25), "waiting is low");
}

/// A reset and a clock on the same frame give step 0 (spec, acceptance).
#[test]
fn a_reset_and_a_clock_together_give_step_0() {
    let mut m = clocked(4.0, 4.0, 0.0);
    let clock = clock_pulses(40, 0);
    let mut reset = vec![0.0; 40];
    reset[20] = 1.0;
    let silent = vec![0.0; 40];
    let out = process(m.as_mut(), &silent, &clock, &reset);
    assert_eq!(Outputs::high(&out.accent), [0, 20]);
}

/// With fewer steps than the current step, the next clock wraps to 0.
#[test]
fn lowering_the_steps_wraps_the_next_clock() {
    let mut m = clocked(16.0, 16.0, 0.0);
    let clock = clock_pulses(150, 0);
    let silent = vec![0.0; 150];
    process(m.as_mut(), &silent, &clock, &silent);
    m.set_param(Euclid::STEPS, 8.0);
    // Step 14 is playing. The next clock is step 0 of the shorter pattern.
    let clock = clock_pulses(10, 9);
    let silent = vec![0.0; 10];
    let out = process(m.as_mut(), &silent, &clock, &silent);
    assert_eq!(Outputs::high(&out.accent), [9]);
}

/// In Clock mode the phase is ignored, and in Phase mode the clock and reset are.
#[test]
fn each_source_ignores_the_other() {
    let frames = 40;
    let (clock, silent) = (clock_pulses(frames, 0), vec![0.0; frames]);
    let mut by_clock = clocked(4.0, 4.0, 0.0);
    let out = process(by_clock.as_mut(), &ramp(frames, 5.0), &silent, &silent);
    assert!(Outputs::high(&out.trigger).is_empty(), "no clock, no step");

    let mut by_phase = module(&[(Euclid::STEPS, 4.0), (Euclid::HITS, 4.0)]);
    let still = vec![0.1; frames];
    let out = process(by_phase.as_mut(), &still, &clock, &clock);
    assert_eq!(Outputs::high(&out.trigger), [0]);
}

/// Going from Clock to Phase enters the phase's step. Going back stays on the
/// current step, with no wait (spec).
#[test]
fn a_change_of_source_follows_the_spec() {
    let mut m = clocked(4.0, 4.0, 0.0);
    let silent = [0.0; 2];
    process(m.as_mut(), &silent, &[1.0, 0.0], &silent);
    m.set_param(Euclid::SOURCE, 0.0);
    let out = process(m.as_mut(), &[0.6, 0.6], &silent, &silent);
    assert_eq!(Outputs::high(&out.trigger), [0], "phase 0.6 is step 2");
    m.set_param(Euclid::SOURCE, 1.0);
    let out = process(m.as_mut(), &[0.6, 0.6], &[1.0, 0.0], &silent);
    assert_eq!(Outputs::high(&out.trigger), [0], "the clock moves on from step 2");
    assert!(Outputs::high(&out.accent).is_empty(), "step 3 is not step 0");
}

/// `reset()` goes back to the wait, and a copy keeps the wait and the step.
#[test]
fn a_copy_and_a_reset_keep_the_wait() {
    let mut running = clocked(4.0, 4.0, 0.0);
    let silent = [0.0; 2];
    let mut copy = clocked(4.0, 4.0, 0.0);
    process(running.as_mut(), &silent, &[1.0, 0.0], &silent);
    assert!(copy.copy_state_from(&*running));
    let out = process(copy.as_mut(), &silent, &[1.0, 0.0], &silent);
    assert!(Outputs::high(&out.accent).is_empty(), "the copy carries on from step 0");

    copy.reset();
    let out = process(copy.as_mut(), &silent, &[1.0, 0.0], &silent);
    assert_eq!(Outputs::high(&out.accent), [0], "the next clock is step 0 again");
}
