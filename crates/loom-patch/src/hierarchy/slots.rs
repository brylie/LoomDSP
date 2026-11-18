//! Slots in the editor's hierarchy (`docs/design/sub-patches.md`, Slots
//! and "How a swap crossfades"): making a slot from a module in place,
//! swapping its part, finishing the crossfade, and dissolving it.
//!
//! A slot is a tile with a flat id of its own. Its current part is an
//! ordinary module of the same level, hidden behind the tile, and every
//! cable on the tile is a link, so the flattener routes it to the part by
//! the role's names and `sync` keeps the engine's cables in step. During a
//! swap the leaving part and one swap fade per role output stay in the
//! graph as **transient** modules: not written to the file, not in an
//! undo snapshot, and removed once every fade reports it is done.

use super::*;
use crate::slot::FADE_KIND;
use crate::{Held, PartKey};
use loom_core::{FillsRole, OutputRef, ParamRange, RoleDescriptor};
use loom_dsp::blend::Law;

/// The modules the editor hides, from [`Hierarchy::hidden`].
pub struct Hidden<'h> {
    hierarchy: &'h Hierarchy,
    /// The paths of every slot's part and every leaving part.
    parts: BTreeSet<ModulePath>,
}

impl Hidden<'_> {
    /// True for a module the editor hides: a stack copy or a module a mode
    /// adds, anything inside a slot (a swap fade), or a slot's part.
    pub fn contains(&self, id: ModuleId) -> bool {
        let Some(path) = self.hierarchy.ids.path_of(id) else {
            return false;
        };
        // Stack copies and the modules a mode adds are never tiles.
        if path.is_implied() {
            return true;
        }
        let Some(level) = path.parent() else { return false };
        self.hierarchy.slots.contains_key(&level) || self.parts.contains(path)
    }

    /// What tile `id` shows, as [`Hierarchy::tile`], with the hidden test
    /// taken from this set.
    pub fn tile(&self, graph: &Graph, id: ModuleId) -> Option<Tile> {
        if self.contains(id) {
            return None;
        }
        self.hierarchy.shown_tile(graph, id)
    }
}

/// The paths of every leaving part, from [`Hierarchy::leaving_paths`],
/// taken once for a pass such as writing the patch.
pub(super) struct LeavingPaths(BTreeSet<ModulePath>);

impl LeavingPaths {
    /// True for a leaving part or anything inside one.
    pub(super) fn covers(&self, path: &ModulePath) -> bool {
        !self.0.is_empty() && (1..=path.0.len()).any(|n| self.0.contains(&path.prefix(n)))
    }
}

/// What the tile menu's Make slot item is called: `Make Filter slot`.
pub fn make_slot_label(role: &RoleDescriptor) -> String {
    format!("Make {} slot", role.name)
}

/// The status bar text after an empty slot is added from the module
/// browser (sub-patches.md, Empty slots).
pub fn added_slot_text(name: &str) -> String {
    format!("Added {name} slot. Swap a part in from its menu or the inspector.")
}

/// The note an empty slot's tile draws and a screen reader reads: a thru
/// role says it passes its input through (sub-patches.md, Empty slots).
pub fn no_part_text(role: &RoleDescriptor) -> &'static str {
    if role.thru {
        "No part. Passes its input through. Swap to choose one."
    } else {
        "No part. Swap to choose one."
    }
}

/// The status bar text after Remove part, naming the parameters whose
/// assignments or mappings were removed, as a swap does
/// (sub-patches.md, Empty slots).
pub fn removed_part_text(slot_name: &str, part_name: &str, removed: &[String]) -> String {
    let done = format!("Removed {part_name} from {slot_name}.");
    let mut names: Vec<&str> = Vec::new();
    for label in removed {
        if !names.contains(&label.as_str()) {
            names.push(label);
        }
    }
    match removed.len() {
        0 => done,
        1 => format!("{done} 1 modulation on {} was removed.", removed[0]),
        n => format!("{done} {n} modulations on {} were removed.", names.join(", ")),
    }
}

