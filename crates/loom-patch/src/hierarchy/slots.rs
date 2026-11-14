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

/// The status bar text when Make sub-patch is asked for a selection that
/// holds a slot. Moving a slot into a sub-patch is a follow-up.
pub const NO_SLOT_IN_COLLAPSE: &str = "Cannot make a sub-patch from a slot yet; dissolve the slot first";

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
}

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
}

impl Hierarchy {
    /// The slot tile `id`, if it is one.
    pub fn slot(&self, id: ModuleId) -> Option<&SlotState> {
        self.slots.get(self.ids.path_of(id)?)
    }

    /// True when the sub-patch tile `instance` holds a slot directly, which
    /// Dissolve sub-patch refuses until slots move with their part (#702).
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
            .iter()
            .filter(|(path, state)| other.slots.get(*path).is_none_or(|o| o.part != state.part))
            .filter_map(|(path, _)| self.ids.get(path))
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

    /// The flat output behind the role output `name` of the part `lid` at
    /// `level`.
    fn part_output(&self, graph: &Graph, level: &ModulePath, lid: u32, name: &str) -> Option<(ModuleId, String)> {
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
    fn part_inputs(&self, graph: &Graph, level: &ModulePath, lid: u32, name: &str) -> Vec<(ModuleId, String)> {
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

    /// What `sync` wires while slots fade (sub-patches.md, How a swap
    /// crossfades): each slot input to the leaving part too, each part's
    /// role output to its fade, and what the slot's output fed onto the
    /// fade's `out`.
    pub(super) fn fade_wiring(&self, graph: &Graph) -> FadeWiring {
        let mut wiring = FadeWiring::default();
        for (slot_path, state) in &self.slots {
            let Some(fading) = &state.fading else { continue };
            let own = self.slot_fade_wiring(graph, slot_path, state, fading);
            // Every other stack copy is wired as copy 0 is, each end moved
            // into that copy by path; an end outside the stack feeds every
            // copy and stays (ADR-0044).
            for copy in &fading.copies {
                let fade_k = |fade: u32| -> Option<u32> {
                    let (role_out, _) = fading.fades.iter().find(|(_, f)| f.0 == fade)?;
                    copy.fades.iter().find(|(r, _)| r == role_out).map(|(_, f)| f.0)
                };
                let end_k = |end: &End| -> Option<End> {
                    let id = fade_k(end.0)
                        .map(ModuleId)
                        .or_else(|| self.in_copy(ModuleId(end.0), &copy.level))?;
                    Some((id.0, end.1.clone()))
                };
                for (from, to) in &own.add {
                    if let (Some(from), Some(to)) = (end_k(from), end_k(to)) {
                        wiring.add.push((from, to));
                    }
                }
                for (from, to) in &own.reroute {
                    if let (Some(from), Some(to)) = (end_k(from), end_k(to)) {
                        wiring.reroute.push((from, to));
                    }
                }
            }
            wiring.add.extend(own.add);
            wiring.reroute.extend(own.reroute);
            wiring.retarget.extend(own.retarget);
        }
        wiring
    }

    /// The fade wiring of one slot in copy 0.
    fn slot_fade_wiring(
        &self,
        graph: &Graph,
        slot_path: &ModulePath,
        state: &SlotState,
        fading: &Fading,
    ) -> FadeWiring {
        let mut wiring = FadeWiring::default();
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let slot_lid = slot_path.last();
        // What feeds the slot's role input `name`, at the flat level.
        let fed = |name: &str| -> Option<(ModuleId, String)> {
            let link = self.link_into(&level, &(slot_lid, name.to_string()))?;
            self.source_behind(graph, &level, &link.from)
        };
        for (role_out, fade) in &fading.fades {
            // `a`: the leaving part, or the thru signal a part fades in
            // from (sub-patches.md, Empty slots).
            let a = match fading.leaving {
                Some(leaving) => self.part_output(graph, &level, leaving, role_out),
                None => fed(loom_core::THRU_IN),
            };
            if let Some((l, l_out)) = a {
                wiring.add.push(((l.0, l_out), (fade.0, "a".to_string())));
            }
            match state.part {
                Some(part) => {
                    if let Some((p, p_out)) = self.part_output(graph, &level, part, role_out) {
                        wiring.add.push(((p.0, p_out.clone()), (fade.0, "b".to_string())));
                        wiring.reroute.push(((p.0, p_out), (fade.0, "out".to_string())));
                    }
                }
                None => {
                    // A part fading out to the thru: `b` is the thru
                    // signal, and the fade takes over only the inputs
                    // the slot's output fed, by destination, so other
                    // inputs the thru source feeds are untouched. With
                    // nothing on `in`, the part fades to 0.
                    if let Some((x, x_out)) = fed(loom_core::THRU_IN) {
                        wiring.add.push(((x.0, x_out), (fade.0, "b".to_string())));
                    }
                    let dests: Vec<End> = self
                        .links_from(&level, &(slot_lid, role_out.clone()))
                        .map(|l| l.to)
                        .collect();
                    for dest in dests {
                        for (to, input) in self.inputs_behind(graph, &level, &dest) {
                            // A stack downstream runs this input in every
                            // copy, each fed from the same source, so the
                            // fade feeds them all (ADR-0044).
                            for copy in std::iter::once(to).chain(self.copies_of(to).iter().map(|(c, _)| *c)) {
                                wiring
                                    .retarget
                                    .push(((copy.0, input.clone()), (fade.0, "out".to_string())));
                            }
                        }
                    }
                }
            }
        }
        // The leaving part keeps its inputs while it fades.
        if let Some(leaving) = fading.leaving {
            for role_in in state.role.inputs {
                let Some(src) = fed(role_in.name) else { continue };
                for dest in self.part_inputs(graph, &level, leaving, role_in.name) {
                    wiring.add.push(((src.0.0, src.1.clone()), (dest.0.0, dest.1)));
                }
            }
        }
        wiring
    }

    /// The slot's level `level` in every stack copy past the first, at any
    /// nesting depth: copy 0's level with each copy's steps put in after
    /// its stack's instance (ADR-0040, Copies in the editor). Empty outside
    /// a stack of more than one copy.
    pub fn copy_levels(&self, level: &ModulePath) -> Vec<ModulePath> {
        let mut partial = vec![ModulePath(Vec::new())];
        for n in 1..=level.0.len() {
            let step = level.0[n - 1];
            for path in &mut partial {
                path.0.push(step);
            }
            let copies = self
                .modes_at(&level.prefix(n))
                .filter(|m| m.mode == crate::modes::Mode::Stack)
                .map_or(1, |m| m.copy_count());
            if copies > 1 {
                let more: Vec<ModulePath> = partial
                    .iter()
                    .flat_map(|path| (1..copies).map(move |k| path.copy(k)))
                    .collect();
                partial.extend(more);
            }
        }
        partial.retain(|path| path != level);
        partial
    }

    /// The flat module in the copy whose level is `level_k` that stands for
    /// copy 0's module `id`: its path with the copy's steps put in where
    /// both run inside the same stacks. A module outside those stacks, which
    /// feeds every copy, is itself.
    fn in_copy(&self, id: ModuleId, level_k: &ModulePath) -> Option<ModuleId> {
        let path = self.ids.path_of(id)?;
        let mut out = Vec::new();
        let mut i = 0;
        for &step in &level_k.0 {
            if step >= crate::flatten::COPY_STEP {
                // A copy step goes in only after a stack both paths share.
                if i > 0 {
                    out.push(step);
                }
                continue;
            }
            if path.0.get(i) == Some(&step) {
                out.push(step);
                i += 1;
            } else {
                break;
            }
        }
        out.extend_from_slice(&path.0[i..]);
        self.ids.get(&ModulePath(out))
    }

    /// Adds, in every other copy of the stacks the slot at `slot_path` is
    /// in, the same fades as `fades` (copy 0's), at the same steps under
    /// that copy's slot path and with the same laws (ADR-0044). A copy
    /// whose leaving part is missing is skipped. On an error part-way, the
    /// caller rolls back by comparing modules, as for [`Hierarchy::add_fades`].
    fn add_copy_fades(
        &mut self,
        controller: &mut Controller,
        slot_path: &ModulePath,
        leaving: Option<u32>,
        fades: &[(String, ModuleId)],
    ) -> Result<Vec<CopyFade>, String> {
        let Some(leaving) = leaving else {
            return Ok(Vec::new());
        };
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let mut made = Vec::new();
        for level_k in self.copy_levels(&level) {
            if self.ids.get(&level_k.child(leaving)).is_none()
                && !self.ids.paths().any(|(p, _)| p.starts_with(&level_k.child(leaving)))
            {
                continue;
            }
            let slot_k = level_k.child(slot_path.last());
            let mut copy_fades = Vec::new();
            for (role_out, fade) in fades {
                let step = self
                    .ids
                    .path_of(*fade)
                    .map(ModulePath::last)
                    .ok_or("a fade has no path")?;
                let law = controller
                    .graph()
                    .node(*fade)
                    // Copy 0's law, by name, so another parameter cannot change it.
                    .and_then(|n| Some(n.params[n.descriptor.param_index("law")?]))
                    .unwrap_or(0.0);
                let id = self.ids.id_of(&slot_k.child(step)).map_err(|e| e.to_string())?;
                controller
                    .add_module_with_id(id, FADE_KIND)
                    .map_err(|e| e.to_string())?;
                copy_fades.push((role_out.clone(), id));
                controller.set_param_named(id, "law", law).map_err(|e| e.to_string())?;
            }
            made.push(CopyFade {
                level: level_k,
                fades: copy_fades,
            });
        }
        Ok(made)
    }

    /// Every module that exists only while a slot fades, in every copy:
    /// each leaving part and everything under it, and each swap fade.
    /// `sync` leaves them, and the assignments onto them, alone
    /// (ADR-0044).
    pub fn fading_ids(&self) -> BTreeSet<ModuleId> {
        let mut ids = BTreeSet::new();
        for (path, state) in &self.slots {
            let Some(fading) = &state.fading else { continue };
            let level = path.parent().unwrap_or_else(ModulePath::root);
            let mut levels = vec![level];
            levels.extend(fading.copies.iter().map(|c| c.level.clone()));
            if let Some(lid) = fading.leaving {
                for level in &levels {
                    let leaving = level.child(lid);
                    ids.extend(
                        self.ids
                            .paths()
                            .filter(|(p, _)| p.starts_with(&leaving))
                            .map(|(_, id)| id),
                    );
                }
            }
            ids.extend(fading.fades.iter().map(|(_, id)| *id));
            ids.extend(fading.copies.iter().flat_map(|c| c.fades.iter().map(|(_, id)| *id)));
        }
        ids
    }

    /// Adds one swap fade per `(role output, law)` under the slot at
    /// `slot_path`, with its law set (sub-patches.md, How a swap
    /// crossfades). Returns them as `(role output, flat id)`. On an error
    /// part-way, the fades already added stay in the controller: the
    /// caller works on a candidate hierarchy and rolls back by comparing
    /// the graph's modules with what it held before (`swap_now`).
    fn add_fades(
        &mut self,
        controller: &mut Controller,
        slot_path: &ModulePath,
        outputs: Vec<(String, Law)>,
    ) -> Result<Vec<(String, ModuleId)>, String> {
        let mut made = Vec::new();
        for (role_out, law) in outputs {
            let k = self.next_level_id(slot_path);
            let fade_path = slot_path.child(k);
            let fade_id = self.ids.id_of(&fade_path).map_err(|e| e.to_string())?;
            controller
                .add_module_with_id(fade_id, FADE_KIND)
                .map_err(|e| e.to_string())?;
            made.push((role_out, fade_id));
            controller
                .set_param_named(fade_id, "law", fade_law_value(law))
                .map_err(|e| e.to_string())?;
        }
        Ok(made)
    }

    /// True when a swap into or out of the empty slot at `slot_path` fades
    /// from or to its thru signal: a thru role, outside a stack of more
    /// than one copy. Inside a stack the swap is a cable change in every
    /// copy, which live-patching rule 9 ramps over 5 ms, until each copy
    /// has its own thru wiring (#768; sub-patches.md, Empty slots).
    fn fades_with_thru(&self, slot_path: &ModulePath) -> bool {
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        self.slots.get(slot_path).is_some_and(|s| s.role.thru) && !self.in_stack(&level)
    }

    /// The thru output of the slot at `slot_path` that `part` (a level id)
    /// has, for a fade between the part and the thru signal: Linear, since
    /// the dry signal and the signal through the part are nearly the same.
    fn thru_fade_outputs(&self, graph: &Graph, slot_path: &ModulePath, part: u32) -> Vec<(String, Law)> {
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let out = loom_core::THRU_OUT;
        if self.part_output(graph, &level, part, out).is_some() {
            vec![(out.to_string(), Law::Linear)]
        } else {
            Vec::new()
        }
    }

    /// Adds an empty slot for `role` at `level`, labelled with the role's
    /// name made unique among the level's slots ("Filter 2"), as one tile
    /// with no part (sub-patches.md, Empty slots). Refused at a layer or
    /// select member level. Nothing is added to the graph: a slot is routed
    /// through, and an empty one has nothing behind it. The caller syncs
    /// and commits.
    pub fn add_empty_slot(&mut self, level: &ModulePath, role: &'static RoleDescriptor) -> Result<ModuleId, String> {
        if let Some(why) = self.level_refusal(level) {
            return Err(why);
        }
        let taken: BTreeSet<String> = self
            .slots
            .iter()
            .filter(|(path, _)| path.parent().as_ref() == Some(level))
            .map(|(_, state)| state.name())
            .collect();
        let label = crate::unique_name(role.name, |name| taken.contains(name));
        let (slot_path, slot_id) = self.new_module(level).map_err(|e| e.to_string())?;
        self.slots.insert(
            slot_path,
            SlotState {
                role,
                part: None,
                part_fills: None,
                held: BTreeMap::new(),
                label: (label != role.name).then_some(label),
                fading: None,
                values: BTreeMap::new(),
            },
        );
        Ok(slot_id)
    }

    /// Makes the module or sub-patch tile `member` the part of a new slot
    /// for `role`, in place (sub-patches.md, Slots: Make slot). Its cables
    /// on role ports move to the slot's ports; a cable on another port is
    /// removed, and its label returned, as are exposures of parameters
    /// outside the role. Refused for a tile that does not declare the
    /// role, or is already a slot or a part. The caller syncs and commits.
    pub fn make_slot(
        &mut self,
        graph: &Graph,
        member: ModuleId,
        role: &'static RoleDescriptor,
    ) -> Result<MadeSlot, String> {
        let path = self.path_of(member);
        let level = path.parent().unwrap_or_else(ModulePath::root);
        let lid = path.last();
        if self.slots.contains_key(&path) {
            return Err("it is a slot already".to_string());
        }
        if self.slot_owning(&level, lid).is_some() {
            return Err("it is the part of a slot already".to_string());
        }
        let part_fills = if let Some(definition) = self.definition_at(&path) {
            if !definition.roles.iter().any(|r| crate::role_id(r) == role.id) {
                return Err(format!("{} does not fill {}", definition.metadata.name, role.name));
            }
            None
        } else {
            let Some(node) = graph.node(member) else {
                return Err("select a module or a sub-patch".to_string());
            };
            if node.unknown.is_some() {
                return Err(format!("{} is not available in this build", node.descriptor.name));
            }
            let Some(fills) = node.descriptor.fills(role.id) else {
                return Err(format!("{} does not fill {}", node.descriptor.name, role.name));
            };
            Some(fills)
        };
        let (slot_path, slot_id) = self.new_module(&level).map_err(|e| e.to_string())?;
        let slot_lid = slot_path.last();
        let state = SlotState {
            role,
            part: Some(lid),
            part_fills,
            held: BTreeMap::new(),
            label: None,
            fading: None,
            values: BTreeMap::new(),
        };

        // Cables: a role port's move to the slot; another port's go.
        let mut removed = Vec::new();
        let shown = self.level_cables(graph, &level);
        for cable in shown {
            let (is_from, is_to) = (cable.from.0 == lid, cable.to.0 == lid);
            if !is_from && !is_to {
                continue;
            }
            if cable.link {
                self.links.remove(&Link {
                    level: level.clone(),
                    to: cable.to.clone(),
                    from: cable.from.clone(),
                });
            }
            let (from, to) = (cable.from.clone(), cable.to.clone());
            let mapped_from = if is_from {
                state.role_output_of(&from.1).map(|name| (slot_lid, name.to_string()))
            } else {
                Some(from.clone())
            };
            let mapped_to = if is_to {
                state.role_input_of(&to.1).map(|name| (slot_lid, name.to_string()))
            } else {
                Some(to.clone())
            };
            match (mapped_from, mapped_to) {
                (Some(from), Some(to)) => {
                    self.links.insert(Link {
                        level: level.clone(),
                        to,
                        from,
                    });
                }
                _ => {
                    let own = if is_from { &from.1 } else { &to.1 };
                    removed.push(port_label(graph, member, own, is_from));
                }
            }
        }

        // Exposures of the part: a role parameter's follows it onto the
        // slot; another's goes.
        let mut dropped = Vec::new();
        if let Some(key) = self.instances.get(&level).map(|i| i.key.clone()) {
            let definition = self.definitions.get_mut(&key).expect("the level's definition");
            let mut moved = Vec::new();
            for entry in definition.exposed.iter_mut().filter(|e| e.module == lid) {
                match state.role_param_of(&entry.param) {
                    Some(role_name) => {
                        let stored = definition
                            .defaults
                            .remove(&(lid, entry.param.clone()))
                            .unwrap_or_default();
                        entry.module = slot_lid;
                        entry.param = role_name.to_string();
                        moved.push(((slot_lid, role_name.to_string()), stored));
                    }
                    None => dropped.push(entry.param.clone()),
                }
            }
            definition.defaults.extend(moved);
            if !dropped.is_empty() {
                self.drop_exposed(&level, lid, Some(&dropped));
            }
        } else {
            for entry in self.top_exposed.iter_mut().filter(|e| e.module == lid) {
                if let Some(role_name) = state.role_param_of(&entry.param) {
                    entry.module = slot_lid;
                    entry.param = role_name.to_string();
                }
            }
            self.top_exposed.retain(|e| e.module != lid);
        }
        self.slots.insert(slot_path, state);
        Ok(MadeSlot { slot: slot_id, removed })
    }

    /// Makes the slot tile `slot`'s part a plain module again, in place,
    /// with its cables on its own ports, and forgets the held parts
    /// (sub-patches.md, Slots: Dissolve slot). A fade in progress is
    /// finished first; the modules to remove are returned. The caller
    /// removes them, syncs, and commits.
    pub fn dissolve_slot(&mut self, graph: &Graph, slot: ModuleId) -> Result<Vec<ModuleId>, String> {
        let Some(slot_path) = self.ids.path_of(slot).cloned() else {
            return Err("select a slot".to_string());
        };
        let Some(state) = self.slots.get(&slot_path).cloned() else {
            return Err("select a slot".to_string());
        };
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let slot_lid = slot_path.last();
        let transient = self.end_fade(&slot_path);
        let Some(part) = state.part else {
            // An empty slot leaves nothing behind.
            self.remove_slot_record(&slot_path, &state);
            return Ok(transient);
        };
        let links: Vec<Link> = self
            .links_at(&level)
            .filter(|l| l.from.0 == slot_lid || l.to.0 == slot_lid)
            .cloned()
            .collect();
        for link in links {
            self.links.remove(&link);
            let from = if link.from.0 == slot_lid {
                (part, state.own_output(&link.from.1))
            } else {
                link.from.clone()
            };
            let to = if link.to.0 == slot_lid {
                (part, state.own_input(&link.to.1))
            } else {
                link.to.clone()
            };
            // Between two modules the graph holds the cable already; it
            // becomes direct. A link stays a link.
            if !(self.is_module_end(graph, &level, &from) && self.is_module_end(graph, &level, &to)) {
                self.links.insert(Link {
                    level: level.clone(),
                    to,
                    from,
                });
            }
        }
        // An exposure of a role parameter the part has follows it; one of an
        // optional parameter the part lacks has nothing behind it and goes,
        // with its re-exposures above.
        let has = |role_param: &str| {
            self.slot_param_target(&slot_path, role_param)
                .is_some_and(|(id, own)| graph.node(id).is_some_and(|n| n.parameter_index(&own).is_some()))
        };
        let gone: Vec<String> = self
            .exposures_on(&level, slot_lid)
            .into_iter()
            .filter(|role_param| !has(role_param))
            .collect();
        if !gone.is_empty() {
            self.drop_exposed(&level, slot_lid, Some(&gone));
            self.top_exposed
                .retain(|e| !(level.is_root() && e.module == slot_lid && gone.contains(&e.param)));
        }
        if let Some(key) = self.instances.get(&level).map(|i| i.key.clone()) {
            let definition = self.definitions.get_mut(&key).expect("the level's definition");
            let mut moved = Vec::new();
            for entry in definition.exposed.iter_mut().filter(|e| e.module == slot_lid) {
                let stored = definition
                    .defaults
                    .remove(&(slot_lid, entry.param.clone()))
                    .unwrap_or_default();
                entry.module = part;
                entry.param = state.own_param(&entry.param);
                moved.push(((part, entry.param.clone()), stored));
            }
            definition.defaults.extend(moved);
        } else {
            for entry in self.top_exposed.iter_mut().filter(|e| e.module == slot_lid) {
                entry.module = part;
                entry.param = state.own_param(&entry.param);
            }
        }
        self.remove_slot_record(&slot_path, &state);
        Ok(transient)
    }

    /// The parameter names exposed on the member `lid` of `level`: by the
    /// level's definition, or by the patch's own surface at the top.
    fn exposures_on(&self, level: &ModulePath, lid: u32) -> Vec<String> {
        match self.definition_at(level) {
            Some(definition) => definition
                .exposed
                .iter()
                .filter(|e| e.module == lid)
                .map(|e| e.param.clone())
                .collect(),
            None if level.is_root() => self
                .top_exposed
                .iter()
                .filter(|e| e.module == lid)
                .map(|e| e.param.clone())
                .collect(),
            None => Vec::new(),
        }
    }

    /// Forgets a slot and the definitions its held sub-patches kept.
    pub(super) fn remove_slot_record(&mut self, slot_path: &ModulePath, state: &SlotState) {
        for key in state.held.keys() {
            if let PartKey::SubPatch(key) = key {
                self.definitions.remove(key);
            }
        }
        self.slots.remove(slot_path);
    }

    /// Ends a fade on the slot at `slot_path` in the record: the leaving
    /// part and the fades are forgotten here, and their flat ids returned
    /// for the controller to remove.
    pub(super) fn end_fade(&mut self, slot_path: &ModulePath) -> Vec<ModuleId> {
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let Some(fading) = self.slots.get_mut(slot_path).and_then(|s| s.fading.take()) else {
            return Vec::new();
        };
        let mut removed: Vec<ModuleId> = fading.fades.iter().map(|(_, id)| *id).collect();
        // Every other stack copy's fades and leaving part, with what is
        // under it, go too, and their paths are forgotten (ADR-0044).
        for copy in &fading.copies {
            let mut gone: Vec<ModulePath> = copy
                .fades
                .iter()
                .filter_map(|(_, id)| self.ids.path_of(*id).cloned())
                .collect();
            if let Some(lid) = fading.leaving {
                let leaving = copy.level.child(lid);
                gone.extend(
                    self.ids
                        .paths()
                        .filter(|(p, _)| p.starts_with(&leaving))
                        .map(|(p, _)| p.clone()),
                );
            }
            for path in gone {
                if let Some(id) = self.ids.get(&path) {
                    removed.push(id);
                }
                self.ids.forget(&path);
            }
        }
        let Some(leaving) = fading.leaving.map(|lid| level.child(lid)) else {
            // A part faded in from the thru: only the fades go.
            return removed;
        };
        if self.instances.contains_key(&leaving) {
            // A leaving sub-patch part: everything under it goes.
            let under: Vec<ModulePath> = self
                .ids
                .paths()
                .filter(|(p, _)| p.starts_with(&leaving))
                .map(|(p, _)| p.clone())
                .collect();
            for path in under {
                if let Some(instance) = self.instances.remove(&path) {
                    // The held definition stays, stored whole; nested ones
                    // live inside it.
                    if self.definitions.get(&instance.key).is_some_and(|d| d.stored.is_none()) {
                        self.definitions.remove(&instance.key);
                    }
                } else if self.ports.remove(&path).is_none()
                    && let Some(id) = self.ids.get(&path)
                {
                    removed.push(id);
                }
            }
            self.links.retain(|link| !link.level.starts_with(&leaving));
        } else if let Some(id) = self.ids.get(&leaving) {
            removed.push(id);
        }
        removed
    }

    /// Finishes the fade on the slot tile `slot`: the leaving part and the
    /// fades are removed from the controller. The caller syncs and
    /// commits. Nothing happens on a slot that is not fading.
    pub fn finish_fade(&mut self, controller: &mut Controller, slot: ModuleId) -> Vec<ModuleId> {
        let Some(slot_path) = self.ids.path_of(slot).cloned() else {
            return Vec::new();
        };
        self.retarget_fade_sources(controller, &slot_path);
        let removed = self.end_fade(&slot_path);
        for id in &removed {
            let _ = controller.remove_module(*id);
        }
        removed
    }

    /// The new part's output behind a swap fade's `out`, when `source` is
    /// one: what an assignment from the fade stands for once the fade is
    /// done, and what the file and an undo snapshot write for it
    /// (sub-patches.md, How a swap crossfades, point 4). `None` when
    /// `source` is not a fade.
    pub fn output_behind_fade(&self, graph: &Graph, source: OutputRef) -> Option<OutputRef> {
        let fade_path = self.ids.path_of(source.module)?;
        let slot_path = fade_path.parent()?;
        let state = self.slots.get(&slot_path)?;
        let fading = state.fading.as_ref()?;
        let (role_out, _) = fading.fades.iter().find(|(_, fade)| *fade == source.module)?;
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let (id, own) = self.part_output_of_state(graph, &level, state, role_out)?;
        let port = graph.node(id)?.output_index(&own)?;
        Some(OutputRef { module: id, port })
    }

    /// Points every assignment whose source is one of the slot's fades at
    /// the new part's output behind that fade, before the fade goes.
    fn retarget_fade_sources(&self, controller: &mut Controller, slot_path: &ModulePath) {
        let graph = controller.graph();
        let moves: Vec<_> = graph
            .assignments()
            .filter(|(_, a)| self.ids.path_of(a.source.module).and_then(|p| p.parent()).as_ref() == Some(slot_path))
            .filter_map(|(aid, a)| Some((aid, self.output_behind_fade(graph, a.source)?, a.target, a.param)))
            .collect();
        for (aid, source, target, param) in moves {
            let _ = controller.retarget_assignment(aid, source, target, param);
        }
    }

    /// Finishes every fade in progress, as before an undo or a swap during
    /// a fade (sub-patches.md, How a swap crossfades, point 5).
    pub fn finish_all_fades(&mut self, controller: &mut Controller) -> Vec<ModuleId> {
        let fading: Vec<ModuleId> = self.fading_slots().into_iter().map(|(slot, _)| slot).collect();
        fading
            .into_iter()
            .flat_map(|slot| self.finish_fade(controller, slot))
            .collect()
    }

    /// The role values of the slot's current part, by role name: what
    /// carries across a swap.
    fn role_values(&self, graph: &Graph, slot_path: &ModulePath) -> BTreeMap<String, f32> {
        let mut values = BTreeMap::new();
        let Some(state) = self.slots.get(slot_path) else {
            return values;
        };
        // An empty slot carries the values the last part left with it.
        if state.part.is_none() {
            return state.values.clone();
        }
        for param in state.role.params {
            let Some((id, own)) = self.slot_param_target(slot_path, param.name) else {
                continue;
            };
            let Some(node) = graph.node(id) else { continue };
            let Some(index) = node.parameter_index(&own) else {
                continue;
            };
            values.insert(
                param.name.to_string(),
                node.descriptor.saved_values(&node.params)[index],
            );
        }
        values
    }

    /// Puts the current part of the slot at `slot_path` into its held
    /// parts: a module's values and ranges by its own names, a sub-patch's
    /// exposed values with its definition stored whole.
    fn hold_part(
        &mut self,
        graph: &Graph,
        registry: &loom_core::Registry,
        layout: &dyn Fn(u32) -> Layout,
        slot_path: &ModulePath,
    ) -> Option<PartKey> {
        let state = self.slots.get(slot_path)?;
        let level = slot_path.parent()?;
        let part_path = level.child(state.part?);
        let part_id = self.ids.get(&part_path)?;
        if let Some(instance) = self.instances.get(&part_path).cloned() {
            let params = self.instance_values(graph, &part_path);
            // `definition_file` writes the definition as a file's top level
            // with the nested definitions in its table; a file the
            // hierarchy wrote holds no patch-wide module, so nothing is
            // left out on the way back.
            let file = self.definition_file(graph, layout, part_id)?;
            let (definition, _) = SubPatch::from_patch(&file, registry, instance_library(self, &instance.key));
            let nested = file.subpatches.clone();
            let key = PartKey::SubPatch(instance.key.clone());
            let record = self.definitions.get_mut(&instance.key)?;
            record.stored = Some(Box::new(StoredPart { definition, nested }));
            let state = self.slots.get_mut(slot_path)?;
            state.held.insert(
                key.clone(),
                Held {
                    params,
                    ranges: BTreeMap::new(),
                },
            );
            return Some(key);
        }
        let node = graph.node(part_id)?;
        let values = node.descriptor.saved_values(&node.params);
        let mut held = Held::default();
        for (index, spec) in node.descriptor.params.iter().enumerate() {
            held.params.insert(spec.name.to_string(), values[index]);
            if let Some(range) = stored_range(node, index) {
                held.ranges.insert(spec.name.to_string(), range);
            }
        }
        let key = PartKey::Module(node.descriptor.kind());
        let state = self.slots.get_mut(slot_path)?;
        state.held.insert(key.clone(), held);
        Some(key)
    }

    /// Swaps the slot tile `slot` to the module type `kind`, which must
    /// declare the slot's role (sub-patches.md, Slots). Role values carry
    /// across by role name, clamped; the new part's other values come from
    /// its held entry, else its defaults; the leaving part goes into the
    /// held parts and fades out over one swap fade per role output both
    /// parts have. Assignments and mappings on role parameters move to the
    /// new part; those on other parameters of the leaving part are
    /// removed and their labels returned. A fade already in progress is
    /// finished first. `layout` gives tile layouts for a sub-patch part
    /// that is stored. The caller syncs and commits.
    pub fn swap_to_module(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        kind: &str,
    ) -> Result<Swapped, String> {
        self.swap_to_module_at(controller, layout, slot, kind, None)
    }

    /// [`Hierarchy::swap_to_module`], with the new part at the level id
    /// `at` when given: undo and redo put a part back at the id it had, so
    /// the step's assignments and mappings find it (sub-patches.md, Slots:
    /// undo of a swap fades too).
    pub fn swap_to_module_at(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        kind: &str,
        at: Option<u32>,
    ) -> Result<Swapped, String> {
        let slot_path = self
            .ids
            .path_of(slot)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let state = self
            .slots
            .get(&slot_path)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let descriptor = controller.registry().resolve(kind).map_err(|e| e.to_string())?;
        let Some(fills) = descriptor.fills(state.role.id) else {
            return Err(format!("{} does not fill {}", descriptor.name, state.role.name));
        };
        self.finish_fade(controller, slot);
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let old_values = self.role_values(controller.graph(), &slot_path);
        let old_part = state.part;
        let old_id = old_part.and_then(|lid| self.ids.get(&level.child(lid)));
        let held_key = PartKey::Module(descriptor.kind());
        let restore = self.slots.get(&slot_path).and_then(|s| s.held.get(&held_key).cloned());
        self.hold_part(controller.graph(), controller.registry(), layout, &slot_path);

        // The new part, at its held values, then the role values.
        let (new_path, new_id) = match at {
            Some(lid) => {
                let path = level.child(lid);
                let id = self.ids.id_of(&path).map_err(|e| e.to_string())?;
                (path, id)
            }
            None => self.new_module(&level).map_err(|e| e.to_string())?,
        };
        controller.add_module_with_id(new_id, kind).map_err(|e| e.to_string())?;
        let mut added = vec![new_id];
        if let Some(held) = &restore {
            for (name, &value) in &held.params {
                let _ = controller.set_param_named(new_id, name, value);
            }
            for (name, &(min, max)) in &held.ranges {
                if let Some(index) = descriptor.param_index(name) {
                    let _ = controller.set_range(new_id, index, ParamRange::new(min, max));
                }
            }
        }
        for (role_name, value) in &old_values {
            let own = fills.own_param(role_name);
            if descriptor.param_index(&own).is_some() {
                let _ = controller.set_param_named(new_id, &own, *value);
            }
        }
        {
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.held.remove(&held_key);
            record.part = Some(new_path.last());
            record.part_fills = Some(fills);
            record.values.clear();
        }

        // Assignments and mappings move by role name.
        let mut removed = Vec::new();
        if let Some(old_id) = old_id {
            let (moved, mut gone, fades) =
                self.move_links_to_new_part(controller, &slot_path, &state, old_id, new_id)?;
            added.extend(moved);
            removed.append(&mut gone);
            let copies = self.add_copy_fades(controller, &slot_path, old_part, &fades)?;
            added.extend(copies.iter().flat_map(|c| c.fades.iter().map(|(_, id)| *id)));
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.fading = Some(Fading {
                leaving: old_part,
                fades,
                copies,
            });
        } else if self.fades_with_thru(&slot_path) {
            // Into an empty thru slot: the part fades in from the thru
            // signal (sub-patches.md, Empty slots).
            let outputs = self.thru_fade_outputs(controller.graph(), &slot_path, new_path.last());
            let fades = self.add_fades(controller, &slot_path, outputs)?;
            added.extend(fades.iter().map(|(_, id)| *id));
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.fading = Some(Fading {
                leaving: None,
                fades,
                copies: Vec::new(),
            });
        }
        Ok(Swapped {
            part: new_id,
            part_name: descriptor.name.to_string(),
            added,
            removed,
        })
    }

    /// Swaps the slot tile `slot` to a sub-patch: the file `file` inserted
    /// as a copy (or linked to `library`), which must declare the role and
    /// fit it. See [`Hierarchy::swap_to_module`] for what carries across.
    pub fn swap_to_subpatch(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        file: &Patch,
        library: Option<&str>,
    ) -> Result<Swapped, String> {
        self.swap_to_subpatch_at(controller, layout, slot, file, library, None, None)
    }

    /// [`Hierarchy::swap_to_subpatch`], with the new instance at the level
    /// id `at` and its definition under `key` when given, as
    /// [`Hierarchy::swap_to_module_at`] does for undo.
    #[allow(clippy::too_many_arguments)]
    pub fn swap_to_subpatch_at(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        file: &Patch,
        library: Option<&str>,
        at: Option<u32>,
        key: Option<&str>,
    ) -> Result<Swapped, String> {
        let slot_path = self
            .ids
            .path_of(slot)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let state = self
            .slots
            .get(&slot_path)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let (definition, _) = SubPatch::from_patch(file, controller.registry(), library);
        if !definition.fills(state.role, controller.registry(), &file.subpatches) {
            let why: Vec<String> = definition
                .role_mismatches(state.role, controller.registry(), &file.subpatches)
                .iter()
                .map(ToString::to_string)
                .collect();
            let why = if why.is_empty() {
                "it does not declare the role".to_string()
            } else {
                why.join(", ")
            };
            return Err(format!(
                "{} does not fill {}: {why}",
                file.metadata.name, state.role.name
            ));
        }
        self.finish_fade(controller, slot);
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let old_values = self.role_values(controller.graph(), &slot_path);
        let old_part = state.part;
        let old_id = old_part.and_then(|lid| self.ids.get(&level.child(lid)));
        self.hold_part(controller.graph(), controller.registry(), layout, &slot_path);

        let inserted = self
            .insert_with(controller, &level, file, library, at, key)
            .map_err(|e| e.to_string())?;
        let new_id = inserted.instance;
        let new_path = self.ids.path_of(new_id).cloned().expect("just inserted");
        let mut added = inserted.modules.clone();
        // A held entry for this definition would be keyed by the key the
        // insert gave it; a fresh insert has none. The role values land on
        // the exposed items by role name.
        for (role_name, value) in &old_values {
            if let Some((id, own)) = self.slot_param_target_for_part(&new_path, role_name)
                && let Some(index) = controller.graph().node(id).and_then(|n| n.parameter_index(&own))
            {
                let _ = controller.set_param(id, index, *value);
            }
        }
        {
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.part = Some(new_path.last());
            record.part_fills = None;
            record.values.clear();
        }
        let mut removed = Vec::new();
        if let Some(old_id) = old_id {
            let (moved, mut gone, fades) =
                self.move_links_to_new_part(controller, &slot_path, &state, old_id, new_id)?;
            added.extend(moved);
            removed.append(&mut gone);
            let copies = self.add_copy_fades(controller, &slot_path, old_part, &fades)?;
            added.extend(copies.iter().flat_map(|c| c.fades.iter().map(|(_, id)| *id)));
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.fading = Some(Fading {
                leaving: old_part,
                fades,
                copies,
            });
        } else if self.fades_with_thru(&slot_path) {
            // Into an empty thru slot: the part fades in from the thru
            // signal (sub-patches.md, Empty slots).
            let outputs = self.thru_fade_outputs(controller.graph(), &slot_path, new_path.last());
            let fades = self.add_fades(controller, &slot_path, outputs)?;
            added.extend(fades.iter().map(|(_, id)| *id));
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.fading = Some(Fading {
                leaving: None,
                fades,
                copies: Vec::new(),
            });
        }
        Ok(Swapped {
            part: new_id,
            part_name: inserted.name,
            added,
            removed,
        })
    }

    /// Swaps the slot tile `slot` back to a held sub-patch part, whose
    /// definition was stored whole when it left.
    pub fn swap_to_held_subpatch(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        key: &str,
    ) -> Result<Swapped, String> {
        self.swap_to_held_subpatch_at(controller, layout, slot, key, None)
    }

    /// [`Hierarchy::swap_to_held_subpatch`], with the instance back at the
    /// level id `at` when given, under the key it had.
    pub fn swap_to_held_subpatch_at(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        key: &str,
        at: Option<u32>,
    ) -> Result<Swapped, String> {
        let stored = self
            .definitions
            .get(key)
            .and_then(|d| d.stored.clone())
            .ok_or_else(|| format!("the slot no longer holds `{key}`"))?;
        let held = self
            .slot(slot)
            .and_then(|s| s.held.get(&PartKey::SubPatch(key.to_string())).cloned())
            .unwrap_or_default();
        let library = stored.definition.library.clone();
        let file = Patch::from_definition(&stored.definition, &stored.nested);
        // The stored definition goes, so the insert can take its key back.
        self.definitions.remove(key);
        if let Some(path) = self.ids.path_of(slot).cloned()
            && let Some(state) = self.slots.get_mut(&path)
        {
            state.held.remove(&PartKey::SubPatch(key.to_string()));
        }
        let swapped = self.swap_to_subpatch_at(controller, layout, slot, &file, library.as_deref(), at, Some(key))?;
        // Its own values, by exposed name, under the role values.
        let new_path = self.ids.path_of(swapped.part).cloned().expect("just inserted");
        if let Some(definition) = self.definition_at(&new_path).cloned() {
            let role = self.slot(slot).map(|s| s.role);
            for (name, value) in &held.params {
                if role.is_some_and(|r| r.param(name).is_some()) {
                    continue;
                }
                if let Some(exposed) = definition.exposed.iter().find(|e| e.name == *name)
                    && let Some((id, param)) = self.resolve_exposed(&new_path, exposed)
                    && let Some(index) = controller.graph().node(id).and_then(|n| n.parameter_index(&param))
                {
                    let _ = controller.set_param(id, index, *value);
                }
            }
        }
        Ok(swapped)
    }

    /// Remove part: empties the slot tile `slot` (sub-patches.md, Empty
    /// slots). The leaving part goes into `held`, so swapping it back
    /// restores its values. Assignments and MIDI mappings on it, and
    /// assignments from it, have nowhere to go: they are removed, and
    /// their labels returned for the status text. A thru role outside a
    /// stack fades the part out to the thru signal, Linear; any other slot
    /// lets it go at once, and `Emptied::gone` names the modules the caller
    /// leaves out of its sync and then removes. A fade in progress is
    /// finished first. Refused on an empty slot.
    pub fn swap_to_empty(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
    ) -> Result<Emptied, String> {
        let slot_path = self
            .ids
            .path_of(slot)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let Some(old_lid) = self.slots.get(&slot_path).and_then(|s| s.part) else {
            return Err("it holds no part".to_string());
        };
        self.finish_fade(controller, slot);
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let old_path = level.child(old_lid);
        let old_id = self
            .ids
            .get(&old_path)
            .ok_or_else(|| "its part is missing".to_string())?;
        let part_name = self
            .tile_name(old_id)
            .or_else(|| controller.graph().node(old_id).map(|n| n.descriptor.name.to_string()))
            .unwrap_or_default();
        // Worked out while the part is still the slot's.
        let fade_outputs = if self.fades_with_thru(&slot_path) {
            Some(self.thru_fade_outputs(controller.graph(), &slot_path, old_lid))
        } else {
            None
        };
        // The role values stay with the slot, so the next part takes them,
        // as a swap between two parts carries them (FR-ENCAP-08).
        let values = self.role_values(controller.graph(), &slot_path);
        self.hold_part(controller.graph(), controller.registry(), layout, &slot_path);
        let removed = self.drop_links_under(controller, &old_path);
        {
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.part = None;
            record.part_fills = None;
            record.values = values;
        }
        let gone = match fade_outputs {
            Some(outputs) => {
                let fades = self.add_fades(controller, &slot_path, outputs)?;
                let record = self.slots.get_mut(&slot_path).expect("checked above");
                record.fading = Some(Fading {
                    leaving: Some(old_lid),
                    fades,
                    copies: Vec::new(),
                });
                Vec::new()
            }
            None => {
                // No fade: the part goes now, as a pulled cable does.
                let record = self.slots.get_mut(&slot_path).expect("checked above");
                record.fading = Some(Fading {
                    leaving: Some(old_lid),
                    fades: Vec::new(),
                    copies: Vec::new(),
                });
                self.end_fade(&slot_path)
            }
        };
        Ok(Emptied {
            part_name,
            removed,
            gone,
        })
    }

    /// Removes every assignment onto or from a module under `part`, and
    /// every MIDI mapping onto one, returning their labels as a swap's
    /// status names them.
    fn drop_links_under(&self, controller: &mut Controller, part: &ModulePath) -> Vec<String> {
        let graph = controller.graph();
        let under = |id: ModuleId| self.ids.path_of(id).is_some_and(|p| p.starts_with(part));
        let mut labels = Vec::new();
        let mut drops = Vec::new();
        for (aid, a) in graph.assignments() {
            // An implied assignment, such as a stack's spread, is the
            // flattener's, not the player's: it is not named, and `sync`
            // takes it away with the part (ADR-0044).
            if self.is_implied_module(a.source.module) || self.is_implied_module(a.target) {
                continue;
            }
            if under(a.target) {
                let param = crate::param_name(graph, a.target, a.param);
                labels.push(param_label(graph, a.target, &param));
                drops.push(aid);
            } else if under(a.source.module) {
                labels.push(format!("{} (a modulation source)", crate::port_name(graph, a.source)));
                drops.push(aid);
            }
        }
        let unmaps: Vec<(ModuleId, usize, String)> = graph
            .midi_mappings()
            .filter(|(_, m)| under(m.target))
            .map(|(_, m)| {
                let param = crate::param_name(graph, m.target, m.param);
                (m.target, m.param, param_label(graph, m.target, &param))
            })
            .collect();
        for aid in drops {
            let _ = controller.remove_assignment(aid);
        }
        for (target, param, label) in unmaps {
            let _ = controller.unmap_midi(target, param);
            labels.push(label);
        }
        labels
    }

    /// The flat parameter behind the role parameter `param` of a sub-patch
    /// part at `part_path`, by exposed name.
    fn slot_param_target_for_part(&self, part_path: &ModulePath, param: &str) -> Option<(ModuleId, String)> {
        let definition = self.definition_at(part_path)?;
        let inner = definition.exposed.iter().find(|e| e.name == param)?;
        self.resolve_exposed(part_path, inner)
    }

    /// Moves the assignments and mappings on the leaving part `old` to the
    /// new part `new` by role name, removes those on parameters outside
    /// the role (returning their labels), and adds one swap fade per role
    /// output both parts have. Returns the fades' ids, the labels, and the
    /// fades as `(role output, flat id)`.
    #[allow(clippy::type_complexity)]
    fn move_links_to_new_part(
        &mut self,
        controller: &mut Controller,
        slot_path: &ModulePath,
        old_state: &SlotState,
        old: ModuleId,
        new: ModuleId,
    ) -> Result<(Vec<ModuleId>, Vec<String>, Vec<(String, ModuleId)>), String> {
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let new_state = self.slots.get(slot_path).cloned().expect("the slot");
        let graph = controller.graph();
        let old_path = self.path_of(old);
        let new_path = self.path_of(new);
        // Targets under the leaving part: its own parameters, or anything
        // inside a sub-patch part.
        let under_old = |id: ModuleId| self.path_of(id).starts_with(&old_path);
        let mut removed = Vec::new();
        let mut retargets = Vec::new();
        let mut drops = Vec::new();
        for (aid, a) in graph.assignments() {
            let target_under = under_old(a.target);
            let source_under = under_old(a.source.module);
            if !target_under && !source_under {
                continue;
            }
            // An implied assignment, such as a stack's spread, is the
            // flattener's: the new part gets its own from `sync`, and the
            // leaving part keeps this one until the fade ends (ADR-0044).
            if self.is_implied_module(a.source.module) || self.is_implied_module(a.target) {
                continue;
            }
            let mut source = a.source;
            let mut target = (a.target, a.param);
            let mut keep = true;
            if target_under {
                let param = crate::param_name(graph, a.target, a.param);
                // A role parameter the new part lacks counts as one only
                // the leaving part had.
                let behind = self
                    .role_name_of_param(graph, &level, old_state, a.target, &param)
                    .and_then(|role_name| self.slot_param_target(slot_path, &role_name))
                    .and_then(|(id, own)| graph.node(id).and_then(|n| n.parameter_index(&own)).map(|i| (id, i)));
                match behind {
                    Some(found) => target = found,
                    None => {
                        keep = false;
                        removed.push(param_label(graph, a.target, &param));
                    }
                }
            }
            if source_under && keep {
                let output = crate::port_name(graph, a.source);
                match self.role_name_of_output(graph, &level, old_state, a.source.module, &output) {
                    Some(role_name) => match self.part_output_of_state(graph, &level, &new_state, &role_name) {
                        Some((id, own)) => match graph.node(id).and_then(|n| n.output_index(&own)) {
                            Some(port) => source = OutputRef { module: id, port },
                            None => keep = false,
                        },
                        None => keep = false,
                    },
                    None => {
                        keep = false;
                        removed.push(format!("{output} (a modulation source)"));
                    }
                }
            }
            if keep {
                retargets.push((aid, source, target));
            } else {
                drops.push(aid);
            }
        }
        let mut remaps = Vec::new();
        let mut unmaps = Vec::new();
        for (_, mapping) in graph.midi_mappings() {
            if !under_old(mapping.target) {
                continue;
            }
            let param = crate::param_name(graph, mapping.target, mapping.param);
            let behind = self
                .role_name_of_param(graph, &level, old_state, mapping.target, &param)
                .and_then(|role_name| self.slot_param_target(slot_path, &role_name))
                .and_then(|(id, own)| graph.node(id).and_then(|n| n.parameter_index(&own)).map(|i| (id, i)));
            match behind {
                Some((id, index)) => {
                    remaps.push((
                        mapping.target,
                        mapping.param,
                        mapping.control,
                        id,
                        index,
                        mapping.options,
                    ));
                }
                None => {
                    unmaps.push((mapping.target, mapping.param));
                    removed.push(param_label(graph, mapping.target, &param));
                }
            }
        }
        // Fades: one per role output both parts have, unless its signal
        // switches rather than fades (sub-patches.md, How a swap crossfades,
        // point 6).
        let mut fade_outputs = Vec::new();
        for output in new_state.role.outputs {
            let old_has = self
                .part_output_of_state(graph, &level, old_state, output.name)
                .is_some();
            let new_has = self
                .part_output_of_state(graph, &level, &new_state, output.name)
                .is_some();
            if old_has && new_has && fades(output) {
                fade_outputs.push(output.name.to_string());
            }
        }
        let _ = new_path;
        // The new part's output behind each faded role output, so a
        // modulation from the slot's output goes through its fade while the
        // slot fades, and a Modulator or Envelope slot swaps click-free as
        // a source too; `finish_fade` points it at the new part.
        let new_outputs: Vec<(String, OutputRef)> = fade_outputs
            .iter()
            .filter_map(|role_out| {
                let (id, own) = self.part_output_of_state(graph, &level, &new_state, role_out)?;
                let port = graph.node(id)?.output_index(&own)?;
                Some((role_out.clone(), OutputRef { module: id, port }))
            })
            .collect();
        let with_laws: Vec<(String, Law)> = fade_outputs
            .into_iter()
            .filter_map(|role_out| {
                let law = new_state
                    .role
                    .outputs
                    .iter()
                    .find(|o| o.name == role_out)
                    .and_then(fade_law)?;
                Some((role_out, law))
            })
            .collect();
        let fades_made = self.add_fades(controller, slot_path, with_laws)?;
        let added: Vec<ModuleId> = fades_made.iter().map(|(_, id)| *id).collect();
        let through_fade: Vec<(OutputRef, ModuleId)> = fades_made
            .iter()
            .filter_map(|(role_out, fade)| {
                let (_, output) = new_outputs.iter().find(|(name, _)| name == role_out)?;
                Some((*output, *fade))
            })
            .collect();

        for (aid, mut source, (target, param)) in retargets {
            if let Some((_, fade)) = through_fade.iter().find(|(output, _)| *output == source) {
                source = OutputRef {
                    module: *fade,
                    port: FADE_OUT,
                };
            }
            controller
                .retarget_assignment(aid, source, target, param)
                .map_err(|e| e.to_string())?;
        }
        for aid in drops {
            let _ = controller.remove_assignment(aid);
        }
        for (target, param) in unmaps {
            let _ = controller.unmap_midi(target, param);
        }
        for (old_target, old_param, control, target, param, options) in remaps {
            let _ = controller.unmap_midi(old_target, old_param);
            controller
                .map_midi_with(control, target, param, options)
                .map_err(|e| e.to_string())?;
        }
        Ok((added, removed, fades_made))
    }

    /// The role name of a parameter of the leaving part: `param` of the
    /// flat module `module`, which is the part itself or inside a sub-patch
    /// part.
    fn role_name_of_param(
        &self,
        graph: &Graph,
        level: &ModulePath,
        state: &SlotState,
        module: ModuleId,
        param: &str,
    ) -> Option<String> {
        let part_path = level.child(state.part?);
        if self.instances.contains_key(&part_path) {
            let name = self.exposed_name_for(&part_path, &(module, param.to_string()))?;
            return state.role.param(&name).map(|p| p.name.to_string());
        }
        let _ = graph;
        state.role_param_of(param).map(str::to_string)
    }

    /// The role name of an output of the leaving part.
    fn role_name_of_output(
        &self,
        graph: &Graph,
        level: &ModulePath,
        state: &SlotState,
        module: ModuleId,
        output: &str,
    ) -> Option<String> {
        let part_path = level.child(state.part?);
        if self.instances.contains_key(&part_path) {
            let port = self.port_for_output(graph, &part_path, &(module, output.to_string()))?;
            return state.role.output(&port).map(|p| p.name.to_string());
        }
        state.role_output_of(output).map(str::to_string)
    }

    /// The flat output behind a role output of the part `state` names.
    fn part_output_of_state(
        &self,
        graph: &Graph,
        level: &ModulePath,
        state: &SlotState,
        role_out: &str,
    ) -> Option<(ModuleId, String)> {
        let part = state.part?;
        let part_path = level.child(part);
        if self.instances.contains_key(&part_path) {
            return self.source_behind(graph, level, &(part, role_out.to_string()));
        }
        let own = state.own_output(role_out);
        let id = self.ids.get(&part_path)?;
        graph.node(id)?.output_index(&own)?;
        Some((id, own))
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
