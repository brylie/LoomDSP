//! Every built-in DSP module survives huge finite inputs (#552).
//!
//! Cables carry any `f32` and are never clipped (ADR-0007), so a feedback patch
//! can send very large values into a module. See
//! `Harness::check_extreme_recovery`.

mod common;

use common::dsp_modules;
use loom_core::{ModuleDescriptor, TransportSettings};
use loom_modules::{ALL, level, math, mix};
use loom_test::{EXTREME_RENDER, Harness, Signal};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Inputs where a huge value rightly leaves a module different from a clean
/// render, and why: `(module, input, reason)`. The module must still stay finite
/// and return to a normal size. Every other input of the module gets the full
/// check, so a reason only excuses the input it is about.
const EXCEPTIONS: &[(&str, &str, &str)] = &[
    (
        "loom.dyn.dynamics",
        "in",
        "A huge input is a real 300 dB peak. Its release takes longer than the settling time.",
    ),
    (
        "loom.dyn.dynamics",
        "sidechain",
        "A huge key is a real 300 dB peak. Its release takes longer than the settling time.",
    ),
    (
        "loom.env.envelope",
        "gate",
        "A huge gate is a real rising edge, so the envelope really plays.",
    ),
    (
        "loom.logic.memory",
        "a",
        "A huge input is a real gate, and the module's job is to remember it.",
    ),
    (
        "loom.logic.memory",
        "b",
        "A huge input is a real gate, and the module's job is to remember it.",
    ),
    (
        "loom.rand.random",
        "trigger",
        "A huge trigger is a real rising edge, so it takes a value from the stream early.",
    ),
    (
        "loom.rand.sample-hold",
        "in",
        "A trigger during the huge value captures it, and holds it until the next trigger.",
    ),
    (
        "loom.rand.sample-hold",
        "trigger",
        "A huge trigger is a real rising edge, so it takes a value at another moment.",
    ),
    (
        "loom.time.allpass",
        "in",
        "A 300 dB peak recirculates in the line, and takes longer than the settling time to ring down.",
    ),
];

/// True for an input that swings a delay time up to nearly a second, which
/// rightly keeps the huge value in the line past the end of the check.
fn sets_delay_time(descriptor: &ModuleDescriptor, port: &str) -> bool {
    port == "time" && matches!(descriptor.id, "loom.time.delay" | "loom.time.allpass")
}

/// An entry of [`ALL_INPUT_EXCEPTIONS`]: `(module, option, reason)`.
type AllInputException = (&'static str, Option<(&'static str, f32)>, &'static str);

/// Where every input being huge at once rightly leaves a module different from a
/// clean render, and why: `(module, option, reason)`. The module must still stay
/// finite and return to a normal size. An entry with an option, as `(parameter,
/// value)`, excuses only that option, so every other option gets the full check. A
/// bug can live in one mode, so an entry with `None`, which excuses them all, is
/// for a module that needs it at its defaults.
const ALL_INPUT_EXCEPTIONS: &[AllInputException] = &[
    (
        "loom.dyn.dynamics",
        None,
        "A huge input is a real 300 dB peak. Its release takes longer than the settling time.",
    ),
    (
        "loom.env.envelope",
        None,
        "A huge gate is a real rising edge, so the envelope really plays.",
    ),
    (
        "loom.eq.eq",
        Some(("low_slope", 2.0)),
        "A 300 dB peak rings down through the 24 dB low cut. At the 40 Hz default its slowest pole falls 96 per second, too slow for the settling time.",
    ),
    (
        "loom.gate.euclid",
        Some(("source", 1.0)),
        "A huge clock is a real rising edge, so the pattern really moves.",
    ),
    (
        "loom.logic.memory",
        Some(("mode", 2.0)),
        "In Data mode, a huge clock remembers a huge input, and the module's job is to keep it.",
    ),
    (
        "loom.rand.random",
        None,
        "A huge trigger is a real rising edge, so it takes a value from the stream early.",
    ),
    (
        "loom.rand.sample-hold",
        None,
        "A huge trigger captures the huge input, and holds it until the next trigger.",
    ),
    (
        "loom.time.allpass",
        None,
        "A 300 dB peak recirculates in the line, and takes longer than the settling time to ring down.",
    ),
];

