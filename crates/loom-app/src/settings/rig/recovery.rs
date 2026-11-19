//! Audio output loss and recovery after a device disappears or stalls.

use crate::preferences::DeviceChoice;
use crate::{App, alerts, devices};
use std::time::{Duration, Instant};

/// What was playing when an audio device was lost, and reconnection attempts
/// (device-changes.md, Audio device loss).
#[derive(Debug, Clone)]
pub struct LostAudio {
    /// The device name that was lost.
    pub device: String,
    /// What the app saw when it was lost, for the status bar.
    pub reason: String,
    /// Whether the device was chosen as SystemDefault.
    pub was_system_default: bool,
    /// Number of consecutive reopen attempts that failed (0..10).
    pub attempts: u32,
    /// When the last poll / reopen attempt occurred.
    pub last_attempt: std::time::Instant,
    /// The error message if the last reopen attempt failed.
    pub last_error: Option<String>,
}

impl App {
    /// Extracts the underlying reason from an audio restart error message for
    /// reporting in the status bar.
    pub(crate) fn unwrap_reopen_error(raw: &str) -> &str {
        let mut s = raw;
        if let Some(rest) = s.strip_prefix("Could not use \"")
            && let Some(idx) = rest.find("\": ")
        {
            s = &rest[idx + 3..];
        }
        let without_suffix = s.strip_suffix(". Audio is off.").unwrap_or(s);
        without_suffix.strip_suffix('.').unwrap_or(without_suffix)
    }

    /// Stops audio immediately without fading out. Used when the audio device
    /// was lost or disconnected (device-changes.md, Audio device loss).
    pub(crate) fn stop_audio_immediate(&mut self) {
        self.alerts.set_input_drift(None);
        // A new stream gets a new watcher, and a clean first read sends
        // nothing, so nothing else would turn these off.
        self.alerts.set_output_warnings(false, false);
        if let Some(audio) = self.session.engine.stream.take_running() {
            audio.stop_immediate();
        }
        self.rig.input.stream_stopped();
        self.rig.output.playing = None;
    }

    /// Handles loss of the active audio output (device-changes.md, Audio device loss).
    pub(crate) fn handle_audio_loss(&mut self, reason: &str) {
        // Rule 3: The engine is never rebuilt twice for one loss. A second error
        // from the same dead stream is ignored.
        if !self.session.engine.stream.is_playing() && self.rig.output.lost_audio.is_some() {
            return;
        }
        if !self.session.engine.stream.is_playing() && self.rig.output.playing.is_none() {
            return;
        }
        let was_system_default = matches!(self.rig.wanted().device, devices::Pick::SystemDefault);
        let old_name = self
            .rig
            .output
            .playing
            .as_ref()
            .map(|p| p.device.clone())
            .or_else(|| match &self.rig.store.saved().audio.device {
                DeviceChoice::Named(n) => Some(n.clone()),
                DeviceChoice::SystemDefault => None,
            })
            .unwrap_or_else(|| "audio device".into());

        self.stop_audio_immediate();
        self.session.engine.midi = None;
        self.keep_engine_idle();

        if was_system_default {
            // System default: at once, open the current system default
            let mut switched = false;
            if let Ok(outputs) = self.rig.host.outputs()
                && let Some(new_default) = outputs.default
            {
                let options = loom_host_desktop::AudioOptions {
                    device: Some(new_default.clone()),
                    exact: true,
                    sample_rate: None,
                    buffer_size: None,
                };
                if let Ok(output) = self.rig.host.open(&options)
                    && self.restart_audio(output).is_ok()
                {
                    self.audio_back();
                    self.alerts.warn(format!(
                        "Audio: \"{old_name}\" was lost: {reason}. Now playing on \"{new_default}\"."
                    ));
                    switched = true;
                }
            }
            if !switched {
                let message = format!("Audio: \"{old_name}\" was lost: {reason}.");
                self.audio_lost(
                    LostAudio {
                        device: old_name,
                        reason: reason.to_string(),
                        was_system_default: true,
                        attempts: 0,
                        last_attempt: Instant::now(),
                        last_error: None,
                    },
                    message,
                );
            }
        } else {
            // Named device: plays nothing, keeps engine idle
            let message = format!(
                "Audio: \"{old_name}\" was lost: {reason}. Plug it in again, or choose another device in Preferences."
            );
            self.audio_lost(
                LostAudio {
                    device: old_name,
                    reason: reason.to_string(),
                    was_system_default: false,
                    attempts: 0,
                    last_attempt: Instant::now(),
                    last_error: None,
                },
                message,
            );
        }
    }

    /// The output is lost: the app remembers it and keeps the message on
    /// screen. This and [`Self::audio_back`] are the only ways in and out, so
    /// the two cannot disagree. The retries in `check_audio` take the record
    /// out and put it back, which changes nothing the player sees.
    fn audio_lost(&mut self, lost: LostAudio, message: String) {
        self.alerts.error(alerts::Cause::AudioOutput, message);
        self.rig.output.lost_audio = Some(lost);
    }

    /// Sound plays again, or the player chose a device.
    pub(crate) fn audio_back(&mut self) {
        self.rig.output.lost_audio = None;
        self.alerts.clear(alerts::Cause::AudioOutput);
    }

    /// Checks for audio device loss, silent stalls, and device reconnection
    /// (device-changes.md, Audio device loss).
    pub(crate) fn poll_audio(&mut self) {
        self.check_audio(Instant::now());
        self.sync_audio_input();
    }

