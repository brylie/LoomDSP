//! `reset()` snaps smoothing to its targets, for every module (live
//! patching spec, rule 7). The `Controller` and the harness call it once
//! after a module's first values are set, so a module made with values
//! other than its built-in ones does not glide to them.

use loom_core::{ModuleDescriptor, ModuleRole, ParamSpec};
use loom_modules::ALL;
use loom_test::{Harness, Signal};
use std::f32::consts::TAU;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// A value of `spec` away from its default: a quarter of the way along the
/// control, or three quarters when that is the default.
fn changed(spec: &ParamSpec) -> f32 {
    let at = |position: f32| {
        let value = spec.denormalize(position);
        if spec.options.integer { value.round() } else { value }
    };
    let value = at(0.25);
    if value == spec.default { at(0.75) } else { value }
}

/// A harness for `descriptor` with every parameter away from its default
/// and a busy signal on every input.
fn harness(descriptor: &'static ModuleDescriptor) -> Harness {
    let harness = descriptor
        .params
        .iter()
        .fold(Harness::new(descriptor).frames(4_800), |h, spec| {
            h.param(spec.name, changed(spec))
        });
    descriptor.inputs.iter().fold(harness, |h, port| {
        h.input(
            port.name,
            Signal::from_fn(|frame| 0.75 * (frame as f32 * TAU * 50.0 / 48_000.0).sin()),
        )
    })
}

/// Every DSP module in [`ALL`], built with values away from its defaults,
/// plays from its first frame as one that held those values for a long
/// time. Panics with the IDs of all that fail.
#[test]
fn every_module_starts_at_its_values() {
    let failed: Vec<&str> = ALL
        .iter()
        .filter(|descriptor| matches!(descriptor.role, ModuleRole::Dsp(_)))
        .filter(|descriptor| {
            let harness = harness(descriptor);
            catch_unwind(AssertUnwindSafe(|| harness.check_reset_snaps_smoothing())).is_err()
        })
        .map(|descriptor| descriptor.id)
        .collect();
    assert!(
        failed.is_empty(),
        "these modules glide or keep state after reset(): {failed:?}"
    );
}
