//! Patch info: the patch's name, authors, tags, and licence, shown in the
//! inspector when no module is selected. Spec:
//! `docs/design/patch-metadata.md`, Patch info.
//!
//! The rows are plain data ([`PatchRows`]), as for the rest of the
//! inspector, so [`crate::App::ui_snapshot`] shows what the window draws.
//! Root patch edits change the document's metadata at once and mark it
//! unsaved. Authors on a selected sub-patch edit its definition.

use crate::Element;
use crate::style;
use crate::{App, DIM, licence_chooser};
use iced::Length;
use iced::widget::{column, text, text_editor};
use loom_patch::Metadata;
use loom_patch::metadata::{self, Licence};
use serde::Serialize;

/// The history step for one session in a sub-patch's Authors box.
pub(crate) const EDIT_PART_AUTHORS: &str = "edit sub-patch authors";

/// The Authors box's part: `inspector/patch/authors`.
pub(crate) fn authors_part() -> loom_ui_core::UiId {
    crate::inspector::ids::part(&crate::inspector::ids::patch(), "authors")
}

/// The Authors box on a selected sub-patch's own inspector.
pub(crate) fn part_authors_id(tile: u32) -> loom_ui_core::UiId {
    crate::inspector::ids::part(&crate::inspector::ids::panel(tile), "authors")
}

/// The tag field's part: `inspector/patch/tags`.
pub(crate) fn tags_part() -> loom_ui_core::UiId {
    crate::inspector::ids::part(&crate::inspector::ids::patch(), "tags")
}

/// The tags' toolbar: `inspector/patch/tag`.
pub(crate) fn tags_toolbar_id() -> loom_ui_core::UiId {
    crate::inspector::ids::part(&crate::inspector::ids::patch(), "tag")
}

/// The remove button of the chip for `tag`:
/// `inspector/patch/tag/<tag>/remove`.
pub(crate) fn tag_remove_id(tag: &str) -> loom_ui_core::UiId {
    crate::inspector::ids::part(&tags_toolbar_id().with(tag), "remove")
}

/// The form's text fields, in the order Tab moves between them.
pub(crate) fn text_fields() -> [loom_ui_core::UiId; 2] {
    [authors_part(), tags_part()]
}

/// The message for an event on the Tags field: an edit's text, where a
/// comma adds a tag, or Enter, which adds what is left.
pub(crate) fn tags_message(event: &loom_ui::FieldEvent) -> Option<crate::Message> {
    match event {
        loom_ui::FieldEvent::Edited(text) => Some(crate::Message::PatchInfo(Message::TagDraft(text.clone()))),
        loom_ui::FieldEvent::Committed(_) => Some(crate::Message::PatchInfo(Message::TagSubmit)),
        _ => None,
    }
}

/// The message for an event on the Authors box: each edit's text. It has
/// no Enter of its own, since Enter there inserts a line break.
pub(crate) fn authors_message(event: &loom_ui::FieldEvent) -> Option<crate::Message> {
    match event {
        loom_ui::FieldEvent::Edited(text) => Some(crate::Message::PatchInfo(Message::Authors(text.clone()))),
        _ => None,
    }
}

/// A patch info edit.
#[derive(Debug, Clone)]
pub enum Message {
    /// The Authors box's text, as typed, from an edit or a screen
    /// reader's SetValue (ui-layer.md, section 8, rule 7).
    Authors(String),
    /// An edit to the selected sub-patch's list of authors.
    PartAuthors(u32, text_editor::Action),
    /// The tag field's text changed. A comma adds what comes before it.
    TagDraft(String),
    /// Enter in the tag field adds its text as a tag.
    TagSubmit,
    /// A chip's remove button, by position in the list.
    RemoveTag(usize),
    /// The **Choose** button opens the licence chooser.
    ChooseLicence,
    /// Left, Right, Home, or End on the tags moved the focus to this tag's
    /// remove button (the WAI-ARIA toolbar pattern).
    FocusTag(String),
    /// Escape on one of patch info's buttons: the keys go back to the
    /// canvas, as from the toolbar.
    Leave,
}

