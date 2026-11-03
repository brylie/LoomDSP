//! State variable filter with trapezoidal integration.
//!
//! Each integrator uses the trapezoidal rule, so the loop between them has no
//! unit delay and is solved for its input each sample. The response is the
//! bilinear transform of the analogue filter, and the filter stays stable when
//! its cutoff moves at audio rate.
//!
//! Source: V. Zavalishin, "The Art of VA Filter Design".
//!
//! ```
//! use loom_dsp::filter::svf::{Coefficients, Svf};
//!
//! let coefficients = Coefficients::new(1_000.0, 1.0, 48_000.0);
//! let mut filter = Svf::new();
//! let outputs = filter.tick(0.5, coefficients);
//! assert!(outputs.low.is_finite());
//! ```

use super::prewarp;

/// The filter's settings for one sample. Computing them costs a tangent, so a
/// caller with a fixed cutoff computes them once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    damping: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Coefficients {
    /// Computes the settings for a cutoff and a damping.
    ///
    /// `cutoff_hz` must be above 0 and below half of `sample_rate`. `damping`
    /// must be above 0. It is `1 / Q`: 2 is a Q of 0.5, and small values ring.
    #[inline]
    pub fn new(cutoff_hz: f32, damping: f32, sample_rate: f32) -> Self {
        Self::from_gain(prewarp(cutoff_hz, sample_rate), damping)
    }

    /// Computes the settings from an integrator gain, as [`prewarp`] gives. A
    /// shelf needs this: it scales the gain after the warp, and scaling the
    /// cutoff first would warp to a different place.
    #[inline]
    pub fn from_gain(g: f32, damping: f32) -> Self {
        debug_assert!(damping > 0.0, "damping {damping}");
        let a1 = 1.0 / (1.0 + g * (g + damping));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { damping, a1, a2, a3 }
    }
}

/// All four responses for one sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outputs {
    /// Low-pass.
    pub low: f32,
    /// Band-pass, with a gain of `1 / damping` at the cutoff.
    pub band: f32,
    /// High-pass.
    pub high: f32,
    /// Notch: the low-pass plus the high-pass.
    pub notch: f32,
}

/// The running state of one filter.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// Creates a filter with cleared state.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the filter's memory.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Processes one sample. `input` must be finite: NaN or infinity would
    /// stay in the state until [`Svf::reset`].
    #[inline]
    pub fn tick(&mut self, input: f32, coefficients: Coefficients) -> Outputs {
        debug_assert!(input.is_finite(), "input {input}");
        let Coefficients { damping, a1, a2, a3 } = coefficients;
        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;

        let high = input - damping * v1 - v2;
        Outputs {
            low: v2,
            band: v1,
            high,
            // The band-pass terms cancel and `input - damping * v1` is left.
            // One addition, always in step.
            notch: v2 + high,
        }
    }
}

#[cfg(test)]
mod tests;