/// True if [`ALL_INPUT_EXCEPTIONS`] excuses `id` in `option`, or at its defaults
/// if `option` is `None`.
fn all_input_excused(id: &str, option: Option<(&str, f32)>) -> bool {
    ALL_INPUT_EXCEPTIONS
        .iter()
        .any(|(entry, excused, _)| *entry == id && (excused.is_none() || *excused == option))
}

/// Modules from [`ALL_INPUT_EXCEPTIONS`] that keep the huge value with no later
/// input to replace it. On port defaults, a trigger rises once, as the hold
/// starts, and never again, so Sample and Hold rightly holds what it took. On busy
/// inputs its next trigger replaces it, so it stays bounded there.
const HOLDS_UNTIL_NEXT_TRIGGER: &[&str] = &["loom.rand.sample-hold"];

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

/// The values to try for parameters with named choices, one parameter at a time:
/// `(parameter, value)` for each option that is not its default.
fn choice_options(descriptor: &ModuleDescriptor) -> Vec<(&'static str, f32)> {
    descriptor
        .params
        .iter()
        .flat_map(|spec| {
            let values = (0..spec.options.choices.len()).map(|option| spec.min + option as f32);
            values
                .filter(|value| *value != spec.default)
                .map(|value| (spec.name, value))
        })
        .collect()
}

/// `harness` with its continuous parameters at their maximum. Whole-number and
/// choice parameters keep their defaults: a mute at its maximum would silence the
/// input under test, and modes have their own run.
fn turned_up(descriptor: &'static ModuleDescriptor, harness: Harness) -> Harness {
    descriptor
        .params
        .iter()
        .filter(|spec| !spec.options.integer)
        .fold(harness, |h, spec| h.param(spec.name, spec.max))
}

/// The panic message of `check` on module `id`, if it fails. A plain assert
/// panics with a `&str`, and an `unwrap` with no message says nothing about the
/// module, so the report always names it.
fn failure(id: &str, check: impl FnOnce()) -> Option<String> {
    let failed = catch_unwind(AssertUnwindSafe(check)).err()?;
    let message = failed
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failed.downcast_ref::<&str>().copied())
        .unwrap_or("panicked with no message");
    Some(if message.starts_with(id) {
        message.to_string()
    } else {
        format!("{id}: {message}")
    })
}

