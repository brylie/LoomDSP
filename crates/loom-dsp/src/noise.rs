//! Seeded white, pink, and brown noise.
//!
//! White noise comes from one xorshift stream. Pink and brown noise are built
//! from the same white samples, so every colour at one seed shares its white
//! stream. The pink rows draw from a separate stream, so they never move the
//! white one.
//!
//! Pink noise is the Voss-McCartney method (R. F. Voss and J. Clarke, "1/f
//! noise in music and speech", Nature 258, 1975; the row-update form is from
//! M. Gardner, "White and brown music, fractal curves and one-over-f
//! fluctuations", Scientific American, 1978). Brown noise is a leaky integral of white noise,
//! so it cannot drift away. xorshift is from G. Marsaglia, "Xorshift RNGs",
//! Journal of Statistical Software, 2003.
//!
//! ```
//! use loom_dsp::noise::{Colour, Noise};
//!
//! let mut noise = Noise::new(48_000.0);
//! noise.restart(42);
//! let first = noise.tick(Colour::Pink);
//! noise.restart(42);
//! assert_eq!(noise.tick(Colour::Pink), first);
//! ```

use crate::random::xorshift32;
use std::f32::consts::TAU;

/// Rows in the pink noise generator. Row `k` changes every `2^k` samples, so
/// 16 rows reach down to well under 1 Hz at audio rates.
const PINK_ROWS: usize = 16;

/// The corner of the brown noise leak, in Hz. Far below the audible slope, so
/// the -6 dB per octave fall holds above about 100 Hz.
const BROWN_LEAK_HZ: f32 = 10.0;

/// The colour of the noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    /// Equal power per hertz.
    White,
    /// Equal power per octave.
    Pink,
    /// Falls 6 dB per octave.
    Brown,
}

/// One xorshift32 step, as a value from -1 to 1.
///
/// xorshift32 is fast and deterministic, and good enough for audio noise.
#[inline]
pub fn next_white(state: &mut u32) -> f32 {
    (xorshift32(state) as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// A noise generator. The same seed gives the same numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Noise {
    state: u32,
    /// Stream for the pink rows.
    aux: u32,
    rows: [f32; PINK_ROWS],
    row_sum: f32,
    counter: u32,
    brown: f32,
    /// Brown leak coefficient, just under 1.
    leak: f32,
    /// Brown input gain that keeps the RMS equal to white noise.
    brown_gain: f32,
}

impl Noise {
    /// Creates a generator started from seed 1.
    #[inline]
    pub fn new(sample_rate: f32) -> Self {
        let leak = (-TAU * BROWN_LEAK_HZ / sample_rate).exp();
        let mut noise = Self {
            state: 1,
            aux: 1,
            rows: [0.0; PINK_ROWS],
            row_sum: 0.0,
            counter: 0,
            brown: 0.0,
            leak,
            // The leaky integral of unit-variance noise has variance
            // 1 / (1 - leak^2), so this gain restores a variance of 1.
            brown_gain: (1.0 - leak * leak).sqrt(),
        };
        noise.restart(1);
        noise
    }

    /// Starts every stream again from `seed`, and clears the brown memory.
    #[inline]
    pub fn restart(&mut self, seed: u32) {
        self.restart_keeping_brown(seed);
        self.brown = 0.0;
    }

    /// Starts the random streams again from `seed`, and keeps the brown
    /// memory, so a brown signal does not jump.
    #[inline]
    pub fn restart_keeping_brown(&mut self, seed: u32) {
        self.state = seed.max(1);
        self.aux = (seed ^ 0x9E37_79B9).max(1);
        for row in 0..PINK_ROWS {
            self.rows[row] = next_white(&mut self.aux);
        }
        self.row_sum = self.rows.iter().sum();
        self.counter = 0;
    }

    /// Takes the sample rate settings and the brown memory of another
    /// generator, and nothing from its random streams.
    #[inline]
    pub fn copy_brown_from(&mut self, other: &Self) {
        self.leak = other.leak;
        self.brown_gain = other.brown_gain;
        self.brown = other.brown;
    }

    /// The next sample of a colour. All colours advance the white stream, so
    /// a change of colour does not shift it.
    #[inline]
    pub fn tick(&mut self, colour: Colour) -> f32 {
        let white = next_white(&mut self.state);
        match colour {
            Colour::White => white,
            Colour::Pink => self.pink(white),
            Colour::Brown => {
                // Uniform noise has variance 1/3, so scaling to the same RMS
                // as white is the same gain.
                self.brown = self.leak * self.brown + self.brown_gain * white;
                self.brown
            }
        }
    }

    /// The next pink sample, given the next white one.
    #[inline]
    fn pink(&mut self, white: f32) -> f32 {
        // Row k changes when bit k is the lowest set bit of the counter, so
        // it changes every 2^k samples.
        self.counter = self.counter.wrapping_add(1);
        let row = self.counter.trailing_zeros() as usize;
        if row < PINK_ROWS {
            let value = next_white(&mut self.aux);
            self.row_sum += value - self.rows[row];
            self.rows[row] = value;
        }
        // The running sum drifts by rounding. Summing again every 65,536
        // samples keeps it exact enough, at no cost per sample.
        if self.counter & 0xFFFF == 0 {
            self.row_sum = self.rows.iter().sum();
        }
        // One white sample on top fills in the top of the spectrum. The sum
        // of PINK_ROWS + 1 independent values has that many times the
        // variance, so dividing by the square root restores white's RMS.
        (self.row_sum + white) / ((PINK_ROWS + 1) as f32).sqrt()
    }
}

#[cfg(test)]
mod tests;
