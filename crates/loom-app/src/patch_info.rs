//! Patch info: the patch's name, authors, tags, and licence, shown in the
//! inspector when no module is selected. Spec:
//! `docs/design/patch-metadata.md`, Patch info.
//!
//! The rows are plain data ([`PatchRows`]), as for the rest of the
//! inspector, so [`crate::App::ui_snapshot`] shows what the window draws.
//! Edits change the document's metadata at once. They mark the patch
//! unsaved, but they are not undo steps: each text field has its own undo.

use crate::Element;
use crate::style::{self, button};
use crate::view::remove_button;
use crate::{App, DIM, licence_chooser};
use iced::Length;
use iced::widget::{column, container, row, text, text_editor, text_input};
use loom_patch::Metadata;
use loom_patch::metadata::{self, Licence};
use serde::Serialize;

/// The Authors box's part: `inspector/patch/authors`.
pub(crate) fn authors_part() -> loom_ui_core::UiId {
    crate::inspector::ids::part(&crate::inspector::ids::patch(), "authors")
}

/// The tag field's part: `inspector/patch/tags`.
pub(crate) fn tags_part() -> loom_ui_core::UiId {
    crate::inspector::ids::part(&crate::inspector::ids::patch(), "tags")
}

/// The form's text fields, in the order Tab moves between them.
pub(crate) fn text_fields() -> [loom_ui_core::UiId; 2] {
    [authors_part(), tags_part()]
}

/// A patch info edit.
#[derive(Debug, Clone)]
pub enum Message {
    /// An edit in the authors box.
    Authors(text_editor::Action),
    /// The tag field's text changed. A comma adds what comes before it.
    TagDraft(String),
    /// Enter in the tag field adds its text as a tag.
    TagSubmit,
    /// A chip's remove button, by position in the list.
    RemoveTag(usize),
    /// The **Choose** button opens the licence chooser.
    ChooseLicence,
}

/// What patch info keeps between messages: the text being typed.
pub struct State {
    /// The authors box, one author per line, as the user typed it. The
    /// patch stores the tidied list.
    pub authors: text_editor::Content<crate::Renderer>,
    /// The tag being typed.
    pub tag_draft: String,
    /// Why the last tag was not added, until the next one is typed.
    pub tag_note: Option<String>,
}

impl State {
    /// The fields for a patch just opened or started.
    pub fn new(info: &Metadata) -> Self {
        Self {
            authors: text_editor::Content::with_text(&info.authors.join("\n")),
            tag_draft: String::new(),
            tag_note: None,
        }
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
    /// patch's own (Credit for parts). Empty until sub-patches exist (#51).
    pub parts_by: Vec<String>,
}

impl App {
    /// Patch info for the inspector.
    pub(crate) fn patch_rows(&self) -> PatchRows {
        let info = self.session.document.info();
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
            parts_by: Vec::new(),
        }
    }

    /// Handles a patch info edit.
    pub(crate) fn patch_info_message(&mut self, message: Message) {
        match message {
            Message::Authors(action) => {
                let edits = action.is_edit();
                self.session.patch_info.authors.perform(action);
                if edits {
                    let authors = metadata::tidy_authors(self.session.patch_info.authors.text().lines());
                    if authors != self.session.document.info().authors {
                        self.session.document.edit_info(|info| info.authors = authors);
                    }
                }
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
                    self.session.document.edit_info(|info| {
                        info.tags.remove(index);
                    });
                    self.session.patch_info.tag_note = None;
                }
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

/// Patch info, drawn from its rows.
pub fn view<'a>(rows: PatchRows, authors: &'a text_editor::Content<crate::Renderer>) -> Element<'a, crate::Message> {
    let msg = crate::Message::PatchInfo;
    let label = |s: &'static str| text(s).size(style::BODY).color(DIM);
    let mut chips: Vec<Element<'a, crate::Message>> = Vec::new();
    let panel = crate::inspector::ids::patch();
    let part = |name| crate::inspector::ids::part(&panel, name);
    for (index, tag) in rows.tags.into_iter().enumerate() {
        let key = part("tag").with(tag.as_str());
        let chip = container(
            row![
                text(tag.clone()).size(style::SMALL),
                crate::inspector::ids::named(
                    &crate::inspector::ids::part(&key, "remove"),
                    remove_button(format!("Remove tag {tag}"), msg(Message::RemoveTag(index)))
                )
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center),
        )
        .padding([0, 6])
        .style(container::rounded_box);
        chips.push(chip.into());
    }
    // No row at all without tags, so the field sits under its label.
    let tags: Element<'a, crate::Message> = if chips.is_empty() {
        column![].into()
    } else {
        iced::widget::Row::with_children(chips).spacing(4).wrap().into()
    };
    let mut column = column![
        text(rows.name).size(style::HEADING),
        label("Authors, one per line"),
        crate::inspector::ids::named(
            &part("authors"),
            text_editor(authors)
                .id(crate::places::field_widget_id(&authors_part()))
                .placeholder("Your name")
                .on_action(move |action| msg(Message::Authors(action)))
                .size(style::CONTROL)
                .height(Length::Shrink)
        ),
        label("Tags"),
        tags,
        crate::inspector::ids::named(
            &part("tags"),
            text_input("Type a tag, then Enter or comma", &rows.tag_draft)
                .id(crate::places::field_widget_id(&tags_part()))
                .on_input(move |value| msg(Message::TagDraft(value)))
                .on_submit(msg(Message::TagSubmit))
                .size(style::CONTROL)
        ),
    ]
    .spacing(6);
    if let Some(note) = rows.tag_note {
        column = column.push(text(note).size(style::SMALL));
    }
    column = column.push(label("Licence")).push(
        row![
            text(rows.licence).size(style::CONTROL).width(Length::Fill),
            crate::inspector::ids::named(
                &part("choose_licence"),
                button(text("Choose").size(style::CONTROL)).on_press(msg(Message::ChooseLicence))
            ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    );
    if let Some(summary) = rows.licence_summary {
        column = column.push(text(summary).size(style::SMALL));
    }
    if let Some(url) = rows.licence_url {
        column = column.push(text(url).size(style::SMALL).color(DIM));
    }
    if !rows.parts_by.is_empty() {
        column = column.push(text(format!("Includes parts by {}", rows.parts_by.join(", "))).size(style::SMALL));
    }
    column.into()
}

#[cfg(test)]
mod tests;
