//! The editor's overlay for sub-patch modes (`docs/design/sub-patches.md`,
//! Modes; ADR-0040).
//!
//! Stack copies past the first and the modules the flattener adds for a
//! mode are implied: their paths hold a step no module id can take
//! ([`ModulePath::is_implied`]), they are never tiles, never in the file or
//! an undo snapshot, and [`Hierarchy::sync`] makes the controller's implied
//! modules, values, cables, and assignments the flattener's. The editor
//! shows and edits copy 0, and a parameter write reaches every copy through
//! [`Hierarchy::copies_of`].
//!
//! The mode edits here change only the overlay, as collapse and dissolve
//! do: the caller syncs and commits.

use super::*;
use crate::flatten::{ADDED_STEP, Added, Flat, MEMBER_STEP};

/// The level id of a select's Member module: the added step of its path,
/// so the surface entry for the Member control finds it as a member of the
/// level.
pub const MEMBER_LID: u32 = ADDED_STEP + MEMBER_STEP;
use crate::modes::{self, MEMBER_PARAM, MOST_MEMBERS, Mode};

/// What a select plays before an edit to its members
/// ([`Hierarchy::playing_members`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayingMember {
    /// The select instance's path.
    pub instance: ModulePath,
    /// Its Member module.
    pub module: ModuleId,
    /// The member tile it plays, if any.
    pub playing: Option<ModuleId>,
    /// Its Member value.
    pub value: f32,
}

/// One assignment as the overlay compares them: source end, target end.
type AssignmentEnds = (End, End);

impl Hierarchy {
    /// The flat ids of every implied module: stack copies past the first,
    /// and every module the flattener added for a mode (ADR-0040).
    pub fn implied_ids(&self) -> BTreeSet<ModuleId> {
        self.ids
            .paths()
            .filter(|(path, _)| path.is_implied())
            .map(|(_, id)| id)
            .collect()
    }

    /// The modules an undo snapshot leaves out: [`Hierarchy::transient_ids`]
    /// less each select's Member module, whose value, assignments, and
    /// mappings are the player's and saved on the instance, so undo must
    /// restore them (sub-patches.md, Copies in the editor).
    pub fn undo_skip_ids(&self) -> BTreeSet<ModuleId> {
        let mut ids = self.transient_ids();
        for (id, added) in &self.added {
            if matches!(added, Added::Member { .. }) {
                ids.remove(id);
            }
        }
        ids
    }

    /// True for an implied module.
    pub fn is_implied_module(&self, id: ModuleId) -> bool {
        self.ids.path_of(id).is_some_and(ModulePath::is_implied)
    }

    /// The same module in every other copy of the stacks `id` is in, with
    /// the seed offset each adds, as of the last sync. Empty outside a
    /// stack. A parameter write to `id` reaches them all.
    pub fn copies_of(&self, id: ModuleId) -> &[(ModuleId, u32)] {
        self.copies.get(&id).map_or(&[], Vec::as_slice)
    }

    /// What an implied module the flattener added stands for, as of the
    /// last sync.
    pub fn added_module(&self, id: ModuleId) -> Option<&Added> {
        self.added.get(&id)
    }

    /// The Member module of the select instance at `instance`.
    pub fn member_module(&self, instance: &ModulePath) -> Option<ModuleId> {
        self.ids.get(&instance.added(MEMBER_STEP))
    }

    /// The module the flattener added behind the output `port` of the
    /// instance at `instance`, if its mode adds one.
    pub fn added_output(&self, instance: &ModulePath, port: &str) -> Option<ModuleId> {
        self.added.iter().find_map(|(id, added)| match added {
            Added::Output {
                instance: at,
                port: name,
            } if at == instance && name == port => Some(*id),
            _ => None,
        })
    }

