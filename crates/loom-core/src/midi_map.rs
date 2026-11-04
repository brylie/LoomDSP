//! MIDI learn: control changes that set parameters.
//!
//! Spec: `docs/design/midi-learn.md`.
//!
//! The engine applies mappings (rule 1) and reports back through tables of
//! atomics, allocated once with the engine (rules 2 and 3). There is one
//! entry per MIDI control, 16 channels times 128 controller numbers, because
//! a control has at most one mapping. Each entry holds the id of the mapping
//! that wrote it, a sequence number, the 7-bit value, and whether a pickup is
//! waiting. A second table holds the exact parameter value the control change
//! set, because an encoder's byte is a step and cannot be turned into a value
//! again. The controller applies an entry only when the id matches, the
//! sequence number moved, and no pickup is waiting, so a write from a mapping
//! that no longer exists is never applied, and the same value sent twice is
//! still seen twice.
//!
//! The engine also keeps each control's [`ControlState`]: the position a
//! pickup compares with and a relative step starts from.
//!
//! Screen edits and control changes can race: the controller changes the
//! graph at once, but the engine applies the edit at its next buffer. So a
//! screen edit on a mapped parameter carries an edit token, and the engine
//! stores that token with its own sequence number in a second table when it
//! applies the edit. Control changes and edits share one sequence, so the
//! controller can tell which came last in the engine (MIDI learn, rule 2).

use crate::descriptor::ParamSpec;
use crate::graph::ModuleId;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Number of MIDI channels.
const CHANNELS: usize = 16;
/// Number of controller numbers on a channel.
const CONTROLLERS: usize = 128;
/// Number of MIDI controls: one entry each in the engine's tables.
pub(crate) const MIDI_CONTROLS: usize = CHANNELS * CONTROLLERS;

/// One hardware control: a MIDI channel and a controller number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MidiControl {
    /// Channel, 0 to 15. Shown to users as 1 to 16.
    pub channel: u8,
    /// Controller number, 0 to 127.
    pub controller: u8,
}

impl MidiControl {
    /// True when the channel and controller number are in range.
    pub fn is_valid(self) -> bool {
        usize::from(self.channel) < CHANNELS && usize::from(self.controller) < CONTROLLERS
    }

    /// Index into the feedback and state tables. Only for valid controls.
    pub(crate) fn index(self) -> usize {
        usize::from(self.channel) * CONTROLLERS + usize::from(self.controller)
    }
}

impl fmt::Display for MidiControl {
    /// As the inspector shows it, with channels counted from 1.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CC {} · ch {}", self.controller, u16::from(self.channel) + 1)
    }
}

/// Stable identifier of one MIDI mapping, for this session only. Patch files
/// identify a mapping by its control and target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MidiMappingId(pub u32);

/// How a relative encoder writes a step in its 7-bit value (spec, Mapping
/// modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MidiEncoding {
    /// 65 and up is up by value − 64, 63 and below is down by 64 − value.
    #[default]
    Offset64,
    /// 1 to 63 is up, 127 down to 65 is −1 to −63. 0 and 64 are no step.
    TwosComplement,
    /// 1 to 63 is up, 65 to 127 is down by value − 64. 0 and 64 are no step.
    SignBit,
}

impl MidiEncoding {
    /// The step a control change's value stands for, in steps up or down.
    pub fn step(self, value: u8) -> i32 {
        let value = i32::from(value & 0x7F);
        match self {
            Self::Offset64 => value - 64,
            Self::TwosComplement => match value {
                0 | 64 => 0,
                1..=63 => value,
                _ => value - 128,
            },
            Self::SignBit => match value {
                0 | 64 => 0,
                1..=63 => value,
                _ => 64 - value,
            },
        }
    }
}

/// What a control change does to its parameter (spec, Mapping modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MidiMode {
    /// Sets the position.
    #[default]
    Absolute,
    /// Ignored until the control reaches the parameter's position, then as
    /// Absolute until the parameter changes from somewhere else.
    Pickup,
    /// Reads the value as a step up or down.
    Relative(MidiEncoding),
}

/// The options of one mapping: its mode, and the span of the parameter's
/// position that the control's 0 to 127 covers. `low` and `high` are in the
/// parameter's selected control range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidiMappingOptions {
    /// What a control change does.
    pub mode: MidiMode,
    /// The position at the control's 0, before [`Self::invert`].
    pub low: f32,
    /// The position at the control's 127, before [`Self::invert`].
    pub high: f32,
    /// Swaps the ends of the span.
    pub invert: bool,
}

impl MidiMappingOptions {
    /// Absolute, over the whole travel, not inverted.
    pub const DEFAULT: Self = Self {
        mode: MidiMode::Absolute,
        low: 0.0,
        high: 1.0,
        invert: false,
    };

