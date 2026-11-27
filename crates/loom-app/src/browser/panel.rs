//! The module browser as panels of `loom-ui`'s Overlays layer (plan 3.2,
//! row 5; module-browser.md, Focus and Tab): the browser at its corner,
//! closing on a press outside it, and a library row's menu under its row.
//!
//! The browser is a modal dialog holding a combobox and its list box, by
//! the WAI-ARIA patterns. The search field is its one focus stop and keeps
//! the focus, even while a row's menu is open: it takes the list's keys
//! before it edits (`Element::takes_first`), and its active descendant is
//! the highlighted row, or the menu's highlighted item. The list scrolls
//! from the field's unused scroll keys (`keys_from`) and the wheel.
//! Hovering a row moves the highlight, and a right press or a screen
//! reader's ShowContextMenu opens a library row's menu (ui-layer.md,
//! section 5, rule 19).

use super::{
    About, Browser, DESCRIPTION_HEIGHT, INPUT_HEIGHT, INSERT_LINKED, LIST_HEIGHT, Message, Nav, PADDING, ROW_HEIGHT,
    SPACING, WIDTH, browser_id, insert_linked_id, menu_id, search_id,
};
use crate::App;
use crate::style;
use loom_ui::access::{Access, Action, Role, States};
use loom_ui::{Align, Element, Layer, Length, Placement, View};
use loom_ui_core::{Color, IdKey, Key, KeyEvent, Point, UiId};

/// The keys the search field leaves to the browser, before it edits: the
/// list's, the menu's, and the keys that open a row's menu. Tab is never
/// taken this way; the field is the dialog's only stop, so Tab stays.
const KEYS: [Key; 8] = [
    Key::Up,
    Key::Down,
    Key::PageUp,
    Key::PageDown,
    Key::Enter,
    Key::Escape,
    Key::ContextMenu,
    Key::F10,
];

/// Width of a library item's menu.
const MENU_WIDTH: f32 = 140.0;

/// The colours the browser is painted in, from the style tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Looks {
    fill: Color,
    text: Color,
    quiet: Color,
    border: Color,
    border_width: f32,
    radius: f32,
    field: Color,
    row: style::RowLook,
}

/// One row as the list shows it.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    /// The key of the group it is in: its kind (`module`, `library`, ...).
    group: IdKey,
    /// Its own key in the group.
    key: IdKey,
    /// Its name.
    name: String,
    /// The words beside its swatch.
    mark: &'static str,
    /// The swatch's colour, the family's.
    swatch: Color,
    /// What a screen reader names it: its name, then its mark.
    label: String,
    /// What a screen reader says about it.
    description: String,
    /// A library item, which has a menu.
    library: bool,
    /// The highlighted row.
    highlighted: bool,
}

/// Everything the browser's panel shows, so the runtime builds it again
/// only when this changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    at: Point,
    query: String,
    rows: Vec<Row>,
    empty: Option<&'static str>,
    about: Option<About>,
    /// The field's active descendant: the menu's highlighted item, or the
    /// highlighted row.
    active: Option<UiId>,
    looks: Looks,
}

/// A library row's open menu, as its panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MenuPanel {
    /// The row it hangs under.
    row: UiId,
    /// Its one item is highlighted by the keys.
    highlighted: bool,
    /// The outline of the highlighted item, so the highlight never rests
    /// on its fill alone.
    outline: Color,
    fill: Color,
    hover: Color,
    text: Color,
}

impl App {
    /// Adds the open module browser, and its row's menu while one is open,
    /// to the runtime's view.
    pub(crate) fn browser_panel(&self, view: &mut View) {
        let Some(browser) = self.overlays.browser() else {
            return;
        };
        let floating = crate::style::FLOATING_PANEL;
        let looks = Looks {
            fill: floating.fill,
            text: floating.text,
            quiet: crate::style::QUIET,
            border: floating.border,
            border_width: floating.border_width,
            radius: floating.radius,
            field: crate::style::RAISED,
            row: crate::style::HIGHLIGHTED_ROW,
        };
        let panel = browser.panel(looks);
        view.panel_with(browser_id(), Layer::Overlays, &panel, build);
        if let Some(menu) = browser.menu_panel() {
            view.panel_with(menu_id(&menu.row), Layer::Overlays, &menu, build_menu);
        }
    }

