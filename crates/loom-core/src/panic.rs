//! Panic: silence everything and clear stuck notes, without a click.
//!
//! Spec: `docs/design/panic.md`. The engine runs it from one command or
//! from a MIDI trigger: the output fades to silence, every module is reset
//! at the frame where the fade reaches 0, and the output fades back in.
//! This file holds the trigger and the state machine; the engine does the
//! work. Nothing here allocates.

use crate::midi::MidiMessage;

/// A MIDI message that runs panic, set in the player's preferences, not in
/// the patch (panic.md, MIDI trigger).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanicTrigger {
    /// A control change on `channel` (0 to 15) and `controller` (0 to
    /// 127). It fires when the value rises through 64.
    Cc {
        /// Channel, 0 to 15.
        channel: u8,
        /// Controller number, 0 to 127.
        controller: u8,
    },
    /// A note on (velocity above 0) on `channel` (0 to 15) and `note` (0 to
    /// 127).
    Note {
        /// Channel, 0 to 15.
        channel: u8,
        /// Note number, 0 to 127.
        note: u8,
    },
}

impl PanicTrigger {
    /// The trigger a message would set in learn mode: a control change or
    /// a note on. Other messages set nothing.
    pub fn learn(message: MidiMessage) -> Option<Self> {
        match message {
            MidiMessage::ControlChange {
                channel, controller, ..
            } => Some(Self::Cc { channel, controller }),
            MidiMessage::NoteOn { channel, note, .. } => Some(Self::Note { channel, note }),
            _ => None,
        }
    }

    /// True if the trigger and its channel, controller, and note numbers
    /// are in range.
    pub fn is_valid(self) -> bool {
        match self {
            Self::Cc { channel, controller } => channel < 16 && controller < 128,
            Self::Note { channel, note } => channel < 16 && note < 128,
        }
    }
}

impl std::fmt::Display for PanicTrigger {
    /// As the player reads it, with channels from 1: "Channel 1,
    /// controller 123" or "Channel 10, note 36".
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Cc { channel, controller } => write!(f, "Channel {}, controller {controller}", channel + 1),
            Self::Note { channel, note } => write!(f, "Channel {}, note {note}", channel + 1),
        }
    }
}

/// Watches the MIDI stream for the panic trigger.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TriggerWatch {
    trigger: Option<PanicTrigger>,
    /// The last value of the trigger control was 64 or more.
    high: bool,
}

impl TriggerWatch {
    /// Sets the trigger. The control starts low, so its first value at 64
    /// or more fires.
    pub(crate) fn set(&mut self, trigger: Option<PanicTrigger>) {
        self.trigger = trigger;
        self.high = false;
    }

    /// True if `message` fires the trigger: a note on for a note trigger,
    /// or a control value rising through 64 for a control trigger.
    pub(crate) fn fires(&mut self, message: MidiMessage) -> bool {
        match (self.trigger, message) {
            (
                Some(PanicTrigger::Cc { channel, controller }),
                MidiMessage::ControlChange {
                    channel: c,
                    controller: n,
                    value,
                },
            ) if c == channel && n == controller => {
                let high = value >= 64;
                let rose = high && !self.high;
                self.high = high;
                rose
            }
            (
                Some(PanicTrigger::Note { channel, note }),
                MidiMessage::NoteOn {
                    channel: c, note: n, ..
                },
            ) => c == channel && n == note,
            _ => false,
        }
    }
}

/// Where a panic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PanicState {
    /// No panic running.
    #[default]
    Idle,
    /// Fading out. The reset happens after this many more frames.
    Fading(usize),
    /// Reset done, fading back in. A new panic is ignored until the fade-in
    /// is complete (panic.md, rule 3).
    Rising,
}

#[cfg(test)]
mod tests {
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
}