    /// These options in another mode. Relative has no range or invert, so
    /// moving to it resets them, and the result always passes
    /// [`check`](Self::check) if these do.
    pub fn with_mode(self, mode: MidiMode) -> Self {
        match mode {
            MidiMode::Relative(_) => Self { mode, ..Self::DEFAULT },
            _ => Self { mode, ..self },
        }
    }

    /// Why these options cannot be used, if they cannot. The one check for
    /// edits and for loading a patch, so an edit can never make a patch that
    /// does not load.
    pub fn check(&self) -> Result<(), &'static str> {
        // Written so that NaN fails too.
        if !(self.low >= 0.0 && self.high <= 1.0) {
            return Err("its range must be from 0 to 1");
        }
        if !(self.low < self.high) {
            return Err("the low end of its range must be below the high end");
        }
        if matches!(self.mode, MidiMode::Relative(_)) && (self.low != 0.0 || self.high != 1.0 || self.invert) {
            return Err("a Relative mapping has no range or invert");
        }
        Ok(())
    }

    /// The position a control's value stands for in Absolute and Pickup.
    pub fn position(&self, value: u8) -> f32 {
        let t = f32::from(value.min(127)) / 127.0;
        let t = if self.invert { 1.0 - t } else { t };
        self.low + t * (self.high - self.low)
    }
}

impl Default for MidiMappingOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One link from a hardware control to a parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidiMapping {
    /// The hardware control.
    pub control: MidiControl,
    /// The module whose parameter it sets.
    pub target: ModuleId,
    /// Parameter index on [`Self::target`].
    pub param: usize,
    /// Its mode and range.
    pub options: MidiMappingOptions,
    /// True for a mapping made by learning: a Pickup then starts caught,
    /// because the control change that made it set the value. For this
    /// session only, so it is not saved.
    pub(crate) learned: bool,
}

/// A pickup that is waiting, and where its hardware stands, for the ghost mark
/// on its control (spec, Mapping modes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidiWaiting {
    /// The mapped module.
    pub target: ModuleId,
    /// Parameter index on [`Self::target`].
    pub param: usize,
    /// The position the hardware stands for, from 0 to 1.
    pub position: f32,
    /// Changes each time the hardware moves.
    pub sequence: u32,
}

/// The last control change the engine received, for learning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlChangeSeen {
    /// Which control moved.
    pub control: MidiControl,
    /// Its value, 0 to 127.
    pub value: u8,
    /// Goes up by one with each control change. Compare two readings to see
    /// whether a new one arrived in between.
    pub serial: u32,
}

/// The last note on the engine received, as the controller reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteOnSeen {
    /// Channel, 0 to 15.
    pub channel: u8,
    /// Note number, 0 to 127.
    pub note: u8,
    /// Counted with control changes: a higher serial came later.
    pub serial: u32,
}

/// What the engine knows about one mapped control between control changes.
/// One per control, in a table allocated once with the engine, so it never
/// allocates. Read and written on the audio thread, except that learning
/// starts a fresh one on the control thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ControlState {
    /// The mapping this state belongs to. A new id starts the state again.
    pub id: MidiMappingId,
    /// The parameter's position, not rounded for a whole number, so small
    /// relative steps add up.
    pub position: f32,
    /// The parameter's value at [`Self::position`]. A compiled base value
    /// that differs from it was changed from somewhere else.
    pub value: f32,
    /// True while a pickup waits for the hardware to reach the parameter.
    pub waiting: bool,
    /// The last position the hardware stood for, or `None` before its first
    /// control change. With none there is no crossing to detect.
    pub hardware: Option<f32>,
}

/// What a control change did to a [`ControlState`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ControlOutcome {
    /// The parameter takes this value.
    Set(f32),
    /// A pickup is still waiting. Only its ghost mark moves.
    Waiting,
    /// No step: nothing happened.
    Nothing,
}

impl ControlState {
    /// The state of no mapping. Mapping ids start at 1.
    pub const NONE: Self = Self {
        id: MidiMappingId(0),
        position: 0.0,
        value: 0.0,
        waiting: false,
        hardware: None,
    };

    /// A new mapping's state, with the parameter at `value`.
    pub fn seeded(id: MidiMappingId, spec: &ParamSpec, value: f32, waiting: bool) -> Self {
        Self {
            id,
            position: spec.normalize(value),
            value,
            waiting,
            hardware: None,
        }
    }

    /// The parameter changed from somewhere else, to `value`. A pickup waits
    /// again. The hardware has not moved, so its last position stays.
    pub fn changed_elsewhere(&mut self, options: &MidiMappingOptions, spec: &ParamSpec, value: f32) {
        self.position = spec.normalize(value);
        self.value = value;
        self.waiting = options.mode == MidiMode::Pickup;
    }

