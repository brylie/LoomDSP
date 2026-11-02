use super::*;
use loom_core::testing::run;

const RATE: f32 = 48_000.0;

/// A Random module with these parameters, by name, at their values at once.
fn random(params: &[(&str, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = RANDOM.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: 64,
    });
    for (name, value) in params {
        m.set_param(RANDOM.param_index(name).unwrap(), *value);
    }
    m.reset();
    m
}

/// Renders both outputs in blocks of `block`.
fn render(m: &mut dyn Module, trigger: &[f32], reset: &[f32], block: usize) -> [Vec<f32>; 2] {
    let frames = trigger.len();
    let (mut out, mut out2) = (vec![0.0; frames], vec![0.0; frames]);
    for start in (0..frames).step_by(block) {
        let end = (start + block).min(frames);
        run(
            m,
            RATE,
            &[&trigger[start..end], &reset[start..end]],
            &[true, true],
            &mut [&mut out[start..end], &mut out2[start..end]],
            &[],
        );
    }
    [out, out2]
}

/// One-sample triggers every other frame, for `count` triggers.
fn clock(count: usize) -> Vec<f32> {
    (0..count * 2).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect()
}

/// Both outputs start in the middle of the range and hold until a trigger.
#[test]
fn outputs_start_in_the_middle_and_hold() {
    let mut m = random(&[("low", 2.0), ("high", 4.0)]);
    let [out, out2] = render(m.as_mut(), &[0.0; 16], &[0.0; 16], 16);
    assert!(out.iter().chain(&out2).all(|&v| v == 3.0));
}

/// Over 100,000 triggers with Uniform, each tenth of the range gets 10% of
/// the values, within 1 percentage point, on both outputs.
#[test]
fn uniform_fills_each_tenth_of_the_range() {
    let trigger = clock(100_000);
    let mut m = random(&[]);
    for out in render(m.as_mut(), &trigger, &vec![0.0; trigger.len()], 64) {
        let mut tenths = [0u32; 10];
        for value in out.iter().step_by(2) {
            tenths[((value * 10.0) as usize).min(9)] += 1;
        }
        for (tenth, count) in tenths.iter().enumerate() {
            let share = *count as f32 / 100_000.0;
            assert!((0.09..=0.11).contains(&share), "tenth {tenth}: {share}");
        }
    }
}

/// Centred puts more values in the middle fifth than in either outer fifth.
#[test]
fn centred_prefers_the_middle() {
    let trigger = clock(20_000);
    let mut m = random(&[("shape", 1.0)]);
    let [out, _] = render(m.as_mut(), &trigger, &vec![0.0; trigger.len()], 64);
    let count = |low: f32, high: f32| out.iter().step_by(2).filter(|&&v| v >= low && v < high).count();
    let middle = count(0.4, 0.6);
    assert!(middle > count(0.0, 0.2) * 2, "{middle}");
    assert!(middle > count(0.8, 1.0) * 2, "{middle}");
}

/// `out2` draws on its own: the outputs are not the same values or related
/// by a fixed offset.
#[test]
fn the_two_outputs_are_independent() {
    let trigger = clock(10_000);
    let mut m = random(&[]);
    let [out, out2] = render(m.as_mut(), &trigger, &vec![0.0; trigger.len()], 64);
    let (mean, mean2) = (
        out.iter().sum::<f32>() / out.len() as f32,
        out2.iter().sum::<f32>() / out2.len() as f32,
    );
    let (mut together, mut spread, mut spread2) = (0.0, 0.0, 0.0);
    for (a, b) in out.iter().zip(&out2) {
        together += (a - mean) * (b - mean2);
        spread += (a - mean).powi(2);
        spread2 += (b - mean2).powi(2);
    }
    let correlation = together / (spread * spread2).sqrt();
    assert!(correlation.abs() < 0.05, "{correlation}");
}