/// The status bar text after Make slot, naming the cables on ports the
/// role does not have, which were removed.
pub fn made_slot_text(role: &RoleDescriptor, removed: &[String]) -> String {
    let made = format!("{} slot made.", role.name);
    match removed.len() {
        0 => made,
        1 => format!(
            "{made} The cable from {} was removed: a slot has only the {} role's ports.",
            removed[0], role.name
        ),
        _ => format!(
            "{made} The cables on {} were removed: a slot has only the {} role's ports.",
            removed.join(", "),
            role.name
        ),
    }
}

/// The status bar text after a swap, with the assignments and mappings on
/// parameters only the leaving part had, which were removed.
pub fn swapped_text(slot_name: &str, part_name: &str, removed: &[String]) -> String {
    let swapped = format!("Swapped {slot_name} to {part_name}.");
    let mut names: Vec<&str> = Vec::new();
    for label in removed {
        if !names.contains(&label.as_str()) {
            names.push(label);
        }
    }
    match removed.len() {
        0 => swapped,
        1 => format!("{swapped} 1 modulation on {} was removed.", removed[0]),
        n => format!("{swapped} {n} modulations on {} were removed.", names.join(", ")),
    }
}

/// Why an edit that would pass the engine's module limit `limit` is
/// refused, and what to do, in the same words for a swap, an undo, and a
/// redo (sub-patches.md, The module limit). Inside a stack, `stack` names
/// the stack whose Copies the player can lower. The caller puts its own
/// start before it, such as `Cannot swap Filter: `.
pub fn too_many_modules_text(limit: usize, stack: Option<&str>) -> String {
    match stack {
        Some(stack) => format!(
            "the patch would have more than {limit} modules. Delete modules or lower Copies on {stack} to make room."
        ),
        None => format!("the patch would have more than {limit} modules. Delete modules to make room."),
    }
}

/// The refusal when an undo or redo step changes a slot that no longer
/// exists, as after a dissolve (sub-patches.md, The module limit).
pub const SLOT_GONE_TEXT: &str = "the slot it changes is gone";

/// The refusal when Expose is asked for a parameter only the current part
/// has (sub-patches.md, Slots).
pub fn expose_role_only_text(role: &RoleDescriptor, param_label: &str, part_name: &str) -> String {
    format!("Expose a {} parameter: {param_label} is only on {part_name}", role.name)
}

/// A slot tile: its role, its part, the parts it has held, and a fade in
/// progress.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotState {
    /// The role.
    pub role: &'static RoleDescriptor,
    /// The level id of the current part, or `None` for an empty slot.
    pub part: Option<u32>,
    /// The current part's declaration of the role, for a module part this
    /// build knows. `None` for a sub-patch part, whose names are the
    /// role's, and for a placeholder.
    pub part_fills: Option<&'static FillsRole>,
    /// Every other part the slot has held, by what it is.
    pub held: BTreeMap<PartKey, Held>,
    /// The tile's own label, or `None` for the role's name.
    pub label: Option<String>,
    /// The crossfade in progress, if any. Never saved.
    pub fading: Option<Fading>,
    /// The role values an empty slot keeps from the part that left it, by
    /// role name, which the next part takes (sub-patches.md, Empty slots).
    /// Empty while the slot holds a part.
    pub values: BTreeMap<String, f32>,
}

impl SlotState {
    /// The part's own name for the role parameter `name`.
    pub fn own_param(&self, name: &str) -> String {
        self.part_fills.map_or_else(|| name.to_string(), |f| f.own_param(name))
    }

    /// The part's own name for the role input `name`.
    pub fn own_input(&self, name: &str) -> String {
        self.part_fills.map_or_else(|| name.to_string(), |f| f.own_input(name))
    }

    /// The part's own name for the role output `name`.
    pub fn own_output(&self, name: &str) -> String {
        self.part_fills.map_or_else(|| name.to_string(), |f| f.own_output(name))
    }