    /// The browser's message for a click on one of its parts: a row adds
    /// it, and Insert linked inserts its row linked. From the pointer, or a
    /// screen reader's Click.
    pub(crate) fn browser_click(&self, id: &UiId) -> Option<crate::Message> {
        let model = self.overlays.browser()?.model();
        let message = if let Some(at) = model.rows.iter().position(|row| row.id == *id) {
            Message::Pick(at)
        } else {
            let at = model.menu?;
            (*id == insert_linked_id(&model.rows.get(at)?.id)).then_some(Message::InsertLinked(at))?
        };
        Some(crate::Message::Browser(message))
    }

    /// The pointer came onto a row: it is highlighted, as the keys would
    /// highlight it (module-browser.md). Only the highlight moves; nothing
    /// is chosen.
    pub(crate) fn browser_hovered(&self, id: &UiId) -> Option<crate::Message> {
        let model = self.overlays.browser()?.model();
        let at = model.rows.iter().position(|row| row.id == *id)?;
        Some(crate::Message::Browser(Message::Hover(at)))
    }

    /// A row's context menu: a library row opens its menu.
    pub(crate) fn browser_context_menu(&self, id: &UiId, by_key: bool) -> Option<crate::Message> {
        let model = self.overlays.browser()?.model();
        let row = model.rows.iter().position(|row| row.id == *id)?;
        Some(crate::Message::Browser(Message::Menu { row, by_key }))
    }

    /// A press outside the browser closes it; one outside a row's menu only
    /// closes the menu.
    pub(crate) fn browser_pressed_outside(&self, root: &UiId) -> Option<crate::Message> {
        let model = self.overlays.browser()?.model();
        if *root == browser_id() {
            return Some(crate::Message::Browser(Message::Close));
        }
        let open = model.rows.get(model.menu?)?;
        (*root == menu_id(&open.id)).then_some(crate::Message::Browser(Message::CloseMenu))
    }

    /// A row's menu whose row scrolled out of view, or was cut away, closes
    /// with no choice, as a select's list does when its select scrolls
    /// away (ui-layer.md, section 5, rules 13 and 18). The focus never left
    /// the search field, so it has the keys again at once.
    pub(crate) fn browser_anchor_hidden(&self, root: &UiId) -> Option<crate::Message> {
        let model = self.overlays.browser()?.model();
        let open = model.rows.get(model.menu?)?;
        (*root == menu_id(&open.id)).then_some(crate::Message::Browser(Message::CloseMenu))
    }

    /// The browser's message for a key the search field leaves to it
    /// (`KEYS`), or `None` to let the field have it: Shift+F10 opens a
    /// row's menu, and F10 alone goes on.
    pub(crate) fn browser_panel_key(&self, id: &UiId, key: &KeyEvent) -> Option<crate::Message> {
        self.overlays.browser()?;
        let KeyEvent::Pressed { key, modifiers, .. } = key else {
            return None;
        };
        if *id != search_id() {
            return None;
        }
        let message = match key {
            Key::Up => Message::Key(Nav::Up),
            Key::Down => Message::Key(Nav::Down),
            Key::PageUp => Message::Key(Nav::PageUp),
            Key::PageDown => Message::Key(Nav::PageDown),
            Key::Escape => Message::Key(Nav::Escape),
            Key::Enter => Message::Submit,
            Key::ContextMenu => Message::Key(Nav::Menu),
            Key::F10 if modifiers.shift && !modifiers.meta && !modifiers.control && !modifiers.alt => {
                Message::Key(Nav::Menu)
            }
            _ => return None,
        };
        Some(crate::Message::Browser(message))
    }

    /// A library row's open menu, as the tree names it around its item: the
    /// row's name, and the menu's UI ID. Only while the browser owns the
    /// input.
    pub(crate) fn browser_menu_group(&self) -> Option<(String, UiId)> {
        self.browser_stops().first()?;
        let model = self.overlays.browser()?.model();
        let row = model.rows.get(model.menu?)?;
        Some((row.name.clone(), menu_id(&row.id)))
    }

