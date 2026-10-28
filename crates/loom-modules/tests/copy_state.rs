//! Every built-in module with state hands it over with
//! `Module::copy_state_from` (ADR-0022, stereo spec rules 25 to 27). When a
//! module turns stereo, its new right-side copy takes the running left
//! copy's state, so both sides continue together.

use loom_core::{MidiEvent, ModuleDescriptor, ModuleRole, ParamSpec};
use loom_modules::ALL;
use loom_test::{Harness, Signal};
use std::f32::consts::TAU;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Frames rendered in each check.
const FRAMES: usize = 9_600;

/// The frame where the copy takes the state: in the middle of the MIDI
/// notes, the gates, and the delay times, so each kind of state is busy.
const AT: usize = 2_500;

/// A value of `spec` away from its default, so smoothing and settings
/// differ from the built-in ones.
fn changed(spec: &ParamSpec) -> f32 {
    let at = |position: f32| {
        let value = spec.denormalize(position);
        if spec.options.integer { value.round() } else { value }
    };
    let value = at(0.25);
    if value == spec.default { at(0.75) } else { value }
}

/// A harness for `descriptor` with every parameter away from its default,
/// a busy signal on every input, and notes held across the copy.
fn harness(descriptor: &'static ModuleDescriptor) -> Harness {
    let harness = descriptor
        .params
        .iter()
        .fold(Harness::new(descriptor).frames(FRAMES), |h, spec| {
            h.param(spec.name, changed(spec))
        });
    let harness = descriptor.inputs.iter().fold(harness, |h, port| {
        h.input(
            port.name,
            Signal::from_fn(|frame| 0.75 * (frame as f32 * TAU * 37.0 / 48_000.0).sin()),
        )
    });
    harness.midi([
        MidiEvent::note_on(100, 0, 60, 100),
        MidiEvent::note_on(1_000, 0, 67, 64),
        MidiEvent::from_bytes(2_000, &[0xE0, 0x00, 0x60]),
        MidiEvent::note_off(3_000, 0, 67),
        MidiEvent::note_off(6_000, 0, 60),
    ])
}

/// True for a module with a random generator, which re-derives it when it
/// copies (rule 27).
fn seeded(descriptor: &ModuleDescriptor) -> bool {
    descriptor.param_index("seed").is_some()
}

/// Every DSP module in [`ALL`] except seeded ones continues bit-identically
/// in a copy that took its state. Panics with the IDs of all that fail.
#[test]
fn every_module_hands_over_its_state() {
    let failed: Vec<&str> = ALL
        .iter()
        .filter(|descriptor| matches!(descriptor.role, ModuleRole::Dsp(_)) && !seeded(descriptor))
        .filter(|descriptor| {
            let harness = harness(descriptor);
            catch_unwind(AssertUnwindSafe(|| harness.check_copy_state(AT))).is_err()
        })
        .map(|descriptor| descriptor.id)
        .collect();
    assert!(
        failed.is_empty(),
        "these modules do not hand over their state: {failed:?}"
    );
}

/// A seeded module copies, and two copies taken at the same frame play the
/// same numbers, so a swap stays deterministic (rule 27).
#[test]
fn seeded_modules_copy_repeatably() {
    let seeded: Vec<_> = ALL.iter().filter(|descriptor| seeded(descriptor)).collect();
    assert!(!seeded.is_empty(), "Noise has a seed");
    for descriptor in seeded {
        harness(descriptor).check_copy_state_repeatable(AT);
    }
}

/// The copy gets a different stream from the running instance, so the
/// right side of stereo noise is not the left side again (rules 15, 27).
#[test]
fn a_noise_copy_does_not_repeat_the_running_numbers() {
    let harness = Harness::new(&loom_modules::osc::NOISE).frames(FRAMES);
    let result = catch_unwind(AssertUnwindSafe(|| harness.check_copy_state(AT)));
    assert!(result.is_err(), "the copy played the running instance's numbers");
}

/// A copy takes state only from its own type.
#[test]
fn a_copy_refuses_another_type() {
    let config = loom_core::ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 512,
    };
    let make = |descriptor: &ModuleDescriptor| match descriptor.role {
        ModuleRole::Dsp(factory) => factory(&config),
        _ => unreachable!("DSP modules only"),
    };
    let mut svf = make(&loom_modules::filter::SVF);
    let delay = make(&loom_modules::time::DELAY);
    assert!(!svf.copy_state_from(&*delay));
    let mut other = make(&loom_modules::time::DELAY);
    assert!(other.copy_state_from(&*delay));
}
