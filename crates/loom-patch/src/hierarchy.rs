//! The editor's sub-patch hierarchy, kept beside the flat graph
//! (`docs/design/sub-patches.md`, Editor model; ADR-0039).
//!
//! The engine runs the flat graph a [`Controller`] holds, and that graph is
//! the only home of parameters, cables, assignments, and MIDI mappings. A
//! [`Hierarchy`] records what the flat graph cannot say: where each module
//! sits (its [`ModulePath`]), the definitions, the instance and port tiles,
//! and the cables of each level that touch a port or an instance, in that
//! level's ids. Collapsing and dissolving change only this record, so the
//! sound never changes and every inner module keeps its state.
//!
//! Saving is [`Hierarchy::unflatten`], the inverse of [`Patch::flatten`].
//! [`Hierarchy::sync`] recomputes the flat cables the boundary records
//! imply with the flattener itself and applies the difference to the
//! controller, so what plays is always what the file would load.
//!
//! Every tile on any canvas has a flat id from the one [`FlatIds`] table:
//! a module's, an instance's, or a port's. At the top level a tile's id in
//! its level is its flat id. Inside a definition, level ids are the
//! definition's own and the table maps the path to the flat id.
//!
//! Control thread only. Nothing here opens a file.

use crate::LoadWarning;
use crate::flatten::Added;
use crate::flatten::{FlatIds, ModulePath, port_name};
use crate::library::{LeftOut, reachable_keys, rekey};
use crate::modes::{MEMBER_PARAM, Mode, ModeSettings};
use crate::{
    Exposed, INPUT_KIND, Layout, Metadata, OUTPUT_KIND, Patch, PatchCable, PatchError, PatchMidiMapping,
    PatchModulation, PatchModule, PatchPolarity, ResolvedModule, SLOT_KIND, SUBPATCH_KIND, Slot, SubPatch, unique_name,
};
use behind::{InputStep, Step};
use loom_core::{
    AssignmentId, Controller, Graph, InputRef, ModuleId, ModuleNames, OutputRef, Registry, TRANSPORT_MODULE,
};
use std::collections::{BTreeMap, BTreeSet};

mod modes;
pub use modes::{MEMBER_LID, PlayingMember};
pub mod behind;
mod collapse;
mod exposed;
mod library;
mod links;
mod query;
mod read;
mod slots;
mod sync;
mod unflatten;
pub use slots::{
    CopyFade, Emptied, Fading, Hidden, MadeSlot, SLOT_GONE_TEXT, SlotState, StoredPart, Swapped, added_slot_text,
    expose_role_only_text, fade_law, fade_law_value, made_slot_text, make_slot_label, no_part_text, removed_part_text,
    swapped_text, too_many_modules_text,
};

/// The name a new sub-patch gets when it is made, made unique with a
/// number ("Sub-patch 2").
pub const DEFAULT_NAME: &str = "Sub-patch";

/// The status bar text when only the Transport, or nothing, is selected
/// (Collapsing, point 6).
pub const NOTHING_TO_COLLAPSE: &str = "Select modules to make a sub-patch: the Transport stays at the top level";

/// One end of a cable at one level: the id of a module, instance, or port
/// in that level, and a port name.
pub type End = (u32, String);

/// The ports of an instance tile facing one way: each port tile's flat id
/// and its name, in level-id order.
pub type PortList = Vec<(ModuleId, String)>;

/// Which way a port of a definition faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    /// A Patch input: a cable from outside comes in through it.
    Input,
    /// A Patch output: a cable from inside goes out through it.
    Output,
}

impl Direction {
    /// The module kind that stands for this port in a file.
    pub fn kind(self) -> &'static str {
        match self {
            Self::Input => INPUT_KIND,
            Self::Output => OUTPUT_KIND,
        }
    }

    /// The name of the one port a port tile has: `out` on a Patch input,
    /// `in` on a Patch output.
    pub fn own_port(self) -> &'static str {
        match self {
            Self::Input => "out",
            Self::Output => "in",
        }
    }
}

/// A port of a definition: a Patch input or output tile inside it, and a
/// port on its instance's tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    /// Which way it faces.
    pub direction: Direction,
    /// Its name, which is the label of the Patch input or output module.
    pub name: String,
}

/// An instance tile: which definition it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// The key in the `subpatches` table.
    pub key: String,
    /// The instance's own label, if the file gave one.
    pub label: Option<String>,
}

