//! Your patches as panels of `loom-ui`'s Overlays layer (plan 3.2, row 5b;
//! patch-metadata.md, Your patches browser): a backdrop over the window
//! and a centred box with Close, the two fields, the licence check boxes,
//! the tags, the list, and the folder line.
//!
//! It is a modal dialog (the WAI-ARIA modal dialog pattern). Tab loops
//! through Close, the search field, the Author field, each licence check
//! box, then the tags as one stop: a toolbar, whose Left, Right, Home, and
//! End move among the tags (the WAI-ARIA toolbar pattern). Each field holds
//! the list as a combobox does: it takes Up, Down, Page Up, Page Down,
//! Enter, and Escape before it edits, and its active descendant is the
//! highlighted row. Hovering a row highlights it.

use super::{Message, PAGE_ROWS, PatchBrowser, ROW_HEIGHT, licence_id, part_id, patches_id, tag_id};
use crate::App;
use crate::style::{self, Role as ButtonRole};
use loom_ui::access::{Access, Action, Role, States, Value};
use loom_ui::{Align, Element, Layer, Length, View};
use loom_ui_core::{Color, IdKey, Key, KeyEvent, UiId};

/// The keys each field leaves to the browser before it edits: the list's.
/// Tab is never taken this way.
const FIELD_KEYS: [Key; 6] = [Key::Up, Key::Down, Key::PageUp, Key::PageDown, Key::Enter, Key::Escape];

/// The keys the tags take, as one toolbar stop, with Escape, which closes
/// Your patches from any of its stops.
const TAG_KEYS: [Key; 5] = [Key::Left, Key::Right, Key::Home, Key::End, Key::Escape];

/// The key Close and each check box take first: Escape closes Your patches
/// from any stop, through the same hook the fields use, with no listener
/// of Iced's.
const ESCAPE: [Key; 1] = [Key::Escape];

/// The search field's words, shown while it is empty and given to a screen
/// reader as its name.
const SEARCH: &str = "Search names, descriptions, and tags";

/// Width of the box, as before it moved.
const WIDTH: f32 = 720.0;

/// Width of the Author field.
const AUTHOR_WIDTH: f32 = 200.0;

/// The colours Your patches is painted in, from the style tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Looks {
    scrim: Color,
    fill: Color,
    text: Color,
    quiet: Color,
    field: Color,
    radius: f32,
    row: style::RowLook,
    /// A picked tag's fill and outline.
    picked: Color,
    picked_border: Color,
}

/// One row as the list shows it.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    key: IdKey,
    name: String,
    details: String,
    readable: bool,
    highlighted: bool,
}

/// Everything the box shows, so the runtime builds it again only when this
/// changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    search: String,
    author: String,
    licences: Vec<(&'static str, &'static str, bool)>,
    tags: Vec<(String, bool)>,
    rows: Vec<Row>,
    empty: Option<String>,
    folder: String,
    /// Each field's active descendant: the highlighted row.
    active: Option<UiId>,
    looks: Looks,
}

/// The UI ID of the backdrop's own panel, so a press on it is told apart
/// from one on the box.
fn backdrop_panel() -> UiId {
    patches_id().with("backdrop")
}

/// The UI ID of the backdrop, which closes Your patches on a press.
fn backdrop_id() -> UiId {
    backdrop_panel().with("scrim")
}

/// The UI ID of the tags' toolbar.
pub(crate) fn tags_id() -> UiId {
    patches_id().with("tag")
}

impl App {
    /// Adds Your patches' panels to the runtime's view while it is open.
    pub(crate) fn patches_panel(&self, view: &mut View) {
        let Some(browser) = self.overlays.patches() else {
            return;
        };
        let dialog = crate::style::DIALOG;
        let looks = Looks {
            scrim: style::SCRIM,
            fill: dialog.fill,
            text: dialog.text,
            quiet: crate::style::QUIET,
            field: crate::style::RAISED,
            radius: dialog.radius,
            row: crate::style::HIGHLIGHTED_ROW,
            picked: crate::style::RAISED,
            picked_border: crate::style::EDGE,
        };
        let panel = browser.panel(looks);
        view.panel_with(backdrop_panel(), Layer::Overlays, &looks.scrim, backdrop);
        view.panel_with(patches_id(), Layer::Overlays, &panel, build);
    }

