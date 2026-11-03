//! Filters. Each cutoff uses [`prewarp`], so it lands in the right place at any
//! sample rate.
//!
//! ```
//! use loom_dsp::filter::prewarp;
//!
//! // A quarter of the sample rate warps to exactly 1.
//! assert!((prewarp(12_000.0, 48_000.0) - 1.0).abs() < 1e-6);
//! ```

pub mod crossover;
pub mod dc_blocker;
pub mod eq;
pub mod ladder;
pub mod one_pole;
pub mod svf;

use std::f32::consts::PI;

/// The integrator gain for a cutoff under the bilinear transform. Warping the
/// cutoff with `tan` puts it exactly where it should be at any sample rate.
///
/// `cutoff_hz` must be above 0 and below half of `sample_rate`.
#[inline]
pub fn prewarp(cutoff_hz: f32, sample_rate: f32) -> f32 {
    debug_assert!(
        cutoff_hz > 0.0 && cutoff_hz < sample_rate / 2.0,
        "cutoff {cutoff_hz} Hz at {sample_rate} Hz"
    );
    (PI * cutoff_hz / sample_rate).tan()
}
