//! Virtual MIDI controllers, so tests and scripts can play the app through
//! a real MIDI input with no hardware (`docs/design/audio-qa.md`, Virtual MIDI controllers).
//!
//! A virtual port is a MIDI source made by a program inside the operating
//! system's MIDI service. Other programs list it as an input, like a
//! controller plugged in by USB. Only macOS (CoreMIDI) and Linux (the ALSA
//! sequencer) have them. Elsewhere [`VirtualController::create`] returns an
//! error.

use crate::{HeldNotes, HostError};
use std::time::{Duration, Instant};

/// The MIDI client name of every virtual controller. On Linux, ALSA puts it
/// in front of the port name in the input list.
const CLIENT: &str = "LoomDSP virtual";

/// Where a virtual controller's messages go: the virtual port, or a list in
/// tests.
type Output = Box<dyn FnMut(&[u8]) -> Result<(), String>>;

/// A virtual MIDI source that plays notes. It remembers which notes it
/// holds, and releases them when it is dropped, so none hangs in the app.
pub struct VirtualController {
    output: Output,
    name: String,
    held: HeldNotes,
}

impl VirtualController {
    /// Creates a virtual MIDI source called `name`.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn create(name: &str) -> Result<Self, HostError> {
        use midir::os::unix::VirtualOutput;
        // No MIDI client at all: the system has no MIDI service.
        let output = midir::MidiOutput::new(CLIENT).map_err(|e| HostError::NoMidiService(e.to_string()))?;
        let mut connection = output
            .create_virtual(name)
            .map_err(|e| HostError::Midi(format!("{name}: {e}")))?;
        Ok(Self::with_output(
            name,
            Box::new(move |bytes| connection.send(bytes).map_err(|e| e.to_string())),
        ))
    }

    /// Virtual ports need macOS or Linux.
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub fn create(name: &str) -> Result<Self, HostError> {
        Err(HostError::NoMidiService(format!(
            "{name}: virtual MIDI ports need macOS or Linux"
        )))
    }

    /// A controller called `name` that sends to `output`. Only `create` and
    /// the tests call it, so it is not built where virtual ports do not exist.
    #[cfg(any(test, target_os = "macos", target_os = "linux"))]
    fn with_output(name: &str, output: Output) -> Self {
        Self {
            output,
            name: name.to_string(),
            held: HeldNotes::default(),
        }
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
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let names = crate::midi_input_names()?;
            if let Some(found) = names.into_iter().find(|n| self.is_listed_as(n)) {
                return Ok(found);
            }
            if Instant::now() > deadline {
                return Err(HostError::NoMidiPort(Some(self.name.clone())));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// True if `input` is the input list's name for this port: the name
    /// itself, or ALSA's `client:name client:port`.
    fn is_listed_as(&self, input: &str) -> bool {
        input == self.name
            || input
                .strip_prefix(CLIENT)
                .and_then(|rest| rest.strip_prefix(':'))
                .and_then(|rest| rest.strip_prefix(self.name.as_str()))
                .and_then(|rest| rest.strip_prefix(' '))
                .and_then(|rest| rest.split_once(':'))
                .is_some_and(|(client, port)| is_number(client) && is_number(port))
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
    /// A note whose note off fails stays held, so a later call, or the
    /// drop, tries it again.
    pub fn release_all(&mut self) -> Result<(), HostError> {
        for (channel, note) in self.held.notes() {
            self.note_off(channel, note)?;
        }
        Ok(())
    }

    /// True if any note is held.
    pub fn holds_notes(&self) -> bool {
        self.held.any()
    }

    /// Sends raw MIDI bytes. Once they are sent, it keeps track of the
    /// notes they hold, so what it holds is what the app received.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        (self.output)(bytes).map_err(|e| HostError::Midi(format!("{}: {e}", self.name)))?;
        self.held.track(bytes);
        Ok(())
    }
}

/// True if `text` is one or more digits.
fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

impl Drop for VirtualController {
    fn drop(&mut self) {
        // Best effort: the port goes away next, so an error here changes nothing.
        let _ = self.release_all();
    }
}

#[cfg(test)]
mod tests;
