//! MIDI 1.0 MPE zone configuration and pitch bend sensitivity (RPN 6 and 0).
//! The decoder is shared by host ingress and the sample-accurate engine timeline.
//! Reference: MIDI Association, MIDI Polyphonic Expression, RP-053 (2018).
//! <https://d30pueezughrda.cloudfront.net/campaigns/mpe/mpespec.pdf>.

use crate::{MAX_MIDI_INPUTS, MidiEvent, MidiMessage};

/// Configuration of one physical input. Channels here are zero-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiInputConfig {
    /// Number of member channels above channel 1.
    pub lower: u8,
    /// Number of member channels below channel 16.
    pub upper: u8,
    /// Changes when an input is closed or opened under a reused identity.
    pub generation: u64,
    rpn: [[u8; 2]; 16],
    semitones: [u8; 16],
    cents: [u8; 16],
}

impl Default for MidiInputConfig {
    fn default() -> Self {
        Self {
            lower: 0,
            upper: 0,
            generation: 0,
            rpn: [[127; 2]; 16],
            semitones: [2; 16],
            cents: [0; 16],
        }
    }
}

impl MidiInputConfig {
    /// Manager of this channel's active zone, including the manager itself.
    pub fn manager(&self, channel: u8) -> Option<u8> {
        if self.lower > 0 && channel <= self.lower {
            Some(0)
        } else if self.upper > 0 && channel >= 15 - self.upper {
            Some(15)
        } else {
            None
        }
    }

    /// A module's channel filter, including an announced manager's zone.
    pub fn accepts(&self, listen: u8, channel: u8) -> bool {
        listen == 0
            || listen == channel + 1
            || (listen == 1 && self.manager(channel) == Some(0))
            || (listen == 16 && self.manager(channel) == Some(15))
    }

    /// Bend sensitivity in semitones; ordinary MIDI retains the module's range.
    pub fn bend_range(&self, channel: u8, ordinary: f32) -> f32 {
        if self.manager(channel).is_none() {
            return ordinary;
        }
        let i = usize::from(channel);
        f32::from(self.semitones[i]) + f32::from(self.cents[i]) / 100.0
    }

    /// Applies a channel's data-entry byte to its selected RPN.
    fn data(&mut self, channel: u8, controller: u8, value: u8) {
        let index = usize::from(channel);
        match self.rpn[index] {
            [0, 6] if controller == 6 && matches!(channel, 0 | 15) => {
                let members = value.min(15);
                if channel == 0 {
                    self.lower = members;
                    if members == 15 {
                        self.upper = 0;
                    } else if members > 0 {
                        self.upper = self.upper.min(14 - members);
                    }
                } else {
                    self.upper = members;
                    if members == 15 {
                        self.lower = 0;
                    } else if members > 0 {
                        self.lower = self.lower.min(14 - members);
                    }
                }
                // An announcement restores the MPE bend defaults for this zone.
                if members > 0 {
                    self.semitones[index] = 2;
                    self.cents[index] = 0;
                    for ch in 0..16u8 {
                        if ch != channel && self.manager(ch) == Some(channel) {
                            self.semitones[usize::from(ch)] = 48;
                            self.cents[usize::from(ch)] = 0;
                        }
                    }
                }
            }
            [0, 0] => {
                let manager = self.manager(channel);
                for ch in 0..16u8 {
                    if ch == channel || manager.is_some_and(|m| channel != m && ch != m && self.manager(ch) == Some(m))
                    {
                        if controller == 6 {
                            self.semitones[usize::from(ch)] = value;
                        } else {
                            self.cents[usize::from(ch)] = value.min(99);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Follows RPN selection and data entry; NRPN selection cancels RPN selection.
    pub fn follow(&mut self, message: MidiMessage) {
        if let MidiMessage::ControlChange {
            channel,
            controller,
            value,
        } = message
        {
            let channel = channel & 15;
            match controller {
                101 => self.rpn[usize::from(channel)][0] = value,
                100 => self.rpn[usize::from(channel)][1] = value,
                98 | 99 => self.rpn[usize::from(channel)] = [127; 2],
                6 | 38 => self.data(channel, controller, value),
                _ => {}
            }
        }
    }
}

/// Per-input configuration retained by the rig across patch sessions.
#[derive(Clone, Debug)]
pub struct MidiConfigTable {
    inputs: Box<[MidiInputConfig]>,
}

impl Default for MidiConfigTable {
    fn default() -> Self {
        Self {
            inputs: vec![MidiInputConfig::default(); usize::from(MAX_MIDI_INPUTS)].into_boxed_slice(),
        }
    }
}

impl MidiConfigTable {
    /// Current configuration, with untracked input 0 always ordinary MIDI.
    pub fn input(&self, input: u16) -> MidiInputConfig {
        self.inputs.get(usize::from(input)).copied().unwrap_or_default()
    }

    /// Forgets a disconnected input's announcements and partial RPN selection.
    pub fn clear(&mut self, input: u16) {
        if input > 0
            && let Some(config) = self.inputs.get_mut(usize::from(input))
        {
            *config = MidiInputConfig {
                generation: config.generation.wrapping_add(1),
                ..MidiInputConfig::default()
            };
        }
    }

    /// Follows an accepted ingress event. Input 0 never announces a zone.
    pub fn follow(&mut self, event: &MidiEvent) {
        if event.input > 0
            && let Some(config) = self.inputs.get_mut(usize::from(event.input))
        {
            config.follow(event.message());
        }
    }
}

/// One configuration change at an absolute frame within the current MIDI buffer.
struct Change {
    frame: usize,
    input: u16,
    config: MidiInputConfig,
}

/// Engine-owned configuration, including earlier values within a buffer.
/// Storage is allocated once; processing never allocates.
pub struct MidiConfigTimeline {
    current: MidiConfigTable,
    start: MidiConfigTable,
    changes: Vec<Change>,
}

impl MidiConfigTimeline {
    /// Preallocates room for every possible event in one buffer.
    pub fn new(capacity: usize) -> Self {
        Self {
            current: MidiConfigTable::default(),
            start: MidiConfigTable::default(),
            changes: Vec::with_capacity(capacity),
        }
    }

    /// Installs a rig snapshot, leaving the previous table for control-thread disposal.
    pub fn swap(&mut self, table: &mut MidiConfigTable) {
        std::mem::swap(&mut self.current, table);
    }

    /// Clears a closed or newly opened input's zone state.
    pub fn clear(&mut self, input: u16) {
        self.current.clear(input);
    }

    /// Builds the timeline from events already in stable frame order.
    pub fn begin_block(&mut self, events: &[MidiEvent]) {
        self.start.inputs.copy_from_slice(&self.current.inputs);
        self.changes.clear();
        for event in events {
            let before = self.current.input(event.input);
            self.current.follow(event);
            let config = self.current.input(event.input);
            if config != before && self.changes.len() < self.changes.capacity() {
                self.changes.push(Change {
                    frame: event.frame as usize,
                    input: event.input,
                    config,
                });
            }
        }
    }

    /// Configuration at `frame`, including changes on that sample only.
    pub fn input_at(&self, input: u16, frame: usize) -> MidiInputConfig {
        self.changes
            .iter()
            .rev()
            .find(|change| change.input == input && change.frame <= frame)
            .map_or_else(|| self.start.input(input), |change| change.config)
    }
}

#[cfg(test)]
mod tests;
