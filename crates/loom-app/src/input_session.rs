//! The life of the audio input stream: when it opens and closes, what happens
//! when it fails, and what the player is told (`docs/design/audio-input.md`,
//! `docs/design/device-changes.md`).
//!
//! [`InputSession`] lives in the `Rig`, so it outlives each engine. It runs
//! only on the control thread. The stream's own callbacks reach it through
//! the failure flag in the port's `InputStatus`.

use crate::alerts::{Alerts, Cause};
use crate::devices::{Host, InputPort};
use crate::input_watch::Tracker;
use crate::preferences::InputChoice;
use loom_core::Controller;
use loom_host_desktop::{AudioOptions, InputError};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// The shortest time between two attempts to open a device.
const RETRY_EVERY: Duration = Duration::from_secs(1);

/// Where the stream is in its life.
///
/// The port and its drift tracker exist together, and only an open stream has
/// them. A stream that has failed is still `Open` with a port that is not
/// live: the failure flag is set from other threads, so the session finds out
/// at the next [`InputSession::sync`], which replaces it.
enum Phase {
    /// The patch or the settings do not need an input.
    Off,
    /// An input is needed and no stream is open.
    Waiting {
        /// The last attempt to open, to avoid repeated device opens.
        last_attempt: Option<Instant>,
        /// A named input was lost while playing: wait for that device to
        /// return, rather than falling back to the system default.
        named_lost: bool,
    },
    /// A stream is open.
    Open {
        /// The stream. Dropping it closes the device.
        port: Box<dyn InputPort>,
        /// Drift warning history, shared with the input watcher.
        tracker: Arc<Tracker>,
    },
}

/// What the app knows about the input it needs now.
pub(crate) struct Demand<'a> {
    /// The patch has an Audio In and the input is not off, whether or not an
    /// output plays. A lost named input is waited for as long as this holds.
    pub wanted: bool,
    /// An input should be open now: it is wanted and a real output plays.
    pub needed: bool,
    /// Part of a device name from `--input`, for this run.
    pub flag: Option<&'a str>,
    /// The saved choice.
    pub choice: &'a InputChoice,
    /// The output's buffer in frames, once known. The ring is sized for it.
    pub buffer: Option<u32>,
    /// The output's sample rate.
    pub sample_rate: u32,
}

/// Owns the input stream, its retry and recovery state, and which of its
/// messages have been shown.
///
/// The messages outlive the phases. A refusal shows once per run, however
/// often the stream opens and closes, and a device error stays reported until
/// the stream opens again.
pub struct InputSession {
    phase: Phase,
    /// A device error has been reported since the stream last opened.
    error_reported: bool,
    /// The refused permission has been reported in this run.
    permission_reported: bool,
    /// The live-input feedback warning has been shown in this run.
    feedback_warned: bool,
}

impl InputSession {
    /// Starts with no stream and nothing reported.
    pub(crate) fn new() -> Self {
        Self {
            phase: Phase::Off,
            error_reported: false,
            permission_reported: false,
            feedback_warned: false,
        }
    }

    /// The open stream, if any. It may have failed since the last sync.
    pub(crate) fn port(&self) -> Option<&dyn InputPort> {
        self.open().map(|(port, _)| port)
    }

    /// The open stream and its drift tracker, for the input watcher.
    pub(crate) fn open(&self) -> Option<(&dyn InputPort, &Arc<Tracker>)> {
        match &self.phase {
            Phase::Open { port, tracker } => Some((port.as_ref(), tracker)),
            _ => None,
        }
    }

    /// Whether a lost named input is being waited for.
    pub(crate) fn named_lost(&self) -> bool {
        matches!(self.phase, Phase::Waiting { named_lost: true, .. })
    }

    /// Whether the feedback warning has been shown in this run.
    pub(crate) fn feedback_warned(&self) -> bool {
        self.feedback_warned
    }

