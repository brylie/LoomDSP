//! What a screen reader is told of the module browser's rows. The browser
//! is the runtime's (plan 3.2, row 5): its nodes come from its panel
//! (`panel.rs`), a dialog named "Add module" holding its search field, the
//! list box of results, and the text under it (ui-layer.md, section 7,
//! rule 27).

use super::BrowserRow;

/// The name a screen reader gives a row: its name and its mark, as the row
/// shows them side by side, since a sub-patch can share a module's name. A
/// name that already ends with its mark (`Filter slot`) is not read twice.
pub(crate) fn label(row: &BrowserRow) -> String {
    if row.name.to_lowercase().ends_with(&row.mark.to_lowercase()) {
        row.name.clone()
    } else {
        format!("{}, {}", row.name, row.mark)
    }
}

#[cfg(test)]
mod tests;
