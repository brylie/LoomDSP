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
pub mod filter;
pub mod limiter;
pub mod loudness;
pub mod noise;
pub mod oscillator;
pub mod phase;
pub mod slew;

mod static_checks;
#[cfg(test)]
mod testing;
