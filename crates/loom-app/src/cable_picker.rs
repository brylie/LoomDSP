//! The canvas's keyboard cable picker (#524, canvas-accessibility.md).

use crate::menus::{MenuItem, MenuKind, MenuModel};
use crate::overlays::{ContextMenu, Overlay};
use crate::{App, Message};
use loom_canvas::{CanvasEvent, PortRef};
use loom_ui_core::UiId;

/// The UI ID of the cable picker, a menu of the runtime's.
pub(crate) fn cable_id() -> UiId {
    UiId::panel("menu").with("cable")
}

/// The UI ID of the picker's scrolling list. Every row is keyed under it,
/// since a scroll container keeps its offset by its UI ID (ui-layer.md,
/// section 5, rule 12).
pub(crate) fn list_id() -> UiId {
    cable_id().with("list")
}

impl App {
    /// Opens the port list for the one selected tile.
    pub(crate) fn open_cable_picker(&mut self) -> bool {
        let mut selected = self.session.selection.iter();
        let (Some(&module), None) = (selected.next(), selected.next()) else {
            return false;
        };
        let Some(tile) = self.session.scene.module(module) else {
            return false;
        };
        if let Some(why) = self.session.hierarchy.level_refusal(&self.session.level) {
            self.alerts.warn(why);
        } else if tile.unknown || (tile.inputs.is_empty() && tile.outputs.is_empty()) {
            self.alerts.say("This tile has no ports to patch");
        } else {
            self.show_cable_picker(module, None);
        }
        true
    }

    /// Opens a fresh stage of the picker with the focus on its first row.
    /// A new stage keeps the list's UI ID, so its old offset would stay;
    /// focusing the first row scrolls it back to the top.
    fn show_cable_picker(&mut self, module: u32, start: Option<PortRef>) {
        self.overlays.open(Overlay::Menu(ContextMenu::Cable { module, start }));
        self.overlays.set_menu_highlight(Some(0));
        if let Some(item) = self.cable_menu_model(module, start, Some(0)).items.first() {
            self.effect(loom_ui::Effects::focus(item.id.clone()));
        }
    }

    /// The same port rows for the pointer, keys and accessibility tree.
    pub(crate) fn cable_menu_model(&self, module: u32, start: Option<PortRef>, highlight: Option<usize>) -> MenuModel {
        let id = cable_id();
        let list = list_id();
        let scene = &self.session.scene;
        let structure = &self.session.patch_map.structure;
        let mut items = Vec::new();
        if let Some(port) = start.filter(|port| self.input_has_cable(*port)) {
            items.push(MenuItem {
                id: list
                    .clone()
                    .with("disconnect")
                    .with(port.module)
                    .with(port.index as u32),
                label: "Disconnect cable".to_string(),
                description: None,
                message: Some(Message::CablePort { start, port: None }),
            });
        }
        for tile in scene
            .modules()
            .iter()
            .filter(|tile| !tile.unknown && (start.is_some() || tile.id == module))
        {
            for (output, ports) in [(false, &tile.inputs), (true, &tile.outputs)] {
                if start.is_some_and(|port| port.output == output) {
                    continue;
                }
                let direction = if output { "output" } else { "input" };
                for index in 0..ports.len() {
                    let port = PortRef {
                        module: tile.id,
                        output,
                        index,
                    };
                    items.push(MenuItem {
                        id: list.clone().with(tile.id).with(direction).with(index as u32),
                        label: format!("{} ({direction})", structure.named_port(port)),
                        description: Some(direction.to_string()),
                        message: Some(Message::CablePort {
                            start,
                            port: Some(port),
                        }),
                    });
                }
            }
        }
        // With no port to offer, one item says so: turned off, it keeps
        // the focus, so Escape and Tab still reach the menu and a screen
        // reader hears why there is nothing to pick (the WAI-ARIA menu
        // pattern, which needs an item in a menu).
        if items.is_empty() {
            items.push(MenuItem {
                id: list.clone().with("none"),
                label: "No ports to connect".to_string(),
                description: None,
                message: None,
            });
        }
        let name = start
            .filter(|port| scene.port(*port).is_some())
            .map(|port| structure.port_label(port));
        let label = name.map_or_else(
            || "Choose a port".to_string(),
            |name| format!("Choose a port for {name}"),
        );
        MenuModel {
            kind: MenuKind::Cable,
            id,
            label,
            items,
            highlight,
        }
    }

    /// True for an input that already has a visible cable.
    fn input_has_cable(&self, port: PortRef) -> bool {
        !port.output
            && self
                .session
                .scene
                .cables()
                .iter()
                .any(|wire| wire.to == (port.module, port.index))
    }

    /// Picks a port, or sends a completed gesture through the ordinary cable editor.
    pub(crate) fn pick_cable_port(&mut self, start: Option<PortRef>, picked: Option<PortRef>) {
        let Some((module, current)) = self.overlays.cable_menu() else {
            return;
        };
        let exists = |port: PortRef| self.session.scene.port(port).is_some();
        if start != current || start.is_some_and(|port| !exists(port)) || picked.is_some_and(|port| !exists(port)) {
            return;
        }
        let event = match (start, picked) {
            (None, Some(port)) if port.module == module => {
                self.show_cable_picker(module, Some(port));
                return;
            }
            (Some(a), Some(b)) if a.output != b.output => {
                let (from, to) = if a.output { (a, b) } else { (b, a) };
                CanvasEvent::Connect { from, to }
            }
            (Some(port), None) if self.input_has_cable(port) => CanvasEvent::Disconnect(port),
            _ => return,
        };
        self.overlays.close_tile_menu();
        self.canvas_event(event);
    }
}

#[cfg(test)]
mod tests;
