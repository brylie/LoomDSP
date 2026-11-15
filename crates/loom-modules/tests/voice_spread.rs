//! Voice spread's offsets, rendered as a voice past 1 with `Harness::voice`
//! at block sizes 1, 77, and 512 (`docs/design/polyphony.md`, Voice spread).

use loom_dsp::blend::spread_offset;
use loom_modules::voice::SPREAD;
use loom_test::{Harness, Signal};

/// Frames in each render.
const FRAMES: usize = 1_024;

/// The input every test feeds: a slow sine with a few awkward values, so
/// `out = in` is checked on more than one number.
fn input(frame: usize) -> f32 {
    match frame {
        10 => -0.0,
        11 => f32::MIN_POSITIVE / 4.0,
        _ => (frame as f32 * 0.01).sin() * 0.3,
    }
}

/// Voice spread as voice `voice` of `count`, in `mode`, with `amount` and
/// `seed`, checked at every block size. Returns its output.
fn spread(voice: usize, count: usize, mode: f32, amount: f32, seed: f32) -> Vec<f32> {
    Harness::new(&SPREAD)
        .frames(FRAMES)
        .voice(voice, count)
        .input("in", Signal::from_fn(input))
        .param("mode", mode)
        .param("amount", amount)
        .param("seed", seed)
        .check_block_sizes()
        .output("out")
        .to_vec()
}

/// Checks that a render added `amount * offset` to the input on every frame.
/// `out` must equal `in + amount * offset` exactly, so the test compares
/// against that sum, not against `out - in`, which rounds. An offset of 0
/// passes the input as it is, so -0.0 stays -0.0.
fn offset_of(out: &[f32], amount: f32, offset: f32) {
    for (frame, &value) in out.iter().enumerate() {
        let expected = if offset == 0.0 {
            input(frame)
        } else {
            input(frame) + amount * offset
        };
        assert_eq!(
            value.to_bits(),
            expected.to_bits(),
            "frame {frame}: {value} != {expected}"
        );
    }
}

/// The first Random offset of voice `voice` of `count` at `seed`, read back
/// from a render of a constant 0 input with `amount` 1.
fn random_offset(voice: usize, count: usize, seed: f32) -> f32 {
    let out = Harness::new(&SPREAD)
        .frames(FRAMES)
        .voice(voice, count)
        .param("mode", 2.0)
        .param("amount", 1.0)
        .param("seed", seed)
        .check_block_sizes()
        .output("out")
        .to_vec();
    assert!(out.iter().all(|&v| v == out[0]), "voice {voice}: the offset moves");
    out[0]
}

/// Even gives the stack's offsets: `spread_offset`, from -1 at voice 1 to 1
/// at the last voice.
#[test]
fn even_gives_the_stacks_offsets() {
    for count in [2, 3, 8, 16] {
        for voice in 1..=count {
            let expected = spread_offset(voice as u32 - 1, count as u32);
            offset_of(&spread(voice, count, 0.0, 0.25, 1.0), 0.25, expected);
        }
    }
}

/// Even's middle voice of an odd count has offset 0, so it plays `in` bit
/// for bit.
#[test]
fn even_middle_voice_passes_the_input() {
    let out = spread(2, 3, 0.0, 0.5, 1.0);
    for (frame, value) in out.iter().enumerate() {
        assert_eq!(value.to_bits(), input(frame).to_bits(), "frame {frame}");
    }
}

/// Series runs from 0 at voice 1 up to `amount` at the last voice.
#[test]
fn series_runs_from_zero_up_to_the_amount() {
    for count in [2, 5, 8] {
        for voice in 1..=count {
            let expected = (voice - 1) as f32 / (count - 1) as f32;
            offset_of(&spread(voice, count, 1.0, 0.4, 1.0), 0.4, expected);
        }
    }
    let first = spread(1, 8, 1.0, 0.4, 1.0);
    let last = spread(8, 8, 1.0, 0.4, 1.0);
    assert_eq!(first[100], input(100));
    assert_eq!(last[100], input(100) + 0.4);
}

