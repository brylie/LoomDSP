//! Every built-in module with state hands it over with
//! `Module::copy_state_from` (ADR-0022, stereo spec rules 25 to 27). When a
//! module turns stereo, its new right-side copy takes the running left
//! copy's state, so both sides continue together.

mod common;

use common::{
    SEEDED_ROUTES, WRAP_FRAME, busy_each, busy_without_reset, changed, choice_failures, dsp_modules, playing_short_bar,
    pulses, selector,
};
use loom_core::{MidiEvent, ModuleDescriptor, ModuleRole, TransportSettings};
use loom_test::Harness;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Frames rendered in each check.
const FRAMES: usize = 9_600;

/// The frame where the copy takes the state: in the middle of the MIDI
/// notes, the gates, and the delay times, so each kind of state is busy.
const AT: usize = 2_500;

/// Frames in a playing render, long enough to pass the bar wrap.
const PLAYING_FRAMES: usize = WRAP_FRAME + 7_846;

/// Notes held across every copy frame.
fn notes() -> [MidiEvent; 5] {
    [
        MidiEvent::note_on(100, 0, 60, 100),
        MidiEvent::note_on(1_000, 0, 67, 64),
        MidiEvent::from_bytes(2_000, &[0xE0, 0x00, 0x60]),
        MidiEvent::note_off(3_000, 0, 67),
        MidiEvent::note_off(6_000, 0, 60),
    ]
}

/// A harness for `descriptor` with every parameter away from its default,
/// distinct busy inputs, and notes held across the copy. Reset inputs stay
/// unconnected when `reset_busy` is false.
fn harness(descriptor: &'static ModuleDescriptor, reset_busy: bool) -> Harness {
    let harness = descriptor
        .params
        .iter()
        .fold(Harness::new(descriptor).frames(FRAMES), |h, spec| {
            h.param(spec.name, changed(spec))
        });
    let harness = if reset_busy {
        busy_each(harness, descriptor)
    } else {
        busy_without_reset(harness, descriptor)
    };
    harness.midi(notes())
}

/// True for a module with a random generator, which re-derives it when it
/// copies (rule 27). A seeded route copies it as it is (ADR-0036), so it takes
/// the strict check, which is what proves both sides match.
fn seeded(descriptor: &ModuleDescriptor) -> bool {
    descriptor.param_index("seed").is_some() && !SEEDED_ROUTES.contains(&descriptor.id)
}

/// Every choice continues identically on distinct busy inputs, including reset.
#[test]
fn every_module_hands_over_its_state() {
    check_busy_copies(true, false);
}

/// Without reset edges, a random stream advances far enough to expose its loss.
#[test]
fn every_module_hands_over_its_state_without_busy_resets() {
    check_busy_copies(false, false);
}

/// A seeded signal generator re-derives its stream; each option copies repeatably.
#[test]
fn seeded_modules_copy_repeatably() {
    assert!(dsp_modules().any(seeded), "Noise has a seed");
    check_busy_copies(true, true);
}

/// Runs every option, retaining strict continuation for seeded routing modules.
fn check_busy_copies(reset_busy: bool, repeatable: bool) {
    let failed: Vec<String> = dsp_modules()
        .filter(|d| seeded(d) == repeatable)
        .flat_map(|descriptor| {
            choice_failures(
                descriptor,
                changed,
                || harness(descriptor, reset_busy),
                |h| {
                    if repeatable {
                        h.check_copy_state_repeatable(AT);
                    } else {
                        h.check_copy_state(AT);
                    }
                },
            )
        })
        .collect();
    assert!(
        failed.is_empty(),
        "busy copy at {AT}, reset busy = {reset_busy}, repeatable = {repeatable}:\n{}",
        failed.join("\n")
    );
}

/// All default-input playing scenarios, at one copy frame. Separate tests let
/// the runner spread the longer per-option renders across workers.
fn check_playing_copies(at: usize) {
    let mut failed = Vec::new();
    for descriptor in dsp_modules() {
        for turned in [false, true] {
            let baseline = |spec: &loom_core::ParamSpec| if turned { changed(spec) } else { spec.default };
            let build = || {
                descriptor.params.iter().fold(
                    Harness::new(descriptor)
                        .frames(PLAYING_FRAMES)
                        .transport(playing_short_bar())
                        .midi(notes()),
                    |h, spec| h.param(spec.name, baseline(spec)),
                )
            };
            failed.extend(
                choice_failures(descriptor, baseline, build, |h| {
                    if seeded(descriptor) {
                        h.check_copy_state_repeatable(at);
                    } else {
                        h.check_copy_state(at);
                    }
                })
                .into_iter()
                .map(|message| format!("turned = {turned}: {message}")),
            );
        }
    }
    assert!(failed.is_empty(), "playing copy at {at}:\n{}", failed.join("\n"));
}

/// Every choice on port defaults with the transport playing, copied mid-cycle.
#[test]
fn every_playing_module_hands_over_its_state_mid_cycle() {
    check_playing_copies(AT);
}

/// A copy on the wrap must remember the preceding phase.
#[test]
fn every_playing_module_hands_over_its_state_on_the_wrap() {
    check_playing_copies(WRAP_FRAME);
}

/// A copy after the wrap must keep the pending trigger and flash state.
#[test]
fn every_playing_module_hands_over_its_state_after_the_wrap() {
    check_playing_copies(WRAP_FRAME + 1);
}

/// At one step, the wrap alone enters the step again. With more steps a step
/// change can hide a dropped wrap detector.
#[test]
fn a_one_step_euclidean_rhythm_copies_its_wrap_detector() {
    let h = Harness::new(&loom_modules::gate::EUCLID)
        .frames(PLAYING_FRAMES)
        .transport(playing_short_bar())
        .param("source", 0.0)
        .param("steps", 1.0)
        .param("hits", 1.0);
    for at in [WRAP_FRAME, WRAP_FRAME + 1] {
        assert!(h.check_copy_state(at), "frame {at}: copy_state_from returned false");
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

/// A Selector copy takes the whole random stream, so a new right side makes the
/// left side's picks (ADR-0036), and the wait, so its first step stays on input
/// 1. It copies at the wait, and in the middle of the picks.
#[test]
fn a_selector_copies_its_stream_and_its_wait() {
    for mode in [0.0, 2.0] {
        for at in [100, 750] {
            assert!(selector(mode).check_copy_state(at), "mode {mode}, frame {at}");
        }
    }
}

/// A Euclidean rhythm copy takes the wait, so the next clock still plays the
/// first step, and the step it is on. It copies at the wait, and mid-pattern.
#[test]
fn a_euclidean_rhythm_copies_its_wait_and_its_step() {
    let harness = Harness::new(&loom_modules::gate::EUCLID)
        .frames(2_000)
        .param("source", 1.0)
        .param("steps", 8.0)
        .param("hits", 3.0)
        .input("clock", pulses(500));
    for at in [100, 750] {
        assert!(harness.check_copy_state(at), "frame {at}");
    }
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
