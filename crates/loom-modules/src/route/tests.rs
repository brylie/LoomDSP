use super::*;
use loom_core::testing::run;

/// A Selector with these parameters, as the `Controller` starts one, with no
/// fade unless a test sets one.
fn selector(params: &[(usize, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = SELECTOR.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 4_096,
    });
    m.set_param(Selector::FADE, 0.0);
    for &(index, value) in params {
        m.set_param(index, value);
    }
    m.reset();
    m
}

/// Input `n` of eight holds `n + 1`, so the output names the picked input, from 1.
fn numbered(frames: usize) -> Vec<Vec<f32>> {
    (0..INPUTS).map(|n| vec![(n + 1) as f32; frames]).collect()
}

/// The module's output, with `step`, `select`, and `reset` on those inputs.
fn process(m: &mut dyn Module, signals: &[Vec<f32>], step: &[f32], select: &[f32], reset: &[f32]) -> Vec<f32> {
    let mut inputs: Vec<&[f32]> = signals.iter().map(Vec::as_slice).collect();
    inputs.extend([step, select, reset]);
    let mut out = vec![0.0; step.len()];
    run(m, 48_000.0, &inputs, &vec![true; inputs.len()], &mut [&mut out], &[]);
    out
}

/// A step edge on each of these frames of a render.
fn edges(frames: usize, at: &[usize]) -> Vec<f32> {
    (0..frames).map(|i| if at.contains(&i) { 1.0 } else { 0.0 }).collect()
}

/// The module's output after one frame of Select mode at `value`, with `count`
/// inputs.
fn selected(count: f32, value: f32) -> f32 {
    let mut m = selector(&[(Selector::MODE, 1.0), (Selector::COUNT, count)]);
    process(m.as_mut(), &numbered(1), &[0.0], &[value], &[0.0])[0]
}

/// With 4 inputs in Select mode, 0.6 picks the third; -1 and 2 clamp to the
/// first and fourth (spec, acceptance).
#[test]
fn select_picks_by_value_and_clamps() {
    assert_eq!(selected(4.0, 0.6), 3.0);
    assert_eq!(selected(4.0, -1.0), 1.0);
    assert_eq!(selected(4.0, 2.0), 4.0);
    assert_eq!(selected(4.0, 1.0), 4.0);
}

/// A bad `select` holds the current input.
#[test]
fn a_bad_select_holds_the_input() {
    let mut m = selector(&[(Selector::MODE, 1.0)]);
    let out = process(
        m.as_mut(),
        &numbered(3),
        &[0.0; 3],
        &[0.6, f32::NAN, f32::INFINITY],
        &[0.0; 3],
    );
    assert_eq!(out, [3.0, 3.0, 3.0]);
}

