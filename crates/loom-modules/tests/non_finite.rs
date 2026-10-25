//! Every built-in DSP module recovers from a NaN or infinite input (#127).
//!
//! Cables carry any `f32` (ADR-0007). A module that keeps a bad value in its
//! state would stay broken after its input is sane again, until the patch is
//! reloaded. See `Harness::check_non_finite_recovery`.

use loom_core::{ModuleDescriptor, ModuleRole};
use loom_modules::{ALL, time};
use loom_test::{Harness, Signal};
use std::f32::consts::TAU;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Long enough for the default delay time (0.2 s) to pass after the bad
/// frames, so a bad value kept in a delay line would come out again.
const SECONDS: f64 = 0.25;

/// A 50 Hz wave from -0.75 to 0.75. On a gate input it crosses the 0.5
/// threshold, on an audio input it is a tone, so most modules have active
/// state when the bad value arrives.
fn busy() -> Signal {
    Signal::from_fn(|frame| 0.75 * (frame as f32 * TAU * 50.0 / 48_000.0).sin())
}

/// Runs the check on a harness for every DSP module in [`ALL`] and panics
/// with the IDs of all that fail, not only the first.
///
/// Unit delays and audio outputs are left out: the engine runs them, not
/// `process`. A unit delay holds one sample, and the engine turns NaN into
/// silence at the final output.
fn check_all(harness: impl Fn(&'static ModuleDescriptor) -> Harness) {
    let failed: Vec<&str> = ALL
        .iter()
        .filter(|descriptor| matches!(descriptor.role, ModuleRole::Dsp(_)))
        .filter(|descriptor| {
            let harness = harness(descriptor);
            catch_unwind(AssertUnwindSafe(|| harness.check_non_finite_recovery())).is_err()
        })
        .map(|descriptor| descriptor.id)
        .collect();
    assert!(failed.is_empty(), "these modules keep a non-finite input: {failed:?}");
}

/// Every DSP module, with each input on its port default.
#[test]
fn every_module_recovers_with_default_inputs() {
    check_all(|descriptor| Harness::new(descriptor).seconds(SECONDS));
}

/// Every DSP module, with each input on a busy signal.
#[test]
fn every_module_recovers_with_busy_inputs() {
    check_all(|descriptor| {
        descriptor
            .inputs
            .iter()
            .fold(Harness::new(descriptor).seconds(SECONDS), |harness, port| {
                harness.input(port.name, busy())
            })
    });
}

/// The delay lines with short times, so a bad value kept in the line would
/// come round many times within the render.
#[test]
fn delay_lines_do_not_keep_bad_values() {
    Harness::new(&time::DELAY)
        .seconds(SECONDS)
        .param("time", 0.01)
        .input("in", busy())
        .check_non_finite_recovery();
    Harness::new(&time::ALLPASS)
        .seconds(SECONDS)
        .param("time", 0.001)
        .param("gain", 0.9)
        .input("in", busy())
        .check_non_finite_recovery();
}