/// What a definition's file text stores for a parameter whose live value
/// belongs to an outer instance: the inner module's own value and range
/// (ADR-0039, invariant 6). Flattening writes the instance's value over
/// them, so the graph alone cannot restore the file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stored {
    /// The value written in the definition, if any.
    pub value: Option<f32>,
    /// The control range written in the definition, if any.
    pub range: Option<(f32, f32)>,
}

/// A definition: the inside of one instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    /// Its name and credits. The name is what the player sees.
    pub metadata: Metadata,
    /// The parameters it exposes, in exposure order.
    pub exposed: Vec<Exposed>,
    /// The definition-side value and range of each exposed parameter, by
    /// the member's level id and the parameter (or exposed) name.
    pub defaults: BTreeMap<(u32, String), Stored>,
    /// The library file this definition follows, as the file writes it
    /// (sub-patches.md, Linked library items), or `None` for a copy.
    pub library: Option<String>,
    /// The roles it declares (FR-ENCAP-09), as role kinds.
    pub roles: Vec<String>,
    /// Its mode and the fields that go with it (sub-patches.md, Modes).
    pub modes: ModeSettings,
    /// For a definition a slot holds but does not play: the definition
    /// whole, since no instance carries its modules. `None` for a
    /// definition with an instance.
    pub stored: Option<Box<StoredPart>>,
}

/// What inserting a patch file as a sub-patch made (sub-patches.md,
/// Library).
#[derive(Debug, Clone, PartialEq)]
pub struct Inserted {
    /// The new instance tile.
    pub instance: ModuleId,
    /// The definition's name, from the file.
    pub name: String,
    /// The flat modules added to the controller, for a rollback.
    pub modules: Vec<ModuleId>,
    /// Where the file placed each tile inside, by flat id.
    pub layouts: Vec<(ModuleId, Layout)>,
    /// The patch-wide modules the file had, which were left out (rule 8).
    pub left_out: Vec<LeftOut>,
    /// What loaded differently from the file inside, such as an unknown
    /// module.
    pub notes: Vec<LoadWarning>,
}

/// A cable of one level that touches a port tile or an instance port, in
/// that level's ids. Ordered by level and then by the input it feeds, so
/// the one cable into an input is found by range.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Link {
    /// The level the cable is drawn on: the instance path, empty at the top.
    pub level: ModulePath,
    /// The input end.
    pub to: End,
    /// The output end.
    pub from: End,
}

/// What a flat id stands for on a canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tile {
    /// A module of the flat graph.
    Module,
    /// An instance tile, with its definition key.
    Instance(String),
    /// A port tile of a definition.
    Port(Direction, String),
    /// A slot tile, with its role's kind.
    Slot(String),
}

/// One cable as a level shows it, in that level's ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelCable {
    /// The output end.
    pub from: End,
    /// The input end.
    pub to: End,
    /// True for a boundary record; false for a cable the graph holds
    /// between two modules of the level.
    pub link: bool,
}

/// What collapsing made (Collapsing, points 1 to 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collapsed {
    /// The new instance tile.
    pub instance: ModuleId,
    /// The new Patch input tiles, in port order.
    pub inputs: Vec<ModuleId>,
    /// The new Patch output tiles, in port order.
    pub outputs: Vec<ModuleId>,
    /// The tiles that moved inside.
    pub members: Vec<ModuleId>,
}

/// What dissolving did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dissolved {
    /// The definition's name, for the step name.
    pub name: String,
    /// The tiles that came back out, now at the parent level.
    pub members: Vec<ModuleId>,
    /// The tiles that are gone: the instance and its ports.
    pub removed: Vec<ModuleId>,
}

/// What removing a tile takes with it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Removed {
    /// Flat modules the controller must remove.
    pub modules: Vec<ModuleId>,
    /// Every tile that is gone, modules included: positions and sizes to
    /// forget.
    pub tiles: Vec<ModuleId>,
}

/// What an exposed entry of an instance tile stands for: the flat module
/// and parameter it moves, through any nesting, and the range the surface
/// gives it (sub-patches.md, Exposing).
#[derive(Debug, Clone, PartialEq)]
pub struct ExposedTarget {
    /// The flat module whose parameter the surface control moves.
    pub module: ModuleId,
    /// That parameter's name.
    pub param: String,
    /// The narrowest range of every level on the way in, as a finite span,
    /// or `None` when no level narrows it. See [`Hierarchy::exposed_range`].
    pub range: Option<(f32, f32)>,
}

