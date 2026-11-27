//! Measurements on recorded audio, for QA checks of what the app plays.
//!
//! The same functions serve the app tests (on the stepped output's tape)
//! and `loom analyze` (on a WAV file), so both measure in the same way.
//! Every measurement is plain arithmetic on the samples, with no
//! oversampling. The only filter is the 1 Hz low-pass of the DC
//! measurement, two one-pole stages that can be checked by hand. Spec:
//! `docs/design/audio-qa.md`, `loom analyze` and Measuring DC.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Cut-off of each stage of the DC low-pass, in Hz. Far below any musical
/// tone, which the two stages cut by about (cut-off / f) squared.
pub const DC_CUTOFF_HZ: f64 = 1.0;

/// How long the DC low-pass settles before it is read, in seconds: about
/// six time constants of one stage, 1 / (2 pi [`DC_CUTOFF_HZ`]).
pub const DC_SETTLE_SECONDS: f64 = 1.0;

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
    /// The low-pass DC: the largest size of the signal after two one-pole
    /// low-pass stages at [`DC_CUTOFF_HZ`], kept with its sign, from
    /// [`DC_SETTLE_SECONDS`] into the part. `None` when the part is no
    /// longer than that, so there is nothing settled to read.
    pub dc_low_pass: Option<f64>,
    /// Root mean square of the finite samples.
    pub rms: f64,
    /// The RMS in dBFS.
    pub rms_dbfs: f64,
}

/// Measures the part of `audio` from `from` to `to` seconds (both
/// optional). A range outside the audio is clamped to it.
pub fn analyze(audio: &Audio, from: Option<f64>, to: Option<f64>) -> Analysis {
    let rate = f64::from(audio.sample_rate.max(1));
    let frames = audio.frames();
    let start = from.map_or(0, |s| ((s.max(0.0) * rate).round() as usize).min(frames));
    let end = to.map_or(frames, |s| ((s.max(0.0) * rate).round() as usize).clamp(start, frames));
    let channels = audio
        .channels
        .iter()
        .map(|samples| channel(&samples[start..end], start, rate))
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
fn channel(samples: &[f32], offset: usize, rate: f64) -> ChannelAnalysis {
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
        peak_dbfs: loom_dsp::decibels::gain_to_db_f64(f64::from(peak)),
        peak_at: at(peak_index),
        over_ceiling: over,
        full_scale: full,
        non_finite: bad,
        max_step,
        max_step_at: at(step_index),
        dc,
        dc_low_pass: low_pass_dc(samples, rate),
        rms,
        rms_dbfs: loom_dsp::decibels::gain_to_db_f64(rms),
    }
}

/// The low-pass DC of `samples` at `rate`: two one-pole low-pass stages at
/// [`DC_CUTOFF_HZ`], each `y += a (x - y)` with `a = 1 - exp(-2 pi fc / fs)`,
/// starting at 0. Returns the largest size of the second stage's output,
/// kept with its sign, from [`DC_SETTLE_SECONDS`] on. Non-finite samples
/// are read as 0, so one NaN cannot stay in the state. `None` when there
/// are no samples past the settling time.
///
/// Sources: J. O. Smith III, *Introduction to Digital Filters with Audio
/// Applications* (W3K, 2007), the one-pole low-pass `y += a (x - y)`; and
/// A. V. Oppenheim and R. W. Schafer, *Discrete-Time Signal Processing*,
/// impulse invariance, which gives the pole `exp(-2 pi fc / fs)` of an RC
/// low-pass with the same cut-off.
///
/// *Why not `loom_dsp::filter::one_pole`:* that is a trapezoidal (TPT)
/// filter in `f32`, a different form with a prewarped cut-off. This one is
/// the form the spec gives, so the reading can be checked by hand, and it
/// keeps its state in `f64`: `a` is about 1.3e-4 at 48 kHz, and an `f32`
/// state would lose the small steps it adds each sample.
fn low_pass_dc(samples: &[f32], rate: f64) -> Option<f64> {
    let settle = (DC_SETTLE_SECONDS * rate).round() as usize;
    if samples.len() <= settle {
        return None;
    }
    let a = 1.0 - (-2.0 * std::f64::consts::PI * DC_CUTOFF_HZ / rate).exp();
    let (mut first, mut second) = (0.0f64, 0.0f64);
    let mut largest = 0.0f64;
    for (i, &x) in samples.iter().enumerate() {
        let x = if x.is_finite() { f64::from(x) } else { 0.0 };
        first += a * (x - first);
        second += a * (first - second);
        if i >= settle && second.abs() > largest.abs() {
            largest = second;
        }
    }
    Some(largest)
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