/// Every input of every DSP module, on `running` and `busy`: one with an
/// exception stays bounded, and any other recovers. Panics with all that fail,
/// not only the first.
fn check_all(running: bool, busy: bool) {
    let mut failed = Vec::new();
    for descriptor in dsp_modules() {
        let harness = harness(descriptor, running, busy);
        for port in descriptor.inputs {
            let excused = EXCEPTIONS
                .iter()
                .any(|(id, input, _)| *id == descriptor.id && *input == port.name);
            failed.extend(if excused {
                failure(descriptor.id, || harness.check_extreme_stays_bounded_on(port.name))
            } else {
                failure(descriptor.id, || harness.check_extreme_recovery_on(port.name))
            });
        }
    }
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

/// Every input of every DSP module takes the huge value together, on `running`
/// and `busy`, at the defaults and in each option of each choice parameter. An
/// excused run stays bounded, and any other recovers. Panics with all that fail.
fn check_all_at_once(running: bool, busy: bool) {
    let mut failed = Vec::new();
    for descriptor in dsp_modules() {
        let check = |harness: &Harness, option: Option<(&str, f32)>, what: &str| {
            failure(descriptor.id, || {
                if !busy && HOLDS_UNTIL_NEXT_TRIGGER.contains(&descriptor.id) {
                    harness.check_extreme_finite_all();
                } else if all_input_excused(descriptor.id, option) {
                    harness.check_extreme_stays_bounded_all();
                } else {
                    harness.check_extreme_recovery_all();
                }
            })
            .map(|message| format!("{what}: {message}"))
        };
        failed.extend(check(&harness(descriptor, running, busy), None, "defaults"));
        for (name, value) in choice_options(descriptor) {
            let harness = harness(descriptor, running, busy).param(name, value);
            failed.extend(check(&harness, Some((name, value)), &format!("{name} = {value}")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Every input at once on its port default's run, on a stopped transport.
#[test]
fn every_module_survives_every_input_huge_with_default_inputs_when_stopped() {
    check_all_at_once(false, false);
}

/// Every input at once on its port default's run, on a playing transport.
#[test]
fn every_module_survives_every_input_huge_with_default_inputs_when_playing() {
    check_all_at_once(true, false);
}

/// Every input at once on a busy signal, on a stopped transport.
#[test]
fn every_module_survives_every_input_huge_with_busy_inputs_when_stopped() {
    check_all_at_once(false, true);
}

/// Every input at once on a busy signal, on a playing transport.
#[test]
fn every_module_survives_every_input_huge_with_busy_inputs_when_playing() {
    check_all_at_once(true, true);
}

/// Each module on `busy` with its continuous parameters at their maximum stays
/// finite, each input in turn and every input at once. A gain above 1 overflows
/// on one huge input. Panics with all that fail.
fn check_turned_up(busy: bool) {
    let mut failed = Vec::new();
    for descriptor in dsp_modules() {
        let harness = turned_up(descriptor, harness(descriptor, true, busy));
        failed.extend(failure(descriptor.id, || harness.check_extreme_finite()));
        failed.extend(failure(descriptor.id, || harness.check_extreme_finite_all()));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Parameters at their maximum, with inputs on their port defaults.
#[test]
fn every_module_stays_finite_turned_up_with_default_inputs() {
    check_turned_up(false);
}

/// Parameters at their maximum, with inputs on a busy signal.
#[test]
fn every_module_stays_finite_turned_up_with_busy_inputs() {
    check_turned_up(true);
}

/// The stopped and playing, default and busy runs of the check.
const RUNS: [(bool, bool); 4] = [(false, false), (false, true), (true, false), (true, true)];

/// Each all-input exception is still needed for what it names: an option in that
/// option, and an entry for the whole module at its defaults. So the table cannot
/// go stale and hide a later bug.
#[test]
fn every_all_input_exception_is_needed() {
    for (id, option, _) in ALL_INPUT_EXCEPTIONS {
        let descriptor = ALL
            .iter()
            .find(|descriptor| descriptor.id == *id)
            .unwrap_or_else(|| panic!("{id} is not a module"));
        let needed = RUNS.into_iter().any(|(running, busy)| {
            let harness = harness(descriptor, running, busy);
            let harness = match option {
                Some((name, value)) => harness.param(name, *value),
                None => harness,
            };
            failure(id, || harness.check_extreme_recovery_all()).is_some()
        });
        assert!(
            needed,
            "{id} {option:?} passes the full check with every input huge, so it needs no exception"
        );
    }
}

/// Each module that keeps the huge value on port defaults still does, so
/// [`HOLDS_UNTIL_NEXT_TRIGGER`] cannot go stale.
#[test]
fn every_held_value_exception_is_needed() {
    for id in HOLDS_UNTIL_NEXT_TRIGGER {
        let descriptor = ALL
            .iter()
            .find(|descriptor| descriptor.id == *id)
            .unwrap_or_else(|| panic!("{id} is not a module"));
        for running in [false, true] {
            let harness = harness(descriptor, running, false);
            assert!(
                failure(id, || harness.check_extreme_stays_bounded_all()).is_some(),
                "{id} returns to a normal size with every input huge, so it needs no exception"
            );
        }
    }
}

/// Each exception is still needed for its own input, so the table cannot go
/// stale and hide a later bug.
#[test]
fn every_exception_is_needed() {
    for (id, input, _) in EXCEPTIONS {
        let descriptor = ALL
            .iter()
            .find(|descriptor| descriptor.id == *id)
            .unwrap_or_else(|| panic!("{id} is not a module"));
        let needed = [(false, false), (false, true), (true, false), (true, true)]
            .into_iter()
            .any(|(running, busy)| {
                failure(id, || {
                    harness(descriptor, running, busy).check_extreme_recovery_on(input)
                })
                .is_some()
            });
        assert!(needed, "{id} `{input}` passes the full check, so it needs no exception");
    }
}

/// The last output of `harness`, past any start-up.
fn last(harness: Harness) -> f32 {
    *harness.render().output("out").last().expect("the render has frames")
}

/// Multiply holds a product that overflows, before and after its gain: a gain of
/// 0 is not NaN, and a gain of 10 does not overflow again.
#[test]
fn multiply_holds_a_product_that_overflows() {
    let both_huge = |gain: f32| {
        Harness::new(&math::MUL)
            .frames(8)
            .param("gain", gain)
            .input("a", Signal::constant(f32::MAX))
            .input("b", Signal::constant(f32::MAX))
    };
    assert_eq!(last(both_huge(0.0)), 0.0);
    assert_eq!(last(both_huge(1.0)), f32::MAX);
    assert_eq!(last(both_huge(10.0)), f32::MAX);
}

/// The overflow rule keeps the sign.
#[test]
fn a_held_overflow_keeps_its_sign() {
    let huge = Signal::constant;
    let mul = Harness::new(&math::MUL)
        .frames(8)
        .input("a", huge(f32::MAX))
        .input("b", huge(-f32::MAX));
    assert_eq!(last(mul), -f32::MAX);
    let add = Harness::new(&math::ADD)
        .frames(8)
        .input("a", huge(-f32::MAX))
        .input("b", huge(-f32::MAX));
    assert_eq!(last(add), -f32::MAX);
    let sub = Harness::new(&math::SUB)
        .frames(8)
        .input("a", huge(f32::MAX))
        .input("b", huge(-f32::MAX));
    assert_eq!(last(sub), f32::MAX);
    let fade = Harness::new(&mix::CROSSFADE)
        .frames(8)
        .input("a", huge(-f32::MAX))
        .input("b", huge(-f32::MAX));
    assert_eq!(last(fade), -f32::MAX);
}

/// A muted Mixer input with its level turned up gives 0, not NaN: the level
/// overflows it first, and infinity times a mute of 0 is NaN.
#[test]
fn a_muted_mixer_input_stays_silent_at_a_huge_level() {
    let mixer = Harness::new(&mix::MIXER)
        .frames(8)
        .param("level1", 2.0)
        .param("mute1", 1.0)
        .input("in1", Signal::constant(f32::MAX));
    assert_eq!(last(mixer), 0.0);
}

/// Amplifier holds a huge input at a gain above 1, and its gain of 0 is silence.
#[test]
fn the_amplifier_holds_an_overflow() {
    let amp = |gain: f32| {
        Harness::new(&level::AMP)
            .frames(8)
            .param("gain", gain)
            .input("in", Signal::constant(f32::MAX))
    };
    assert_eq!(last(amp(4.0)), f32::MAX);
    assert_eq!(last(amp(0.0)), 0.0);
}

/// The rule is about finite inputs only: an infinite input still passes through,
/// as a cable may carry it (ADR-0007).
#[test]
fn a_non_finite_input_still_passes_through() {
    let infinity = || Signal::constant(f32::INFINITY);
    assert_eq!(
        last(
            Harness::new(&math::MUL)
                .frames(8)
                .input("a", infinity())
                .input("b", Signal::constant(2.0))
        ),
        f32::INFINITY
    );
    assert_eq!(
        last(Harness::new(&math::ADD).frames(8).input("a", infinity())),
        f32::INFINITY
    );
    assert_eq!(
        last(Harness::new(&math::SUB).frames(8).input("a", infinity())),
        f32::INFINITY
    );
    assert_eq!(
        last(Harness::new(&mix::MIXER).frames(8).input("in1", infinity())),
        f32::INFINITY
    );
}
