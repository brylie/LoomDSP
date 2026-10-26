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

/// Named channels of audio, with what a message should call them: a
/// module's outputs, or the two sides of a patch's output.
pub(crate) struct Channels<'a> {
    /// What rendered them, for messages: a module ID or a patch file name.
    pub id: &'a str,
    /// One name per channel, for messages.
    pub names: Vec<&'a str>,
    /// The samples, one list per channel, all the same length.
    pub samples: &'a [Vec<f32>],
    /// Sample rate in Hz.
    pub sample_rate: f32,
}

impl<'a> Channels<'a> {
    /// A module render's outputs, in descriptor order.
    pub(crate) fn of(render: &'a Render) -> Self {
        Self {
            id: render.descriptor.id,
            names: render.descriptor.outputs.iter().map(|o| o.name).collect(),
            samples: &render.outputs,
            sample_rate: render.sample_rate,
        }
    }

    fn frames(&self) -> usize {
        self.samples.first().map_or(0, Vec::len)
    }
}

/// Writes `channels` to `path` as a reference, creating its folder.
#[track_caller]
pub(crate) fn write(channels: &Channels<'_>, path: &Path) {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    }
    let spec = WavSpec {
        channels: u16::try_from(channels.samples.len()).expect("at most 65 535 outputs"),
        sample_rate: wav_rate(channels.sample_rate),
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer =
        WavWriter::create(path, spec).unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    for frame in 0..channels.frames() {
        for output in channels.samples {
            writer.write_sample(output[frame]).expect("write a sample");
        }
    }
    writer
        .finalize()
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
}

/// Compares `channels` with the reference at `path`. Panics with what
/// differs.
#[track_caller]
pub(crate) fn compare(channels: &Channels<'_>, path: &Path, tolerance: f32) {
    let id = channels.id;
    if !path.exists() {
        panic!(
            "{id}: no reference file at {}.\n\
             Create it with `{UPDATE_VAR}=1 cargo test`, listen to it, and commit it.",
            path.display()
        );
    }
    let mut reader = WavReader::open(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let spec = reader.spec();
    let expected_spec = (
        channels.samples.len(),
        wav_rate(channels.sample_rate),
        channels.frames(),
    );
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
    let count = channels.samples.len();
    for (port, actual) in channels.samples.iter().enumerate() {
        let expected = samples.iter().skip(port).step_by(count);
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
                channels.names[port],
                path.display(),
            );
        }
    }
}

/// Compares channels of audio with the reference file at `path`, or writes
/// the file when [`UPDATE_VAR`] is `1`. For renders that are not one
/// module, such as a whole patch through the engine (`#93`). `id` names
/// what rendered them, and `names` names each channel, for messages.
///
/// Panics, with what differs, when a sample is NaN or infinite, when a
/// channel differs by more than `tolerance` at any frame, or when the file is missing or has another
/// channel count, sample rate, or length.
#[track_caller]
pub fn check_channels(id: &str, names: &[&str], samples: &[Vec<f32>], sample_rate: f32, path: &Path, tolerance: f32) {
    assert_eq!(names.len(), samples.len(), "{id}: one name per channel");
    assert!(
        samples.iter().all(|c| c.len() == samples[0].len()),
        "{id}: every channel has the same length"
    );
    // A NaN or infinite sample fails every check, and is never written.
    for (name, channel) in names.iter().zip(samples) {
        if let Some((frame, sample)) = channel.iter().enumerate().find(|(_, s)| !s.is_finite()) {
            panic!("{id}: output `{name}` is {sample} at frame {frame}");
        }
    }
    let channels = Channels {
        id,
        names: names.to_vec(),
        samples,
        sample_rate,
    };
    if update_requested() {
        write(&channels, path);
    } else {
        compare(&channels, path, tolerance);
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