/// It starts on input 1, the first step stays there, and later ones move on,
/// wrapping after `count` (decision in the spec).
#[test]
fn step_stays_on_input_one_then_moves() {
    let mut m = selector(&[(Selector::COUNT, 3.0)]);
    let frames = 6;
    let out = process(
        m.as_mut(),
        &numbered(frames),
        &[1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
        &[0.0; 6],
        &[0.0; 6],
    );
    assert_eq!(out, [1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
    let out = process(m.as_mut(), &numbered(2), &[1.0, 0.0], &[0.0; 2], &[0.0; 2]);
    assert_eq!(out, [1.0, 1.0], "wraps after the third input");
}

/// After a reset, the next step stays on input 1, and one on the reset's frame
/// counts as that step (decision in the spec).
#[test]
fn a_reset_goes_back_to_the_wait() {
    let mut m = selector(&[]);
    let frames = 12;
    let step = edges(frames, &[0, 2, 4, 8, 10]);
    let reset = edges(frames, &[6]);
    let out = process(m.as_mut(), &numbered(frames), &step, &vec![0.0; frames], &reset);
    assert_eq!(out[5], 3.0);
    assert_eq!(out[6], 1.0, "the reset goes to input 1");
    assert_eq!(out[8], 1.0, "the next step stays");
    assert_eq!(out[10], 2.0, "the one after moves");

    let mut m = selector(&[]);
    let step = edges(6, &[0, 2, 4]);
    let reset = edges(6, &[4]);
    let out = process(m.as_mut(), &numbered(6), &step, &[0.0; 6], &reset);
    assert_eq!(out[4], 1.0, "a reset and a step together give input 1");
}

/// In Select mode a step does nothing, and it does not end the wait.
#[test]
fn a_step_in_select_mode_keeps_the_wait() {
    let mut m = selector(&[(Selector::MODE, 1.0)]);
    process(m.as_mut(), &numbered(2), &[1.0, 0.0], &[0.0; 2], &[0.0; 2]);
    m.set_param(Selector::MODE, 0.0);
    let out = process(m.as_mut(), &numbered(2), &[1.0, 0.0], &[0.0; 2], &[0.0; 2]);
    assert_eq!(out, [1.0, 1.0], "the first step in Step mode still stays");
}

/// Lowering `count` below the current input moves to the last in use (spec).
#[test]
fn lowering_the_count_moves_to_the_last_input() {
    let mut m = selector(&[(Selector::COUNT, 8.0)]);
    let step = edges(8, &[0, 2, 4, 6]);
    assert_eq!(process(m.as_mut(), &numbered(8), &step, &[0.0; 8], &[0.0; 8])[7], 4.0);
    m.set_param(Selector::COUNT, 2.0);
    assert_eq!(process(m.as_mut(), &numbered(1), &[0.0], &[0.0], &[0.0])[0], 2.0);
}

/// The picks of a Random selector over `steps` steps, with `params`.
fn picks(steps: usize, params: &[(usize, f32)]) -> Vec<usize> {
    let mut all = vec![(Selector::MODE, 2.0)];
    all.extend_from_slice(params);
    let mut m = selector(&all);
    let frames = steps * 2;
    let step: Vec<f32> = (0..frames).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect();
    let out = process(
        m.as_mut(),
        &numbered(frames),
        &step,
        &vec![0.0; frames],
        &vec![0.0; frames],
    );
    out.iter().step_by(2).map(|&value| value as usize).collect()
}

/// Random with Avoid never picks the current input twice in a row over 10,000
/// steps (spec, acceptance), and uses every input.
#[test]
fn avoid_never_repeats() {
    let picks = picks(10_000, &[(Selector::COUNT, 4.0)]);
    assert!(picks.windows(2).all(|pair| pair[0] != pair[1]));
    for input in 1..=4 {
        assert!(picks.contains(&input), "{input} is never picked");
    }
    assert!(picks.iter().all(|&input| input <= 4));
}

/// Allow can repeat, and each input gets about its share.
#[test]
fn allow_can_repeat() {
    let picks = picks(10_000, &[(Selector::COUNT, 4.0), (Selector::REPEATS, 1.0)]);
    assert!(picks.windows(2).any(|pair| pair[0] == pair[1]));
    for input in 1..=4 {
        let share = picks.iter().filter(|&&pick| pick == input).count() as f32 / 10_000.0;
        assert!((share - 0.25).abs() < 0.02, "input {input}: {share}");
    }
}

/// A seed gives the same picks every time, and another seed gives others.
#[test]
fn a_seed_replays_its_picks() {
    assert_eq!(picks(64, &[(Selector::SEED, 7.0)]), picks(64, &[(Selector::SEED, 7.0)]));
    assert_ne!(picks(64, &[(Selector::SEED, 7.0)]), picks(64, &[(Selector::SEED, 8.0)]));
}

/// A reset restarts the picks, and a step on its frame takes the first one
/// (rule 2; decision in the spec).
#[test]
fn a_reset_replays_the_random_picks() {
    let mut m = selector(&[(Selector::MODE, 2.0), (Selector::COUNT, 8.0)]);
    let frames = 16;
    let (every, silent) = (edges(frames, &[0, 2, 4, 6, 8, 10, 12, 14]), vec![0.0; frames]);
    let first = process(m.as_mut(), &numbered(frames), &every, &silent, &silent);
    let reset = edges(frames, &[0]);
    let second = process(m.as_mut(), &numbered(frames), &every, &silent, &reset);
    assert_eq!(first[..], second[..]);
}

/// With a fade, no step in the output is larger than the step between the
/// inputs (spec, acceptance). A lone fade is a straight line, reaching its pick
/// in the fade's time. Switches faster than the fade divide by a smaller sum, so
/// they move faster, but they stay far from a jump and never overshoot.
#[test]
fn a_fade_never_jumps() {
    let mut m = selector(&[(Selector::FADE, 0.01)]);
    let frames = 2_000;
    let step = edges(frames, &[0, 100, 200, 800]);
    let mut signals = vec![
        vec![0.0; frames],
        vec![1.0; frames],
        vec![-1.0; frames],
        vec![0.5; frames],
    ];
    signals.extend((4..INPUTS).map(|_| vec![0.0; frames]));
    let out = process(m.as_mut(), &signals, &step, &vec![0.0; frames], &vec![0.0; frames]);

    let moved = |range: std::ops::Range<usize>| {
        out[range]
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0f32, f32::max)
    };
    assert!(
        moved(800..1_500) <= 1.5 / 480.0 + 1e-4,
        "a lone fade is one straight line"
    );
    assert!(moved(0..800) < 0.05, "two switches inside one fade, against a gap of 2");
    assert!(
        out.iter().all(|value| (-1.0..=1.0).contains(value)),
        "stays between the inputs"
    );
    assert_eq!(out[1_999], 0.5, "reaches the last pick");
}

/// Faster switches than the fade keep the level steady: three equal inputs give
/// the same level all through (spec, the sum of the gains).
#[test]
fn fast_switches_keep_the_level() {
    let mut m = selector(&[(Selector::FADE, 0.1), (Selector::COUNT, 3.0)]);
    let frames = 4_000;
    let step = edges(frames, &[0, 500, 1_000, 1_500, 2_000]);
    let same = vec![vec![0.5; frames]; INPUTS];
    let out = process(m.as_mut(), &same, &step, &vec![0.0; frames], &vec![0.0; frames]);
    assert!(out.iter().all(|&value| (value - 0.5).abs() < 1e-5), "{:?}", &out[..4]);
}

/// An infinite input that is not in use cannot make NaN.
#[test]
fn an_unused_bad_input_stays_out() {
    let mut m = selector(&[]);
    let mut signals = numbered(4);
    signals[1] = vec![f32::INFINITY; 4];
    signals[2] = vec![f32::NAN; 4];
    let out = process(m.as_mut(), &signals, &[0.0; 4], &[0.0; 4], &[0.0; 4]);
    assert_eq!(out, [1.0; 4]);
}

/// Large inputs fading into each other stay finite, because each gain is shared
/// out before the sum.
#[test]
fn large_inputs_stay_finite_in_a_fade() {
    let mut m = selector(&[(Selector::FADE, 0.01), (Selector::COUNT, 2.0)]);
    let step = edges(960, &[0, 10]);
    let signals = vec![vec![f32::MAX / 2.0 * 1.5; 960]; INPUTS];
    let out = process(m.as_mut(), &signals, &step, &[0.0; 960], &[0.0; 960]);
    assert!(out.iter().all(|value| value.is_finite()));
}

/// A copy keeps the wait, and the stream, so it makes the same next pick.
#[test]
fn a_copy_keeps_the_wait_and_the_stream() {
    let (running, mut copy) = (selector(&[]), selector(&[]));
    assert!(copy.copy_state_from(&*running));
    let out = process(copy.as_mut(), &numbered(2), &[1.0, 0.0], &[0.0; 2], &[0.0; 2]);
    assert_eq!(out, [1.0, 1.0], "the copy is still waiting");

    let mut running = selector(&[(Selector::MODE, 2.0), (Selector::COUNT, 8.0)]);
    let mut copy = selector(&[(Selector::MODE, 2.0), (Selector::COUNT, 8.0)]);
    let step = edges(40, &[0, 10, 20]);
    process(running.as_mut(), &numbered(40), &step, &[0.0; 40], &[0.0; 40]);
    assert!(copy.copy_state_from(&*running));
    let every = edges(100, &[0, 20, 40, 60, 80]);
    let a = process(running.as_mut(), &numbered(100), &every, &[0.0; 100], &[0.0; 100]);
    let b = process(copy.as_mut(), &numbered(100), &every, &[0.0; 100], &[0.0; 100]);
    assert_eq!(a, b);
}
