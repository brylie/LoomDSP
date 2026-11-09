use super::*;

/// Equal-power gains keep the sum of their squares at 1 everywhere, start
/// and end on one signal, and meet at 1/√2 in the middle (-3 dB each), where
/// linear gains would meet at 0.5 (-6 dB).
#[test]
fn equal_power_gains_keep_the_power() {
    for step in 0..=100 {
        let (a, b) = equal_power(step as f32 / 100.0);
        assert!((a * a + b * b - 1.0).abs() < 1e-6, "step {step}");
    }
    assert_eq!(equal_power(0.0), (1.0, 0.0));
    let (a, b) = equal_power(1.0);
    assert!(a.abs() < 1e-7 && (b - 1.0).abs() < 1e-7);
    let (a, b) = equal_power(0.5);
    assert!((a - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((b - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert_eq!(equal_power(-1.0), equal_power(0.0));
    assert_eq!(equal_power(f32::NAN), equal_power(0.5));
}

/// The constant-power pan law: hard left and right put everything on one
/// side, the centre is -3 dB on each, and the power is 1 everywhere.
#[test]
fn the_pan_law_keeps_the_power() {
    let (left, right) = pan(-1.0);
    assert!((left - 1.0).abs() < 1e-7 && right.abs() < 1e-7);
    let (left, right) = pan(1.0);
    assert!(left.abs() < 1e-7 && (right - 1.0).abs() < 1e-7);
    let (left, right) = pan(0.0);
    let centre_db = 20.0 * left.log10();
    assert!((centre_db + 3.0103).abs() < 1e-3, "{centre_db} dB");
    assert_eq!(left, right);
    for step in -10..=10 {
        let (left, right) = pan(step as f32 / 10.0);
        assert!((left * left + right * right - 1.0).abs() < 1e-6);
    }
    assert_eq!(pan(f32::NAN), pan(0.0));
    assert_eq!(pan(3.0), pan(1.0));
}

/// 1/√N copies of N uncorrelated signals of power P sum to power P: the
/// variance of a sum of independent variables is the sum of their variances.
/// Checked with seeded noise, which is uncorrelated between seeds.
#[test]
fn unison_gain_keeps_uncorrelated_copies_at_the_loudness_of_one() {
    assert_eq!(unison_gain(0), 1.0);
    assert_eq!(unison_gain(1), 1.0);
    assert!((unison_gain(4) - 0.5).abs() < 1e-7);
    let frames = 200_000;
    for copies in [2_u32, 4, 9, 16] {
        let mut sources: Vec<crate::random::Rng> = (0..copies).map(|seed| crate::random::Rng::new(seed + 1)).collect();
        let (mut one, mut sum) = (0.0_f64, 0.0_f64);
        for _ in 0..frames {
            let values: Vec<f32> = sources.iter_mut().map(|s| 2.0 * s.draw() - 1.0).collect();
            one += f64::from(values[0]).powi(2);
            let mixed = unison_gain(copies) * values.iter().sum::<f32>();
            sum += f64::from(mixed).powi(2);
        }
        let ratio_db = 10.0 * (sum / one).log10();
        assert!(ratio_db.abs() < 0.1, "{copies} copies: {ratio_db} dB");
    }
}

/// Identical copies sum coherently: 1/√N of N equal signals is √N times one,
/// +6 dB for 4. Spread is what makes the copies differ.
#[test]
fn identical_copies_sum_louder() {
    let mixed = unison_gain(4) * 4.0;
    assert!((20.0 * mixed.log10() - 6.0206).abs() < 1e-3);
}

/// Offsets run evenly from -1 to 1, and one copy sits at 0.
#[test]
fn spread_offsets_are_even() {
    assert_eq!(spread_offset(0, 1), 0.0);
    assert_eq!(spread_offset(0, 0), 0.0);
    assert_eq!(spread_offset(0, 2), -1.0);
    assert_eq!(spread_offset(1, 2), 1.0);
    let four: Vec<f32> = (0..4).map(|i| spread_offset(i, 4)).collect();
    let third = 1.0 / 3.0;
    for (got, want) in four.iter().zip([-1.0, -third, third, 1.0]) {
        assert!((got - want).abs() < 1e-6, "{four:?}");
    }
    assert_eq!(spread_offset(9, 4), 1.0);
}

/// A fade starts on the old member alone, moves on a quarter circle, and
/// ends on the new one alone after its length.
#[test]
fn a_fade_runs_its_length_on_a_quarter_circle() {
    let mut fade = MemberFade::new(4, 0);
    assert_eq!(
        fade.tick(0),
        Blend {
            from: 0,
            to: 0,
            gain_from: 0.0,
            gain_to: 1.0
        }
    );
    let frames: Vec<Blend> = (0..5).map(|_| fade.tick(1)).collect();
    for (n, blend) in frames.iter().take(4).enumerate() {
        let (gain_from, gain_to) = equal_power(n as f32 / 4.0);
        assert_eq!((blend.from, blend.to), (0, 1));
        assert_eq!((blend.gain_from, blend.gain_to), (gain_from, gain_to));
    }
    assert_eq!(
        frames[4],
        Blend {
            from: 1,
            to: 1,
            gain_from: 0.0,
            gain_to: 1.0
        }
    );
    assert!(!fade.is_fading());
    assert_eq!(fade.member(), 1);
}

/// Going back to the member fading out reverses the fade with no step: the
/// output on the turning frame is what it would have been, and it then
/// returns along the same curve.
#[test]
fn going_back_reverses_without_a_step() {
    let (a, b) = (0.3_f32, -0.8_f32);
    let mix = |blend: Blend| {
        let value = |member| if member == 0 { a } else { b };
        value(blend.from) * blend.gain_from + value(blend.to) * blend.gain_to
    };
    let mut fade = MemberFade::new(480, 0);
    let mut last = mix(fade.tick(0));
    let mut largest_step = 0.0_f32;
    let mut steps = Vec::new();
    for _ in 0..200 {
        let value = mix(fade.tick(1));
        steps.push(value);
        largest_step = largest_step.max((value - last).abs());
        last = value;
    }
    // Back to member 0: the next frames retrace the way they came.
    let turned = fade.tick(0);
    let on_turn = mix(turned);
    let mut ahead = MemberFade::new(480, 0);
    ahead.tick(0);
    let mut continued = 0.0;
    for _ in 0..201 {
        continued = mix(ahead.tick(1));
    }
    assert!((on_turn - continued).abs() < 1e-6, "{on_turn} vs {continued}");
    largest_step = largest_step.max((on_turn - last).abs());
    for back in (0..200).rev() {
        let value = mix(fade.tick(0));
        largest_step = largest_step.max((value - last).abs());
        last = value;
        assert!((value - steps[back]).abs() < 1e-5, "frame {back}");
    }
    // The largest step is the curve's own, about (π/2)/480 of the swing.
    let swing = (a - b).abs().max(a.abs() + b.abs());
    assert!(largest_step <= swing * core::f32::consts::FRAC_PI_2 / 480.0 + 1e-6);
}

/// A third member during a fade waits; the fade ends, then a new one starts
/// toward the target of that frame, and only the latest target counts.
#[test]
fn a_third_member_waits_for_the_fade() {
    let mut fade = MemberFade::new(4, 0);
    fade.tick(1);
    fade.tick(2);
    fade.tick(3);
    let blend = fade.tick(2);
    assert_eq!((blend.from, blend.to), (0, 1));
    // Frame 4 of the fade ends it; the target is now 2, so a new fade starts.
    let next = fade.tick(2);
    assert_eq!((next.from, next.to), (1, 2));
    assert_eq!(next.gain_from, 1.0);
}

/// A linear fade keeps a value both members share exactly, where equal
/// power would lift it by up to √2 in the middle; its gains sum to 1.
#[test]
fn a_linear_fade_keeps_a_shared_value() {
    for step in 0..=100 {
        let (a, b) = linear(step as f32 / 100.0);
        assert_eq!(a + b, 1.0, "step {step}");
    }
    assert_eq!(linear(-1.0), (1.0, 0.0));
    assert_eq!(linear(f32::NAN), (0.5, 0.5));
    let mut fade = MemberFade::new(480, 0);
    fade.tick_with(0, Law::Linear);
    let mut largest: f32 = 0.0;
    for n in 0..480 {
        let blend = fade.tick_with(1, Law::Linear);
        let value = 0.2 * blend.gain_from + 0.2 * blend.gain_to;
        largest = largest.max((value - 0.2).abs());
        let _ = n;
    }
    assert!(largest < 1e-7, "{largest}");
    let (a, b) = Law::EqualPower.gains(0.5);
    assert!(
        (0.2 * (a + b) - 0.2 * std::f32::consts::SQRT_2).abs() < 1e-6,
        "equal power lifts it"
    );
}

/// A linear fade turns back without a step, as an equal-power one does.
#[test]
fn a_linear_fade_turns_back_without_a_step() {
    let mut fade = MemberFade::new(100, 0);
    fade.tick_with(0, Law::Linear);
    let mut last = 0.0;
    for _ in 0..40 {
        let b = fade.tick_with(1, Law::Linear);
        last = b.gain_to;
    }
    let turned = fade.tick_with(0, Law::Linear);
    // Member 1's gain on the turning frame is the next one along the way in.
    assert!(
        (turned.gain_from - (last + 0.01)).abs() < 1e-6,
        "{} vs {last}",
        turned.gain_from
    );
}

/// A cut switches on the frame, with no fade.
#[test]
fn a_cut_switches_on_the_frame() {
    let mut fade = MemberFade::new(480, 0);
    fade.tick(1);
    assert!(fade.is_fading());
    assert_eq!(
        fade.cut(2),
        Blend {
            from: 2,
            to: 2,
            gain_from: 0.0,
            gain_to: 1.0
        }
    );
    assert!(!fade.is_fading());
    assert_eq!(fade.tick(2).gain_to, 1.0);
}

/// A zero length is held at one sample, so a fade always ends.
#[test]
fn a_zero_length_fade_is_one_sample() {
    let mut fade = MemberFade::new(0, 0);
    let first = fade.tick(1);
    assert_eq!((first.from, first.to, first.gain_from), (0, 1, 1.0));
    assert_eq!(fade.tick(1).to, 1);
    assert!(!fade.is_fading());
}