/// What patch info keeps between messages: the text being typed.
pub struct State {
    /// The Authors box's text: as typed while it has focus, and the stored
    /// authors, one per line, once focus leaves it. The patch stores the
    /// tidied list (ui-layer.md, section 8, rule 7).
    pub authors: String,
    /// The tag being typed.
    pub tag_draft: String,
    /// Why the last tag was not added, until the next one is typed.
    pub tag_note: Option<String>,
    /// The tag whose remove button the tags' one Tab stop lands on: the
    /// one last focused while it is still there.
    pub last_tag: Option<String>,
    /// The text box for the sub-patch currently selected.
    part: Option<PartAuthors>,
}

/// An authors field tied to one sub-patch tile, with the last saved text.
struct PartAuthors {
    tile: u32,
    saved: Vec<String>,
    before_edit: Option<Vec<String>>,
    content: text_editor::Content<crate::Renderer>,
}

impl State {
    /// The fields for a patch just opened or started.
    pub fn new(info: &Metadata) -> Self {
        Self {
            authors: info.authors.join("\n"),
            tag_draft: String::new(),
            tag_note: None,
            last_tag: None,
            part: None,
        }
    }

    /// The current sub-patch's box, when the field belongs to this tile.
    pub(crate) fn part_authors(&self, tile: u32) -> Option<&text_editor::Content<crate::Renderer>> {
        self.part
            .as_ref()
            .filter(|part| part.tile == tile)
            .map(|part| &part.content)
    }

    /// Whether the Authors box owns the current undo gesture.
    pub(crate) fn part_authors_edit_open(&self) -> bool {
        self.part.as_ref().is_some_and(|part| part.before_edit.is_some())
    }

    /// Whether the open edit changed the authors before reaching history.
    pub(crate) fn part_authors_dirty(&self) -> bool {
        self.part
            .as_ref()
            .is_some_and(|part| part.before_edit.as_ref().is_some_and(|before| *before != part.saved))
    }

    /// Releases the Authors box's undo gesture, if it owns one.
    pub(crate) fn end_part_authors_edit(&mut self) -> bool {
        self.part.as_mut().and_then(|part| part.before_edit.take()).is_some()
    }
}

/// What patch info shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PatchRows {
    /// The patch name, as the title shows it.
    pub name: String,
    /// The authors, in credit order.
    pub authors: Vec<String>,
    /// The tags, as chips.
    pub tags: Vec<String>,
    /// The tag being typed.
    pub tag_draft: String,
    /// Why the last tag was not added.
    pub tag_note: Option<String>,
    /// The licence's short name, "Unrecognised licence", or "No licence
    /// stated".
    pub licence: String,
    /// The licence's one-line summary, or the text as written for an
    /// unrecognised one.
    pub licence_summary: Option<String>,
    /// The licence's own page.
    pub licence_url: Option<String>,
    /// **Includes parts by**: authors of sub-patches who are not the
    /// patch's own (Credit for parts).
    pub parts_by: Vec<String>,
}

impl App {
    /// Patch info for the inspector.
    pub(crate) fn patch_rows(&self) -> PatchRows {
        let info = self.session.document.info();
        let parts = metadata::tidy_authors(self.session.hierarchy.part_authors());
        let licence = Licence::of(info.license.as_deref());
        let (licence_summary, licence_url) = match &licence {
            Licence::None => (None, None),
            Licence::Unrecognised(written) => (Some(format!("\"{written}\" is kept as written.")), None),
            Licence::Known(known) => (Some(known.summary.clone()), known.url.clone()),
        };
        PatchRows {
            name: self.patch_name(),
            authors: info.authors.clone(),
            tags: info.tags.clone(),
            tag_draft: self.session.patch_info.tag_draft.clone(),
            tag_note: self.session.patch_info.tag_note.clone(),
            licence: licence.short(),
            licence_summary,
            licence_url,
            parts_by: parts
                .into_iter()
                .filter(|name| !metadata::has_author(&info.authors, name))
                .collect(),
        }
    }

