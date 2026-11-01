//! Measurements on recorded audio, for QA checks of what the app plays.
//!
//! The same functions serve the app tests (on the stepped output's tape)
//! and `loom analyze` (on a WAV file), so both measure in the same way.
//! Every measurement is plain arithmetic on the samples: no oversampling,
//! no filtering. Spec: `docs/design/audio-qa.md`, `loom analyze`.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Length of the windows the DC measurement averages over, in seconds.
pub const DC_WINDOW_SECONDS: f64 = 0.1;

/// Audio split into channels, at one sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    /// Samples per second.
    pub sample_rate: u32,
    /// One list of samples per channel, all the same length.
    pub channels: Vec<Vec<f32>>,
}

impl Audio {
    /// Frames in the audio: the length of each channel.
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
}

/// Why a WAV file could not be read. It names the file, and keeps the
/// decoder's error as its source.
#[derive(Debug)]
pub struct ReadError {
    /// The file that was read.
    pub path: PathBuf,
    /// What went wrong.
    pub kind: ReadErrorKind,
}

/// What went wrong while reading a WAV file.
#[derive(Debug)]
pub enum ReadErrorKind {
    /// The file could not be opened or decoded.
    Decode(hound::Error),
    /// A sample format other than 16-bit or 24-bit integer, or 32-bit float.
    Unsupported {
        /// Bits per sample.
        bits: u16,
        /// Integer or float.
        format: hound::SampleFormat,
    },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = self.path.display();
        match &self.kind {
            ReadErrorKind::Decode(e) => write!(f, "{path}: {e}"),
            ReadErrorKind::Unsupported { bits, format } => write!(
                f,
                "{path}: unsupported WAV format ({bits}-bit {format:?}); use 16-bit, 24-bit, or 32-bit float"
            ),
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ReadErrorKind::Decode(e) => Some(e),
            ReadErrorKind::Unsupported { .. } => None,
        }
    }
}

/// Reads a WAV file: 16-bit or 24-bit integer, or 32-bit float. Integer
/// samples are scaled to -1 to 1.
pub fn read_wav(path: &Path) -> Result<Audio, ReadError> {
    let error = |kind| ReadError {
        path: path.to_path_buf(),
        kind,
    };
    let mut reader = hound::WavReader::open(path).map_err(|e| error(ReadErrorKind::Decode(e)))?;
    let spec = reader.spec();
    let count = usize::from(spec.channels.max(1));
    let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| error(ReadErrorKind::Decode(e)))?,
        (hound::SampleFormat::Int, bits @ (16 | 24)) => {
            let scale = 1.0 / (1_i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(|e| error(ReadErrorKind::Decode(e)))?
        }
        (format, bits) => return Err(error(ReadErrorKind::Unsupported { bits, format })),
    };
    let mut channels = vec![Vec::with_capacity(samples.len() / count); count];
    for frame in samples.chunks_exact(count) {
        for (channel, &sample) in channels.iter_mut().zip(frame) {
            channel.push(sample);
        }
    }
    Ok(Audio {
        sample_rate: spec.sample_rate,
        channels,
    })
}

/// What was measured on a whole file or a part of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Analysis {
    /// Samples per second.
    pub sample_rate: u32,
    /// Where the measured part starts, in seconds from the start.
    pub from_seconds: f64,
    /// Frames measured.
    pub frames: usize,
    /// Length of the measured part, in seconds.
    pub seconds: f64,
    /// The output safety ceiling the `over_ceiling` counts compare with.
    pub ceiling: f32,
    /// One entry per channel.
    pub channels: Vec<ChannelAnalysis>,
}

/// What was measured on one channel. Times are in seconds from the start
/// of the file, not of the measured part.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelAnalysis {
    /// The first sample, so a fade-in from 0 can be seen.
    pub first: f32,
    /// Largest absolute finite sample.
    pub peak: f32,
    /// The peak in dBFS.
    pub peak_dbfs: f64,
    /// When the peak happens.
    pub peak_at: f64,
    /// Samples larger in size than the output safety ceiling.
    pub over_ceiling: usize,
    /// Samples of size 1.0 or more: what a hard clip leaves.
    pub full_scale: usize,
    /// NaN and infinite samples. Other measurements leave them out.
    pub non_finite: usize,
    /// Largest difference between two neighbouring finite samples.
    pub max_step: f32,
    /// When the second sample of the largest step happens.
    pub max_step_at: f64,
    /// The mean of all finite samples.
    pub dc: f64,
    /// The largest mean, in size, of any whole window of
    /// [`DC_WINDOW_SECONDS`]. The whole-part mean when it is shorter.
    pub dc_window: f64,
    /// Root mean square of the finite samples.
    pub rms: f64,
    /// The RMS in dBFS.
    pub rms_dbfs: f64,
}

