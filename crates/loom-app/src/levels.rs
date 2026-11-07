//! Moving between levels: the top patch and the inside of a sub-patch
//! (`docs/design/sub-patches.md`, Make, open, and dissolve; Editor model).

use crate::{App, Message, browser};
use iced::{Point, Rectangle};
use loom_canvas::{GRID, PortRef, TILE_WIDTH};
use loom_core::{InputRef, ModuleId, OutputRef, TRANSPORT_MODULE};
use loom_patch::{Direction, End, Hierarchy, INPUT_KIND, ModulePath, OUTPUT_KIND, Tile};
use std::collections::BTreeSet;

/// The status bar text when the Transport is added inside a sub-patch
/// (sub-patches.md, rule 8).
pub(crate) const NO_TRANSPORT_INSIDE: &str = "A sub-patch cannot hold the Transport: it lives at the top level";

/// The status bar text when a modulation would cross a sub-patch boundary
/// (sub-patches.md, rule 4).
pub(crate) const NO_MODULATION_ACROSS: &str =
    "Modulation cannot cross a sub-patch boundary; expose the parameter instead";

/// The status bar text when a selection with a sub-patch tile is copied,
/// cut, or duplicated, until the clipboard carries definitions.
pub(crate) const NO_COPY_YET: &str = "Cannot copy a sub-patch yet";

impl App {
    /// Shows `level` on the canvas. The selection belongs to the level it
    /// was made on, so it clears. The canvas takes the level's camera, or
    /// frames the level on a first visit.
    pub(crate) fn show_level(&mut self, level: &ModulePath) {
        if self.session.level == *level {
            return;
        }
        self.session.level = level.clone();
        self.session.level_key += 1;
        self.session.selection.clear();
        self.refresh();
    }
}

impl App {
    /// Opens the sub-patch tile `id`: the canvas shows its inside. A plain
    /// module or a port tile opens nothing.
    pub(crate) fn open_tile(&mut self, id: u32) {
        let id = loom_core::ModuleId(id);
        let session = &self.session;
        if !matches!(
            session.hierarchy.tile(session.controller.graph(), id),
            Some(loom_patch::Tile::Instance(_))
        ) {
            return;
        }
        let inside = session.hierarchy.path_of(id);
        self.show_level(&inside);
    }
}

impl App {
    /// Shows the level above the one shown. At the top, nothing.
    pub(crate) fn go_up(&mut self) {
        if let Some(parent) = self.session.level.parent() {
            self.show_level(&parent);
        }
    }

    /// Opens the selected sub-patch, when exactly one tile is selected and
    /// it is one (Ctrl+Enter).
    pub(crate) fn open_selected(&mut self) {
        let mut selected = self.session.selection.iter();
        if let (Some(&id), None) = (selected.next(), selected.next()) {
            self.open_tile(id);
        }
    }
}

impl App {
    /// The end a port of a tile stands for in the shown level's ids: a
    /// module's port by name, an instance's by its place in the port list,
    /// a port tile's one port.
    pub(crate) fn level_end(&self, port: PortRef) -> Option<End> {
        let session = &self.session;
        let graph = session.controller.graph();
        let id = ModuleId(port.module);
        let lid = session.hierarchy.path_of(id).last();
        if let Some(node) = graph.node(id) {
            use loom_core::ModuleNames;
            let name = if port.output {
                node.output_name(port.index)?
            } else {
                node.input_name(port.index)?
            };
            return Some((lid, name.to_string()));
        }
        match session.hierarchy.tile(graph, id)? {
            Tile::Instance(_) => {
                let (inputs, outputs) = session.hierarchy.instance_ports(id);
                let list = if port.output { outputs } else { inputs };
                Some((lid, list.get(port.index)?.1.clone()))
            }
            Tile::Port(direction, _) => Some((lid, direction.own_port().to_string())),
            Tile::Module => None,
        }
    }

