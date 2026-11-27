//! The inspector as a panel of `loom-ui`'s Panels layer (plan 3.2, row 4,
//! part 4d), in a slot of the root panel below the right column's tabs,
//! which Iced still draws until 4e. It draws the variants moved so far:
//! patch info when nothing is selected, the count of a selection of
//! several, a sub-patch's port tile, and nothing for a module that went.
//! Every other variant is still Iced's ([`App::inspector_ours`]), and Iced
//! keeps an empty space where this panel draws.
//!
//! Each part keeps the UI ID it had: the scroll container is keyed by what
//! the inspector shows (`inspector/patch`, `inspector/several`,
//! `inspector/<tile>`), the ID Iced's content container had, so the parts
//! nest under it as before. Drawn from the same plain-data model as the
//! UI snapshot and, until the Iced inspector goes, its accessibility items
//! (ui-layer.md, section 7, rule 1).

use super::{Inspector, ids};
use crate::App;
use crate::matrix::Tab;
use crate::patch_info::PatchRows;
use crate::style::{self, Role as ButtonRole};
use loom_ui::access::{Access, Action, Role};
use loom_ui::{Align, Element, Layer, Length, View};
use loom_ui_core::{Color, Key, KeyEvent, UiId};

/// The keys the tags take, as one toolbar stop, with Escape (the WAI-ARIA
/// toolbar pattern, as in Your patches).
const TAG_KEYS: [Key; 5] = [Key::Left, Key::Right, Key::Home, Key::End, Key::Escape];

/// The key each of patch info's stops takes first: Escape gives the keys
/// back to the canvas, as from the toolbar, and from a field, as Iced's
/// text box did.
const ESCAPE: [Key; 1] = [Key::Escape];

/// The UI ID of the inspector's panel.
pub(crate) fn inspector_id() -> UiId {
    UiId::panel("inspector")
}

/// Everything the panel shows, so the runtime builds it again only when
/// this changes.
#[derive(Debug, Clone, PartialEq)]
struct Panel {
    /// What the inspector shows: one of the variants this panel draws.
    model: Inspector,
    /// Patch info's Authors box as typed, while it has focus; the stored
    /// authors, one per line, otherwise (ui-layer.md, section 8, rule 7).
    authors: String,
    /// The look of the column's text, fields, and buttons.
    looks: Looks,
    /// The look of a module's rows.
    rows: rows::Looks,
    /// The row a dragged output is over, while one is dragged.
    drop_row: Option<(u32, usize)>,
    /// A selected sub-patch's Authors box as typed, while it has focus;
    /// its saved authors otherwise.
    part_authors: String,
}

/// The colours the inspector is painted in, from the style tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Looks {
    text: Color,
    quiet: Color,
    rule: Color,
    chip: style::DialogLook,
    field: style::FieldLook,
    /// The warning outline of a field whose text is refused.
    danger: Color,
}

impl App {
    /// Whether the runtime draws the inspector now: its tab is open, no
    /// jump list covers it, and it shows a variant moved so far. The one
    /// answer the view, the stops, Tab, and the tree read, so the column is
    /// drawn, focused, and reported by one toolkit at a time.
    pub(crate) fn inspector_ours(&self) -> bool {
        self.right_tab == Tab::Inspector && !self.session.outline.has_jump()
    }

    /// Whether the inspector's panel owns the input: it is the runtime's,
    /// and nothing is open over the canvas. Under an overlay it is drawn
    /// but inert: its parts take no click and no focus (ui-layer.md,
    /// section 7, rule 24).
    pub(crate) fn inspector_owns(&self) -> bool {
        self.inspector_ours() && self.input_owner() == crate::input_owner::InputOwner::Canvas
    }

