//! Equaliser sections built on the state variable filter: a bell, two shelves,
//! and two cuts.
//!
//! Each section is a mix of the filter's outputs, so it keeps the filter's
//! stability when its settings move at audio rate. At a gain of 0 dB, the mix
//! leaves only the input.
//!
//! Sources: R. Bristow-Johnson, "Audio EQ Cookbook" (the analogue responses),
//! and V. Zavalishin, "The Art of VA Filter Design" (the filter).
//!
//! ```
//! use loom_dsp::filter::eq::{Coefficients, Section, Shape};
//!
//! let shape = Shape::Bell { hz: 1_000.0, gain_db: 6.0, q: 1.0 };
//! let coefficients = Coefficients::new(shape, 48_000.0);
//! let mut section = Section::new();
//! assert!(section.tick(0.5, coefficients).is_finite());
//! assert!((shape.analogue_gain_db(1_000.0) - 6.0).abs() < 1e-3);
//! ```

use super::prewarp;
use super::svf::{self, Svf};

/// The Q of a section with no peak or dip: a second-order Butterworth.
pub const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// The two Qs of a fourth-order Butterworth, as two sections in series.
const FOURTH_ORDER_Q: [f32; 2] = [0.541_196_1, 1.306_563];

/// What one section does. `q` must be above 0, and `hz` above 0 and below half
/// of the sample rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Removes what is below `hz`, at 12 dB per octave.
    LowCut {
        /// The corner in hertz.
        hz: f32,
        /// How sharp the corner is.
        q: f32,
    },
    /// Removes what is above `hz`, at 12 dB per octave.
    HighCut {
        /// The corner in hertz.
        hz: f32,
        /// How sharp the corner is.
        q: f32,
    },
    /// Changes what is below `hz`. It reaches half of `gain_db` at `hz`.
    LowShelf {
        /// The middle of the slope in hertz.
        hz: f32,
        /// The gain far below `hz`.
        gain_db: f32,
        /// How sharp the slope is.
        q: f32,
    },
    /// Changes what is above `hz`. It reaches half of `gain_db` at `hz`.
    HighShelf {
        /// The middle of the slope in hertz.
        hz: f32,
        /// The gain far above `hz`.
        gain_db: f32,
        /// How sharp the slope is.
        q: f32,
    },
    /// A bump or dip centred on `hz`.
    Bell {
        /// The centre in hertz.
        hz: f32,
        /// The gain at `hz`.
        gain_db: f32,
        /// How narrow it is.
        q: f32,
    },
}

/// The mix of filter outputs for a shape, in terms of the analogue filter
/// `s^2 + damping s + 1`.
#[derive(Debug, Clone, Copy)]
struct Mix {
    /// The cutoff is multiplied by this after the warp.
    stretch: f32,
    damping: f32,
    dry: f32,
    band: f32,
    low: f32,
}

impl Shape {
    /// The shape's corner frequency.
    #[inline]
    fn hz(self) -> f32 {
        match self {
            Self::LowCut { hz, .. }
            | Self::HighCut { hz, .. }
            | Self::LowShelf { hz, .. }
            | Self::HighShelf { hz, .. }
            | Self::Bell { hz, .. } => hz,
        }
    }

    /// Works out the mix.
    #[inline]
    fn mix(self) -> Mix {
        // `a` squared is the gain as a factor.
        let amplitude = |gain_db: f32| 10.0f32.powf(gain_db / 40.0);
        match self {
            Self::LowCut { q, .. } => Mix {
                stretch: 1.0,
                damping: 1.0 / q,
                dry: 1.0,
                band: -1.0 / q,
                low: -1.0,
            },
            Self::HighCut { q, .. } => Mix {
                stretch: 1.0,
                damping: 1.0 / q,
                dry: 0.0,
                band: 0.0,
                low: 1.0,
            },
            Self::LowShelf { gain_db, q, .. } => {
                let a = amplitude(gain_db);
                Mix {
                    stretch: 1.0 / a.sqrt(),
                    damping: 1.0 / q,
                    dry: 1.0,
                    band: (a - 1.0) / q,
                    low: a * a - 1.0,
                }
            }
            Self::HighShelf { gain_db, q, .. } => {
                let a = amplitude(gain_db);
                Mix {
                    stretch: a.sqrt(),
                    damping: 1.0 / q,
                    dry: a * a,
                    band: a * (1.0 - a) / q,
                    low: 1.0 - a * a,
                }
            }
            Self::Bell { gain_db, q, .. } => {
                let a = amplitude(gain_db);
                let damping = 1.0 / (q * a);
                Mix {
                    stretch: 1.0,
                    damping,
                    dry: 1.0,
                    band: (a * a - 1.0) * damping,
                    low: 0.0,
                }
            }
        }
    }

