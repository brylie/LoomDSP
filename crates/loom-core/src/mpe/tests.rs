use super::*;

/// RPN selection and data entry, optionally split across engine attachments.
fn rpn(input: u16, channel: u8, number: u8, value: u8, frame: u32) -> [MidiEvent; 3] {
    [
        MidiEvent::control_change(frame, channel, 101, 0),
        MidiEvent::control_change(frame, channel, 100, number),
        MidiEvent::control_change(frame, channel, 6, value),
    ]
    .map(|event| event.from_input(crate::MidiInputId(input)))
}

/// Inputs and untracked sources cannot announce zones for each other.
#[test]
fn zones_are_per_input_and_zero_is_ordinary() {
    let mut table = MidiConfigTable::default();
    for event in rpn(1, 0, 6, 7, 0)
        .into_iter()
        .chain(rpn(2, 15, 6, 3, 0))
        .chain(rpn(0, 0, 6, 15, 0))
    {
        table.follow(&event);
    }
    assert_eq!(table.input(1).manager(7), Some(0));
    assert_eq!(table.input(1).manager(8), None);
    assert_eq!(table.input(2).manager(12), Some(15));
    assert_eq!(table.input(0).manager(1), None);
    assert!(table.input(1).accepts(1, 5));
    assert!(!table.input(1).accepts(2, 5));
    assert_eq!(table.input(1).bend_range(1, 2.0), 48.0);
    assert_eq!(table.input(1).bend_range(0, 12.0), 2.0);
    assert_eq!(table.input(1).bend_range(9, 12.0), 12.0);
    table.clear(1);
    assert_eq!(table.input(1).manager(1), None);
    assert_eq!(table.input(1).generation, 1);
}

/// The latest zone wins overlaps; disabling one does not resize the other.
#[test]
fn conflicting_zones_and_disable() {
    let mut table = MidiConfigTable::default();
    for event in rpn(1, 0, 6, 10, 0).into_iter().chain(rpn(1, 15, 6, 8, 0)) {
        table.follow(&event);
    }
    assert_eq!((table.input(1).lower, table.input(1).upper), (6, 8));
    for event in rpn(1, 0, 6, 15, 0).into_iter().chain(rpn(1, 15, 6, 0, 0)) {
        table.follow(&event);
    }
    assert_eq!((table.input(1).lower, table.input(1).upper), (15, 0));
}

/// A member's sensitivity reaches the zone's members but leaves its manager alone.
#[test]
fn sensitivity_and_partial_rpn_survive_snapshot() {
    let mut table = MidiConfigTable::default();
    for event in rpn(1, 0, 6, 4, 0).into_iter().chain(rpn(1, 2, 0, 12, 0)) {
        table.follow(&event);
    }
    table.follow(&MidiEvent::control_change(0, 2, 38, 50).from_input(crate::MidiInputId(1)));
    assert_eq!(table.input(1).bend_range(4, 2.0), 12.5);
    assert_eq!(table.input(1).bend_range(0, 2.0), 2.0);
    table.follow(&MidiEvent::control_change(0, 0, 101, 0).from_input(crate::MidiInputId(2)));
    let mut copy = table.clone();
    for event in rpn(2, 0, 6, 3, 0).into_iter().skip(1) {
        copy.follow(&event);
    }
    assert_eq!(copy.input(2).lower, 3);
    copy.follow(&MidiEvent::control_change(0, 0, 99, 0).from_input(crate::MidiInputId(2)));
    copy.follow(&MidiEvent::control_change(0, 0, 6, 9).from_input(crate::MidiInputId(2)));
    assert_eq!(copy.input(2).lower, 3, "NRPN must not write the last selected RPN");
}

/// A later announcement must not alter the configuration seen by an earlier sample.
#[test]
fn timeline_keeps_sample_offsets_and_block_history() {
    let mut timeline = MidiConfigTimeline::new(16);
    timeline.begin_block(&rpn(1, 0, 6, 4, 90));
    assert_eq!(timeline.input_at(1, 89).lower, 0);
    assert_eq!(timeline.input_at(1, 90).lower, 4);
    timeline.begin_block(&[]);
    assert_eq!(timeline.input_at(1, 0).lower, 4);
    let ctx = crate::ProcessContext::new(48_000.0, 10, &[]).with_midi_config(&timeline);
    assert_eq!(ctx.midi_config(1, 0).lower, 4);
}