    /// Keeps the Member control first on every select's surface, spanning 1
    /// to the member count, and off every other surface (sub-patches.md,
    /// The editor for modes). It is an exposed item of the Member module,
    /// named by its reserved level id ([`MEMBER_LID`]), so every surface
    /// path works on it unchanged; the file never holds it.
    pub fn refresh_member_entries(&mut self) {
        for definition in self.definitions.values_mut() {
            definition.exposed.retain(|e| e.module != MEMBER_LID);
            if definition.modes.mode == Mode::Select {
                let count = definition.modes.members.len().max(1) as f32;
                definition.exposed.insert(
                    0,
                    Exposed {
                        name: MEMBER_PARAM.to_string(),
                        module: MEMBER_LID,
                        param: MEMBER_PARAM.to_string(),
                        range: (count > 1.0).then_some((1.0, count)),
                    },
                );
            }
        }
    }

    /// True for the Member control's surface entry, which cannot be
    /// unexposed or renamed.
    pub fn is_member_entry(exposed: &Exposed) -> bool {
        exposed.module == MEMBER_LID
    }

    /// True when any sub-patch has a mode other than single, or implied
    /// modules are left from one (an undo back to single): only then does a
    /// commit need a sync for the copies and added modules to follow.
    pub fn has_modes(&self) -> bool {
        self.definitions.values().any(|d| !d.modes.mode.is_single()) || self.ids.paths().any(|(p, _)| p.is_implied())
    }

    /// The definition of the sub-patch tile `instance` as the file would
    /// write it, with every definition nested in it, mode fields included:
    /// what Add member copies (sub-patches.md, The editor for modes).
    pub fn whole_definition(
        &self,
        graph: &Graph,
        layout: impl Fn(u32) -> Layout,
        instance: ModuleId,
    ) -> Option<(SubPatch, BTreeMap<String, SubPatch>)> {
        let key = self.instances.get(self.ids.path_of(instance)?)?.key.clone();
        let patch = self.unflatten(graph, layout);
        let definition = patch.subpatches.get(&key)?.clone();
        let nested = crate::reachable_keys(&definition, &patch.subpatches)
            .into_iter()
            .filter(|k| *k != key)
            .filter_map(|k| patch.subpatches.get(&k).map(|d| (k, d.clone())))
            .collect();
        Some((definition, nested))
    }

    /// The mode settings of the definition shown at `level`.
    pub fn modes_at(&self, level: &ModulePath) -> Option<&ModeSettings> {
        self.definition_at(level).map(|d| &d.modes)
    }

    /// The mode settings of the sub-patch tile `instance`.
    pub fn modes_of(&self, instance: ModuleId) -> Option<&ModeSettings> {
        self.definition_of(instance).map(|d| &d.modes)
    }

    /// True when a stack with more than one copy holds `level` or a level
    /// above it.
    pub fn in_stack(&self, level: &ModulePath) -> bool {
        (1..=level.0.len()).any(|n| {
            self.modes_at(&level.prefix(n))
                .is_some_and(|m| m.mode == Mode::Stack && m.copy_count() > 1)
        })
    }

    /// The mode of the level `level` when it is a layer or select level,
    /// whose only tiles are its members.
    pub fn member_level(&self, level: &ModulePath) -> Option<Mode> {
        self.modes_at(level).map(|m| m.mode).filter(|m| m.has_members())
    }

    /// The breadcrumb name of the level at `level`: its name, with the copy
    /// count for a stack of more than one copy, `Voice ×4`.
    pub fn level_name(&self, level: &ModulePath) -> Option<String> {
        let definition = self.definition_at(level)?;
        let name = definition.metadata.name.clone();
        Some(match definition.modes.copy_count() {
            1 => name,
            n => format!("{name} ×{n}"),
        })
    }

    /// True when the last sync ran against the controller's current
    /// revision: nothing structural changed since, so a commit can skip
    /// another sync (ADR-0040).
    pub fn synced_with(&self, controller: &Controller) -> bool {
        self.synced_revision.0 == Some(controller.revision())
    }

