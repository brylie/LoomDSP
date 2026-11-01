use super::*;

fn run(mode: u8, x: f32) -> f32 {
    Convert { mode }.convert(x)
}

/// The conversions hit the known points from the issue.
#[test]
fn known_points() {
    assert_eq!(run(0, 0.0), 1.0);
    assert!((run(0, -6.02) - 0.5).abs() < 0.001);
    assert!((run(2, 0.0) - 261.63).abs() < 0.01);
    assert!(run(3, 261.625_58).abs() < 1e-6);
}

/// dB to level and back returns the input.
#[test]
fn db_round_trip() {
    for db in [-60.0, -6.0, 0.0, 12.0] {
        assert!((run(1, run(0, db)) - db).abs() < 1e-3, "{db}");
    }
}

/// A NaN input stays NaN in every mode.
#[test]
fn nan_passes_through() {
    for mode in 0..4 {
        assert!(run(mode, f32::NAN).is_nan(), "mode {mode}");
    }
}

/// Silence and extreme inputs stay finite.
#[test]
fn stays_finite() {
    assert_eq!(run(1, 0.0), -120.0);
    for mode in 0..4 {
        for x in [0.0, -1.0, 1e30, -1e30, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(run(mode, x).is_finite(), "mode {mode} input {x}");
        }
    }
}
