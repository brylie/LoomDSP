//! The UI IDs of the inspector's parts (`docs/design/ui-layer.md`, section
//! 4), and the containers that name those parts in the window.
//!
//! Every key comes from what the part shows: the tile's flat ID, the kind
//! of item, the parameter's name in patch files or an exposed item's name
//! on its surface, an assignment's ID. None comes from a row's position,
//! so a row keeps its ID when rows are added or removed above it.
//!
//! The view wraps each part in a container whose widget ID is the UI ID's
//! text, so the window can say where the part is (`places.rs`), and the
//! accessibility tree gives the same ID to the part's node.

use iced::Element;
use iced::widget::container;
use loom_ui_core::UiId;

/// The UI ID of the inspector's content for tile `tile`: every row's ID is
/// inside it.
pub(crate) fn panel(tile: u32) -> UiId {
    UiId::panel("inspector").with(tile)
}

/// What kind of item a row edits, as the key in its UI ID names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A module's own parameter.
    Param,
    /// An item a sub-patch exposes on its surface.
    Exposed,
}

impl Kind {
    /// The key in a UI ID.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Param => "param",
            Self::Exposed => "exposed",
        }
    }
}

/// The UI ID of a parameter's row on tile `tile`: `inspector/4/param/cutoff`
/// for a module's parameter, `inspector/7/exposed/Cutoff` for an item a
/// sub-patch exposes.
pub(crate) fn row(tile: u32, kind: Kind, name: &str) -> UiId {
    panel(tile).with(kind.key()).with(name)
}

/// The UI ID of one part of a row, or of the panel, by a fixed name:
/// `value` for the slider or the choices, `field` for the typed value,
/// `range`, `expose`, `midi/options`, and so on.
pub(crate) fn part(outer: &UiId, name: &'static str) -> UiId {
    name.split('/').fold(outer.clone(), UiId::with)
}

/// The UI ID of a modulation assignment's card under a parameter's row,
/// by the assignment's own ID.
pub(crate) fn assignment(row: &UiId, id: u32) -> UiId {
    row.clone().with("assignment").with(id)
}

/// `content` in a container named `id`, so the window can say where it is.
/// The container adds no padding and takes its content's size, so it
/// moves nothing.
pub(crate) fn named<'a, Message: 'a>(id: &UiId, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).id(crate::places::widget_id(id)).into()
}

#[cfg(test)]
mod tests;
