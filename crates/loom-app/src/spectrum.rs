//! A spectrum on the telemetry thread: the frequency content of one scope
//! probe (`docs/design/panel-widgets.md`, Spectrum, and
//! `docs/learn/reading-a-spectrum.md`).
//!
//! The FFT runs here and never on the audio thread. Its plan and buffers are
//! made once, when the analyser starts, so each analysis allocates nothing.

use loom_core::ScopeChunk;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Arc;

/// Samples in one analysis: about 12 Hz per bin at 48 kHz.
const SIZE: usize = 4096;
/// Samples between analyses: half overlap, because a Hann window hides what
/// lies at its edges, and the next analysis sees it in its middle.
const HOP: usize = SIZE / 2;
/// Columns on the line, whatever the tile's width, so a resize needs no new
/// analysis.
pub const COLUMNS: usize = 256;
/// The lowest and highest frequency drawn.
const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 20_000.0;
/// The bottom of the frame, in dB. The top is 0 dB.
const FLOOR_DB: f32 = -90.0;
/// How fast the line and the hold fall after a peak.
const FALL_DB_PER_SECOND: f32 = 20.0;
/// How long the hold line stays at a peak before it falls.
const HOLD_SECONDS: f32 = 2.0;
/// The tilt's slope, around [`TILT_PIVOT_HZ`].
const TILT_DB_PER_OCTAVE: f32 = 4.5;
const TILT_PIVOT_HZ: f32 = 1000.0;

/// Where a column reads its level.
#[derive(Debug, Clone)]
enum Reads {
    /// The highest of these bins.
    Bins(Range<usize>),
    /// A column narrower than a bin holds none. It is interpolated in dB
    /// between this bin and the next, so the line stays continuous.
    Between(usize, f32),
}

/// One column on the line.
#[derive(Debug, Clone)]
struct Column {
    /// The column's middle in Hz, for the tilt.
    hz: f32,
    reads: Reads,
}

/// The analysis of one scope probe. Lives on the telemetry thread.
pub struct SpectrumAnalyser {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// The level of each bin, in dB, in the last analysis.
    bins: Vec<f32>,
    columns: Vec<Column>,
    /// Samples waiting for an analysis.
    samples: VecDeque<f32>,
    /// Engine frame of the next expected sample.
    next: Option<u64>,
    /// Seconds between analyses.
    step: f32,
    /// The line, in dB, per column.
    line: Vec<f32>,
    /// The hold line, in dB, per column.
    hold: Vec<f32>,
    /// Seconds each column's hold still stays.
    hold_left: Vec<f32>,
}

