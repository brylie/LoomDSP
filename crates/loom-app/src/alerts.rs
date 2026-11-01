//! Audio load and dropout warnings for the stats bar (FR-PERF-04,
//! `docs/design/panels.md`, Audio load and dropouts).

use std::time::{Duration, Instant};

/// How long the dropout count stays red after it grows.
pub const DROPOUT_ALARM: Duration = Duration::from_secs(10);

/// How loudly the stats bar shows a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Normal,
    /// Amber.
    Warning,
    /// Red.
    Alarm,
}

/// Load below 70% is normal, from 70% amber, from 90% red. `load` is the
/// share of the callback's time budget, 1.0 = 100%.
pub fn load_severity(load: f64) -> Severity {
    if load >= 0.9 {
        Severity::Alarm
    } else if load >= 0.7 {
        Severity::Warning
    } else {
        Severity::Normal
    }
}

/// Remembers when the dropout count last grew.
#[derive(Debug, Default)]
pub struct Dropouts {
    count: u64,
    grew: Option<Instant>,
}

impl Dropouts {
    /// Takes the latest count.
    pub fn update(&mut self, count: u64, now: Instant) {
        if count > self.count {
            self.grew = Some(now);
        }
        self.count = count;
    }

    /// The latest count.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Red for [`DROPOUT_ALARM`] after the count grows.
    pub fn severity(&self, now: Instant) -> Severity {
        match self.grew {
            Some(at) if now - at < DROPOUT_ALARM => Severity::Alarm,
            _ => Severity::Normal,
        }
    }
}

#[cfg(test)]
mod tests;
