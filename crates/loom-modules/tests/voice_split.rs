//! Voice split's outputs, checked at block sizes 1, 77, and 512 with the
//! harness's bundle inputs (`docs/design/polyphony.md`, Voice split).

use loom_modules::voice::SPLIT;
use loom_test::{Harness, Signal};

/// Frames in each render.
const FRAMES: usize = 1_024;

/// The output names, `out1` to `out8`.
const OUTS: [&str; 8] = ["out1", "out2", "out3", "out4", "out5", "out6", "out7", "out8"];

/// What zero-based voice `voice` of a test bundle plays on `frame`: a value
/// that moves and differs per voice.
fn voice_value(voice: usize, frame: usize) -> f32 {
    (voice as f32 + 1.0) * 0.1 + (frame as f32 * 0.01).sin() * 0.01
}

/// A bundle of `count` voices, each playing [`voice_value`].
fn bundle(count: usize) -> Vec<Signal> {
    (0..count)
        .map(|voice| Signal::from_fn(move |frame| voice_value(voice, frame)))
        .collect()
}

/// `in`'s declared default, which an output past the last voice holds.
fn default() -> f32 {
    match SPLIT.inputs[0].default {
        loom_core::PortDefault::Value(value) => value,
        other => panic!("unexpected default {other:?}"),
    }
}

/// Checks that output `k` (from 0) carries zero-based voice `voice` on every
/// frame, or the default when `voice` is `None`.
fn assert_carries(out: &[f32], k: usize, voice: Option<usize>) {
    for (frame, &value) in out.iter().enumerate() {
        let expected = voice.map_or(default(), |voice| voice_value(voice, frame));
        assert_eq!(value.to_bits(), expected.to_bits(), "{} at frame {frame}", OUTS[k]);
    }
}

/// `out1` to `out8` are the voices from `first` on, and an output past the
/// last voice holds the default.
#[test]
fn outputs_are_the_voices_from_first_on() {
    for (count, first) in [(8, 1), (8, 3), (12, 5), (3, 1), (16, 9)] {
        let render = Harness::new(&SPLIT)
            .frames(FRAMES)
            .param("first", first as f32)
            .bundle_input("in", bundle(count))
            .check_block_sizes();
        for (k, name) in OUTS.iter().enumerate() {
            let voice = first - 1 + k;
            assert_carries(render.output(name), k, (voice < count).then_some(voice));
        }
    }
}

/// A mono cable is voice 1 only: it reaches `out1`, and the other outputs
/// hold the default. With `first` at 2, every output holds the default.
#[test]
fn a_mono_cable_gives_out1_only() {
    for first in [1.0, 2.0] {
        let render = Harness::new(&SPLIT)
            .frames(FRAMES)
            .param("first", first)
            .input("in", Signal::from_fn(|frame| voice_value(0, frame)))
            .check_block_sizes();
        for (k, name) in OUTS.iter().enumerate() {
            let voice = (first == 1.0 && k == 0).then_some(0);
            assert_carries(render.output(name), k, voice);
        }
    }
}

/// An unpatched `in` reads its default as one voice, so every output is 0.
#[test]
fn an_unpatched_input_gives_the_default_everywhere() {
    let render = Harness::new(&SPLIT).frames(FRAMES).check_block_sizes();
    for (k, name) in OUTS.iter().enumerate() {
        assert_carries(render.output(name), k, None);
    }
}

/// `first` at 16 on a 16-voice bundle gives voice 16 on `out1` and the
/// default on `out2` to `out8`.
#[test]
fn first_at_16_reads_the_last_voice() {
    let render = Harness::new(&SPLIT)
        .frames(FRAMES)
        .param("first", 16.0)
        .bundle_input("in", bundle(16))
        .check_block_sizes();
    assert_carries(render.output("out1"), 0, Some(15));
    for (k, name) in OUTS.iter().enumerate().skip(1) {
        assert_carries(render.output(name), k, None);
    }
}

/// A change of `first` moves the outputs on its frame, with no glide.
#[test]
fn a_change_of_first_lands_on_its_frame() {
    let render = Harness::new(&SPLIT)
        .frames(FRAMES)
        .bundle_input("in", bundle(8))
        .param_at(300.0 / 48_000.0, "first", 4.0)
        .check_block_sizes();
    let out = render.output("out1");
    assert_eq!(out[299], voice_value(0, 299));
    assert_eq!(out[300], voice_value(3, 300));
    assert_eq!(render.output("out5")[299], voice_value(4, 299));
    assert_eq!(render.output("out5")[300], voice_value(7, 300), "voice 8 is the last");
    assert_eq!(render.output("out6")[299], voice_value(5, 299));
    assert_eq!(render.output("out6")[300], default(), "past voice 8");
}

/// A stereo bundle gives stereo outputs, each voice with both of its sides.
#[test]
fn a_stereo_bundle_gives_stereo_outputs() {
    let sides = (0..4)
        .map(|voice| {
            [
                Signal::from_fn(move |frame| voice_value(voice, frame)),
                Signal::from_fn(move |frame| -voice_value(voice, frame)),
            ]
        })
        .collect();
    let render = Harness::new(&SPLIT)
        .frames(FRAMES)
        .param("first", 2.0)
        .stereo_bundle_input("in", sides)
        .check_block_sizes();
    for (k, name) in OUTS.iter().enumerate() {
        let voice = 1 + k;
        for frame in [0, 511, 1_023] {
            let (left, right) = if voice < 4 {
                (voice_value(voice, frame), -voice_value(voice, frame))
            } else {
                (default(), default())
            };
            assert_eq!(render.voice_output(name, 0, 0)[frame], left, "{name} left");
            assert_eq!(render.voice_output(name, 0, 1)[frame], right, "{name} right");
        }
    }
}

/// Voice split has no state of its own, so a copy plays the same from the
/// frame it is made.
#[test]
fn a_copy_plays_the_same() {
    let harness = Harness::new(&SPLIT)
        .frames(FRAMES)
        .param("first", 3.0)
        .bundle_input("in", bundle(8));
    harness.check_copy_state(400);
    harness.check_reset();
}