    /// The role parameter the part's own parameter `own` stands for.
    pub fn role_param_of(&self, own: &str) -> Option<&'static str> {
        match self.part_fills {
            Some(fills) => fills.role_param_of(own).map(|p| p.name),
            None => self.role.param(own).map(|p| p.name),
        }
    }

    /// The role output the part's own output `own` stands for.
    pub fn role_output_of(&self, own: &str) -> Option<&'static str> {
        match self.part_fills {
            Some(fills) => fills.role_output_of(own).map(|p| p.name),
            None => self.role.output(own).map(|p| p.name),
        }
    }

    /// The role input the part's own input `own` stands for.
    pub fn role_input_of(&self, own: &str) -> Option<&'static str> {
        match self.part_fills {
            Some(fills) => fills.role_input_of(own).map(|p| p.name),
            None => self.role.input(own).map(|p| p.name),
        }
    }

    /// The name the tile shows.
    pub fn name(&self) -> String {
        self.label.clone().unwrap_or_else(|| self.role.name.to_string())
    }
}

/// A crossfade in progress on a slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Fading {
    /// The level id of the part that is leaving, or `None` when a part
    /// fades into an empty thru slot from the thru signal.
    pub leaving: Option<u32>,
    /// One swap fade per role output that both parts have: the role
    /// output's name and the fade's flat id.
    pub fades: Vec<(String, ModuleId)>,
    /// The same fade in every other copy of the stacks the slot is in
    /// (ADR-0044). Empty outside a stack.
    pub copies: Vec<CopyFade>,
    /// Inside a stack, out of an empty thru slot only: copy 0's thru
    /// source, read from the graph before the swap, as each [`CopyFade`]
    /// holds its own (ADR-0046). Outside a stack copy 0 finds its ends from
    /// the slot's links at every sync, and this is `None`.
    pub thru_source: Option<End>,
    /// Inside a stack, out of an empty thru slot only: the inputs copy 0's
    /// leaving part fed, read before the swap. The slot's links cannot name
    /// copy 0's own input on the stack's Sum, any more than another copy's.
    pub thru_dests: Vec<End>,
}

/// A swap's fade in one stack copy past the first (ADR-0044): the leaving
/// part there keeps running, and its fades sit under the copy's slot path.
/// All of these are transient, named only here, and never saved.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyFade {
    /// The slot's level in this copy: copy 0's level with this copy's
    /// steps.
    pub level: ModulePath,
    /// One fade per faded role output, as copy 0's: the role output's name
    /// and the fade's flat id.
    pub fades: Vec<(String, ModuleId)>,
    /// Out of an empty thru slot only: the output that fed the leaving
    /// part's `in` in this copy, read from the graph before the swap, or
    /// `None` when nothing fed it (ADR-0046). The fade's `b` takes it.
    pub thru_source: Option<End>,
    /// Out of an empty thru slot only: every input the leaving part's role
    /// output fed in this copy, read before the swap. While the fade runs,
    /// its `out` takes over each one whose thru cable is still there, so a
    /// cable the player moved is never put back (ADR-0046).
    pub thru_dests: Vec<End>,
}

/// One copy's thru ends out of an empty thru slot: the output that fed the
/// leaving part's `in`, if any, and the inputs its role output fed
/// ([`CopyFade::thru_source`] and [`CopyFade::thru_dests`]).
type ThruEnds = (Option<End>, Vec<End>);

/// What Make slot did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeSlot {
    /// The new slot tile.
    pub slot: ModuleId,
    /// The labels of the part's ports outside the role whose cables were
    /// removed.
    pub removed: Vec<String>,
}

/// What a swap did. The caller syncs and commits; on a later refusal it
/// removes `added`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swapped {
    /// The new part's flat id.
    pub part: ModuleId,
    /// The display name of the new part.
    pub part_name: String,
    /// Every flat module added: the part, what a sub-patch part brought,
    /// and the fades.
    pub added: Vec<ModuleId>,
    /// The labels of the leaving part's parameters outside the role whose
    /// assignments or mappings were removed.
    pub removed: Vec<String>,
}

/// What Remove part did. The caller syncs and commits; it leaves `gone`
/// out of its sync and removes those modules after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emptied {
    /// The display name of the part that left.
    pub part_name: String,
    /// The labels of the parameters and outputs whose assignments or
    /// mappings were removed.
    pub removed: Vec<String>,
    /// Modules that go now, with no fade.
    pub gone: Vec<ModuleId>,
}