    /// Checks for audio device loss, silent stalls, and device reconnection at `now`.
    pub(crate) fn check_audio(&mut self, now: Instant) {
        if self.session.engine.stream.without_device() {
            return;
        }

        // 1. If audio is running, check for loss (dead stream, stall > 1s, missing device)
        if self.session.engine.stream.is_playing() {
            let mut callbacks_advancing = false;
            if let Some(stats) = self.session.engine.stream.stats().cloned() {
                let snap = stats.snapshot();
                if snap.is_dead {
                    self.handle_audio_loss("the audio device reported an error");
                    return;
                }
                if self.session.engine.stream.observe_callbacks(snap.callbacks, now) {
                    callbacks_advancing = true;
                } else if self.session.engine.stream.stalled(now) {
                    let mut rate_changed = false;
                    let device_default = matches!(
                        self.rig.store.saved().audio.sample_rate,
                        crate::preferences::RateChoice::DeviceDefault
                    ) && self.rig.flags.sample_rate.is_none();
                    let current = self.rig.output.playing.as_ref().map(|p| (p.device.clone(), p.rate));
                    if device_default
                        && let Some((device_name, current_rate)) = current
                        && let Ok(outputs) = self.rig.host.outputs()
                        && let Some(info) = outputs.devices.iter().find(|d| d.name == device_name)
                        && info.default_rate != current_rate
                    {
                        let new_rate = info.default_rate;
                        let mut choice = self.rig.store.saved().audio.clone();
                        choice.sample_rate = crate::preferences::RateChoice::Hz(new_rate);
                        if let Ok(output) = devices::open_choice(&*self.rig.host, &choice)
                            && self.restart_audio(output).is_ok()
                        {
                            self.alerts.say(format!(
                                "Audio: \"{device_name}\" sample rate changed to {new_rate} Hz."
                            ));
                            rate_changed = true;
                        }
                    }
                    if !rate_changed {
                        self.handle_audio_loss("the device stopped calling back");
                        return;
                    }
                }
            }
            if !callbacks_advancing {
                let playing_name = self.rig.output.playing.as_ref().map(|p| p.device.clone());
                if let Some(device_name) = playing_name
                    && let Ok(outputs) = self.rig.host.outputs()
                    && !outputs.devices.iter().any(|d| d.name == device_name)
                {
                    self.handle_audio_loss("it was unplugged");
                    return;
                }
            }
        }

        // 2. If audio was lost, watch and attempt recovery
        let Some(mut lost) = self.rig.output.lost_audio.take() else {
            return;
        };

        if lost.was_system_default {
            if now.saturating_duration_since(lost.last_attempt) >= Duration::from_secs(1) {
                lost.last_attempt = now;
                if lost.attempts < 10 {
                    if let Ok(outputs) = self.rig.host.outputs()
                        && let Some(new_default) = outputs.default
                    {
                        let options = loom_host_desktop::AudioOptions {
                            device: Some(new_default.clone()),
                            exact: true,
                            sample_rate: None,
                            buffer_size: None,
                        };
                        match self.rig.host.open(&options) {
                            Ok(output) => match self.restart_audio(output) {
                                Ok(()) => {
                                    self.audio_back();
                                    self.alerts.warn(format!(
                                        "Audio: \"{}\" was lost: {}. Now playing on \"{new_default}\".",
                                        lost.device, lost.reason
                                    ));
                                    return;
                                }
                                Err(e) => {
                                    lost.attempts += 1;
                                    lost.last_error = Some(e);
                                }
                            },
                            Err(e) => {
                                lost.attempts += 1;
                                lost.last_error = Some(e);
                            }
                        }
                    }
                    if lost.attempts >= 10 {
                        let raw = lost.last_error.as_deref().unwrap_or("could not reopen device");
                        let reason = Self::unwrap_reopen_error(raw);
                        let message = format!(
                            "Audio: could not reopen \"{}\": {reason}. Choose a device in Preferences.",
                            lost.device
                        );
                        self.alerts.error(alerts::Cause::AudioOutput, message);
                    }
                }
            }
            self.rig.output.lost_audio = Some(lost);
        } else {
            // Named device: repeats once a second for up to 10 tries
            if now.saturating_duration_since(lost.last_attempt) >= Duration::from_secs(1) {
                lost.last_attempt = now;
                if lost.attempts < 10
                    && let Ok(outputs) = self.rig.host.outputs()
                    && outputs.devices.iter().any(|d| d.name == lost.device)
                {
                    let saved = self.rig.store.saved().audio.clone();
                    match devices::open_choice(&*self.rig.host, &saved) {
                        Ok(output) => match self.restart_audio(output) {
                            Ok(()) => {
                                self.audio_back();
                                self.alerts.say(format!("Audio: \"{}\" is back.", lost.device));
                                return;
                            }
                            Err(e) => {
                                lost.attempts += 1;
                                lost.last_error = Some(e);
                            }
                        },
                        Err(e) => {
                            lost.attempts += 1;
                            lost.last_error = Some(e);
                        }
                    }
                    if lost.attempts >= 10 {
                        let raw = lost.last_error.as_deref().unwrap_or("could not start stream");
                        let reason = Self::unwrap_reopen_error(raw);
                        let message = format!(
                            "Audio: could not reopen \"{}\": {reason}. Choose a device in Preferences.",
                            lost.device
                        );
                        self.alerts.error(alerts::Cause::AudioOutput, message);
                    }
                }
            }
            self.rig.output.lost_audio = Some(lost);
        }
    }
}
