use super::*;
use crate::testing::{RATE, tone_gain};

/// The measured gain of a section in dB.
fn measured_db(shape: Shape, hz: f64) -> f64 {
    let coefficients = Coefficients::new(shape, RATE);
    let mut section = Section::new();
    20.0 * tone_gain(hz, 0.1, |x| section.tick(x, coefficients)).log10()
}

/// Every shape, at several settings.
fn shapes() -> Vec<Shape> {
    let mut shapes = vec![
        Shape::LowCut {
            hz: 80.0,
            q: BUTTERWORTH_Q,
        },
        Shape::HighCut {
            hz: 8_000.0,
            q: BUTTERWORTH_Q,
        },
    ];
    for gain_db in [-24.0, -6.0, 3.0, 12.0, 24.0] {
        shapes.push(Shape::LowShelf {
            hz: 200.0,
            gain_db,
            q: BUTTERWORTH_Q,
        });
        shapes.push(Shape::HighShelf {
            hz: 4_000.0,
            gain_db,
            q: BUTTERWORTH_Q,
        });
        for q in [0.3, 1.0, 8.0] {
            shapes.push(Shape::Bell {
                hz: 1_000.0,
                gain_db,
                q,
            });
        }
    }
    shapes
}

/// Each section's measured response matches the analogue one, once the
/// frequencies are warped as the bilinear transform warps them.
#[test]
fn response_matches_the_theory() {
    for shape in shapes() {
        for hz in [40.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 15_000.0] {
            let measured = measured_db(shape, hz);
            let expected = f64::from(shape.gain_db(hz as f32, RATE));
            // A deep cut far from its corner is a tiny signal, so the
            // measurement itself is less exact there.
            if expected > -50.0 {
                assert!(
                    (measured - expected).abs() < 0.1,
                    "{shape:?} at {hz} Hz: {measured} against {expected}"
                );
            }
        }
    }
}

/// The gain knob means what it says: a bell has its gain at its centre, and a
/// shelf has half of its gain in dB at its frequency, at any sample rate.
#[test]
fn gains_land_on_their_frequencies() {
    for gain_db in [-18.0f32, 6.0, 18.0] {
        let bell = Shape::Bell {
            hz: 1_000.0,
            gain_db,
            q: 2.0,
        };
        assert!(
            (measured_db(bell, 1_000.0) - f64::from(gain_db)).abs() < 0.02,
            "{bell:?}"
        );
        for shape in [
            Shape::LowShelf {
                hz: 1_000.0,
                gain_db,
                q: BUTTERWORTH_Q,
            },
            Shape::HighShelf {
                hz: 1_000.0,
                gain_db,
                q: BUTTERWORTH_Q,
            },
        ] {
            assert!(
                (measured_db(shape, 1_000.0) - f64::from(gain_db / 2.0)).abs() < 0.02,
                "{shape:?}"
            );
        }
    }
}

/// The panel's response agrees with the textbook formulas, written out here on
/// their own, so a mistake in the mix cannot agree with itself. `w` is the
/// frequency over the corner, and `a` squared is the gain as a factor.
#[test]
fn analogue_response_matches_the_formulas() {
    let magnitude = |top: (f32, f32, f32), bottom: (f32, f32, f32), w: f32| {
        let part = |(s2, s1, s0): (f32, f32, f32)| f32::hypot(s0 - s2 * w * w, s1 * w);
        20.0 * (part(top) / part(bottom)).log10()
    };
    for gain_db in [-24.0f32, -6.0, 12.0] {
        let a = 10.0f32.powf(gain_db / 40.0);
        for q in [0.5f32, 1.0, 4.0] {
            for w in [0.1f32, 0.5, 1.0, 1.7, 8.0] {
                let hz = 1_000.0 * w;
                let bell = Shape::Bell {
                    hz: 1_000.0,
                    gain_db,
                    q,
                }
                .analogue_gain_db(hz);
                let expected = magnitude((1.0, a / q, 1.0), (1.0, 1.0 / (a * q), 1.0), w);
                assert!(
                    (bell - expected).abs() < 1e-3,
                    "bell {gain_db} {q} {w}: {bell} against {expected}"
                );
                let root = a.sqrt();
                let low = Shape::LowShelf {
                    hz: 1_000.0,
                    gain_db,
                    q,
                }
                .analogue_gain_db(hz);
                let expected = magnitude((a, a * root / q, a * a), (a, root / q, 1.0), w);
                assert!(
                    (low - expected).abs() < 1e-3,
                    "low {gain_db} {q} {w}: {low} against {expected}"
                );
                let high = Shape::HighShelf {
                    hz: 1_000.0,
                    gain_db,
                    q,
                }
                .analogue_gain_db(hz);
                let expected = magnitude((a * a, a * root / q, a), (1.0, root / q, a), w);
                assert!(
                    (high - expected).abs() < 1e-3,
                    "high {gain_db} {q} {w}: {high} against {expected}"
                );
            }
        }
    }
}

