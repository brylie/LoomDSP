use super::*;
use crate::testing::RATE;

/// A curve with these settings.
fn curve(mode: Mode) -> Curve {
    Curve {
        mode,
        threshold_db: -20.0,
        ratio: 4.0,
        knee_db: 0.0,
        range_db: 40.0,
    }
}

/// Coefficients with fast times.
fn fast(curve: Curve, detection: Detection) -> Coefficients {
    Coefficients::new(Settings {
        curve,
        detection,
        attack_seconds: 0.001,
        release_seconds: 0.01,
        sample_rate: RATE,
    })
}

/// A factor from dB.
fn factor(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// The settled gain in dB for a steady level, with a sine as the signal.
fn settled_db(coefficients: Coefficients, level_db: f32) -> f32 {
    let mut dynamics = Dynamics::new(RATE);
    let amplitude = factor(level_db);
    for i in 0..48_000 {
        dynamics.tick(amplitude * (i as f32 * 0.1).sin(), coefficients);
    }
    // The gain ripples with a sine, so read it at the top of the swing.
    let mut lowest = f32::MAX;
    for i in 48_000..49_000 {
        dynamics.tick(amplitude * (i as f32 * 0.1).sin(), coefficients);
        lowest = lowest.min(dynamics.gain_db());
    }
    lowest
}

/// Above the threshold, the output rises by 1 dB for every `ratio` dB in.
#[test]
fn compression_matches_the_static_curve() {
    for threshold_db in [-40.0, -20.0, -6.0] {
        for ratio in [1.5, 4.0, 20.0] {
            let curve = Curve {
                threshold_db,
                ratio,
                ..curve(Mode::Compress)
            };
            for over in [3.0, 6.0] {
                let level = threshold_db + over;
                let expected = (1.0 / ratio - 1.0) * over;
                assert!((curve.gain_db(level) - expected).abs() < 1e-3);
                // A slow release holds the sine's peak, so the gain follows it.
                let coefficients = Coefficients::new(Settings {
                    curve,
                    detection: Detection::Peak,
                    attack_seconds: 0.000_1,
                    release_seconds: 2.0,
                    sample_rate: RATE,
                });
                let measured = settled_db(coefficients, level);
                assert!(
                    (measured - expected).abs() < 0.1,
                    "{curve:?} at {level}: {measured} against {expected}"
                );
            }
        }
    }
}

/// A soft knee is continuous and has no corner where it meets the straight parts.
#[test]
fn the_knee_joins_with_no_corner() {
    for mode in [Mode::Compress, Mode::Expand] {
        let curve = Curve {
            knee_db: 12.0,
            range_db: 96.0,
            ..curve(mode)
        };
        let mut last_slope = None;
        for i in 0..4_000 {
            let level = -40.0 + i as f32 * 0.01;
            let slope = (curve.gain_db(level + 0.01) - curve.gain_db(level)) / 0.01;
            if let Some(last) = last_slope {
                let change: f32 = slope - last;
                assert!(change.abs() < 0.02, "{mode:?} at {level}: {last} to {slope}");
            }
            last_slope = Some(slope);
        }
    }
}

/// With no knee at all, the curve still gives a finite answer at the threshold.
#[test]
fn no_knee_is_finite_at_the_threshold() {
    assert_eq!(curve(Mode::Compress).gain_db(-20.0), 0.0);
    assert_eq!(curve(Mode::Expand).gain_db(-20.0), 0.0);
}

/// An expander turns down what is below the threshold, and stops at the range.
#[test]
fn expansion_matches_the_static_curve() {
    let curve = curve(Mode::Expand);
    assert_eq!(curve.gain_db(-10.0), 0.0);
    assert_eq!(curve.gain_db(-22.0), -6.0);
    assert_eq!(curve.gain_db(-90.0), -40.0);
}

/// No limited sample passes the ceiling, on any signal, within rounding.
#[test]
fn a_limiter_holds_its_ceiling() {
    let coefficients = fast(curve(Mode::Limit), Detection::Rms);
    let ceiling = factor(-20.0);
    let mut dynamics = Dynamics::new(RATE);
    for i in 0..48_000 {
        let t = i as f32;
        let x = 8.0 * (t * 0.03).sin() * (t * 0.000_7).sin();
        let y = x * dynamics.tick(x, coefficients);
        assert!(y.abs() <= ceiling * (1.0 + 1e-6), "frame {i}: {y}");
    }
}

/// A signal under the limiter's ceiling passes with unity gain.
#[test]
fn a_limiter_leaves_quiet_signals_alone() {
    let coefficients = fast(curve(Mode::Limit), Detection::Peak);
    let mut dynamics = Dynamics::new(RATE);
    for i in 0..1_000 {
        assert_eq!(dynamics.tick(0.05 * (i as f32 * 0.1).sin(), coefficients), 1.0);
    }
}

/// The time constants act: after a step, the gain has gone about 63% of the
/// way in one attack or release time.
#[test]
fn attack_and_release_follow_their_times() {
    let (attack, release) = (0.01, 0.05);
    let coefficients = Coefficients::new(Settings {
        curve: curve(Mode::Compress),
        detection: Detection::Peak,
        attack_seconds: attack,
        release_seconds: release,
        sample_rate: RATE,
    });
    let target = coefficients.curve.gain_db(0.0);
    let mut dynamics = Dynamics::new(RATE);
    for _ in 0..(attack * RATE) as usize {
        dynamics.tick(1.0, coefficients);
    }
    let expected = target * (1.0 - (-1.0f32).exp());
    assert!(
        (dynamics.gain_db() - expected).abs() < 0.1,
        "{} against {expected}",
        dynamics.gain_db()
    );
    for _ in 0..24_000 {
        dynamics.tick(1.0, coefficients);
    }
    for _ in 0..(release * RATE) as usize {
        dynamics.tick(0.0, coefficients);
    }
    let expected = target * (-1.0f32).exp();
    assert!(
        (dynamics.gain_db() - expected).abs() < 0.1,
        "{} against {expected}",
        dynamics.gain_db()
    );
}

/// On a rising signal a gate opens at its attack time, and a compressor turns
/// down at its attack time: the word means the signal, not the gain.
#[test]
fn attack_means_the_signal_rose_in_every_mode() {
    let (attack, release) = (0.000_5, 1.0);
    for mode in [Mode::Compress, Mode::Expand, Mode::Gate, Mode::Duck] {
        let coefficients = Coefficients::new(Settings {
            curve: curve(mode),
            detection: Detection::Peak,
            attack_seconds: attack,
            release_seconds: release,
            sample_rate: RATE,
        });
        let mut dynamics = Dynamics::new(RATE);
        // Settle low on a quiet signal, with the slow release.
        for _ in 0..480_000 {
            dynamics.tick(factor(-60.0), coefficients);
        }
        let before = dynamics.gain_db();
        for _ in 0..240 {
            dynamics.tick(1.0, coefficients);
        }
        let settled = if matches!(mode, Mode::Compress) {
            coefficients.curve.gain_db(0.0)
        } else if mode == Mode::Duck {
            -40.0
        } else {
            0.0
        };
        assert!(
            (dynamics.gain_db() - settled).abs() < 1.0,
            "{mode:?}: {before} to {}",
            dynamics.gain_db()
        );
    }
}

/// The RMS of a sine is 3 dB under its peak.
#[test]
fn rms_reads_a_sine_3_db_under_its_peak() {
    let mut dynamics = Dynamics::new(RATE);
    let mut level = 0.0;
    for i in 0..48_000 {
        level = dynamics.detect((i as f32 * 0.1).sin(), Detection::Rms);
    }
    assert!((level - -3.01).abs() < 0.1, "{level}");
}

/// Silence reads as the floor and stays finite.
#[test]
fn silence_is_finite() {
    let mut dynamics = Dynamics::new(RATE);
    for detection in [Detection::Peak, Detection::Rms] {
        assert_eq!(dynamics.detect(0.0, detection), FLOOR_DB);
    }
}

/// A level sitting on the threshold, with a ripple across it, does not flip
/// the state of a gate or a duck. The ripple is slow enough that the hold alone
/// could not save it, so this tests the second threshold.
#[test]
fn a_gate_does_not_chatter() {
    for mode in [Mode::Gate, Mode::Duck] {
        let coefficients = fast(curve(mode), Detection::Peak);
        let mut dynamics = Dynamics::new(RATE);
        dynamics.tick(1.0, coefficients);
        assert!(dynamics.above);
        for i in 0..96_000 {
            // From 2 dB above the threshold to 2 dB below it, over 100 ms.
            let db = -20.0 + 2.0 * (i as f32 * std::f32::consts::TAU / 4_800.0).sin();
            dynamics.tick(factor(db), coefficients);
            assert!(dynamics.above, "{mode:?}, frame {i}");
        }
    }
}

/// It changes back only after the hold has passed.
#[test]
fn a_gate_holds_before_it_closes() {
    let coefficients = fast(curve(Mode::Gate), Detection::Peak);
    let mut dynamics = Dynamics::new(RATE);
    dynamics.tick(1.0, coefficients);
    let hold = (HOLD_SECONDS * RATE) as usize;
    for i in 0..hold {
        dynamics.tick(factor(-30.0), coefficients);
        assert!(dynamics.above, "frame {i}");
    }
    dynamics.tick(factor(-30.0), coefficients);
    assert!(!dynamics.above);
}

/// A closed gate settles at its range, and an open duck at unity.
#[test]
fn gate_and_duck_reach_their_range() {
    let quiet = factor(-60.0);
    let loud = 1.0;
    for (mode, key, expected) in [
        (Mode::Gate, quiet, -40.0),
        (Mode::Gate, loud, 0.0),
        (Mode::Duck, quiet, 0.0),
        (Mode::Duck, loud, -40.0),
    ] {
        let coefficients = fast(curve(mode), Detection::Peak);
        let mut dynamics = Dynamics::new(RATE);
        let mut gain = 0.0;
        for _ in 0..48_000 {
            gain = dynamics.tick(key, coefficients);
        }
        assert_eq!(dynamics.gain_db(), expected, "{mode:?}");
        assert!((gain - factor(expected)).abs() < 1e-4);
    }
}

/// A gain returning to 0 dB arrives there exactly, so it cannot become a denormal.
#[test]
fn the_gain_settles_exactly() {
    let coefficients = fast(curve(Mode::Compress), Detection::Peak);
    let mut dynamics = Dynamics::new(RATE);
    dynamics.tick(1.0, coefficients);
    for _ in 0..48_000 {
        dynamics.tick(0.0, coefficients);
    }
    assert_eq!(dynamics.gain_db(), 0.0);
}

/// A slow release reaches its target. In an `f32` it would stall short of it,
/// because each step is too small to change a number near -40.
#[test]
fn a_slow_release_does_not_stall() {
    let coefficients = Coefficients::new(Settings {
        curve: curve(Mode::Gate),
        detection: Detection::Peak,
        attack_seconds: 0.001,
        release_seconds: 2.0,
        sample_rate: RATE,
    });
    let mut dynamics = Dynamics::new(RATE);
    for _ in 0..RATE as usize * 60 {
        dynamics.tick(0.0, coefficients);
    }
    assert_eq!(dynamics.gain_db(), -40.0);
}

/// `reset` returns to the state of a new processor.
#[test]
fn reset_returns_to_new() {
    let coefficients = fast(curve(Mode::Duck), Detection::Rms);
    let mut dynamics = Dynamics::new(RATE);
    for _ in 0..100 {
        dynamics.tick(1.0, coefficients);
    }
    assert_ne!(dynamics, Dynamics::new(RATE));
    dynamics.reset();
    assert_eq!(dynamics, Dynamics::new(RATE));
}

/// A huge finite sample leaves no bad state, in any mode and with either
/// detector: squaring it must not overflow into the detector or the gain. The
/// detector still works afterwards.
#[test]
fn a_huge_sample_leaves_no_bad_state() {
    for huge in [1e20, f32::MAX] {
        for mode in [Mode::Compress, Mode::Limit, Mode::Expand, Mode::Gate, Mode::Duck] {
            for detection in [Detection::Peak, Detection::Rms] {
                let coefficients = fast(curve(mode), detection);
                let mut dynamics = Dynamics::new(RATE);
                for _ in 0..8 {
                    let gain = dynamics.tick(huge, coefficients);
                    assert!(gain.is_finite(), "{mode:?} {detection:?} {huge}: gain {gain}");
                }
                assert!(dynamics.power.is_finite(), "{mode:?} {detection:?} {huge}");
                // A steady level gives what a new processor gives.
                let mut new = Dynamics::new(RATE);
                let (mut after, mut expected) = (0.0, 0.0);
                for _ in 0..RATE as usize {
                    after = dynamics.tick(0.5, coefficients);
                    expected = new.tick(0.5, coefficients);
                }
                assert!(
                    (after - expected).abs() < 1e-3,
                    "{mode:?} {detection:?} {huge}: {after} against {expected}"
                );
            }
        }
    }
}
