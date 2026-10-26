//! Shared helpers for engine integration tests.

#![allow(dead_code)]

use loom_core::{EngineConfig, EngineParts, MeterReading, MidiEvent, ModuleId, OutputSafety, ProbeId, create_engine};
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

/// As [`load`], with the output safety stage off, for tests that compare
/// the output with exact values (ADR-0019).
pub fn load_raw(name: &str) -> EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    load_with(name, config)
}

pub fn load_with(name: &str, config: EngineConfig) -> EngineParts {
    let patch = Patch::load(patch_path(name)).expect("example patch loads");
    let mut parts = create_engine(loom_modules::registry(), config);
    patch.load_into(&mut parts.controller).expect("example patch is valid");
    parts
}

/// Sets a parameter after selecting the smallest control range that holds
/// `value`, as a loaded patch does (control-ranges.md, rule 7). Without
/// it, a value outside the default range of Add, Multiply, or Constant is
/// clamped into that range.
pub fn set_wide(controller: &mut loom_core::Controller, module: ModuleId, name: &str, value: f32) {
    let node = controller.graph().node(module).expect("module exists");
    let index = node.descriptor.param_index(name).expect("parameter exists");
    if let Some(range) = node.descriptor.params[index].smallest_range_holding(value) {
        controller.set_range(module, index, range).expect("a listed range");
    }
    controller.set_param(module, index, value).expect("parameter exists");
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

/// A display value read during a render: `(frame, value)` pairs in frame
/// order, one for each buffer.
pub type Trace = Vec<(usize, f32)>;

/// The index of a module's display value, by name.
pub fn display_index(parts: &EngineParts, module: ModuleId, name: &str) -> usize {
    let node = parts.controller.graph().node(module).expect("module exists");
    node.descriptor
        .display_index(name)
        .unwrap_or_else(|| panic!("{} has no display value {name}", node.descriptor.id))
}

/// Renders `frames` frames in host buffers of `block` frames, as
/// [`render`], and reads display value `name` of `module` before each
/// buffer, as the app's light watcher would.
pub fn watch(
    parts: &mut EngineParts,
    frames: usize,
    block: usize,
    notes: &[Note],
    module: ModuleId,
    name: &str,
) -> Trace {
    let index = display_index(parts, module, name);
    let mut trace = Trace::new();
    render_with(parts, frames, block, notes, |position, parts| {
        let value = parts.controller.display(module, index).expect("display value exists");
        trace.push((position, value));
    });
    trace
}

/// Renders as [`render`] and collects every meter reading. The queue is
/// drained before each buffer, as the app's telemetry thread would, so it
/// never fills, even at a block size of 1.
pub fn meter_readings(parts: &mut EngineParts, frames: usize, block: usize, notes: &[Note]) -> Vec<MeterReading> {
    let mut readings = Vec::new();
    render_with(parts, frames, block, notes, |_, parts| {
        readings.extend(parts.telemetry.meters());
    });
    readings.extend(parts.telemetry.meters());
    readings
}

/// The last value read at or before `seconds`.
pub fn at(trace: &Trace, seconds: f32) -> f32 {
    assert!(!trace.is_empty(), "empty trace");
    let frame = (seconds * SAMPLE_RATE) as usize;
    let i = trace.partition_point(|&(f, _)| f <= frame);
    trace[i.saturating_sub(1)].1
}

/// The largest value read between `from` and `to` seconds.
pub fn max_between(trace: &Trace, from: f32, to: f32) -> f32 {
    let (from, to) = ((from * SAMPLE_RATE) as usize, (to * SAMPLE_RATE) as usize);
    trace
        .iter()
        .filter(|(f, _)| (from..to).contains(f))
        .map(|&(_, v)| v)
        .fold(0.0, f32::max)
}

/// Peak and RMS of a probe's meter readings for the blocks that lie wholly
/// between `from` and `to` seconds.
pub fn level_between(readings: &[MeterReading], probe: ProbeId, from: f32, to: f32) -> (f32, f32) {
    let (from, to) = ((from * SAMPLE_RATE) as u64, (to * SAMPLE_RATE) as u64);
    let mut peak = 0.0f32;
    let (mut squares, mut frames) = (0.0f64, 0u64);
    for r in readings
        .iter()
        .filter(|r| r.probe == probe && r.frame >= from && r.frame + u64::from(r.frames) <= to)
    {
        peak = peak.max(r.peak);
        squares += f64::from(r.rms).powi(2) * f64::from(r.frames);
        frames += u64::from(r.frames);
    }
    assert!(frames > 0, "no meter readings for {probe:?} in {from}..{to}");
    (peak, (squares / frames as f64).sqrt() as f32)
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