    /// Lets the next sync try to open at once, as if the last attempt were
    /// long ago.
    #[cfg(test)]
    pub(crate) fn forget_attempt(&mut self) {
        if let Phase::Waiting { last_attempt, .. } = &mut self.phase {
            *last_attempt = None;
        }
    }

    /// Takes the stream out of the engine. A full queue cannot be waited for
    /// here, so the engine then keeps its reader on a closed ring until the
    /// next swap: it reads silence, which is safe.
    fn close(&mut self, controller: &mut Controller) {
        if matches!(self.phase, Phase::Open { .. }) {
            let _ = controller.set_audio_input(None);
        }
        self.phase = Phase::Off;
    }

    /// The output stream stopped, and its engine with it, so no command is
    /// sent. A lost named input is still waited for on the next stream.
    pub(crate) fn stream_stopped(&mut self) {
        self.phase = match self.phase {
            Phase::Off => Phase::Off,
            Phase::Waiting { named_lost, .. } => Phase::Waiting {
                last_attempt: None,
                named_lost,
            },
            Phase::Open { .. } => Phase::Waiting {
                last_attempt: None,
                named_lost: false,
            },
        };
    }

    /// The player chose an input: start again, so a refusal or device error
    /// can show again for it. The feedback warning stays once per run.
    pub(crate) fn choose(&mut self, controller: &mut Controller, alerts: &mut Alerts) {
        self.close(controller);
        alerts.clear(Cause::AudioInput);
        self.error_reported = false;
        self.permission_reported = false;
    }

    /// Reports a problem, and keeps it on screen until its cause is gone.
    fn report(&mut self, alerts: &mut Alerts, message: String) {
        alerts.error(Cause::AudioInput, message);
    }