    /// Switches the sub-patch text box when the selection or saved authors change.
    pub(crate) fn sync_part_authors(&mut self) {
        let selected = &self.session.selection;
        let selected = selected.first().copied().filter(|_| selected.len() == 1);
        let current = selected.and_then(|tile| {
            let authors = &self
                .session
                .hierarchy
                .definition_of(loom_core::ModuleId(tile))?
                .metadata
                .authors;
            Some((tile, authors))
        });
        let field = self.session.patch_info.part.as_ref();
        if field.map(|part| (part.tile, &part.saved)) == current {
            return;
        }
        let next = current.map(|(tile, saved)| PartAuthors {
            tile,
            saved: saved.clone(),
            before_edit: None,
            content: text_editor::Content::with_text(&saved.join("\n")),
        });
        self.close_part_authors_edit();
        self.session.patch_info.part = next;
    }

    /// Shows the stored authors in patch info's Authors box when focus
    /// leaves it, so a blank line typed between names goes once the box is
    /// done with (ui-layer.md, section 8, rule 7).
    ///
    /// A tag's remove button that takes the focus becomes the one the
    /// tags' Tab stop lands on next.
    pub(crate) fn authors_focus_moved(&mut self, focus: Option<&loom_ui_core::UiId>) {
        // Leaving the Tags field by any means adds what is typed, as Enter
        // does, as a value field's draft commits (Brylie's decision,
        // 2026-11-25). Escape drops it before the focus leaves.
        if self.focus.as_ref() == Some(&tags_part())
            && focus != Some(&tags_part())
            && !self.focus_went_into_an_overlay()
        {
            let draft = std::mem::take(&mut self.session.patch_info.tag_draft);
            self.session.patch_info.tag_note = None;
            self.add_tag(&draft);
        }
        if focus != Some(&authors_part()) {
            self.session.patch_info.authors = self.session.document.info().authors.join("\n");
        }
        if let Some(focus) = focus
            && let Some(tag) = self
                .session
                .document
                .info()
                .tags
                .iter()
                .find(|tag| tag_remove_id(tag) == *focus)
        {
            self.session.patch_info.last_tag = Some(tag.clone());
        }
    }

    /// Records the sub-patch authors step when focus leaves its field.
    pub(crate) fn part_authors_focus_moved(&mut self, focus: Option<&loom_ui_core::UiId>) {
        if let Some(part) = &self.session.patch_info.part
            && focus != Some(&part_authors_id(part.tile))
        {
            self.close_part_authors_edit();
        }
    }

