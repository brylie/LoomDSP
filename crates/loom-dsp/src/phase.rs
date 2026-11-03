//! A phase generator: a ramp from 0 to 1 that drives the shapes in
//! [`crate::oscillator`].
//!
//! Keeping time apart from wave shape lets one ramp drive any shape, and lets a
//! caller bend time before it becomes sound. The phase is an `f64`, because
//! adding a small `f32` step to a phase near 1 would round, and the pitch would
//! drift. This is plain accumulation, from first principles.
//!
//! See `docs/learn/phase-and-cycles.md`.
//!
//! ```
//! use loom_dsp::oscillator::sine;
//! use loom_dsp::phase::Phasor;
//!
//! let step = Phasor::step_for(440.0, 48_000.0);
//! let mut phasor = Phasor::new();
//! let first = sine(phasor.tick(step));
//! assert_eq!(first, 0.0);
//! assert!(phasor.phase() > 0.0);
//! assert_eq!(loom_dsp::phase::wrap(1.25), 0.25);
//! ```

/// A value as a place in the cycle: `x - floor(x)`, from 0 up to but not
/// including 1. NaN and infinity give NaN, so a caller that keeps the result in
/// its state must check first. An oscillator shape need not: it keeps no value
/// past the next good phase.
///
/// A tiny negative value rounds up to exactly 1, which is not a place in the
/// cycle, so that gives 0. The two are the same point of the cycle.
#[inline]
pub fn wrap(x: f32) -> f32 {
    let wrapped = x - x.floor();
    if wrapped >= 1.0 { 0.0 } else { wrapped }
}

/// The largest phase step per sample: just under half a cycle, the Nyquist
/// frequency. A frequency above it cannot be heard as itself anyway.
///
/// Not exactly 0.5: the phase would then read 0, 0.5, 0, 0.5, and no step would
/// be more than half a cycle, so nothing watching the ramp could tell a wrap
/// from a step. At 0.49 every wrap is a jump of 0.51, clear of `f32` rounding.
pub const MAX_STEP: f64 = 0.49;

/// A running phase, from 0 up to but not including 1.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Phasor {
    phase: f64,
}

impl Phasor {
    /// Creates a phasor at phase 0.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// The phase step per sample for a frequency, from `-MAX_STEP` to
    /// [`MAX_STEP`]. A negative frequency runs the phase backward.
    ///
    /// The limit also keeps a bad value out of the state: `inf - floor(inf)` is
    /// NaN, and the phase would then stay NaN for good. NaN gives a step of 0,
    /// so the phase stops.
    #[inline]
    #[allow(clippy::manual_clamp)] // See the comment in the body.
    pub fn step_for(hz: f32, sample_rate: f32) -> f64 {
        let step = f64::from(hz) / f64::from(sample_rate);
        if step.is_nan() {
            0.0
        } else {
            // `min` and `max`, not `clamp`: this runs per sample in a feedback
            // loop, and `clamp` measured 13% slower on the whole of
            // `fm-feedback.ron`.
            step.min(MAX_STEP).max(-MAX_STEP)
        }
    }

    /// The phase now.
    #[inline]
    pub fn phase(&self) -> f64 {
        self.phase
    }

    /// Moves the phase on by `step`, wrapping it to 0 to 1. `step` must be
    /// finite, which any value from [`Phasor::step_for`] is.
    #[inline]
    pub fn advance(&mut self, step: f64) {
        debug_assert!(step.is_finite(), "step {step}");
        self.phase += step;
        self.phase -= self.phase.floor();
    }

    /// Back to phase 0.
    #[inline]
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Returns the phase now, then moves on by `step`. So the first sample
    /// after a reset is exactly 0.
    #[inline]
    pub fn tick(&mut self, step: f64) -> f32 {
        let phase = self.phase as f32;
        self.advance(step);
        phase
    }
}

#[cfg(test)]
mod tests;