    /// Keeps the stream in step with what is needed now. A failure leaves the
    /// input silent and the rest of the patch playing.
    pub(crate) fn sync(
        &mut self,
        demand: Demand<'_>,
        host: &dyn Host,
        controller: &mut Controller,
        alerts: &mut Alerts,
    ) {
        if !demand.needed {
            // One interface often carries both directions, so a lost output
            // and a lost input come together. The audio poll syncs while the
            // output is stopped, and that must not end the wait: the patch
            // would come back on the default microphone. Once the input is
            // no longer wanted, a later Audio In starts fresh.
            let keep_waiting = demand.wanted && self.named_lost();
            // With no input wanted, its error no longer matters, and a later
            // Audio In may report one again.
            if !demand.wanted && alerts.clear(Cause::AudioInput) {
                self.error_reported = false;
            }
            self.close(controller);
            if keep_waiting {
                self.phase = Phase::Waiting {
                    last_attempt: None,
                    named_lost: true,
                };
            }
            alerts.set_input_drift(None);
            return;
        }
        if host.audio_input_permission_denied() {
            if matches!(self.phase, Phase::Open { .. }) {
                self.close(controller);
            }
            alerts.set_input_drift(None);
            if !self.permission_reported {
                self.report(alerts, input_error_message(&InputError::PermissionDenied));
                self.permission_reported = true;
            }
            return;
        }
        let (last_attempt, named_lost) = match std::mem::replace(&mut self.phase, Phase::Off) {
            Phase::Open { port, tracker } if port.is_live() => {
                self.phase = Phase::Open { port, tracker };
                self.warn_about_feedback(controller, alerts);
                return;
            }
            Phase::Open { port, .. } => {
                let name = port.name().to_string();
                drop(port);
                alerts.set_input_drift(None);
                let _ = controller.set_audio_input(None);
                if !self.error_reported {
                    self.report(
                        alerts,
                        format!("Audio input: \"{name}\" was lost. Trying to reopen it."),
                    );
                    self.error_reported = true;
                }
                (
                    None,
                    demand.flag.is_some() || matches!(demand.choice, InputChoice::Named(_)),
                )
            }
            Phase::Waiting {
                last_attempt,
                named_lost,
            } => (last_attempt, named_lost),
            Phase::Off => (None, false),
        };
        self.phase = Phase::Waiting {
            last_attempt,
            named_lost,
        };
        let now = Instant::now();
        if last_attempt.is_some_and(|at| now.duration_since(at) < RETRY_EVERY) {
            return;
        }
        // With the output's default buffer, wait for its first callback so
        // the ring is sized for the actual device rather than max_block.
        let Some(buffer) = demand.buffer else { return };
        self.phase = Phase::Waiting {
            last_attempt: Some(now),
            named_lost,
        };
        let mut options = AudioOptions {
            sample_rate: Some(demand.sample_rate),
            buffer_size: Some(buffer),
            ..Default::default()
        };
        let mut fallback = None;
        if let Some(wanted) = demand.flag {
            options.device = Some(wanted.to_string());
        } else if let InputChoice::Named(name) = demand.choice {
            if host.audio_inputs().is_ok_and(|inputs| !inputs.devices.contains(name)) {
                if named_lost {
                    return;
                }
                fallback = Some(name.clone());
            } else {
                options.device = Some(name.clone());
                options.exact = true;
            }
        }
        match host.open_audio_input(&options, buffer as usize) {
            Ok((port, reader)) => match controller.set_audio_input(Some(reader)) {
                Ok(()) => {
                    let stats = controller.stats();
                    let counts = (
                        stats.input_underruns.load(Ordering::Relaxed),
                        stats.input_overruns.load(Ordering::Relaxed),
                    );
                    let name = port.name().to_string();
                    self.phase = Phase::Open {
                        port,
                        tracker: Arc::new(Tracker::new(counts)),
                    };
                    self.error_reported = false;
                    // A dismissed error gets no "is back" message.
                    let error_was_kept = alerts.clear(Cause::AudioInput);
                    if let Some(wanted) = fallback {
                        alerts.warn(format!(
                            "Audio input: \"{wanted}\" is not available. Using the system default."
                        ));
                    } else if error_was_kept {
                        alerts.say(format!("Audio input: \"{name}\" is back."));
                    }
                }
                Err(error) => alerts.warn(format!("Audio input: {error}.")),
            },
            Err(error) => {
                let reported = if matches!(error, InputError::PermissionDenied) {
                    self.permission_reported
                } else {
                    self.error_reported
                };
                if !reported {
                    self.report(alerts, input_error_message(&error));
                    if matches!(error, InputError::PermissionDenied) {
                        self.permission_reported = true;
                    } else {
                        self.error_reported = true;
                    }
                }
            }
        }
    }

    /// Warns once per run when an Audio In module first plays live sound.
    fn warn_about_feedback(&mut self, controller: &Controller, alerts: &mut Alerts) {
        if self.feedback_warned {
            return;
        }
        let live = controller.graph().nodes().any(|(id, node)| {
            node.descriptor.id == "loom.io.audio-in"
                && node
                    .params
                    .get(2)
                    .is_some_and(|&mute| controller.modulated(id, 2).unwrap_or(mute) < 0.5)
                && controller.display(id, 0).is_some_and(|active| active >= 0.5)
        });
        if live {
            alerts.warn("Audio input is live. Use headphones, or keep the gain low, to avoid feedback.");
            self.feedback_warned = true;
        }
    }
}

/// Makes an input error actionable without reporting it from a callback.
fn input_error_message(error: &InputError) -> String {
    if matches!(error, InputError::PermissionDenied) {
        let place = if cfg!(target_os = "macos") {
            "System Settings, Privacy and Security, Microphone"
        } else if cfg!(target_os = "windows") {
            "Settings, Privacy and security, Microphone"
        } else {
            "your system's microphone privacy settings"
        };
        format!("Audio input: access to the microphone was denied. Allow LoomDSP in {place}.")
    } else {
        format!("Audio input: {error}.")
    }
}

#[cfg(test)]
mod tests;
