//! Wave shapes from a phase, usually from a [`crate::phase::Phasor`].
//!
//! The band-limited shapes come in two layers, because a band limit needs to
//! know the phase step, and there are two ways to know it:
//!
//! - **A function** such as [`saw`] takes the true step. A caller who knows its
//!   own rate uses this. A reset or a hard sync jump stays a jump, and does not
//!   read as a very high pitch. The step is a size, so for a phase running
//!   backward pass the absolute value of a negative step.
//! - **A struct** such as [`Saw`] works the step out from the last phase. This
//!   suits a phase that arrives from elsewhere, such as a cable, and a phase
//!   can be bent or modulated with nothing else to say how fast it moves.
//!
//! The corrections are polynomial. Background: V. Välimäki and A. Huovilainen,
//! "Antialiasing Oscillators in Subtractive Synthesis", IEEE Signal Processing
//! Magazine, 2007. See `docs/learn/phase-and-cycles.md`.
//!
//! ```
//! use loom_dsp::oscillator::{Saw, saw};
//! use loom_dsp::phase::Phasor;
//!
//! let step = Phasor::step_for(110.0, 48_000.0);
//! let (mut phasor, mut inferred) = (Phasor::new(), Saw::new());
//! // The first sample has no earlier phase to work the step out from.
//! inferred.tick(phasor.tick(step));
//! for _ in 0..100 {
//!     let phase = phasor.tick(step);
//!     let known = saw(phase, step as f32);
//!     assert!((known - inferred.tick(phase)).abs() < 1e-3);
//! }
//! ```

use crate::phase::wrap;
use std::f32::consts::TAU as TAU_F32;
use std::f64::consts::TAU;

/// A sine at `phase`, which wraps to 0 to 1. It has no harmonics, so it has
/// nothing to band limit and no state.
#[inline]
pub fn sine(phase: f32) -> f32 {
    (TAU_F32 * wrap(phase)).sin()
}

/// Polynomial band-limited step. Smooths the jump at the wrap point, which
/// removes most of the aliasing a naive saw produces.
#[inline]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

/// Polynomial band-limited ramp: the integral of a two-sample polynomial step.
/// Smooths a unit slope change at `t = 0`, rounding the sharp corners of a
/// triangle wave without a leaky integrator.
#[inline]
pub fn poly_blamp(t: f32, dt: f32) -> f32 {
    if t < dt {
        let u = 1.0 - t / dt;
        (1.0 / 6.0) * u * u * u
    } else if t > 1.0 - dt {
        let u = (t - 1.0) / dt + 1.0;
        (1.0 / 6.0) * u * u * u
    } else {
        0.0
    }
}

/// A band-limited saw at `t` from 0 to 1. `step` is the size of the phase step
/// per sample, positive in either direction. With a `step` outside 0 to 0.5
/// there is no band limiting.
#[inline]
pub fn saw(t: f32, step: f32) -> f32 {
    let mut y = 2.0 * t - 1.0;
    if step > 0.0 && step < 0.5 {
        y -= poly_blep(t, step);
    }
    y
}

/// A band-limited pulse at `t` from 0 to 1, with `width` between 0 and 1. `step`
/// is the size of the phase step at the rising edge, positive in either
/// direction. `step_fall` is the same at the falling edge, which differs while
/// the width moves.
#[inline]
pub fn pulse(t: f32, width: f32, step: f32, step_fall: f32) -> f32 {
    pulse_unclamped(t, width, step, step_fall).clamp(-1.0, 1.0)
}

/// [`pulse`] before the final limit to -1 to 1.
#[inline]
fn pulse_unclamped(t: f32, width: f32, step: f32, step_fall: f32) -> f32 {
    debug_assert!(width > 0.0 && width < 1.0, "width {width}");
    let t_fall = wrap(t - width);
    let mut y = if t < width { 1.0 } else { -1.0 };
    // When the pulse or gap is narrower than `step + step_fall`, the
    // two BLEP windows overlap; use a shared step so their slopes cancel.
    let (step, step_fall) = if width < step + step_fall || 1.0 - width < step + step_fall {
        let shared = step.max(step_fall);
        (shared, shared)
    } else {
        (step, step_fall)
    };
    if step > 0.0 && step < 0.5 {
        // Jump at the start of the cycle (+2 ascending, -2 descending).
        y += poly_blep(t, step);
    }
    // Jump when `t` crosses `width` (-2 ascending, +2 descending).
    if step_fall > 0.0 && step_fall < 0.5 {
        y -= poly_blep(t_fall, step_fall);
    }
    y
}