/// A walk never leaves the range, and reaches both ends of it.
#[test]
fn a_walk_stays_in_the_range_and_covers_it() {
    let trigger = clock(50_000);
    for step in [0.1, 1.0] {
        let mut m = random(&[("mode", 1.0), ("step", step), ("low", -2.0), ("high", 3.0)]);
        for out in render(m.as_mut(), &trigger, &vec![0.0; trigger.len()], 64) {
            let (min, max) = out
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            assert!(min >= -2.0 && max <= 3.0, "step {step}: {min} to {max}");
            assert!(min < -1.5 && max > 2.5, "step {step}: {min} to {max}");
        }
    }
}

/// Each walk move is no larger than `step`, measured in the range.
#[test]
fn a_walk_moves_by_at_most_the_step() {
    let trigger = clock(10_000);
    let mut m = random(&[("mode", 1.0), ("step", 0.05)]);
    let [out, _] = render(m.as_mut(), &trigger, &vec![0.0; trigger.len()], 64);
    for pair in out.windows(2) {
        assert!((pair[1] - pair[0]).abs() <= 0.05 + 1e-5, "{pair:?}");
    }
    assert!(out.windows(2).any(|pair| pair[0] != pair[1]));
}

/// An overshoot reflects back; it does not stop at the edge.
#[test]
fn edges_bounce() {
    assert_eq!(bounce(-0.25), 0.25);
    assert_eq!(bounce(1.25), 0.75);
    assert_eq!(bounce(0.5), 0.5);
    assert_eq!(bounce(-1.0), 1.0);
    assert_eq!(bounce(2.0), 0.0);
}

/// A modulated range rescales the output at once, and the walk stays inside.
#[test]
fn a_moved_range_rescales_the_walk() {
    let trigger = clock(2_000);
    let mut m = random(&[("mode", 1.0), ("step", 0.5)]);
    let [before, _] = render(m.as_mut(), &trigger, &vec![0.0; trigger.len()], 64);
    assert!(before.iter().all(|v| (0.0..=1.0).contains(v)));
    m.set_param(RANDOM.param_index("low").unwrap(), 5.0);
    m.set_param(RANDOM.param_index("high").unwrap(), 6.0);
    let settle = 48_000 / 10;
    let quiet = vec![0.0; settle];
    let [held, _] = render(m.as_mut(), &quiet, &quiet, 64);
    let last = *held.last().unwrap();
    assert!((5.0..=6.0).contains(&last), "{last}");
    let [after, _] = render(m.as_mut(), &trigger, &vec![0.0; trigger.len()], 64);
    assert!(after.iter().all(|v| (5.0..=6.0).contains(v)));
}

/// Crossed limits swap: the same values as the ordered range.
#[test]
fn crossed_limits_swap() {
    let trigger = clock(100);
    let reset = vec![0.0; trigger.len()];
    let [ordered, _] = render(random(&[("low", -1.0), ("high", 1.0)]).as_mut(), &trigger, &reset, 64);
    let [crossed, _] = render(random(&[("low", 1.0), ("high", -1.0)]).as_mut(), &trigger, &reset, 64);
    assert_eq!(ordered, crossed);
}

/// A reset replays the same values from the start, in step and walk mode. On
/// the same frame as a trigger it comes first.
#[test]
fn a_reset_replays_the_values() {
    let trigger = clock(2_000);
    let mut reset = vec![0.0; trigger.len()];
    reset[2_000] = 1.0;
    for mode in [0.0, 1.0] {
        let mut m = random(&[("mode", mode)]);
        for out in render(m.as_mut(), &trigger, &reset, 64) {
            assert_eq!(out[..2_000], out[2_000..], "mode {mode}");
        }
    }
}

/// In step mode a reset alone does not move the outputs.
#[test]
fn a_reset_in_step_mode_holds_the_output() {
    let mut trigger = vec![0.0; 100];
    trigger[0] = 1.0;
    let mut reset = vec![0.0; 100];
    reset[50] = 1.0;
    let mut m = random(&[]);
    let [out, _] = render(m.as_mut(), &trigger, &reset, 64);
    assert!(out.iter().all(|&v| v == out[0]));
}

