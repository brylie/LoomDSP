//! Slots in the app (`docs/design/sub-patches.md`, Slots): Make slot from
//! a module in place, Swap from the tile menu or the inspector, Dissolve
//! slot, and finishing a swap's crossfade once every fade reports it is
//! done. The hierarchy does the bookkeeping (`loom_patch::Hierarchy`);
//! here each gesture is one undo step with a status text.

use crate::{App, Message, browser};
use loom_core::{ModuleId, RoleDescriptor};
use loom_patch::{PartKey, Patch, hierarchy};
use serde::Serialize;

/// The status bar text when a selection with a slot tile is copied or cut
/// (sub-patches.md, Slots). A follow-up carries the slot.
pub const CANNOT_COPY_SLOT: &str = "Cannot copy a slot yet";

/// The inspector's Part choice that empties the slot, and names an empty
/// slot's part (sub-patches.md, Empty slots).
pub const NO_PART_CHOICE: &str = "No part";

/// The graph's assignments and MIDI mappings at one moment, so a failed
/// swap can put back what it moved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Links {
    /// Every assignment, by id.
    pub assignments: Vec<(loom_core::AssignmentId, loom_core::Assignment)>,
    /// Every mapping.
    pub midi: Vec<loom_core::MidiMapping>,
}

impl Links {
    /// The links `graph` holds now.
    pub(crate) fn of(graph: &loom_core::Graph) -> Self {
        Self {
            assignments: graph.assignments().map(|(id, a)| (id, *a)).collect(),
            midi: graph.midi_mappings().map(|(_, m)| *m).collect(),
        }
    }
}

/// What a slot can swap to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SwapTarget {
    /// A module type, by kind.
    Module(String),
    /// A library item, by its path in the library folder, inserted as a
    /// copy.
    Library(String),
    /// A sub-patch the slot held before, by definition key.
    Held(String),
    /// No part: Remove part empties the slot (sub-patches.md, Empty slots).
    /// Never in a Swap list; the tile menu's Remove part and the
    /// inspector's `No part` choice send it.
    Empty,
}

/// One entry of a slot's Swap list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SwapChoice {
    /// What the player sees: the part's name.
    pub label: String,
    /// What choosing it does.
    pub to: SwapTarget,
    /// True for the part the slot holds now.
    pub current: bool,
}

impl std::fmt::Display for SwapChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl App {
    /// The parts the slot tile `slot` can swap to: every registered module
    /// type that declares its role, every library item whose file declares
    /// it, and every sub-patch the slot holds, the current part marked
    /// (sub-patches.md, Slots: Swap).
    pub(crate) fn swap_choices(&self, slot: u32) -> Vec<SwapChoice> {
        let session = &self.session;
        let Some(state) = session.hierarchy.slot(ModuleId(slot)) else {
            return Vec::new();
        };
        let graph = session.controller.graph();
        let current_kind = session
            .hierarchy
            .part_of_slot(ModuleId(slot))
            .and_then(|part| graph.node(part))
            .map(|node| node.descriptor.kind());
        let current_sub_patch = session.hierarchy.part_of_slot(ModuleId(slot)).and_then(|part| {
            let path = session.hierarchy.path_of(part);
            Some((
                session.hierarchy.key_at(&path)?.to_string(),
                session.hierarchy.tile_name(part)?,
            ))
        });
        let mut choices: Vec<SwapChoice> = session
            .controller
            .registry()
            .parts_of(state.role.id)
            .into_iter()
            .map(|d| SwapChoice {
                label: d.name.to_string(),
                current: current_kind.as_deref() == Some(d.kind().as_str()),
                to: SwapTarget::Module(d.kind()),
            })
            .collect();
        for (key, held) in &state.held {
            if let PartKey::SubPatch(key) = key {
                let _ = held;
                let name = session
                    .hierarchy
                    .held_definition_name(key)
                    .unwrap_or_else(|| key.clone());
                choices.push(SwapChoice {
                    label: name,
                    to: SwapTarget::Held(key.clone()),
                    current: false,
                });
            }
        }
        if let Some((key, name)) = current_sub_patch {
            choices.push(SwapChoice {
                label: name,
                to: SwapTarget::Held(key),
                current: true,
            });
        }
        for item in &self.library {
            if item.roles.iter().any(|r| loom_patch::role_id(r) == state.role.id) {
                choices.push(SwapChoice {
                    label: item.name.clone(),
                    to: SwapTarget::Library(item.library.clone()),
                    current: false,
                });
            }
        }
        choices
    }

