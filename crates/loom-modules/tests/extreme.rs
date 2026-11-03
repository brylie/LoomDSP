//! Every built-in DSP module survives huge finite inputs (#552).
//!
//! Cables carry any `f32` and are never clipped (ADR-0007), so a feedback patch
//! can send very large values into a module. See
//! `Harness::check_extreme_recovery`.

use loom_core::{ModuleDescriptor, ModuleRole, TransportSettings};
use loom_modules::ALL;
use loom_test::{EXTREME_RENDER, Harness, Signal};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Modules that rightly differ from a clean render after a huge input, and why.
/// They must still stay finite and return to a normal size.
const EXCEPTIONS: &[(&str, &str)] = &[
    (
        "loom.dyn.dynamics",
        "A huge input is a real 300 dB peak. Its release takes longer than the settling time.",
    ),
    (
        "loom.env.envelope",
        "A huge gate is a real rising edge, so the envelope really plays.",
    ),
    (
        "loom.logic.memory",
        "A huge input is a real gate, and the module's job is to remember it.",
    ),
    (
        "loom.rand.random",
        "A huge trigger is a real rising edge, so it takes a value from the stream early.",
    ),
    (
        "loom.rand.sample-hold",
        "A trigger during the huge value captures it, and holds it until the next trigger.",
    ),
    (
        "loom.time.allpass",
        "A 300 dB peak recirculates in the line, and takes longer than the settling time to ring down.",
    ),
];

/// True for an input that swings a delay time up to nearly a second, which
/// rightly keeps the huge value in the line past the end of the check.
fn sets_delay_time(descriptor: &ModuleDescriptor, port: &str) -> bool {
    port == "time" && matches!(descriptor.id, "loom.time.delay" | "loom.time.allpass")
}

/// A harness for `descriptor`, on a transport that plays if `running`, with the
/// check's slow signal on every input if `busy`.
fn harness(descriptor: &'static ModuleDescriptor, running: bool, busy: bool) -> Harness {
    let harness = Harness::new(descriptor)
        .frames(EXTREME_RENDER)
        .transport(TransportSettings {
            running,
            ..TransportSettings::default()
        });
    descriptor
        .inputs
        .iter()
        .filter(|port| busy && !sets_delay_time(descriptor, port.name))
        .fold(harness, |h, port| h.input(port.name, Signal::extreme_busy()))
}

/// The panic message of `check`, if it fails.
fn failure(check: impl FnOnce()) -> Option<String> {
    let failed = catch_unwind(AssertUnwindSafe(check)).err()?;
    Some(failed.downcast_ref::<String>().cloned().unwrap_or_default())
}

/// Every DSP module, on `running` and `busy`: one with an exception stays
/// bounded, and any other recovers. Panics with all that fail, not only the
/// first.
fn check_all(running: bool, busy: bool) {
    let failed: Vec<String> = ALL
        .iter()
        .filter(|descriptor| matches!(descriptor.role, ModuleRole::Dsp(_)))
        .filter_map(|descriptor| {
            let harness = harness(descriptor, running, busy);
            if EXCEPTIONS.iter().any(|(id, _)| *id == descriptor.id) {
                failure(|| harness.check_extreme_stays_bounded())
            } else {
                failure(|| harness.check_extreme_recovery())
            }
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Each input on its port default, on a stopped transport.
#[test]
fn every_module_survives_with_default_inputs_when_stopped() {
    check_all(false, false);
}

/// Each input on its port default, on a playing transport.
#[test]
fn every_module_survives_with_default_inputs_when_playing() {
    check_all(true, false);
}

/// Each input on a busy signal, on a stopped transport.
#[test]
fn every_module_survives_with_busy_inputs_when_stopped() {
    check_all(false, true);
}

/// Each input on a busy signal, on a playing transport.
#[test]
fn every_module_survives_with_busy_inputs_when_playing() {
    check_all(true, true);
}

/// Each exception is still needed, so the table cannot go stale and hide a
/// later bug.
#[test]
fn every_exception_is_needed() {
    for (id, _) in EXCEPTIONS {
        let descriptor = ALL
            .iter()
            .find(|descriptor| descriptor.id == *id)
            .unwrap_or_else(|| panic!("{id} is not a module"));
        let needed = [(false, false), (false, true), (true, false), (true, true)]
            .into_iter()
            .any(|(running, busy)| failure(|| harness(descriptor, running, busy).check_extreme_recovery()).is_some());
        assert!(needed, "{id} passes the full check, so it needs no exception");
    }
}
