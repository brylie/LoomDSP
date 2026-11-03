//! A seeded random stream, for anything that must play the same on every load.
//!
//! The stream is xorshift32 (G. Marsaglia, "Xorshift RNGs", Journal of
//! Statistical Software 8(14), 2003). Xorshift is poor just after a small seed:
//! seeds 1 and 2 begin with small, similar values. So the start state goes
//! through the 32-bit finaliser of MurmurHash3 (A. Appleby, public domain),
//! which spreads similar words apart. Each input bit changes about half of the
//! output bits.
//!
//! ```
//! use loom_dsp::random::Rng;
//!
//! let (mut a, mut b) = (Rng::new(42), Rng::new(42));
//! let first = a.draw();
//! assert!((0.0..1.0).contains(&first));
//! assert_eq!(b.draw(), first);
//! ```

/// One xorshift32 step: moves `state` on and returns the new value. `state` must
/// not be 0, because 0 only ever gives 0.
#[inline]
pub fn xorshift32(state: &mut u32) -> u32 {
    debug_assert!(*state != 0, "xorshift32 stays at 0 forever");
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// A seeded stream of numbers from 0 up to but not including 1. The same word
/// gives the same numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(u32);

impl Rng {
    /// A stream started from `word`, which can be any value. Similar words give
    /// unrelated streams.
    #[inline]
    pub fn new(word: u32) -> Self {
        let mut x = word;
        x ^= x >> 16;
        x = x.wrapping_mul(0x85EB_CA6B);
        x ^= x >> 13;
        x = x.wrapping_mul(0xC2B2_AE35);
        x ^= x >> 16;
        // The finaliser maps exactly one word to 0, where xorshift would stay.
        Self(x.max(1))
    }

    /// The next value, from 0 up to but not including 1. Only the top 24 bits
    /// are used: an `f32` holds them exactly, so the result cannot round up to
    /// 1, and a chance of 1 always passes.
    #[inline]
    pub fn draw(&mut self) -> f32 {
        (xorshift32(&mut self.0) >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[cfg(test)]
mod tests;
