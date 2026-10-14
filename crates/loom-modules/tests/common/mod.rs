//! Shared helpers for engine integration tests.

#![allow(dead_code)]

use loom_core::{EngineConfig, EngineParts, MidiEvent, create_engine};
use loom_patch::Patch;
use std::path::PathBuf;

pub const SAMPLE_RATE: f32 = 48_000.0;

pub fn patch_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/patches")
        .join(name)
}

pub fn load(name: &str) -> EngineParts {
    load_with(name, EngineConfig::with_sample_rate(SAMPLE_RATE))
}

pub fn load_with(name: &str, config: EngineConfig) -> EngineParts {
    let patch = Patch::load(patch_path(name)).expect("example patch loads");
    let mut parts = create_engine(loom_modules::registry(), config);
    patch.load_into(&mut parts.controller).expect("example patch is valid");
    parts
}

/// A note held from frame `on` to frame `off`.
pub struct Note {
    pub note: u8,
    pub on: usize,
    pub off: usize,
}

/// Renders `frames` frames in host buffers of `block` frames.
/// `between` runs on the "control thread" before each buffer.
pub fn render_with(
    parts: &mut EngineParts,
    frames: usize,
    block: usize,
    notes: &[Note],
    mut between: impl FnMut(usize, &mut EngineParts),
) -> (Vec<f32>, Vec<f32>) {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut position = 0;
    while position < frames {
        let n = block.min(frames - position);
        between(position, parts);
        for note in notes {
            if (position..position + n).contains(&note.on) {
                assert!(
                    parts
                        .midi
                        .send(MidiEvent::note_on((note.on - position) as u32, 0, note.note, 100))
                );
            }
            if (position..position + n).contains(&note.off) {
                assert!(
                    parts
                        .midi
                        .send(MidiEvent::note_off((note.off - position) as u32, 0, note.note))
                );
            }
        }
        parts
            .engine
            .process(&mut left[position..position + n], &mut right[position..position + n]);
        position += n;
    }
    (left, right)
}

pub fn render(parts: &mut EngineParts, frames: usize, block: usize, notes: &[Note]) -> (Vec<f32>, Vec<f32>) {
    render_with(parts, frames, block, notes, |_, _| {})
}

pub fn rms(signal: &[f32]) -> f32 {
    (signal.iter().map(|x| x * x).sum::<f32>() / signal.len().max(1) as f32).sqrt()
}

/// One second with a note held for the first half.
pub fn one_note() -> Vec<Note> {
    vec![Note {
        note: 57,
        on: 100,
        off: 24_000,
    }]
}
