use super::*;

/// The clock encodes each message in its MIDI 1.0 wire form.
#[test]
fn clock_messages_round_trip() {
    for (wire, decoded) in [
        (ClockMessage::Pulse, MidiMessage::TimingClock),
        (ClockMessage::Start, MidiMessage::Start),
        (ClockMessage::Continue, MidiMessage::Continue),
        (ClockMessage::Stop, MidiMessage::Stop),
        (ClockMessage::at_position(9999.0), MidiMessage::SongPosition(16_383)),
        (ClockMessage::at_position(1.49), MidiMessage::SongPosition(5)),
    ] {
        assert_eq!(
            wire.encode(|bytes| {
                let mut data = [0; 3];
                data[..bytes.len()].copy_from_slice(bytes);
                decode(data, bytes.len() as u8)
            }),
            decoded
        );
    }
}

/// An estimate includes identical arrivals and drops intervals outside one beat.
#[test]
fn interval_history() {
    let mut history = PulseHistory::default();
    assert_eq!(history.pulse(0), None);
    assert_eq!(history.pulse(0), None);
    assert_eq!(history.pulse(20), Some(10.0));
    for pulse in 2..=30 {
        assert!(history.pulse(pulse * 20).is_some());
    }
    assert_eq!(history.pulse(620), Some(20.0));
}
