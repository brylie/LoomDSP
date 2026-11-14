//! The built-in Synth voice (MT-SRC-08; sub-patches.md, Built-in
//! abstractions): with its velocity input unpatched it plays at full
//! velocity, because the Amp's `level` input reads its normal of 1
//! (ADR-0043). The voice is the one in `examples/patches/synth-voice.ron`,
//! which a test in `loom-app` keeps equal to the built-in file.

mod common;

use common::{Note, SAMPLE_RATE, patch_path, render};
use loom_core::{EngineConfig, OutputSafety, create_engine};
use loom_patch::{Patch, PatchCable, PatchModule};

const FRAMES: usize = 24_000;
const NOTE: Note = Note {
    note: 60,
    on: 240,
    off: 12_000,
};

/// The example with its velocity cable replaced: by nothing, or by a
/// constant of `value`.
fn voice(velocity: Option<f32>) -> Patch {
    let mut patch = common::load_patch(&patch_path("synth-voice.ron"));
    patch.cables.retain(|c| c.to.1 != "velocity");
    if let Some(value) = velocity {
        let mut constant = PatchModule::new(9, "loom.math.constant@1");
        constant.params.insert("value".to_string(), value);
        patch.modules.push(constant);
        patch.cables.push(PatchCable {
            from: (9, "out".to_string()),
            to: (2, "velocity".to_string()),
        });
    }
    patch
}

/// The left channel of `patch` played with [`NOTE`] in buffers of `block`,
/// output safety off.
fn play(patch: &Patch, block: usize) -> Vec<f32> {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    patch.load_into(&mut parts.controller).expect("the patch loads");
    render(&mut parts, FRAMES, block, &[NOTE]).0
}

/// Unpatched velocity is full velocity, bit for bit: the same as a
/// constant 1 patched in, at block sizes 1, 77, and 512. A constant 0.5
/// plays quieter, so the input is really wired.
#[test]
fn unpatched_velocity_plays_at_full_velocity() {
    for block in [1, 77, 512] {
        let unpatched = play(&voice(None), block);
        assert!(unpatched.iter().any(|v| v.abs() > 0.05), "the voice plays");
        assert_eq!(unpatched, play(&voice(Some(1.0)), block), "block {block}");
    }
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let half = play(&voice(Some(0.5)), 512);
    assert!(peak(&half) < peak(&play(&voice(None), 512)) * 0.6);
}