    /// The message for a click on a part of Your patches: Close and the
    /// backdrop close it, a check box turns its filter on or off, a tag is
    /// picked or unpicked, and a row opens its patch. From the pointer,
    /// Enter or Space, or a screen reader.
    pub(crate) fn patches_click(&self, id: &UiId) -> Option<crate::Message> {
        let model = self.overlays.patches()?.model();
        let message = if *id == part_id("close") || *id == backdrop_id() {
            Message::Close
        } else if let Some(licence) = model.licences.iter().find(|licence| licence_id(licence.key) == *id) {
            Message::Licence(licence.filter, !licence.on)
        } else if let Some(tag) = model.tags.iter().find(|tag| tag_id(&tag.text) == *id) {
            Message::Tag(tag.text.clone())
        } else {
            Message::Pick(model.rows.iter().position(|row| row.id == *id)?)
        };
        Some(crate::Message::Patches(message))
    }

    /// The pointer came onto a row: it is highlighted, and nothing opens.
    pub(crate) fn patches_hovered(&self, id: &UiId) -> Option<crate::Message> {
        let model = self.overlays.patches()?.model();
        let at = model.rows.iter().position(|row| row.id == *id)?;
        Some(crate::Message::Patches(Message::Hover(at)))
    }

    /// The message for a key a field or the tags take first: the list's keys
    /// from either field, the toolbar's keys on the tags.
    pub(crate) fn patches_panel_key(&self, id: &UiId, key: &KeyEvent) -> Option<crate::Message> {
        let browser = self.overlays.patches()?;
        let KeyEvent::Pressed { key, .. } = key else {
            return None;
        };
        // Escape closes it from any of its stops (the WAI-ARIA modal dialog
        // pattern).
        if *key == Key::Escape && id.is_under(&patches_id()) {
            return Some(crate::Message::Patches(Message::Close));
        }
        let message = if super::text_fields().contains(id) {
            match key {
                Key::Up => Message::Step(-1),
                Key::Down => Message::Step(1),
                Key::PageUp => Message::Step(-PAGE_ROWS),
                Key::PageDown => Message::Step(PAGE_ROWS),
                Key::Enter => Message::Submit,
                Key::Escape => Message::Close,
                _ => return None,
            }
        } else if *id == tags_id() {
            let tags = &browser.model().tags;
            let last = tags.len().checked_sub(1)?;
            let at = self
                .focus
                .as_ref()
                .and_then(|focus| tags.iter().position(|tag| tag_id(&tag.text) == *focus))
                .unwrap_or(0);
            // Left and Right stop at the ends, as the app's toolbar does.
            let to = match key {
                Key::Left => at.saturating_sub(1),
                Key::Right => (at + 1).min(last),
                Key::Home => 0,
                Key::End => last,
                _ => return None,
            };
            Message::FocusTag(tags[to].text.clone())
        } else {
            return None;
        };
        Some(crate::Message::Patches(message))
    }

    /// Your patches' focus stops, in the Tab order: Close, the search
    /// field, the Author field, each licence check box, then the tags as
    /// one stop, on the tag last used, or the first. Empty unless it is
    /// open and owns the input.
    pub(crate) fn patches_stops(&self) -> Vec<UiId> {
        let Some(browser) = self.overlays.patches() else {
            return Vec::new();
        };
        if self.overlays.owner() != crate::input_owner::InputOwner::Patches {
            return Vec::new();
        }
        let model = browser.model();
        let mut stops = vec![part_id("close"), part_id("search"), part_id("author")];
        stops.extend(model.licences.iter().map(|licence| licence_id(licence.key)));
        if let Some(entry) = browser.tag_entry() {
            stops.push(tag_id(entry));
        }
        stops
    }

