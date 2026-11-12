//! Every built-in DSP module recovers from a NaN or infinite input (#127).
//!
//! Cables carry any `f32` (ADR-0007). A module that keeps a bad value in its
//! state would stay broken after its input is sane again, until the patch is
//! reloaded. See `Harness::check_non_finite_recovery`.

mod common;

use common::{WRAP_FRAME, choice_failures, dsp_modules, playing_short_bar};
use loom_core::ModuleDescriptor;
use loom_modules::time;
use loom_test::{Harness, Signal};

/// Long enough for the default delay time (0.2 s) to pass after the bad
/// frames, so a bad value kept in a delay line would come out again.
const SECONDS: f64 = 0.25;

/// Frames in a playing render, long enough to pass the bar wrap, so a module has
/// run through it after the bad frames.
const PLAYING_FRAMES: usize = WRAP_FRAME + 1_846;

/// A harness for `descriptor` on the playing transport.
fn playing(descriptor: &'static ModuleDescriptor) -> Harness {
    Harness::new(descriptor)
        .frames(PLAYING_FRAMES)
        .transport(playing_short_bar())
}

/// The check's busy signal on every input. All inputs move together, so each
/// rising edge falls inside the bad frames.
fn on_busy_inputs(harness: Harness, descriptor: &'static ModuleDescriptor) -> Harness {
    descriptor.inputs.iter().fold(harness, |harness, port| {
        harness.input(port.name, Signal::non_finite_busy())
    })
}

/// Runs the check on a harness for every DSP module in `ALL` and panics
/// with one message per failing option, naming the setting and module.
///
/// Unit delays and audio outputs are left out: the engine runs them, not
/// `process`. A unit delay holds one sample, and the engine turns NaN into
/// silence at the final output.
fn check_all(build: impl Fn(&'static ModuleDescriptor) -> Harness) {
    let failed: Vec<String> = dsp_modules()
        .flat_map(|descriptor| {
            choice_failures(
                descriptor,
                |spec| spec.default,
                || build(descriptor),
                Harness::check_non_finite_recovery,
            )
        })
        .collect();
    assert!(
        failed.is_empty(),
        "these modules keep a non-finite input:\n{}",
        failed.join("\n")
    );
}

/// Port defaults keep the bar phase moving while a different input is bad.
/// Do not use the busy signal here: its edges align only with the default start.
fn on_wrap(descriptor: &'static ModuleDescriptor) -> Harness {
    playing(descriptor).non_finite_start(WRAP_FRAME - 4)
}

/// Every choice on port defaults, with bad values spanning the bar wrap.
#[test]
fn every_module_recovers_on_the_bar_wrap() {
    check_all(on_wrap);
}

#[path = "non_finite/wrap.rs"]
mod wrap;

/// Every DSP module, with each input on its port default.
#[test]
fn every_module_recovers_with_default_inputs() {
    check_all(|descriptor| Harness::new(descriptor).seconds(SECONDS));
}

/// Every DSP module, with each input on a busy signal.
#[test]
fn every_module_recovers_with_busy_inputs() {
    check_all(|descriptor| on_busy_inputs(Harness::new(descriptor).seconds(SECONDS), descriptor));
}

/// Every DSP module, with each input on its port default, on a playing
/// transport. Only here do modules that follow the bar through a port default see
/// it move.
#[test]
fn every_module_recovers_with_default_inputs_when_playing() {
    check_all(playing);
}

/// Every DSP module, with each input on a busy signal, on a playing transport.
#[test]
fn every_module_recovers_with_busy_inputs_when_playing() {
    check_all(|descriptor| on_busy_inputs(playing(descriptor), descriptor));
}

/// The delay lines with short times, so a bad value kept in the line would
/// come round many times within the render.
#[test]
fn delay_lines_do_not_keep_bad_values() {
    Harness::new(&time::DELAY)
        .seconds(SECONDS)
        .param("time", 0.01)
        .input("in", Signal::non_finite_busy())
        .check_non_finite_recovery();
    Harness::new(&time::ALLPASS)
        .seconds(SECONDS)
        .param("time", 0.001)
        .param("gain", 0.9)
        .input("in", Signal::non_finite_busy())
        .check_non_finite_recovery();
}
