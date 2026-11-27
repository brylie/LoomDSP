//! The matrix's UI IDs (`docs/design/ui-layer.md`, section 4). The panel
//! (`panel.rs`) keys its elements so that each part has the ID named
//! here, and its nodes are the runtime's.
//!
//! The assignments are a list of rows to a screen reader, not a table or
//! a grid. AccessKit's macOS adapter (0.27.2) reports no columns, no row or
//! column counts, no header cells, and no rows for a table or a grid, so a
//! screen reader's table navigation would find nothing in one (the
//! decisions rows of 2026-11-11 and 2026-11-27). A list gives each row its
//! place and the list's size, and the panel draws one card per assignment.
//!
//! Each row is keyed by its `AssignmentId`, as in the inspector, under the
//! panel `matrix`, so the two places never share a node.

use super::Column;
use loom_core::AssignmentId;
use loom_ui_core::UiId;

/// The UI ID of the matrix's content. Every part's ID is inside it.
pub(crate) fn panel() -> UiId {
    UiId::panel("matrix")
}

/// The UI ID of one part of the matrix by a fixed name: `filter`,
/// `selected-only`, `add`, `sort`, `reverse`, `assignment` (the list),
/// `new/source`, and so on.
pub(crate) fn part(name: &'static str) -> UiId {
    crate::inspector::ids::part(&panel(), name)
}

/// The UI ID of an assignment's row, by the assignment's own ID:
/// `matrix/assignment/12`.
pub(crate) fn row(id: AssignmentId) -> UiId {
    panel().with("assignment").with(id.0)
}

/// The UI ID of one cell of an assignment's row: `matrix/assignment/12/depth`.
pub(crate) fn cell(id: AssignmentId, column: Column) -> UiId {
    row(id).with(column.key())
}

/// The UI ID of an assignment's remove button.
pub(crate) fn remove(id: AssignmentId) -> UiId {
    row(id).with("remove")
}

impl Column {
    /// The key that names this column's cell in a row's UI ID.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Source => "source",
            Self::Target => "target",
            Self::Depth => "depth",
            Self::Polarity => "polarity",
        }
    }
}
