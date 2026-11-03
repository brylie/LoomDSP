use super::*;
use crate::testing::{RATE, tone_gain};

/// The level of `band` at `hz`, in dB.
fn band_db(hz: f64, low: f32, high: f32, band: fn(Bands) -> f32) -> f64 {
    let coefficients = Coefficients::new(low, high, RATE);
    let mut crossover = Crossover::new();
    20.0 * tone_gain(hz, 0.5, |x| band(crossover.tick(x, coefficients))).log10()
}

/// Frequencies across the audible range.
const SWEEP: [f64; 11] = [
    20.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 15_000.0, 20_000.0,
];

/// Three bands sum to a flat level within 0.01 dB, however the splits are set,
/// including when they cross.
#[test]
fn three_bands_sum_flat() {
    for (low, high) in [(200.0, 2_000.0), (80.0, 12_000.0), (1_000.0, 1_100.0), (3_000.0, 300.0)] {
        for hz in SWEEP {
            let sum = band_db(hz, low, high, |b| b.low + b.mid + b.high);
            assert!(sum.abs() < 0.01, "{low} and {high}, at {hz} Hz: {sum} dB");
        }
    }
}

/// Crossed splits use the lower one for both, so the bands cannot swap.
#[test]
fn crossed_splits_stay_in_order() {
    assert_eq!(
        Coefficients::new(3_000.0, 300.0, RATE),
        Coefficients::new(3_000.0, 3_000.0, RATE)
    );
}

/// Two bands sum to a flat level within 0.01 dB.
#[test]
fn two_bands_sum_flat() {
    for hz in SWEEP {
        let sum = band_db(hz, 1_000.0, 2_000.0, |b| b.lower + b.upper);
        assert!(sum.abs() < 0.01, "at {hz} Hz: {sum} dB");
    }
}

/// The sum does not null against the input: its phase turns. This is the
/// promise the specification makes, so a test holds it.
#[test]
fn the_sum_rotates_the_phase() {
    let coefficients = Coefficients::new(1_000.0, 4_000.0, RATE);
    let mut crossover = Crossover::new();
    let difference = tone_gain(1_000.0, 0.5, |x| {
        let bands = crossover.tick(x, coefficients);
        bands.low + bands.mid + bands.high - x
    });
    assert!(difference > 0.5, "{difference}");
}

/// Each band is 6 dB down at its split, and falls 24 dB per octave past it.
#[test]
fn bands_have_their_corners_and_slopes() {
    let lower = |hz| band_db(hz, 500.0, 8_000.0, |b| b.lower);
    let upper = |hz| band_db(hz, 500.0, 8_000.0, |b| b.upper);
    assert!((lower(500.0) - -6.02).abs() < 0.02);
    assert!((upper(500.0) - -6.02).abs() < 0.02);
    // Far enough past the split, and well below half the sample rate.
    assert!(((lower(2_000.0) - lower(4_000.0)) - 24.0).abs() < 1.0);
    assert!(((upper(125.0) - upper(62.5)) - 24.0).abs() < 1.0);
}

/// `reset` clears the memory.
#[test]
fn reset_clears_the_state() {
    let coefficients = Coefficients::new(200.0, 2_000.0, RATE);
    let mut crossover = Crossover::new();
    crossover.tick(1.0, coefficients);
    assert_ne!(crossover, Crossover::new());
    crossover.reset();
    assert_eq!(crossover, Crossover::new());
}
