//! Weighted loudness, and a gain that makes one signal as loud as another.
//!
//! The filter is the K-weighting of ITU-R BS.1770: a shelf for the effect of the
//! head, and a high-pass that discounts the lowest bass. The standard gives it
//! as coefficients at 48 kHz. Solving those for the analogue filter gives a
//! +4 dB shelf at 1500 Hz and a high-pass at 38 Hz, so it can be built at any
//! sample rate from the sections in [`crate::filter::eq`].
//!
//! ```
//! use loom_dsp::loudness::LevelMatch;
//!
//! let mut level_match = LevelMatch::new(48_000.0);
//! let gain = level_match.tick([0.5, 0.5], [0.25, 0.25]);
//! assert!(gain.is_finite());
//! ```

use crate::filter::eq::{Coefficients, Section, Shape};

/// The time constant of the power filter, from the standard's 400 ms block.
const WINDOW_SECONDS: f32 = 0.4;

/// The largest change of level, as a factor: 24 dB.
const MOST_GAIN: f32 = 15.848_932;

/// The largest sample the meters read, as a factor: 40 dB over full scale.
const MOST_MEASURED: f32 = 100.0;

/// Below this power, about -70 dB, the ratio of two powers means nothing.
const SILENCE: f32 = 1e-7;

/// One channel of the K-weighting filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KWeighting {
    shelf: Section,
    rumble: Section,
    shelf_coefficients: Coefficients,
    rumble_coefficients: Coefficients,
}

impl KWeighting {
    /// Creates a cleared filter. `sample_rate` must be above 3 kHz.
    #[inline]
    pub fn new(sample_rate: f32) -> Self {
        let shelf = Shape::HighShelf {
            hz: 1_500.312,
            gain_db: 3.999_84,
            q: 0.707_175,
        };
        let rumble = Shape::LowCut {
            hz: 38.135_47,
            q: 0.500_327,
        };
        Self {
            shelf: Section::new(),
            rumble: Section::new(),
            shelf_coefficients: Coefficients::new(shelf, sample_rate),
            rumble_coefficients: Coefficients::new(rumble, sample_rate),
        }
    }

    /// Clears the memory.
    #[inline]
    pub fn reset(&mut self) {
        self.shelf.reset();
        self.rumble.reset();
    }

    /// Processes one sample. `input` must be finite.
    #[inline]
    pub fn tick(&mut self, input: f32) -> f32 {
        let shelved = self.shelf.tick(input, self.shelf_coefficients);
        self.rumble.tick(shelved, self.rumble_coefficients)
    }
}

/// Weighted power of a stereo signal, over a sliding window.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Meter {
    filters: [KWeighting; 2],
    power: f32,
}

impl Meter {
    /// Moves on by one frame.
    #[inline]
    fn tick(&mut self, input: [f32; 2], pole: f32) -> f32 {
        // A huge sample would otherwise fill the slow power filter, and hold the
        // gain at its limit long after the signal returned to normal.
        let [left, right] = input.map(|x| x.clamp(-MOST_MEASURED, MOST_MEASURED));
        let left = self.filters[0].tick(left);
        let right = self.filters[1].tick(right);
        let power = left * left + right * right;
        self.power = power + pole * (self.power - power);
        // The power of a falling signal would become a denormal.
        if self.power < 1e-30 {
            self.power = 0.0;
        }
        self.power
    }

    /// Clears the memory.
    #[inline]
    fn reset(&mut self) {
        self.filters.iter_mut().for_each(KWeighting::reset);
        self.power = 0.0;
    }
}

/// Works out the gain that makes a processed signal as loud as the signal it
/// came from. It is slow on purpose. It compares settings, and does not
/// control a level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelMatch {
    pole: f32,
    reference: Meter,
    processed: Meter,
    gain: f32,
}

impl LevelMatch {
    /// Creates a matcher with a gain of 1.
    #[inline]
    pub fn new(sample_rate: f32) -> Self {
        let meter = Meter {
            filters: [KWeighting::new(sample_rate); 2],
            power: 0.0,
        };
        Self {
            pole: (-1.0 / (WINDOW_SECONDS * sample_rate)).exp(),
            reference: meter,
            processed: meter,
            gain: 1.0,
        }
    }

    /// Back to a gain of 1, with empty meters.
    #[inline]
    pub fn reset(&mut self) {
        self.reference.reset();
        self.processed.reset();
        self.gain = 1.0;
    }

    /// Takes one stereo frame of the signal to match, and of the processed
    /// signal before this gain. Returns the gain. All must be finite. In
    /// silence the gain holds, so the next note does not jump.
    #[inline]
    pub fn tick(&mut self, reference: [f32; 2], processed: [f32; 2]) -> f32 {
        let wanted = self.reference.tick(reference, self.pole);
        let got = self.processed.tick(processed, self.pole);
        if wanted >= SILENCE {
            self.gain = if got > 0.0 {
                (wanted / got).sqrt().clamp(1.0 / MOST_GAIN, MOST_GAIN)
            } else {
                MOST_GAIN
            };
        }
        self.gain
    }
}

#[cfg(test)]
mod tests;
