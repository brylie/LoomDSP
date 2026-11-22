//! The app's clock (#868).
//!
//! The window reads the system clock. The test harness (`ui_test.rs`) gives
//! the app a stepped clock that moves only when the harness moves it: a
//! little on each step, and by a chosen amount on `Clock::advance`. So a
//! test of a double-click, an undo group, or a hover delay gives the same
//! result however busy the machine is (AGENTS.md, hard rule 3).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Where the app, its bridge, and the canvas read the time.
#[derive(Debug, Clone, Default)]
pub(crate) struct Clock {
    /// The stepped clock, or `None` for the system clock.
    stepped: Option<Arc<Stepped>>,
}

/// A clock that moves only when told: its start, and how far it has moved.
#[derive(Debug)]
struct Stepped {
    /// The time it started at.
    start: Instant,
    /// Nanoseconds since `start`.
    elapsed: AtomicU64,
}

impl Clock {
    /// A stepped clock that starts at `start` and stays there until moved.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn stepped(start: Instant) -> Self {
        Self {
            stepped: Some(Arc::new(Stepped {
                start,
                elapsed: AtomicU64::new(0),
            })),
        }
    }

    /// The time now: the system's, or where the stepped clock stands.
    pub(crate) fn now(&self) -> Instant {
        match &self.stepped {
            None => Instant::now(),
            Some(clock) => clock.start + Duration::from_nanos(clock.elapsed.load(Ordering::Relaxed)),
        }
    }

    /// Moves a stepped clock on by `by`. The system clock moves by itself,
    /// so this does nothing to it.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn advance(&self, by: Duration) {
        if let Some(clock) = &self.stepped {
            let nanos = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
            clock.elapsed.fetch_add(nanos, Ordering::Relaxed);
        }
    }

    /// Moves a stepped clock on to `at`, if `at` is later. A clock never
    /// goes back.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn advance_to(&self, at: Instant) {
        let now = self.now();
        if at > now {
            self.advance(at - now);
        }
    }
}

impl loom_canvas::Clock for Clock {
    fn now(&self) -> Instant {
        Clock::now(self)
    }
}

#[cfg(test)]
mod tests;
