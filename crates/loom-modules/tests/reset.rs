//! `reset()` returns a module to its start, for every module (module test
//! harness, Reset returns a module to its start). Panic and a transport reset
//! rely on it, and the `Controller` calls it once after a module's first values
//! are set, so a module made with values other than its built-in ones does not
//! glide to them (live patching spec, rule 7).

mod common;

use common::{busy_each, changed, choice_failures, dsp_modules, pulses, selector};
use loom_core::{MidiEvent, ModuleDescriptor, TransportSettings};
use loom_test::Harness;

/// A one-second harness for `descriptor` with every parameter away from its
/// default and a note left held across `reset()`, so MIDI Note's state is
/// tested.
fn harness(descriptor: &'static ModuleDescriptor, running: bool) -> Harness {
    descriptor
        .params
        .iter()
        .fold(Harness::new(descriptor).seconds(1.0), |h, spec| {
            h.param(spec.name, changed(spec))
        })
        .midi([MidiEvent::note_on(100, 0, 60, 100)])
        .transport(TransportSettings {
            running,
            ..TransportSettings::default()
        })
}

/// A busy signal on every input, each at its own frequency.
fn on_busy_inputs(harness: Harness, descriptor: &'static ModuleDescriptor) -> Harness {
    busy_each(harness, descriptor)
}

/// One message per failing option of each DSP module in `ALL`, naming its
/// setting and module, on the harness `build` makes.
fn failing(build: impl Fn(&'static ModuleDescriptor) -> Harness) -> Vec<String> {
    dsp_modules()
        .flat_map(|descriptor| choice_failures(descriptor, changed, || build(descriptor), Harness::check_reset))
        .collect()
}

/// With a busy signal on every input, on a stopped transport. Panics with the
/// IDs of all that fail.
#[test]
fn reset_clears_every_module_on_busy_inputs_when_stopped() {
    let failed = failing(|d| on_busy_inputs(harness(d, false), d));
    assert!(failed.is_empty(), "reset() leaves state in:\n{}", failed.join("\n"));
}

/// With a busy signal on every input, on a running transport, so state that
/// only moves during playback is covered too.
#[test]
fn reset_clears_every_module_on_busy_inputs_when_playing() {
    let failed = failing(|d| on_busy_inputs(harness(d, true), d));
    assert!(failed.is_empty(), "reset() leaves state in:\n{}", failed.join("\n"));
}

/// With every input on its port default, on a stopped transport.
#[test]
fn reset_clears_every_module_on_default_inputs_when_stopped() {
    let failed = failing(|d| harness(d, false));
    assert!(failed.is_empty(), "reset() leaves state in:\n{}", failed.join("\n"));
}

/// With every input on its port default, on a running transport, so a module
/// also clears what it gathers from the bar phase and the play state.
#[test]
fn reset_clears_every_module_on_default_inputs_when_playing() {
    let failed = failing(|d| harness(d, true));
    assert!(failed.is_empty(), "reset() leaves state in:\n{}", failed.join("\n"));
}

/// A Selector's reset goes back to the wait and the start of the picks, with a
/// different value on each input so a wrong pick shows.
#[test]
fn reset_clears_selector_picks_and_wait() {
    for mode in [0.0, 2.0] {
        selector(mode).check_reset();
    }
}

/// A clocked Euclidean rhythm's reset goes back to the wait, so its first clock
/// plays step 0 again.
#[test]
fn reset_clears_a_clocked_euclidean_rhythm() {
    Harness::new(&loom_modules::gate::EUCLID)
        .frames(2_000)
        .param("source", 1.0)
        .param("steps", 8.0)
        .param("hits", 3.0)
        .input("clock", pulses(500))
        .check_reset();
}
