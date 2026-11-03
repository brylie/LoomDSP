//! Euclidean rhythms: a number of hits spread as evenly as possible over a
//! number of steps.
//!
//! G. Toussaint, "The Euclidean Algorithm Generates Traditional Musical
//! Rhythms", BRIDGES 2005, pages 47 to 56, builds these patterns with Euclid's
//! algorithm. Step `i` of the same spread is a hit exactly when
//! `(i * hits) mod steps < hits`: this closed form needs no memory or buffer,
//! gives the published patterns up to rotation, and fixes which rotation is 0,
//! so tests are exact.
//!
//! ```
//! use loom_dsp::euclid::Pattern;
//!
//! let pattern = Pattern::new(8, 3, 0);
//! let shown: String = (0..8).map(|step| if pattern.is_hit(step) { 'x' } else { '.' }).collect();
//! assert_eq!(shown, "x..x..x.");
//! ```

/// One Euclidean pattern. It is small and `Copy`, so a caller can keep the one
/// that applies to the current step while the settings move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pattern {
    steps: u32,
    hits: u32,
    rotation: u32,
}

impl Pattern {
    /// `hits` spread over `steps`, moved later by `rotation` steps. `steps` must
    /// be at least 1. `hits` above `steps` counts as `steps`, and `rotation` can
    /// be any size, because it goes round the pattern.
    #[inline]
    pub fn new(steps: u32, hits: u32, rotation: u32) -> Self {
        debug_assert!(steps >= 1, "a pattern needs a step");
        Self {
            steps,
            hits: hits.min(steps),
            rotation: rotation.rem_euclid(steps),
        }
    }

    /// The step of the unrotated pattern that plays at `step`.
    #[inline]
    fn source(&self, step: u32) -> u32 {
        (step + self.steps - self.rotation) % self.steps
    }

    /// True when `step`, from 0, is a hit. `step` goes round the pattern.
    #[inline]
    pub fn is_hit(&self, step: u32) -> bool {
        // Steps and hits are small in real use. A `u64` keeps a large pattern
        // from overflowing the product.
        u64::from(self.source(step % self.steps)) * u64::from(self.hits) % u64::from(self.steps) < u64::from(self.hits)
    }

    /// True when `step` holds the first hit of the unrotated pattern, which is
    /// where the pattern starts. False for a pattern with no hits.
    #[inline]
    pub fn is_first_hit(&self, step: u32) -> bool {
        self.hits > 0 && self.source(step % self.steps) == 0
    }
}

#[cfg(test)]
mod tests;
