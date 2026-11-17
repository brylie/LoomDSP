//! Transport: one group of [`Controller`]'s methods.

use super::*;

/// # Transport
///
/// The patch transport: tempo, time signature, running, and reset
/// (`docs/design/clock.md`).
impl Controller {
    /// Selects a MIDI clock input or returns to Internal with `None`.
    ///
    /// Use input zero with `reconnect` to preserve state when a selected port is unavailable.
    /// On its return, pass the new input ID with `reconnect`.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine's command queue is full.
    pub fn select_midi_clock(&mut self, input: Option<crate::MidiInputId>, reconnect: bool) -> Result<(), LoomError> {
        self.send(Command::SelectMidiClock(input, reconnect))?;
        self.following_midi_clock = input.is_some();
        Ok(())
    }

    /// Whether timing is owned by MIDI, including while its source is absent.
    pub fn follows_midi_clock(&self) -> bool {
        self.following_midi_clock
    }

    /// The effective clock tempo last published by the engine, once it has rendered.
    pub fn rendered_tempo(&self) -> Option<f64> {
        let tempo = f64::from_bits(self.stats.transport_tempo.load(Ordering::Relaxed));
        (tempo > 0.0).then_some(tempo)
    }

    /// The transport settings last sent to the engine (ADR-0015).
    pub fn transport(&self) -> TransportSettings {
        self.transport
    }

    /// Sets the tempo in beats per minute, clamped to 20 to 300. Returns the
    /// tempo used. Takes effect at the next buffer, and the position does
    /// not jump.
    pub fn set_tempo(&mut self, bpm: f64) -> Result<f64, LoomError> {
        let tempo = clamp_tempo(bpm);
        self.send(Command::SetTempo(tempo))?;
        self.transport.tempo = tempo;
        Ok(tempo)
    }

    /// Sets the time signature. One out of range is refused with
    /// [`LoomError::InvalidTimeSignature`], and the transport does not
    /// change.
    pub fn set_time_signature(&mut self, time_signature: TimeSignature) -> Result<(), LoomError> {
        if !time_signature.is_valid() {
            return Err(LoomError::InvalidTimeSignature(time_signature));
        }
        self.send(Command::SetTimeSignature(time_signature))?;
        self.transport.time_signature = time_signature;
        Ok(())
    }

    /// Starts or stops the transport at the next buffer. Stopping holds the
    /// position; starting continues from there.
    pub fn set_running(&mut self, running: bool) -> Result<(), LoomError> {
        self.send(Command::SetRunning(running))?;
        self.transport.running = running;
        Ok(())
    }

    /// Returns the transport to bar 1 at the next buffer, running or stopped.
    /// Modules see it as a jump ([`TransportInfo::jumped`](crate::TransportInfo::jumped)).
    pub fn reset_transport(&mut self) -> Result<(), LoomError> {
        self.send(Command::ResetTransport)
    }
}