/// Shelves reach their whole gain far from their frequency, and nothing on
/// the other side.
#[test]
fn shelves_reach_their_gain() {
    let low = Shape::LowShelf {
        hz: 1_000.0,
        gain_db: 12.0,
        q: BUTTERWORTH_Q,
    };
    assert!((low.analogue_gain_db(5.0) - 12.0).abs() < 0.01);
    assert!(low.analogue_gain_db(100_000.0).abs() < 0.01);
    let high = Shape::HighShelf {
        hz: 1_000.0,
        gain_db: 12.0,
        q: BUTTERWORTH_Q,
    };
    assert!(high.analogue_gain_db(5.0).abs() < 0.01);
    assert!((high.analogue_gain_db(100_000.0) - 12.0).abs() < 0.01);
}

/// At 0 dB a bell or shelf leaves the input exactly, bit for bit.
#[test]
fn zero_gain_leaves_the_input_untouched() {
    for shape in [
        Shape::Bell {
            hz: 1_000.0,
            gain_db: 0.0,
            q: 1.0,
        },
        Shape::LowShelf {
            hz: 200.0,
            gain_db: 0.0,
            q: BUTTERWORTH_Q,
        },
        Shape::HighShelf {
            hz: 8_000.0,
            gain_db: 0.0,
            q: BUTTERWORTH_Q,
        },
    ] {
        let coefficients = Coefficients::new(shape, RATE);
        let mut section = Section::new();
        for i in 1..10_000 {
            let x = (i as f32 * 0.37).sin();
            assert_eq!(
                section.tick(x, coefficients).to_bits(),
                x.to_bits(),
                "{shape:?}, frame {i}"
            );
        }
    }
}

/// A cut is 3 dB down at its corner on both slopes, and falls at its slope
/// well past it.
#[test]
fn cuts_have_their_corner_and_slope() {
    for (side, corner, far, farther) in [
        (CutSide::Low, 1_000.0, 100.0, 50.0),
        (CutSide::High, 1_000.0, 10_000.0, 20_000.0),
    ] {
        for (slope, per_octave) in [(CutSlope::Db12, 12.0), (CutSlope::Db24, 24.0)] {
            let gain = |hz| side.analogue_gain_db(corner, slope, hz);
            assert!((gain(corner) - -3.01).abs() < 0.01, "{side:?} {slope:?}");
            let fall = gain(far) - gain(farther);
            assert!((fall - per_octave).abs() < 0.3, "{side:?} {slope:?}: {fall}");
        }
    }
}

/// A running cut gives both slopes, and each matches its analogue response.
#[test]
fn cut_outputs_match_the_theory() {
    let coefficients = CutCoefficients::new(CutSide::Low, 1_000.0, RATE);
    for (hz, slope) in [
        (500.0, CutSlope::Db12),
        (500.0, CutSlope::Db24),
        (2_000.0, CutSlope::Db24),
    ] {
        let mut cut = Cut::new();
        let measured = 20.0
            * tone_gain(hz, 0.1, |x| {
                let outputs = cut.tick(x, coefficients);
                if slope == CutSlope::Db12 {
                    outputs.db12
                } else {
                    outputs.db24
                }
            })
            .log10();
        let expected = f64::from(CutSide::Low.analogue_gain_db(1_000.0, slope, hz as f32));
        assert!(
            (measured - expected).abs() < 0.2,
            "{slope:?} at {hz}: {measured} against {expected}"
        );
    }
}

/// Settings moving at audio rate stay bounded.
#[test]
fn audio_rate_modulation_stays_bounded() {
    let mut section = Section::new();
    for i in 0..100_000 {
        let t = i as f32 / RATE;
        let shape = Shape::Bell {
            hz: 2_000.0 + 1_900.0 * (t * 3_000.0).sin(),
            gain_db: 24.0 * (t * 4_100.0).sin(),
            q: 5.0,
        };
        let y = section.tick((t * 2_500.0).sin(), Coefficients::new(shape, RATE));
        assert!(y.abs() < 100.0, "frame {i}: {y}");
    }
}

/// `reset` clears the memory.
#[test]
fn reset_clears_the_state() {
    let coefficients = Coefficients::new(
        Shape::Bell {
            hz: 1_000.0,
            gain_db: 6.0,
            q: 1.0,
        },
        RATE,
    );
    let mut section = Section::new();
    section.tick(1.0, coefficients);
    assert_ne!(section, Section::new());
    section.reset();
    assert_eq!(section, Section::new());
    let cut_coefficients = CutCoefficients::new(CutSide::High, 1_000.0, RATE);
    let mut cut = Cut::new();
    cut.tick(1.0, cut_coefficients);
    cut.reset();
    assert_eq!(cut, Cut::new());
}
