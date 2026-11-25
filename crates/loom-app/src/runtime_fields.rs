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
        if *id == crate::browser::search_id() {
            return crate::browser::field_message(event);
        }
        if crate::patches::text_fields().contains(id) {
            return crate::patches::field_message(id, event);
        }
        // Patch info's Tags field: each edit is the text as typed, where a
        // comma adds a tag, and Enter adds what is left (patch-metadata.md,
        // Patch info). Its Authors box never commits: each edit is the
        // text as typed (ui-layer.md, section 8, rule 7).
        if *id == crate::patch_info::tags_part() {
            return crate::patch_info::tags_message(event);
        }
        if *id == crate::patch_info::authors_part() {
            return crate::patch_info::authors_message(event);
        }
        // Preferences' **Your name**: each edit is the text as typed, and
        // Enter saves it (preferences.md, Keys and focus).
        if *id == crate::settings::author_part() {
            return match event {
                FieldEvent::Edited(text) => Some(Message::Preferences(crate::settings::Message::Author(text.clone()))),
                FieldEvent::Committed(_) => Some(Message::Preferences(crate::settings::Message::SaveAuthor)),
                _ => None,
            };
        }
        None
    }
}

#[cfg(test)]
mod tests;
