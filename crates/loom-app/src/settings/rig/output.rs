//! Audio output choices, stream changes, and the cross-stream recording.

use super::Rig;
use crate::App;
use crate::devices::{self, Playing};
use crate::preferences::{AudioPrefs, BufferChoice, DeviceChoice, RateChoice};
use crate::settings::{Change, Failed, Message};
use loom_core::{EngineConfig, EngineParts, OutputSafety};
use loom_host_desktop::{AudioOutput, Outputs, Recorder, STOP_LIMIT};
use std::borrow::Cow;

/// Audio output state that survives replacing the stream.
#[derive(Default)]
pub(crate) struct OutputSession {
    /// True while `--output-safety` sets the mode.
    pub safety_flag: bool,
    /// The device playing now, when one is.
    pub playing: Option<Playing>,
    /// A mode change the engine's full queue refused, sent again on the
    /// next update.
    pub safety_pending: Option<OutputSafety>,
    /// Whether the last audio stop waited for the fade-out to end, for tests.
    pub last_stop: Option<bool>,
    /// The `--record` file. It belongs to the rig, not to one stream, so
    /// it records every engine the run plays (audio-qa.md, Recording the whole run).
    pub recorder: Option<Recorder>,
    /// A lost audio output being watched for recovery (device-changes.md, Audio device loss).
    pub lost_audio: Option<super::recovery::LostAudio>,
    /// Every mode change sent to the engine, for tests.
    #[cfg(test)]
    pub sent: Vec<OutputSafety>,
}

impl OutputSession {
    /// Starts with the command-line safety override, if there is one.
    pub(super) fn new(safety_flag: bool) -> Self {
        Self {
            safety_flag,
            ..Self::default()
        }
    }

    /// Sends what `output` will play to the recording, if there is one.
    /// Returns the refusal when the output runs at another sample rate. The
    /// recording stays open: call [`Self::end_recording`] once that output
    /// has started. A stream that fails to start must not end the recording,
    /// because the old device may then play again at the old rate.
    pub(crate) fn record_into(&self, output: &mut AudioOutput) -> Option<loom_host_desktop::HostError> {
        output.record(self.recorder.as_ref()?).err()
    }

    /// Ends the recording because a stream at another sample rate started
    /// (audio-qa.md, rule 6). Returns the status bar line, which is also
    /// printed to stderr.
    pub(crate) fn end_recording(&mut self, error: &loom_host_desktop::HostError) -> String {
        // Dropping the recorder closes the file with what it holds.
        self.recorder = None;
        let message = format!("The recording stopped: {error}.");
        eprintln!("loom-app: {message}");
        message
    }
}

/// Opens the audio output for a run: none with `--no-audio`, a timer with
/// `--fake-audio`, and otherwise the saved device with the flags on top.
/// Returns the output and status bar lines about fallbacks.
pub fn open_output(rig: &Rig, no_audio: bool, fake_audio: bool) -> (Result<AudioOutput, String>, Vec<String>) {
    if no_audio {
        (Err("audio off".to_string()), Vec::new())
    } else if fake_audio {
        let rate = rig.flags.sample_rate.unwrap_or(48_000);
        let block = rig.flags.buffer.unwrap_or(256);
        (Ok(AudioOutput::clock(rate, block)), Vec::new())
    } else {
        devices::open_at_start(&*rig.host, &rig.wanted())
    }
}

impl App {
    /// The change a control channel `audio` command asks for, checked
    /// against what the window offers (control-channel.md, rule S2). Give
    /// exactly one of the three. The device lists are the window's, or read
    /// now when the window is closed, as opening it would.
    pub(crate) fn control_audio_change(
        &self,
        device: Option<&str>,
        rate: Option<Option<u32>>,
        buffer: Option<Option<u32>>,
    ) -> Result<Change, String> {
        if self.session.engine.stream.without_device() {
            let flag = if self.session.engine.stream.playback().is_off() {
                "--no-audio"
            } else {
                "--fake-audio"
            };
            return Err(format!("there are no audio devices to choose with {flag}"));
        }
        let outputs = self
            .output_devices()
            .map_err(|e| format!("could not list the audio outputs: {e}"))?;
        let shown = self.rig.shown_audio();
        let info = || {
            let name = match &shown.device {
                DeviceChoice::SystemDefault => outputs.default.as_ref(),
                DeviceChoice::Named(name) => Some(name),
            };
            name.and_then(|name| outputs.devices.iter().find(|d| d.name == *name))
                .ok_or_else(|| "the chosen device is not connected; choose a device first".to_string())
        };
        let offered = |values: Vec<u32>| values.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
        match (device, rate, buffer) {
            (Some("default"), None, None) => Ok(Change::Device(DeviceChoice::SystemDefault)),
            (Some(wanted), None, None) => {
                let names: Vec<String> = outputs.devices.iter().map(|d| d.name.clone()).collect();
                devices::match_name(&names, wanted, "audio output")
                    .map(|name| Change::Device(DeviceChoice::Named(name)))
            }
            (None, Some(None), None) => Ok(Change::Rate(RateChoice::DeviceDefault)),
            (None, Some(Some(hz)), None) => {
                let rates = devices::offered_rates(info()?);
                if rates.contains(&hz) {
                    Ok(Change::Rate(RateChoice::Hz(hz)))
                } else {
                    Err(format!(
                        "{hz} Hz is not offered; rates: \"default\", {}",
                        offered(rates)
                    ))
                }
            }
            (None, None, Some(None)) => Ok(Change::Buffer(BufferChoice::DeviceDefault)),
            (None, None, Some(Some(frames))) => {
                let sizes = devices::offered_buffers(info()?);
                if sizes.contains(&frames) {
                    Ok(Change::Buffer(BufferChoice::Frames(frames)))
                } else {
                    Err(format!(
                        "{frames} frames is not offered; sizes: \"default\", {}",
                        offered(sizes)
                    ))
                }
            }
            _ => Err("give exactly one of `device`, `rate`, or `buffer`".to_string()),
        }
    }

