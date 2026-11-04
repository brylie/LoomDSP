//! A level history on the telemetry thread: the RMS and peak of one meter
//! probe, one column per 1/100 of a time window (`docs/design/panel-widgets.md`,
//! Level history).
//!
//! Unlike a meter, a column holds the true level of its frames, with no fall
//! over a second. A tone that stops scrolls away, so the history reaches the
//! floor after one window.

use crate::live::normalise;
use loom_core::MeterReading;
use std::collections::VecDeque;

/// Columns a history draws.
pub const COLUMNS: usize = 100;

/// The frames of one column.
#[derive(Debug, Clone, Copy, Default)]
struct Column {
    /// Sum of squares of the left channel, or of a mono signal.
    left: f64,
    /// Sum of squares of the right channel.
    right: f64,
    /// Frames counted.
    frames: u64,
    /// Highest peak of either side.
    peak: f32,
}

impl Column {
    /// The higher side's RMS, and the peak, each from 0 to 1 (-60 dB to 0 dB).
    fn levels(&self) -> (f32, f32) {
        if self.frames == 0 {
            return (0.0, 0.0);
        }
        let rms = (self.left.max(self.right) / self.frames as f64).sqrt() as f32;
        let rms = normalise(rms);
        (rms, normalise(self.peak).max(rms))
    }
}

/// The last window of one meter probe. Lives on the telemetry thread.
#[derive(Debug)]
pub struct LevelHistory {
    window: f32,
    column_frames: u64,
    /// The finished columns, oldest first, then the one still filling.
    columns: VecDeque<Column>,
    /// The number of the column still filling, counted in engine frames.
    current: Option<u64>,
}

impl LevelHistory {
    /// An empty history of `window` seconds at this sample rate.
    pub fn new(window: f32, sample_rate: f32) -> Self {
        let frames = (window * sample_rate / COLUMNS as f32).round() as u64;
        Self {
            window,
            column_frames: frames.max(1),
            columns: VecDeque::with_capacity(COLUMNS + 1),
            current: None,
        }
    }

    /// The seconds this history shows.
    pub fn window(&self) -> f32 {
        self.window
    }

    /// Files one reading by its frame stamp. Columns no reading reached stay
    /// at the floor, and a reading that is not finite counts for nothing.
    pub fn push(&mut self, reading: &MeterReading) {
        let number = reading.frame / self.column_frames;
        match self.current {
            Some(current) if number < current => return,
            Some(current) => {
                // Far past the window, every column is empty anyway.
                let empty = (number - current).min(COLUMNS as u64 + 1);
                self.columns.extend((0..empty).map(|_| Column::default()));
            }
            None => self.columns.push_back(Column::default()),
        }
        self.current = Some(number);
        let excess = self.columns.len().saturating_sub(COLUMNS + 1);
        self.columns.drain(..excess);
        let right = reading.right.unwrap_or((0.0, 0.0));
        if ![reading.peak, reading.rms, right.0, right.1]
            .iter()
            .all(|x| x.is_finite())
        {
            return;
        }
        if let Some(column) = self.columns.back_mut() {
            let frames = f64::from(reading.frames);
            column.left += f64::from(reading.rms).powi(2) * frames;
            column.right += f64::from(right.1).powi(2) * frames;
            column.frames += u64::from(reading.frames);
            column.peak = column.peak.max(reading.peak).max(right.0);
        }
    }

    /// What the canvas draws: the RMS of each finished column in `points`
    /// and the peak in `second`, newest at the right. The column still filling
    /// is left out, because its level swings with how much of a cycle it has
    /// seen so far. A history at the floor is empty, so it draws nothing.
    pub fn trace(&self) -> loom_canvas::Trace {
        let finished = self.columns.len().saturating_sub(1);
        let levels: Vec<(f32, f32)> = self.columns.iter().take(finished).map(Column::levels).collect();
        if levels.iter().all(|&(rms, peak)| rms <= 0.0 && peak <= 0.0) {
            return loom_canvas::Trace::default();
        }
        let start = COLUMNS - finished;
        let at = |i: usize, level: f32| [(start + i) as f32 / COLUMNS as f32, 2.0 * level - 1.0];
        loom_canvas::Trace {
            points: levels.iter().enumerate().map(|(i, l)| at(i, l.0)).collect(),
            second: levels.iter().enumerate().map(|(i, l)| at(i, l.1)).collect(),
            level: 0.0,
        }
    }
}

#[cfg(test)]
mod tests;