    /// Every tag's UI ID, while Your patches owns the input: each stands for
    /// the toolbar's one stop.
    pub(crate) fn patches_tags(&self) -> Vec<UiId> {
        match self.overlays.patches() {
            Some(browser) if self.overlays.owner() == crate::input_owner::InputOwner::Patches => {
                browser.model().tags.iter().map(|tag| tag_id(&tag.text)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Where Tab goes from `from` inside Your patches, or the one before
    /// when `back`, wrapping at both ends (the WAI-ARIA modal dialog
    /// pattern). Any tag stands for the toolbar's one stop. `None` when
    /// `from` is no part of it.
    pub(crate) fn patches_next_stop(&self, from: &UiId, back: bool) -> Option<UiId> {
        let stops = self.patches_stops();
        let from = if self.patches_tags().contains(from) {
            stops.last()?
        } else {
            from
        };
        let at = stops.iter().position(|stop| stop == from)?;
        let last = stops.len() - 1;
        let to = match back {
            false if at == last => 0,
            false => at + 1,
            true => at.checked_sub(1).unwrap_or(last),
        };
        stops.get(to).cloned()
    }
}

impl PatchBrowser {
    /// The box's panel model, in `looks`.
    fn panel(&self, looks: Looks) -> Panel {
        let model = self.model();
        let rows = model
            .rows
            .iter()
            .enumerate()
            .map(|(at, row)| Row {
                key: row.id.keys().last().cloned().expect("a row's UI ID has keys"),
                name: row.name.clone(),
                details: row.details.clone(),
                readable: row.readable,
                highlighted: model.highlight == Some(at),
            })
            .collect();
        Panel {
            search: model.search.clone(),
            author: model.author.clone(),
            licences: model
                .licences
                .iter()
                .map(|licence| (licence.key, licence.label, licence.on))
                .collect(),
            tags: model.tags.iter().map(|tag| (tag.text.clone(), tag.picked)).collect(),
            rows,
            empty: model.empty.clone(),
            folder: model.folder.clone(),
            active: model
                .highlight
                .and_then(|at| model.rows.get(at))
                .map(|row| row.id.clone()),
            looks,
        }
    }
}

/// The backdrop over the whole window, which takes every press under it.
fn backdrop(scrim: &Color) -> Element {
    Element::column([])
        .width(Length::FILL)
        .height(Length::FILL)
        .background(*scrim)
        .key("scrim")
        .opaque()
}

/// The box, centred over the backdrop.
fn build(panel: &Panel) -> Element {
    let looks = panel.looks;
    let title = Element::row([
        Element::text("Your patches", style::TITLE_TEXT).colour(looks.text),
        Element::space(Length::FILL, Length::Shrink),
        button("close", "Close"),
    ])
    .align_y(Align::Center)
    .width(Length::FILL);
    let field = |key: &'static str, text: &str, label: &'static str, width: Length| {
        let mut access = Access::new(Role::TextInput, label);
        if let Some(active) = &panel.active {
            access = access.active_descendant(active.clone());
        }
        Element::field(text.to_string(), style::CONTROL_TEXT)
            .placeholder(label, looks.quiet)
            .padding(6.0)
            .width(width)
            .background(looks.field)
            .radius(style::FIELD_RADIUS)
            .key(key)
            .takes_first(FIELD_KEYS)
            .access(access)
    };
    let search = field("search", &panel.search, SEARCH, Length::FILL);
    let licences = Element::row(panel.licences.iter().map(|(key, label, on)| {
        Element::checkbox(*label, style::CONTROL_TEXT, *on)
            .key(*key)
            .takes_first(ESCAPE)
    }))
    .spacing(12.0)
    .key("licence");
    let filters = Element::row([
        field("author", &panel.author, "Author", Length::Fixed(AUTHOR_WIDTH)),
        licences,
    ])
    .spacing(12.0)
    .align_y(Align::Center);
    let tags = Element::row(panel.tags.iter().map(|(text, picked)| tag(text, *picked, looks)))
        .spacing(4.0)
        .wrap()
        .width(Length::FILL)
        .key("tag")
        .takes_first(TAG_KEYS)
        .access(Access::new(Role::Toolbar, "Tags"));
    let list = match &panel.empty {
        Some(line) => Element::text(line.clone(), style::CONTROL_TEXT)
            .colour(looks.quiet)
            .wrap()
            .padding(8.0)
            .width(Length::FILL)
            .height(Length::Fixed(super::LIST_HEIGHT))
            .key("empty")
            .access(Access::label(line.clone())),
        None => list(panel),
    };
    let folder = Element::text(panel.folder.clone(), style::SMALL_TEXT)
        .colour(looks.quiet)
        .key("folder")
        .access(Access::label(panel.folder.clone()));
    let dialog = Element::column([title, search, filters, tags, list, folder])
        .spacing(8.0)
        .padding(16.0)
        .max_width(WIDTH)
        .width(Length::FILL)
        .min_height(0.0)
        .background(looks.fill)
        .radius(looks.radius)
        .opaque();
    Element::column([dialog])
        .width(Length::FILL)
        .height(Length::FILL)
        .padding(16.0)
        .align_x(Align::Center)
        .align_y(Align::Center)
}

/// The list box of patches, scrolling under the search field's keys.
fn list(panel: &Panel) -> Element {
    let size = panel.rows.len();
    let rows = Element::column(
        panel
            .rows
            .iter()
            .enumerate()
            .map(|(at, row)| row_element(row, at, panel.looks)),
    )
    .spacing(4.0)
    .width(Length::FILL)
    .key("row");
    Element::scroll(rows, style::scroll())
        .key("list")
        .keys_from(part_id("search"))
        .width(Length::FILL)
        .height(Length::Fixed(super::LIST_HEIGHT))
        .min_height(0.0)
        .access(Access::new(Role::ListBox, "Patches").size_of_set(size))
}

/// One row: the patch's name and the line under it. A file that does not
/// read says so in its name and offers no Click; it stays enabled, so a
/// screen reader does not skip it (ui-layer.md, section 7, rule 2).
fn row_element(row: &Row, at: usize, looks: Looks) -> Element {
    let mut element = Element::column([
        Element::text(row.name.clone(), style::CONTROL_TEXT).colour(looks.text),
        Element::text(row.details.clone(), style::SMALL_TEXT).colour(looks.quiet),
    ])
    .spacing(2.0)
    .padding(loom_ui::Padding::symmetric(4.0, 8.0))
    .width(Length::FILL)
    .height(Length::Fixed(ROW_HEIGHT));
    if row.highlighted {
        element = element
            .background(looks.row.fill)
            .border(looks.row.border_width, looks.row.border)
            .radius(looks.row.radius);
    }
    let name = if row.readable {
        row.name.clone()
    } else {
        format!("{}, could not read", row.name)
    };
    let mut access = Access::new(Role::ListBoxOption, name)
        .states(States {
            selected: row.highlighted,
            ..States::default()
        })
        .position_in_set(at);
    // Only a row that can be opened offers Click: a file that does not
    // read is listed, enabled, and says why in its name.
    if row.readable {
        access = access.action(Action::Click);
    }
    if row.readable && !row.details.is_empty() {
        access = access.description(row.details.clone());
    }
    element.key(row.key.clone()).reports_hover().access(access)
}

/// One tag: a toggle button, pressed while it filters, filled and outlined
/// like an open tab, so the state never rests on colour alone.
fn tag(text: &str, picked: bool, looks: Looks) -> Element {
    let normal = loom_ui::StateLook {
        background: picked.then_some(looks.picked),
        text: Some(if picked { looks.text } else { looks.quiet }),
    };
    let hovered = loom_ui::StateLook {
        background: Some(looks.picked),
        text: Some(looks.text),
    };
    Element::text(text.to_string(), style::SMALL_TEXT)
        .padding(loom_ui::Padding::symmetric(2.0, 8.0))
        .border(
            style::TAG_BORDER,
            if picked {
                looks.picked_border
            } else {
                Color::TRANSPARENT
            },
        )
        .radius(style::TAG_RADIUS)
        .looks(loom_ui::Looks {
            normal,
            hovered,
            pressed: hovered,
            off: loom_ui::StateLook::default(),
        })
        .key(text)
        .focusable()
        .access(
            Access::new(Role::Button, text)
                .value(Value::On(picked))
                .action(Action::Click),
        )
}

/// A button in the secondary role, keyed `key`, which takes Escape first.
fn button(key: &'static str, label: &'static str) -> Element {
    let role = ButtonRole::Secondary;
    Element::text(label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(key)
        .takes_first(ESCAPE)
        .focusable()
        .access(Access::new(Role::Button, label).action(Action::Click))
}

#[cfg(test)]
mod tests;