/// The controller revision a hierarchy was last synced against. Bookkeeping,
/// not state: two hierarchies are equal whatever their marks, so an undo
/// snapshot never sees a sync as a change.
#[derive(Debug, Clone, Copy, Default)]
struct SyncMark(Option<u64>);

impl PartialEq for SyncMark {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// The status bar text when Expose is asked at the top level, which has no
/// surface to expose onto.
pub const EXPOSE_INSIDE_ONLY: &str = "Expose works inside a sub-patch";

/// The hierarchy of the open patch.
#[derive(Debug, Clone, PartialEq)]
pub struct Hierarchy {
    ids: FlatIds,
    definitions: BTreeMap<String, Definition>,
    instances: BTreeMap<ModulePath, Instance>,
    ports: BTreeMap<ModulePath, Port>,
    /// The slot tiles, by path (sub-patches.md, Slots).
    slots: BTreeMap<ModulePath, SlotState>,
    links: links::LinkSet,
    /// The flat cables the links imply, as (output, input) in flat ids and
    /// port names, from the last sync or load.
    implied: BTreeSet<(End, End)>,
    /// For each module of copy 0 of a stack, its copies, from the last sync
    /// ([`Flat::copies`](crate::Flat::copies)).
    copies: BTreeMap<ModuleId, Vec<(ModuleId, u32)>>,
    /// The modules the flattener added for modes, from the last sync.
    added: BTreeMap<ModuleId, crate::flatten::Added>,
    /// The controller's revision at the end of the last sync, so a commit
    /// that follows with nothing structural changed skips another.
    synced_revision: SyncMark,
    /// The `exposed` list of the top level, as the file had it: a library
    /// file opened on its own keeps its surface for the next insert. Nothing
    /// shows it; entries whose module is gone are dropped when it is written.
    top_exposed: Vec<Exposed>,
}

impl Default for Hierarchy {
    fn default() -> Self {
        Self::flat()
    }
}

impl Hierarchy {
    /// Updates the authors stored with a sub-patch definition.
    pub fn set_part_authors(&mut self, instance: ModuleId, authors: Vec<String>) {
        if let Some(key) = self.key_at(&self.path_of(instance)).map(str::to_owned)
            && let Some(definition) = self.definitions.get_mut(&key)
        {
            definition.metadata.authors = authors;
        }
    }

    /// The hierarchy of a flat patch: no definitions, a fresh id table.
    pub fn flat() -> Self {
        Self::with_ids(FlatIds::new())
    }

    /// A flat hierarchy over an id table that already holds the graph's
    /// modules.
    pub fn with_ids(ids: FlatIds) -> Self {
        Self {
            ids,
            definitions: BTreeMap::new(),
            instances: BTreeMap::new(),
            ports: BTreeMap::new(),
            slots: BTreeMap::new(),
            links: links::LinkSet::default(),
            implied: BTreeSet::new(),
            copies: BTreeMap::new(),
            added: BTreeMap::new(),
            synced_revision: SyncMark(None),
            top_exposed: Vec::new(),
        }
    }

    /// The next free id in a level: one above every id the level ever had.
    pub fn next_level_id(&self, level: &ModulePath) -> u32 {
        self.ids
            .paths()
            .filter(|(path, _)| path.parent().as_ref() == Some(level) && !path.is_implied())
            .map(|(path, _)| path.last())
            .max()
            .map_or(1, |max| max + 1)
    }

    /// A path and flat id for a new module at `level`. At the top the two
    /// ids are the same number; inside, the level id is the definition's
    /// next and the flat id the table's.
    pub fn new_module(&mut self, level: &ModulePath) -> Result<(ModulePath, ModuleId), PatchError> {
        if level.is_root() {
            let id = self.ids.next_top()?;
            return Ok((ModulePath::top(id.0), id));
        }
        let path = level.child(self.next_level_id(level));
        let id = self.ids.id_of(&path)?;
        Ok((path, id))
    }

    /// Adds a port to the definition at `level`, named `name` made unique
    /// among the ports facing the same way ("in", "in 2"). Returns its
    /// tile id. Refused at the top level, which has no definition.
    pub fn add_port(&mut self, level: &ModulePath, direction: Direction, name: &str) -> Result<ModuleId, PatchError> {
        if level.is_root() {
            return Err(PatchError::NotAnInstance {
                module: 0,
                kind: direction.kind().to_string(),
            });
        }
        let name = self.unique_port_name(level, direction, name);
        let (path, id) = self.new_module(level)?;
        self.ports.insert(path, Port { direction, name });
        Ok(id)
    }