    /// Adds the inspector's panel to the runtime's view, while it is the
    /// runtime's.
    pub(crate) fn inspector_panel(&self, view: &mut View) {
        if !self.inspector_ours() {
            return;
        }
        let looks = Looks {
            text: crate::style::TEXT,
            quiet: crate::style::QUIET,
            rule: crate::style::EDGE,
            chip: crate::style::DIALOG,
            field: crate::style::FIELD,
            danger: crate::style::DANGER_EDGE,
        };
        let mut small_select = crate::style::select();
        small_select.text = style::SMALL_TEXT;
        small_select.padding_y = 1.0;
        small_select.padding_x = 4.0;
        let mut quiet_select = small_select;
        quiet_select.background = Color::TRANSPARENT;
        quiet_select.border = Color::TRANSPARENT;
        quiet_select.mark = crate::style::QUIET;
        let rows = rows::Looks {
            text: looks.text,
            quiet: looks.quiet,
            field: looks.field,
            select: crate::style::select(),
            small_select,
            quiet_select,
            danger: crate::style::DANGER_EDGE,
            drop: crate::style::ACCENT_WEAK,
            card_fill: crate::style::CARD_FILL,
            card_border: crate::style::CARD_BORDER,
        };
        let dragging = self.session.gestures.assign_drag().is_some() && !self.session.learn.is_on();
        let model = self.inspector_model();
        let panel = Panel {
            model: model.clone(),
            authors: self.session.patch_info.authors.clone(),
            looks,
            rows,
            drop_row: self.session.gestures.drop_row().filter(|_| dragging),
            part_authors: match &model {
                Inspector::SubPatch(sub) => self
                    .session
                    .patch_info
                    .part_authors(sub.id)
                    .map_or_else(|| sub.authors.join("\n"), ToString::to_string),
                _ => String::new(),
            },
        };
        view.panel_with(inspector_id(), Layer::Panels, &panel, build);
    }

    /// The message for a click on a button of the inspector's, from the
    /// pointer, a key, or a screen reader.
    pub(crate) fn inspector_click(&self, id: &UiId) -> Option<crate::Message> {
        if !id.is_under(&inspector_id()) || !self.inspector_owns() {
            return None;
        }
        let patch = ids::patch();
        if *id == ids::part(&patch, "shortcuts") {
            return Some(crate::Message::ShowShortcuts);
        }
        if *id == ids::part(&patch, "choose_licence") {
            return Some(crate::Message::PatchInfo(crate::patch_info::Message::ChooseLicence));
        }
        let Inspector::Patch(rows) = self.inspector_model() else {
            return self.inspector_rows_click(id);
        };
        rows.tags
            .iter()
            .position(|name| *id == crate::patch_info::tag_remove_id(name))
            .map(|index| crate::Message::PatchInfo(crate::patch_info::Message::RemoveTag(index)))
    }

    /// The message for a key one of the column's parts takes first: the
    /// tags' toolbar keys, and Escape on any stop, which gives the keys
    /// back to the canvas.
    pub(crate) fn inspector_panel_key(&self, id: &UiId, key: &KeyEvent) -> Option<crate::Message> {
        use crate::patch_info::{Message as Info, tag_remove_id, tags_toolbar_id};
        if !self.inspector_owns() {
            return None;
        }
        let KeyEvent::Pressed { key, .. } = key else {
            return None;
        };
        // Escape on any of the column's stops gives the keys back to the
        // canvas, as from the toolbar.
        if *key == Key::Escape && id.is_under(&inspector_id()) {
            return Some(crate::Message::PatchInfo(Info::Leave));
        }
        let Inspector::Patch(rows) = self.inspector_model() else {
            return None;
        };
        let message = if *key == Key::Escape {
            Info::Leave
        } else if *id == tags_toolbar_id() {
            let last = rows.tags.len().checked_sub(1)?;
            let at = self
                .focus
                .as_ref()
                .and_then(|focus| rows.tags.iter().position(|tag| tag_remove_id(tag) == *focus))
                .unwrap_or(0);
            // Left and Right stop at the ends, as the app's toolbar does.
            let to = match key {
                Key::Left => at.saturating_sub(1),
                Key::Right => (at + 1).min(last),
                Key::Home => 0,
                Key::End => last,
                _ => return None,
            };
            Info::FocusTag(rows.tags[to].clone())
        } else {
            return None;
        };
        Some(crate::Message::PatchInfo(message))
    }

