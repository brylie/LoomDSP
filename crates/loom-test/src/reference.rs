//! Reference files: 32-bit float WAV, one channel per output.

use crate::Render;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::path::Path;

/// Set this environment variable to `1` to write references instead of
/// comparing with them.
pub const UPDATE_VAR: &str = "LOOM_UPDATE_REFERENCES";

/// True when the tests should write references.
pub(crate) fn update_requested() -> bool {
    std::env::var(UPDATE_VAR).is_ok_and(|value| value == "1")
}

/// Writes `render` to `path` as a reference, creating its folder.
#[track_caller]
pub(crate) fn write(render: &Render, path: &Path) {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    }
    let spec = WavSpec {
        channels: u16::try_from(render.outputs.len()).expect("at most 65 535 outputs"),
        sample_rate: wav_rate(render.sample_rate),
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer =
        WavWriter::create(path, spec).unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    for frame in 0..render.frames() {
        for output in &render.outputs {
            writer.write_sample(output[frame]).expect("write a sample");
        }
    }
    writer
        .finalize()
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
}

/// Compares `render` with the reference at `path`. Panics with what differs.
#[track_caller]
pub(crate) fn compare(render: &Render, path: &Path, tolerance: f32) {
    let id = render.descriptor.id;
    if !path.exists() {
        panic!(
            "{id}: no reference file at {}.\n\
             Create it with `{UPDATE_VAR}=1 cargo test`, listen to it, and commit it.",
            path.display()
        );
    }
    let mut reader = WavReader::open(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let spec = reader.spec();
    let expected_spec = (render.outputs.len(), wav_rate(render.sample_rate), render.frames());
    let found_spec = (usize::from(spec.channels), spec.sample_rate, reader.duration() as usize);
    if spec.sample_format != SampleFormat::Float || spec.bits_per_sample != 32 {
        panic!("{id}: {} is not a 32-bit float WAV file", path.display());
    }
    if found_spec != expected_spec {
        panic!(
            "{id}: {} has {} channels at {} Hz, {} frames long; the render has {} outputs at {} Hz, {} frames long.\n\
             If the change is intended, update it with `{UPDATE_VAR}=1 cargo test`.",
            path.display(),
            found_spec.0,
            found_spec.1,
            found_spec.2,
            expected_spec.0,
            expected_spec.1,
            expected_spec.2,
        );
    }
    let samples: Vec<f32> = reader
        .samples::<f32>()
        .collect::<Result<_, _>>()
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let channels = render.outputs.len();
    for (port, actual) in render.outputs.iter().enumerate() {
        let expected = samples.iter().skip(port).step_by(channels);
        let mut first = None;
        let mut largest = 0.0f32;
        for (frame, (&a, &e)) in actual.iter().zip(expected).enumerate() {
            let difference = (a - e).abs();
            // A NaN in the file gives a NaN difference.
            if (difference.is_nan() || difference > tolerance) && first.is_none() {
                first = Some((frame, e, a));
            }
            largest = largest.max(difference);
        }
        if let Some((frame, expected, actual)) = first {
            panic!(
                "{id}: output `{}` differs from {} at frame {frame}: expected {expected}, got {actual}. \
                 Largest difference {largest}, tolerance {tolerance}.\n\
                 If the change is intended, update it with `{UPDATE_VAR}=1 cargo test`, listen to it, and commit it.",
                render.descriptor.outputs[port].name,
                path.display(),
            );
        }
    }
}

#[track_caller]
fn wav_rate(sample_rate: f32) -> u32 {
    assert!(
        sample_rate.fract() == 0.0 && sample_rate >= 1.0,
        "a reference needs a whole-number sample rate, not {sample_rate}"
    );
    sample_rate as u32
}
