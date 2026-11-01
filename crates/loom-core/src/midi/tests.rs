use super::*;

#[test]
fn decodes_notes() {
    assert_eq!(
        MidiEvent::note_on(0, 1, 60, 100).message(),
        MidiMessage::NoteOn {
            channel: 1,
            note: 60,
            velocity: 100
        }
    );
    assert_eq!(
        MidiEvent::note_on(0, 0, 60, 0).message(),
        MidiMessage::NoteOff { channel: 0, note: 60 }
    );
    assert_eq!(
        MidiEvent::from_bytes(0, &[0xE0, 0x00, 0x40]).message(),
        MidiMessage::PitchBend { channel: 0, value: 0 }
    );
}

/// The pitch bend and channel pressure helpers round-trip through
/// `message`.
#[test]
fn decodes_bend_and_pressure() {
    for value in [-8192, -1, 0, 1, 8191] {
        assert_eq!(
            MidiEvent::pitch_bend(0, 3, value).message(),
            MidiMessage::PitchBend { channel: 3, value }
        );
    }
    assert_eq!(
        MidiEvent::channel_pressure(0, 2, 90).message(),
        MidiMessage::ChannelPressure { channel: 2, value: 90 }
    );
    assert_eq!(MidiEvent::from_bytes(0, &[0xD0]).message(), MidiMessage::Other);
}