    /// `base` made unique among the ports of `level` facing `direction`.
    fn unique_port_name(&self, level: &ModulePath, direction: Direction, base: &str) -> String {
        let taken: BTreeSet<&str> = self
            .ports
            .iter()
            .filter(|(path, port)| path.parent().as_ref() == Some(level) && port.direction == direction)
            .map(|(_, port)| port.name.as_str())
            .collect();
        unique_name(base, |name| taken.contains(name))
    }

    /// Renames the definition of the instance tile `instance`. The name is
    /// what the tile, the breadcrumb, and the inspector show; the
    /// definition's key stays, since it is not shown. An empty name is
    /// refused.
    pub fn rename(&mut self, instance: ModuleId, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a sub-patch needs a name".to_string());
        }
        let key = self
            .ids
            .path_of(instance)
            .and_then(|path| self.instances.get(path))
            .map(|i| i.key.clone())
            .ok_or_else(|| "select a sub-patch to rename".to_string())?;
        let definition = self
            .definitions
            .get_mut(&key)
            .expect("every instance has its definition");
        definition.metadata.name = name.to_string();
        Ok(())
    }

    /// Records a cable on `level` from `from` to `to`, where at least one
    /// end is a port or instance. Any cable into `to` on this level goes.
    pub fn connect(&mut self, level: &ModulePath, from: End, to: End) {
        self.disconnect(level, &to);
        self.links.insert(Link {
            level: level.clone(),
            to,
            from,
        });
    }

    /// True when a link on `level` feeds `to`, whether or not the graph holds
    /// a cable for it: a port with nothing behind it still has its record.
    pub fn has_link_into(&self, level: &ModulePath, to: &End) -> bool {
        self.link_into(level, to).is_some()
    }

    /// Removes the link into `to` on `level`. True when there was one.
    pub fn disconnect(&mut self, level: &ModulePath, to: &End) -> bool {
        let before = self.links.len();
        self.links.retain(|link| !(link.level == *level && link.to == *to));
        self.links.len() != before
    }

    /// Takes a tile out: a module loses its links; a port goes with its
    /// links; an instance goes with everything inside it. The table keeps
    /// every path, so no id is given out again.
    pub fn remove(&mut self, graph: &Graph, id: ModuleId) -> Removed {
        let mut removed = Removed::default();
        let Some(path) = self.ids.path_of(id).cloned() else {
            if graph.node(id).is_some() {
                removed.modules.push(id);
                removed.tiles.push(id);
            }
            return removed;
        };
        let level = path.parent().unwrap_or_else(ModulePath::root);
        let lid = path.last();
        // A slot goes with its part, its leaving part, and its fades.
        if let Some(slot) = self.slots.get(&path).cloned() {
            removed.modules.extend(self.end_fade(&path));
            if let Some(part) = slot.part
                && let Some(part_id) = self.ids.get(&level.child(part))
            {
                let inner = self.remove(graph, part_id);
                removed.modules.extend(inner.modules);
                removed.tiles.extend(inner.tiles);
            }
            self.remove_slot_record(&path, &slot);
            self.links
                .retain(|link| !(link.level == level && (link.from.0 == lid || link.to.0 == lid)));
            self.drop_exposed(&level, lid, None);
            removed.tiles.push(id);
            return removed;
        }
        let was_instance = self.instances.contains_key(&path);
        if was_instance {
            // Everything under the instance, deepest first.
            let under: Vec<ModulePath> = self
                .ids
                .paths()
                .filter(|(p, _)| p.starts_with(&path) && **p != path)
                .map(|(p, _)| p.clone())
                .collect();
            for inner in &under {
                let Some(inner_id) = self.ids.get(inner) else { continue };
                if let Some(instance) = self.instances.remove(inner) {
                    self.definitions.remove(&instance.key);
                    removed.tiles.push(inner_id);
                } else if let Some(slot) = self.slots.get(inner).cloned() {
                    // A nested slot goes with its record and the definitions
                    // it held; its part and fades are graph nodes under the
                    // instance, removed like any other.
                    self.remove_slot_record(inner, &slot);
                    removed.tiles.push(inner_id);
                } else if self.ports.remove(inner).is_some() {
                    removed.tiles.push(inner_id);
                } else if graph.node(inner_id).is_some() {
                    removed.modules.push(inner_id);
                    removed.tiles.push(inner_id);
                }
            }
            self.links.retain(|link| !link.level.starts_with(&path));
            if let Some(instance) = self.instances.remove(&path) {
                self.definitions.remove(&instance.key);
            }
        }
        let mut removed_port = None;
        if !was_instance {
            removed_port = self.ports.remove(&path);
            if removed_port.is_none() && graph.node(id).is_some() {
                removed.modules.push(id);
            }
        }
        self.links
            .retain(|link| !(link.level == level && (link.from.0 == lid || link.to.0 == lid)));
        // A port's cables outside, on the instance's tile.
        if let Some(port) = removed_port
            && let Some(parent) = level.parent()
        {
            let outside = (level.last(), port.name);
            self.links
                .retain(|link| !(link.level == parent && (link.from == outside || link.to == outside)));
        }
        // What the enclosing definition exposed on the tile goes with it,
        // and so does every re-exposure of that above.
        self.drop_exposed(&level, lid, None);
        // A member leaves its layer's or select's list.
        if let Some(key) = self.instances.get(&level).map(|i| i.key.clone())
            && let Some(definition) = self.definitions.get_mut(&key)
        {
            definition.modes.members.retain(|member| *member != lid);
        }
        removed.tiles.push(id);
        removed
    }

