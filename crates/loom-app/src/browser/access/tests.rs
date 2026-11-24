use super::*;
use loom_core::Family;
use loom_ui_core::UiId;

/// A row called `name` with `mark`.
fn row(name: &str, mark: &'static str) -> BrowserRow {
    BrowserRow {
        id: UiId::panel("browser"),
        name: name.to_string(),
        mark,
        family: Family::Interface,
        library: false,
        description: String::new(),
        key: String::new(),
    }
}

/// A row is named by its name and its mark, and a name that ends with its
/// mark is read once.
#[test]
fn a_row_is_named_by_its_name_and_mark_once() {
    assert_eq!(label(&row("Pad", "Sub-patch")), "Pad, Sub-patch");
    assert_eq!(label(&row("Filter slot", "Slot")), "Filter slot");
}