    /// Keeps what the flattener said about copies and added modules.
    pub(super) fn note_flat(&mut self, flat: &Flat) {
        self.copies = flat.copies.clone();
        self.added = flat.added.clone();
    }

    /// Makes the controller's implied modules the ones `flat` has: those
    /// gone are removed, new ones added at their values and given their
    /// paths, and the values of the others set to the flattener's. Runs
    /// before the cables are settled, so every cable has both ends.
    pub(super) fn sync_implied_modules(
        &mut self,
        controller: &mut Controller,
        flat: &Flat,
        scratch: &FlatIds,
    ) -> Result<(), PatchError> {
        let desired: BTreeMap<ModuleId, &crate::FlatModule> = flat
            .modules
            .iter()
            .filter(|m| m.path.is_implied())
            .map(|m| (m.id, m))
            .collect();
        // A leaving part or a fade of a stack copy sits at an implied path,
        // but the swap made it, not the flattener (ADR-0044).
        let fading = self.fading_ids();
        let current: Vec<ModuleId> = controller
            .graph()
            .nodes()
            .map(|(id, _)| id)
            .filter(|id| self.is_implied_module(*id) && !fading.contains(id))
            .collect();
        for id in current.iter().filter(|id| !desired.contains_key(id)) {
            controller.remove_module(*id)?;
            if let Some(path) = self.ids.path_of(*id).cloned() {
                self.ids.forget(&path);
            }
        }
        for (id, module) in &desired {
            if controller.graph().node(*id).is_none() {
                if let Some(path) = scratch.path_of(*id) {
                    self.ids.reserve(path.clone(), *id)?;
                }
                module.module.add_to(controller, *id)?;
                continue;
            }
            // Ranges first, as a load sets them, so a range never clamps a
            // value; spread works in the control's range, so copies must share
            // copy 0's.
            let ranges: Vec<(usize, loom_core::ParamRange)> = {
                let node = controller.graph().node(*id).expect("checked above");
                module
                    .module
                    .ranges
                    .iter()
                    .enumerate()
                    .filter_map(|(index, range)| range.filter(|r| node.range(index) != Some(*r)).map(|r| (index, r)))
                    .collect()
            };
            for (index, range) in ranges {
                controller.set_range(*id, index, range)?;
            }
            let changes: Vec<(usize, f32)> = {
                let node = controller.graph().node(*id).expect("checked above");
                module
                    .module
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(index, value)| {
                        !module.module.descriptor.params[*index].is_transient()
                            && node.params.get(*index) != Some(value)
                    })
                    .map(|(index, value)| (index, *value))
                    .collect()
            };
            for (index, value) in changes {
                controller.set_param(*id, index, value)?;
            }
        }
        Ok(())
    }

    /// Makes the controller's implied assignments (those with an implied
    /// source or target, other than onto a select's Member) the ones `flat`
    /// has, keeping the id of each that stays, and sets the followers of
    /// each mapping (ADR-0040).
    pub(super) fn sync_implied_links(
        &mut self,
        controller: &mut Controller,
        flat: &Flat,
        through_fade: &BTreeMap<End, End>,
    ) -> Result<(), PatchError> {
        // An assignment onto a fading module keeps going until the fade
        // ends: a leaving copy keeps its spread (ADR-0044).
        let fading = self.fading_ids();
        let is_implied = |hierarchy: &Self, end: u32| {
            let id = ModuleId(end);
            hierarchy.is_implied_module(id) && !matches!(hierarchy.added.get(&id), Some(Added::Member { .. }))
        };
        let graph = controller.graph();
        let mut current: BTreeMap<AssignmentEnds, Vec<AssignmentId>> = BTreeMap::new();
        for (id, a) in graph.assignments() {
            let source = (a.source.module.0, crate::port_name(graph, a.source));
            let target = (a.target.0, crate::param_name(graph, a.target, a.param));
            if fading.contains(&a.target) {
                continue;
            }
            if is_implied(self, source.0) || is_implied(self, target.0) {
                current.entry((source, target)).or_default().push(id);
            }
        }
        let mut wanted: Vec<PatchModulation> = Vec::new();
        for entry in &flat.modulation {
            if is_implied(self, entry.source.0) || is_implied(self, entry.target.0) {
                let mut entry = entry.clone();
                if let Some(fade_out) = through_fade.get(&entry.source) {
                    entry.source = fade_out.clone();
                }
                wanted.push(entry);
            }
        }
        let mut add = Vec::new();
        for entry in wanted {
            let ends = (entry.source.clone(), entry.target.clone());
            match current
                .get_mut(&ends)
                .and_then(|ids| (!ids.is_empty()).then(|| ids.remove(0)))
            {
                Some(id) => {
                    let assignment = *controller.graph().assignment(id).expect("listed above");
                    if assignment.depth != entry.depth {
                        controller.set_assignment_depth(id, entry.depth)?;
                    }
                    let polarity = entry.polarity.map(Into::into);
                    if assignment.polarity != polarity {
                        controller.set_assignment_polarity(id, polarity)?;
                    }
                    if assignment.enabled != entry.enabled {
                        controller.set_assignment_enabled(id, entry.enabled)?;
                    }
                }
                None => add.push(entry),
            }
        }
        for ids in current.into_values() {
            for id in ids {
                controller.remove_assignment(id)?;
            }
        }
        for entry in add {
            let id = controller.add_assignment_named(
                ModuleId(entry.source.0),
                &entry.source.1,
                ModuleId(entry.target.0),
                &entry.target.1,
                entry.depth,
            )?;
            if let Some(polarity) = entry.polarity {
                controller.set_assignment_polarity(id, Some(polarity.into()))?;
            }
            if !entry.enabled {
                controller.set_assignment_enabled(id, false)?;
            }
        }
        controller.set_midi_followers(flat.followers_in(controller.graph()))?;
        Ok(())
    }

    /// The copies of each assignment onto a parameter of copy 0, the same
    /// source onto the same parameter of every other copy: an edit to the
    /// assignment's depth, polarity, or on state reaches them too.
    pub fn assignment_copies(&self, graph: &Graph, id: AssignmentId) -> Vec<AssignmentId> {
        let Some(assignment) = graph.assignment(id) else {
            return Vec::new();
        };
        let targets: BTreeSet<ModuleId> = self.copies_of(assignment.target).iter().map(|(c, _)| *c).collect();
        if targets.is_empty() {
            return Vec::new();
        }
        graph
            .assignments()
            .filter(|(other, a)| {
                *other != id
                    && a.source == assignment.source
                    && a.param == assignment.param
                    && targets.contains(&a.target)
            })
            .map(|(other, _)| other)
            .collect()
    }

    /// Why the sub-patch tile `instance` cannot change to `mode`, or `None`
    /// when it can (sub-patches.md, The editor for modes, How modes meet
    /// the rest). `role_name` names a role kind for the player.
    pub fn mode_refusal(&self, instance: ModuleId, mode: Mode, role_name: &dyn Fn(&str) -> String) -> Option<String> {
        let path = self.ids.path_of(instance)?;
        let definition = self.definition_at(path)?;
        let name = definition.metadata.name.clone();
        let from = definition.modes.mode;
        if from == mode {
            return None;
        }
        if let Some(role) = definition.roles.first() {
            return Some(modes::fills_a_role(&name, &role_name(role)));
        }
        // Single and stack keep copy 0's paths, so a slot stays put; a
        // change to or from members moves the inside, and a slot with it.
        if from.has_members() != mode.has_members() && self.slots.keys().any(|slot| slot.starts_with(path)) {
            return Some(modes::mode_moves_slot(&name, mode));
        }
        if from.has_members() && !mode.has_members() && definition.modes.members.len() > 1 {
            return Some(modes::several_members(&name, mode));
        }
        // The one member's own mode would be lost when its inside moves up.
        if from.has_members() && !mode.has_members() {
            let member = definition.modes.members.first().map(|lid| path.child(*lid));
            if let Some(member) = member
                && let Some(inner) = self.definition_at(&member)
                && !inner.modes.mode.is_single()
            {
                return Some(modes::member_has_mode(&inner.metadata.name, &name, mode));
            }
        }
        None
    }

    /// Changes the mode of the sub-patch tile `instance` (sub-patches.md,
    /// The editor for modes). Never changes the sound by itself: single to
    /// layer or select makes the inside member 1, keeping its flat ids and
    /// exposed names; back takes the one member's inside up again; a stack
    /// starts at one copy, and leaving one clears Copies and spread.
    pub fn set_mode(
        &mut self,
        graph: &Graph,
        instance: ModuleId,
        mode: Mode,
        role_name: &dyn Fn(&str) -> String,
    ) -> Result<(), String> {
        if let Some(refusal) = self.mode_refusal(instance, mode, role_name) {
            return Err(refusal);
        }
        let Some(path) = self.ids.path_of(instance).cloned() else {
            return Err(modes::NOT_A_SUBPATCH.to_string());
        };
        let Some(key) = self.instances.get(&path).map(|i| i.key.clone()) else {
            return Err(modes::NOT_A_SUBPATCH.to_string());
        };
        let from = self.modes_at(&path).map(|m| m.mode).unwrap_or_default();
        if from == mode {
            return Ok(());
        }
        if from.has_members() && !mode.has_members() {
            self.lift_member(graph, &path).map_err(|e| e.to_string())?;
        }
        if !from.has_members() && mode.has_members() {
            self.sink_into_member(graph, &path).map_err(|e| e.to_string())?;
        }
        // `member` is the Member control's name on a select's surface: an
        // item already called that takes the next free name, "member 2".
        if mode == Mode::Select {
            self.free_member_name(&path);
        }
        let settings = &mut self
            .definitions
            .get_mut(&key)
            .expect("an instance has a definition")
            .modes;
        settings.mode = mode;
        if mode != Mode::Stack {
            settings.copies = 0;
            settings.spread.clear();
            settings.stereo_spread = false;
        }
        Ok(())
    }

    /// Renames an exposed item called `member` on the surface of the instance
    /// at `path` to the next free name, with the outer surface's exposures
    /// of it and the instance's stored value, so the Member control can take
    /// the name. Assignments and mappings follow by themselves, since the
    /// file writes them by the name the surface has.
    fn free_member_name(&mut self, path: &ModulePath) {
        let key = self.instances[path].key.clone();
        let definition = &self.definitions[&key];
        if !definition
            .exposed
            .iter()
            .any(|e| e.name == MEMBER_PARAM && !Self::is_member_entry(e))
        {
            return;
        }
        let taken: BTreeSet<String> = definition.exposed.iter().map(|e| e.name.clone()).collect();
        let name = unique_name(MEMBER_PARAM, |n| taken.contains(n));
        for exposed in &mut self.definitions.get_mut(&key).expect("checked above").exposed {
            if exposed.name == MEMBER_PARAM && !Self::is_member_entry(exposed) {
                exposed.name = name.clone();
            }
        }
        let Some(parent) = path.parent() else { return };
        let Some(parent_key) = self.instances.get(&parent).map(|i| i.key.clone()) else {
            return;
        };
        let outer = self
            .definitions
            .get_mut(&parent_key)
            .expect("an instance has a definition");
        for exposed in &mut outer.exposed {
            if exposed.module == path.last() && exposed.param == MEMBER_PARAM {
                exposed.param = name.clone();
            }
        }
        if let Some(stored) = outer.defaults.remove(&(path.last(), MEMBER_PARAM.to_string())) {
            outer.defaults.insert((path.last(), name), stored);
        }
    }

    /// Moves the inside of the instance at `path` into a new member
    /// instance, member 1: every tile keeps its flat id, the ports and
    /// exposed items move with it, and the definition exposes each of them
    /// again through the member under its own name.
    fn sink_into_member(&mut self, graph: &Graph, path: &ModulePath) -> Result<(), PatchError> {
        let key = self.instances[path].key.clone();
        let outer = self
            .definitions
            .get(&key)
            .expect("an instance has a definition")
            .clone();
        // The surface's values, read before anything moves.
        let values: Vec<(String, Option<f32>)> = outer
            .exposed
            .iter()
            .map(|e| (e.name.clone(), self.exposed_value(graph, path, e)))
            .collect();
        let member_lid = self.next_level_id(path);
        let member_path = path.child(member_lid);
        let children: Vec<ModulePath> = self
            .ids
            .paths()
            .filter(|(p, _)| p.parent().as_ref() == Some(path) && !p.is_implied())
            .map(|(p, _)| p.clone())
            .collect();
        for child in children {
            self.rename_prefix(&child, &member_path.child(child.last()))?;
        }
        for link in std::mem::take(&mut self.links) {
            let mut link = link;
            if link.level == *path {
                link.level = member_path.clone();
            }
            self.links.insert(link);
        }
        let member_key = unique_name(&format!("{key} member"), |k| self.definitions.contains_key(k));
        self.ids.id_of(&member_path)?;
        self.instances.insert(
            member_path,
            Instance {
                key: member_key.clone(),
                label: None,
            },
        );
        self.definitions.insert(
            member_key,
            Definition {
                metadata: outer.metadata.clone(),
                exposed: outer.exposed.clone(),
                defaults: outer.defaults.clone(),
                library: None,
                roles: Vec::new(),
                modes: ModeSettings::default(),
                stored: None,
            },
        );
        let definition = self.definitions.get_mut(&key).expect("an instance has a definition");
        definition.exposed = outer
            .exposed
            .iter()
            .map(|e| Exposed {
                name: e.name.clone(),
                module: member_lid,
                param: e.name.clone(),
                range: None,
            })
            .collect();
        definition.defaults = values
            .into_iter()
            .map(|(name, value)| ((member_lid, name), Stored { value, range: None }))
            .collect();
        definition.modes.members = vec![member_lid];
        Ok(())
    }

    /// The reverse of [`Hierarchy::sink_into_member`]: the one member's
    /// inside moves up into the instance at `path`, keeping flat ids, and
    /// each exposed item through the member becomes the member's own.
    fn lift_member(&mut self, graph: &Graph, path: &ModulePath) -> Result<(), PatchError> {
        let key = self.instances[path].key.clone();
        let Some(&member_lid) = self.definitions[&key].modes.members.first() else {
            return Ok(());
        };
        let member_path = path.child(member_lid);
        let Some(member) = self.instances.remove(&member_path) else {
            return Ok(());
        };
        let inner = self.definitions.remove(&member.key).expect("a member has a definition");
        let outer = self.definitions[&key].clone();
        // The member's implied modules go; the next sync makes them again
        // where they belong.
        let implied: Vec<ModulePath> = self
            .ids
            .paths()
            .filter(|(p, _)| p.starts_with(&member_path) && p.is_implied())
            .map(|(p, _)| p.clone())
            .collect();
        for gone in implied {
            self.ids.forget(&gone);
        }
        self.ids.forget(&member_path);
        let children: Vec<ModulePath> = self
            .ids
            .paths()
            .filter(|(p, _)| p.parent().as_ref() == Some(&member_path))
            .map(|(p, _)| p.clone())
            .collect();
        for child in children {
            self.rename_prefix(&child, &path.child(child.last()))?;
        }
        for link in std::mem::take(&mut self.links) {
            let mut link = link;
            if link.level == member_path {
                link.level = path.clone();
            }
            self.links.insert(link);
        }
        let _ = graph;
        let definition = self.definitions.get_mut(&key).expect("an instance has a definition");
        definition.exposed = outer
            .exposed
            .iter()
            .filter_map(|e| {
                let through = inner.exposed.iter().find(|i| i.name == e.param)?;
                Some(Exposed {
                    name: e.name.clone(),
                    module: through.module,
                    param: through.param.clone(),
                    range: through.range.or(e.range),
                })
            })
            .collect();
        definition.defaults = inner.defaults;
        definition.modes.members.clear();
        Ok(())
    }

    /// Sets the copy count of the stack tile `instance`, 1 to 16 (sub-patches.md,
    /// The editor for modes). A sub-patch that holds a slot may be a stack:
    /// a swap fades in every copy (ADR-0044).
    pub fn set_copies(&mut self, instance: ModuleId, copies: u32) -> Result<(), String> {
        let path = self.ids.path_of(instance).cloned().ok_or_else(String::new)?;
        self.definition_at(&path).ok_or_else(String::new)?;
        let copies = copies.clamp(1, MOST_MEMBERS as u32);
        let key = self.instances[&path].key.clone();
        let settings = &mut self.definitions.get_mut(&key).expect("checked above").modes;
        settings.copies = if copies <= 1 { 0 } else { copies };
        Ok(())
    }

    /// Sets the spread of the exposed item `name` of the stack tile
    /// `instance`, 0 to 1; 0 leaves it unspread.
    pub fn set_spread(&mut self, instance: ModuleId, name: &str, amount: f32) {
        let Some(key) = self
            .ids
            .path_of(instance)
            .and_then(|p| self.instances.get(p))
            .map(|i| i.key.clone())
        else {
            return;
        };
        let Some(definition) = self.definitions.get_mut(&key) else {
            return;
        };
        if !definition.exposed.iter().any(|e| e.name == name) {
            return;
        }
        let amount = if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if amount == 0.0 {
            definition.modes.spread.remove(name);
        } else {
            definition.modes.spread.insert(name.to_string(), amount);
        }
    }

    /// Turns stereo spread on or off for the stack tile `instance`.
    pub fn set_stereo_spread(&mut self, instance: ModuleId, on: bool) {
        let Some(key) = self
            .ids
            .path_of(instance)
            .and_then(|p| self.instances.get(p))
            .map(|i| i.key.clone())
        else {
            return;
        };
        if let Some(definition) = self.definitions.get_mut(&key) {
            definition.modes.stereo_spread = on;
        }
    }

    /// The member tiles of the layer or select tile `instance`, in order.
    pub fn members_of(&self, instance: ModuleId) -> Vec<ModuleId> {
        let Some(path) = self.ids.path_of(instance) else {
            return Vec::new();
        };
        self.modes_at(path)
            .map(|m| {
                m.members
                    .iter()
                    .filter_map(|lid| self.ids.get(&path.child(*lid)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Moves member `member` of its layer or select one place earlier
    /// (`up`) or later. Returns false when it cannot move.
    pub fn move_member(&mut self, member: ModuleId, up: bool) -> bool {
        let Some(path) = self.ids.path_of(member).cloned() else {
            return false;
        };
        let Some(level) = path.parent() else { return false };
        let Some(key) = self.instances.get(&level).map(|i| i.key.clone()) else {
            return false;
        };
        let members = &mut self
            .definitions
            .get_mut(&key)
            .expect("an instance has a definition")
            .modes
            .members;
        let Some(at) = members.iter().position(|lid| *lid == path.last()) else {
            return false;
        };
        let to = if up {
            at.checked_sub(1)
        } else {
            Some(at + 1).filter(|to| *to < members.len())
        };
        match to {
            Some(to) => {
                members.swap(at, to);
                true
            }
            None => false,
        }
    }

    /// Why the tile `id` cannot be removed, or `None`: the last member of a
    /// layer or select stays (sub-patches.md, The editor for modes).
    pub fn remove_refusal(&self, id: ModuleId) -> Option<String> {
        let path = self.ids.path_of(id)?;
        let level = path.parent()?;
        let mode = self.member_level(&level)?;
        let members = &self.modes_at(&level)?.members;
        (members.len() <= 1 && members.contains(&path.last())).then(|| modes::last_member(mode))
    }

    /// Why the tiles `ids` cannot be removed together, or `None`: a layer
    /// or select keeps at least one member, however many go at once.
    pub fn removal_refusal(&self, ids: &[ModuleId]) -> Option<String> {
        let mut gone: BTreeMap<ModulePath, usize> = BTreeMap::new();
        for id in ids {
            let Some(path) = self.ids.path_of(*id) else { continue };
            let Some(level) = path.parent() else { continue };
            if self.modes_at(&level).is_some_and(|m| m.members.contains(&path.last())) {
                *gone.entry(level).or_default() += 1;
            }
        }
        gone.into_iter().find_map(|(level, count)| {
            let mode = self.member_level(&level)?;
            (count >= self.modes_at(&level)?.members.len()).then(|| modes::last_member(mode))
        })
    }

    /// Every select instance's path, its Member module, the member tile it
    /// plays, and its Member value, read before an edit to its members.
    pub fn playing_members(&self, graph: &Graph) -> Vec<PlayingMember> {
        self.instances
            .keys()
            .filter(|path| self.modes_at(path).is_some_and(|m| m.mode == Mode::Select))
            .filter_map(|path| {
                let module = self.member_module(path)?;
                let value = self.member_value(graph, path)?;
                let members = &self.modes_at(path)?.members;
                let n = (value.round().max(1.0) as usize).min(members.len().max(1));
                let playing = members.get(n - 1).and_then(|lid| self.ids.get(&path.child(*lid)));
                Some(PlayingMember {
                    instance: path.clone(),
                    module,
                    playing,
                    value,
                })
            })
            .collect()
    }

    /// The Member value each select of `before` takes after an edit to its
    /// members, so the member that played still plays: its new place, or,
    /// when it is gone, the place it had, held to the members left
    /// (sub-patches.md, The editor for modes). Only those that change.
    pub fn members_kept_playing(&self, before: &[PlayingMember]) -> Vec<(ModuleId, f32)> {
        before
            .iter()
            .filter_map(|was| {
                let members = &self.modes_at(&was.instance)?.members;
                let ids: Vec<ModuleId> = members
                    .iter()
                    .filter_map(|lid| self.ids.get(&was.instance.child(*lid)))
                    .collect();
                let value = match was.playing.and_then(|p| ids.iter().position(|id| *id == p)) {
                    Some(at) => (at + 1) as f32,
                    None => was.value.round().clamp(1.0, ids.len().max(1) as f32),
                };
                (value != was.value).then_some((was.module, value))
            })
            .collect()
    }

    /// Why a tile other than a member cannot be added or moved into `level`,
    /// or `None` when `level` takes any tile.
    pub fn level_refusal(&self, level: &ModulePath) -> Option<String> {
        self.member_level(level).map(modes::only_members)
    }

    /// The value of the Member control of the select instance at `path`,
    /// from its Member module.
    pub fn member_value(&self, graph: &Graph, path: &ModulePath) -> Option<f32> {
        let node = graph.node(self.member_module(path)?)?;
        node.params.get(node.parameter_index(MEMBER_PARAM)?).copied()
    }

    /// The flat targets of the Member parameter, as an exposed item, for
    /// the instance at `instance`, when it is a select.
    pub(super) fn member_target(&self, instance: &ModulePath) -> Option<(ModuleId, String)> {
        (self.modes_at(instance)?.mode == Mode::Select)
            .then(|| self.member_module(instance))
            .flatten()
            .map(|id| (id, MEMBER_PARAM.to_string()))
    }
}

#[cfg(test)]
mod tests;
