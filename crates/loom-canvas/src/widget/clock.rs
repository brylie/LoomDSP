//! Where the widget reads the time of an event (#868).
//!
//! Iced gives a redraw its frame time, but no other event a time, so the
//! widget asks a clock. The window uses the system clock. The app's test
//! harness gives a clock it moves itself, so a double-click or a hover
//! delay in a test does not depend on how busy the machine is.

use std::time::Instant;

/// A source of the current time.
pub trait Clock {
    /// The time now.
    fn now(&self) -> Instant;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[cfg(test)]
mod tests;
