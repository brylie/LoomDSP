//! The audio output stream as the app sees it: the stream that plays the
//! engine, or the idle engine when none does, its callback counters and
//! load, how this run plays (a device, a timer, or not at all), and the
//! output safety mode. Specs: `docs/design/device-changes.md`,
//! `docs/design/output-safety.md`.
//!
//! The app owns one [`Stream`] and reaches it through its methods.
//! Building engines, choosing devices, and the status bar stay with the
//! app and its `settings::Rig`.

use loom_core::{Engine, OutputSafety};
use loom_host_desktop::{CallbackStats, RunningAudio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long callbacks may stop before the stream counts as stalled.
pub const STALL: Duration = Duration::from_secs(1);

/// How this run plays, from the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    /// On an audio device.
    Device,
    /// From a timer, with no device (`--fake-audio`).
    Fake,
    /// Not at all (`--no-audio`): the engine stays idle.
    Off,
}

impl Playback {
    /// The playback for the `--no-audio` and `--fake-audio` flags.
    pub fn from_flags(no_audio: bool, fake_audio: bool) -> Self {
        match (no_audio, fake_audio) {
            (true, _) => Self::Off,
            (false, true) => Self::Fake,
            (false, false) => Self::Device,
        }
    }

    /// True for `--no-audio`.
    pub fn is_off(self) -> bool {
        self == Self::Off
    }

    /// True for `--fake-audio`.
    pub fn is_fake(self) -> bool {
        self == Self::Fake
    }
}

/// The output stream, or the idle engine, and what the app knows about it.
pub struct Stream {
    /// The stream that plays the engine.
    running: Option<RunningAudio>,
    /// The engine, when no stream plays it. The app never renders with it:
    /// each update only applies the queued edits, so the queue never fills.
    /// Tests run it by hand.
    idle: Option<Engine>,
    /// The playing stream's callback counters.
    stats: Option<Arc<CallbackStats>>,
    /// Audio load from the watcher's last tick, latest and highest.
    load: (f64, f64),
    /// Callbacks seen so far, and when the count last grew, for the stall
    /// check.
    callbacks: (u64, Instant),
    /// What the stats bar says plays: `<device> at <rate> Hz`, or why
    /// nothing does.
    label: String,
    /// The rate of the engine, and of the device that plays it.
    sample_rate: f32,
    playback: Playback,
    /// The safety stage's mode, for every engine the app builds.
    output_safety: OutputSafety,
}

impl Stream {
    /// The stream for a new engine: `running` if a device started, or the
    /// idle `engine` if not.
    pub fn new(
        running: Option<RunningAudio>,
        idle: Option<Engine>,
        label: String,
        sample_rate: f32,
        playback: Playback,
        output_safety: OutputSafety,
    ) -> Self {
        Self {
            stats: running.as_ref().map(|audio| audio.stats().clone()),
            running,
            idle,
            load: (0.0, 0.0),
            callbacks: (0, Instant::now()),
            label,
            sample_rate,
            playback,
            output_safety,
        }
    }

    /// True while a stream plays the engine.
    pub fn is_playing(&self) -> bool {
        self.running.is_some()
    }

    /// The playing stream's callback counters.
    pub fn stats(&self) -> Option<&Arc<CallbackStats>> {
        self.stats.as_ref()
    }

    /// Audio load, latest and highest, from the watcher's last tick.
    pub fn load(&self) -> (f64, f64) {
        self.load
    }

    /// Takes the load the watcher measured.
    pub fn set_load(&mut self, latest: f64, highest: f64) {
        self.load = (latest, highest);
    }

    /// What the stats bar says plays.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The engine's sample rate.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// How this run plays.
    pub fn playback(&self) -> Playback {
        self.playback
    }

    /// True when this run never uses an audio device: `--no-audio` or
    /// `--fake-audio`.
    pub fn without_device(&self) -> bool {
        self.playback != Playback::Device
    }

    /// The output safety mode.
    pub fn output_safety(&self) -> OutputSafety {
        self.output_safety
    }

    /// Sets the output safety mode. Returns false if it was already set.
    pub fn set_output_safety(&mut self, mode: OutputSafety) -> bool {
        let changed = mode != self.output_safety;
        self.output_safety = mode;
        changed
    }

    /// The idle engine, if no stream plays.
    pub fn idle_mut(&mut self) -> Option<&mut Engine> {
        self.idle.as_mut()
    }

    /// True when the engine runs somewhere: on a stream, or idle.
    pub fn has_engine(&self) -> bool {
        self.running.is_some() || self.idle.is_some()
    }

    /// Keeps `engine` idle, as when no stream can play it.
    pub fn keep_idle(&mut self, engine: Engine) {
        self.idle = Some(engine);
    }

    /// Readies for a new engine at `sample_rate`: the old idle engine goes.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.idle = None;
    }

    /// A stream started: it plays the engine, and its counters start over.
    pub fn started(&mut self, audio: RunningAudio, label: String) {
        self.stats = Some(audio.stats().clone());
        self.load = (0.0, 0.0);
        self.running = Some(audio);
        self.label = label;
        self.callbacks = (0, Instant::now());
    }

    /// A stream refused to start: `engine` stays idle, and the label says why.
    pub fn refused(&mut self, engine: Engine, reason: &str) {
        self.label = format!("no audio ({reason})");
        self.idle = Some(engine);
    }

    /// Takes the playing stream to stop it. Its counters and load go.
    pub fn take_running(&mut self) -> Option<RunningAudio> {
        self.stats = None;
        self.load = (0.0, 0.0);
        self.running.take()
    }

    /// Notes the callback count read at `now`. Returns true if it grew.
    pub fn observe_callbacks(&mut self, count: u64, now: Instant) -> bool {
        if count > self.callbacks.0 {
            self.callbacks = (count, now);
            true
        } else {
            false
        }
    }

    /// Takes a count from the watcher thread, which saw it grow at
    /// `advanced`, so a stall is timed from there.
    pub fn watcher_saw(&mut self, count: u64, advanced: Instant) {
        if count >= self.callbacks.0 {
            self.callbacks = (count, self.callbacks.1.max(advanced));
        }
    }

    /// True when callbacks have not grown for longer than [`STALL`] at `now`.
    pub fn stalled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.callbacks.1) > STALL
    }

    /// True when the engine is idle, for tests.
    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.idle.is_some()
    }

    /// Callbacks seen, and when the count last grew, for tests.
    #[cfg(test)]
    pub fn callbacks(&self) -> (u64, Instant) {
        self.callbacks
    }

    /// Puts a stream in place, or none, for tests that start one by hand.
    #[cfg(test)]
    pub fn set_running(&mut self, running: Option<RunningAudio>) {
        self.stats = running.as_ref().map(|audio| audio.stats().clone());
        self.running = running;
    }

    /// Sets how this run plays, for tests.
    #[cfg(test)]
    pub fn set_playback(&mut self, playback: Playback) {
        self.playback = playback;
    }
}

#[cfg(test)]
mod tests;
