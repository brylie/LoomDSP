//! What the app tells the player: the status line, and audio load and
//! dropout warnings for the stats bar (FR-PERF-04,
//! `docs/design/panels.md`, Audio load and dropouts).

use serde::Serialize;
use std::time::{Duration, Instant};

/// How long the dropout count stays red after it grows.
pub const DROPOUT_ALARM: Duration = Duration::from_secs(10);

/// How loudly the status bar shows a number or a message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Severity {
    /// Plain. A message with this severity is info.
    #[default]
    #[serde(rename = "info")]
    Normal,
    /// Amber.
    #[serde(rename = "warning")]
    Warning,
    /// Red. A message with this severity is an error.
    #[serde(rename = "error")]
    Alarm,
}

/// What a kept error is about, which decides what clears it
/// (`docs/design/visual-system.md`, Status messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    /// The output device is lost. Clears when sound plays again or the
    /// player chooses a device, and has no dismiss button.
    AudioOutput,
    /// The input device failed. Clears when the stream opens, the player
    /// chooses an input, or an input is no longer wanted.
    AudioInput,
    /// A file could not be read or written. Clears after a later successful
    /// save, open, or new patch.
    File,
    /// Anything else that lost the player's work.
    Other,
}

/// A problem that stays on screen after newer messages arrive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeptError {
    /// What it is about.
    pub cause: Cause,
    /// The message, without its `Error:` label.
    pub text: String,
    /// Whether the bar offers a × for it.
    pub dismissible: bool,
}

/// The text the bar shows for a message: its severity in words as well as in
/// colour (NFR-UX-03).
pub fn labelled(severity: Severity, text: &str) -> String {
    match severity {
        Severity::Normal => text.to_string(),
        Severity::Warning => format!("Warning: {text}"),
        Severity::Alarm => format!("Error: {text}"),
    }
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

/// Remembers when a count last grew: the dropouts, and the output samples
/// silenced or clipped.
#[derive(Debug, Default)]
pub struct GrowthAlarm {
    count: u64,
    grew: Option<Instant>,
}

impl GrowthAlarm {
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
    /// How the status line reads: info, a warning, or an error.
    severity: Severity,
    /// The problems kept on screen, one per cause, oldest first.
    kept: Vec<KeptError>,
    /// When the dropout count last grew.
    dropouts: GrowthAlarm,
    /// Input drift counts (underruns, overruns) while recently active.
    input_drift: Option<(u64, u64)>,
    /// The stats bar says the output silenced bad samples. The audio watcher
    /// owns the timing, so this holds no clock of its own.
    output_silenced: bool,
    /// The stats bar says the output clipped.
    output_clipped: bool,
    /// True while the live-value table is full and that was reported.
    live_values_full: bool,
    /// True once a recording that stopped early was reported.
    recording_failed: bool,
}

impl Alerts {
    /// Starts with `notes` as a warning, or with nothing if it is empty.
    pub fn with_notes(notes: String) -> Self {
        let mut alerts = Self::default();
        if !notes.is_empty() {
            alerts.warn(notes);
        }
        alerts
    }

    /// The status line.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// How the status line reads.
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// Replaces the status line with info.
    pub fn say(&mut self, status: impl Into<String>) {
        self.status = status.into();
        self.severity = Severity::Normal;
    }

    /// Replaces the status line with a warning: a refused gesture, or a
    /// problem that fixed itself. It is not kept.
    pub fn warn(&mut self, status: impl Into<String>) {
        self.status = status.into();
        self.severity = Severity::Warning;
    }

    /// Replaces the status line with an error, and keeps it on screen until
    /// its cause is gone. An error of the same cause gives way to it.
    pub fn error(&mut self, cause: Cause, status: impl Into<String>) {
        self.status = status.into();
        self.severity = Severity::Alarm;
        self.kept.retain(|kept| kept.cause != cause);
        self.kept.push(KeptError {
            cause,
            text: self.status.clone(),
            dismissible: cause != Cause::AudioOutput,
        });
    }

    /// The cause of a kept error is gone. Returns whether one was kept. If
    /// it was also the latest message, the status line empties, so it does
    /// not go on saying `Error:`.
    pub fn clear(&mut self, cause: Cause) -> bool {
        let Some(at) = self.kept.iter().position(|kept| kept.cause == cause) else {
            return false;
        };
        let kept = self.kept.remove(at);
        if self.severity == Severity::Alarm && kept.text == self.status {
            self.say(String::new());
        }
        true
    }

    /// The player pressed × on a kept error. A lost output has no ×.
    pub fn dismiss(&mut self, cause: Cause) {
        if cause != Cause::AudioOutput {
            self.clear(cause);
        }
    }

    /// The problems kept on screen, oldest first.
    pub fn kept(&self) -> &[KeptError] {
        &self.kept
    }

    /// The newest kept error, if any.
    pub fn newest_kept(&self) -> Option<&KeptError> {
        self.kept.last()
    }

    /// The error for the bar's own field: the newest kept one, unless the
    /// status line already says it, so the same text never shows twice.
    pub fn kept_field(&self) -> Option<&KeptError> {
        self.newest_kept()
            .filter(|kept| self.severity != Severity::Alarm || kept.text != self.status)
    }

    /// All the kept errors, newest first, for hover text.
    pub fn kept_hover(&self) -> String {
        self.kept
            .iter()
            .rev()
            .map(|kept| labelled(Severity::Alarm, &kept.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Takes over the kept errors of the app this one replaces: opening a
    /// patch does not make a lost device come back. An error this app already
    /// raised is newer, so it wins over an old one of the same cause.
    pub fn carry_kept_from(&mut self, old: &mut Self) {
        let mut carried = std::mem::take(&mut old.kept);
        carried.retain(|kept| !self.kept.iter().any(|fresh| fresh.cause == kept.cause));
        carried.append(&mut self.kept);
        self.kept = carried;
    }

    /// Adds `more` to the end of the status line. If the line is a kept
    /// error, the kept text grows with it, so the two still match.
    pub fn append(&mut self, more: &str) {
        if self.severity == Severity::Alarm
            && let Some(kept) = self.kept.iter_mut().find(|kept| kept.text == self.status)
        {
            kept.text.push_str(more);
        }
        self.status.push_str(more);
    }

    /// Adds `more` to the end of the status line, and makes the line at
    /// least a warning.
    pub fn append_warning(&mut self, more: &str) {
        self.append(more);
        self.severity = self.severity.max(Severity::Warning);
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

    /// Whether the stats bar says the output silenced bad samples.
    pub fn output_silenced(&self) -> bool {
        self.output_silenced
    }

    /// Whether the stats bar says the output clipped.
    pub fn output_clipped(&self) -> bool {
        self.output_clipped
    }

    /// Takes what the audio watcher last said about the output.
    pub fn set_output_warnings(&mut self, silenced: bool, clipped: bool) {
        self.output_silenced = silenced;
        self.output_clipped = clipped;
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