/// A sub-patch a slot holds but does not play: its definition and the
/// definitions nested in it, as a file keeps them.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredPart {
    /// The definition.
    pub definition: SubPatch,
    /// The definitions nested in it, by key.
    pub nested: BTreeMap<String, SubPatch>,
}

/// What `sync` adds to the flat cables while a slot fades: the fade
/// wiring, in flat ids and port names.
#[derive(Debug, Default)]
pub(super) struct FadeWiring {
    /// Cables to add.
    pub add: Vec<(End, End)>,
    /// The new part's outputs whose cables move onto a fade's `out`, as
    /// `(part output, fade output)`.
    pub reroute: Vec<(End, End)>,
    /// Inputs a fade's `out` feeds in place of whatever the flat graph
    /// feeds them, as `(input, fade output)`: a part fading out to an
    /// empty thru slot's signal, by destination.
    pub retarget: Vec<(End, End)>,
    /// Inputs a fade's `out` takes over only while their thru cable is
    /// still wanted, as `(thru source, input, fade output)`: each stack
    /// copy's part, copy 0's too, fading out to its own thru signal
    /// (ADR-0046). With no
    /// thru source, it takes over an input whose module is still there
    /// and that nothing else feeds.
    pub replace: Vec<(Option<End>, End, End)>,
}

impl Hierarchy {
    /// The slot tile `id`, if it is one.
    pub fn slot(&self, id: ModuleId) -> Option<&SlotState> {
        self.slots.get(self.ids.path_of(id)?)
    }

    /// The name of the innermost stack of more than one copy that holds
    /// the slot tile `slot`, as its tile shows it: the stack whose Copies a
    /// refusal at the module limit names. `None` outside a stack.
    pub fn stack_holding(&self, slot: ModuleId) -> Option<String> {
        let level = self.ids.path_of(slot)?.parent()?;
        (1..=level.0.len()).rev().find_map(|n| {
            let definition = self.definition_at(&level.prefix(n))?;
            let stacked = definition.modes.mode == crate::modes::Mode::Stack && definition.modes.copy_count() > 1;
            stacked.then(|| definition.metadata.name.clone())
        })
    }

    /// True when the sub-patch tile `instance` holds a slot directly.
    pub fn holds_slot(&self, instance: ModuleId) -> bool {
        let Some(path) = self.ids.path_of(instance) else {
            return false;
        };
        self.slots.keys().any(|p| p.parent().as_ref() == Some(path))
    }

    /// The slot at `level` whose part is the member `lid`.
    pub(super) fn slot_owning(&self, level: &ModulePath, lid: u32) -> Option<(&ModulePath, &SlotState)> {
        self.slots
            .iter()
            .find(|(path, state)| path.parent().as_ref() == Some(level) && state.part == Some(lid))
    }

    /// The name of a definition a slot holds but does not play.
    pub fn held_definition_name(&self, key: &str) -> Option<String> {
        let definition = self.definitions.get(key)?;
        definition.stored.as_ref()?;
        Some(definition.metadata.name.clone())
    }

    /// The slot tile whose current part is the flat module `part`.
    pub fn slot_of_part(&self, part: ModuleId) -> Option<ModuleId> {
        let path = self.ids.path_of(part)?;
        let level = path.parent()?;
        let (slot_path, _) = self.slot_owning(&level, path.last())?;
        self.ids.get(slot_path)
    }

    /// The flat id of the slot tile `slot`'s current part.
    pub fn part_of_slot(&self, slot: ModuleId) -> Option<ModuleId> {
        let path = self.ids.path_of(slot)?;
        let part = self.slots.get(path)?.part?;
        self.ids.get(&path.parent()?.child(part))
    }