    /// Reads the library folder's items into the app, once, for the Swap
    /// lists. Nothing when no folder is set.
    pub(crate) fn refresh_library_items(&mut self) {
        self.library = match self.rig.library() {
            crate::settings::RigLibrary::Folder(folder) => browser::library_items(&folder.root),
            crate::settings::RigLibrary::None => Vec::new(),
        };
    }

    /// The tile menu's Make slot items for the tile `module`: one per role
    /// it declares.
    pub(crate) fn make_slot_items(&self, module: u32) -> Vec<(String, Message)> {
        self.roles_of(module)
            .into_iter()
            .map(|role| {
                (
                    hierarchy::make_slot_label(role),
                    Message::MakeSlot {
                        module,
                        role: role.id.to_string(),
                    },
                )
            })
            .collect()
    }

    /// The roles the module or sub-patch tile `module` declares.
    fn roles_of(&self, module: u32) -> Vec<&'static RoleDescriptor> {
        let session = &self.session;
        let registry = session.controller.registry();
        if let Some(node) = session.controller.graph().node(ModuleId(module)) {
            if node.unknown.is_some() {
                return Vec::new();
            }
            return node.descriptor.roles.iter().map(|f| f.role).collect();
        }
        session
            .hierarchy
            .definition_of(ModuleId(module))
            .map(|d| {
                d.roles
                    .iter()
                    .filter_map(|kind| registry.resolve_role(kind).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Make slot, from the tile menu: the tile `module` becomes the part of
    /// a new slot for `role_id`, in place, as one undo step (sub-patches.md,
    /// Slots: Make slot). Cables on ports the role does not have are
    /// removed, and the status bar names them.
    pub(crate) fn make_slot(&mut self, module: u32, role_id: &str) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        let Some(role) = self.session.controller.registry().role(role_id) else {
            self.alerts.warn(format!("Cannot make a slot: unknown role {role_id}"));
            return;
        };
        self.session.step_name = Some(format!("make {} slot", role.name));
        let position = self.session.positions.get(&module).copied();
        let size = self.session.sizes.get(&module).copied();
        self.edit(|app| {
            let mut made = None;
            if app.change_hierarchy("make a slot", |h, graph| {
                made = Some(h.make_slot(graph, ModuleId(module), role)?);
                Ok(())
            }) && let Some(made) = made
            {
                // The slot tile stands where the module stood.
                if let Some(position) = position {
                    app.session.positions.insert(made.slot.0, position);
                }
                if let Some(size) = size {
                    app.session.sizes.insert(made.slot.0, size);
                }
                app.session.positions.remove(&module);
                app.session.sizes.remove(&module);
                app.session.selection = std::collections::BTreeSet::from([made.slot.0]);
                if app.commit() {
                    app.alerts.say(hierarchy::made_slot_text(role, &made.removed));
                }
            }
        });
    }

    /// Adds, places, and selects an empty slot for `role` in one undo step
    /// (sub-patches.md, Empty slots).
    pub(crate) fn add_empty_slot(&mut self, role: &'static RoleDescriptor, placement: loom_canvas::BrowserPlacement) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        let level = self.session.level.clone();
        self.session.step_name = Some(format!("add {} slot", role.name));
        self.edit(|app| {
            let mut added = None;
            if app.change_hierarchy("add a slot", |h, _| {
                added = Some(h.add_empty_slot(&level, role)?);
                Ok(())
            }) && let Some(slot) = added
            {
                let name = app
                    .session
                    .hierarchy
                    .tile_name(slot)
                    .unwrap_or_else(|| role.name.to_string());
                if app.commit_added_tile(slot, placement) {
                    app.alerts.say(hierarchy::added_slot_text(&name));
                }
            }
        });
    }

    /// Dissolve slot, from the tile menu: the part is a plain module again,
    /// in place, as one undo step.
    pub(crate) fn dissolve_slot(&mut self, slot: u32) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        self.finish_all_fades();
        let Some(name) = self.session.hierarchy.tile_name(ModuleId(slot)) else {
            return;
        };
        let part = self.session.hierarchy.part_of_slot(ModuleId(slot));
        self.session.step_name = Some(format!("dissolve {name} slot"));
        self.edit(|app| {
            let mut removed = Vec::new();
            if app.change_hierarchy("dissolve the slot", |h, graph| {
                removed = h.dissolve_slot(graph, ModuleId(slot))?;
                Ok(())
            }) {
                for id in removed {
                    let _ = app.session.controller.remove_module(id);
                }
                if let (Some(part), Some(position)) = (part, app.session.positions.remove(&slot)) {
                    app.session.positions.insert(part.0, position);
                }
                if let (Some(part), Some(size)) = (part, app.session.sizes.remove(&slot)) {
                    app.session.sizes.insert(part.0, size);
                }
                if let Some(part) = part {
                    app.session.selection = std::collections::BTreeSet::from([part.0]);
                }
                if app.commit() {
                    app.alerts.say(format!("{name} slot dissolved"));
                }
            }
        });
    }

    /// Swap, from the tile menu, the inspector, or a bound widget's tile
    /// menu: the slot tile `slot` takes the part `to`, as one undo step,
    /// with a 10 ms crossfade (sub-patches.md, Slots: Swap).
    pub(crate) fn swap_slot(&mut self, slot: u32, to: SwapTarget) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        let Some(name) = self.session.hierarchy.tile_name(ModuleId(slot)) else {
            return;
        };
        let file = match &to {
            SwapTarget::Library(path) => match self.rig.library() {
                crate::settings::RigLibrary::Folder(folder) => {
                    use loom_patch::Library;
                    match folder.read(path) {
                        Ok(mut file) => {
                            let registry = self.session.controller.registry().clone();
                            file.refresh_linked(&folder, &registry);
                            Some(file)
                        }
                        Err(e) => {
                            self.alerts.warn(format!("Cannot swap {name}: {path} {e:?}"));
                            return;
                        }
                    }
                }
                crate::settings::RigLibrary::None => {
                    self.alerts
                        .warn(format!("Cannot swap {name}: there is no library folder"));
                    return;
                }
            },
            _ => None,
        };
        // A fade in progress is finished on the live session first, so a
        // refused swap leaves nothing half done (sub-patches.md, How a swap
        // crossfades, point 5).
        self.finish_all_fades();
        if to == SwapTarget::Empty {
            self.remove_part(slot, &name);
            return;
        }
        self.session.step_name = Some(format!("swap {name}"));
        self.edit(|app| {
            let result = app.swap_now(slot, &to, file.as_ref());
            match result {
                Ok(swapped) => {
                    if app.commit() {
                        app.alerts
                            .say(hierarchy::swapped_text(&name, &swapped.part_name, &swapped.removed));
                    }
                }
                Err(why) => app.alerts.warn(format!("Cannot swap {name}: {why}")),
            }
        });
    }