    /// Moves every path under `old` to the same place under `new`: the
    /// table, the instances, the ports, and the links drawn inside.
    fn rename_prefix(&mut self, old: &ModulePath, new: &ModulePath) -> Result<(), PatchError> {
        let moved: Vec<ModulePath> = self
            .ids
            .paths()
            .filter(|(path, _)| path.starts_with(old))
            .map(|(path, _)| path.clone())
            .collect();
        for path in moved {
            let target = path.replace_prefix(old, new);
            self.ids.rename(&path, target.clone())?;
            if let Some(instance) = self.instances.remove(&path) {
                self.instances.insert(target.clone(), instance);
            }
            if let Some(port) = self.ports.remove(&path) {
                self.ports.insert(target.clone(), port);
            }
            if let Some(slot) = self.slots.remove(&path) {
                self.slots.insert(target, slot);
            }
        }
        self.links = self
            .links
            .iter()
            .cloned()
            .map(|mut link| {
                if link.level.starts_with(old) {
                    link.level = link.level.replace_prefix(old, new);
                }
                link
            })
            .collect();
        Ok(())
    }

    /// Sets the instance tile's own label, as a file or the clipboard gives
    /// it. Nothing for a tile that is not an instance.
    pub fn set_label(&mut self, instance: ModuleId, label: Option<String>) {
        if let Some(path) = self.ids.path_of(instance).cloned()
            && let Some(record) = self.instances.get_mut(&path)
        {
            record.label = label;
        }
    }

    /// Restores a snapshot of the hierarchy, keeping the ids the table has
    /// handed out since: no id is ever given out twice (undo.md, rule 5).
    pub fn restore(&mut self, snapshot: &Hierarchy) {
        let next = self.ids.next_free();
        // Implied modules outlive the snapshot: the next sync keeps, adds,
        // or removes them, so their state survives an unrelated undo.
        let implied: Vec<(ModulePath, ModuleId)> = self
            .ids
            .paths()
            .filter(|(path, _)| path.is_implied())
            .map(|(path, id)| (path.clone(), id))
            .collect();
        let (copies, added) = (std::mem::take(&mut self.copies), std::mem::take(&mut self.added));
        *self = snapshot.clone();
        for (path, id) in implied {
            if self.ids.path_of(id).is_some() {
                continue;
            }
            // A copy made again since the snapshot, as by an undo step's
            // swap that synced before the rest of the step: the one in the
            // graph keeps its path, not the snapshot's older id.
            if self.ids.get(&path).is_some() {
                self.ids.forget(&path);
            }
            let _ = self.ids.reserve(path, id);
        }
        self.copies = copies;
        self.added = added;
        // The restored overlay was never synced against this graph.
        self.synced_revision = SyncMark(None);
        self.ids.reserve_id(ModuleId(next.saturating_sub(1)));
    }
}

/// One level of a patch being written.
#[derive(Default)]
struct Level {
    modules: Vec<PatchModule>,
    cables: Vec<PatchCable>,
    modulation: Vec<PatchModulation>,
    midi: Vec<PatchMidiMapping>,
}