    /// The inspector's focus stops, in reading order, while its panel owns
    /// the input: patch info's Authors box, the tags as one stop, on the
    /// tag last focused or the first, the Tags field, Choose, and Keyboard
    /// shortcuts, top to bottom as they are laid out (Brylie's decision,
    /// 2026-11-25; the WAI-ARIA toolbar pattern for the tags). The other
    /// variants it draws have none.
    pub(crate) fn inspector_stops(&self) -> Vec<UiId> {
        let Some(rows) = self.inspector_patch_rows() else {
            return Vec::new();
        };
        let patch = ids::patch();
        let mut stops = vec![crate::patch_info::authors_part()];
        let last = self.session.patch_info.last_tag.as_ref();
        let entry = last.filter(|last| rows.tags.contains(last)).or(rows.tags.first());
        stops.extend(entry.map(|tag| crate::patch_info::tag_remove_id(tag)));
        stops.push(crate::patch_info::tags_part());
        stops.push(ids::part(&patch, "choose_licence"));
        stops.push(ids::part(&patch, "shortcuts"));
        stops
    }

    /// Every tag's remove button, while the inspector's panel owns the
    /// input: each stands for the tags' one stop, and a screen reader's
    /// Focus can reach any of them.
    pub(crate) fn inspector_tags(&self) -> Vec<UiId> {
        self.inspector_patch_rows().map_or_else(Vec::new, |rows| {
            rows.tags
                .iter()
                .map(|tag| crate::patch_info::tag_remove_id(tag))
                .collect()
        })
    }

    /// Where Tab goes from `from` in the column, or the one before when
    /// `back`: the next stop, or past either end the bar's first stop
    /// forward and the top run's last back, as from patch info's fields
    /// before (ui-layer.md, section 6, rule 3). Any tag stands for the
    /// tags' one stop. `None` when `from` is no stop of the column's.
    pub(crate) fn inspector_next_stop(&self, from: &UiId, back: bool) -> Option<loom_ui::TabStep<crate::Message>> {
        let stops = self.inspector_stops();
        let from = if self.inspector_tags().contains(from) {
            stops.get(1)?
        } else {
            from
        };
        let at = stops.iter().position(|stop| stop == from)?;
        let to = if back { at.checked_sub(1) } else { Some(at + 1) };
        let past = || {
            let (top, bar) = (self.top_stops(), self.bar_stops());
            // Back past the first stop, the tab list, the one stop before
            // its panel (the WAI-ARIA Tabs pattern).
            if back {
                self.tabs_stops().first().or(top.last()).or(bar.last()).cloned()
            } else {
                bar.first().or(top.first()).cloned()
            }
        };
        let stop = to.and_then(|to| stops.get(to)).cloned().or_else(past);
        Some(stop.map_or(loom_ui::TabStep::Leave, loom_ui::TabStep::To))
    }

    /// Patch info's rows, while the inspector's panel owns the input and
    /// shows them.
    fn inspector_patch_rows(&self) -> Option<PatchRows> {
        if !self.inspector_owns() {
            return None;
        }
        match self.inspector_model() {
            Inspector::Patch(rows) => Some(rows),
            _ => None,
        }
    }
}