    /// Remove part: empties the slot tile `slot`, named `name`, as one undo
    /// step (sub-patches.md, Empty slots). A thru slot fades the part out
    /// to its thru signal, in every stack copy; any other lets it go now,
    /// removed from the graph only once the sync has succeeded. When the
    /// fades do not fit the module limit, they are dropped and the part
    /// goes now too, so a removal never fails for room (The module limit).
    /// The status names the assignments and mappings that had nowhere to
    /// go.
    fn remove_part(&mut self, slot: u32, name: &str) {
        self.session.step_name = Some(format!("remove part from {name}"));
        self.edit(|app| {
            let layout = app.layout_fn_owned();
            let mut candidate = app.session.hierarchy.clone();
            let before: std::collections::BTreeSet<ModuleId> =
                app.session.controller.graph().nodes().map(|(id, _)| id).collect();
            let links = Links::of(app.session.controller.graph());
            let emptied = match candidate.swap_to_empty(&mut app.session.controller, &layout, ModuleId(slot)) {
                Ok(emptied) => emptied,
                Err(why) => {
                    app.remove_added_since(&before);
                    app.restore_links(&links);
                    app.alerts.warn(format!("Cannot remove the part from {name}: {why}"));
                    return;
                }
            };
            let mut gone = emptied.gone.clone();
            let mut renamed = Vec::new();
            let mut synced = candidate.sync_without(&mut app.session.controller, &gone);
            // Measured after the sync, by the commit's own rule. A sync can
            // also meet the limit itself, when it adds a mixer the editor
            // implies, so a refusal for room takes the same way out.
            let no_room = match &synced {
                Ok(_) => !app.fits(0),
                Err(e) => is_room_error(&app.session.controller, &sync_why(e)),
            };
            if no_room {
                // The fades do not fit, so the part goes now and rule 9
                // ramps each copy over 5 ms: a removal never fails for room.
                if let Ok(first) = &synced {
                    renamed.extend(first.iter().copied());
                }
                gone.extend(candidate.drop_removal_fades(&mut app.session.controller, ModuleId(slot)));
                synced = candidate.sync_without(&mut app.session.controller, &gone);
            }
            match synced {
                Ok(last) => {
                    renamed.extend(last);
                    for id in &gone {
                        let _ = app.session.controller.remove_module(*id);
                    }
                    app.session.hierarchy = candidate;
                    app.assignments_renumbered(&renamed);
                    if app.commit() {
                        app.alerts
                            .say(hierarchy::removed_part_text(name, &emptied.part_name, &emptied.removed));
                    }
                }
                Err(e) => {
                    app.remove_added_since(&before);
                    app.restore_links(&links);
                    let why = app.room_text_for(slot, sync_why(&e));
                    app.alerts.warn(format!("Cannot remove the part from {name}: {why}"));
                }
            }
        });
    }