/// The definition-side value and range of each exposed parameter. Through
/// a slot, they are the part's, under the part's own name.
fn defaults_of(definition: &SubPatch, registry: &Registry) -> BTreeMap<(u32, String), Stored> {
    let mut defaults = BTreeMap::new();
    for exposed in &definition.exposed {
        let Some(member) = definition.modules.iter().find(|m| m.id == exposed.module) else {
            continue;
        };
        let (member, param) = match &member.slot {
            None => (member, exposed.param.clone()),
            Some(slot) => {
                let part = slot
                    .part
                    .and_then(|part| definition.modules.iter().find(|m| m.id == part));
                let Some(part) = part else {
                    defaults.insert((exposed.module, exposed.param.clone()), Stored::default());
                    continue;
                };
                let own = if part.subpatch.is_some() {
                    Some(exposed.param.clone())
                } else {
                    crate::flatten::part_own_param(registry, &part.kind, crate::role_id(&slot.role), &exposed.param)
                };
                match own {
                    Some(own) => (part, own),
                    None => {
                        defaults.insert((exposed.module, exposed.param.clone()), Stored::default());
                        continue;
                    }
                }
            }
        };
        let stored = Stored {
            value: member.params.get(&param).copied(),
            range: member.ranges.get(&param).copied(),
        };
        defaults.insert((exposed.module, exposed.param.clone()), stored);
    }
    defaults
}

/// The path of the member `lid` of `level`.
fn path_of_member(level: &ModulePath, lid: u32) -> ModulePath {
    level.child(lid)
}

/// Writes a stored value and range into a module entry.
fn apply_stored(module: &mut PatchModule, param: &str, stored: &Stored) {
    match stored.value {
        Some(value) => {
            module.params.insert(param.to_string(), value);
        }
        None => {
            module.params.remove(param);
        }
    }
    match stored.range {
        Some(range) => {
            module.ranges.insert(param.to_string(), range);
        }
        None => {
            module.ranges.remove(param);
        }
    }
}

/// `value` clamped to an exposed range, when the range is a finite span.
fn clamp_exposed(value: f32, range: Option<(f32, f32)>) -> f32 {
    match range {
        Some(range) if is_span(range) => value.clamp(range.0, range.1),
        _ => value,
    }
}

/// The control range of parameter `index` as the file would write it: the
/// selected one, unless it is the parameter's default range, which the
/// file leaves out (control-ranges.md, rule 7).
fn stored_range(node: &loom_core::Node, index: usize) -> Option<(f32, f32)> {
    let range = node.range(index)?;
    let default = node
        .descriptor
        .params
        .get(index)
        .and_then(|spec| spec.options.default_range);
    (Some(range) != default).then_some((range.min, range.max))
}

/// True for a finite range with its minimum below its maximum: the only
/// kind the flattener accepts and a control can span.
fn is_span((min, max): (f32, f32)) -> bool {
    min.is_finite() && max.is_finite() && min < max
}

/// The range a chain of exposed ranges gives, outer first: each overlap in
/// turn, and when one does not overlap, the inner range alone, because its
/// clamp is applied last. `None` for an empty chain. The one rule for the
/// surface control ([`Hierarchy::exposed_range`]) and for the span MIDI
/// sweeps (`Flat::limits`), so the two cannot drift apart.
pub(crate) fn fold_ranges(chain: impl IntoIterator<Item = (f32, f32)>) -> Option<(f32, f32)> {
    chain.into_iter().reduce(|(min, max), (inner_min, inner_max)| {
        let overlap = (min.max(inner_min), max.min(inner_max));
        if is_span(overlap) {
            overlap
        } else {
            (inner_min, inner_max)
        }
    })
}

/// A cable's ends in flat ids and port names.
fn cable_ends(graph: &Graph, from: OutputRef, to: InputRef) -> (End, End) {
    (
        (from.module.0, crate::port_name(graph, from)),
        (to.module.0, crate::input_name(graph, to)),
    )
}

/// Every cable of the graph, in flat ids and port names.
fn graph_cables(graph: &Graph) -> BTreeSet<(End, End)> {
    graph.cables().map(|(from, to)| cable_ends(graph, from, to)).collect()
}

/// The input an end names, by index.
fn input_ref(graph: &Graph, end: &End) -> Option<InputRef> {
    let module = ModuleId(end.0);
    let port = graph.node(module)?.input_index(&end.1)?;
    Some(InputRef { module, port })
}

impl Direction {
    /// The port name a Patch input or output takes when it has no label:
    /// "in" or "out".
    pub fn own_port_fallback(self) -> &'static str {
        match self {
            Self::Input => "in",
            Self::Output => "out",
        }
    }
}

#[cfg(test)]
mod tests;
