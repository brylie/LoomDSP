use super::*;
use loom_core::testing::run;

/// Runs one block with a fixed input and level and returns the last sample.
fn amp(response: f32, level: Option<f32>) -> f32 {
    let mut module = Amp {
        gain: Smoothed::new(1.0, 48_000.0, 0.005),
        exponential: false,
    };
    module.set_param(1, response);
    let input = [0.8_f32; 8];
    let levels = [level.unwrap_or(0.0); 8];
    let mut out = [0.0_f32; 8];
    run(
        &mut module,
        48_000.0,
        &[&input, &levels],
        &[true, level.is_some()],
        &mut [&mut out],
        &[],
    );
    out[7]
}

/// Level 0.5 halves the input in linear mode.
#[test]
fn linear_halves() {
    assert!((amp(0.0, Some(0.5)) - 0.4).abs() < 1e-6);
}

/// Level 0.5 is -30 dB in exponential mode.
#[test]
fn exponential_minus_30_db() {
    let db = 20.0 * (amp(1.0, Some(0.5)) / 0.8).log10();
    assert!((db + 30.0).abs() < 1e-3, "{db}");
    assert!((amp(1.0, Some(1.0)) - 0.8).abs() < 1e-6);
}

/// Level 0 and negative levels give silence in both modes.
#[test]
fn zero_and_negative_levels_are_silent() {
    for response in [0.0, 1.0] {
        assert_eq!(amp(response, Some(0.0)), 0.0);
        assert_eq!(amp(response, Some(-0.5)), 0.0);
    }
}

/// With no cable on `level`, only the gain acts.
#[test]
fn unconnected_level_reads_one() {
    assert!((amp(0.0, None) - 0.8).abs() < 1e-6);
    assert!((amp(1.0, None) - 0.8).abs() < 1e-6);
}
