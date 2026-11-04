use super::*;

#[test]
fn feedback_round_trips() {
    let table = MidiFeedback::new();
    let control = MidiControl {
        channel: 15,
        controller: 127,
    };
    assert_eq!(table.read(control).id, MidiMappingId(0));
    table.write(control, MidiMappingId(9), 0x0123_4567, 100, 0.25);
    assert_eq!(
        table.read(control),
        FeedbackEntry {
            id: MidiMappingId(9),
            sequence: 0x23_4567,
            value: 100,
            waiting: false,
        }
    );
    assert_eq!(table.result(control), 0.25);

    // A waiting pickup reports its hardware, and leaves the result alone.
    table.write_waiting(control, MidiMappingId(9), 5, 20);
    let entry = table.read(control);
    assert!(entry.waiting);
    assert_eq!((entry.sequence, entry.value), (5, 20));
    assert_eq!(table.result(control), 0.25);
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

/// A 0 to 1 parameter that is linear, so a position is its value.
const UNIT: ParamSpec = ParamSpec {
    name: "unit",
    label: "Unit",
    description: "A test parameter.",
    min: 0.0,
    max: 1.0,
    default: 0.0,
    unit: "",
    options: crate::ParamOptions::DEFAULT,
};

/// Options in a mode, over the whole travel.
fn in_mode(mode: MidiMode) -> MidiMappingOptions {
    MidiMappingOptions {
        mode,
        ..MidiMappingOptions::DEFAULT
    }
}

/// The value a control change set, if it set one.
fn set(outcome: ControlOutcome) -> f32 {
    match outcome {
        ControlOutcome::Set(value) => value,
        other => panic!("expected a value, got {other:?}"),
    }
}

/// A pickup waits while the hardware stays on one side, and catches when the
/// hardware crosses the parameter, then follows.
#[test]
fn a_pickup_catches_on_a_crossing() {
    let options = in_mode(MidiMode::Pickup);
    let mut state = ControlState::seeded(MidiMappingId(1), &UNIT, 0.5, true);
    assert_eq!(state.control_change(&options, &UNIT, 127), ControlOutcome::Waiting);
    assert_eq!(state.control_change(&options, &UNIT, 90), ControlOutcome::Waiting);
    assert!((set(state.control_change(&options, &UNIT, 30)) - 30.0 / 127.0).abs() < 1e-6);
    assert!(!state.waiting);
    assert!((set(state.control_change(&options, &UNIT, 100)) - 100.0 / 127.0).abs() < 1e-6);
}

/// With no earlier hardware position there is nothing to cross, so the first
/// control change catches only within one step (midi-learn.md).
#[test]
fn a_first_control_change_catches_only_within_one_step() {
    let options = in_mode(MidiMode::Pickup);
    let mut far = ControlState::seeded(MidiMappingId(1), &UNIT, 0.5, true);
    assert_eq!(far.control_change(&options, &UNIT, 0), ControlOutcome::Waiting);
    let mut near = ControlState::seeded(MidiMappingId(1), &UNIT, 0.5, true);
    assert!(matches!(near.control_change(&options, &UNIT, 64), ControlOutcome::Set(_)));
}

/// One step stays 1/127 of position on a narrow range, so a step on the
/// narrow range does not catch from further away than that.
#[test]
fn one_step_is_the_same_on_a_narrow_range() {
    let options = MidiMappingOptions {
        mode: MidiMode::Pickup,
        low: 0.5,
        high: 0.51,
        invert: false,
    };
    let mut state = ControlState::seeded(MidiMappingId(1), &UNIT, 0.505, true);
    assert!(matches!(state.control_change(&options, &UNIT, 0), ControlOutcome::Set(_)));
}

/// A parameter outside the span is caught at the span's nearest end, because
/// the hardware can never cross it.
#[test]
fn a_parameter_outside_the_span_is_caught_at_the_nearest_end() {
    let options = MidiMappingOptions {
        mode: MidiMode::Pickup,
        low: 0.2,
        high: 0.8,
        invert: false,
    };
    let mut state = ControlState::seeded(MidiMappingId(1), &UNIT, 0.9, true);
    assert_eq!(state.control_change(&options, &UNIT, 60), ControlOutcome::Waiting);
    assert!((set(state.control_change(&options, &UNIT, 127)) - 0.8).abs() < 1e-6);
}

/// A screen edit makes a pickup wait again, and it catches from the last
/// hardware position, which is still where the hardware stands.
#[test]
fn a_change_from_elsewhere_makes_a_pickup_wait() {
    let options = in_mode(MidiMode::Pickup);
    let mut state = ControlState::seeded(MidiMappingId(1), &UNIT, 0.0, false);
    set(state.control_change(&options, &UNIT, 64));
    state.changed_elsewhere(&options, &UNIT, 0.9);
    assert_eq!(state.control_change(&options, &UNIT, 70), ControlOutcome::Waiting);
    assert!(matches!(state.control_change(&options, &UNIT, 127), ControlOutcome::Set(_)));
}

/// Relative steps by 1/128 from the current position, stops at the ends, and
/// a value with no step does nothing.
#[test]
fn relative_steps_and_stops_at_the_ends() {
    let options = in_mode(MidiMode::Relative(MidiEncoding::TwosComplement));
    let mut state = ControlState::seeded(MidiMappingId(1), &UNIT, 0.5, false);
    assert_eq!(set(state.control_change(&options, &UNIT, 1)), 0.5 + 1.0 / 128.0);
    assert_eq!(set(state.control_change(&options, &UNIT, 127)), 0.5);
    assert_eq!(state.control_change(&options, &UNIT, 64), ControlOutcome::Nothing);
    assert_eq!(state.control_change(&options, &UNIT, 0), ControlOutcome::Nothing);
    for _ in 0..4 {
        set(state.control_change(&options, &UNIT, 63));
    }
    assert_eq!(state.position, 1.0);
    for _ in 0..4 {
        set(state.control_change(&options, &UNIT, 65));
    }
    assert_eq!(state.position, 0.0);
}

/// Small steps add up on a whole number, though each one rounds back to the
/// same value, so an encoder never stays stuck.
#[test]
fn relative_small_steps_add_up_on_a_whole_number() {
    let whole = ParamSpec {
        max: 3.0,
        ..UNIT
    }
    .integer();
    let options = in_mode(MidiMode::Relative(MidiEncoding::Offset64));
    let mut state = ControlState::seeded(MidiMappingId(1), &whole, 0.0, false);
    let mut last = 0.0;
    for _ in 0..128 {
        last = set(state.control_change(&options, &whole, 65));
    }
    assert_eq!(last, 3.0);
}
