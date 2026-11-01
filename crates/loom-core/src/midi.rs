//! MIDI events as seen by modules.

/// A short MIDI message with a frame offset.
///
/// The frame is relative to the start of the buffer passed to
/// [`Engine::process`](crate::Engine::process). Live input usually uses 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    /// Frame offset within the next processed buffer.
    pub frame: u32,
    /// Raw bytes. Unused bytes are zero.
    pub data: [u8; 3],
    /// Number of valid bytes in `data` (1 to 3).
    pub len: u8,
    /// The MIDI input it came from, or 0 for a source the engine does not
    /// track, such as the app's Hold note button. The engine keeps the
    /// notes each tracked input holds, so it can release them when the
    /// input goes away (`docs/design/device-changes.md`, Held notes belong
    /// to an input).
    pub input: u16,
}

/// A MIDI input the engine tracks, from 1 to [`MAX_MIDI_INPUTS`] - 1.
/// The host chooses it, opens it on the engine with
/// [`Controller::open_midi_input`](crate::Controller::open_midi_input), and
/// stamps it on every event from that input with [`MidiEvent::from_input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MidiInputId(pub u16);

/// How many MIDI inputs the engine can track at once, counting the unused
/// id 0.
pub const MAX_MIDI_INPUTS: u16 = 64;

/// Which notes one MIDI input holds: one bit per note on each of the 16
/// channels. Allocated on the control thread when the input opens.
#[derive(Debug, Default)]
pub(crate) struct HeldSet([u128; 16]);

impl HeldSet {
    /// True if `note` on `channel` is held.
    pub(crate) fn has(&self, channel: u8, note: u8) -> bool {
        self.0[usize::from(channel & 0x0F)] & (1u128 << (note & 0x7F)) != 0
    }

    /// Marks a note held or released.
    pub(crate) fn set(&mut self, channel: u8, note: u8, held: bool) {
        let bit = 1u128 << (note & 0x7F);
        let row = &mut self.0[usize::from(channel & 0x0F)];
        if held { *row |= bit } else { *row &= !bit }
    }

    /// How many notes are held.
    pub(crate) fn count(&self) -> usize {
        self.0.iter().map(|row| row.count_ones() as usize).sum()
    }

    /// Calls `f` for each held note, then holds none.
    pub(crate) fn drain(&mut self, mut f: impl FnMut(u8, u8)) {
        for (channel, row) in self.0.iter_mut().enumerate() {
            while *row != 0 {
                let note = row.trailing_zeros() as u8;
                *row &= !(1u128 << note);
                f(channel as u8, note);
            }
        }
    }
}

/// A decoded channel message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiMessage {
    /// Note on with velocity above zero.
    NoteOn {
        /// Channel, 0 to 15.
        channel: u8,
        /// Note number.
        note: u8,
        /// Velocity, 1 to 127.
        velocity: u8,
    },
    /// Note off (or note on with velocity zero).
    NoteOff {
        /// Channel, 0 to 15.
        channel: u8,
        /// Note number.
        note: u8,
    },
    /// Control change.
    ControlChange {
        /// Channel, 0 to 15.
        channel: u8,
        /// Controller number.
        controller: u8,
        /// Value, 0 to 127.
        value: u8,
    },
    /// Channel pressure (aftertouch for the whole channel).
    ChannelPressure {
        /// Channel, 0 to 15.
        channel: u8,
        /// Pressure, 0 to 127.
        value: u8,
    },
    /// Pitch bend, -8192 to 8191.
    PitchBend {
        /// Channel, 0 to 15.
        channel: u8,
        /// Bend amount.
        value: i16,
    },
    /// Anything else.
    Other,
}

impl MidiEvent {
    /// Builds an event from raw bytes. Extra bytes beyond 3 are ignored.
    pub fn from_bytes(frame: u32, bytes: &[u8]) -> Self {
        let mut data = [0u8; 3];
        let len = bytes.len().min(3);
        data[..len].copy_from_slice(&bytes[..len]);
        Self {
            frame,
            data,
            len: len as u8,
            input: 0,
        }
    }

    /// The same event, marked as coming from `input`.
    #[must_use]
    pub fn from_input(self, input: MidiInputId) -> Self {
        Self { input: input.0, ..self }
    }

    /// A note-on event.
    pub fn note_on(frame: u32, channel: u8, note: u8, velocity: u8) -> Self {
        Self::from_bytes(frame, &[0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F])
    }

    /// A note-off event.
    pub fn note_off(frame: u32, channel: u8, note: u8) -> Self {
        Self::from_bytes(frame, &[0x80 | (channel & 0x0F), note & 0x7F, 0])
    }

    /// A control change event.
    pub fn control_change(frame: u32, channel: u8, controller: u8, value: u8) -> Self {
        Self::from_bytes(frame, &[0xB0 | (channel & 0x0F), controller & 0x7F, value & 0x7F])
    }

    /// A pitch bend event. `value` is from -8192 to 8191, with 0 at the
    /// centre.
    pub fn pitch_bend(frame: u32, channel: u8, value: i16) -> Self {
        let raw = (i32::from(value).clamp(-8192, 8191) + 8192) as u16;
        Self::from_bytes(frame, &[0xE0 | (channel & 0x0F), (raw & 0x7F) as u8, (raw >> 7) as u8])
    }

    /// A channel pressure (aftertouch) event.
    pub fn channel_pressure(frame: u32, channel: u8, value: u8) -> Self {
        Self::from_bytes(frame, &[0xD0 | (channel & 0x0F), value & 0x7F])
    }

    /// Decodes the event.
    pub fn message(&self) -> MidiMessage {
        if self.len < 2 {
            return MidiMessage::Other;
        }
        let status = self.data[0] & 0xF0;
        let channel = self.data[0] & 0x0F;
        let a = self.data[1];
        let b = self.data[2];
        match status {
            0x90 if self.len == 3 && b > 0 => MidiMessage::NoteOn {
                channel,
                note: a,
                velocity: b,
            },
            0x90 | 0x80 if self.len == 3 => MidiMessage::NoteOff { channel, note: a },
            0xB0 if self.len == 3 => MidiMessage::ControlChange {
                channel,
                controller: a,
                value: b,
            },
            0xD0 => MidiMessage::ChannelPressure { channel, value: a },
            0xE0 if self.len == 3 => MidiMessage::PitchBend {
                channel,
                value: ((i16::from(b) << 7) | i16::from(a)) - 8192,
            },
            _ => MidiMessage::Other,
        }
    }
}

#[cfg(test)]
mod tests;
