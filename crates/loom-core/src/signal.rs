//! Signal types and value conventions (ADR-0007).
//!
//! Every cable carries `f32` samples at the engine sample rate. The type of a
//! port is metadata: it sets colour and default ranges, but never blocks a
//! connection. A useful mental model for Eurorack users: 1.0 equals 10 volts.

/// The purpose of a port. Used for colour and defaults, never to block a cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalType {
    /// Bipolar audio, nominally -1.0 to 1.0. Not clipped on cables.
    Audio,
    /// Modulation. Bipolar (-1.0 to 1.0) or unipolar (0.0 to 1.0).
    Modulation,
    /// Pitch at 0.1 per octave, 0.0 = middle C (MIDI note 60).
    Pitch,
    /// A ramp from 0.0 to 1.0 that wraps.
    Phase,
    /// On/off. Values above 0.5 are high.
    Gate,
}

impl SignalType {
    /// Short lowercase name, for display and file formats.
    pub fn name(self) -> &'static str {
        match self {
            SignalType::Audio => "audio",
            SignalType::Modulation => "modulation",
            SignalType::Pitch => "pitch",
            SignalType::Phase => "phase",
            SignalType::Gate => "gate",
        }
    }

    /// Suggested cable colour as RGB. The UI may override this with a theme.
    pub fn colour(self) -> [u8; 3] {
        match self {
            SignalType::Audio => [0xE0, 0x4F, 0x4F],
            SignalType::Modulation => [0x4F, 0x8F, 0xE0],
            SignalType::Pitch => [0xF0, 0x9A, 0x3E],
            SignalType::Phase => [0x9B, 0x6B, 0xE0],
            SignalType::Gate => [0xE8, 0xD2, 0x4A],
        }
    }
}

/// Gate threshold. Any value above this counts as "high".
pub const GATE_THRESHOLD: f32 = 0.5;

/// Returns true when a gate value is high.
#[inline]
pub fn gate_is_high(value: f32) -> bool {
    value > GATE_THRESHOLD
}

/// Pitch conventions: 0.1 per octave, 0.0 = middle C.
///
/// Why 0.1 per octave: it keeps 20 octaves inside the normalised -1.0 to 1.0
/// range, so pitch can modulate anything and anything can modulate pitch.
/// It is also 1 V/octave under the "1.0 = 10 V" model.
pub mod pitch {
    /// Frequency of pitch 0.0 (middle C, MIDI note 60) in Hz.
    pub const REFERENCE_HZ: f32 = 261.625_58;
    /// MIDI note number of pitch 0.0.
    pub const REFERENCE_NOTE: f32 = 60.0;
    /// Pitch units per octave.
    pub const PER_OCTAVE: f32 = 0.1;
    /// Pitch units per semitone.
    pub const PER_SEMITONE: f32 = PER_OCTAVE / 12.0;

    /// Converts a pitch value to Hz.
    #[inline]
    pub fn to_hz(pitch: f32) -> f32 {
        REFERENCE_HZ * (pitch / PER_OCTAVE).exp2()
    }

    /// Converts Hz to a pitch value.
    #[inline]
    pub fn from_hz(hz: f32) -> f32 {
        (hz / REFERENCE_HZ).log2() * PER_OCTAVE
    }

    /// Converts a MIDI note number to a pitch value.
    #[inline]
    pub fn from_midi_note(note: f32) -> f32 {
        (note - REFERENCE_NOTE) * PER_SEMITONE
    }

    /// Converts a pitch value to a (fractional) MIDI note number.
    #[inline]
    pub fn to_midi_note(pitch: f32) -> f32 {
        pitch / PER_SEMITONE + REFERENCE_NOTE
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reference_points() {
            assert!((to_hz(0.0) - REFERENCE_HZ).abs() < 1e-3);
            assert!((to_hz(0.1) - 2.0 * REFERENCE_HZ).abs() < 1e-2);
            // A4 = MIDI 69 = 440 Hz.
            assert!((to_hz(from_midi_note(69.0)) - 440.0).abs() < 1e-2);
            assert!((to_midi_note(from_hz(440.0)) - 69.0).abs() < 1e-3);
        }
    }
}
