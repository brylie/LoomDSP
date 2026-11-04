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

/// Each encoding reads its steps as the spec's table says, and the values
/// the spec leaves without a meaning are no step.
#[test]
fn encodings_read_steps() {
    use MidiEncoding::{Offset64, SignBit, TwosComplement};
    for (encoding, value, step) in [
        (Offset64, 64, 0),
        (Offset64, 65, 1),
        (Offset64, 127, 63),
        (Offset64, 63, -1),
        (Offset64, 0, -64),
        (TwosComplement, 1, 1),
        (TwosComplement, 63, 63),
        (TwosComplement, 127, -1),
        (TwosComplement, 65, -63),
        (TwosComplement, 0, 0),
        (TwosComplement, 64, 0),
        (SignBit, 1, 1),
        (SignBit, 63, 63),
        (SignBit, 65, -1),
        (SignBit, 127, -63),
        (SignBit, 0, 0),
        (SignBit, 64, 0),
    ] {
        assert_eq!(encoding.step(value), step, "{encoding:?} {value}");
    }
}

/// A span with its ends, inverted or not.
fn span(low: f32, high: f32, invert: bool) -> MidiMappingOptions {
    MidiMappingOptions {
        low,
        high,
        invert,
        ..MidiMappingOptions::DEFAULT
    }
}

/// The control's 0 to 127 covers the span, and Invert swaps its ends.
#[test]
fn a_span_sets_where_the_control_lands() {
    let default = MidiMappingOptions::DEFAULT;
    assert_eq!(default.position(0), 0.0);
    assert_eq!(default.position(127), 1.0);
    let middle = span(0.2, 0.8, false);
    assert!((middle.position(0) - 0.2).abs() < 1e-6);
    assert!((middle.position(127) - 0.8).abs() < 1e-6);
    let inverted = span(0.2, 0.8, true);
    assert!((inverted.position(0) - 0.8).abs() < 1e-6);
    assert!((inverted.position(127) - 0.2).abs() < 1e-6);
}

/// Bad options are refused with a reason, NaN included, and a move to
/// Relative resets what Relative does not have, so it stays valid.
#[test]
fn options_are_checked_in_one_place() {
    assert!(MidiMappingOptions::DEFAULT.check().is_ok());
    assert!(span(0.2, 0.8, true).check().is_ok());
    assert!(span(0.8, 0.2, false).check().is_err());
    assert!(span(0.5, 0.5, false).check().is_err());
    assert!(span(-0.1, 0.5, false).check().is_err());
    assert!(span(0.5, 1.1, false).check().is_err());
    assert!(span(f32::NAN, 0.5, false).check().is_err());
    assert!(span(0.5, f32::NAN, false).check().is_err());
    let relative = MidiMode::Relative(MidiEncoding::SignBit);
    let bad = MidiMappingOptions {
        mode: relative,
        ..span(0.2, 0.8, false)
    };
    assert!(bad.check().is_err());
    let moved = span(0.2, 0.8, true).with_mode(relative);
    assert!(moved.check().is_ok());
    assert_eq!((moved.low, moved.high, moved.invert), (0.0, 1.0, false));
    assert_eq!(span(0.2, 0.8, true).with_mode(MidiMode::Pickup).low, 0.2);
}
