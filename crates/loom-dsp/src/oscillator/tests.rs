use super::*;
use crate::testing::alias_share;

/// A high note that aliases badly when it is naive.
const NOTE: u32 = 3_517;

/// The correction removes at least 90% of a saw's alias power.
#[test]
fn saw_has_far_less_aliasing_than_a_naive_saw() {
    let naive = alias_share(NOTE, |t| 2.0 * t - 1.0);
    let mut saw = Saw::new();
    let corrected = alias_share(NOTE, |t| saw.tick(t));
    assert!(corrected < naive * 0.1, "{corrected} against {naive}");
}

/// The correction removes at least 90% of a pulse's alias power.
#[test]
fn pulse_has_far_less_aliasing_than_a_naive_pulse() {
    let naive = alias_share(NOTE, |t| if t < 0.5 { 1.0 } else { -1.0 });
    let mut pulse = Pulse::new();
    let corrected = alias_share(NOTE, |t| pulse.tick(t, 0.5));
    assert!(corrected < naive * 0.1, "{corrected} against {naive}");
}

/// The correction removes at least 90% of a triangle's alias power.
#[test]
fn triangle_has_far_less_aliasing_than_a_naive_triangle() {
    let naive = alias_share(NOTE, |t| 1.0 - 4.0 * ((t + 0.25).fract() - 0.5).abs());
    let mut triangle = Triangle::new();
    let corrected = alias_share(NOTE, |t| triangle.tick(t));
    assert!(corrected < naive * 0.1, "{corrected} against {naive}");
}

/// The corrections only act near a jump.
#[test]
fn corrections_are_zero_away_from_the_edge() {
    assert_eq!(poly_blep(0.5, 0.01), 0.0);
    assert_eq!(poly_blamp(0.5, 0.01), 0.0);
}

/// Under audio-rate changes of width, the two edge corrections overlap, and the
/// shared step in `pulse` keeps them bounded. This looks before the final limit,
/// which would otherwise hide any fault here.
#[test]
fn pulse_overlap_stays_bounded_with_moving_width() {
    let mut most = 0.0f32;
    let (mut previous, mut previous_fall) = (0.0, 0.0);
    for i in 0..48_000 {
        let t = (i as f32 * 0.21).fract();
        let width = 0.5 + 0.49 * (i as f32 * 0.37).sin();
        let t_fall = wrap(t - width);
        let dt = wrap(t - previous);
        let dt_fall = wrap(t_fall - previous_fall);
        (previous, previous_fall) = (t, t_fall);
        let y = pulse_unclamped(t, width, dt.min(1.0 - dt), dt_fall.min(1.0 - dt_fall));
        most = most.max(y.abs());
    }
    assert!(most <= 1.0, "{most}");
}

/// The correction works the same on a falling phase: a saw read backward
/// loses at least 90% of its alias power (#543).
#[test]
fn descending_saw_has_far_less_aliasing_than_a_naive_saw() {
    let backward = |t: f32| wrap(1.0 - t);
    let naive = alias_share(NOTE, |t| 2.0 * backward(t) - 1.0);
    let mut saw = Saw::new();
    let corrected = alias_share(NOTE, |t| saw.tick(backward(t)));
    assert!(corrected < naive * 0.1, "{corrected} against {naive}");
}

/// A descending phase reads the ramp backward. This looks only away from the
/// edge.
#[test]
fn descending_phase_reads_the_ramp_backward() {
    let mut saw = Saw::new();
    saw.tick(0.5);
    assert!((saw.tick(0.4) - -0.2).abs() < 1e-5);
}

/// Each struct gives what its function gives for the true step. The phases are
/// exact in binary, so the step it works out is exact too.
#[test]
fn structs_match_their_functions() {
    let step = 1.0 / 64.0;
    let width = 0.25;
    let (mut saw_tick, mut pulse_tick, mut triangle_tick) = (Saw::new(), Pulse::new(), Triangle::new());
    pulse_tick.reset(width);
    for i in 0..256 {
        let t = (i % 64) as f32 * step;
        let (s, p, tri) = (saw_tick.tick(t), pulse_tick.tick(t, width), triangle_tick.tick(t));
        // The first frame has no earlier phase to compare with.
        if i > 0 {
            assert_eq!(s, saw(t, step), "saw frame {i}");
            assert_eq!(p, pulse(t, width, step, step), "pulse frame {i}");
            assert_eq!(tri, triangle(t, step), "triangle frame {i}");
        }
    }
}

/// A jump in the phase, as from a reset or a hard sync. The struct reads the
/// jump as one large step, so its correction has the wrong width. The function
/// knows the true step and gets it right. This is why both layers exist.
#[test]
fn a_known_step_is_not_fooled_by_a_phase_jump() {
    let (step, t) = (0.01, 0.005);
    let mut inferred = Saw::new();
    inferred.tick(0.3);
    assert_ne!(inferred.tick(t), saw(t, step));
    assert_eq!(saw(t, step), 2.0 * t - 1.0 - poly_blep(t, step));
    assert_ne!(poly_blep(t, step), 0.0);
}

/// The sine crosses 0 going up at phase 0, peaks at a quarter, and wraps.
#[test]
fn the_sine_lines_up_with_its_phase() {
    assert_eq!(sine(0.0), 0.0);
    assert!((sine(0.25) - 1.0).abs() < 1e-6);
    assert!((sine(0.75) + 1.0).abs() < 1e-6);
    assert_eq!(sine(1.25), sine(0.25));
    assert_eq!(sine(-0.75), sine(0.25));
}

/// Every slow wave crosses 0 going up at phase 0, except the square, which
/// starts high.
#[test]
fn slow_waves_line_up() {
    let waves = [SlowWave::Sine, SlowWave::Triangle, SlowWave::RampUp];
    for wave in waves {
        assert!(wave.value(0.0).abs() < 1e-6, "{wave:?}");
        assert!(wave.value(0.01) > 0.0, "{wave:?}");
    }
    assert!(SlowWave::RampDown.value(0.0).abs() < 1e-6);
    assert!(SlowWave::RampDown.value(0.01) < 0.0);
    assert_eq!(SlowWave::Square.value(0.25), 1.0);
    assert_eq!(SlowWave::Square.value(0.75), -1.0);
}

/// `reset` returns each shape to phase 0, so the next sample is as new.
#[test]
fn reset_returns_to_the_start() {
    let mut saw = Saw::new();
    saw.tick(0.3);
    saw.reset();
    assert_eq!(saw, Saw::new());
    let mut triangle = Triangle::new();
    triangle.tick(0.3);
    triangle.reset();
    assert_eq!(triangle, Triangle::new());
    let mut pulse = Pulse::new();
    pulse.tick(0.3, 0.5);
    pulse.reset(0.5);
    assert_eq!(pulse, Pulse::new());
}

/// A phase running backward, with the size of its step, is band limited too. So
/// a negative step from a `Phasor` needs only its absolute value.
#[test]
fn a_backward_phase_is_band_limited_with_the_step_size() {
    let step = 0.01;
    let t = 0.005;
    assert_eq!(saw(t, step), 2.0 * t - 1.0 - poly_blep(t, step));
    assert_eq!(saw(t, -step), 2.0 * t - 1.0);
    assert_ne!(triangle(0.25, step), triangle(0.25, -step));
    assert_ne!(pulse(t, 0.5, step, step), pulse(t, 0.5, -step, -step));
}
