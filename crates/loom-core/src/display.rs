//! Module display values: the latest few numbers each module publishes for
//! lights on its panel (ADR-0013). Spec: `docs/design/panels.md`, Display
//! values.
//!
//! One table of atomics, allocated once with the engine. The engine stores
//! after each block; the controller loads whenever the UI draws. Nothing
//! waits and nothing allocates, and only the latest value is kept.
//!
//! A `Trigger` value is latched instead: the engine stores with `max`, so a
//! 1.0 stays until the UI reads it, and the UI reads it with `swap(0)`.
//! Many blocks pass between two frames, so the light then shows "fired since
//! the last read". The table has one reader for trigger values, the UI
//! (`docs/design/panels.md`, Lights for one-sample triggers).
//!
//! Each slot also stores the ID of the module that owns it. A reader that
//! asks for module 7 gets 0 unless slot owner and module agree, before and
//! after reading the value. So a new module in a reused slot never shows the
//! values of the module that was there before.

use crate::descriptor::{DisplayKind, MAX_DISPLAYS};
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// Owner value of a slot with no module. Module IDs count up from 1, so
/// no module has this one.
const NO_OWNER: u32 = u32::MAX;

pub(crate) struct DisplayTable {
    owners: Box<[AtomicU32]>,
    values: Box<[AtomicU32]>,
    /// One bit per value of each slot: set for a `Trigger` value. Written
    /// and read by the engine only.
    triggers: Box<[AtomicU8]>,
}

impl DisplayTable {
    pub fn new(slots: usize) -> Self {
        Self {
            owners: (0..slots).map(|_| AtomicU32::new(NO_OWNER)).collect(),
            values: (0..slots * MAX_DISPLAYS).map(|_| AtomicU32::new(0)).collect(),
            triggers: (0..slots).map(|_| AtomicU8::new(0)).collect(),
        }
    }

    fn values(&self, slot: usize) -> &[AtomicU32] {
        &self.values[slot * MAX_DISPLAYS..(slot + 1) * MAX_DISPLAYS]
    }

    /// Engine: gives `slot` to module `owner`, with every value at 0.
    /// Bit `i` of `triggers` is set when value `i` is a `Trigger`.
    pub fn claim(&self, slot: usize, owner: u32, triggers: u8) {
        self.owners[slot].store(NO_OWNER, Ordering::Release);
        self.triggers[slot].store(triggers, Ordering::Relaxed);
        for value in self.values(slot) {
            value.store(0, Ordering::Release);
        }
        self.owners[slot].store(owner, Ordering::Release);
    }

    /// Engine: marks `slot` as empty.
    pub fn release(&self, slot: usize) {
        self.owners[slot].store(NO_OWNER, Ordering::Release);
    }

    /// Engine: stores the values `slot`'s module wrote this block. A
    /// trigger value is latched: it only rises until the reader takes it.
    pub fn write(&self, slot: usize, values: &[f32]) {
        let triggers = self.triggers[slot].load(Ordering::Relaxed);
        for (i, (cell, value)) in self.values(slot).iter().zip(values).enumerate() {
            // Release, so a reader that sees this value also sees an owner
            // change that came before it (see `read`).
            if triggers & (1 << i) != 0 {
                // Clamped to 0 to 1, NaN to 0. For floats that are not
                // negative, a larger value has larger bits, so `fetch_max`
                // on the bits is `max` on the values.
                let fired = if *value > 0.0 { value.min(1.0) } else { 0.0 };
                cell.fetch_max(fired.to_bits(), Ordering::Release);
            } else {
                cell.store(value.to_bits(), Ordering::Release);
            }
        }
    }

    /// Controller: value `index` of module `owner` in `slot`, or 0 while the
    /// slot belongs to another module. A trigger value is taken: the cell
    /// goes back to 0, so the next read shows only triggers after this one.
    pub fn read(&self, slot: usize, owner: u32, index: usize, trigger: bool) -> f32 {
        if self.owners[slot].load(Ordering::Acquire) != owner {
            return 0.0;
        }
        let cell = &self.values(slot)[index];
        let bits = if trigger {
            cell.swap(0, Ordering::AcqRel)
        } else {
            cell.load(Ordering::Acquire)
        };
        let value = f32::from_bits(bits);
        // If the slot changed hands while we read, the value may belong to
        // the next module. Its owner change is visible by now.
        if self.owners[slot].load(Ordering::Acquire) != owner {
            return 0.0;
        }
        value
    }
}

/// Where one display value lives: a slot, the module that should own it,
/// and an index. Get one from
/// [`Controller::display_source`](crate::Controller::display_source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplaySource {
    slot: usize,
    owner: u32,
    index: usize,
    kind: DisplayKind,
}

impl DisplaySource {
    pub(crate) fn new(slot: usize, owner: u32, index: usize, kind: DisplayKind) -> Self {
        Self {
            slot,
            owner,
            index,
            kind,
        }
    }

