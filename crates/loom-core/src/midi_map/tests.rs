use super::*;

#[test]
fn feedback_round_trips() {
    let table = MidiFeedback::new();
    let control = MidiControl {
        channel: 15,
        controller: 127,
    };
    assert_eq!(table.read(control).id, MidiMappingId(0));
    table.write(control, MidiMappingId(9), 0x0123_4567, 100);
    assert_eq!(
        table.read(control),
        FeedbackEntry {
            id: MidiMappingId(9),
            sequence: 0x23_4567,
            value: 100
        }
    );
}

#[test]
fn sequences_compare_across_the_wrap() {
    assert!(later(5, 4));
    assert!(!later(4, 5));
    assert!(!later(4, 4));
    assert!(later(1, 0x00FF_FFFF));
    assert!(!later(0x00FF_FFFF, 1));
}

/// The last note on reads back through a reader, with its serial.
#[test]
fn last_note_on_round_trips() {
    let table = Arc::new(MidiFeedback::new());
    let reader = MidiReader::new(table.clone());
    assert_eq!(reader.last_note_on(), None);
    table.record_note(9, 36, 5);
    assert_eq!(
        reader.last_note_on(),
        Some(NoteOnSeen {
            channel: 9,
            note: 36,
            serial: 5
        })
    );
}

#[test]
fn last_control_change_round_trips() {
    let table = MidiFeedback::new();
    assert_eq!(table.last(), None);
    let control = MidiControl {
        channel: 3,
        controller: 74,
    };
    table.record(control, 64, 7);
    assert_eq!(
        table.last(),
        Some(ControlChangeSeen {
            control,
            value: 64,
            serial: 7
        })
    );
}

#[test]
fn shows_channels_from_one() {
    let control = MidiControl {
        channel: 0,
        controller: 74,
    };
    assert_eq!(control.to_string(), "CC 74 · ch 1");
}