    /// The browser's focus stops: its search field, while it is open and
    /// owns the input.
    pub(crate) fn browser_stops(&self) -> Vec<UiId> {
        let owns = self.overlays.owner() == crate::input_owner::InputOwner::Browser;
        if owns && self.overlays.browser().is_some() {
            vec![search_id()]
        } else {
            Vec::new()
        }
    }
}

impl Browser {
    /// The browser's panel model, in `looks`.
    fn panel(&self, looks: Looks) -> Panel {
        let model = &self.model;
        let rows = model
            .rows
            .iter()
            .enumerate()
            .map(|(at, row)| {
                let keys = row.id.keys();
                let (key, group) = match keys {
                    [.., group, key] => (key.clone(), group.clone()),
                    _ => unreachable!("a row's UI ID is browser/list/<kind>/<name>"),
                };
                Row {
                    group,
                    key,
                    name: row.name.clone(),
                    mark: row.mark,
                    swatch: super::family_swatch(row.family),
                    label: super::access::label(row),
                    description: row.description.clone(),
                    library: row.library,
                    highlighted: model.highlight == Some(at),
                }
            })
            .collect();
        let highlighted = model.highlight.and_then(|at| model.rows.get(at));
        let active = match (model.menu, model.menu_highlight) {
            (Some(open), Some(_)) => model.rows.get(open).map(|row| insert_linked_id(&row.id)),
            _ => highlighted.map(|row| row.id.clone()),
        };
        Panel {
            at: self.at,
            query: model.query.clone(),
            rows,
            empty: model.empty,
            about: model.about.clone(),
            active,
            looks,
        }
    }

    /// The open row menu's panel model, if a menu is open.
    fn menu_panel(&self) -> Option<MenuPanel> {
        let row = self.model.rows.get(self.model.menu?)?;
        Some(MenuPanel {
            row: row.id.clone(),
            highlighted: self.model.menu_highlight.is_some(),
            outline: crate::style::ACCENT,
            fill: crate::style::PANEL_FILL,
            hover: crate::style::RAISED,
            text: crate::style::TEXT,
        })
    }
}

/// The browser: its search field, the list or the line in its place, and
/// what the highlighted row is, in a floating box at its corner.
fn build(panel: &Panel) -> Element {
    let looks = panel.looks;
    let mut access = Access::new(Role::TextInput, "Search modules");
    if let Some(active) = &panel.active {
        access = access.active_descendant(active.clone());
    }
    let search = Element::field(panel.query.clone(), style::CONTROL_TEXT)
        .placeholder("Search modules", looks.quiet)
        .padding(6.0)
        .width(Length::FILL)
        .height(Length::Fixed(INPUT_HEIGHT))
        .background(looks.field)
        .radius(style::FIELD_RADIUS)
        .key("search")
        .takes_first(KEYS)
        .access(access);
    let list = match panel.empty {
        Some(line) => Element::column([Element::text(line, style::CONTROL_TEXT)
            .colour(looks.quiet)
            .wrap()
            .key("empty")
            .access(Access::label(line))])
        .padding(8.0)
        .width(Length::FILL)
        .height(Length::Fixed(LIST_HEIGHT))
        .key("list"),
        None => list(panel),
    };
    let about = match &panel.about {
        Some(about) => Element::column([
            Element::text(about.text.clone(), style::BODY_TEXT)
                .colour(looks.text)
                .wrap(),
            Element::text(about.detail.clone(), style::SMALL_TEXT).colour(looks.quiet),
        ])
        .spacing(2.0)
        .key("about")
        .access(Access::label(format!("{} {}", about.text, about.detail))),
        None => Element::column([]),
    };
    let about = Element::column([about])
        .width(Length::FILL)
        .height(Length::Fixed(DESCRIPTION_HEIGHT))
        .clip();
    // The border takes room, so the padding gives it back: the parts
    // inside keep their widths, and the panel stays `WIDTH` by `HEIGHT`,
    // which the canvas keeps it inside by.
    Element::column([search, list, about])
        .spacing(SPACING)
        .padding(PADDING - looks.border_width)
        .width(Length::Fixed(WIDTH))
        .background(looks.fill)
        .border(looks.border_width, looks.border)
        .radius(looks.radius)
        .shadow(style::MENU_SHADOW)
        .placed(Placement::At(panel.at))
        .closes_on_press_outside()
        .opaque()
}

