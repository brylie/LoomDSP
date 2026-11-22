//! Tests for the menu panel's keys: which item orders `rows` can key with
//! each row's whole UI ID.

use super::{Item, keys_grouped};
use loom_ui_core::IdKey;

/// An item at `path`, keys joined by `/`, with nothing else set.
fn item(path: &str) -> Item {
    Item {
        path: path.split('/').map(IdKey::from).collect(),
        position: 0,
        label: path.to_string(),
        second: None,
        dim: false,
        highlighted: false,
    }
}

/// True when the items at `paths`, in order, pass the check.
fn grouped(paths: &[&str]) -> bool {
    let items: Vec<Item> = paths.iter().map(|path| item(path)).collect();
    keys_grouped(&items.iter().collect::<Vec<_>>())
}

/// The tile menu's own order passes: one-key items, then the make-slot
/// items together, then a slot's swaps, by kind.
#[test]
fn the_menus_order_keeps_each_shared_key_together() {
    assert!(grouped(&[
        "open",
        "help",
        "make-slot/filter",
        "make-slot/effect",
        "swap/7/module/ladder",
        "swap/7/module/one-pole",
        "swap/7/library/Comb",
        "remove-part/7",
    ]));
}

/// Items with one shared key split by another would give two columns with
/// one UI ID, and so would a run split deeper down.
#[test]
fn a_split_run_fails() {
    assert!(!grouped(&["make-slot/filter", "open", "make-slot/effect"]));
    assert!(!grouped(&[
        "swap/7/module/ladder",
        "swap/7/library/Comb",
        "swap/7/module/one-pole",
    ]));
}

/// An item whose keys begin another's would share its ID with that one's
/// column, and two items with one ID share a node.
#[test]
fn a_prefix_or_a_duplicate_fails() {
    assert!(!grouped(&["swap", "swap/7/module/ladder"]));
    assert!(!grouped(&["swap/7/module/ladder", "swap"]));
    assert!(!grouped(&["open", "open"]));
}
