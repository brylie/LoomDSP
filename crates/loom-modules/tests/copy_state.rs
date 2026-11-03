//! Every built-in module with state hands it over with
//! `Module::copy_state_from` (ADR-0022, stereo spec rules 25 to 27). When a
//! module turns stereo, its new right-side copy takes the running left
//! copy's state, so both sides continue together.

mod common;

use common::{busy, changed};
use loom_core::{MidiEvent, ModuleDescriptor, ModuleRole, TransportSettings};
use loom_modules::ALL;
use loom_test::Harness;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Frames rendered in each check.
const FRAMES: usize = 9_600;

/// The frame where the copy takes the state: in the middle of the MIDI
/// notes, the gates, and the delay times, so each kind of state is busy.
const AT: usize = 2_500;

/// A harness for `descriptor` with every parameter away from its default,
/// a busy signal on every input, and notes held across the copy.
fn harness(descriptor: &'static ModuleDescriptor) -> Harness {
    let harness = descriptor
        .params
        .iter()
        .fold(Harness::new(descriptor).frames(FRAMES), |h, spec| {
            h.param(spec.name, changed(spec))
        });
    let harness = descriptor
        .inputs
        .iter()
        .fold(harness, |h, port| h.input(port.name, busy(37.0)));
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
    let message = result.expect_err("the copy played the running instance's numbers");
    let message = message.downcast_ref::<String>().expect("the harness reports a String");
    assert!(message.contains("output `out` at frame"), "{message}");
    assert!(message.contains("`copy_state_from` returned true"), "{message}");
}

/// A harness on a playing transport at 130 BPM. A beat is 22153.8 frames, so the
/// frame after a boundary has a phase above 0. The wrap detector fires there
/// only if it remembers the frame before, which is what a dropped detector
/// loses. At a tempo with whole-frame beats, a new detector fires on the same
/// frame by its first-frame rule, and the copy would pass without its state.
fn playing(descriptor: &'static ModuleDescriptor, frames: usize) -> Harness {
    Harness::new(descriptor).frames(frames).transport(TransportSettings {
        tempo: 130.0,
        running: true,
        ..TransportSettings::default()
    })
}

/// Checks a copy taken in the middle of a cycle, on the frame `output` fires
/// its second time, and on the frame after that, while the light still has the
/// flash to show. Each copy runs on through the next firing.
fn check_playing_copy(harness: &Harness, output: &str) {
    let render = harness.render();
    let fires: Vec<usize> = (0..render.frames())
        .filter(|&frame| render.output(output)[frame] > 0.5)
        .collect();
    assert!(fires.len() >= 3, "{output} fired on frames {fires:?}");
    let (second, third) = (fires[1], fires[2]);
    for at in [second + (third - second) / 2, second, second + 1] {
        assert!(
            at < third,
            "the copy at frame {at} must come before the firing on frame {third}"
        );
        assert!(
            harness.check_copy_state(at),
            "frame {at}: copy_state_from returned false"
        );
    }
}

/// A Clock copied while the transport plays keeps its wrap detector and its
/// pending flash, so its triggers land with the running instance's (#417).
#[test]
fn a_playing_clock_copies_its_running_state() {
    check_playing_copy(&playing(&loom_modules::clock::CLOCK, 70_000), "trigger");
}

/// A Transport copied while it plays keeps its beat detector and its pending
/// flash (#417).
#[test]
fn a_playing_transport_copies_its_beat_state() {
    check_playing_copy(&playing(&loom_modules::transport::TRANSPORT, 70_000), "beat");
}

/// A Transport copied while it plays keeps its bar detector. The third bar
/// starts past frame 177,000.
#[test]
fn a_playing_transport_copies_its_bar_state() {
    check_playing_copy(&playing(&loom_modules::transport::TRANSPORT, 180_000), "bar");
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