    /// The analogue filter's gain at `hz`, in dB. It needs no sample rate, so
    /// a panel can draw it. The digital filter matches it, with the top of the
    /// range squeezed toward half of the sample rate.
    #[inline]
    pub fn analogue_gain_db(self, hz: f32) -> f32 {
        let mix = self.mix();
        let w = hz / (self.hz() * mix.stretch);
        let real = mix.dry * (1.0 - w * w) + mix.low;
        let imaginary = (mix.dry * mix.damping + mix.band) * w;
        let denominator = f32::hypot(1.0 - w * w, mix.damping * w);
        20.0 * (f32::hypot(real, imaginary) / denominator).max(1e-6).log10()
    }
}

/// A section's settings for one sample. Computing them costs a tangent and two
/// powers, so a caller with fixed settings computes them once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    filter: svf::Coefficients,
    dry: f32,
    band: f32,
    low: f32,
}

impl Coefficients {
    /// Computes the settings for a shape.
    #[inline]
    pub fn new(shape: Shape, sample_rate: f32) -> Self {
        let mix = shape.mix();
        Self {
            filter: svf::Coefficients::from_gain(prewarp(shape.hz(), sample_rate) * mix.stretch, mix.damping),
            dry: mix.dry,
            band: mix.band,
            low: mix.low,
        }
    }
}

/// The running state of one section.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Section {
    filter: Svf,
}

impl Section {
    /// Creates a section with cleared state.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the section's memory.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Processes one sample. `input` must be finite.
    #[inline]
    pub fn tick(&mut self, input: f32, coefficients: Coefficients) -> f32 {
        let outputs = self.filter.tick(input, coefficients.filter);
        coefficients.dry * input + coefficients.band * outputs.band + coefficients.low * outputs.low
    }
}

/// Which side of its frequency a [`Cut`] removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutSide {
    /// Removes what is below.
    Low,
    /// Removes what is above.
    High,
}

/// How steep a [`Cut`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutSlope {
    /// 12 dB per octave.
    Db12,
    /// 24 dB per octave.
    Db24,
}

impl CutSide {
    /// The shape of one stage at `hz`.
    #[inline]
    const fn shape(self, hz: f32, q: f32) -> Shape {
        match self {
            Self::Low => Shape::LowCut { hz, q },
            Self::High => Shape::HighCut { hz, q },
        }
    }

    /// The analogue gain of a cut at `at_hz`, in dB. Both slopes are 3 dB down
    /// at `hz`.
    #[inline]
    pub fn analogue_gain_db(self, hz: f32, slope: CutSlope, at_hz: f32) -> f32 {
        match slope {
            CutSlope::Db12 => self.shape(hz, BUTTERWORTH_Q).analogue_gain_db(at_hz),
            CutSlope::Db24 => FOURTH_ORDER_Q
                .iter()
                .map(|&q| self.shape(hz, q).analogue_gain_db(at_hz))
                .sum(),
        }
    }
}

/// A [`Cut`]'s settings for one sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutCoefficients {
    db12: Coefficients,
    db24: [Coefficients; 2],
}

impl CutCoefficients {
    /// Computes the settings for both slopes at once.
    #[inline]
    pub fn new(side: CutSide, hz: f32, sample_rate: f32) -> Self {
        let stage = |q| Coefficients::new(side.shape(hz, q), sample_rate);
        Self {
            db12: stage(BUTTERWORTH_Q),
            db24: FOURTH_ORDER_Q.map(stage),
        }
    }
}

/// Both slopes of a [`Cut`] for one sample, so a caller can blend between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutOutputs {
    /// The 12 dB per octave slope.
    pub db12: f32,
    /// The 24 dB per octave slope.
    pub db24: f32,
}

/// A cut that runs both slopes all the time, so a change of slope finds its
/// filters settled.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cut {
    db12: Section,
    db24: [Section; 2],
}

impl Cut {
    /// Creates a cut with cleared state.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the cut's memory.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Processes one sample. `input` must be finite.
    #[inline]
    pub fn tick(&mut self, input: f32, coefficients: CutCoefficients) -> CutOutputs {
        let first = self.db24[0].tick(input, coefficients.db24[0]);
        CutOutputs {
            db12: self.db12.tick(input, coefficients.db12),
            db24: self.db24[1].tick(first, coefficients.db24[1]),
        }
    }
}

#[cfg(test)]
mod tests;