/// Random gives each voice its own offset from -1 up to 1, which is not 0.
/// The same seed gives the same offsets, and another seed other ones.
#[test]
fn random_differs_per_voice_and_repeats_per_seed() {
    let offsets: Vec<f32> = (1..=8).map(|voice| random_offset(voice, 8, 7.0)).collect();
    for (i, &offset) in offsets.iter().enumerate() {
        assert!((-1.0..1.0).contains(&offset), "voice {}: {offset}", i + 1);
        assert_ne!(offset, 0.0, "voice {}", i + 1);
        assert!(!offsets[..i].contains(&offset), "voice {} repeats {offset}", i + 1);
    }
    let again: Vec<f32> = (1..=8).map(|voice| random_offset(voice, 8, 7.0)).collect();
    assert_eq!(again, offsets, "the same seed gives the same offsets");
    let other: Vec<f32> = (1..=8).map(|voice| random_offset(voice, 8, 8.0)).collect();
    assert_ne!(other, offsets, "another seed gives other offsets");
    // A signal on `in` gets exactly the offset times the amount.
    offset_of(&spread(5, 8, 2.0, 0.3, 7.0), 0.3, offsets[4]);
}

/// Random's offsets are not the first numbers of the streams Random, Chance,
/// and Selector play at the same seed, because Voice spread draws from its own
/// stream number.
#[test]
fn random_offsets_are_not_another_modules_first_numbers() {
    for seed in [1.0, 7.0] {
        let spread: Vec<f32> = (1..=4).map(|voice| random_offset(voice, 4, seed)).collect();
        let shared: Vec<f32> = (0..4)
            .map(|index| {
                let mut rng = loom_dsp::random::Rng::new(seed as u32 | ((index as u32) << 24));
                2.0 * rng.draw() - 1.0
            })
            .collect();
        assert_ne!(spread, shared, "seed {seed}");
    }
}

/// With one voice, every mode gives `out = in` bit for bit, whatever the
/// amount and the seed.
#[test]
fn one_voice_passes_the_input_in_every_mode() {
    for mode in [0.0, 1.0, 2.0] {
        for amount in [0.0, 0.5, -10.0] {
            let out = spread(1, 1, mode, amount, 9.0);
            for (frame, value) in out.iter().enumerate() {
                assert_eq!(
                    value.to_bits(),
                    input(frame).to_bits(),
                    "mode {mode}, amount {amount}, frame {frame}"
                );
            }
        }
    }
}

/// Raising the count from 4 to 8 leaves the Random offsets of voices 1 to 4
/// where they were, so the voices already tuned do not move.
#[test]
fn random_offsets_hold_when_the_count_grows() {
    for voice in 1..=4 {
        assert_eq!(
            random_offset(voice, 4, 3.0).to_bits(),
            random_offset(voice, 8, 3.0).to_bits(),
            "voice {voice}"
        );
    }
}

/// A right side plays what its left side plays: the side is not in the draw
/// (ADR-0045). A right side that takes the left side's state continues it
/// bit for bit (the strict copy check, stereo spec, rule 27).
#[test]
fn both_sides_of_a_voice_agree() {
    for mode in [0.0, 1.0, 2.0] {
        let side = |channel| {
            Harness::new(&SPREAD)
                .frames(FRAMES)
                .voice(6, 8)
                .channel(channel)
                .input("in", Signal::from_fn(input))
                .param("mode", mode)
                .param("amount", 0.2)
                .param("seed", 11.0)
        };
        assert_eq!(
            side(0).render().output("out"),
            side(1).render().output("out"),
            "mode {mode}"
        );
        assert!(
            side(1).check_copy_state(300),
            "mode {mode}: copy_state_from returned false"
        );
    }
}

/// A mode set during the render switches on its frame, and a seed set
/// during the render picks the new offset on its frame. These are knob
/// changes (`set_param`), not modulation: `modulated_choices.rs` checks a
/// modulated mode and seed.
#[test]
fn mode_and_seed_changes_land_on_their_frame() {
    let out = Harness::new(&SPREAD)
        .frames(FRAMES)
        .voice(1, 8)
        .param("amount", 1.0)
        .param_at(200.0 / 48_000.0, "mode", 1.0)
        .param_at(400.0 / 48_000.0, "mode", 2.0)
        .param_at(600.0 / 48_000.0, "seed", 5.0)
        .check_block_sizes()
        .output("out")
        .to_vec();
    assert_eq!(out[199], -1.0, "Even, voice 1");
    assert_eq!(out[200], 0.0, "Series, voice 1");
    assert_eq!(out[400], random_offset(1, 8, 1.0));
    assert_eq!(out[599], random_offset(1, 8, 1.0));
    assert_eq!(out[600], random_offset(1, 8, 5.0));
}