/// The column: the content, scrolling, under a key named for what it
/// shows, so its parts keep their UI IDs.
fn build(panel: &Panel) -> Element {
    let looks = panel.looks;
    let (key, label, content) = match &panel.model {
        Inspector::Patch(rows) => ("patch", "Patch".to_string(), patch(rows, &panel.authors, looks)),
        Inspector::Several(n) => {
            let words = format!("{n} modules selected");
            let line = Element::text(words.clone(), style::CONTROL_TEXT).colour(looks.text);
            ("several", words, line)
        }
        Inspector::Port(port) => {
            let column = Element::column([
                Element::text(port.name.clone(), style::HEADING_TEXT).colour(looks.text),
                Element::text(port.direction.clone(), style::BODY_TEXT).colour(looks.quiet),
                Element::text(
                    "A port of this sub-patch. Its name is the port's label on the tile outside.",
                    style::BODY_TEXT,
                )
                .colour(looks.text)
                .wrap()
                .width(Length::FILL),
            ])
            .spacing(6.0)
            .width(Length::FILL);
            return scroll(column, ids::panel(port.id).keys().last().cloned(), &port.name);
        }
        Inspector::Module(_) | Inspector::Unknown(_) | Inspector::Slot(_) | Inspector::SubPatch(_) => {
            let shown = rows::Shown {
                looks: panel.rows,
                drop_row: panel.drop_row,
            };
            let (id, label, content) = match &panel.model {
                Inspector::Module(m) => (m.id, m.name.clone(), rows::module(m, shown)),
                Inspector::Unknown(m) => (m.id, m.kind.clone(), rows::unknown(m, panel.rows)),
                Inspector::Slot(s) => (s.id, s.name.clone(), rows::slot(s, shown)),
                // A sub-patch may have no name yet; its column is named
                // for what it is then (section 7, rule 2).
                Inspector::SubPatch(s) => {
                    let label = if s.name.is_empty() {
                        "Sub-patch".to_string()
                    } else {
                        s.name.clone()
                    };
                    (s.id, label, rows::sub_patch(s, &panel.part_authors, shown))
                }
                _ => unreachable!("matched above"),
            };
            return scroll(content, ids::panel(id).keys().last().cloned(), &label);
        }
        // A module that went: nothing to show, and nothing to scroll.
        _ => return Element::column([]).width(Length::FILL).height(Length::FILL),
    };
    scroll(content, Some(key.into()), &label)
}

/// `content` in the column's scroll container, keyed `key`, padded as
/// Iced's was, and filling the slot.
fn scroll(content: Element, key: Option<loom_ui_core::IdKey>, label: &str) -> Element {
    let content = content.padding(12.0).width(Length::FILL);
    let scroll = Element::scroll(content, style::scroll())
        .height(Length::FILL)
        .min_height(0.0)
        .width(Length::FILL)
        // A scroll view named by what it shows, its offset and the cut
        // bounds of its parts the runtime's (section 7, rule 28).
        .access(Access::new(Role::ScrollView, label));
    match key {
        Some(key) => scroll.key(key),
        None => scroll,
    }
}

