//! Behaviour tests for `loom.level.amp` (MT-LEVEL-02, issue #298).

use loom_modules::level;
use loom_test::{Harness, Signal};

const TOLERANCE: f32 = 1.0e-4;

fn amp() -> Harness {
    Harness::new(&level::AMP)
}

#[test]
fn unconnected_level_passes_the_signal_at_gain_one() {
    // a constant signal, long enough for the 5 ms gain glide to settle
    let out = amp().seconds(0.1).input("in", Signal::constant(0.5)).render();
    let tail = &out.output("out")[4800..];
    for &v in tail {
        assert!((v - 0.5).abs() < TOLERANCE, "got {v}");
    }
}

#[test]
fn linear_response_follows_the_level() {
    let out = amp()
        .seconds(0.1)
        .input("in", Signal::constant(1.0))
        .input("level", Signal::constant(0.25))
        .render();
    let tail = &out.output("out")[4800..];
    for &v in tail {
        assert!((v - 0.25).abs() < TOLERANCE, "got {v}");
    }
}

#[test]
fn negative_level_counts_as_zero() {
    let out = amp()
        .seconds(0.1)
        .input("in", Signal::constant(1.0))
        .input("level", Signal::constant(-0.5))
        .render();
    let tail = &out.output("out")[4800..];
    for &v in tail {
        assert!(v.abs() < TOLERANCE, "got {v}");
    }
}

#[test]
fn exponential_response_maps_one_to_unity_and_a_quarter_to_minus_twelve_db() {
    // level 1 -> 0 dB (unity); level 0.25 -> 10^(-3*0.75) = -22.5 dB
    let unity = amp()
        .seconds(0.1)
        .input("in", Signal::constant(1.0))
        .input("level", Signal::constant(1.0))
        .param("response", 1.0)
        .render();
    for &v in &unity.output("out")[4800..] {
        assert!((v - 1.0).abs() < TOLERANCE, "unity got {v}");
    }
    let quarter = amp()
        .seconds(0.1)
        .input("in", Signal::constant(1.0))
        .input("level", Signal::constant(0.25))
        .param("response", 1.0)
        .render();
    let want = 10.0_f32.powf(-3.0 * 0.75);
    for &v in &quarter.output("out")[4800..] {
        assert!((v - want).abs() < TOLERANCE, "quarter got {v}, want {want}");
    }
}

#[test]
fn exponential_level_zero_is_silence() {
    let out = amp()
        .seconds(0.1)
        .input("in", Signal::constant(1.0))
        .input("level", Signal::constant(0.0))
        .param("response", 1.0)
        .render();
    for &v in &out.output("out")[4800..] {
        assert!(v.abs() < TOLERANCE, "got {v}");
    }
}

#[test]
fn gain_multiplies_after_the_response_curve() {
    let out = amp()
        .seconds(0.1)
        .input("in", Signal::constant(0.5))
        .input("level", Signal::constant(0.5))
        .param("gain", 2.0)
        .render();
    for &v in &out.output("out")[4800..] {
        assert!((v - 0.5).abs() < TOLERANCE, "got {v}");
    }
}