impl SpectrumAnalyser {
    /// An analyser at this sample rate. Plans the FFT, so make one per
    /// spectrum on screen and keep it.
    pub fn new(sample_rate: f32) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(SIZE);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let columns = columns(sample_rate);
        let count = columns.len();
        Self {
            fft,
            // A periodic Hann window, so overlapping windows add up to a
            // constant.
            window: (0..SIZE)
                .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / SIZE as f32).cos())
                .collect(),
            buffer: vec![Complex::default(); SIZE],
            scratch,
            bins: vec![FLOOR_DB; SIZE / 2 + 1],
            columns,
            samples: VecDeque::with_capacity(SIZE + HOP),
            next: None,
            step: HOP as f32 / sample_rate.max(1.0),
            line: vec![FLOOR_DB; count],
            hold: vec![FLOOR_DB; count],
            hold_left: vec![0.0; count],
        }
    }

    /// Adds a chunk from the probe, and runs each analysis it completes. If
    /// chunks were lost in between, the waiting samples are dropped: an
    /// analysis across a gap would show frequencies that are not in the signal.
    pub fn push(&mut self, chunk: &ScopeChunk) {
        if self.next.is_some_and(|next| next != chunk.frame) {
            self.samples.clear();
        }
        self.next = Some(chunk.frame + u64::from(chunk.len));
        for &sample in chunk.samples() {
            // One sample that is not finite would spoil a whole analysis.
            self.samples.push_back(if sample.is_finite() { sample } else { 0.0 });
            if self.samples.len() == SIZE {
                self.analyse();
                self.samples.drain(..HOP);
            }
        }
    }

    /// One analysis of the waiting samples: the level of every bin, then the
    /// line and the hold.
    fn analyse(&mut self) {
        for ((out, &sample), &weight) in self.buffer.iter_mut().zip(&self.samples).zip(&self.window) {
            *out = Complex::new(sample * weight, 0.0);
        }
        self.fft.process_with_scratch(&mut self.buffer, &mut self.scratch);
        // A sine of amplitude A gives a bin of A * N / 2 with no window. The
        // Hann window averages 0.5, so N / 4. This makes a full-scale sine
        // read 0 dB.
        let scale = 4.0 / SIZE as f32;
        for (level, bin) in self.bins.iter_mut().zip(&self.buffer) {
            *level = (20.0 * (bin.norm() * scale).max(1e-9).log10()).max(FLOOR_DB);
        }
        let fall = FALL_DB_PER_SECOND * self.step;
        for (index, column) in self.columns.iter().enumerate() {
            let level = match column.reads {
                Reads::Bins(ref bins) => self.bins[bins.clone()].iter().copied().fold(FLOOR_DB, f32::max),
                Reads::Between(bin, along) => self.bins[bin] + (self.bins[bin + 1] - self.bins[bin]) * along,
            };
            self.line[index] = level.max(self.line[index] - fall).max(FLOOR_DB);
            if level >= self.hold[index] {
                self.hold[index] = level;
                self.hold_left[index] = HOLD_SECONDS;
            } else if self.hold_left[index] > 0.0 {
                self.hold_left[index] -= self.step;
            } else {
                self.hold[index] = (self.hold[index] - fall).max(FLOOR_DB);
            }
        }
    }

    /// What the canvas draws: the line in `points`, and the hold in `second`
    /// when asked. A spectrum at the floor is empty, so it draws nothing.
    pub fn trace(&self, tilt: bool, hold: bool) -> loom_canvas::Trace {
        let silent = |line: &[f32]| line.iter().all(|&db| db <= FLOOR_DB);
        if silent(&self.line) && (!hold || silent(&self.hold)) {
            return loom_canvas::Trace::default();
        }
        let drawn = |line: &[f32]| {
            line.iter()
                .zip(&self.columns)
                .enumerate()
                .map(|(index, (&db, column))| {
                    let tilted = if tilt && db > FLOOR_DB {
                        db + TILT_DB_PER_OCTAVE * (column.hz / TILT_PIVOT_HZ).log2()
                    } else {
                        db
                    };
                    let up = ((tilted - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
                    [(index as f32 + 0.5) / COLUMNS as f32, 2.0 * up - 1.0]
                })
                .collect()
        };
        loom_canvas::Trace {
            points: drawn(&self.line),
            second: if hold { drawn(&self.hold) } else { Vec::new() },
            level: 0.0,
        }
    }
}

/// The columns at a sample rate, spaced evenly in log frequency, up to half
/// the sample rate, where the line stops.
fn columns(sample_rate: f32) -> Vec<Column> {
    let sample_rate = sample_rate.max(1.0);
    let nyquist = sample_rate / 2.0;
    let per_bin = sample_rate / SIZE as f32;
    let edge = |index: usize| LOW_HZ * (HIGH_HZ / LOW_HZ).powf(index as f32 / COLUMNS as f32);
    (0..COLUMNS)
        .take_while(|&index| edge(index) < nyquist)
        .map(|index| {
            let (low, high) = (edge(index), edge(index + 1));
            let hz = (low * high).sqrt();
            let first = (low / per_bin).ceil() as usize;
            let end = ((high / per_bin).ceil() as usize).min(SIZE / 2 + 1);
            let reads = if first < end {
                Reads::Bins(first..end)
            } else {
                let at = (hz / per_bin).min((SIZE / 2 - 1) as f32);
                Reads::Between(at.floor() as usize, at.fract())
            };
            Column { hz, reads }
        })
        .collect()
}

#[cfg(test)]
mod tests;
