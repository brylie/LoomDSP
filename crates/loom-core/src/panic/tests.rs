use super::*;

/// A control trigger fires once as the value rises through 64, and a
/// value of 0 does nothing.
#[test]
fn a_control_fires_when_it_rises_through_64() {
    let mut watch = TriggerWatch::default();
    let cc = |value| MidiMessage::ControlChange {
        channel: 0,
        controller: 123,
        value,
    };
    assert!(!watch.fires(cc(127)), "no trigger set");
    watch.set(Some(PanicTrigger::Cc {
        channel: 0,
        controller: 123,
    }));
    assert!(!watch.fires(cc(0)));
    assert!(watch.fires(cc(127)));
    assert!(!watch.fires(cc(100)), "still high");
    assert!(!watch.fires(cc(10)));
    assert!(watch.fires(cc(64)));
    let other = MidiMessage::ControlChange {
        channel: 1,
        controller: 123,
        value: 0,
    };
    assert!(!watch.fires(other));
}

/// A note trigger fires on its note on only.
#[test]
fn a_note_fires_on_note_on() {
    let mut watch = TriggerWatch::default();
    watch.set(Some(PanicTrigger::Note { channel: 9, note: 36 }));
    let on = MidiMessage::NoteOn {
        channel: 9,
        note: 36,
        velocity: 100,
    };
    assert!(watch.fires(on));
    assert!(!watch.fires(MidiMessage::NoteOff { channel: 9, note: 36 }));
    let elsewhere = MidiMessage::NoteOn {
        channel: 0,
        note: 36,
        velocity: 100,
    };
    assert!(!watch.fires(elsewhere));
}

/// Learn takes a control change or a note on, and the text counts
/// channels from 1.
#[test]
fn learning_and_text() {
    let cc = MidiMessage::ControlChange {
        channel: 0,
        controller: 123,
        value: 5,
    };
    let trigger = PanicTrigger::learn(cc).unwrap();
    assert_eq!(trigger.to_string(), "Channel 1, controller 123");
    let note = MidiMessage::NoteOn {
        channel: 9,
        note: 36,
        velocity: 1,
    };
    assert_eq!(PanicTrigger::learn(note).unwrap().to_string(), "Channel 10, note 36");
    assert!(PanicTrigger::learn(MidiMessage::NoteOff { channel: 0, note: 1 }).is_none());
    assert!(trigger.is_valid());
    assert!(!PanicTrigger::Note { channel: 16, note: 0 }.is_valid());
}