    /// Handles a patch info edit.
    pub(crate) fn patch_info_message(&mut self, message: Message) {
        match message {
            Message::Authors(text) => {
                let authors = metadata::tidy_authors(text.lines());
                if authors != self.session.document.info().authors {
                    self.session.document.edit_info(|info| info.authors = authors);
                }
                // A box without focus, as a screen reader's SetValue can
                // reach it, shows the stored authors at once; with focus it
                // keeps what was typed until focus leaves.
                self.session.patch_info.authors = if self.focus.as_ref() == Some(&authors_part()) {
                    text
                } else {
                    self.session.document.info().authors.join("\n")
                };
            }
            Message::PartAuthors(tile, action) => {
                let Some(part) = self.session.patch_info.part.as_mut().filter(|part| part.tile == tile) else {
                    return;
                };
                let edits = action.is_edit();
                part.content.perform(action);
                if !edits {
                    return;
                }
                let authors = metadata::tidy_authors(part.content.text().lines());
                if authors == part.saved {
                    return;
                }
                let starting = if part.before_edit.is_none() {
                    self.close_slider_gesture();
                    self.begin_edit();
                    !self.session.edits.gesture_open()
                } else {
                    false
                };
                let part = self
                    .session
                    .patch_info
                    .part
                    .as_mut()
                    .expect("the field handled this action");
                if starting {
                    self.session.edits.start_gesture();
                    part.before_edit = Some(part.saved.clone());
                }
                part.saved.clone_from(&authors);
                self.session
                    .hierarchy
                    .set_part_authors(loom_core::ModuleId(tile), authors);
            }
            Message::TagDraft(text) => {
                self.session.patch_info.tag_note = None;
                // Each comma ends a tag; the rest stays in the field.
                let mut parts: Vec<&str> = text.split(',').collect();
                let rest = parts.pop().unwrap_or_default().to_string();
                for part in parts {
                    self.add_tag(part);
                }
                self.session.patch_info.tag_draft = rest;
            }
            Message::TagSubmit => {
                let draft = std::mem::take(&mut self.session.patch_info.tag_draft);
                self.session.patch_info.tag_note = None;
                self.add_tag(&draft);
            }
            Message::RemoveTag(index) => {
                if index < self.session.document.info().tags.len() {
                    self.tag_removed(index);
                    self.session.document.edit_info(|info| {
                        info.tags.remove(index);
                    });
                    self.session.patch_info.tag_note = None;
                }
            }
            Message::FocusTag(tag) => {
                let id = tag_remove_id(&tag);
                self.session.patch_info.last_tag = Some(tag);
                self.effect(loom_ui::Effects::focus_as_input(id));
            }
            Message::Leave => {
                // Escape drops a value field's, the MIDI range's, or the
                // Tags field's draft before the focus leaves, which would
                // commit it.
                self.session.value_draft = None;
                self.session.learn.set_range_draft(None);
                self.session.patch_info.tag_draft.clear();
                self.effect(loom_ui::Effects::unfocus());
                self.toolbar_focus(None);
            }
            Message::ChooseLicence => {
                let current = self.session.document.info().license.clone();
                self.overlays.chooser = Some(licence_chooser::Chooser::open(
                    licence_chooser::Target::Patch,
                    current.as_deref(),
                ));
            }
        }
    }

    /// Before tag `index` goes: when its remove button has the focus, the
    /// focus goes to the next tag's, or else the previous one's, or else to
    /// the Tags field, so a keyboard user keeps their place (the WAI-ARIA
    /// toolbar pattern). The tags' Tab stop never lands on a tag that went.
    fn tag_removed(&mut self, index: usize) {
        let tags = &self.session.document.info().tags;
        let gone = &tags[index];
        let neighbour = tags
            .get(index + 1)
            .or_else(|| index.checked_sub(1).and_then(|i| tags.get(i)))
            .cloned();
        let focused = self.focus.as_ref() == Some(&tag_remove_id(gone));
        if self.session.patch_info.last_tag.as_ref() == Some(gone) {
            self.session.patch_info.last_tag.clone_from(&neighbour);
        }
        if focused {
            match neighbour {
                Some(tag) => {
                    self.effect(loom_ui::Effects::focus_as_input(tag_remove_id(&tag)));
                    self.session.patch_info.last_tag = Some(tag);
                }
                None => self.effect(loom_ui::Effects::focus_as_input(tags_part())),
            }
        }
    }

    /// Adds typed text as a tag, or notes why it was refused. Blank text
    /// between commas is skipped quietly.
    fn add_tag(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let mut tags = self.session.document.info().tags.clone();
        match metadata::add_tag(&mut tags, text) {
            Ok(Some(_)) => self.session.document.edit_info(|info| info.tags = tags),
            Ok(None) => {}
            Err(e) => self.session.patch_info.tag_note = Some(e.to_string()),
        }
    }
}

/// The editable credit on a selected sub-patch's inspector.
pub(crate) fn part_authors_view(
    tile: u32,
    authors: &text_editor::Content<crate::Renderer>,
) -> Element<'_, crate::Message> {
    let id = part_authors_id(tile);
    column![
        text("Authors, one per line").size(style::BODY).color(DIM),
        crate::inspector::ids::named(
            &id,
            text_editor(authors)
                .id(crate::places::field_widget_id(&id))
                .placeholder("Author name")
                .on_action(move |action| crate::Message::PatchInfo(Message::PartAuthors(tile, action)))
                .size(style::CONTROL)
                .height(Length::Shrink)
        ),
    ]
    .spacing(6)
    .into()
}

#[cfg(test)]
mod tests;
