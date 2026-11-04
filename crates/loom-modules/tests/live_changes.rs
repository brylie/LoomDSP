//! A parameter or transport change during a render lands on the same frame at
//! every block size (module test harness, Changes during a render). A module that
//! follows a change by the block, not by the frame, breaks hard rule 2.

mod common;

use common::{busy_each, changed, dsp_modules};
use loom_core::{ModuleDescriptor, TimeSignature, TransportSettings};
use loom_modules::{clock, lfo, math, transport};
use loom_test::{DEFAULT_SAMPLE_RATE, Harness, Signal};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Frames in each render.
const FRAMES: usize = 9_600;

/// The frame of each change. No block size above 1 lands on it, so a block must
/// end there for the change to land on its frame.
const CHANGE: usize = 5_003;

/// [`CHANGE`] in seconds.
fn change_at() -> f64 {
    CHANGE as f64 / f64::from(DEFAULT_SAMPLE_RATE)
}

/// A playing transport at `tempo` in 4/4.
fn playing(tempo: f64) -> TransportSettings {
    TransportSettings {
        tempo,
        ..TransportSettings::default()
    }
}

/// Moves each parameter of every DSP module in turn, on `inputs`, and panics with
/// all that depend on the block size.
fn check_every_parameter(inputs: impl Fn(Harness, &'static ModuleDescriptor) -> Harness) {
    let mut failed = Vec::new();
    for descriptor in dsp_modules() {
        for spec in descriptor.params {
            let harness = inputs(
                Harness::new(descriptor).frames(FRAMES).transport(playing(130.0)),
                descriptor,
            )
            .param_at(change_at(), spec.name, changed(spec));
            if catch_unwind(AssertUnwindSafe(|| harness.check_block_sizes())).is_err() {
                failed.push((descriptor.id, spec.name));
            }
        }
    }
    assert!(failed.is_empty(), "these depend on the block size: {failed:?}");
}

/// Every parameter of every module, with inputs on their port defaults.
#[test]
fn every_parameter_change_lands_on_its_frame_with_default_inputs() {
    check_every_parameter(|harness, _| harness);
}

/// Every parameter of every module, with a busy signal on every input.
#[test]
fn every_parameter_change_lands_on_its_frame_with_busy_inputs() {
    check_every_parameter(busy_each);
}

/// Runs every DSP module through a tempo change, a stop, and a time signature
/// change, on `inputs`, and panics with all that depend on the block size.
fn check_every_transport_change(inputs: impl Fn(Harness, &'static ModuleDescriptor) -> Harness) {
    let start = playing(130.0);
    let changes = [
        playing(90.0),
        TransportSettings {
            running: false,
            ..start
        },
        TransportSettings {
            time_signature: TimeSignature::new(3, 4),
            ..start
        },
    ];
    let mut failed = Vec::new();
    for descriptor in dsp_modules() {
        for change in changes {
            let harness = inputs(Harness::new(descriptor).frames(FRAMES).transport(start), descriptor)
                .transport_at(change_at(), change);
            if catch_unwind(AssertUnwindSafe(|| harness.check_block_sizes())).is_err() {
                failed.push((descriptor.id, change.tempo, change.running, change.time_signature));
            }
        }
    }
    assert!(failed.is_empty(), "these depend on the block size: {failed:?}");
}

/// Every transport change on every module, with inputs on their port defaults, so
/// a module that follows the bar through a port default sees it.
#[test]
fn every_transport_change_lands_on_its_frame_with_default_inputs() {
    check_every_transport_change(|harness, _| harness);
}

/// Every transport change on every module, with a busy signal on every input.
#[test]
fn every_transport_change_lands_on_its_frame_with_busy_inputs() {
    check_every_transport_change(busy_each);
}

/// A smoothed parameter glides from the frame of the change, not before, and the
/// glide is bit-identical at every block size. Multiply's gain is smoothed by the
/// module, so its output shows the glide.
#[test]
fn a_smoothed_parameter_glides_from_its_frame() {
    let render = Harness::new(&math::MUL)
        .frames(FRAMES)
        .input("a", Signal::constant(1.0))
        .input("b", Signal::constant(1.0))
        .param_at(change_at(), "gain", 2.0)
        .check_block_sizes();
    let out = render.output("out");
    assert_eq!(out[CHANGE - 1], 1.0, "no change before the frame");
    let early = out[CHANGE + 8];
    assert!(early > 1.0 && early < 2.0, "it glides, not jumps: {early}");
    assert_eq!(out[FRAMES - 1], 2.0, "it arrives");
}

/// Clock follows a tempo change and a stop at every block size.
#[test]
fn clock_follows_transport_changes() {
    let stopped = TransportSettings {
        running: false,
        ..playing(130.0)
    };
    for settings in [playing(90.0), stopped] {
        Harness::new(&clock::CLOCK)
            .frames(FRAMES)
            .transport(playing(130.0))
            .transport_at(change_at(), settings)
            .check_block_sizes();
    }
}

/// An LFO in sync mode follows a tempo change at every block size.
#[test]
fn a_synced_lfo_follows_a_tempo_change() {
    Harness::new(&lfo::LFO)
        .frames(FRAMES)
        .param("sync", 1.0)
        .transport(playing(130.0))
        .transport_at(change_at(), playing(90.0))
        .check_block_sizes();
}

/// The frames where `harness` fires Transport's `jump` output.
fn jumps(harness: &Harness) -> Vec<usize> {
    let render = harness.check_block_sizes();
    let jump = render.output("jump");
    (0..jump.len()).filter(|&frame| jump[frame] > 0.5).collect()
}

/// A time signature change is a jump for one call, on its frame (clock spec,
/// rule 10). A tempo or running change is not, and neither is setting the same
/// time signature again.
#[test]
fn only_a_time_signature_change_is_a_jump() {
    let start = || {
        Harness::new(&transport::TRANSPORT)
            .frames(FRAMES)
            .transport(playing(130.0))
    };
    let three_four = TransportSettings {
        time_signature: TimeSignature::new(3, 4),
        ..playing(130.0)
    };
    assert_eq!(jumps(&start().transport_at(change_at(), three_four)), [CHANGE]);
    assert_eq!(
        jumps(&start().transport_at(change_at(), playing(90.0))),
        Vec::<usize>::new()
    );
    assert_eq!(
        jumps(&start().transport_at(change_at(), playing(130.0))),
        Vec::<usize>::new()
    );
    let stopped = TransportSettings {
        running: false,
        ..playing(130.0)
    };
    assert_eq!(jumps(&start().transport_at(change_at(), stopped)), Vec::<usize>::new());
}

/// Two changes on one frame are one change: a time signature change keeps its
/// jump when a tempo change follows it there. A change on frame 0 replaces the
/// starting transport, and is a jump there.
#[test]
fn changes_on_one_frame_keep_the_jump() {
    let start = || {
        Harness::new(&transport::TRANSPORT)
            .frames(FRAMES)
            .transport(playing(130.0))
    };
    let three_four = TransportSettings {
        time_signature: TimeSignature::new(3, 4),
        ..playing(130.0)
    };
    let both = start().transport_at(change_at(), three_four).transport_at(
        change_at(),
        TransportSettings {
            tempo: 90.0,
            ..three_four
        },
    );
    assert_eq!(jumps(&both), [CHANGE]);
    assert_eq!(jumps(&start().transport_at(0.0, three_four)), [0]);
}
