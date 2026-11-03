//! A crossover: two or three frequency bands that sum to a flat level.
//!
//! Each split is two second-order Butterworth low-passes in series, and two
//! high-passes in series. The two sum to an all-pass, with the same level as
//! the input at every frequency and a rotated phase, so the sum does not null
//! against the input. A steep split cannot rebuild the waveform with no latency.
//!
//! Three bands need one step more. The mid and high bands have been through the
//! upper split, so the low band passes through that split's all-pass as well.
//!
//! Source: S. H. Linkwitz, "Active Crossover Networks for Noncoincident
//! Drivers", JAES 24, 1976.
//!
//! ```
//! use loom_dsp::filter::crossover::{Coefficients, Crossover};
//!
//! let coefficients = Coefficients::new(200.0, 2_000.0, 48_000.0);
//! let mut crossover = Crossover::new();
//! let bands = crossover.tick(0.5, coefficients);
//! assert!((bands.low + bands.mid + bands.high).is_finite());
//! ```

use super::svf::{self, Svf};

/// The damping of a second-order Butterworth.
const DAMPING: f32 = std::f32::consts::SQRT_2;

/// A crossover's settings for one sample. Computing them costs two tangents, so
/// a caller with fixed splits computes them once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    low_split: svf::Coefficients,
    high_split: svf::Coefficients,
}

impl Coefficients {
    /// Computes the settings for two splits. If `high_split_hz` is below
    /// `low_split_hz`, both use `low_split_hz`, so the bands never swap. Both
    /// must be above 0 and below half of `sample_rate`.
    #[inline]
    pub fn new(low_split_hz: f32, high_split_hz: f32, sample_rate: f32) -> Self {
        Self {
            low_split: svf::Coefficients::new(low_split_hz, DAMPING, sample_rate),
            high_split: svf::Coefficients::new(high_split_hz.max(low_split_hz), DAMPING, sample_rate),
        }
    }
}

/// One split: two low-passes in series and two high-passes in series.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Split {
    low: [Svf; 2],
    high: [Svf; 2],
}

impl Split {
    /// Returns the low and high parts.
    #[inline]
    fn tick(&mut self, input: f32, coefficients: svf::Coefficients) -> (f32, f32) {
        let low = self.low[0].tick(input, coefficients).low;
        let high = self.high[0].tick(input, coefficients).high;
        (
            self.low[1].tick(low, coefficients).low,
            self.high[1].tick(high, coefficients).high,
        )
    }
}

/// All the bands for one sample. A caller uses `low`, `mid`, and `high` for
/// three bands, or `lower` and `upper` for two, and can blend between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bands {
    /// Three bands: below the lower split, aligned with the others.
    pub low: f32,
    /// Three bands: between the splits.
    pub mid: f32,
    /// Three bands: above the upper split.
    pub high: f32,
    /// Two bands: below the lower split.
    pub lower: f32,
    /// Two bands: above the lower split.
    pub upper: f32,
}

/// The running state of one crossover. All its filters always run, so a change
/// finds them settled.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Crossover {
    low_split: Split,
    high_split: Split,
    align: Svf,
}

impl Crossover {
    /// Creates a crossover with cleared state.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the crossover's memory.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Processes one sample. `input` must be finite.
    #[inline]
    pub fn tick(&mut self, input: f32, coefficients: Coefficients) -> Bands {
        let (lower, upper) = self.low_split.tick(input, coefficients.low_split);
        let (mid, high) = self.high_split.tick(upper, coefficients.high_split);
        // The sum of a split's two parts is `x - 2 k band`, one filter's work.
        let low = lower - 2.0 * DAMPING * self.align.tick(lower, coefficients.high_split).band;
        Bands {
            low,
            mid,
            high,
            lower,
            upper,
        }
    }
}

#[cfg(test)]
mod tests;
