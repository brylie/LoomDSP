//! What the app tells the player: the status line, and audio load and
//! dropout warnings for the stats bar (FR-PERF-04,
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

/// What the app tells the player: the status line, the dropout and input
/// drift warnings in the stats bar, and problems reported once. The app
/// owns one and reaches it through these methods.
#[derive(Debug, Default)]
pub struct Alerts {
    /// The status line: the result of the last action, or a problem.
    status: String,
    /// When the dropout count last grew.
    dropouts: Dropouts,
    /// Input drift counts (underruns, overruns) while recently active.
    input_drift: Option<(u64, u64)>,
    /// True while the live-value table is full and that was reported.
    live_values_full: bool,
    /// True once a recording that stopped early was reported.
    recording_failed: bool,
}

impl Alerts {
    /// Starts with `status` in the status line.
    pub fn new(status: String) -> Self {
        Self {
            status,
            ..Self::default()
        }
    }

    /// The status line.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Replaces the status line.
    pub fn say(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    /// Adds `more` to the end of the status line.
    pub fn append(&mut self, more: &str) {
        self.status.push_str(more);
    }

    /// Takes the status line, leaving it empty.
    pub fn take_status(&mut self) -> String {
        std::mem::take(&mut self.status)
    }

    /// Takes the latest dropout count.
    pub fn note_dropouts(&mut self, count: u64, now: Instant) {
        self.dropouts.update(count, now);
    }

    /// The latest dropout count.
    pub fn dropout_count(&self) -> u64 {
        self.dropouts.count()
    }

    /// Red for [`DROPOUT_ALARM`] after the dropout count grows.
    pub fn dropout_severity(&self, now: Instant) -> Severity {
        self.dropouts.severity(now)
    }

    /// The input drift counts to show, if any.
    pub fn input_drift(&self) -> Option<(u64, u64)> {
        self.input_drift
    }

    /// Shows or hides the input drift warning.
    pub fn set_input_drift(&mut self, drift: Option<(u64, u64)>) {
        self.input_drift = drift;
    }

    /// Notes whether the live-value table is full after a compile. Returns
    /// true only on entering the full state, so it is reported once, and
    /// again after the patch has gone back below the limit.
    pub fn live_values_full(&mut self, full: bool) -> bool {
        let entered = full && !self.live_values_full;
        self.live_values_full = full;
        entered
    }

    /// Returns true the first time a stopped recording is reported, and
    /// false after.
    pub fn recording_stopped(&mut self) -> bool {
        !std::mem::replace(&mut self.recording_failed, true)
    }
}

#[cfg(test)]
mod tests;
