//! Module display values: the latest few numbers each module publishes for
//! lights on its panel (ADR-0013). Spec: `docs/design/panels.md`, Display
//! values.
//!
//! One table of atomics, allocated once with the engine. The engine stores
//! after each block; the controller loads whenever the UI draws. Nothing
//! waits and nothing allocates, and only the latest value is kept.
//!
//! Each slot also stores the ID of the module that owns it. A reader that
//! asks for module 7 gets 0 unless slot owner and module agree, before and
//! after reading the value. So a new module in a reused slot never shows the
//! values of the module that was there before.

use crate::descriptor::MAX_DISPLAYS;
use std::sync::atomic::{AtomicU32, Ordering};

/// Owner value of a slot with no module. Module IDs count up from 1, so
/// no module has this one.
const NO_OWNER: u32 = u32::MAX;

pub(crate) struct DisplayTable {
    owners: Box<[AtomicU32]>,
    values: Box<[AtomicU32]>,
}

impl DisplayTable {
    pub fn new(slots: usize) -> Self {
        Self {
            owners: (0..slots).map(|_| AtomicU32::new(NO_OWNER)).collect(),
            values: (0..slots * MAX_DISPLAYS).map(|_| AtomicU32::new(0)).collect(),
        }
    }

    fn values(&self, slot: usize) -> &[AtomicU32] {
        &self.values[slot * MAX_DISPLAYS..(slot + 1) * MAX_DISPLAYS]
    }

    /// Engine: gives `slot` to module `owner`, with every value at 0.
    pub fn claim(&self, slot: usize, owner: u32) {
        self.owners[slot].store(NO_OWNER, Ordering::Release);
        for value in self.values(slot) {
            value.store(0, Ordering::Release);
        }
        self.owners[slot].store(owner, Ordering::Release);
    }

    /// Engine: marks `slot` as empty.
    pub fn release(&self, slot: usize) {
        self.owners[slot].store(NO_OWNER, Ordering::Release);
    }

    /// Engine: stores the values `slot`'s module wrote this block.
    pub fn write(&self, slot: usize, values: &[f32]) {
        for (cell, value) in self.values(slot).iter().zip(values) {
            // Release, so a reader that sees this value also sees an owner
            // change that came before it (see `read`).
            cell.store(value.to_bits(), Ordering::Release);
        }
    }

    /// Controller: value `index` of module `owner` in `slot`, or 0 while the
    /// slot belongs to another module.
    pub fn read(&self, slot: usize, owner: u32, index: usize) -> f32 {
        if self.owners[slot].load(Ordering::Acquire) != owner {
            return 0.0;
        }
        let value = f32::from_bits(self.values(slot)[index].load(Ordering::Acquire));
        // If the slot changed hands while we read, the value may belong to
        // the next module. Its owner change is visible by now.
        if self.owners[slot].load(Ordering::Acquire) != owner {
            return 0.0;
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_belong_to_their_owner() {
        let table = DisplayTable::new(2);
        table.claim(1, 7);
        table.write(1, &[0.5, 0.25]);
        assert_eq!(table.read(1, 7, 0), 0.5);
        assert_eq!(table.read(1, 7, 1), 0.25);
        // Another module, or an empty slot, reads 0.
        assert_eq!(table.read(1, 8, 0), 0.0);
        assert_eq!(table.read(0, 7, 0), 0.0);
    }

    #[test]
    fn a_new_owner_starts_at_zero() {
        let table = DisplayTable::new(1);
        table.claim(0, 7);
        table.write(0, &[1.0, 1.0, 1.0, 1.0]);
        table.release(0);
        assert_eq!(table.read(0, 7, 0), 0.0);
        table.claim(0, 9);
        assert!((0..MAX_DISPLAYS).all(|i| table.read(0, 9, i) == 0.0));
    }
}
