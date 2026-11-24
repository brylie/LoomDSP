//! The probe menu and Recent as panels of `loom-ui`'s Overlays layer
//! (ui-layer.md, section 6, rule 8; plan 3.2, row 3b): placed where they
//! were asked for, closing on a press outside, with the menu shadow. Their
//! items are menu items and focus stops, and the menu's highlight is the
//! runtime's focus on an item. The tile menu is one too (row 3d): placed
//! at the tile's top-left corner from the keyboard, or where the pointer
//! pressed.

use super::{MenuKind, MenuModel};
use crate::style;
use crate::{App, Message};
use loom_canvas::iced_convert::ToCore as _;
use loom_ui::access::{Access, Action, Role};
use loom_ui::{Element, Layer, Length, Placement, View};
use loom_ui_core::{Color, IdKey, UiId};

/// The width of the tile menu, in logical pixels: its items' 200 and the
/// menu's padding, as Iced drew it.
const TILE_MENU_WIDTH: f32 = 208.0;

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
    /// Where it goes.
    placement: Placement,
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
                Some(probe) => (Placement::At(probe.at()), crate::probe_menu::WIDTH),
                None => return,
            },
            MenuKind::Recent => (
                Placement::Under(crate::toolbar::toolbar_id().with("recent")),
                crate::recent::LIST_WIDTH,
            ),
            MenuKind::Tile(_) => match self.overlays.tile_menu() {
                Some((_, at)) => (Placement::At(at), TILE_MENU_WIDTH),
                None => return,
            },
            MenuKind::Cable => return,
        };
        let missing: Vec<bool> = match menu.kind {
            MenuKind::Recent => self
                .overlays
                .recent_rows()
                .map(|rows| rows.iter().map(|row| row.missing).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let theme = self.theme();
        let palette = theme.extended_palette();
        let panel = Panel {
            items: menu
                .items
                .iter()
                .enumerate()
                .map(|(at, item)| Item {
                    path: item.id.keys()[menu.id.keys().len()..].to_vec(),
                    position: at,
                    label: item.label.clone(),
                    second: item.description.clone(),
                    dim: missing.get(at).copied().unwrap_or(false),
                    highlighted: menu.highlight == Some(at),
                })
                .collect(),
            placement,
            width,
            fill: palette.background.base.color.to_core(),
            hover: palette.background.weak.color.to_core(),
            text: palette.background.base.text.to_core(),
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
        // The cable menu shares the tile menu's owner, but is still Iced's
        // (#856), so it is no runtime menu.
        self.menu_model().filter(|menu| owns && menu.kind != MenuKind::Cable)
    }

    /// The message for a click on one of the runtime menu's items, from the
    /// pointer, Enter or Space, or a screen reader: what picking it sends.
    pub(crate) fn menu_click(&self, id: &UiId) -> Option<Message> {
        self.runtime_menu()?
            .items
            .into_iter()
            .find(|item| item.id == *id)
            .map(|item| item.message)
    }

    /// The message for a press outside the runtime menu `root`: close it
    /// (section 5, rule 14).
    pub(crate) fn menu_pressed_outside(&self, root: &UiId) -> Option<Message> {
        if *root == super::probe_id() {
            Some(Message::ProbeMenu(crate::probe_menu::Message::Close))
        } else if *root == super::recent_id() {
            Some(Message::Recent(crate::recent::Message::Close))
        } else if self
            .overlays
            .tile_menu()
            .is_some_and(|(module, _)| *root == super::tile_id(module))
        {
            Some(Message::CloseTileMenu)
        } else {
            None
        }
    }

    /// True while one of the runtime's overlays owns the input: the
    /// question, help, the probe menu, or Recent.
    pub(crate) fn runtime_overlay_open(&self) -> bool {
        self.overlays.question.is_some()
            || self.overlays.help.is_some()
            || self.overlays.chooser.is_some()
            || self.runtime_menu().is_some()
    }

    /// The runtime menu's items as focus stops, in order.
    pub(crate) fn menu_stops(&self) -> Vec<UiId> {
        self.runtime_menu()
            .map(|menu| menu.items.into_iter().map(|item| item.id).collect())
            .unwrap_or_default()
    }
}

/// The menu: a column of items in a box with the menu shadow, placed, and
/// closing on a press outside.
fn build(panel: &Panel) -> Element {
    let items: Vec<&Item> = panel.items.iter().collect();
    debug_assert!(
        keys_grouped(&items),
        "a menu's items keep each shared key together, and no item is a prefix of another"
    );
    Element::column(rows(panel, &items, 0))
        .padding(4.0)
        .width(Length::Fixed(panel.width))
        .background(panel.fill)
        .border(style::MENU_BORDER, style::QUIET.to_core())
        .radius(style::MENU_RADIUS)
        .shadow(style::MENU_SHADOW)
        .placed(panel.placement.clone())
        .closes_on_press_outside()
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
    let label_colour = if item.dim { style::QUIET.to_core() } else { panel.text };
    let mut lines = vec![Element::text(item.label.clone(), style::CONTROL_TEXT).colour(label_colour)];
    let mut access = Access::new(Role::MenuItem, item.label.clone())
        .action(Action::Click)
        .position_in_set(item.position);
    if let Some(second) = &item.second {
        lines.push(Element::text(second.clone(), style::BODY_TEXT).colour(style::QUIET.to_core()));
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
    Element::column(lines)
        .padding(style::BUTTON_PADDING)
        .width(Length::FILL)
        .border(style::MENU_BORDER, style::QUIET.to_core())
        .radius(style::MENU_ITEM_RADIUS)
        .looks(loom_ui::Looks {
            normal,
            hovered,
            pressed: hovered,
            off: loom_ui::StateLook::default(),
        })
        .key(key)
        .focusable()
        .access(access)
}

#[cfg(test)]
mod tests;