/// A band-limited triangle at `t` from 0 to 1, aligned with a sine: 0 at 0, +1
/// at 0.25, 0 at 0.5, and -1 at 0.75. `step` is the size of the phase step per
/// sample, positive in either direction.
#[inline]
pub fn triangle(t: f32, step: f32) -> f32 {
    let t_top = wrap(t - 0.25);
    let mut y = 4.0 * (t_top - 0.5).abs() - 1.0;
    if step > 0.0 && step < 0.5 {
        let t_bottom = wrap(t - 0.75);
        // Slope changes by -8*step at the top peak and +8*step at the bottom peak.
        y += 8.0 * step * (poly_blamp(t_bottom, step) - poly_blamp(t_top, step));
    }
    y
}

/// A band-limited saw that works out its phase step: see [`saw`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Saw {
    previous: f32,
}

impl Saw {
    /// Creates a saw at phase 0.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Back to phase 0.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The wave at `phase`, which wraps to 0 to 1.
    #[inline]
    pub fn tick(&mut self, phase: f32) -> f32 {
        let t = wrap(phase);
        // The phase step tells us the frequency, so the shaper does not
        // need a separate frequency input. A falling phase wraps to a step
        // near 1, so take the shorter way round the cycle.
        let step = wrap(t - self.previous);
        self.previous = t;
        saw(t, step.min(1.0 - step))
    }
}

/// A band-limited pulse that works out its phase steps: see [`pulse`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pulse {
    previous: f32,
    previous_fall: f32,
}

impl Default for Pulse {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Pulse {
    /// Creates a pulse at phase 0, as if the width had been 0.5.
    #[inline]
    pub fn new() -> Self {
        Self {
            previous: 0.0,
            previous_fall: 0.5,
        }
    }

    /// Back to phase 0 at `width`.
    #[inline]
    pub fn reset(&mut self, width: f32) {
        self.previous = 0.0;
        self.previous_fall = 1.0 - width;
    }

    /// The wave at `phase`, which wraps to 0 to 1. `width` must be above 0 and
    /// below 1.
    #[inline]
    pub fn tick(&mut self, phase: f32, width: f32) -> f32 {
        let t = wrap(phase);
        let dt = wrap(t - self.previous);
        self.previous = t;
        let t_fall = wrap(t - width);
        let dt_fall = wrap(t_fall - self.previous_fall);
        self.previous_fall = t_fall;
        pulse(t, width, dt.min(1.0 - dt), dt_fall.min(1.0 - dt_fall))
    }
}

/// A band-limited triangle that works out its phase step: see [`triangle`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Triangle {
    previous: f32,
}

impl Triangle {
    /// Creates a triangle at phase 0.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Back to phase 0.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The wave at `phase`, which wraps to 0 to 1.
    #[inline]
    pub fn tick(&mut self, phase: f32) -> f32 {
        let t = wrap(phase);
        let dt = wrap(t - self.previous);
        self.previous = t;
        triangle(t, dt.min(1.0 - dt))
    }
}

/// A wave with sharp corners and no band limiting, for slow modulation. Every
/// shape crosses 0 going up at phase 0, so a sine and a triangle line up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlowWave {
    /// A sine.
    Sine,
    /// A triangle.
    Triangle,
    /// A ramp that rises.
    RampUp,
    /// A ramp that falls.
    RampDown,
    /// A square.
    Square,
}

impl SlowWave {
    /// The wave at phase `p` in 0 to 1.
    #[inline]
    pub fn value(self, p: f64) -> f32 {
        debug_assert!((0.0..=1.0).contains(&p), "phase {p}");
        let fract = |x: f64| x - x.floor();
        let value = match self {
            Self::Sine => (TAU * p).sin(),
            Self::Triangle => 1.0 - 4.0 * (fract(p + 0.25) - 0.5).abs(),
            Self::RampUp => 2.0 * fract(p + 0.5) - 1.0,
            Self::RampDown => -(2.0 * fract(p + 0.5) - 1.0),
            Self::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        };
        value as f32
    }
}

#[cfg(test)]
mod tests;