    /// Applies a control change's 7-bit `value`.
    pub fn control_change(&mut self, options: &MidiMappingOptions, spec: &ParamSpec, value: u8) -> ControlOutcome {
        let position = match options.mode {
            MidiMode::Absolute => options.position(value),
            MidiMode::Pickup => {
                let hardware = options.position(value);
                let previous = self.hardware.replace(hardware);
                if self.waiting {
                    // A parameter outside the span can never be crossed, so
                    // the nearest end of the span stands for it.
                    let target = self.position.clamp(options.low, options.high);
                    let near = (hardware - target).abs() <= 1.0 / 127.0;
                    let crossed = previous.is_some_and(|last| (last - target) * (hardware - target) <= 0.0);
                    if !near && !crossed {
                        return ControlOutcome::Waiting;
                    }
                    self.waiting = false;
                }
                hardware
            }
            MidiMode::Relative(encoding) => {
                let step = encoding.step(value);
                if step == 0 {
                    return ControlOutcome::Nothing;
                }
                (self.position + step as f32 / 128.0).clamp(0.0, 1.0)
            }
        };
        self.position = position;
        self.value = spec.denormalize(position);
        ControlOutcome::Set(self.value)
    }
}

/// One compiled mapping, as the engine reads it.
pub(crate) struct MidiMapOp {
    pub control: MidiControl,
    pub id: MidiMappingId,
    /// The feedback sequence number the graph held when this schedule was
    /// compiled. A newer one at the swap means a control change arrived in
    /// between, and the engine applies it again on the new schedule.
    pub synced: Option<u32>,
    pub slot: u32,
    pub param: u16,
    /// The parameter limited to its selected range, so a control change
    /// spans the range the knob does (control-ranges.md, rule 4).
    pub spec: ParamSpec,
    /// The mapping's mode and range.
    pub options: MidiMappingOptions,
    /// The parameter's value in the graph when this schedule was compiled.
    /// The engine has no other record of a value it handed to a module.
    pub base: f32,
    /// True for a pickup that starts caught, because learning made it.
    pub caught: bool,
}

/// What the engine reports to the controller. See the module docs.
pub(crate) struct MidiFeedback {
    /// `id << 32 | sequence << 8 | waiting << 7 | value`.
    entries: Box<[AtomicU64]>,
    /// The exact value the last applied control change set, as `f32` bits.
    values: Box<[AtomicU32]>,
    /// The last screen edit the engine applied, per control:
    /// `token << 32 | sequence`.
    edits: Box<[AtomicU64]>,
    /// `serial << 32 | channel << 16 | controller << 8 | value`. 0 is none.
    last: AtomicU64,
    /// The last note on: `serial << 32 | channel << 8 | note`. 0 is none.
    /// It shares the serial count with control changes, so the two can be
    /// put in order (panic learn).
    last_note: AtomicU64,
}

/// One feedback entry, unpacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FeedbackEntry {
    pub id: MidiMappingId,
    pub sequence: u32,
    pub value: u8,
    /// True when a pickup was waiting: this control change set nothing, so
    /// only a ghost mark may use it.
    pub waiting: bool,
}

/// The bit of a feedback entry that says a pickup is waiting.
const WAITING: u64 = 0x80;

impl MidiFeedback {
    pub fn new() -> Self {
        Self {
            entries: (0..CHANNELS * CONTROLLERS).map(|_| AtomicU64::new(0)).collect(),
            values: (0..CHANNELS * CONTROLLERS).map(|_| AtomicU32::new(0)).collect(),
            edits: (0..CHANNELS * CONTROLLERS).map(|_| AtomicU64::new(0)).collect(),
            last: AtomicU64::new(0),
            last_note: AtomicU64::new(0),
        }
    }

    /// Engine: a note on arrived. `serial` is never 0.
    pub fn record_note(&self, channel: u8, note: u8, serial: u32) {
        let packed = (u64::from(serial) << 32) | (u64::from(channel & 0x0F) << 8) | u64::from(note & 0x7F);
        self.last_note.store(packed, Ordering::Release);
    }

    /// Controller: the last note on, or `None` before the first.
    pub fn last_note(&self) -> Option<NoteOnSeen> {
        let packed = self.last_note.load(Ordering::Acquire);
        let serial = (packed >> 32) as u32;
        (serial != 0).then_some(NoteOnSeen {
            channel: ((packed >> 8) & 0x0F) as u8,
            note: (packed & 0x7F) as u8,
            serial,
        })
    }