    /// True for a module the editor hides: a slot's current part, a part
    /// that is leaving, or a swap fade. For many ids, take [`Hierarchy::hidden`]
    /// once instead: this scans every slot.
    pub fn is_hidden(&self, id: ModuleId) -> bool {
        let Some(path) = self.ids.path_of(id) else { return false };
        // Stack copies and the modules a mode adds are never tiles.
        if path.is_implied() {
            return true;
        }
        let Some(level) = path.parent() else { return false };
        if self.slots.contains_key(&level) {
            return true;
        }
        // One scan, with no allocation; a pass over many ids takes
        // [`Hierarchy::hidden`] once instead.
        self.slots.iter().any(|(p, s)| {
            let leaving = s.fading.as_ref().and_then(|f| f.leaving);
            p.parent().as_ref() == Some(&level) && (s.part == Some(path.last()) || leaving == Some(path.last()))
        })
    }

    /// The modules the editor hides, worked out in one scan of the slots,
    /// for a pass over many ids such as one scene build. Take it again
    /// after a swap.
    pub fn hidden(&self) -> Hidden<'_> {
        let mut parts = BTreeSet::new();
        for (path, state) in &self.slots {
            let level = path.parent().unwrap_or_else(ModulePath::root);
            let leaving = state.fading.as_ref().and_then(|f| f.leaving);
            for lid in state.part.into_iter().chain(leaving) {
                parts.insert(level.child(lid));
            }
        }
        Hidden { hierarchy: self, parts }
    }

    /// The modules that exist only while a slot fades: every leaving part
    /// and every swap fade, and the implied modules of modes. Left out of
    /// the file and of undo snapshots.
    pub fn transient_ids(&self) -> BTreeSet<ModuleId> {
        // Implied modules are not written either: the flattener makes them
        // again from copy 0 (ADR-0040). They hold every copy's part of a
        // fade too.
        let mut ids = self.fading_ids();
        ids.extend(self.implied_ids());
        ids
    }

    /// Takes the fades in progress from `live` onto this hierarchy, slot by
    /// slot: a restored snapshot carries no fade, and the swap that undo
    /// made on the live hierarchy must keep fading.
    pub fn carry_fading_from(&mut self, live: &Hierarchy) {
        for (path, state) in &mut self.slots {
            state.fading = live.slots.get(path).and_then(|s| s.fading.clone());
            let Some(fading) = &state.fading else { continue };
            // The leaving part and the fades keep their paths and flat ids,
            // which a snapshot taken before them does not have.
            let level = path.parent().unwrap_or_else(ModulePath::root);
            let mut leaving: Vec<ModulePath> = Vec::new();
            let mut slot_paths = vec![path.clone()];
            for copy in &fading.copies {
                slot_paths.push(copy.level.child(path.last()));
            }
            if let Some(lid) = fading.leaving {
                leaving.push(level.child(lid));
                leaving.extend(fading.copies.iter().map(|c| c.level.child(lid)));
            }
            for (p, id) in live.ids.paths() {
                if leaving.iter().any(|l| p.starts_with(l))
                    || p.parent().is_some_and(|parent| slot_paths.contains(&parent))
                {
                    let _ = self.ids.reserve(p.clone(), id);
                }
            }
        }
    }

    /// The slot tiles whose current part differs between `self` and
    /// `other`, by flat id: what an undo step must swap rather than add
    /// and remove.
    pub fn slots_differing_from(&self, other: &Hierarchy) -> Vec<ModuleId> {
        self.slots
            .keys()
            .filter_map(|path| self.ids.get(path))
            .filter(|&id| other.slot(id).is_none() || self.part_of_slot(id) != other.part_of_slot(id))
            .collect()
    }

    /// The slot tiles with a fade in progress, each with its fades' flat
    /// ids. The app reads each fade's `done` and finishes the slot when
    /// all of them read 1.
    pub fn fading_slots(&self) -> Vec<(ModuleId, Vec<ModuleId>)> {
        self.slots
            .iter()
            .filter_map(|(path, state)| {
                let fading = state.fading.as_ref()?;
                Some((self.ids.get(path)?, fading.fades.iter().map(|(_, id)| *id).collect()))
            })
            .collect()
    }

    /// True while any slot's swap fade runs: a cheap check, with no list.
    pub fn is_fading(&self) -> bool {
        self.slots.values().any(|state| state.fading.is_some())
    }

    /// The end at `level` a file writes for a parameter: through the slot,
    /// by the role's name, when `lid` is a slot's part and `name` one of
    /// its role parameters; else itself.
    pub(super) fn slot_end_for_param(&self, level: &ModulePath, lid: u32, name: &str) -> End {
        if let Some((slot_path, state)) = self.slot_owning(level, lid)
            && let Some(role_name) = state.role_param_of(name)
        {
            return (slot_path.last(), role_name.to_string());
        }
        (lid, name.to_string())
    }

    /// The end at `level` a file writes for an output: through the slot
    /// when `lid` is a slot's part and `name` a role output.
    pub(super) fn slot_end_for_output(&self, level: &ModulePath, lid: u32, name: &str) -> End {
        if let Some((slot_path, state)) = self.slot_owning(level, lid)
            && let Some(role_name) = state.role_output_of(name)
        {
            return (slot_path.last(), role_name.to_string());
        }
        (lid, name.to_string())
    }

    /// The flat parameter behind the role parameter `param` of the slot at
    /// `slot_path`, or `None` for an empty slot or a part without the
    /// optional parameter.
    pub(super) fn slot_param_target(&self, slot_path: &ModulePath, param: &str) -> Option<(ModuleId, String)> {
        let state = self.slots.get(slot_path)?;
        state.role.param(param)?;
        let level = slot_path.parent()?;
        let part_path = level.child(state.part?);
        if let Some(definition) = self.definition_at(&part_path) {
            let inner = definition.exposed.iter().find(|e| e.name == param)?;
            return self.resolve_exposed(&part_path, inner);
        }
        Some((self.ids.get(&part_path)?, state.own_param(param)))
    }

    /// The live module and parameter reached through a slot's role name.
    pub fn slot_parameter(&self, slot: ModuleId, param: &str) -> Option<(ModuleId, String)> {
        self.slot_param_target(self.ids.path_of(slot)?, param)
    }

    /// The flat output behind the role output `name` of the part `lid` at
    /// `level`.
    pub(in crate::hierarchy::slots) fn part_output(
        &self,
        graph: &Graph,
        level: &ModulePath,
        lid: u32,
        name: &str,
    ) -> Option<(ModuleId, String)> {
        let path = level.child(lid);
        if self.instances.contains_key(&path) {
            return self.source_behind(graph, level, &(lid, name.to_string()));
        }
        let (_, state) = self.slot_owning(level, lid).or_else(|| self.slot_leaving(level, lid))?;
        let own = state.own_output_for(lid, name, self, graph, level);
        let id = self.ids.get(&path)?;
        let node = graph.node(id)?;
        node.output_index(&own)?;
        Some((id, own))
    }

    /// The flat inputs behind the role input `name` of the part `lid`.
    pub(in crate::hierarchy::slots) fn part_inputs(
        &self,
        graph: &Graph,
        level: &ModulePath,
        lid: u32,
        name: &str,
    ) -> Vec<(ModuleId, String)> {
        let path = level.child(lid);
        if self.instances.contains_key(&path) {
            return self.inputs_behind(graph, level, &(lid, name.to_string()));
        }
        let Some((_, state)) = self.slot_owning(level, lid).or_else(|| self.slot_leaving(level, lid)) else {
            return Vec::new();
        };
        let own = state.own_input_for(lid, name, self, graph, level);
        let Some(id) = self.ids.get(&path) else {
            return Vec::new();
        };
        match graph.node(id).and_then(|n| n.input_index(&own)) {
            Some(_) => vec![(id, own)],
            None => Vec::new(),
        }
    }

    /// The paths of the parts leaving a slot: what exists only while the
    /// slot fades, with everything inside it.
    pub(super) fn leaving_paths(&self) -> LeavingPaths {
        LeavingPaths(
            self.slots
                .iter()
                .filter_map(|(path, state)| {
                    let lid = state.fading.as_ref()?.leaving?;
                    Some(path.parent().unwrap_or_else(ModulePath::root).child(lid))
                })
                .collect(),
        )
    }

    /// The slot at `level` whose leaving part is `lid`.
    fn slot_leaving(&self, level: &ModulePath, lid: u32) -> Option<(&ModulePath, &SlotState)> {
        self.slots.iter().find(|(path, state)| {
            path.parent().as_ref() == Some(level) && state.fading.as_ref().is_some_and(|f| f.leaving == Some(lid))
        })
    }
}