/// With `glide` at 0.1 s, a new value is reached after 0.1 s in a straight line.
#[test]
fn a_glide_reaches_its_value_in_a_straight_line() {
    let frames = 4_800;
    let mut trigger = vec![0.0; frames + 100];
    trigger[0] = 1.0;
    let reset = vec![0.0; trigger.len()];
    let mut m = random(&[("glide", 0.1), ("low", 0.0), ("high", 1.0)]);
    let [out, _] = render(m.as_mut(), &trigger, &reset, 64);
    let target = out[frames];
    assert_ne!(target, 0.5);
    assert_eq!(out[0], 0.5);
    for (i, value) in out[..frames].iter().enumerate() {
        let line = 0.5 + (target - 0.5) * (i as f32 / frames as f32);
        assert!((value - line).abs() < 1e-4, "frame {i}: {value}, line {line}");
    }
    assert!(out[frames..].iter().all(|&v| v == target));
}

/// A trigger during a glide starts a new glide from where the output is.
#[test]
fn a_trigger_during_a_glide_starts_from_where_the_output_is() {
    let mut trigger = vec![0.0; 2_000];
    trigger[0] = 1.0;
    trigger[500] = 1.0;
    let reset = vec![0.0; 2_000];
    let mut m = random(&[("glide", 0.1)]);
    let [out, _] = render(m.as_mut(), &trigger, &reset, 64);
    assert!((out[500] - out[499]).abs() < 1e-3, "{} then {}", out[499], out[500]);
}

/// In walk mode, a reset ends a running glide: the outputs jump to the middle.
#[test]
fn a_reset_in_walk_mode_ends_a_glide_at_the_middle() {
    let mut trigger = vec![0.0; 1_000];
    trigger[0] = 1.0;
    let mut reset = vec![0.0; 1_000];
    reset[500] = 1.0;
    let mut m = random(&[("mode", 1.0), ("step", 1.0), ("glide", 1.0)]);
    let [out, _] = render(m.as_mut(), &trigger, &reset, 64);
    assert_ne!(out[499], 0.5);
    assert!(out[500..].iter().all(|&v| v == 0.5));
}

/// In step mode, a reset leaves a running glide to finish, then holds.
#[test]
fn a_reset_in_step_mode_lets_a_glide_finish() {
    let frames = 960;
    let mut trigger = vec![0.0; frames * 2];
    trigger[0] = 1.0;
    let mut reset = vec![0.0; frames * 2];
    reset[frames / 2] = 1.0;
    let glide = frames as f32 / RATE;
    let with_reset = render(random(&[("glide", glide)]).as_mut(), &trigger, &reset, 64);
    let without = render(
        random(&[("glide", glide)]).as_mut(),
        &trigger,
        &vec![0.0; frames * 2],
        64,
    );
    assert_eq!(with_reset, without);
}

/// The same seed gives the same values at any block size, and another seed
/// gives others.
#[test]
fn values_depend_on_the_seed_and_not_the_block_size() {
    let trigger = clock(500);
    let reset = vec![0.0; trigger.len()];
    let values = |seed: f32, block: usize| render(random(&[("seed", seed)]).as_mut(), &trigger, &reset, block);
    let first = values(1.0, 1);
    assert_eq!(first, values(1.0, 77));
    assert_eq!(first, values(1.0, 512));
    assert_ne!(first, values(2.0, 512));
}

/// Changing the shape does not shift later numbers: each trigger always
/// takes the same number of draws.
#[test]
fn a_change_of_shape_does_not_shift_the_stream() {
    let trigger = clock(4);
    let reset = vec![0.0; trigger.len()];
    let values = |shape: [f32; 2]| {
        let mut m = random(&[("shape", shape[0])]);
        let [first, _] = render(m.as_mut(), &trigger, &reset, 64);
        m.set_param(RANDOM.param_index("shape").unwrap(), shape[1]);
        let [second, _] = render(m.as_mut(), &trigger, &reset, 64);
        (first, second)
    };
    assert_eq!(values([0.0, 0.0]).1, values([1.0, 0.0]).1);
}