    /// Runs `change` on a copy of the hierarchy, makes the controller's
    /// cables follow it, and keeps it only when both worked. Returns
    /// whether it did; a refusal is in the status bar.
    pub(crate) fn change_hierarchy(
        &mut self,
        verb: &str,
        change: impl FnOnce(&mut Hierarchy) -> Result<(), String>,
    ) -> bool {
        let mut candidate = self.session.hierarchy.clone();
        if let Err(why) = change(&mut candidate) {
            self.alerts.warn(format!("Cannot {verb}: {why}"));
            return false;
        }
        if let Err(e) = candidate.sync(&mut self.session.controller) {
            self.alerts.warn(format!("Cannot {verb}: {e}"));
            return false;
        }
        self.session.hierarchy = candidate;
        true
    }

    /// Connects an output to an input where at least one end is a port or
    /// a sub-patch tile: the cable becomes a boundary record, and the
    /// graph follows. The cable into the input goes, whichever side it
    /// was on (ADR-0039, invariant 2).
    pub(crate) fn connect_through_hierarchy(&mut self, from: PortRef, to: PortRef) {
        let (Some(from_end), Some(to_end)) = (self.level_end(from), self.level_end(to)) else {
            self.alerts.warn("Cannot connect: the port is gone".to_string());
            return;
        };
        let level = self.session.level.clone();
        // A direct cable into a module's input must go first: inside a
        // sub-patch, two cables into one input have no file form.
        let input = InputRef {
            module: ModuleId(to.module),
            port: to.index,
        };
        if self.session.controller.graph().node(input.module).is_some()
            && self.session.controller.graph().source_of(input).is_some()
        {
            let _ = self.session.controller.disconnect(input);
        }
        if self.change_hierarchy("connect", |h| {
            h.connect(&level, from_end, to_end);
            Ok(())
        }) {
            self.commit();
        }
    }

    /// Removes the cable into an input that a boundary record holds: on a
    /// port or sub-patch tile, or into a module from a port.
    pub(crate) fn disconnect_through_hierarchy(&mut self, to: PortRef) {
        let Some(to_end) = self.level_end(to) else { return };
        let level = self.session.level.clone();
        if self.change_hierarchy("disconnect", |h| {
            h.disconnect(&level, &to_end);
            Ok(())
        }) {
            self.commit();
        }
    }

    /// True when the cable into `input` is there only because a boundary
    /// record implies it, so the canvas shows it from a port tile.
    pub(crate) fn fed_through_port(&self, input: InputRef) -> bool {
        let session = &self.session;
        session
            .controller
            .graph()
            .source_of(input)
            .is_some_and(|from| session.hierarchy.is_implied(session.controller.graph(), from, input))
    }

    /// Takes tiles out of the patch: modules, with their cables; ports,
    /// with their cables; sub-patches, with everything inside. Returns
    /// whether the commit was sent.
    pub(crate) fn delete_tiles(&mut self, ids: &[u32]) -> bool {
        let mut gone: Vec<ModuleId> = Vec::new();
        let mut candidate = self.session.hierarchy.clone();
        for &id in ids {
            let removed = candidate.remove(self.session.controller.graph(), ModuleId(id));
            for module in &removed.modules {
                let _ = self.session.controller.remove_module(*module);
            }
            gone.extend(removed.tiles);
        }
        for id in &gone {
            self.session.positions.remove(&id.0);
            self.session.sizes.remove(&id.0);
            self.session.selection.remove(&id.0);
            self.session.cameras.remove(&self.session.hierarchy.path_of(*id));
        }
        match candidate.sync(&mut self.session.controller) {
            Ok(()) => self.session.hierarchy = candidate,
            Err(e) => self.alerts.warn(format!("Cannot delete: {e}")),
        }
        self.commit()
    }

    /// The bounding box of tiles on the canvas, in world units.
    fn tiles_bounds(&self, ids: &[ModuleId]) -> Option<Rectangle> {
        ids.iter()
            .filter_map(|id| self.session.scene.module(id.0).map(|tile| tile.bounds()))
            .reduce(|a, b| a.union(&b))
    }