    /// Picks the output safety mode (preferences.md, Changing the mode at
    /// run time).
    pub(in crate::settings) fn choose_output_safety(&mut self, mode: OutputSafety) {
        if !self.session.engine.stream.set_output_safety(mode) {
            return;
        }
        self.rig.output.safety_flag = false;
        let mut preferences = self.rig.store.saved().clone();
        preferences.output_safety = mode;
        if let Some(error) = self.rig.save(preferences) {
            self.alerts.warn(error);
        }
        // With no device there is no engine playing to tell. The next
        // engine is built with the new mode.
        if self.session.engine.stream.is_playing() {
            self.rig.output.safety_pending = Some(mode);
            self.send_pending_safety();
        }
    }

    /// Sends a mode change the queue refused before. Runs on every update.
    pub(crate) fn send_pending_safety(&mut self) {
        let Some(mode) = self.rig.output.safety_pending else {
            return;
        };
        if self.session.controller.set_output_safety(mode).is_ok() {
            self.rig.output.safety_pending = None;
            #[cfg(test)]
            self.rig.output.sent.push(mode);
        }
    }

    /// Changes the device, rate, or buffer size (preferences.md, What a
    /// change does).
    pub(in crate::settings) fn change_audio(&mut self, change: Change) {
        let shown = self.rig.shown_audio();
        let mut play = shown.clone();
        change.apply(&mut play);
        if play == shown {
            // Picking what plays now, such as a step back from a device
            // that failed, ends the failure, so steps go on from here.
            if let Some(window) = self.overlays.preferences_mut() {
                window.failed = None;
            }
            return;
        }
        let mut saved = self.rig.store.saved().clone();
        change.apply(&mut saved.audio);
        // A new device starts from its own rate and buffer size when it
        // does not offer the ones chosen for the old one (Rule D4).
        if let Change::Device(_) = change
            && let Some(info) = self.device_info(&play.device)
        {
            for audio in [&mut play, &mut saved.audio] {
                if audio
                    .sample_rate
                    .hz()
                    .is_some_and(|hz| !devices::offered_rates(&info).contains(&hz))
                {
                    audio.sample_rate = RateChoice::DeviceDefault;
                }
                if audio
                    .buffer
                    .frames()
                    .is_some_and(|f| !devices::offered_buffers(&info).contains(&f))
                {
                    audio.buffer = BufferChoice::DeviceDefault;
                }
            }
        }

        if !self.session.engine.stream.without_device() {
            // The new device opens first: if it fails, the old one keeps
            // playing and nothing is saved (Rule D5).
            let output = match devices::open_choice(&*self.rig.host, &play) {
                Ok(output) => output,
                Err(e) => {
                    let error = self.could_not_use(&play, &e);
                    self.audio_failed(&change, &error);
                    self.alerts.warn(error);
                    return;
                }
            };
            if let Err(e) = self.restart_audio(output) {
                self.audio_failed(&change, &e);
                self.alerts.warn(e);
                return;
            }
            self.audio_back();
        } else {
            self.audio_back();
        }
        if let Some(window) = self.overlays.preferences_mut() {
            window.failed = None;
        }
        match change {
            Change::Device(_) => self.rig.flags.device = None,
            Change::Rate(_) => self.rig.flags.sample_rate = None,
            Change::Buffer(_) => self.rig.flags.buffer = None,
        }
        // The flags that are left still set this run; what is saved is the
        // player's choice.
        if let Some(error) = self.rig.save(saved) {
            self.alerts.warn(error);
        }
    }

