//! The small menus over the canvas, as plain data: the tile menu, the probe
//! menu, and the Recent list (ADR-0021). The view, the keys, and the
//! accessibility tree all read [`MenuModel`], so a highlight moved by a key
//! is the one a screen reader hears.
//!
//! The keys work as the question's do: Up and Down move the highlight,
//! wrapping at the ends, Home and End go to the first and last item, Enter
//! picks the highlighted item, and Escape closes the menu
//! (`docs/design/ui-layer.md`, section 6, rule 8). A menu opened by the
//! pointer has no highlight until a key moves it.

use crate::{App, Message, probe_menu, recent};
use iced::keyboard::{self, Key, key::Named};
use loom_ui_core::UiId;

/// Which menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKind {
    /// The menu on a tile, by its module.
    Tile(u32),
    /// The menu on an output or a cable.
    Probe,
    /// The Recent list.
    Recent,
}

/// One item of a menu.
#[derive(Debug, Clone)]
pub(crate) struct MenuItem {
    /// Its UI ID: the menu's, then what the item shows.
    pub id: UiId,
    /// What it says.
    pub label: String,
    /// More about it: a Recent file's folder, or that it is missing.
    pub description: Option<String>,
    /// What picking it sends.
    pub message: Message,
}

/// The open menu, as its view, its keys, and the tree see it.
#[derive(Debug, Clone)]
pub(crate) struct MenuModel {
    /// Which menu it is.
    pub kind: MenuKind,
    /// Its UI ID.
    pub id: UiId,
    /// Its name.
    pub label: String,
    /// Its items, in the order shown.
    pub items: Vec<MenuItem>,
    /// The item the keys are on, if a key has moved there.
    pub highlight: Option<usize>,
}

impl MenuModel {
    /// The UI ID of the highlighted item.
    pub(crate) fn highlighted(&self) -> Option<&MenuItem> {
        self.highlight.and_then(|at| self.items.get(at))
    }
}

/// The UI ID of the menu on tile `module`.
pub(crate) fn tile_id(module: u32) -> UiId {
    UiId::panel("menu").with("tile").with(module)
}

/// The UI ID of the probe menu.
pub(crate) fn probe_id() -> UiId {
    UiId::panel("menu").with("probe")
}

/// The UI ID of the Recent list.
pub(crate) fn recent_id() -> UiId {
    UiId::panel("recent")
}

/// The UI ID of a tile menu's item, under the menu's `menu`, from what the
/// item does, not from what it says: rewording an item, or a name in it,
/// keeps its node (section 4).
fn tile_item_id(menu: &UiId, message: &Message) -> UiId {
    use crate::slots::SwapTarget;
    let id = menu.clone();
    match message {
        Message::OpenTile(_) => id.with("open"),
        Message::DissolveSubPatch(_) => id.with("dissolve"),
        Message::SaveToLibrary(_) => id.with("save-to-library"),
        Message::Unlink(_) => id.with("unlink"),
        Message::ShowModuleHelp(_) => id.with("help"),
        Message::MakeSubPatch(_) => id.with("make-sub-patch"),
        Message::MakeSlot { role, .. } => id.with("make-slot").with(role.as_str()),
        Message::DissolveSlot(_) => id.with("dissolve-slot"),
        Message::SwapSlot { slot, to } => {
            let (kind, name) = match to {
                SwapTarget::Module(name) => ("module", name),
                SwapTarget::Library(name) => ("library", name),
                SwapTarget::Held(name) => ("held", name),
                SwapTarget::Empty => return id.with("remove-part").with(*slot),
            };
            id.with("swap").with(*slot).with(kind).with(name.as_str())
        }
        // An item this list does not name yet still gets a node of its own;
        // a test checks that every item a tile can show is named here.
        _ => id.with("unnamed"),
    }
}

/// Where a key moves the highlight in a menu of `len` items, from `at`. Up
/// and Down wrap; from no highlight, Down goes to the first item and Up to
/// the last. `None` for a key that moves nothing.
pub(crate) fn step(at: Option<usize>, len: usize, key: &Key) -> Option<usize> {
    let last = len.checked_sub(1)?;
    match key.as_ref() {
        Key::Named(Named::ArrowDown) => Some(at.map_or(0, |at| if at >= last { 0 } else { at + 1 })),
        Key::Named(Named::ArrowUp) => Some(at.map_or(last, |at| if at == 0 { last } else { at - 1 })),
        Key::Named(Named::Home) => Some(0),
        Key::Named(Named::End) => Some(last),
        _ => None,
    }
}

