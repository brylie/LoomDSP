//! The runtime's text fields, as the app hears them (ui-layer.md, section
//! 8): each field's edits and Enter become that field's own messages,
//! routed here by the field's UI ID to the view that owns it. Shared
//! groundwork for the licence chooser's Other field (plan 3.2, row 3c) and
//! the browsers' search fields (row 5).
//!
//! Each arm is one line that calls the view's own function, so this match
//! only routes, and each view keeps its field's meaning in its own module.

use crate::{App, Message};
use loom_ui::FieldEvent;
use loom_ui_core::UiId;

impl App {
    /// The message for `event` on the runtime's field with UI ID `id`: its
    /// edit (`Edited`) or its Enter (`Committed`), as its view maps them.
    /// A field whose view maps no Enter ignores it. A refused paste is said
    /// in the status line, for any field. `Left` gives nothing here: the
    /// focus moving reaches the app as `FocusMoved`.
    pub(crate) fn field_message(&self, id: &UiId, event: &FieldEvent) -> Option<Message> {
        if matches!(event, FieldEvent::PasteRefused) {
            return Some(Message::FieldPasteRefused);
        }
        // One arm per field, added with its view: `id == &view::field_id()`
        // then `view::field_message(event)`.
        if *id == crate::licence_chooser::panel::other_text_id() {
            return crate::licence_chooser::field_message(event);
        }
        None
    }
}

#[cfg(test)]
mod tests;
