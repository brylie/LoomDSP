//! The probe menu and Recent as panels of `loom-ui`'s Overlays layer
//! (ui-layer.md, section 6, rule 8; plan 3.2, row 3b): placed where they
//! were asked for, closing on a press outside, with the menu shadow. Their
//! items are menu items and focus stops, and the menu's highlight is the
//! runtime's focus on an item. The tile menu is one too (row 3d): placed
//! at the tile's top-left corner from the keyboard, or where the pointer
//! pressed. So is the cable picker: centred in the window, with its name
//! as a heading over a scrolling list, closing on a press outside as
//! the others do.

use super::{MenuKind, MenuModel};
use crate::style;
use crate::{App, Message};
use loom_ui::access::{Access, Action, Role};
use loom_ui::{Element, Layer, Length, Placement, View};
use loom_ui_core::{Color, IdKey, Key, KeyEvent, UiId};

/// The width of the tile menu, in logical pixels: its items' 200 and the
/// menu's padding, as Iced drew it.
const TILE_MENU_WIDTH: f32 = 208.0;

/// The room inside a placed menu's border, around its rows.
const MENU_PADDING: f32 = 4.0;

/// The width of the cable picker, in logical pixels, as Iced drew it.
const CABLE_MENU_WIDTH: f32 = 270.0;

/// The most height the cable picker takes, in logical pixels, as Iced drew
/// it: a longer list scrolls inside it.
const CABLE_MENU_HEIGHT: f32 = 350.0;

/// How many rows Page Up and Page Down move in the cable picker: about as
/// many as its box shows, as the module browser's list moves by its own.
const CABLE_PAGE_ROWS: usize = 10;

/// One item as the menu's panel shows it.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    /// Its keys under the menu's UI ID: one for most items, more for
    /// one such as `make-slot/<role>` (ui-layer.md, section 4), so the
    /// element the runtime lays out has the item's own UI ID.
    path: Vec<IdKey>,
    /// Its place in the menu, from 0, in the model's order: a row inside a
    /// column says it, so a screen reader counts it among all the items,
    /// not among the column's.
    position: usize,
    /// What it says.
    label: String,
    /// A second, quiet line under it: a Recent file's folder.
    second: Option<String>,
    /// Shown dimmed: a Recent file that was not there.
    dim: bool,
    /// Turned off: it picks nothing, and keeps the focus (rule 2).
    off: bool,
    /// The menu's highlighted item, which is its focused one: the weak
    /// fill, whether the pointer or the keys put it there (Brylie's
    /// decision, 2026-11-20), with the focus ring on top from the keys.
    highlighted: bool,
}

/// Everything the menu's panel shows, so the runtime builds it again only
/// when this changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    /// Its items, in order.
    items: Vec<Item>,
    /// Where it goes: placed, or `None` for centred in the window, with
    /// its heading over a scrolling list (the cable picker).
    placement: Option<Placement>,
    /// Its name over its items, in a centred menu.
    heading: Option<String>,
    /// A placed menu's rows go in a scroll keyed `list`, as Recent's do: a
    /// list long enough to be cut to the window then scrolls (ui-layer.md,
    /// section 5, rule 13).
    scrolls: bool,
    /// How wide it is.
    width: f32,
    /// Its fill.
    fill: Color,
    /// A hovered or pressed item's fill.
    hover: Color,
    /// The colour of its text.
    text: Color,
}