/// True for a key a menu uses: the arrows up and down, Home, End, Enter,
/// and Escape, pressed with no Command, Control, or Alt.
pub(crate) fn is_menu_key(event: &keyboard::Event) -> bool {
    let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return false;
    };
    !modifiers.command()
        && !modifiers.control()
        && !modifiers.alt()
        && matches!(
            key.as_ref(),
            Key::Named(Named::ArrowUp | Named::ArrowDown | Named::Home | Named::End | Named::Enter | Named::Escape)
        )
}

impl App {
    /// The open menu, if one is open.
    pub(crate) fn menu_model(&self) -> Option<MenuModel> {
        let highlight = self.overlays.menu_highlight();
        if let Some((module, _)) = self.overlays.tile_menu() {
            let id = tile_id(module);
            let title = self
                .ui_snapshot_title(module)
                .unwrap_or_else(|| format!("Module {module}"));
            let items = self
                .tile_menu_items(module)
                .into_iter()
                .map(|(label, message)| MenuItem {
                    id: tile_item_id(&id, &message),
                    label,
                    description: None,
                    message,
                })
                .collect();
            return Some(MenuModel {
                kind: MenuKind::Tile(module),
                id,
                label: format!("{title} menu"),
                items,
                highlight,
            });
        }
        if self.overlays.probe_menu().is_some() {
            let id = probe_id();
            let items = [
                ("scope", "Add scope", probe_menu::Probe::Scope),
                ("meter", "Add meter", probe_menu::Probe::Meter),
            ]
            .into_iter()
            .map(|(key, label, probe)| MenuItem {
                id: id.clone().with(key),
                label: label.to_string(),
                description: None,
                message: Message::ProbeMenu(probe_menu::Message::Pick(probe)),
            })
            .collect();
            return Some(MenuModel {
                kind: MenuKind::Probe,
                id,
                label: "Output menu".to_string(),
                items,
                highlight,
            });
        }
        let rows = self.overlays.recent_rows()?;
        let id = recent_id();
        let items = rows
            .iter()
            .enumerate()
            .map(|(at, row)| MenuItem {
                id: id.clone().with(row.path.display().to_string().as_str()),
                label: row.name.clone(),
                description: Some(if row.missing {
                    format!("{} (not found)", row.folder)
                } else {
                    row.folder.clone()
                }),
                message: Message::Recent(recent::Message::Pick(at)),
            })
            .collect();
        Some(MenuModel {
            kind: MenuKind::Recent,
            id,
            label: "Recent".to_string(),
            items,
            highlight,
        })
    }

    /// A key a menu uses ([`is_menu_key`]) while a menu is open. It stops
    /// here, so it reaches neither the canvas nor a window behind the menu.
    pub(crate) fn menu_key(&mut self, event: &keyboard::Event) {
        let keyboard::Event::KeyPressed { key, repeat, .. } = event else {
            return;
        };
        let Some(menu) = self.menu_model() else {
            return;
        };
        match key.as_ref() {
            Key::Named(Named::Escape) => {
                self.close_menu(menu.kind);
            }
            // The item's message is its own update, on the next turn, as a
            // click on the item would send it (ui-layer.md, section 2, rule 5).
            Key::Named(Named::Enter) if !repeat => {
                if let Some(item) = menu.highlighted() {
                    self.post(item.message.clone());
                }
            }
            _ => {
                if let Some(at) = step(menu.highlight, menu.items.len(), key) {
                    self.overlays.set_menu_highlight(Some(at));
                }
            }
        }
    }

    /// Closes the open menu of kind `kind`.
    fn close_menu(&mut self, kind: MenuKind) {
        match kind {
            MenuKind::Tile(_) => self.overlays.close_tile_menu(),
            MenuKind::Probe => self.overlays.close_probe_menu(),
            MenuKind::Recent => self.overlays.close_recent_list(),
        }
    }

    /// The title tile `module` shows, if it is on the canvas.
    fn ui_snapshot_title(&self, module: u32) -> Option<String> {
        self.session.scene.module(module).map(|tile| tile.title.clone())
    }
}

#[cfg(test)]
mod tests;