impl SlotState {
    /// The own output name of the part `lid`, whether it is the current
    /// part or the leaving one: the current part's names come from this
    /// state, the leaving part's from its descriptor.
    fn own_output_for(&self, lid: u32, name: &str, hierarchy: &Hierarchy, graph: &Graph, level: &ModulePath) -> String {
        if self.part == Some(lid) {
            return self.own_output(name);
        }
        leaving_fills(hierarchy, graph, level, lid, self.role).map_or_else(|| name.to_string(), |f| f.own_output(name))
    }

    /// The own input name of the part `lid`, as [`SlotState::own_output_for`].
    fn own_input_for(&self, lid: u32, name: &str, hierarchy: &Hierarchy, graph: &Graph, level: &ModulePath) -> String {
        if self.part == Some(lid) {
            return self.own_input(name);
        }
        leaving_fills(hierarchy, graph, level, lid, self.role).map_or_else(|| name.to_string(), |f| f.own_input(name))
    }
}

/// The swap fade's `out` port index.
const FADE_OUT: usize = 0;

/// The law a swap fades a role output with, or `None` for one it does not
/// fade (sub-patches.md, How a swap crossfades, point 6). Audio fades with
/// equal power, since two parts are different sounds. Pitch and modulation
/// fade linearly, so a value both parts share stays put. A gate or phase
/// output switches to the new part on the swap frame, for the reason
/// live-patching rule 9 gives: a ramped gate fires late and a ramped phase
/// looks like wraps. A trigger is a one-sample gate.
pub fn fade_law(output: &loom_core::RolePort) -> Option<Law> {
    match output.signal {
        loom_core::SignalType::Gate | loom_core::SignalType::Phase => None,
        loom_core::SignalType::Pitch | loom_core::SignalType::Modulation => Some(Law::Linear),
        // Every type is named, so a new one fails the build until its law
        // is chosen.
        loom_core::SignalType::Audio => Some(Law::EqualPower),
    }
}