/// The list box: the rows, in a group per kind so each row's UI ID is
/// `browser/list/<kind>/<name>`, scrolling under the field's keys.
fn list(panel: &Panel) -> Element {
    let size = panel.rows.len();
    let mut groups: Vec<(IdKey, Vec<Element>)> = Vec::new();
    for (at, row) in panel.rows.iter().enumerate() {
        let element = row_element(row, at, panel.looks);
        match groups.last_mut() {
            Some((group, rows)) if *group == row.group => rows.push(element),
            _ => groups.push((row.group.clone(), vec![element])),
        }
    }
    let column = Element::column(
        groups
            .into_iter()
            .map(|(group, rows)| Element::column(rows).spacing(4.0).width(Length::FILL).key(group)),
    )
    .spacing(4.0)
    .width(Length::FILL);
    Element::scroll(column, style::scroll())
        .key("list")
        .keys_from(search_id())
        .width(Length::FILL)
        .height(Length::Fixed(LIST_HEIGHT))
        .access(Access::new(Role::ListBox, "Modules").size_of_set(size))
}

/// One row: its name, its family's swatch, and its mark, so colour is never
/// the only cue (ADR-0029).
fn row_element(row: &Row, at: usize, looks: Looks) -> Element {
    let swatch = Element::space(Length::Fixed(8.0), Length::Fixed(8.0))
        .background(row.swatch)
        .radius(style::SWATCH_RADIUS);
    let mut element = Element::row([
        Element::text(row.name.clone(), style::CONTROL_TEXT).colour(looks.text),
        swatch,
        Element::text(row.mark, style::SMALL_TEXT).colour(looks.quiet),
    ])
    .spacing(8.0)
    .align_y(Align::Center)
    .padding(loom_ui::Padding::symmetric(0.0, 8.0))
    .width(Length::FILL)
    .height(Length::Fixed(ROW_HEIGHT));
    if row.highlighted {
        element = element
            .background(looks.row.fill)
            .border(looks.row.border_width, looks.row.border)
            .radius(looks.row.radius);
    }
    let access = Access::new(Role::ListBoxOption, row.label.clone())
        .description(row.description.clone())
        .states(States {
            selected: row.highlighted,
            ..States::default()
        })
        .position_in_set(at)
        .action(Action::Click);
    let element = element.key(row.key.clone()).reports_hover().access(access);
    if row.library { element.context_menu() } else { element }
}

/// A library row's menu: Insert linked, under its row, closing on a press
/// outside. Its item is no focus stop: the field keeps the focus.
fn build_menu(menu: &MenuPanel) -> Element {
    let hovered = loom_ui::StateLook {
        background: Some(menu.hover),
        text: None,
    };
    let normal = loom_ui::StateLook {
        background: menu.highlighted.then_some(menu.hover),
        text: None,
    };
    let item = Element::text(INSERT_LINKED, style::CONTROL_TEXT)
        .colour(menu.text)
        .padding(style::BUTTON_PADDING)
        .width(Length::FILL)
        // The same width either way, so the highlight moves nothing.
        .border(
            style::MENU_BORDER,
            if menu.highlighted {
                menu.outline
            } else {
                crate::style::QUIET
            },
        )
        .radius(style::MENU_ITEM_RADIUS)
        .looks(loom_ui::Looks {
            normal,
            hovered,
            pressed: hovered,
            off: loom_ui::StateLook::default(),
        })
        .key("insert-linked")
        .access(Access::new(Role::MenuItem, INSERT_LINKED).action(Action::Click));
    Element::column([item])
        .padding(4.0)
        .width(Length::Fixed(MENU_WIDTH))
        .background(menu.fill)
        .border(style::MENU_BORDER, crate::style::QUIET)
        .radius(style::MENU_RADIUS)
        .shadow(style::MENU_SHADOW)
        .placed(Placement::Under(menu.row.clone()))
        .closes_on_press_outside()
}

#[cfg(test)]
mod tests;
