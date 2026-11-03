#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations)]
#![deny(
    clippy::missing_inline_in_public_items,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::dbg_macro,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod delay;
pub mod dynamics;
pub mod envelope;
pub mod euclid;
pub mod filter;
pub mod limiter;
pub mod loudness;
pub mod noise;
pub mod oscillator;
pub mod phase;
pub mod random;
pub mod slew;

mod static_checks;
/// The most a signal can be when it enters filters in series: 300 dB, far above
/// any real signal. Cables are never clipped (ADR-0007), and a filter's gain
/// could take a larger value past the range of an `f32`.
pub const MOST_INPUT: f32 = 1e15;

/// Makes a sample from a cable safe for an algorithm's state. NaN and infinity
/// become 0, and a huge value is held at [`MOST_INPUT`]. Either would stay in a
/// filter's memory after the signal returns to normal.
#[inline]
pub fn guard(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(-MOST_INPUT, MOST_INPUT)
    } else {
        0.0
    }
}

#[cfg(test)]
mod testing;
