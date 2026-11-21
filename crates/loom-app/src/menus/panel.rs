//! The probe menu and Recent as panels of `loom-ui`'s Overlays layer
//! (ui-layer.md, section 6, rule 8; plan 3.2, row 3b): placed where they
//! were asked for, closing on a press outside, with the menu shadow. Their
//! items are menu items and focus stops, and the menu's highlight is the
//! runtime's focus on an item. The tile menu stays Iced's for now.

use super::{MenuKind, MenuModel};
use crate::style;
use crate::{App, Message};
use loom_canvas::iced_convert::ToCore as _;
use loom_ui::access::{Access, Action, Role};
use loom_ui::{Element, Layer, Length, Placement, View};
use loom_ui_core::{Color, IdKey, UiId};

/// One item as the menu's panel shows it.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    /// Its key in the menu: the last key of its UI ID.
    key: IdKey,
    /// What it says.
    label: String,
    /// A second, quiet line under it: a Recent file's folder.
    second: Option<String>,
    /// Shown dimmed: a Recent file that was not there.
    dim: bool,
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
            MenuKind::Tile(_) => return,
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
                    key: item.id.keys().last().cloned().expect("an item's UI ID has keys"),
                    label: item.label.clone(),
                    second: item.description.clone(),
                    dim: missing.get(at).copied().unwrap_or(false),
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

    /// The open menu the runtime draws: the probe menu or Recent, not the
    /// tile menu, which Iced still draws.
    pub(crate) fn runtime_menu(&self) -> Option<MenuModel> {
        self.menu_model()
            .filter(|menu| matches!(menu.kind, MenuKind::Probe | MenuKind::Recent))
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
    /// (section 5, rule 13).
    pub(crate) fn menu_pressed_outside(&self, root: &UiId) -> Option<Message> {
        if *root == super::probe_id() {
            Some(Message::ProbeMenu(crate::probe_menu::Message::Close))
        } else if *root == super::recent_id() {
            Some(Message::Recent(crate::recent::Message::Close))
        } else {
            None
        }
    }

    /// True while one of the runtime's overlays owns the input: the
    /// question, the probe menu, or Recent.
    pub(crate) fn runtime_overlay_open(&self) -> bool {
        self.overlays.question.is_some() || self.runtime_menu().is_some()
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
    Element::column(panel.items.iter().map(|item| row(panel, item)))
        .padding(4.0)
        .width(Length::Fixed(panel.width))
        .background(panel.fill)
        .border(style::MENU_BORDER, style::QUIET.to_core())
        .radius(style::MENU_RADIUS)
        .shadow(style::MENU_SHADOW)
        .placed(panel.placement.clone())
        .closes_on_press_outside()
}

/// One item: its label, and a quiet second line where it has one.
fn row(panel: &Panel, item: &Item) -> Element {
    let label_colour = if item.dim { style::QUIET.to_core() } else { panel.text };
    let mut lines = vec![Element::text(item.label.clone(), style::CONTROL_TEXT).colour(label_colour)];
    let mut access = Access::new(Role::MenuItem, item.label.clone()).action(Action::Click);
    if let Some(second) = &item.second {
        lines.push(Element::text(second.clone(), style::BODY_TEXT).colour(style::QUIET.to_core()));
        access = access.description(second.clone());
    }
    let hovered = loom_ui::StateLook {
        background: Some(panel.hover),
        text: None,
    };
    Element::column(lines)
        .padding(style::BUTTON_PADDING)
        .width(Length::FILL)
        .radius(style::MENU_ITEM_RADIUS)
        .looks(loom_ui::Looks {
            normal: loom_ui::StateLook::default(),
            hovered,
            pressed: hovered,
            off: loom_ui::StateLook::default(),
        })
        .key(item.key.clone())
        .focusable()
        .access(access)
}