    /// Engine: mapping `id` set its parameter to `result` from `value`. Only
    /// the low 24 bits of `sequence` are kept; the reader only compares them.
    /// The result is stored first, so a reader never sees a new sequence with
    /// an old result.
    pub fn write(&self, control: MidiControl, id: MidiMappingId, sequence: u32, value: u8, result: f32) {
        self.values[control.index()].store(result.to_bits(), Ordering::Release);
        self.entries[control.index()].store(Self::pack(id, sequence, value), Ordering::Release);
    }

    /// Engine: a pickup on mapping `id` is still waiting, and its hardware
    /// sent `value`. The parameter did not change, so no result is stored.
    pub fn write_waiting(&self, control: MidiControl, id: MidiMappingId, sequence: u32, value: u8) {
        self.entries[control.index()].store(Self::pack(id, sequence, value) | WAITING, Ordering::Release);
    }

    /// One entry, without the waiting bit.
    fn pack(id: MidiMappingId, sequence: u32, value: u8) -> u64 {
        (u64::from(id.0) << 32) | (u64::from(sequence & 0x00FF_FFFF) << 8) | u64::from(value & 0x7F)
    }

    /// The value the latest non-waiting entry for `control` set. Read the
    /// entry first: the value is then at least that new.
    pub fn result(&self, control: MidiControl) -> f32 {
        f32::from_bits(self.values[control.index()].load(Ordering::Acquire))
    }

    /// Controller: the latest write for `control`. Mapping ids start at 1, so
    /// an entry never written reads as id 0.
    pub fn read(&self, control: MidiControl) -> FeedbackEntry {
        let packed = self.entries[control.index()].load(Ordering::Acquire);
        FeedbackEntry {
            id: MidiMappingId((packed >> 32) as u32),
            sequence: ((packed >> 8) & 0x00FF_FFFF) as u32,
            value: (packed & 0x7F) as u8,
            waiting: packed & WAITING != 0,
        }
    }

    /// Engine: the screen edit with `token` reached the mapped parameter of
    /// `control` at `sequence`.
    pub fn write_edit(&self, control: MidiControl, token: u32, sequence: u32) {
        let packed = (u64::from(token) << 32) | u64::from(sequence & 0x00FF_FFFF);
        self.edits[control.index()].store(packed, Ordering::Release);
    }

    /// Controller: the last screen edit applied for `control`, as
    /// `(token, sequence)`. Tokens start at 1, so none reads as token 0.
    pub fn read_edit(&self, control: MidiControl) -> (u32, u32) {
        let packed = self.edits[control.index()].load(Ordering::Acquire);
        ((packed >> 32) as u32, (packed & 0x00FF_FFFF) as u32)
    }

    /// Engine: a control change arrived. `serial` is never 0.
    pub fn record(&self, control: MidiControl, value: u8, serial: u32) {
        let packed = (u64::from(serial) << 32)
            | (u64::from(control.channel) << 16)
            | (u64::from(control.controller) << 8)
            | u64::from(value & 0x7F);
        self.last.store(packed, Ordering::Release);
    }

    /// Controller: the last control change, or `None` before the first.
    pub fn last(&self) -> Option<ControlChangeSeen> {
        let packed = self.last.load(Ordering::Acquire);
        let serial = (packed >> 32) as u32;
        (serial != 0).then_some(ControlChangeSeen {
            control: MidiControl {
                channel: ((packed >> 16) & 0x0F) as u8,
                controller: ((packed >> 8) & 0x7F) as u8,
            },
            value: (packed & 0x7F) as u8,
            serial,
        })
    }
}

/// True when 24-bit sequence number `a` came after `b`. Sequences wrap, so
/// "after" means less than half the range ahead.
pub(crate) fn later(a: u32, b: u32) -> bool {
    let ahead = a.wrapping_sub(b) & 0x00FF_FFFF;
    ahead != 0 && ahead < 0x0080_0000
}

/// Reads the last control change from any thread, for example a UI watcher
/// that wakes the UI only when MIDI arrives. Cheap to clone. Get one from
/// [`Controller::midi_reader`](crate::Controller::midi_reader).
#[derive(Clone)]
pub struct MidiReader {
    feedback: Arc<MidiFeedback>,
}

impl fmt::Debug for MidiReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MidiReader")
    }
}

impl MidiReader {
    pub(crate) fn new(feedback: Arc<MidiFeedback>) -> Self {
        Self { feedback }
    }

    /// The last control change the engine received, or `None` before the
    /// first. Its serial changes with every control change, mapped or not.
    pub fn last_control_change(&self) -> Option<ControlChangeSeen> {
        self.feedback.last()
    }

    /// The last note on the engine received, or `None` before the first.
    /// Its serial is counted with control changes.
    pub fn last_note_on(&self) -> Option<NoteOnSeen> {
        self.feedback.last_note()
    }
}

#[cfg(test)]
mod tests;
