//! Virtual MIDI controllers, so tests and scripts can play the app through
//! a real MIDI input with no hardware (`docs/design/audio-qa.md`, Part 4).
//!
//! A virtual port is a MIDI source made by a program inside the operating
//! system's MIDI service. Other programs list it as an input, like a
//! controller plugged in by USB. Only macOS (CoreMIDI) and Linux (the ALSA
//! sequencer) have them. Elsewhere [`VirtualController::create`] returns an
//! error.

use crate::HostError;
use std::time::{Duration, Instant};

/// The MIDI client name of every virtual controller. On Linux, ALSA puts it
/// in front of the port name in the input list.
const CLIENT: &str = "LoomDSP virtual";

/// A virtual MIDI source that plays notes. It remembers which notes it
/// holds, and releases them when it is dropped, so none hangs in the app.
pub struct VirtualController {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    connection: midir::MidiOutputConnection,
    name: String,
    /// One bit per note, per channel.
    held: [u128; 16],
}

impl VirtualController {
    /// Creates a virtual MIDI source called `name`.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn create(name: &str) -> Result<Self, HostError> {
        use midir::os::unix::VirtualOutput;
        let output = midir::MidiOutput::new(CLIENT).map_err(|e| HostError::Midi(e.to_string()))?;
        let connection = output
            .create_virtual(name)
            .map_err(|e| HostError::Midi(format!("{name}: {e}")))?;
        Ok(Self {
            connection,
            name: name.to_string(),
            held: [0; 16],
        })
    }

    /// Virtual ports need macOS or Linux.
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub fn create(name: &str) -> Result<Self, HostError> {
        Err(HostError::Midi(format!(
            "{name}: virtual MIDI ports need macOS or Linux"
        )))
    }

    /// The name given to [`create`](Self::create).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name the MIDI input list shows for this port. It is the given
    /// name on macOS. On Linux ALSA adds the client name and a port number,
    /// such as `LoomDSP virtual:Pad 128:0`. Waits up to a second for the
    /// port to appear, because the MIDI service may list it a little late.
    pub fn input_name(&self) -> Result<String, HostError> {
        let alsa = format!("{CLIENT}:{} ", self.name);
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let names = crate::midi_input_names()?;
            if let Some(found) = names.into_iter().find(|n| *n == self.name || n.starts_with(&alsa)) {
                return Ok(found);
            }
            if Instant::now() > deadline {
                return Err(HostError::NoMidiPort(Some(self.name.clone())));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Sends a note on. `channel` is 0 to 15. A velocity of 0 is a note off.
    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) -> Result<(), HostError> {
        self.send(&[0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F])
    }

    /// Sends a note off. `channel` is 0 to 15.
    pub fn note_off(&mut self, channel: u8, note: u8) -> Result<(), HostError> {
        self.send(&[0x80 | (channel & 0x0F), note & 0x7F, 0])
    }

    /// Sends a note off for every note this controller holds.
    pub fn release_all(&mut self) -> Result<(), HostError> {
        for channel in 0..16u8 {
            for note in 0..128u8 {
                if self.held[usize::from(channel)] & (1u128 << note) != 0 {
                    self.note_off(channel, note)?;
                }
            }
        }
        Ok(())
    }

    /// True if any note is held.
    pub fn holds_notes(&self) -> bool {
        self.held.iter().any(|&h| h != 0)
    }

    /// Sends raw MIDI bytes, and keeps track of the notes they hold.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if let [status, note, velocity] = *bytes
            && note < 128
        {
            let channel = usize::from(status & 0x0F);
            match status & 0xF0 {
                0x90 if velocity > 0 => self.held[channel] |= 1u128 << note,
                0x80 | 0x90 => self.held[channel] &= !(1u128 << note),
                _ => {}
            }
        }
        self.write(bytes)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn write(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        self.connection
            .send(bytes)
            .map_err(|e| HostError::Midi(format!("{}: {e}", self.name)))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn write(&mut self, _bytes: &[u8]) -> Result<(), HostError> {
        unreachable!("a virtual controller cannot be created here")
    }
}

impl Drop for VirtualController {
    fn drop(&mut self) {
        // Best effort: the port goes away next, so an error here changes nothing.
        let _ = self.release_all();
    }
}