/// True for a role output a swap fades: one [`fade_law`] gives a law for.
pub fn fades(output: &loom_core::RolePort) -> bool {
    fade_law(output).is_some()
}

/// The swap fade's `law` parameter value for `law`: 0 is Equal power, 1 is
/// Linear (`loom.struct.fade` in `loom-modules`).
pub fn fade_law_value(law: Law) -> f32 {
    match law {
        Law::EqualPower => 0.0,
        Law::Linear => 1.0,
    }
}

/// The leaving module part's declaration of the role, from its descriptor.
fn leaving_fills(
    hierarchy: &Hierarchy,
    graph: &Graph,
    level: &ModulePath,
    lid: u32,
    role: &RoleDescriptor,
) -> Option<&'static FillsRole> {
    let id = hierarchy.ids.get(&level.child(lid))?;
    graph.node(id)?.descriptor.fills(role.id)
}

/// The label of the port `own` of module `module`: an output when
/// `is_output`.
fn port_label(graph: &Graph, module: ModuleId, own: &str, is_output: bool) -> String {
    let Some(node) = graph.node(module) else {
        return own.to_string();
    };
    let ports = if is_output {
        node.descriptor.outputs
    } else {
        node.descriptor.inputs
    };
    ports
        .iter()
        .find(|p| p.name == own)
        .map_or_else(|| own.to_string(), |p| p.label.to_string())
}

/// The label of the parameter `param` of module `module`.
fn param_label(graph: &Graph, module: ModuleId, param: &str) -> String {
    graph
        .node(module)
        .and_then(|n| n.descriptor.params.iter().find(|p| p.name == param))
        .map_or_else(|| param.to_string(), |p| p.label.to_string())
}

/// The library path a definition follows.
fn instance_library<'a>(hierarchy: &'a Hierarchy, key: &str) -> Option<&'a str> {
    hierarchy.definitions.get(key)?.library.as_deref()
}

#[cfg(test)]
mod tests;

mod fade;
mod rewire;
mod swap;

mod edit;
