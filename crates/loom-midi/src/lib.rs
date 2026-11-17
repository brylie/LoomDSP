//! Short MIDI messages and clock arithmetic. Time is supplied by the caller.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// MIDI Timing Clock pulses per quarter note.
pub const PULSES_PER_QUARTER: u32 = 24;

/// Distance between consecutive clock pulses, in quarter notes.
pub const PULSE_BEATS: f64 = 1.0 / PULSES_PER_QUARTER as f64;

/// A decoded short MIDI message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
    /// Pressure for one note on a channel.
    PolyPressure {
        /// Channel, 0 to 15.
        channel: u8,
        /// Note number.
        note: u8,
        /// Pressure, 0 to 127.
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
    /// One of 24 clock pulses per quarter note.
    TimingClock,
    /// Start from the beginning.
    Start,
    /// Resume from the current position.
    Continue,
    /// Hold the current position.
    Stop,
    /// Position in sixteenth notes.
    SongPosition(u16),
    /// Anything else.
    Other,
}

/// Encodes a channel message in the short message format.
pub fn encode_channel(message: MidiMessage) -> Option<([u8; 3], u8)> {
    let (status, channel, first, second, len) = match message {
        MidiMessage::NoteOn {
            channel,
            note,
            velocity,
        } => (0x90, channel, note, velocity, 3),
        MidiMessage::NoteOff { channel, note } => (0x80, channel, note, 0, 3),
        MidiMessage::ControlChange {
            channel,
            controller,
            value,
        } => (0xb0, channel, controller, value, 3),
        MidiMessage::PolyPressure { channel, note, value } => (0xa0, channel, note, value, 3),
        MidiMessage::ChannelPressure { channel, value } => (0xd0, channel, value, 0, 2),
        MidiMessage::PitchBend { channel, value } => {
            let raw = (i32::from(value).clamp(-8192, 8191) + 8192) as u16;
            (0xe0, channel, (raw & 127) as u8, (raw >> 7) as u8, 3)
        }
        _ => return None,
    };
    Some(([status | (channel & 15), first & 127, second & 127], len))
}

/// Decodes a message from its bytes and length.
pub fn decode(data: [u8; 3], len: u8) -> MidiMessage {
    match (len, data[0]) {
        (1, 0xf8) => return MidiMessage::TimingClock,
        (1, 0xfa) => return MidiMessage::Start,
        (1, 0xfb) => return MidiMessage::Continue,
        (1, 0xfc) => return MidiMessage::Stop,
        (3, 0xf2) if data[1] < 128 && data[2] < 128 => {
            return MidiMessage::SongPosition(u16::from(data[1]) | (u16::from(data[2]) << 7));
        }
        _ => {}
    }
    let status = data[0] & 0xf0;
    let a = data[1];
    let b = data[2];
    let expected_len = if status == 0xd0 { 2 } else { 3 };
    if len != expected_len || a >= 128 || (len == 3 && b >= 128) {
        return MidiMessage::Other;
    }
    let channel = data[0] & 0x0f;
    match status {
        0x90 if b > 0 => MidiMessage::NoteOn {
            channel,
            note: a,
            velocity: b,
        },
        0x90 | 0x80 => MidiMessage::NoteOff { channel, note: a },
        0xB0 => MidiMessage::ControlChange {
            channel,
            controller: a,
            value: b,
        },
        0xA0 => MidiMessage::PolyPressure {
            channel,
            note: a,
            value: b,
        },
        0xD0 => MidiMessage::ChannelPressure { channel, value: a },
        0xE0 => MidiMessage::PitchBend {
            channel,
            value: ((i16::from(b) << 7) | i16::from(a)) - 8192,
        },
        _ => MidiMessage::Other,
    }
}

/// Clock messages that can be sent to a MIDI output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClockMessage {
    /// Timing Clock.
    Pulse,
    /// Start from zero.
    Start,
    /// Resume from the current position.
    Continue,
    /// Stop playback.
    Stop,
    /// Locate in sixteenth notes.
    Position(u16),
}

impl ClockMessage {
    /// Encodes the preceding sixteenth note, saturating at the wire limit.
    pub fn at_position(beats: f64) -> Self {
        Self::Position((beats * 4.0).clamp(0.0, 16_383.0) as u16)
    }

    /// Passes the encoded bytes to a caller.
    pub fn encode<T>(self, send: impl FnOnce(&[u8]) -> T) -> T {
        match self {
            Self::Pulse => send(&[0xf8]),
            Self::Start => send(&[0xfa]),
            Self::Continue => send(&[0xfb]),
            Self::Stop => send(&[0xfc]),
            Self::Position(position) => {
                let position = position.min(16_383);
                send(&[0xf2, (position & 127) as u8, (position >> 7) as u8])
            }
        }
    }
}

/// Up to one beat of arrival intervals. Units are chosen by the caller.
#[derive(Default)]
pub struct PulseHistory {
    stamps: [u64; 25],
    next: usize,
    count: usize,
}

impl PulseHistory {
    /// Records a timestamp and returns the mean interval once it is measurable.
    pub fn pulse(&mut self, stamp: u64) -> Option<f64> {
        self.stamps[self.next] = stamp;
        self.next = (self.next + 1) % self.stamps.len();
        self.count = (self.count + 1).min(self.stamps.len());
        let first = (self.next + self.stamps.len() - self.count) % self.stamps.len();
        let elapsed = stamp.saturating_sub(self.stamps[first]);
        (elapsed > 0).then(|| elapsed as f64 / (self.count - 1) as f64)
    }
}

#[cfg(test)]
mod tests;
