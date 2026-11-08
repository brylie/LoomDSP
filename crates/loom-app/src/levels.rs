//! Moving between levels: the top patch and the inside of a sub-patch
//! (`docs/design/sub-patches.md`, Make, open, and dissolve; Editor model).

use crate::{App, Message, browser};
use iced::{Point, Rectangle};
use loom_canvas::{GRID, PortRef, TILE_WIDTH};
use loom_core::{ModuleId, OutputRef, TRANSPORT_MODULE};
use loom_patch::{Direction, End, Hierarchy, INPUT_KIND, ModulePath, OUTPUT_KIND, Tile};
use std::collections::BTreeSet;

/// The status bar text when the Transport is added inside a sub-patch
/// (sub-patches.md, rule 8).
pub(crate) const NO_TRANSPORT_INSIDE: &str = "A sub-patch cannot hold the Transport: it lives at the top level";

/// The status bar text when a modulation would cross a sub-patch boundary
/// (sub-patches.md, rule 4).
pub(crate) const NO_MODULATION_ACROSS: &str =
    "Modulation cannot cross a sub-patch boundary; expose the parameter instead";

/// The status bar text when a linked sub-patch becomes a copy, after an
/// edit inside it or Unlink (sub-patches.md, Copies and editing).
pub(crate) fn now_a_copy(name: &str) -> String {
    format!("{name} is now a copy. It no longer follows the library.")
}

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

    /// Runs `change` on a copy of the hierarchy, with the graph to read,
    /// makes the controller's cables and assignment order follow it, and
    /// keeps it only when both worked. Returns whether it did; a refusal is
    /// in the status bar.
    pub(crate) fn change_hierarchy(
        &mut self,
        verb: &str,
        change: impl FnOnce(&mut Hierarchy, &loom_core::Graph) -> Result<(), String>,
    ) -> bool {
        let mut candidate = self.session.hierarchy.clone();
        if let Err(why) = change(&mut candidate, self.session.controller.graph()) {
            self.alerts.warn(format!("Cannot {verb}: {why}"));
            return false;
        }
        let renamed = match candidate.sync(&mut self.session.controller) {
            Ok(renamed) => renamed,
            Err(e) => {
                self.alerts.warn(format!("Cannot {verb}: {e}"));
                return false;
            }
        };
        self.session.hierarchy = candidate;
        self.assignments_renumbered(&renamed);
        true
    }

    /// After a sync gave assignments new ids: what keeps an id follows,
    /// so the matrix's selection and edit and an arc drag in progress
    /// still mean the same assignment.
    pub(crate) fn assignments_renumbered(&mut self, renamed: &[(loom_core::AssignmentId, loom_core::AssignmentId)]) {
        for &(old, new) in renamed {
            if self.session.matrix.selected == Some(old) {
                self.session.matrix.selected = Some(new);
            }
            if let Some(edit) = &mut self.session.matrix.edit
                && edit.id == old
            {
                edit.id = new;
            }
            self.session.gestures.renumber_depth(old, new);
        }
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
        // A direct cable into the same input yields to the record: the
        // sync replaces it, so nothing is removed before the check passes.
        if self.change_hierarchy("connect", |h, _| {
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
        if self.change_hierarchy("disconnect", |h, _| {
            h.disconnect(&level, &to_end);
            Ok(())
        }) {
            self.commit();
        }
    }

    /// True when a boundary record on the shown level feeds the input
    /// `to`: the canvas draws that cable from a port or sub-patch tile,
    /// whether or not the graph holds a cable for it (a port with nothing
    /// behind it has the record and no cable).
    pub(crate) fn link_into_shown(&self, to: PortRef) -> bool {
        self.level_end(to)
            .is_some_and(|end| self.session.hierarchy.has_link_into(&self.session.level, &end))
    }

    /// Takes tiles out of the patch: modules, with their cables; ports,
    /// with their cables; sub-patches, with everything inside. The cables
    /// are checked and settled first, with the modules still in the graph,
    /// so a refusal changes nothing (ADR-0039, invariant 3). The Transport
    /// stays, as the controller would refuse it. Returns whether the
    /// commit was sent.
    pub(crate) fn delete_tiles(&mut self, ids: &[u32]) -> bool {
        let transport = self.session.controller.transport_module();
        let mut candidate = self.session.hierarchy.clone();
        let mut modules: Vec<ModuleId> = Vec::new();
        let mut gone: Vec<ModuleId> = Vec::new();
        for &id in ids {
            if Some(ModuleId(id)) == transport {
                continue;
            }
            let removed = candidate.remove(self.session.controller.graph(), ModuleId(id));
            modules.extend(removed.modules);
            gone.extend(removed.tiles);
        }
        let renamed = match candidate.sync_without(&mut self.session.controller, &modules) {
            Ok(renamed) => renamed,
            Err(e) => {
                self.alerts.warn(format!("Cannot delete: {e}"));
                return false;
            }
        };
        self.assignments_renumbered(&renamed);
        for module in &modules {
            let _ = self.session.controller.remove_module(*module);
        }
        for id in &gone {
            self.session.positions.remove(&id.0);
            self.session.sizes.remove(&id.0);
            self.session.selection.remove(&id.0);
            self.session.cameras.remove(&self.session.hierarchy.path_of(*id));
        }
        self.session.hierarchy = candidate;
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
                Ok(renamed) => {
                    app.session.hierarchy = candidate;
                    app.assignments_renumbered(&renamed);
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
                Ok(renamed) => {
                    app.session.hierarchy = candidate;
                    app.assignments_renumbered(&renamed);
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
            if app.change_hierarchy("add a port", |h, _| {
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

    /// True when an assignment from `source` onto parameter `index` of
    /// `target` is refused, with the reason in the status bar: the source is
    /// a port tile, or the target is on another level and no sub-patch tile
    /// of the source's level exposes it (rule 4; sub-patches.md, Editor
    /// model). Through an exposed parameter the assignment reaches inside.
    pub(crate) fn refuses_assignment(&mut self, source: OutputRef, target: ModuleId, index: usize) -> bool {
        let session = &self.session;
        let graph = session.controller.graph();
        let allowed = graph.node(source.module).is_some()
            && graph.node(target).is_some_and(|node| {
                let source_level = session.hierarchy.level_of(source.module);
                source_level == session.hierarchy.level_of(target)
                    || node.descriptor.params.get(index).is_some_and(|spec| {
                        session
                            .hierarchy
                            .surface_for(&source_level, &(target, spec.name.to_string()))
                            .is_some()
                    })
            });
        if !allowed {
            self.alerts.warn(NO_MODULATION_ACROSS.to_string());
        }
        !allowed
    }

    /// After an assignment is added or retargeted in a nested patch: puts
    /// the live assignments on each parameter in the order the file loads
    /// them (sub-patches.md, Editor model). A flat patch has that order
    /// already. False, with the reason in the status bar, when the
    /// hierarchy could not be written.
    pub(crate) fn sync_hierarchy(&mut self, verb: &str) -> bool {
        if self.session.hierarchy.is_flat() {
            return true;
        }
        self.change_hierarchy(verb, |_, _| Ok(()))
    }

    /// What an Expose on row `index` of tile `tile` names: the enclosing
    /// level, the tile's id in it, the parameter or exposed name, and the
    /// label the step takes. A module's row names its parameter; a
    /// sub-patch tile's row names its surface item.
    fn exposure_target(&self, tile: u32, index: usize) -> Option<(ModulePath, u32, String, String)> {
        let session = &self.session;
        let graph = session.controller.graph();
        let id = ModuleId(tile);
        let level = session.hierarchy.level_of(id);
        let lid = session.hierarchy.path_of(id).last();
        if let Some(node) = graph.node(id) {
            let spec = node.descriptor.params.get(index)?;
            return Some((level, lid, spec.name.to_string(), spec.label.to_string()));
        }
        let exposed = session.hierarchy.definition_of(id)?.exposed.get(index)?;
        Some((level, lid, exposed.name.clone(), exposed.name.clone()))
    }

    /// Expose, from the inspector: puts row `index` of tile `tile` on the
    /// enclosing sub-patch's surface, as one undo step (sub-patches.md,
    /// Exposing).
    pub(crate) fn expose(&mut self, tile: u32, index: usize) {
        self.close_slider_gesture();
        let Some((level, member, param, label)) = self.exposure_target(tile, index) else {
            return;
        };
        let surface = self
            .session
            .hierarchy
            .definition_at(&level)
            .map(|d| d.metadata.name.clone());
        self.session.step_name = Some(format!("expose {label}"));
        self.edit(|app| {
            let mut name = None;
            if app.change_hierarchy("expose", |h, graph| {
                name = Some(h.expose(graph, &level, member, &param)?);
                Ok(())
            }) && app.commit()
                && let (Some(name), Some(surface)) = (name, surface)
            {
                app.alerts.say(format!("{name} is exposed on {surface}"));
            }
        });
    }

    /// Unexpose, from the inspector: takes the `index`th surface item off the
    /// sub-patch tile `tile`, and off every surface above that re-exposed
    /// it, and with it every assignment onto the parameter from another
    /// level, as one undo step (sub-patches.md, Exposing).
    pub(crate) fn unexpose(&mut self, tile: u32, index: usize) {
        self.close_slider_gesture();
        let hierarchy = &self.session.hierarchy;
        let Some(exposed) = hierarchy
            .definition_of(ModuleId(tile))
            .and_then(|d| d.exposed.get(index).cloned())
        else {
            return;
        };
        let inside = hierarchy.path_of(ModuleId(tile));
        // The flat parameter the surface item stood for, for the assignments.
        let target = self
            .control_target(tile, index, loom_canvas::ControlPart::Main)
            .map(|t| (t.module, t.index));
        let label = exposed.name.clone();
        self.session.step_name = Some(format!("unexpose {label}"));
        self.edit(|app| {
            if !app.change_hierarchy("unexpose", |h, _| {
                h.unexpose(&inside, exposed.module, &exposed.param);
                Ok(())
            }) {
                return;
            }
            // The assignments onto the parameter that no surface of their
            // source's level reaches any more go with the exposure; one
            // through a surface below stays (rule 4).
            if let Some((module, param)) = target {
                let graph = app.session.controller.graph();
                let hierarchy = &app.session.hierarchy;
                let name = graph
                    .node(module)
                    .and_then(|n| n.descriptor.params.get(param))
                    .map(|spec| spec.name.to_string());
                let own_level = hierarchy.level_of(module);
                let stranded: Vec<loom_core::AssignmentId> = graph
                    .assignments()
                    .filter(|(_, a)| a.target == module && a.param == param)
                    .filter(|(_, a)| {
                        let source_level = hierarchy.level_of(a.source.module);
                        source_level != own_level
                            && name.as_ref().is_none_or(|name| {
                                hierarchy.surface_for(&source_level, &(module, name.clone())).is_none()
                            })
                    })
                    .map(|(id, _)| id)
                    .collect();
                for id in stranded {
                    let _ = app.session.controller.remove_assignment(id);
                }
            }
            if app.commit() {
                app.alerts.say(format!("{label} is no longer exposed"));
            }
        });
    }

    /// Renames the sub-patch of tile `tile`, from its inspector, as one undo
    /// step. The tile, the breadcrumb, and the file follow; the definition's
    /// key stays.
    pub(crate) fn rename_subpatch(&mut self, tile: u32, name: &str) {
        self.close_slider_gesture();
        if self.session.hierarchy.tile_name(ModuleId(tile)).as_deref() == Some(name.trim()) {
            return;
        }
        self.session.step_name = Some("rename sub-patch".to_string());
        self.edit(|app| {
            if app.change_hierarchy("rename", |h, _| h.rename(ModuleId(tile), name)) {
                app.commit();
            }
        });
    }

    /// Unlink, from the tile menu: the linked sub-patch tile `tile` becomes
    /// a plain copy, as one undo step (sub-patches.md, Linked library
    /// items).
    pub(crate) fn unlink(&mut self, tile: u32) {
        self.close_slider_gesture();
        let Some(name) = self.session.hierarchy.tile_name(ModuleId(tile)) else {
            return;
        };
        self.session.step_name = Some(format!("unlink {name}"));
        self.edit(|app| {
            if app.change_hierarchy("unlink", |h, _| h.unlink(ModuleId(tile)).map(|_| ())) && app.commit() {
                app.alerts.say(now_a_copy(&name));
            }
        });
    }

    /// Inserts a library item as a new sub-patch tile at `world`, as a copy
    /// or linked, and selects it, as one undo step (sub-patches.md,
    /// Library). The file's own linked sub-patches are refreshed first. A
    /// Transport or Scale and tuning module in the file is left out, its
    /// cables become ports, and the status bar says so (rule 8).
    pub(crate) fn insert_library_item(&mut self, item: &browser::LibraryItem, linked: bool, world: Point) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        let mut file = match crate::patch_file::load(&item.path) {
            Ok(file) => file,
            Err(e) => {
                self.alerts.warn(format!("Cannot insert {}: {e}", item.name));
                return;
            }
        };
        let mut notes = match file.refresh_linked(&self.rig.library(), self.session.controller.registry()) {
            Ok(notes) => notes,
            Err(e) => {
                self.alerts.warn(format!("Cannot insert {}: {e}", item.name));
                return;
            }
        };
        let library = linked.then(|| item.library.clone());
        let level = self.session.level.clone();
        let name = file.metadata.name.clone();
        self.session.step_name = Some(format!("insert {name}"));
        self.edit(|app| {
            let mut candidate = app.session.hierarchy.clone();
            let inserted = match candidate.insert(&mut app.session.controller, &level, &file, library.as_deref()) {
                Ok(inserted) => inserted,
                Err(e) => {
                    app.alerts.warn(format!("Cannot insert {name}: {e}"));
                    return;
                }
            };
            let renamed = match candidate.sync(&mut app.session.controller) {
                Ok(renamed) => renamed,
                Err(e) => {
                    for id in &inserted.modules {
                        let _ = app.session.controller.remove_module(*id);
                    }
                    app.commit();
                    app.alerts.warn(format!("Cannot insert {name}: {e}"));
                    return;
                }
            };
            app.session.hierarchy = candidate;
            app.assignments_renumbered(&renamed);
            let at = browser::snap(world);
            app.session.positions.insert(inserted.instance.0, (at.x, at.y));
            for (id, layout) in &inserted.layouts {
                if let Some(position) = layout.position {
                    app.session.positions.insert(id.0, position);
                }
                if let Some(size) = layout.size {
                    app.session.sizes.insert(id.0, size);
                }
            }
            app.session.selection = BTreeSet::from([inserted.instance.0]);
            if app.commit() {
                notes.extend(inserted.left_out.iter().map(|dropped| dropped.warning(&inserted.name)));
                notes.extend(inserted.notes);
                let done = format!("Inserted {}", inserted.name);
                if notes.is_empty() {
                    app.alerts.say(done);
                } else {
                    app.alerts
                        .warn(format!("{done}. {}", crate::file_commands::join_notes(&notes)));
                }
            }
        });
        self.session.step_name = None;
    }

    /// What the tile menu offers for a tile: Help for a module; Open,
    /// Dissolve, Save to library, and Unlink (when linked) for a sub-patch;
    /// and Make sub-patch for any selection of modules.
    pub(crate) fn tile_menu_items(&self, module: u32) -> Vec<(&'static str, Message)> {
        let session = &self.session;
        let tile = session.hierarchy.tile(session.controller.graph(), ModuleId(module));
        match tile {
            Some(Tile::Instance(_)) => {
                let mut items = vec![
                    ("Open", Message::OpenTile(module)),
                    ("Dissolve sub-patch", Message::DissolveSubPatch(module)),
                    ("Save to library", Message::SaveToLibrary(module)),
                ];
                if session.hierarchy.library_of(ModuleId(module)).is_some() {
                    items.push(("Unlink", Message::Unlink(module)));
                }
                items
            }
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
