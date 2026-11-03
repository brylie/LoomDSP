use super::*;
use crate::testing::{RATE, tone_gain};

/// The magnitude of a biquad at `hz` at 48 kHz, in dB.
fn biquad_db(b: [f64; 3], a: [f64; 3], hz: f64) -> f64 {
    let w = std::f64::consts::TAU * hz / 48_000.0;
    let part = |c: [f64; 3]| {
        f64::hypot(
            c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos(),
            c[1] * w.sin() + c[2] * (2.0 * w).sin(),
        )
    };
    20.0 * (part(b) / part(a)).log10()
}

/// At 48 kHz the filter matches the coefficients the standard publishes
/// (ITU-R BS.1770, Annex 1, Tables 1 and 2). The standard's high-pass has a
/// gain of about 0.04 dB at the top, where this one has none. Both signals pass
/// through the same filter, so a constant cannot change their ratio.
#[test]
fn the_filter_matches_the_standard() {
    let shelf_b = [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85];
    let shelf_a = [1.0, -1.690_659_293_182_41, 0.732_480_774_215_85];
    let rumble_b = [1.0, -2.0, 1.0];
    let rumble_a = [1.0, -1.990_047_454_833_98, 0.990_072_250_366_21];
    for hz in [20.0, 40.0, 100.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 20_000.0] {
        let top: f64 = rumble_a.iter().zip([1.0, -1.0, 1.0]).map(|(a, sign)| a * sign).sum();
        let offset = 20.0 * (4.0 / top).log10();
        let expected = biquad_db(shelf_b, shelf_a, hz) + biquad_db(rumble_b, rumble_a, hz) - offset;
        let mut filter = KWeighting::new(RATE);
        let measured = 20.0 * tone_gain(hz, 0.1, |x| filter.tick(x)).log10();
        assert!(
            (measured - expected).abs() < 0.02,
            "{hz} Hz: {measured} against {expected}"
        );
    }
}

/// A tone at each side.
fn tone(i: usize, amplitude: f32) -> [f32; 2] {
    let x = amplitude * (i as f32 * 0.13).sin();
    [x, x]
}

/// Given a quieter version of a signal, the gain settles at the difference.
#[test]
fn the_gain_makes_up_the_difference() {
    for (processed, expected) in [(0.25, 2.0), (0.5, 1.0), (1.0, 0.5)] {
        let mut level_match = LevelMatch::new(RATE);
        let mut gain = 0.0;
        for i in 0..RATE as usize * 5 {
            gain = level_match.tick(tone(i, 0.5), tone(i, processed));
        }
        assert!((gain - expected).abs() < 0.01, "{processed}: {gain}");
    }
}

/// The gain stays within 24 dB, even if the processed signal is silent.
#[test]
fn the_gain_is_limited() {
    let mut level_match = LevelMatch::new(RATE);
    let mut gain = 0.0;
    for i in 0..RATE as usize {
        gain = level_match.tick(tone(i, 0.5), [0.0, 0.0]);
    }
    assert_eq!(gain, MOST_GAIN);
}

/// A very quiet processed signal, or a very loud one, meets the limit.
#[test]
fn a_large_difference_stops_at_the_limit() {
    for (processed, expected) in [(1e-3, MOST_GAIN), (8.0, 1.0 / MOST_GAIN)] {
        let mut level_match = LevelMatch::new(RATE);
        let mut gain = 0.0;
        for i in 0..RATE as usize * 5 {
            gain = level_match.tick(tone(i, 0.5), tone(i, processed));
        }
        assert_eq!(gain, expected, "{processed}");
    }
}

/// In silence the gain holds where it was, so the next note does not jump.
#[test]
fn silence_holds_the_gain() {
    let mut level_match = LevelMatch::new(RATE);
    for i in 0..RATE as usize * 5 {
        level_match.tick(tone(i, 0.5), tone(i, 0.25));
    }
    // Let the window empty past the silence level.
    for _ in 0..RATE as usize * 10 {
        level_match.tick([0.0, 0.0], [0.0, 0.0]);
    }
    let held = level_match.gain;
    assert!((held - 2.0).abs() < 0.1, "{held}");
    // The processed side alone can no longer move it.
    for i in 0..RATE as usize * 5 {
        assert_eq!(level_match.tick([0.0, 0.0], tone(i, 1.0)), held);
    }
}

/// `reset` returns to the state of a new matcher.
#[test]
fn reset_returns_to_new() {
    let mut level_match = LevelMatch::new(RATE);
    level_match.tick([0.5, 0.5], [0.25, 0.25]);
    assert_ne!(level_match, LevelMatch::new(RATE));
    level_match.reset();
    assert_eq!(level_match, LevelMatch::new(RATE));
}