    /// Performs a swap on a candidate hierarchy and syncs it, rolling the
    /// added modules back on a refusal. A swap that does not fit the module
    /// limit, measured after the sync, is refused whole with the limit's
    /// text (sub-patches.md, The module limit).
    fn swap_now(&mut self, slot: u32, to: &SwapTarget, file: Option<&Patch>) -> Result<hierarchy::Swapped, String> {
        let layout = self.layout_fn_owned();
        let mut candidate = self.session.hierarchy.clone();
        // What the graph held, so a refusal part-way can take back what the
        // swap added before it failed, and put back the links it moved.
        let before: std::collections::BTreeSet<ModuleId> =
            self.session.controller.graph().nodes().map(|(id, _)| id).collect();
        let links = Links::of(self.session.controller.graph());
        let part = candidate.part_of_slot(ModuleId(slot));
        let holds_already = match to {
            SwapTarget::Module(kind) => part
                .and_then(|p| self.session.controller.graph().node(p))
                .is_some_and(|n| n.descriptor.kind() == *kind),
            SwapTarget::Held(key) => {
                part.is_some_and(|p| candidate.key_at(&candidate.path_of(p)) == Some(key.as_str()))
            }
            // Remove part goes through `remove_part`, never here: the swap
            // below refuses it.
            SwapTarget::Library(_) | SwapTarget::Empty => false,
        };
        if holds_already {
            return Err("it holds that part already".to_string());
        }
        let swapped = match to {
            SwapTarget::Module(kind) => {
                candidate.swap_to_module(&mut self.session.controller, &layout, ModuleId(slot), kind)
            }
            SwapTarget::Held(key) => {
                candidate.swap_to_held_subpatch(&mut self.session.controller, &layout, ModuleId(slot), key)
            }
            SwapTarget::Library(path) => {
                let file = file.ok_or_else(|| "the library item did not read".to_string())?;
                let _ = path;
                candidate.swap_to_subpatch(&mut self.session.controller, &layout, ModuleId(slot), file, None)
            }
            SwapTarget::Empty => Err("Remove part is not a swap to a part".to_string()),
        };
        let swapped = match swapped {
            Ok(swapped) => swapped,
            Err(why) => {
                // The live hierarchy is untouched; whatever the swap added to
                // the controller before it failed goes again, and the links
                // it moved come back.
                self.remove_added_since(&before);
                self.restore_links(&links);
                return Err(self.room_text_for(slot, why));
            }
        };
        let why = match candidate.sync(&mut self.session.controller) {
            Ok(renamed) if self.fits(0) => {
                self.session.hierarchy = candidate;
                self.assignments_renumbered(&renamed);
                return Ok(swapped);
            }
            Ok(renamed) => {
                self.assignments_renumbered(&renamed);
                self.no_room_text(slot)
            }
            Err(e) => self.room_text_for(slot, sync_why(&e)),
        };
        // Taken back as a failed sync is: the live hierarchy, which the
        // candidate never replaced, syncs again, so the graph's cables are
        // as they were.
        self.remove_added_since(&before);
        self.restore_links(&links);
        if let Ok(renamed) = self.session.hierarchy.sync(&mut self.session.controller) {
            self.assignments_renumbered(&renamed);
        }
        Err(why)
    }

    /// True when the next commit fits the engine's module limit with `room`
    /// more slots taken: the copy slots it would start, by the commit's own
    /// rule, plus `room`, are at most the free slots (sub-patches.md, The
    /// module limit). Measured after a sync, since a fade's width follows
    /// the cables the sync gives it.
    pub(crate) fn fits(&self, room: usize) -> bool {
        let controller = &self.session.controller;
        controller.slots_needed().saturating_add(room) <= controller.free_module_slots()
    }

    /// Why an edit on the slot tile `slot` is refused at the module limit,
    /// with the Copies hint when a stack holds the slot.
    pub(crate) fn no_room_text(&self, slot: u32) -> String {
        let stack = self.session.hierarchy.stack_holding(ModuleId(slot));
        hierarchy::too_many_modules_text(self.session.controller.config().max_modules, stack.as_deref())
    }

    /// `why`, or the module limit's text when `why` is the engine's refusal
    /// for lack of room, which a part added on the way can meet.
    pub(crate) fn room_text_for(&self, slot: u32, why: String) -> String {
        if is_room_error(&self.session.controller, &why) {
            self.no_room_text(slot)
        } else {
            why
        }
    }

