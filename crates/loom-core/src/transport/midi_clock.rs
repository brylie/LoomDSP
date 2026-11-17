//! The external MIDI clock, integrated with the engine's sample timeline.

use super::{Transport, clamp_tempo};
use crate::{MidiEvent, MidiInputId};
use loom_midi::{MidiMessage, PULSES_PER_QUARTER, PulseHistory};

/// The selected clock and the pulse count since its last locate.
#[derive(Default)]
pub(crate) struct MidiClock {
    pub input: Option<MidiInputId>,
    history: PulseHistory,
    stamped: Option<bool>,
    pending: Option<Run>,
    last_pulse: Option<u64>,
    origin: f64,
    pulses: u64,
    at_origin: bool,
    pub lost: bool,
}

/// How the next pulse starts playback.
enum Run {
    Start,
    Continue,
}

impl MidiClock {
    /// Selects, disconnects, or reconnects the clock. Zero means disconnected.
    pub fn select(&mut self, input: Option<MidiInputId>, reconnect: bool, transport: &mut Transport, frame: u64) {
        if self.input == input {
            return;
        }
        if reconnect && self.input.is_some() && input.is_some() {
            self.input = input;
            self.clear_intervals();
            return;
        }
        let previous = self.input.is_some();
        *self = Self {
            input,
            ..Self::default()
        };
        if input.is_some() {
            transport.set_running(frame, false);
        } else if previous {
            transport.release_limit(frame);
        }
    }

    /// Clears measurements across a gap, retaining the tempo.
    fn clear_intervals(&mut self) {
        self.history = PulseHistory::default();
        self.stamped = None;
    }

    /// The first frame that must free run if no pulse arrives.
    pub fn deadline(&self, transport: &Transport) -> Option<u64> {
        if self.lost || !transport.settings.running {
            return None;
        }
        let last = self.last_pulse?;
        let frames = (120.0 * f64::from(transport.sample_rate) / transport.settings.tempo).ceil() as u64;
        Some(last.saturating_add(frames.max(1)))
    }

    /// Starts free running at the threshold frame.
    pub fn check_loss(&mut self, transport: &mut Transport, frame: u64) {
        if self.deadline(transport).is_some_and(|deadline| frame >= deadline) {
            transport.release_limit(frame);
            self.clear_intervals();
            self.lost = true;
        }
    }

    /// Applies one message in arrival order.
    pub fn receive(&mut self, transport: &mut Transport, frame: u64, event: MidiEvent) -> bool {
        if self.input != Some(MidiInputId(event.input)) || event.input == 0 {
            return false;
        }
        match event.message() {
            MidiMessage::Start | MidiMessage::Continue => {
                transport.set_running(frame, false);
                transport.release_limit(frame);
                self.pending = Some(if event.message() == MidiMessage::Start {
                    Run::Start
                } else {
                    Run::Continue
                });
            }
            MidiMessage::Stop => {
                transport.set_running(frame, false);
                transport.release_limit(frame);
                self.pending = None;
                self.lost = false;
            }
            MidiMessage::SongPosition(sixteenths) => {
                self.origin = f64::from(sixteenths) / 4.0;
                self.pulses = 0;
                self.at_origin = true;
                transport.locate(frame, self.origin, true);
                transport.limit = Some(self.position(1));
                if self.pending.is_some() {
                    self.pending = Some(Run::Continue);
                }
                return true;
            }
            MidiMessage::TimingClock => return self.pulse(transport, frame, event.arrival_micros),
            _ => {}
        }
        false
    }

    /// Calculates an anchor from the integer pulse count.
    fn position(&self, ahead: u64) -> f64 {
        self.origin + self.pulses.saturating_add(ahead) as f64 / f64::from(PULSES_PER_QUARTER)
    }

    /// Updates the measured tempo and, during playback, the position.
    fn pulse(&mut self, transport: &mut Transport, frame: u64, micros: Option<u64>) -> bool {
        if self.stamped != Some(micros.is_some()) {
            self.clear_intervals();
            self.stamped = Some(micros.is_some());
        }
        let old = transport.info(frame).position();
        if let Some(mean) = self.history.pulse(micros.unwrap_or(frame)) {
            let units_per_second = if micros.is_some() {
                1_000_000.0
            } else {
                f64::from(transport.sample_rate)
            };
            transport.set_tempo(
                frame,
                clamp_tempo(60.0 * units_per_second / f64::from(PULSES_PER_QUARTER) / mean),
            );
        }
        self.last_pulse = Some(frame);
        let lost = std::mem::take(&mut self.lost);
        let mut jump = false;
        if let Some(run) = self.pending.take() {
            jump = matches!(run, Run::Start);
            self.origin = if jump { 0.0 } else { old };
            self.pulses = 0;
            self.at_origin = true;
            transport.set_running(frame, true);
        }
        if transport.settings.running {
            if !std::mem::take(&mut self.at_origin) {
                self.pulses = self.pulses.saturating_add(1);
            }
            let position = self.position(0);
            jump |= lost && (old - position).abs() > loom_midi::PULSE_BEATS;
            transport.locate(frame, position, jump);
            transport.limit = Some(self.position(1));
        }
        jump
    }
}
