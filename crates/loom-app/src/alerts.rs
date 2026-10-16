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
mod tests {
    use super::*;

    #[test]
    fn load_thresholds() {
        assert_eq!(load_severity(0.0), Severity::Normal);
        assert_eq!(load_severity(0.69), Severity::Normal);
        assert_eq!(load_severity(0.7), Severity::Warning);
        assert_eq!(load_severity(0.89), Severity::Warning);
        assert_eq!(load_severity(0.9), Severity::Alarm);
        assert_eq!(load_severity(1.5), Severity::Alarm);
    }

    #[test]
    fn dropouts_are_red_for_ten_seconds_after_they_grow() {
        let start = Instant::now();
        let mut dropouts = Dropouts::default();
        dropouts.update(0, start);
        assert_eq!(dropouts.severity(start), Severity::Normal);
        dropouts.update(2, start);
        assert_eq!(dropouts.severity(start + Duration::from_secs(9)), Severity::Alarm);
        assert_eq!(dropouts.severity(start + Duration::from_secs(10)), Severity::Normal);
        // The same count again is not a new dropout.
        dropouts.update(2, start + Duration::from_secs(11));
        assert_eq!(dropouts.severity(start + Duration::from_secs(11)), Severity::Normal);
        assert_eq!(dropouts.count(), 2);
    }
}