    /// Puts back every assignment and mapping in `links` that the graph
    /// lacks or holds changed, with its id: what a failed swap moved onto
    /// a part that was then taken back.
    pub(crate) fn restore_links(&mut self, links: &Links) {
        for (id, assignment) in &links.assignments {
            match self.session.controller.graph().assignment(*id) {
                Some(live) if live == assignment => {}
                Some(_) => {
                    let _ = self.session.controller.retarget_assignment(
                        *id,
                        assignment.source,
                        assignment.target,
                        assignment.param,
                    );
                }
                None => {
                    let _ = self.session.controller.restore_assignment(*id, *assignment);
                }
            }
        }
        for mapping in &links.midi {
            if self
                .session
                .controller
                .graph()
                .midi_mapping_of(mapping.target, mapping.param)
                .is_none()
            {
                let _ = self.session.controller.map_midi_with(
                    mapping.control,
                    mapping.target,
                    mapping.param,
                    mapping.options,
                );
            }
        }
    }

    /// Removes every module the graph holds that `before` did not.
    pub(crate) fn remove_added_since(&mut self, before: &std::collections::BTreeSet<ModuleId>) {
        let added: Vec<ModuleId> = self
            .session
            .controller
            .graph()
            .nodes()
            .map(|(id, _)| id)
            .filter(|id| !before.contains(id))
            .collect();
        for id in added {
            let _ = self.session.controller.remove_module(id);
        }
    }

    /// Finishes every swap whose fades all report done: the leaving part
    /// and the fades go, and the graph is committed again. Bookkeeping,
    /// not an edit: no undo step (sub-patches.md, How a swap crossfades,
    /// point 3). Runs on each tick and after the engine ran in a test.
    pub(crate) fn finish_due_fades(&mut self) {
        let due: Vec<ModuleId> = self
            .session
            .hierarchy
            .fading_slots()
            .into_iter()
            .filter(|(_, fades)| {
                fades
                    .iter()
                    .all(|fade| self.session.controller.display(*fade, 0) == Some(1.0))
            })
            .map(|(slot, _)| slot)
            .collect();
        if due.is_empty() {
            return;
        }
        for slot in due {
            self.session.hierarchy.finish_fade(&mut self.session.controller, slot);
        }
        match self.session.hierarchy.sync(&mut self.session.controller) {
            Ok(renamed) => self.assignments_renumbered(&renamed),
            Err(e) => {
                debug_assert!(false, "finishing a fade must sync: {e}");
                self.alerts.warn(format!("Cannot finish a swap: {e}"));
            }
        }
        let _ = self.session.controller.commit();
        self.refresh();
    }

    /// Finishes every fade at once, whatever the fades report: before an
    /// undo, a redo, or a save of the session (point 5).
    pub(crate) fn finish_all_fades(&mut self) {
        if self.session.hierarchy.fading_slots().is_empty() {
            return;
        }
        self.session.hierarchy.finish_all_fades(&mut self.session.controller);
        if let Ok(renamed) = self.session.hierarchy.sync(&mut self.session.controller) {
            self.assignments_renumbered(&renamed);
        }
        let _ = self.session.controller.commit();
        self.refresh();
    }

    /// An owned copy of the layout closure, for a borrow of the session
    /// beside it.
    pub(crate) fn layout_fn_owned(&self) -> impl Fn(u32) -> loom_patch::Layout + 'static {
        let positions = self.session.positions.clone();
        let sizes = self.session.sizes.clone();
        let unknown = self.session.unknown_positions.clone();
        move |id: u32| {
            let shown = positions.get(&id);
            let position = match unknown.get(&id) {
                Some((original, initial)) if shown == Some(initial) => *original,
                _ => shown.map(|&(x, y)| (x.round(), y.round())),
            };
            loom_patch::Layout {
                position,
                size: sizes.get(&id).copied(),
            }
        }
    }
}

/// True when `why` is the engine's refusal for lack of a module slot.
pub(crate) fn is_room_error(controller: &loom_core::Controller, why: &str) -> bool {
    why == loom_core::LoomError::TooManyModules(controller.config().max_modules).to_string()
}

/// The text of a refused sync, with the engine's own text for an engine
/// error, so [`is_room_error`] can find a lack of room.
pub(crate) fn sync_why(error: &loom_patch::PatchError) -> String {
    match error {
        loom_patch::PatchError::Engine(e) => e.to_string(),
        e => e.to_string(),
    }
}

#[cfg(test)]
mod tests;