impl App {
    /// Adds the open probe menu or Recent list to the runtime's view.
    pub(crate) fn menu_panel(&self, view: &mut View) {
        let Some(menu) = self.runtime_menu() else {
            return;
        };
        let (placement, width) = match menu.kind {
            MenuKind::Probe => match self.overlays.probe_menu() {
                Some(probe) => (Some(Placement::At(probe.at())), crate::probe_menu::WIDTH),
                None => return,
            },
            MenuKind::Recent => (
                Some(Placement::Under(crate::toolbar::toolbar_id().with("recent"))),
                crate::recent::LIST_WIDTH,
            ),
            MenuKind::Tile(_) => match self.overlays.tile_menu() {
                Some((_, at)) => (Some(Placement::At(at)), TILE_MENU_WIDTH),
                None => return,
            },
            // Centred, not anchored: it opens from a tile by a key and
            // lists ports from across the level.
            MenuKind::Cable => (None, CABLE_MENU_WIDTH),
        };
        // The cable picker's rows and Recent's are keyed under their list,
        // which keeps the scroll's offset.
        let base = match menu.kind {
            MenuKind::Cable => crate::cable_picker::list_id(),
            MenuKind::Recent => super::recent_list_id(),
            _ => menu.id.clone(),
        };
        // With rows, which are all menu items and focus stops, the list is
        // never a stop itself, so the menu still holds only its items (the
        // WAI-ARIA menu pattern); with none there is nothing to scroll.
        let scrolls = menu.kind == MenuKind::Recent && !menu.items.is_empty();
        let missing: Vec<bool> = match menu.kind {
            MenuKind::Recent => self
                .overlays
                .recent_rows()
                .map(|rows| rows.iter().map(|row| row.missing).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let panel = Panel {
            items: menu
                .items
                .iter()
                .enumerate()
                .map(|(at, item)| Item {
                    path: item.id.keys()[base.keys().len()..].to_vec(),
                    position: at,
                    label: item.label.clone(),
                    second: item.description.clone(),
                    dim: missing.get(at).copied().unwrap_or(false),
                    off: item.message.is_none(),
                    highlighted: menu.highlight == Some(at),
                })
                .collect(),
            heading: placement.is_none().then(|| menu.label.clone()),
            scrolls,
            placement,
            width,
            fill: crate::style::PANEL_FILL,
            hover: crate::style::RAISED,
            text: crate::style::TEXT,
        };
        view.panel_with(menu.id, Layer::Overlays, &panel, build);
    }

    /// The open menu: the tile menu, the probe menu, or Recent, each the
    /// runtime's.
    pub(crate) fn runtime_menu(&self) -> Option<MenuModel> {
        // Only while it owns the input: under Help or a question it is
        // covered, so it is neither drawn nor reported, and refuses a
        // screen reader's actions (section 7, rule 24).
        let owns = matches!(
            self.overlays.owner(),
            crate::input_owner::InputOwner::ProbeMenu
                | crate::input_owner::InputOwner::RecentList
                | crate::input_owner::InputOwner::TileMenu
        );
        // The cable picker shares the tile menu's owner.
        self.menu_model().filter(|_| owns)
    }

    /// The message for a click on one of the runtime menu's items, from the
    /// pointer, Enter or Space, or a screen reader: what picking it sends.
    pub(crate) fn menu_click(&self, id: &UiId) -> Option<Message> {
        let menu = self.runtime_menu()?;
        menu.items
            .into_iter()
            .find(|item| item.id == *id)
            .and_then(|item| item.message)
    }

    /// The message for a press outside the runtime menu `root`: close it
    /// (section 5, rule 14).
    pub(crate) fn menu_pressed_outside(&self, root: &UiId) -> Option<Message> {
        if *root == super::probe_id() {
            Some(Message::ProbeMenu(crate::probe_menu::Message::Close))
        } else if *root == super::recent_id() {
            Some(Message::Recent(crate::recent::Message::Close))
        } else if *root == crate::cable_picker::cable_id()
            || self
                .overlays
                .tile_menu()
                .is_some_and(|(module, _)| *root == super::tile_id(module))
        {
            Some(Message::CloseTileMenu)
        } else {
            None
        }
    }

    /// The message for Up, Down, Home, or End inside the cable picker's
    /// list or Recent's, or Page Up or Page Down inside the cable picker's,
    /// which take them before their scroll would: the focus moves to the next item, the one before, the
    /// first, or the last, as on any menu, or a page on, stopping at the
    /// ends, as the module browser's list does (section 6, rule 8). `None`
    /// for any other element or key.
    pub(crate) fn menu_panel_key(&self, id: &UiId, key: &KeyEvent) -> Option<Message> {
        let menu = self.runtime_menu()?;
        let list = match menu.kind {
            MenuKind::Cable => crate::cable_picker::list_id(),
            MenuKind::Recent => super::recent_list_id(),
            _ => return None,
        };
        let KeyEvent::Pressed { key, .. } = key else {
            return None;
        };
        if *id != list {
            return None;
        }
        let last = menu.items.len().checked_sub(1)?;
        let at = menu.highlight.unwrap_or(0);
        match key {
            Key::PageUp => {
                return Some(Message::MenuMove {
                    to: at.saturating_sub(CABLE_PAGE_ROWS),
                });
            }
            Key::PageDown => {
                return Some(Message::MenuMove {
                    to: (at + CABLE_PAGE_ROWS).min(last),
                });
            }
            _ => {}
        }
        let to = super::step(menu.highlight, menu.items.len(), key)?;
        Some(Message::MenuMove { to })
    }

    /// True while one of the runtime's overlays owns the input: the
    /// question, help, the probe menu, or Recent.
    pub(crate) fn runtime_overlay_open(&self) -> bool {
        self.overlays.question.is_some()
            || self.overlays.help.is_some()
            || self.overlays.chooser.is_some()
            || self.overlays.preferences().is_some()
            || self.runtime_menu().is_some()
            || self.overlays.browser().is_some()
            || self.overlays.patches().is_some()
    }

    /// The runtime menu's items as focus stops, in order.
    pub(crate) fn menu_stops(&self) -> Vec<UiId> {
        self.runtime_menu()
            .map(|menu| menu.items.into_iter().map(|item| item.id).collect())
            .unwrap_or_default()
    }
}

/// The menu: a column of items in a box with the menu shadow, placed, and
/// closing on a press outside. The cable picker's box is centred, with its
/// heading over its rows in a scroll.
fn build(panel: &Panel) -> Element {
    let items: Vec<&Item> = panel.items.iter().collect();
    debug_assert!(
        keys_grouped(&items),
        "a menu's items keep each shared key together, and no item is a prefix of another"
    );
    let Some(placement) = &panel.placement else {
        return centred(panel, &items);
    };
    // In a scroll, the rows keep the size and place they have without one
    // (Brylie's decision, 2026-11-27): the scroll's ring room takes the
    // place of the menu's padding, and the menu grows by the scrollbar's
    // gutter, which stays when nothing scrolls.
    let (rows, padding, width) = if panel.scrolls {
        let scroll = style::scroll();
        let gutter = style::scroll_gutter();
        (
            vec![list(Element::column(rows(panel, &items, 0)).width(Length::FILL), false)],
            (MENU_PADDING - scroll.ring).max(0.0),
            panel.width + gutter,
        )
    } else {
        (rows(panel, &items, 0), MENU_PADDING, panel.width)
    };
    Element::column(rows)
        .padding(padding)
        .width(Length::Fixed(width))
        .background(panel.fill)
        .border(style::MENU_BORDER, crate::style::QUIET)
        .radius(style::MENU_RADIUS)
        .shadow(style::MENU_SHADOW)
        .placed(placement.clone())
        .closes_on_press_outside()
}

/// The cable picker: its heading, then its rows in a scroll keyed `list`,
/// in a box centred in the window (ui-layer.md, section 5, rule 13),
/// closing on a press outside it with either button (rule 14). The heading
/// gives no node: the menu is named by the same text, and a menu holds only
/// its items (the WAI-ARIA menu pattern).
fn centred(panel: &Panel, items: &[&Item]) -> Element {
    let heading = panel.heading.clone().unwrap_or_default();
    let list = list(
        Element::column(rows(panel, items, 0)).spacing(2.0).width(Length::FILL),
        true,
    )
    .height(Length::FILL);
    Element::column([Element::text(heading, style::HEADING_TEXT).colour(panel.text), list])
        .spacing(6.0)
        .padding(8.0)
        .width(Length::Fixed(panel.width))
        .max_height(CABLE_MENU_HEIGHT)
        .min_height(0.0)
        .background(panel.fill)
        .border(style::MENU_BORDER, crate::style::QUIET)
        .radius(style::MENU_RADIUS)
        .shadow(style::MENU_SHADOW)
        .opaque()
        .placed(Placement::Centred)
        .closes_on_press_outside()
}

/// A menu's rows in a scroll keyed `list`, which shrinks when the menu is
/// cut to the window or to its own most height, and scrolls. With `pages`,
/// Page Up and Page Down move the focus a page, as in the cable picker
/// (section 6, rule 8); without, they scroll the list, as any scroll
/// container's do.
fn list(rows: Element, pages: bool) -> Element {
    let list = Element::scroll(rows, style::scroll()).key("list");
    // Its own arrows would scroll it; in a menu they move the focus among
    // the items, so the list asks the app first (section 6, rule 6, step
    // 2b; the WAI-ARIA menu pattern).
    let list = if pages {
        list.takes_first([Key::Up, Key::Down, Key::Home, Key::End, Key::PageUp, Key::PageDown])
    } else {
        list.takes_first([Key::Up, Key::Down, Key::Home, Key::End])
    };
    list.min_height(0.0).width(Length::FILL)
}

/// True when `rows` can key every row with its whole UI ID and no two
/// columns share one: the items with a shared leading run of keys sit next
/// to each other, and no item's keys begin another's. The menus' own order
/// gives this (the make-slot items, then each slot's swaps, by kind), and
/// a debug build checks it.
fn keys_grouped(items: &[&Item]) -> bool {
    for (at, item) in items.iter().enumerate() {
        for other in &items[at + 1..] {
            let shorter = item.path.len().min(other.path.len());
            if item.path[..shorter] == other.path[..shorter] {
                return false;
            }
        }
        for depth in 1..item.path.len() {
            let prefix = &item.path[..depth];
            let has = |other: &&Item| other.path.len() > depth && other.path[..depth] == *prefix;
            let first = items.iter().position(has).unwrap_or(at);
            let last = items.iter().rposition(has).unwrap_or(at);
            if !items[first..=last].iter().all(has) {
                return false;
            }
        }
    }
    true
}

/// The rows of `items`, in order, keyed by their keys from `depth` on:
/// items next to each other whose IDs share the key at `depth` go in a
/// column keyed by it, so a row's UI ID is its item's whole ID.
fn rows(panel: &Panel, items: &[&Item], depth: usize) -> Vec<Element> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < items.len() {
        let item = items[at];
        if item.path.len() == depth + 1 {
            out.push(row(panel, item, item.path[depth].clone()));
            at += 1;
            continue;
        }
        let key = &item.path[depth];
        let group = items[at..]
            .iter()
            .take_while(|other| other.path.len() > depth + 1 && other.path[depth] == *key)
            .count();
        out.push(
            Element::column(rows(panel, &items[at..at + group], depth + 1))
                .width(Length::FILL)
                .key(key.clone()),
        );
        at += group;
    }
    out
}

/// One item: its label, and a quiet second line where it has one.
fn row(panel: &Panel, item: &Item, key: IdKey) -> Element {
    let label = Element::text(item.label.clone(), style::CONTROL_TEXT);
    // A turned-off item takes its text colour from its off look, the plain
    // role's, as a turned-off control in Preferences does.
    let label = match (item.off, item.dim) {
        (true, _) => label,
        (false, true) => label.colour(crate::style::QUIET),
        (false, false) => label.colour(panel.text),
    };
    let mut lines = vec![label];
    let mut access = Access::new(Role::MenuItem, item.label.clone())
        .action(Action::Click)
        .position_in_set(item.position);
    if item.off {
        access = access.states(loom_ui::access::States {
            disabled: true,
            ..loom_ui::access::States::default()
        });
    }
    if let Some(second) = &item.second {
        lines.push(Element::text(second.clone(), style::BODY_TEXT).colour(crate::style::QUIET));
        access = access.description(second.clone());
    }
    let hovered = loom_ui::StateLook {
        background: Some(panel.hover),
        text: None,
    };
    let normal = loom_ui::StateLook {
        background: item.highlighted.then_some(panel.hover),
        text: None,
    };
    let row = Element::column(lines)
        .padding(style::BUTTON_PADDING)
        .width(Length::FILL)
        .border(style::MENU_BORDER, crate::style::QUIET)
        .radius(style::MENU_ITEM_RADIUS)
        .looks(loom_ui::Looks {
            normal,
            hovered,
            pressed: hovered,
            off: style::Role::Plain.looks().off,
        })
        .key(key);
    // A turned-off item stays a stop, as the APG keeps a disabled menu
    // item focusable (section 6, rule 2).
    let row = if item.off {
        row.focusable_while_off()
    } else {
        row.focusable()
    };
    row.access(access)
}

#[cfg(test)]
mod tests;