    /// Ctrl+G, Make sub-patch: the selection becomes one sub-patch tile
    /// where it was, as one undo step (sub-patches.md, Collapsing). The
    /// members keep their places inside; the new ports stand left and
    /// right of them.
    pub(crate) fn make_subpatch(&mut self) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        let selection: Vec<u32> = self.session.selection.iter().copied().collect();
        let level = self.session.level.clone();
        let mut candidate = self.session.hierarchy.clone();
        let made = match candidate.collapse(self.session.controller.graph(), &level, &selection) {
            Ok(made) => made,
            Err(why) => {
                self.alerts.warn(why);
                return;
            }
        };
        let name = candidate.tile_name(made.instance).unwrap_or_default();
        self.session.step_name = Some("make sub-patch".to_string());
        self.edit(|app| {
            let bounds = app
                .tiles_bounds(&made.members)
                .unwrap_or(Rectangle::new(Point::ORIGIN, iced::Size::ZERO));
            let origin = browser::snap(bounds.position());
            app.session.positions.insert(made.instance.0, (origin.x, origin.y));
            let column = |ids: &[ModuleId], x: f32, positions: &mut std::collections::BTreeMap<u32, (f32, f32)>| {
                for (i, id) in ids.iter().enumerate() {
                    let at = browser::snap(Point::new(x, bounds.y + i as f32 * 6.0 * GRID));
                    positions.insert(id.0, (at.x, at.y));
                }
            };
            column(
                &made.inputs,
                bounds.x - TILE_WIDTH - 4.0 * GRID,
                &mut app.session.positions,
            );
            column(
                &made.outputs,
                bounds.x + bounds.width + 4.0 * GRID,
                &mut app.session.positions,
            );
            // A nested sub-patch among the members keeps its camera under
            // its new path.
            for id in &made.members {
                let new = candidate.path_of(*id);
                let old = level.child(new.last());
                app.rekey_cameras(&old, &new);
            }
            match candidate.sync(&mut app.session.controller) {
                Ok(()) => {
                    app.session.hierarchy = candidate;
                    app.session.selection = BTreeSet::from([made.instance.0]);
                    if app.commit() {
                        app.alerts.say(format!(
                            "Made {name} from {} {}",
                            made.members.len(),
                            if made.members.len() == 1 { "module" } else { "modules" }
                        ));
                    }
                }
                Err(e) => app.alerts.warn(format!("Cannot make a sub-patch: {e}")),
            }
        });
    }

    /// Ctrl+Shift+G, Dissolve sub-patch: the selected sub-patch's inside
    /// comes back into the level where its tile was, as one undo step.
    pub(crate) fn dissolve_subpatch(&mut self) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        let mut selected = self.session.selection.iter().copied();
        let instance = match (selected.next(), selected.next()) {
            (Some(id), None)
                if matches!(
                    self.session
                        .hierarchy
                        .tile(self.session.controller.graph(), ModuleId(id)),
                    Some(Tile::Instance(_))
                ) =>
            {
                ModuleId(id)
            }
            _ => {
                self.alerts.warn("Select one sub-patch to dissolve".to_string());
                return;
            }
        };
        let level = self.session.level.clone();
        let inside = self.session.hierarchy.path_of(instance);
        let mut candidate = self.session.hierarchy.clone();
        let undone = match candidate.dissolve(self.session.controller.graph(), instance) {
            Ok(undone) => undone,
            Err(why) => {
                self.alerts.warn(why);
                return;
            }
        };
        self.session.step_name = Some(format!("dissolve {}", undone.name));
        self.edit(|app| {
            // The members land where the tile was, keeping their layout.
            let at = app.session.positions.get(&instance.0).copied().unwrap_or((0.0, 0.0));
            let top_left = undone
                .members
                .iter()
                .filter_map(|id| app.session.positions.get(&id.0).copied())
                .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))
                .unwrap_or(at);
            let shift = (at.0 - top_left.0, at.1 - top_left.1);
            for id in &undone.members {
                if let Some(p) = app.session.positions.get_mut(&id.0) {
                    p.0 += shift.0;
                    p.1 += shift.1;
                }
                let new = candidate.path_of(*id);
                let old = inside.child(new.last());
                app.rekey_cameras(&old, &new);
            }
            for id in &undone.removed {
                app.session.positions.remove(&id.0);
                app.session.sizes.remove(&id.0);
            }
            app.session.cameras.remove(&inside);
            match candidate.sync(&mut app.session.controller) {
                Ok(()) => {
                    app.session.hierarchy = candidate;
                    app.session.selection = undone.members.iter().map(|id| id.0).collect();
                    if app.commit() {
                        app.alerts.say(format!("Dissolved {}", undone.name));
                    }
                }
                Err(e) => app.alerts.warn(format!("Cannot dissolve: {e}")),
            }
            let _ = level;
        });
    }

    /// Moves the cameras of the level at `old` and of every level under
    /// it to the same place under `new`.
    fn rekey_cameras(&mut self, old: &ModulePath, new: &ModulePath) {
        if old == new {
            return;
        }
        let moved: Vec<(ModulePath, loom_canvas::Camera)> = self
            .session
            .cameras
            .iter()
            .filter(|(path, _)| path.starts_with(old))
            .map(|(path, camera)| (path.clone(), *camera))
            .collect();
        for (path, camera) in moved {
            self.session.cameras.remove(&path);
            self.session.cameras.insert(path.replace_prefix(old, new), camera);
        }
    }

    /// True when a module of `kind` cannot be added at the shown level, with
    /// the reason: the Transport inside a sub-patch (rule 8).
    pub(crate) fn refuses_kind(&self, kind: &str) -> Option<&'static str> {
        (!self.session.level.is_root() && kind == TRANSPORT_MODULE).then_some(NO_TRANSPORT_INSIDE)
    }

    /// Inside a sub-patch, a Patch input or output from the browser is a
    /// port of the sub-patch, not a module. Adds it at `world` and
    /// selects it, as one undo step. Returns false for any other kind, or
    /// at the top level, where it is an ordinary module.
    pub(crate) fn add_port_tile(&mut self, kind: &str, world: Point) -> bool {
        let direction = match kind {
            INPUT_KIND => Direction::Input,
            OUTPUT_KIND => Direction::Output,
            _ => return false,
        };
        if self.session.level.is_root() {
            return false;
        }
        let level = self.session.level.clone();
        self.session.step_name = Some(format!(
            "add {}",
            if direction == Direction::Input {
                "Patch input"
            } else {
                "Patch output"
            }
        ));
        self.edit(|app| {
            let mut added = None;
            if app.change_hierarchy("add a port", |h| {
                added = Some(
                    h.add_port(&level, direction, direction.own_port_fallback())
                        .map_err(|e| e.to_string())?,
                );
                Ok(())
            }) && let Some(id) = added
            {
                app.session.positions.insert(id.0, (world.x, world.y));
                app.session.selection = BTreeSet::from([id.0]);
                app.commit();
            }
        });
        true
    }

    /// True when an assignment from `source` onto `target` would cross a
    /// sub-patch boundary, or start at a port tile: refused with the
    /// reason in the status bar (rule 4).
    pub(crate) fn refuses_assignment(&mut self, source: OutputRef, target: ModuleId) -> bool {
        let session = &self.session;
        let graph = session.controller.graph();
        let crosses = graph.node(source.module).is_none()
            || graph.node(target).is_none()
            || session.hierarchy.level_of(source.module) != session.hierarchy.level_of(target);
        if crosses {
            self.alerts.warn(NO_MODULATION_ACROSS.to_string());
        }
        crosses
    }

    /// The tiles of the selection that are sub-patches or ports, which the
    /// clipboard cannot carry yet.
    pub(crate) fn structural_in_selection(&self) -> bool {
        let session = &self.session;
        session.selection.iter().any(|&id| {
            session.controller.graph().node(ModuleId(id)).is_none()
                && session
                    .hierarchy
                    .tile(session.controller.graph(), ModuleId(id))
                    .is_some()
        })
    }

    /// What the tile menu offers for a tile: Help for a module, Open and
    /// Dissolve for a sub-patch, and Make sub-patch for any selection of
    /// modules.
    pub(crate) fn tile_menu_items(&self, module: u32) -> Vec<(&'static str, Message)> {
        let session = &self.session;
        let tile = session.hierarchy.tile(session.controller.graph(), ModuleId(module));
        match tile {
            Some(Tile::Instance(_)) => vec![
                ("Open", Message::OpenTile(module)),
                ("Dissolve sub-patch", Message::DissolveSubPatch(module)),
            ],
            Some(Tile::Port(..)) => vec![("Help", Message::ShowModuleHelp(module))],
            _ => vec![
                ("Help", Message::ShowModuleHelp(module)),
                ("Make sub-patch", Message::MakeSubPatch(module)),
            ],
        }
    }
}

#[cfg(test)]
mod tests;