    /// Notes in the open window that `change` could not be used.
    fn audio_failed(&mut self, change: &Change, error: &str) {
        if let Some(window) = self.overlays.preferences_mut() {
            window.failed = Some(Failed {
                message: Message::Audio(change.clone()),
                error: error.to_string(),
            });
        }
    }

    /// `Could not use "MOTU M4" at 192000 Hz: <reason>. Still playing on
    /// "MacBook Pro Speakers".`
    fn could_not_use(&self, choice: &AudioPrefs, reason: &str) -> String {
        let name = match &choice.device {
            DeviceChoice::Named(name) => name.clone(),
            DeviceChoice::SystemDefault => self
                .default_device_name()
                .unwrap_or_else(|| "the system default".into()),
        };
        let rate = choice
            .sample_rate
            .hz()
            .or_else(|| self.device_info(&choice.device).map(|info| info.default_rate));
        let at = rate.map(|hz| format!(" at {hz} Hz")).unwrap_or_default();
        let still = match &self.rig.output.playing {
            Some(playing) => format!(" Still playing on \"{}\".", playing.device),
            None => String::new(),
        };
        format!("Could not use \"{name}\"{at}: {reason}.{still}")
    }

    /// Uses the listed outputs while Preferences is open, otherwise queries
    /// the host directly without creating any Preferences state.
    fn output_devices(&self) -> Result<Cow<'_, Outputs>, String> {
        match self.overlays.preferences() {
            Some(window) => window.lists.outputs.as_ref().map(Cow::Borrowed).map_err(Clone::clone),
            None => self.rig.host.outputs().map(Cow::Owned),
        }
    }

    /// The current system-default output's name, if known.
    fn default_device_name(&self) -> Option<String> {
        self.output_devices().ok().and_then(|outputs| outputs.default.clone())
    }

    /// What a device offers, from the lists the window read.
    pub(in crate::settings) fn device_info(&self, device: &DeviceChoice) -> Option<loom_host_desktop::OutputInfo> {
        let outputs = self.output_devices().ok()?;
        let name = match device {
            DeviceChoice::SystemDefault => outputs.default.as_ref()?,
            DeviceChoice::Named(name) => name,
        };
        outputs.devices.iter().find(|d| d.name == *name).cloned()
    }

    /// Builds a new engine from the patch as it is now, with every unsaved
    /// edit, at the new device's rate, fades the old one out, and plays the
    /// new one (preferences.md, Restarting audio). Module state starts
    /// again; the patch, undo history, selection, view, MIDI mappings, and
    /// MIDI inputs are kept (Rule D6). On an error before the old engine
    /// stops, nothing changes. If the new stream then fails to start, the
    /// old device plays again, so a failed change leaves sound playing
    /// (Rule D5).
    pub(in crate::settings) fn restart_audio(&mut self, output: AudioOutput) -> Result<(), String> {
        self.finish_edits();
        let parts = self
            .rebuilt_engine(output.sample_rate() as f32)
            .map_err(|e| format!("Could not restart audio: {e}. The audio is unchanged."))?;
        let previous = self.rig.output.playing.clone();
        self.stop_audio();
        let device = output.name().to_string();
        let error = match self.play(output, parts) {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        // Back to the device that played before, as it was.
        let restored = previous.as_ref().and_then(|old| {
            let options = loom_host_desktop::AudioOptions {
                device: Some(old.device.clone()),
                exact: true,
                sample_rate: Some(old.rate),
                buffer_size: old.buffer,
            };
            let output = self.rig.host.open(&options).ok()?;
            let parts = self.rebuilt_engine(output.sample_rate() as f32).ok()?;
            self.play(output, parts).ok()
        });
        Err(match (restored, previous) {
            (Some(()), Some(old)) => {
                format!(
                    "Could not use \"{device}\": {error}. Still playing on \"{}\".",
                    old.device
                )
            }
            _ => format!("Could not use \"{device}\": {error}. Audio is off."),
        })
    }

    /// Starts the current graph at a new rate, with fresh DSP state.
    fn rebuilt_engine(&self, rate: f32) -> Result<EngineParts, loom_core::LoomError> {
        let config = EngineConfig {
            output_safety: self.session.engine.stream.output_safety(),
            ..EngineConfig::with_sample_rate(rate)
        };
        self.session.controller.rebuild(config)
    }

    /// Gives the app an idle engine after a stream failed to start. The
    /// failed stream dropped the engine it was given, so nothing would drain
    /// the controller's queue, and edits would fail once it is full (#220).
    /// The new engine holds the patch as it is now, at the current rate.
    /// Does nothing while a device plays or an idle engine is in place.
    pub(crate) fn keep_engine_idle(&mut self) {
        if self.session.engine.stream.has_engine() {
            return;
        }
        let Ok(parts) = self.rebuilt_engine(self.session.engine.stream.sample_rate()) else {
            return;
        };
        let EngineParts {
            controller,
            engine,
            midi,
            telemetry,
        } = parts;
        self.rig.midi.attach(midi);
        self.generation += 1;
        self.session.controller = controller;
        // The new engine needs the panic trigger and the open MIDI inputs,
        // as in `play`, so panic and held-note release still work.
        self.send_panic_trigger();
        self.connect_midi(false, false);
        self.session.engine.monitor =
            crate::monitor::Monitor::new(telemetry, self.session.controller.config().sample_rate, self.generation);
        self.session.traces = loom_canvas::ScopeTraces::new();
        self.session.engine.live = crate::live::Live::showing_activity();
        self.session
            .engine
            .live
            .set_activity(self.rig.store.saved().show_activity);
        self.session.engine.stream.keep_idle(engine);
        // The stopped engine's live values are gone; show the new one's.
        self.refresh_values();
    }

    /// Puts a new engine in place of the old one, which has stopped, and
    /// plays it on `output`. On an error the app is left with no audio,
    /// and an idle engine that takes edits.
    fn play(&mut self, mut output: AudioOutput, parts: EngineParts) -> Result<(), String> {
        let EngineParts {
            controller,
            engine,
            midi,
            telemetry,
        } = parts;
        self.rig.midi.attach(midi);
        let playing = Playing::of(&output);
        let label = format!("{} at {} Hz", output.name(), output.sample_rate());
        self.generation += 1;
        self.session.controller = controller;
        self.send_panic_trigger();
        self.connect_midi(false, false);
        self.session.engine.monitor =
            crate::monitor::Monitor::new(telemetry, self.session.controller.config().sample_rate, self.generation);
        self.session.traces = loom_canvas::ScopeTraces::new();
        self.session.engine.live = crate::live::Live::showing_activity();
        self.session
            .engine
            .live
            .set_activity(self.rig.store.saved().show_activity);
        self.session.lights.clear();
        self.session.meter_levels.clear();
        self.session.engine.stream.prepare(output.sample_rate() as f32);
        self.session.engine.holding = false;
        let refused = self.rig.output.record_into(&mut output);
        if self.rig.host.system_clock_outputs() {
            output.set_midi_clock(self.rig.midi.clock_ports.clone());
        }
        let started = self.rig.host.start(output, engine);
        // Set on success; on an error this returns early.
        let record_error;
        match started {
            Ok(audio) => {
                record_error = refused.map(|e| self.rig.output.end_recording(&e));
                self.session.engine.stream.started(audio, label);
                self.rig.output.playing = Some(playing);
                self.session.engine.midi = self.rig.midi.input.clone();
            }
            Err(refused) => {
                let (engine, e) = *refused;
                self.session.engine.stream.refused(engine, &e);
                self.rig.output.playing = None;
                self.session.engine.midi = None;
                return Err(e);
            }
        }
        // Probes for the meters on screen, in the new engine.
        self.commit();
        self.refresh_values();
        // After the commit, which sets the status line too.
        if let Some(error) = record_error {
            self.alerts.warn(error);
        }
        Ok(())
    }

    /// Stops audio without a click: the engine fades out, and the stream
    /// stops after two more silent buffers, or at the stop limit (100 ms,
    /// longer for large buffers; output safety, rule 14). Used by Open, New, quit, and device changes.
    pub(crate) fn stop_audio(&mut self) {
        self.alerts.set_input_drift(None);
        // A new stream gets a new watcher, and a clean first read sends
        // nothing, so nothing else would turn these off.
        self.alerts.set_output_warnings(false, false);
        if let Some(audio) = self.session.engine.stream.take_running() {
            // A full queue cannot fade: the limit then stops the stream.
            let _ = self.session.controller.fade_out();
            self.rig.output.last_stop = Some(audio.stop(STOP_LIMIT));
        }
        self.rig.input.stream_stopped();
        self.rig.output.playing = None;
    }

    /// The line under the audio controls: what is playing now.
    pub(crate) fn playing_line(&self) -> String {
        if self.session.engine.stream.playback().is_off() {
            return "Audio is off for this run (--no-audio).".to_string();
        }
        if self.session.engine.stream.playback().is_fake() {
            return "No device for this run (--fake-audio).".to_string();
        }
        match &self.rig.output.playing {
            Some(playing) => {
                let frames = self
                    .session
                    .engine
                    .stream
                    .stats()
                    .map(|s| s.snapshot().largest_buffer)
                    .filter(|&f| f > 0)
                    .and_then(|f| u32::try_from(f).ok());
                playing.describe(frames)
            }
            None => format!("Not playing: {}.", self.session.engine.stream.label()),
        }
    }
}
