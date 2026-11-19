//! Golden-audio tests for every example patch (#93, NFR-OSS-05).
//!
//! Each patch in `examples/patches/` runs its performance ([`performance`])
//! through the engine, and its stereo output is compared with
//! `tests/reference/patches/<name>.wav`. A change that keeps every module
//! correct can still change how a patch sounds, for example through the
//! compiler's feedback-loop handling; these tests catch it.
//!
//! The render is also played in several host buffer sizes, which must give
//! bit-identical output (AGENTS.md hard rule 2). After an intended change,
//! update the files with `LOOM_UPDATE_REFERENCES=1 cargo test -p
//! loom-modules --test patches`, listen to them, and commit them.

mod common;

use common::{Note, SAMPLE_RATE, patch_path, render};
use loom_core::{EngineConfig, OutputSafety, create_engine};
use loom_test::check_channels;
use std::path::{Path, PathBuf};

/// Default render length: 0.2 s, so most references stay under 80 KB.
const DEFAULT_FRAMES: usize = 9_600;

/// The default note: middle C, from 5 ms to 150 ms, so the
/// reference holds the attack, the held part, and the release.
const NOTE: Note = Note {
    note: 60,
    on: 240,
    off: 7_200,
};

/// The chord the polyphonic examples play: C, E, G, and B up from middle C. The
/// notes start on different frames inside the engine block from frame 128
/// to 255 and end on different frames, so four voices sound at once and
/// their release tails overlap (polyphony.md, The shipped example).
const CHORD: [Note; 4] = [
    Note {
        note: 60,
        on: 240,
        off: 4_800,
    },
    Note {
        note: 64,
        on: 243,
        off: 5_600,
    },
    Note {
        note: 67,
        on: 247,
        off: 6_400,
    },
    Note {
        note: 71,
        on: 251,
        off: 7_200,
    },
];

/// Host buffer sizes the render is played in. 1 is the smallest a host
/// can ask for, 77 does not divide the engine's block, and 1,000 is larger
/// than it, so the engine splits each buffer.
const BUFFER_SIZES: [usize; 4] = [1, 77, 256, 1_000];

/// Largest allowed difference from a reference, per sample. Ten times the
/// module harness default: a patch runs several modules in a row, some in
/// feedback loops, so each platform's maths library can move the last bits
/// further than in one module (module-test-harness.md, rule 6).
const TOLERANCE: f32 = 1e-4;

/// Every example patch, sorted by file name, so a new patch is tested as
/// soon as it is added.
fn example_patches() -> Vec<PathBuf> {
    let folder = patch_path("");
    let mut patches: Vec<PathBuf> = std::fs::read_dir(&folder)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", folder.display()))
        .map(|entry| entry.expect("a folder entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "ron"))
        .collect();
    patches.sort();
    patches
}

/// Render length in frames and the MIDI notes to send.
fn performance(patch: &Path) -> (usize, &'static [Note]) {
    match patch.file_stem().and_then(|name| name.to_str()) {
        Some("glass-chords" | "polysynth" | "polysynth-large") => (DEFAULT_FRAMES, &CHORD),
        // 0.8 s includes several chances to fire the envelope.
        Some("broken-pulse") => (38_400, &[]),
        // 0.7 s includes the dry pluck and two returns from the 278 ms delay.
        Some("tidal-echo") => (33_600, &[]),
        _ => (DEFAULT_FRAMES, &[NOTE]),
    }
}

/// Plays `patch` in host buffers of `buffer` frames. The
/// output safety stage is off, so the reference holds what the patch makes,
/// not what the limiter does to it (ADR-0019).
fn play(patch: &Path, buffer: usize) -> Vec<Vec<f32>> {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let loaded = common::load_patch(patch);
    let mut parts = create_engine(loom_modules::registry(), config);
    loaded
        .load_into(&mut parts.controller)
        .unwrap_or_else(|e| panic!("{} is valid: {e}", patch.display()));
    let (frames, notes) = performance(patch);
    let (left, right) = render(&mut parts, frames, buffer, notes);
    vec![left, right]
}

/// Where the reference of `patch` lives.
fn reference_path(patch: &Path) -> PathBuf {
    let stem = patch.file_stem().expect("a file name").to_string_lossy();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/reference/patches")
        .join(format!("{stem}.wav"))
}

/// Every example patch sounds as its reference does, and the same at every
/// host buffer size. A patch without a reference fails, with how to make
/// one.
#[test]
fn every_example_patch_matches_its_reference() {
    let patches = example_patches();
    assert!(!patches.is_empty(), "examples/patches holds patches");
    for patch in &patches {
        let name = patch.file_name().expect("a file name").to_string_lossy();
        let first = play(patch, BUFFER_SIZES[0]);
        for &buffer in &BUFFER_SIZES[1..] {
            let other = play(patch, buffer);
            for (side, (a, b)) in ["left", "right"].iter().zip(first.iter().zip(&other)) {
                let frame = a.iter().zip(b).position(|(x, y)| x.to_bits() != y.to_bits());
                assert_eq!(
                    frame, None,
                    "{name}: the {side} output in buffers of {buffer} differs from buffers of {} at that frame",
                    BUFFER_SIZES[0]
                );
            }
        }
        assert!(
            first.iter().flatten().all(|s| s.is_finite()),
            "{name}: the output is finite"
        );
        check_channels(
            &name,
            &["left", "right"],
            &first,
            SAMPLE_RATE,
            &reference_path(patch),
            TOLERANCE,
        );
    }
}

/// Every reference belongs to an example patch, so a removed or renamed
/// patch leaves no stale file behind.
#[test]
fn every_patch_reference_has_its_patch() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/reference/patches");
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return;
    };
    let wanted: Vec<PathBuf> = example_patches().iter().map(|p| reference_path(p)).collect();
    for entry in entries {
        let path = entry.expect("a folder entry").path();
        assert!(
            wanted.contains(&path),
            "{} has no patch in examples/patches; remove it",
            path.display()
        );
    }
}