    /// Whether the value is a level or a latched trigger.
    pub fn kind(&self) -> DisplayKind {
        self.kind
    }
}

/// Reads display values from any thread, for example a UI timer that only
/// wakes the UI when a light changes. Cheap to clone.
#[derive(Clone)]
pub struct DisplayReader {
    table: std::sync::Arc<DisplayTable>,
}

impl std::fmt::Debug for DisplayReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DisplayReader")
    }
}

impl DisplayReader {
    pub(crate) fn new(table: std::sync::Arc<DisplayTable>) -> Self {
        Self { table }
    }

    /// The latest value, or 0 while the slot belongs to another module.
    ///
    /// A [`DisplayKind::Trigger`] value is 1.0 if a trigger arrived since
    /// the last read, and reading it clears it. So only one reader, the UI,
    /// may read trigger values.
    pub fn read(&self, source: DisplaySource) -> f32 {
        let trigger = source.kind == DisplayKind::Trigger;
        self.table.read(source.slot, source.owner, source.index, trigger)
    }
}

/// Remembers whether a module's trigger output fired since the module last
/// published its display values, for a [`DisplayKind::Trigger`] value.
///
/// Set it in [`Module::process`](crate::Module::process) and take it in
/// [`Module::displays`](crate::Module::displays). In a feedback loop the
/// engine calls `process` once per frame but `displays` once per block, so
/// clearing the flag in `process` would lose triggers. `displays` takes
/// `&self`, so the flag is a `Cell`. It never changes the audio.
#[derive(Debug, Default, Clone)]
pub struct TriggerFlag(std::cell::Cell<bool>);

impl TriggerFlag {
    /// Remembers a trigger if `fired` is true. False leaves the flag alone.
    pub fn record(&self, fired: bool) {
        if fired {
            self.0.set(true);
        }
    }

    /// 1.0 if a trigger fired since the last take, else 0.0. Clears it.
    pub fn take(&self) -> f32 {
        if self.0.replace(false) { 1.0 } else { 0.0 }
    }

    /// Forgets a trigger not yet taken, as when a module resets.
    pub fn clear(&self) {
        self.0.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flag keeps any trigger until it is taken, once.
    #[test]
    fn a_trigger_flag_is_taken_once() {
        let flag = TriggerFlag::default();
        assert_eq!(flag.take(), 0.0);
        flag.record(true);
        flag.record(false);
        assert_eq!(flag.take(), 1.0);
        assert_eq!(flag.take(), 0.0);
        flag.record(true);
        flag.clear();
        assert_eq!(flag.take(), 0.0);
    }

    #[test]
    fn values_belong_to_their_owner() {
        let table = DisplayTable::new(2);
        table.claim(1, 7, 0);
        table.write(1, &[0.5, 0.25]);
        assert_eq!(table.read(1, 7, 0, false), 0.5);
        assert_eq!(table.read(1, 7, 1, false), 0.25);
        // Another module, or an empty slot, reads 0.
        assert_eq!(table.read(1, 8, 0, false), 0.0);
        assert_eq!(table.read(0, 7, 0, false), 0.0);
    }

    #[test]
    fn a_new_owner_starts_at_zero() {
        let table = DisplayTable::new(1);
        table.claim(0, 7, 0);
        table.write(0, &[1.0, 1.0, 1.0, 1.0]);
        table.release(0);
        assert_eq!(table.read(0, 7, 0, false), 0.0);
        table.claim(0, 9, 0);
        assert!((0..MAX_DISPLAYS).all(|i| table.read(0, 9, i, false) == 0.0));
    }

    /// A trigger written in one block stays until it is read, even when
    /// later blocks write 0, and a read takes it.
    #[test]
    fn a_trigger_is_latched_until_read() {
        let table = DisplayTable::new(1);
        // Value 1 is a trigger, value 0 a level.
        table.claim(0, 7, 0b10);
        table.write(0, &[0.5, 1.0]);
        table.write(0, &[0.25, 0.0]);
        table.write(0, &[0.125, 0.0]);
        // The level shows the latest block, the trigger any block since the read.
        assert_eq!(table.read(0, 7, 0, false), 0.125);
        assert_eq!(table.read(0, 7, 1, true), 1.0);
        assert_eq!(table.read(0, 7, 1, true), 0.0);
        // Values out of range and NaN are kept to 0 to 1.
        table.write(0, &[0.0, f32::NAN]);
        assert_eq!(table.read(0, 7, 1, true), 0.0);
        table.write(0, &[0.0, -3.0]);
        assert_eq!(table.read(0, 7, 1, true), 0.0);
        table.write(0, &[0.0, 7.0]);
        assert_eq!(table.read(0, 7, 1, true), 1.0);
    }
}