/// Patch info (patch-metadata.md, Patch info), then the hints and
/// Keyboard shortcuts, as Iced drew them.
fn patch(rows: &PatchRows, authors: &str, looks: Looks) -> Element {
    let quiet = |words: &'static str| Element::text(words, style::BODY_TEXT).colour(looks.quiet);
    let field = |element: Element| {
        element
            .padding(style::FIELD_PADDING)
            .background(looks.field.fill)
            .border(style::RULE, looks.field.border)
            .radius(looks.field.radius)
            .width(Length::FILL)
    };
    let mut parts = vec![
        Element::text(rows.name.clone(), style::HEADING_TEXT)
            .colour(looks.text)
            .key("name")
            .access(Access::label(rows.name.clone())),
        quiet("Authors, one per line"),
        field(Element::text_area(authors, style::CONTROL_TEXT).placeholder("Your name", looks.quiet))
            .key("authors")
            .takes_first(ESCAPE)
            .access(Access::new(Role::TextInput, "Authors, one per line")),
        quiet("Tags"),
    ];
    // No row at all without tags, so the field sits under its label.
    if !rows.tags.is_empty() {
        let chips = rows.tags.iter().map(|tag| chip(tag, looks));
        parts.push(
            Element::row(chips)
                .spacing(4.0)
                .wrap()
                .width(Length::FILL)
                .key("tag")
                .takes_first(TAG_KEYS)
                .access(Access::new(Role::Toolbar, "Tags")),
        );
    }
    // A refused tag gives the field the warning outline and its reason as
    // the description, as a range that does not pass does (ui-layer.md,
    // section 7, rule 23).
    let mut tags_access = Access::new(Role::TextInput, "Tags");
    let mut tags_field = field(
        Element::field(rows.tag_draft.clone(), style::CONTROL_TEXT)
            .placeholder("Type a tag, then Enter or comma", looks.quiet),
    );
    if let Some(note) = &rows.tag_note {
        tags_access = tags_access.description(note.clone());
        tags_field = tags_field.border(style::INVALID_BORDER, looks.danger);
    }
    parts.push(tags_field.key("tags").takes_first(ESCAPE).access(tags_access));
    if let Some(note) = &rows.tag_note {
        parts.push(
            small(note, looks.text)
                .key("tag_note")
                .access(Access::label(note.clone())),
        );
    }
    parts.push(quiet("Licence"));
    parts.push(
        Element::row([
            Element::text(rows.licence.clone(), style::CONTROL_TEXT)
                .colour(looks.text)
                .width(Length::FILL)
                .key("licence")
                .access(Access::label(format!("Licence: {}", rows.licence))),
            button("choose_licence", "Choose", "Choose licence"),
        ])
        .spacing(8.0)
        .align_y(Align::Center)
        .width(Length::FILL),
    );
    if let Some(summary) = &rows.licence_summary {
        parts.push(
            small(summary, looks.text)
                .key("licence_summary")
                .access(Access::label(summary.clone())),
        );
    }
    if let Some(url) = &rows.licence_url {
        parts.push(
            small(url, looks.quiet)
                .key("licence_url")
                .access(Access::label(url.clone())),
        );
    }
    if !rows.parts_by.is_empty() {
        let words = format!("Includes parts by {}", rows.parts_by.join(", "));
        parts.push(
            small(&words, looks.text)
                .key("parts_by")
                .access(Access::label(words.clone())),
        );
    }
    let info = Element::column(parts)
        .spacing(6.0)
        .width(Length::FILL)
        .align_x(Align::Start);
    let hint = "Select a module to edit it.";
    let add = "Press A over the canvas to add one.";
    Element::column([
        info,
        Element::space(Length::FILL, Length::Fixed(style::RULE)).background(looks.rule),
        Element::text(hint, style::BODY_TEXT)
            .colour(looks.quiet)
            .key("hint")
            .access(Access::label(hint)),
        Element::text(add, style::BODY_TEXT)
            .colour(looks.quiet)
            .key("add_hint")
            .access(Access::label(add)),
        button("shortcuts", "Keyboard shortcuts", "Keyboard shortcuts"),
    ])
    .spacing(8.0)
    .width(Length::FILL)
    .align_x(Align::Start)
}

/// One tag's chip: its name and a remove button, keyed by the tag, so the
/// button is `inspector/patch/tag/<tag>/remove`.
fn chip(tag: &str, looks: Looks) -> Element {
    let what = format!("Remove tag {tag}");
    Element::row([
        Element::text(tag, style::SMALL_TEXT).colour(looks.chip.text),
        Element::text("×", style::CONTROL_TEXT)
            .padding(loom_ui::Padding::symmetric(0.0, 6.0))
            .looks(ButtonRole::Plain.looks())
            .hover_text(what.clone())
            .key("remove")
            .focusable()
            .access(Access::new(Role::Button, what).action(Action::Click)),
    ])
    .spacing(2.0)
    .align_y(Align::Center)
    .padding(loom_ui::Padding::symmetric(0.0, 6.0))
    .background(looks.chip.fill)
    .radius(looks.chip.radius)
    .key(tag)
    .access(Access::new(Role::Group, format!("Tag {tag}")))
}

/// A line of small text in `colour`, wrapping to the column.
fn small(words: &str, colour: Color) -> Element {
    Element::text(words, style::SMALL_TEXT)
        .colour(colour)
        .wrap()
        .width(Length::FILL)
}

/// A button in the secondary role, keyed `key`, showing `label`, named
/// `name` for a screen reader.
fn button(key: &'static str, label: &'static str, name: &'static str) -> Element {
    let role = ButtonRole::Secondary;
    Element::text(label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(key)
        .focusable()
        .takes_first(ESCAPE)
        .access(Access::new(Role::Button, name).action(Action::Click))
}

mod routes;
pub(crate) mod rows;

#[cfg(test)]
mod tests;
