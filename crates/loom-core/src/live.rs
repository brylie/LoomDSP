//! Live modulated values: the latest value of each modulated parameter, for
//! the dot on its knob (FR-MOD-04). Spec: `docs/design/modulation.md`,
//! Live value.
//!
//! The same pattern as display values (ADR-0013): one table of atomics,
//! allocated once with the engine, with a fixed number of cells. The
//! controller gives each modulated parameter a cell when it compiles. The
//! engine claims the cells of a new schedule when it swaps it in, and
//! stores each parameter's value after every block. The controller loads
//! it when the UI draws. Nothing waits and nothing allocates.
//!
//! Each cell stores the key of the parameter that owns it. A reader that
//! asks for another parameter gets nothing, checked before and after
//! reading the value. So after a recompile moves a cell to another
//! parameter, the old knob never shows the new parameter's value.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Owner key of a cell with no parameter. Keys are `(module id << 32) |
/// param index`, and module IDs count up from 1, so no key has this value.
const NO_OWNER: u64 = u64::MAX;

/// The bits of a cell that has no value yet: a quiet NaN.
const EMPTY: u32 = 0x7FC0_0000;

/// "No cell", stored in a parameter fill that has none.
pub(crate) const NO_CELL: u32 = u32::MAX;

pub(crate) struct LiveTable {
    owners: Box<[AtomicU64]>,
    values: Box<[AtomicU32]>,
}

impl LiveTable {
    pub fn new(cells: usize) -> Self {
        Self {
            owners: (0..cells).map(|_| AtomicU64::new(NO_OWNER)).collect(),
            values: (0..cells).map(|_| AtomicU32::new(0)).collect(),
        }
    }

    /// Engine: gives `cell` to the parameter with `key`, unless it already
    /// has it. The cell holds no value until the first block stores one,
    /// so an engine that applies edits but renders nothing (no audio
    /// device, #220) shows no dot rather than a dot at 0.
    pub fn claim(&self, cell: u32, key: u64) {
        let cell = cell as usize;
        if self.owners[cell].load(Ordering::Relaxed) == key {
            return;
        }
        self.owners[cell].store(NO_OWNER, Ordering::Release);
        self.values[cell].store(EMPTY, Ordering::Release);
        self.owners[cell].store(key, Ordering::Release);
    }

    /// Engine: marks `cell` as empty.
    pub fn release(&self, cell: u32) {
        self.owners[cell as usize].store(NO_OWNER, Ordering::Release);
    }

    /// Engine: stores the latest value of the parameter in `cell`.
    pub fn write(&self, cell: u32, value: f32) {
        // Release, so a reader that sees this value also sees an owner
        // change that came before it (see `read`).
        self.values[cell as usize].store(value.to_bits(), Ordering::Release);
    }

    /// Controller: the value in `cell` if the parameter with `key` owns it.
    pub fn read(&self, cell: u32, key: u64) -> Option<f32> {
        let cell = cell as usize;
        if self.owners[cell].load(Ordering::Acquire) != key {
            return None;
        }
        let value = f32::from_bits(self.values[cell].load(Ordering::Acquire));
        // If the cell changed hands while we read, the value may belong to
        // the next parameter. Its owner change is visible by now.
        // A cell with no value yet holds NaN.
        (self.owners[cell].load(Ordering::Acquire) == key && !value.is_nan()).then_some(value)
    }
}

/// The controller's side: which cell each modulated parameter has. Cells
/// stay with their parameter across compiles, so a knob does not lose its
/// dot when another assignment is added.
pub(crate) struct LiveCells {
    cells: BTreeMap<u64, u32>,
    free: Vec<u32>,
}

impl LiveCells {
    pub fn new(cells: usize) -> Self {
        Self {
            cells: BTreeMap::new(),
            free: (0..cells as u32).rev().collect(),
        }
    }

    /// The cell of the parameter with `key`, if it has one.
    pub fn cell(&self, key: u64) -> Option<u32> {
        self.cells.get(&key).copied()
    }

    /// Gives cells to the parameters in `keys`, keeping the ones they had.
    /// Returns the cells taken back, for the engine to release, and how
    /// many parameters got no cell because the table is full.
    pub fn assign(&mut self, keys: &[u64]) -> (Vec<u32>, usize) {
        let mut released = Vec::new();
        self.cells.retain(|key, cell| {
            let keep = keys.contains(key);
            if !keep {
                released.push(*cell);
            }
            keep
        });
        self.free.extend(released.iter().rev());
        let mut missing = 0;
        for &key in keys {
            if self.cells.contains_key(&key) {
                continue;
            }
            match self.free.pop() {
                Some(cell) => {
                    self.cells.insert(key, cell);
                }
                None => missing += 1,
            }
        }
        (released, missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_answers_only_its_owner() {
        let table = LiveTable::new(2);
        assert_eq!(table.read(0, 7), None);
        table.claim(0, 7);
        table.write(0, 0.25);
        assert_eq!(table.read(0, 7), Some(0.25));
        assert_eq!(table.read(0, 8), None);
        // Moved to another parameter: the old one reads nothing, and the
        // new one has no value until a block stores one.
        table.claim(0, 8);
        assert_eq!(table.read(0, 7), None);
        assert_eq!(table.read(0, 8), None);
        table.write(0, 0.0);
        assert_eq!(table.read(0, 8), Some(0.0));
        table.release(0);
        assert_eq!(table.read(0, 8), None);
    }

    #[test]
    fn claiming_the_same_owner_keeps_the_value() {
        let table = LiveTable::new(1);
        table.claim(0, 7);
        table.write(0, 0.5);
        table.claim(0, 7);
        assert_eq!(table.read(0, 7), Some(0.5));
    }

    #[test]
    fn cells_stay_with_their_parameter_and_run_out() {
        let mut cells = LiveCells::new(2);
        assert_eq!(cells.assign(&[10, 20]), (vec![], 0));
        let ten = cells.cell(10).unwrap();
        // A third parameter finds the table full.
        assert_eq!(cells.assign(&[10, 20, 30]), (vec![], 1));
        assert_eq!(cells.cell(30), None);
        // 20 goes away; its cell comes back and 30 gets it. 10 keeps its own.
        let twenty = cells.cell(20).unwrap();
        assert_eq!(cells.assign(&[10, 30]), (vec![twenty], 0));
        assert_eq!(cells.cell(10), Some(ten));
        assert_eq!(cells.cell(30), Some(twenty));
    }
}