/// A level in dBFS. Silence gives negative infinity, which JSON writes as
/// `null`.
fn dbfs(level: f64) -> f64 {
    20.0 * level.log10()
}

/// Measures the part of `audio` from `from` to `to` seconds (both
/// optional). A range outside the audio is clamped to it.
pub fn analyze(audio: &Audio, from: Option<f64>, to: Option<f64>) -> Analysis {
    let rate = f64::from(audio.sample_rate.max(1));
    let frames = audio.frames();
    let start = from.map_or(0, |s| ((s.max(0.0) * rate).round() as usize).min(frames));
    let end = to.map_or(frames, |s| ((s.max(0.0) * rate).round() as usize).clamp(start, frames));
    let window = ((DC_WINDOW_SECONDS * rate).round() as usize).max(1);
    let channels = audio
        .channels
        .iter()
        .map(|samples| channel(&samples[start..end], start, rate, window))
        .collect();
    Analysis {
        sample_rate: audio.sample_rate,
        from_seconds: start as f64 / rate,
        frames: end - start,
        seconds: (end - start) as f64 / rate,
        ceiling: loom_core::safety::CEILING,
        channels,
    }
}

/// Measures one channel's samples. `offset` is the frame index of the first
/// sample in the file, for the times.
fn channel(samples: &[f32], offset: usize, rate: f64, window: usize) -> ChannelAnalysis {
    let ceiling = loom_core::safety::CEILING;
    let at = |i: usize| (offset + i) as f64 / rate;
    let (mut peak, mut peak_index) = (0.0f32, 0);
    let (mut max_step, mut step_index) = (0.0f32, 0);
    let (mut over, mut full, mut bad) = (0, 0, 0);
    let (mut sum, mut squares, mut finite) = (0.0f64, 0.0f64, 0usize);
    let mut previous: Option<f32> = None;
    for (i, &x) in samples.iter().enumerate() {
        if !x.is_finite() {
            bad += 1;
            previous = None;
            continue;
        }
        let size = x.abs();
        if size > peak {
            peak = size;
            peak_index = i;
        }
        over += usize::from(size > ceiling);
        full += usize::from(size >= 1.0);
        sum += f64::from(x);
        squares += f64::from(x) * f64::from(x);
        finite += 1;
        if let Some(p) = previous {
            let step = (x - p).abs();
            if step > max_step {
                max_step = step;
                step_index = i;
            }
        }
        previous = Some(x);
    }
    let dc = if finite > 0 { sum / finite as f64 } else { 0.0 };
    let rms = if finite > 0 {
        (squares / finite as f64).sqrt()
    } else {
        0.0
    };
    ChannelAnalysis {
        first: samples.first().copied().unwrap_or(0.0),
        peak,
        peak_dbfs: dbfs(f64::from(peak)),
        peak_at: at(peak_index),
        over_ceiling: over,
        full_scale: full,
        non_finite: bad,
        max_step,
        max_step_at: at(step_index),
        dc,
        dc_window: window_dc(samples, window).unwrap_or(dc),
        rms,
        rms_dbfs: dbfs(rms),
    }
}

/// The largest mean, in size, of any `window` neighbouring samples, kept
/// with its sign. Non-finite samples count as 0. `None` when the samples
/// are fewer than one window.
fn window_dc(samples: &[f32], window: usize) -> Option<f64> {
    if samples.len() < window {
        return None;
    }
    let value = |x: f32| if x.is_finite() { f64::from(x) } else { 0.0 };
    let mut sum: f64 = samples[..window].iter().map(|&x| value(x)).sum();
    let mut best = sum;
    for i in window..samples.len() {
        sum += value(samples[i]) - value(samples[i - window]);
        if sum.abs() > best.abs() {
            best = sum;
        }
    }
    Some(best / window as f64)
}

/// The largest step between neighbouring samples in `samples`, or 0 when
/// there are fewer than two. For click checks on a tape. Infinity when any
/// sample is NaN or infinite, so a check never passes on broken audio
/// (`f32::max` alone would skip a NaN).
pub fn max_step(samples: &[f32]) -> f32 {
    if samples.iter().any(|x| !x.is_finite()) {
        return f32::INFINITY;
    }
    samples.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// The largest absolute sample in `samples`, or 0 when empty. Infinity when
/// any sample is NaN or infinite.
pub fn peak(samples: &[f32]) -> f32 {
    if samples.iter().any(|x| !x.is_finite()) {
        return f32::INFINITY;
    }
    samples.iter().map(|x| x.abs()).fold(0.0, f32::max)
}

#[cfg(test)]
mod tests;
