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
use crate::flatten::{FlatIds, ModulePath, port_name};
use crate::library::{LeftOut, reachable_keys, rekey};
use crate::{
    Exposed, INPUT_KIND, Layout, Metadata, OUTPUT_KIND, Patch, PatchCable, PatchError, PatchMidiMapping,
    PatchModulation, PatchModule, PatchPolarity, ResolvedModule, SLOT_KIND, SUBPATCH_KIND, Slot, SubPatch, unique_name,
};
use loom_core::{
    AssignmentId, Controller, Graph, InputRef, ModuleId, ModuleNames, OutputRef, Registry, TRANSPORT_MODULE,
};
use std::collections::{BTreeMap, BTreeSet};

mod slots;
pub use slots::{
    Fading, MadeSlot, NO_SLOT_IN_COLLAPSE, SlotState, StoredPart, Swapped, expose_role_only_text, made_slot_text,
    make_slot_label, swapped_text,
};

/// The status bar text when Dissolve is asked for a sub-patch that holds
/// a slot. Moving a slot out of a sub-patch is a follow-up.
pub const NO_SLOT_IN_DISSOLVE: &str = "Cannot dissolve a sub-patch that holds a slot yet; dissolve the slot first";

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
    links: BTreeSet<Link>,
    /// The flat cables the links imply, as (output, input) in flat ids and
    /// port names, from the last sync or load.
    implied: BTreeSet<(End, End)>,
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
            links: BTreeSet::new(),
            implied: BTreeSet::new(),
            top_exposed: Vec::new(),
        }
    }

    /// Reads the hierarchy of `patch` after it was flattened with `ids`
    /// into `graph`: every instance and port gets a flat id in path order,
    /// the definitions keep their exposed parameters and stored values,
    /// and the boundary cables become links.
    pub fn from_patch(patch: &Patch, graph: &Graph, ids: FlatIds, registry: &Registry) -> Result<Self, PatchError> {
        let mut hierarchy = Self::with_ids(ids);
        hierarchy.top_exposed = patch.exposed.clone();
        hierarchy.read_level(
            &patch.modules,
            &patch.cables,
            &ModulePath::root(),
            &patch.subpatches,
            registry,
        )?;
        hierarchy.adopt(graph);
        hierarchy.implied = graph_cables(graph)
            .difference(&hierarchy.direct_cables(patch))
            .cloned()
            .collect();
        Ok(hierarchy)
    }

    /// One level of a file: its instances (recursively), its ports, and
    /// its links.
    fn read_level(
        &mut self,
        modules: &[PatchModule],
        cables: &[PatchCable],
        level: &ModulePath,
        table: &BTreeMap<String, SubPatch>,
        registry: &Registry,
    ) -> Result<(), PatchError> {
        let mut structural = BTreeSet::new();
        let mut sorted: Vec<&PatchModule> = modules.iter().collect();
        sorted.sort_by_key(|m| m.id);
        for module in sorted {
            let path = level.child(module.id);
            if module.is_instance() || module.subpatch.is_some() {
                let Some(key) = module.subpatch.clone() else {
                    return Err(PatchError::NotAnInstance {
                        module: module.id,
                        kind: module.kind.clone(),
                    });
                };
                let Some(definition) = table.get(&key) else {
                    return Err(PatchError::UnknownSubPatch {
                        module: module.id,
                        key: key.clone(),
                    });
                };
                self.ids.id_of(&path)?;
                self.instances.insert(
                    path.clone(),
                    Instance {
                        key: key.clone(),
                        label: module.label.clone(),
                    },
                );
                self.definitions.insert(
                    key,
                    Definition {
                        metadata: definition.metadata.clone(),
                        exposed: definition.exposed.clone(),
                        defaults: defaults_of(definition, registry),
                        library: definition.library.clone(),
                        roles: definition.roles.clone(),
                        stored: None,
                    },
                );
                self.read_level(&definition.modules, &definition.cables, &path, table, registry)?;
                structural.insert(module.id);
            } else if module.is_slot() || module.slot.is_some() {
                let Some(slot) = &module.slot else {
                    return Err(PatchError::BadSlot {
                        module: module.id,
                        reason: "it has no `slot` field".to_string(),
                    });
                };
                let role = registry.resolve_role(&slot.role).map_err(|e| PatchError::BadSlot {
                    module: module.id,
                    reason: e.to_string(),
                })?;
                self.ids.id_of(&path)?;
                let part_fills = slot
                    .part
                    .and_then(|part| modules.iter().find(|m| m.id == part))
                    .filter(|part| part.subpatch.is_none())
                    .and_then(|part| registry.resolve(&part.kind).ok())
                    .and_then(|d| d.fills(role.id));
                self.slots.insert(
                    path,
                    SlotState {
                        role,
                        part: slot.part,
                        part_fills,
                        held: slot.held.clone(),
                        label: module.label.clone(),
                        fading: None,
                    },
                );
                // A held sub-patch keeps its definition, whole, since no
                // instance carries its modules.
                for key in slot.held.keys() {
                    let crate::PartKey::SubPatch(key) = key else { continue };
                    let Some(definition) = table.get(key) else { continue };
                    let nested: BTreeMap<String, SubPatch> = crate::reachable_keys(definition, table)
                        .into_iter()
                        .filter_map(|k| table.get(&k).map(|d| (k, d.clone())))
                        .collect();
                    self.definitions.insert(
                        key.clone(),
                        Definition {
                            metadata: definition.metadata.clone(),
                            exposed: definition.exposed.clone(),
                            defaults: BTreeMap::new(),
                            library: definition.library.clone(),
                            roles: definition.roles.clone(),
                            stored: Some(Box::new(StoredPart {
                                definition: definition.clone(),
                                nested,
                            })),
                        },
                    );
                }
                structural.insert(module.id);
            } else if !level.is_root() && (module.kind == INPUT_KIND || module.kind == OUTPUT_KIND) {
                let direction = if module.kind == INPUT_KIND {
                    Direction::Input
                } else {
                    Direction::Output
                };
                self.ids.id_of(&path)?;
                self.ports.insert(
                    path,
                    Port {
                        direction,
                        name: port_name(module, direction.own_port_fallback()),
                    },
                );
                structural.insert(module.id);
            }
        }
        for cable in cables {
            if structural.contains(&cable.from.0) || structural.contains(&cable.to.0) {
                self.links.insert(Link {
                    level: level.clone(),
                    to: cable.to.clone(),
                    from: cable.from.clone(),
                });
            }
        }
        Ok(())
    }

    /// Gives every graph module the table does not know a top-level path.
    /// A module added straight to the controller is a top-level module.
    pub fn adopt(&mut self, graph: &Graph) {
        for (id, _) in graph.nodes() {
            if self.ids.path_of(id).is_none() {
                // Refused only when another path owns the id, which a
                // module the controller holds never has.
                let _ = self.ids.reserve(ModulePath::top(id.0), id);
            }
        }
    }

    /// The id table.
    pub fn ids(&self) -> &FlatIds {
        &self.ids
    }

    /// True when the patch has no sub-patch.
    pub fn is_flat(&self) -> bool {
        self.instances.is_empty() && self.slots.is_empty()
    }

    /// The name a tile shows: a definition's name for an instance, a port's
    /// name for a port. `None` for a module or an id no tile has.
    pub fn tile_name(&self, id: ModuleId) -> Option<String> {
        let path = self.ids.path_of(id)?;
        if let Some(definition) = self.definition_at(path) {
            return Some(definition.metadata.name.clone());
        }
        if let Some(slot) = self.slots.get(path) {
            return Some(slot.name());
        }
        self.ports.get(path).map(|port| port.name.clone())
    }

    /// The level nearest to `level` that still exists: `level` itself, or
    /// the longest prefix of it that is the top or an instance.
    pub fn nearest_level(&self, level: &ModulePath) -> ModulePath {
        (0..=level.0.len())
            .rev()
            .map(|n| level.prefix(n))
            .find(|prefix| prefix.is_root() || self.instances.contains_key(prefix))
            .unwrap_or_else(ModulePath::root)
    }

    /// Each tile's layout as `patch` wrote it, by flat id: the top level's
    /// modules, and every definition's through its instance.
    pub fn layouts(&self, patch: &Patch) -> Vec<(ModuleId, Layout)> {
        let mut levels: Vec<(ModulePath, &[PatchModule])> = vec![(ModulePath::root(), &patch.modules)];
        for (path, instance) in &self.instances {
            if let Some(definition) = patch.subpatches.get(&instance.key) {
                levels.push((path.clone(), &definition.modules));
            }
        }
        let mut layouts = Vec::new();
        for (level, modules) in levels {
            for module in modules {
                if let Some(id) = self.ids.get(&level.child(module.id)) {
                    layouts.push((
                        id,
                        Layout {
                            position: module.position,
                            size: module.size,
                        },
                    ));
                }
            }
        }
        layouts
    }

    /// The path of a tile: the table's, or a top-level path for a graph
    /// module the table never saw.
    pub fn path_of(&self, id: ModuleId) -> ModulePath {
        self.ids.path_of(id).cloned().unwrap_or_else(|| ModulePath::top(id.0))
    }

    /// The level a tile is shown on.
    pub fn level_of(&self, id: ModuleId) -> ModulePath {
        self.path_of(id).parent().unwrap_or_else(ModulePath::root)
    }

    /// What a flat id stands for, or `None` for an id no live tile has.
    pub fn tile(&self, graph: &Graph, id: ModuleId) -> Option<Tile> {
        if self.is_hidden(id) {
            return None;
        }
        if graph.node(id).is_some() {
            return Some(Tile::Module);
        }
        let path = self.ids.path_of(id)?;
        if let Some(instance) = self.instances.get(path) {
            return Some(Tile::Instance(instance.key.clone()));
        }
        if let Some(slot) = self.slots.get(path) {
            return Some(Tile::Slot(slot.role.kind()));
        }
        self.ports
            .get(path)
            .map(|port| Tile::Port(port.direction, port.name.clone()))
    }

    /// The definition of an instance tile.
    pub fn definition_of(&self, instance: ModuleId) -> Option<&Definition> {
        let path = self.ids.path_of(instance)?;
        self.definition_at(path)
    }

    /// The definition shown at `level`, or `None` at the top.
    pub fn definition_at(&self, level: &ModulePath) -> Option<&Definition> {
        let instance = self.instances.get(level)?;
        self.definitions.get(&instance.key)
    }

    /// The names along a level's path, outermost first, for the breadcrumb.
    pub fn path_names(&self, level: &ModulePath) -> Vec<String> {
        (1..=level.0.len())
            .filter_map(|n| {
                let prefix = ModulePath(level.0[..n].to_vec());
                self.definition_at(&prefix).map(|d| d.metadata.name.clone())
            })
            .collect()
    }

    /// The instance and port tiles of a level, in level-id order, each with
    /// what it is.
    pub fn structural_tiles(&self, level: &ModulePath) -> Vec<(ModuleId, Tile)> {
        let mut tiles: Vec<(u32, ModuleId, Tile)> = Vec::new();
        for (path, instance) in &self.instances {
            if path.parent().as_ref() == Some(level)
                && let Some(id) = self.ids.get(path)
            {
                tiles.push((path.last(), id, Tile::Instance(instance.key.clone())));
            }
        }
        for (path, port) in &self.ports {
            if path.parent().as_ref() == Some(level)
                && let Some(id) = self.ids.get(path)
            {
                tiles.push((path.last(), id, Tile::Port(port.direction, port.name.clone())));
            }
        }
        for (path, slot) in &self.slots {
            if path.parent().as_ref() == Some(level)
                && let Some(id) = self.ids.get(path)
            {
                tiles.push((path.last(), id, Tile::Slot(slot.role.kind())));
            }
        }
        tiles.sort_by_key(|(lid, _, _)| *lid);
        tiles.into_iter().map(|(_, id, tile)| (id, tile)).collect()
    }

    /// The ports of an instance's tile, inputs then outputs, each in
    /// level-id order: the port tile's flat id and its name.
    pub fn instance_ports(&self, instance: ModuleId) -> (PortList, PortList) {
        let Some(path) = self.ids.path_of(instance) else {
            return (Vec::new(), Vec::new());
        };
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for (port_path, port) in &self.ports {
            if port_path.parent().as_ref() != Some(path) {
                continue;
            }
            let Some(id) = self.ids.get(port_path) else { continue };
            match port.direction {
                Direction::Input => inputs.push((port_path.last(), id, port.name.clone())),
                Direction::Output => outputs.push((port_path.last(), id, port.name.clone())),
            }
        }
        inputs.sort_by_key(|(lid, _, _)| *lid);
        outputs.sort_by_key(|(lid, _, _)| *lid);
        let strip = |list: Vec<(u32, ModuleId, String)>| list.into_iter().map(|(_, id, name)| (id, name)).collect();
        (strip(inputs), strip(outputs))
    }

    /// The flat id of the tile with level id `lid` at `level`. At the top
    /// level the two are the same number, so a module the table never saw
    /// is found too.
    pub fn id_at(&self, level: &ModulePath, lid: u32) -> Option<ModuleId> {
        self.ids
            .get(&level.child(lid))
            .or_else(|| level.is_root().then_some(ModuleId(lid)))
    }

    /// The cables a level shows, in its ids: the graph's cables between two
    /// of its modules (those the links do not imply), then the links.
    pub fn level_cables(&self, graph: &Graph, level: &ModulePath) -> Vec<LevelCable> {
        let mut cables = Vec::new();
        for (from, to) in graph.cables() {
            if self.is_hidden(from.module) || self.is_hidden(to.module) {
                continue;
            }
            let (from_end, to_end) = cable_ends(graph, from, to);
            if self.implied.contains(&(from_end.clone(), to_end.clone())) {
                continue;
            }
            let (from_path, to_path) = (self.path_of(from.module), self.path_of(to.module));
            if from_path.parent().as_ref() == Some(level) && to_path.parent().as_ref() == Some(level) {
                cables.push(LevelCable {
                    from: (from_path.last(), from_end.1),
                    to: (to_path.last(), to_end.1),
                    link: false,
                });
            }
        }
        for link in self.links_at(level) {
            cables.push(LevelCable {
                from: link.from.clone(),
                to: link.to.clone(),
                link: true,
            });
        }
        cables
    }

    /// True when the graph holds `from` to `to` only because a link
    /// implies it.
    pub fn is_implied(&self, graph: &Graph, from: OutputRef, to: InputRef) -> bool {
        self.implied.contains(&cable_ends(graph, from, to))
    }

    /// The flat output that stands behind an end of `level`: a module's
    /// output itself, what is behind an instance's output port, or what
    /// feeds a Patch input from outside. `None` when nothing does.
    pub fn source_behind(&self, graph: &Graph, level: &ModulePath, end: &End) -> Option<(ModuleId, String)> {
        self.source_behind_guarded(graph, level, end, &mut Vec::new())
    }

    fn source_behind_guarded(
        &self,
        graph: &Graph,
        level: &ModulePath,
        end: &End,
        seen: &mut Vec<(ModulePath, End)>,
    ) -> Option<(ModuleId, String)> {
        let key = (level.clone(), end.clone());
        if seen.contains(&key) {
            return None;
        }
        seen.push(key);
        let path = level.child(end.0);
        if let Some(port) = self.ports.get(&path) {
            if port.direction != Direction::Input || end.1 != "out" {
                return None;
            }
            // What feeds the instance's port, one level up.
            let parent = level.parent()?;
            let link = self.link_into(&parent, &(level.last(), port.name.clone()))?;
            return self.source_behind_guarded(graph, &parent, &link.from, seen);
        }
        if self.instances.contains_key(&path) {
            let port_path = self.port_path(&path, Direction::Output, &end.1)?;
            let link = self.link_into(&path, &(port_path.last(), "in".to_string()))?;
            return self.source_behind_guarded(graph, &path, &link.from, seen);
        }
        if let Some(slot) = self.slots.get(&path) {
            // A slot's role output: the part's own output behind it.
            slot.role.output(&end.1)?;
            let part = slot.part?;
            let own = if self.instances.contains_key(&level.child(part)) {
                end.1.clone()
            } else {
                slot.own_output(&end.1)
            };
            return self.source_behind_guarded(graph, level, &(part, own), seen);
        }
        let id = self.ids.get(&path)?;
        let node = graph.node(id)?;
        node.output_index(&end.1)?;
        Some((id, end.1.clone()))
    }

    /// The flat inputs that stand behind an input end of `level`: a
    /// module's input itself, every inner input an instance's port feeds,
    /// or what a Patch output feeds outside.
    pub fn inputs_behind(&self, graph: &Graph, level: &ModulePath, end: &End) -> Vec<(ModuleId, String)> {
        let mut found = Vec::new();
        self.inputs_behind_guarded(graph, level, end, &mut Vec::new(), &mut found);
        found
    }

    fn inputs_behind_guarded(
        &self,
        graph: &Graph,
        level: &ModulePath,
        end: &End,
        seen: &mut Vec<(ModulePath, End)>,
        found: &mut Vec<(ModuleId, String)>,
    ) {
        let key = (level.clone(), end.clone());
        if seen.contains(&key) {
            return;
        }
        seen.push(key);
        let path = level.child(end.0);
        if let Some(port) = self.ports.get(&path) {
            if port.direction != Direction::Output || end.1 != "in" {
                return;
            }
            let Some(parent) = level.parent() else { return };
            let outside = (level.last(), port.name.clone());
            for link in self.links_from(&parent, &outside) {
                self.inputs_behind_guarded(graph, &parent, &link.to, seen, found);
            }
            return;
        }
        if self.instances.contains_key(&path) {
            let Some(port_path) = self.port_path(&path, Direction::Input, &end.1) else {
                return;
            };
            let inside = (port_path.last(), "out".to_string());
            for link in self.links_from(&path, &inside) {
                self.inputs_behind_guarded(graph, &path, &link.to, seen, found);
            }
            return;
        }
        if let Some(slot) = self.slots.get(&path) {
            if slot.role.input(&end.1).is_none() {
                return;
            }
            let Some(part) = slot.part else { return };
            let own = if self.instances.contains_key(&level.child(part)) {
                end.1.clone()
            } else {
                slot.own_input(&end.1)
            };
            self.inputs_behind_guarded(graph, level, &(part, own), seen, found);
            return;
        }
        if let Some(id) = self.ids.get(&path)
            && graph.node(id).is_some_and(|n| n.input_index(&end.1).is_some())
        {
            found.push((id, end.1.clone()));
        }
    }

    /// The path of the port tile named `name` facing `direction` inside
    /// the instance at `instance`.
    fn port_path(&self, instance: &ModulePath, direction: Direction, name: &str) -> Option<ModulePath> {
        self.ports
            .iter()
            .find(|(path, port)| {
                path.parent().as_ref() == Some(instance) && port.direction == direction && port.name == name
            })
            .map(|(path, _)| path.clone())
    }

    /// The links drawn on `level`.
    fn links_at(&self, level: &ModulePath) -> impl Iterator<Item = &Link> {
        let level = level.clone();
        self.links.iter().filter(move |link| link.level == level)
    }

    /// The one link into `to` on `level`.
    fn link_into(&self, level: &ModulePath, to: &End) -> Option<&Link> {
        self.links_at(level).find(|link| link.to == *to)
    }

    /// The links out of `from` on `level`.
    fn links_from(&self, level: &ModulePath, from: &End) -> impl Iterator<Item = &Link> {
        let from = from.clone();
        self.links_at(level).filter(move |link| link.from == from)
    }

    /// The exposed name of `instance` that stands for the flat parameter
    /// `target`, through any depth of nesting.
    pub fn exposed_name_for(&self, instance: &ModulePath, target: &(ModuleId, String)) -> Option<String> {
        let definition = self.definition_at(instance)?;
        definition
            .exposed
            .iter()
            .find(|e| self.resolve_exposed(instance, e).as_ref() == Some(target))
            .map(|e| e.name.clone())
    }

    /// The flat module and parameter an exposed entry of the definition at
    /// `instance` stands for.
    pub fn resolve_exposed(&self, instance: &ModulePath, exposed: &Exposed) -> Option<(ModuleId, String)> {
        let member = instance.child(exposed.module);
        if let Some(definition) = self.definition_at(&member) {
            let inner = definition.exposed.iter().find(|e| e.name == exposed.param)?;
            return self.resolve_exposed(&member, inner);
        }
        if self.slots.contains_key(&member) {
            return self.slot_param_target(&member, &exposed.param);
        }
        Some((self.ids.get(&member)?, exposed.param.clone()))
    }

    /// The name of the output port of `instance` behind which the flat
    /// output `source` stands.
    pub fn port_for_output(&self, graph: &Graph, instance: &ModulePath, source: &(ModuleId, String)) -> Option<String> {
        let parent = instance.parent()?;
        self.ports
            .iter()
            .filter(|(path, port)| path.parent().as_ref() == Some(instance) && port.direction == Direction::Output)
            .find(|(_, port)| {
                self.source_behind(graph, &parent, &(instance.last(), port.name.clone()))
                    .as_ref()
                    == Some(source)
            })
            .map(|(_, port)| port.name.clone())
    }

    /// The live value of an exposed parameter of the instance at `path`,
    /// read through any nesting and clamped to each level's range on the
    /// way in, outer first, as the flattener writes it (flattening rule 12).
    pub fn exposed_value(&self, graph: &Graph, instance: &ModulePath, exposed: &Exposed) -> Option<f32> {
        let (id, param) = self.resolve_exposed(instance, exposed)?;
        let node = graph.node(id)?;
        let value = match &node.unknown {
            Some(unknown) => *unknown.params.get(&param)?,
            None => {
                let index = node.parameter_index(&param)?;
                node.descriptor.saved_values(&node.params)[index]
            }
        };
        Some(
            self.range_chain(instance, exposed)
                .into_iter()
                .fold(value, |value, range| clamp_exposed(value, Some(range))),
        )
    }

    /// Every finite range on the way from an exposed entry of the instance
    /// at `instance` to the flat parameter, outer first.
    fn range_chain(&self, instance: &ModulePath, exposed: &Exposed) -> Vec<(f32, f32)> {
        let mut chain = Vec::new();
        if let Some(range) = exposed.range.filter(|r| is_span(*r)) {
            chain.push(range);
        }
        let member = instance.child(exposed.module);
        if let Some(definition) = self.definition_at(&member)
            && let Some(through) = definition.exposed.iter().find(|e| e.name == exposed.param)
        {
            chain.extend(self.range_chain(&member, through));
        }
        chain
    }

    /// The range a surface control for `exposed` spans: the overlap of
    /// every level's range on the way in, or `None` when no level narrows
    /// it. Clamping to each range in turn (as [`Hierarchy::exposed_value`]
    /// does) gives the same value as clamping to the overlap, so the
    /// control and the saved value agree. Ranges that do not overlap, which
    /// only a hand-written file can make, give the innermost one, because
    /// its clamp is applied last.
    pub fn exposed_range(&self, instance: &ModulePath, exposed: &Exposed) -> Option<(f32, f32)> {
        fold_ranges(self.range_chain(instance, exposed))
    }

    /// What the `index`th exposed entry of the instance tile `instance`
    /// stands for, or `None` when the tile is not an instance or has no
    /// such entry.
    pub fn exposed_target(&self, instance: ModuleId, index: usize) -> Option<ExposedTarget> {
        let path = self.ids.path_of(instance)?;
        let definition = self.definition_at(path)?;
        let exposed = definition.exposed.get(index)?;
        let (module, param) = self.resolve_exposed(path, exposed)?;
        Some(ExposedTarget {
            module,
            param,
            range: self.exposed_range(path, exposed),
        })
    }

    /// The instance tile one level up that exposes `param` of the tile
    /// `member` (a module's parameter, or a nested instance's exposed
    /// name), and the name it has on that surface. `None` at the top level
    /// or when it is not exposed.
    pub fn exposure_of(&self, member: ModuleId, param: &str) -> Option<(ModuleId, String)> {
        let path = self.ids.path_of(member)?;
        let level = path.parent()?;
        let definition = self.definition_at(&level)?;
        // A slot's part is exposed through the slot, by the role's name.
        let (lid, param) = self.slot_end_for_param(&level, path.last(), param);
        let entry = definition
            .exposed
            .iter()
            .find(|e| e.module == lid && e.param == param)?;
        Some((self.ids.get(&level)?, entry.name.clone()))
    }

    /// The outermost surface that exposes `param` of the tile `member`,
    /// through every level above it: that instance tile and the index of
    /// its surface item. `None` when nothing exposes it.
    pub fn outermost_surface(&self, member: ModuleId, param: &str) -> Option<(ModuleId, usize)> {
        let mut found = None;
        let mut at = (member, param.to_string());
        while let Some((instance, name)) = self.exposure_of(at.0, &at.1) {
            let index = self
                .definition_of(instance)?
                .exposed
                .iter()
                .position(|e| e.name == name)?;
            found = Some((instance, index));
            at = (instance, name);
        }
        found
    }

    /// The instance tile on `level` whose exposures reach the flat
    /// parameter `target`, through any depth, and the index of the exposed
    /// entry on its surface. `None` when `target` is on `level` itself or
    /// no exposure reaches it. A parameter exposed twice resolves to the
    /// first entry.
    pub fn surface_for(&self, level: &ModulePath, target: &(ModuleId, String)) -> Option<(ModuleId, usize)> {
        let target_level = self.level_of(target.0);
        if target_level == *level || !target_level.starts_with(level) {
            return None;
        }
        let instance = target_level.prefix(level.0.len() + 1);
        let definition = self.definition_at(&instance)?;
        let index = definition
            .exposed
            .iter()
            .position(|e| self.resolve_exposed(&instance, e).as_ref() == Some(target))?;
        Some((self.ids.get(&instance)?, index))
    }

    /// The patch as it would be saved: every level's modules, ports,
    /// instances, cables, assignments, and mappings in the level's ids,
    /// with `layout` giving each tile's position and size by flat id.
    /// The patch's own metadata is left for the caller.
    pub fn unflatten(&self, graph: &Graph, layout: impl Fn(u32) -> Layout) -> Patch {
        self.unflatten_with(graph, layout, true, &BTreeSet::new())
    }

    /// [`Hierarchy::unflatten`], with or without the graph's own cables. Without
    /// them, the file holds only what the links say, so flattening it gives
    /// exactly the cables the links imply.
    fn unflatten_with(
        &self,
        graph: &Graph,
        layout: impl Fn(u32) -> Layout,
        direct: bool,
        skip: &BTreeSet<ModuleId>,
    ) -> Patch {
        let mut levels: BTreeMap<ModulePath, Level> = BTreeMap::new();
        levels.entry(ModulePath::root()).or_default();
        for path in self.instances.keys() {
            levels.entry(path.clone()).or_default();
        }
        // What exists only while a slot fades is never written.
        let mut skip = skip.clone();
        skip.extend(self.transient_ids());
        let skip = &skip;

        // Modules, with the definition-side values of exposed parameters.
        for (id, node) in graph.nodes().filter(|(id, _)| !skip.contains(id)) {
            let path = self.path_of(id);
            let level = path.parent().unwrap_or_else(ModulePath::root);
            let mut module = ResolvedModule::from_node(id, node, layout(id.0)).write();
            module.id = path.last();
            if let Some(definition) = self.definition_at(&level) {
                for ((member, param), stored) in &definition.defaults {
                    if *member == module.id {
                        apply_stored(&mut module, param, stored);
                    }
                }
                // Exposed through a slot: the stored value is the part's,
                // under the part's own name.
                let lid = module.id;
                for (slot_path, slot) in self
                    .slots
                    .iter()
                    .filter(|(p, s)| p.parent().as_ref() == Some(&level) && s.part == Some(lid))
                {
                    for ((member, param), stored) in &definition.defaults {
                        if *member == slot_path.last() {
                            apply_stored(&mut module, &slot.own_param(param), stored);
                        }
                    }
                }
            }
            levels.entry(level).or_default().modules.push(module);
        }
        // Slots, with their held parts.
        for (path, slot) in &self.slots {
            let Some(level) = path.parent() else { continue };
            let Layout { position, size } = self.ids.get(path).map_or_else(Layout::default, |id| layout(id.0));
            levels.entry(level).or_default().modules.push(PatchModule {
                id: path.last(),
                kind: SLOT_KIND.to_string(),
                subpatch: None,
                slot: Some(Slot {
                    role: slot.role.kind(),
                    part: slot.part,
                    held: slot.held.clone(),
                }),
                label: slot.label.clone(),
                params: BTreeMap::new(),
                position,
                size,
                ranges: BTreeMap::new(),
            });
        }
        // Ports.
        for (path, port) in &self.ports {
            let Some(level) = path.parent() else { continue };
            if self.is_leaving_path(path) {
                continue;
            }
            let Layout { position, size } = self.ids.get(path).map_or_else(Layout::default, |id| layout(id.0));
            levels.entry(level).or_default().modules.push(PatchModule {
                id: path.last(),
                kind: port.direction.kind().to_string(),
                subpatch: None,
                slot: None,
                label: Some(port.name.clone()),
                params: BTreeMap::new(),
                position,
                size,
                ranges: BTreeMap::new(),
            });
        }
        // Instances, with their exposed values. One that is leaving a slot
        // is not written; its definition is stored whole instead.
        for (path, instance) in &self.instances {
            let Some(level) = path.parent() else { continue };
            if self.is_leaving_path(path) {
                continue;
            }
            let Layout { position, size } = self.ids.get(path).map_or_else(Layout::default, |id| layout(id.0));
            levels.entry(level.clone()).or_default().modules.push(PatchModule {
                id: path.last(),
                kind: SUBPATCH_KIND.to_string(),
                subpatch: Some(instance.key.clone()),
                slot: None,
                label: instance.label.clone(),
                params: self.instance_values(graph, path),
                position,
                size,
                ranges: BTreeMap::new(),
            });
        }
        // Cables: the graph's between two modules of one level, unless a
        // link implies them, then the links.
        for (from, to) in graph.cables().filter(|_| direct) {
            if skip.contains(&from.module) || skip.contains(&to.module) {
                continue;
            }
            let (from_end, to_end) = cable_ends(graph, from, to);
            if self.implied.contains(&(from_end.clone(), to_end.clone())) {
                continue;
            }
            let (from_path, to_path) = (self.path_of(from.module), self.path_of(to.module));
            if from_path.parent() != to_path.parent() {
                // A cable across a boundary that no link made: the file has
                // no place for it (rule 4).
                continue;
            }
            let level = from_path.parent().unwrap_or_else(ModulePath::root);
            // A link into the same input wins (invariant 2): the graph's
            // cable is the one a sync replaces.
            if self.link_into(&level, &(to_path.last(), to_end.1.clone())).is_some() {
                continue;
            }
            levels.entry(level).or_default().cables.push(PatchCable {
                from: (from_path.last(), from_end.1),
                to: (to_path.last(), to_end.1),
            });
        }
        for link in &self.links {
            if self.is_leaving_path(&link.level.child(0)) && !link.level.is_root() {
                continue;
            }
            levels.entry(link.level.clone()).or_default().cables.push(PatchCable {
                from: link.from.clone(),
                to: link.to.clone(),
            });
        }
        // Assignments: within a level as they are; across levels through
        // the exposed name or the output port they cross.
        for (_, assignment) in graph
            .assignments()
            .filter(|(_, a)| !skip.contains(&a.source.module) && !skip.contains(&a.target))
        {
            let Some(entry) = self.assignment_entry(graph, assignment) else {
                continue;
            };
            levels.entry(entry.0).or_default().modulation.push(entry.1);
        }
        // Mappings live with their module; on a slot's part, through the
        // slot by the role's name.
        for (_, mapping) in graph.midi_mappings().filter(|(_, m)| !skip.contains(&m.target)) {
            let path = self.path_of(mapping.target);
            let level = path.parent().unwrap_or_else(ModulePath::root);
            let param = crate::param_name(graph, mapping.target, mapping.param);
            let target = self.slot_end_for_param(&level, path.last(), &param);
            levels
                .entry(level)
                .or_default()
                .midi
                .push(crate::midi_entry(mapping, target));
        }

        let mut patch = Patch::new("");
        let top = levels.remove(&ModulePath::root()).unwrap_or_default();
        patch.modules = top.modules;
        patch.cables = top.cables;
        patch.modulation = top.modulation;
        patch.midi = top.midi;
        for (path, instance) in &self.instances {
            if self.is_leaving_path(path) {
                continue;
            }
            let Some(definition) = self.definitions.get(&instance.key) else {
                continue;
            };
            let level = levels.remove(path).unwrap_or_default();
            patch.subpatches.insert(
                instance.key.clone(),
                SubPatch {
                    metadata: definition.metadata.clone(),
                    modules: level.modules,
                    cables: level.cables,
                    modulation: level.modulation,
                    midi: level.midi,
                    exposed: definition.exposed.clone(),
                    library: definition.library.clone(),
                    roles: definition.roles.clone(),
                },
            );
        }
        // Definitions a slot holds but does not play, with what nests in
        // them, rekeyed where a live definition took the key since.
        for (key, definition) in &self.definitions {
            let Some(stored) = &definition.stored else { continue };
            if patch.subpatches.contains_key(key) {
                continue;
            }
            let mut root = stored.definition.clone();
            let mut nested = stored.nested.clone();
            crate::library::rekey(&mut root, &mut nested, |candidate| {
                candidate == key || patch.subpatches.contains_key(candidate)
            });
            patch.subpatches.insert(key.clone(), root);
            for (nested_key, nested) in nested {
                patch.subpatches.entry(nested_key).or_insert(nested);
            }
        }
        // The top level's surface, less what no longer stands behind it: an
        // entry names a top-level id, which is a flat id only while the
        // module is still at the top (collapsing keeps its id inside).
        patch.exposed = self
            .top_exposed
            .iter()
            .filter(|exposed| {
                let member = ModulePath::top(exposed.module);
                if let Some(definition) = self.definition_at(&member) {
                    return definition.exposed.iter().any(|e| e.name == exposed.param);
                }
                if let Some(slot) = self.slots.get(&member) {
                    return slot.role.param(&exposed.param).is_some();
                }
                let id = ModuleId(exposed.module);
                !skip.contains(&id)
                    && self.path_of(id) == member
                    && graph.node(id).is_some_and(|node| match &node.unknown {
                        Some(unknown) => unknown.params.contains_key(&exposed.param),
                        None => node.parameter_index(&exposed.param).is_some(),
                    })
            })
            .cloned()
            .collect();
        patch.canonicalise();
        patch
    }

    /// The `params` of the instance at `path`: the live value of each
    /// exposed parameter the outer level does not expose again, else the
    /// stored definition-side value.
    pub fn instance_values(&self, graph: &Graph, path: &ModulePath) -> BTreeMap<String, f32> {
        let mut values = BTreeMap::new();
        let Some(definition) = self.definition_at(path) else {
            return values;
        };
        let outer = path.parent().and_then(|level| self.definition_at(&level));
        for exposed in &definition.exposed {
            let stored = outer.and_then(|outer| outer.defaults.get(&(path.last(), exposed.name.clone())));
            let value = match stored {
                Some(stored) => stored.value,
                None => self.exposed_value(graph, path, exposed),
            };
            if let Some(value) = value {
                values.insert(exposed.name.clone(), value);
            }
        }
        values
    }

    /// An assignment as its level writes it, or `None` when it crosses a
    /// boundary that no exposed parameter or port explains.
    fn assignment_entry(
        &self,
        graph: &Graph,
        assignment: &loom_core::Assignment,
    ) -> Option<(ModulePath, PatchModulation)> {
        let source_path = self.path_of(assignment.source.module);
        let target_path = self.path_of(assignment.target);
        let source_level = source_path.parent().unwrap_or_else(ModulePath::root);
        let target_level = target_path.parent().unwrap_or_else(ModulePath::root);
        let output = crate::port_name(graph, assignment.source);
        let param = crate::param_name(graph, assignment.target, assignment.param);
        let (level, source, target) = if source_level == target_level {
            // On a slot's part, by the role's name through the slot.
            let source = self.slot_end_for_output(&source_level, source_path.last(), &output);
            let target = self.slot_end_for_param(&target_level, target_path.last(), &param);
            (source_level, source, target)
        } else if target_level.starts_with(&source_level) {
            // The target is inside an instance of the source's level.
            let instance = target_level.prefix(source_level.0.len() + 1);
            let name = self.exposed_name_for(&instance, &(assignment.target, param))?;
            let source = self.slot_end_for_output(&source_level, source_path.last(), &output);
            (source_level, source, (instance.last(), name))
        } else if source_level.starts_with(&target_level) {
            // The source is inside an instance of the target's level.
            let instance = source_level.prefix(target_level.0.len() + 1);
            let port = self.port_for_output(graph, &instance, &(assignment.source.module, output))?;
            let target = self.slot_end_for_param(&target_level, target_path.last(), &param);
            (target_level, (instance.last(), port), target)
        } else {
            return None;
        };
        Some((
            level,
            PatchModulation {
                source,
                target,
                depth: assignment.depth,
                polarity: assignment.polarity.map(PatchPolarity::from),
                enabled: assignment.enabled,
            },
        ))
    }

    /// The flat cables of `patch` that are cables between two modules of
    /// one level, in flat ids and names.
    fn direct_cables(&self, patch: &Patch) -> BTreeSet<(End, End)> {
        let mut direct = BTreeSet::new();
        let mut levels: Vec<(ModulePath, &[PatchModule], &[PatchCable])> =
            vec![(ModulePath::root(), &patch.modules, &patch.cables)];
        for (path, instance) in &self.instances {
            if let Some(definition) = patch.subpatches.get(&instance.key) {
                levels.push((path.clone(), &definition.modules, &definition.cables));
            }
        }
        for (level, modules, cables) in levels {
            let structural: BTreeSet<u32> = modules
                .iter()
                .filter(|m| {
                    m.is_instance()
                        || m.subpatch.is_some()
                        || m.is_slot()
                        || (!level.is_root() && (m.kind == INPUT_KIND || m.kind == OUTPUT_KIND))
                })
                .map(|m| m.id)
                .collect();
            for cable in cables {
                if structural.contains(&cable.from.0) || structural.contains(&cable.to.0) {
                    continue;
                }
                if let (Some(from), Some(to)) = (
                    self.ids.get(&level.child(cable.from.0)),
                    self.ids.get(&level.child(cable.to.0)),
                ) {
                    direct.insert(((from.0, cable.from.1.clone()), (to.0, cable.to.1.clone())));
                }
            }
        }
        direct
    }

    /// Makes the controller's cables the ones the file would load: the
    /// hierarchy is written out, flattened with the flattener itself (which
    /// also refuses what a file could not hold), and the difference in
    /// cables is applied. It also puts the live assignments on each
    /// parameter in the order the file loads them (flattening rule 11),
    /// giving a new id only to one that must move, so the sum plays the
    /// same after save and reload (sub-patches.md, Editor model), and gives
    /// each exposed parameter the range MIDI sweeps on it (Exposing). A
    /// refusal changes nothing. The caller commits. Returns the assignments
    /// that got a new id, as (old, new), so what keeps an id can follow.
    pub fn sync(&mut self, controller: &mut Controller) -> Result<Vec<(AssignmentId, AssignmentId)>, PatchError> {
        self.sync_without(controller, &[])
    }

    /// [`Hierarchy::sync`] with `skip` left out of the file, as if those
    /// modules were gone: a delete checks and settles the cables first,
    /// while the modules are still in the graph, so a refusal changes
    /// nothing. The cables into and out of them are disconnected.
    pub fn sync_without(
        &mut self,
        controller: &mut Controller,
        skip: &[ModuleId],
    ) -> Result<Vec<(AssignmentId, AssignmentId)>, PatchError> {
        let mut skip: BTreeSet<ModuleId> = skip.iter().copied().collect();
        skip.extend(self.transient_ids());
        self.adopt(controller.graph());
        // What the links imply, from the links alone: a cable the graph
        // holds for a link that is gone must be written as direct now.
        let links_only = self.unflatten_with(controller.graph(), |_| Layout::default(), false, &skip);
        let mut scratch = self.ids.clone();
        let flat = links_only.flatten(controller.registry(), &mut scratch)?;
        self.implied = flat
            .cables
            .iter()
            .map(|cable| (cable.from.clone(), cable.to.clone()))
            .collect();
        let patch = self.unflatten_with(controller.graph(), |_| Layout::default(), true, &skip);
        let mut scratch = self.ids.clone();
        let flat = patch.flatten(controller.registry(), &mut scratch)?;
        let mut desired: BTreeSet<(End, End)> = flat
            .cables
            .iter()
            .map(|cable| (cable.from.clone(), cable.to.clone()))
            .collect();
        debug_assert_eq!(
            self.implied,
            desired.difference(&self.direct_cables(&patch)).cloned().collect(),
            "the links imply the same cables both ways"
        );
        // While a slot fades, both parts run and the fades sit between the
        // new part and what the slot's output fed (sub-patches.md, How a
        // swap crossfades).
        let wiring = self.fade_wiring(controller.graph());
        for (part_out, fade_out) in &wiring.reroute {
            let moved: Vec<(End, End)> = desired
                .iter()
                .filter(|(from, to)| from == part_out && to.0 != fade_out.0)
                .cloned()
                .collect();
            for (from, to) in moved {
                desired.remove(&(from, to.clone()));
                desired.insert((fade_out.clone(), to));
            }
        }
        desired.extend(wiring.add);
        let current = graph_cables(controller.graph());
        for (_, to) in current.difference(&desired) {
            let input = input_ref(controller.graph(), to);
            if let Some(input) = input {
                controller.disconnect(input)?;
            }
        }
        for (from, to) in desired.difference(&current) {
            controller.connect_named(ModuleId(from.0), &from.1, ModuleId(to.0), &to.1)?;
        }
        let renamed = order_assignments(controller, &flat.modulation).map_err(PatchError::Engine)?;
        // The range MIDI sweeps follows the surfaces: a range that changed
        // or went away reaches the engine with the caller's commit.
        controller
            .set_limits(flat.limits_in(controller.graph()))
            .map_err(PatchError::Engine)?;
        Ok(renamed)
    }

    /// The next free id in a level: one above every id the level ever had.
    pub fn next_level_id(&self, level: &ModulePath) -> u32 {
        self.ids
            .paths()
            .filter(|(path, _)| path.parent().as_ref() == Some(level))
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

    /// Exposes `param` of the member `member` of the definition at `level`
    /// on that definition's surface (sub-patches.md, Exposing). The member
    /// is a module, and `param` one of its parameters, or a nested
    /// instance, and `param` one of its exposed names. Returns the surface
    /// name: the parameter's label, made unique. A parameter already
    /// exposed keeps its name and nothing changes. Refused at the top
    /// level, for a port, for a transient parameter, and for a name the
    /// member does not have. The caller syncs and commits.
    pub fn expose(&mut self, graph: &Graph, level: &ModulePath, member: u32, param: &str) -> Result<String, String> {
        let Some(key) = self.instances.get(level).map(|i| i.key.clone()) else {
            return Err(EXPOSE_INSIDE_ONLY.to_string());
        };
        // A slot's part is exposed through the slot, by the role's name;
        // a parameter only the part has is refused (sub-patches.md, Slots).
        let (member, param) = if let Some((slot_path, slot)) = self.slot_owning(level, member) {
            match slot.role_param_of(param) {
                Some(role_name) => (slot_path.last(), role_name.to_string()),
                None => {
                    let part_id = self.ids.get(&path_of_member(level, member));
                    let node = part_id.and_then(|id| graph.node(id));
                    let label = node
                        .and_then(|n| n.descriptor.params.iter().find(|p| p.name == param))
                        .map_or(param.to_string(), |p| p.label.to_string());
                    let part_name = node.map_or_else(
                        || {
                            self.definition_at(&level.child(member))
                                .map_or_else(String::new, |d| d.metadata.name.clone())
                        },
                        |n| n.descriptor.name.to_string(),
                    );
                    return Err(expose_role_only_text(slot.role, &label, &part_name));
                }
            }
        } else {
            (member, param.to_string())
        };
        let param = param.as_str();
        let path = level.child(member);
        let (label, stored) = if let Some(definition) = self.definition_at(&path) {
            let Some(through) = definition.exposed.iter().find(|e| e.name == param).cloned() else {
                return Err(format!("{} exposes no `{param}`", definition.metadata.name));
            };
            let value = self.exposed_value(graph, &path, &through);
            (param.to_string(), Stored { value, range: None })
        } else if let Some(slot) = self.slots.get(&path) {
            let Some(role_param) = slot.role.param(param) else {
                return Err(format!("{} has no role parameter `{param}`", slot.role.name));
            };
            let stored = match self.slot_param_target(&path, param) {
                Some((id, own)) => {
                    let node = graph.node(id);
                    let index = node.and_then(|n| n.parameter_index(&own));
                    match (node, index) {
                        (Some(node), Some(index)) => Stored {
                            value: Some(node.descriptor.saved_values(&node.params)[index]),
                            range: stored_range(node, index),
                        },
                        _ => Stored::default(),
                    }
                }
                None => Stored::default(),
            };
            (role_param.label.to_string(), stored)
        } else if self.ports.contains_key(&path) {
            return Err("a port has no parameters".to_string());
        } else {
            let Some(node) = self.ids.get(&path).and_then(|id| graph.node(id)) else {
                return Err(format!("there is no module {member}"));
            };
            match &node.unknown {
                Some(unknown) => {
                    let Some(&value) = unknown.params.get(param) else {
                        return Err(format!("{} has no parameter `{param}`", unknown.kind));
                    };
                    (
                        param.to_string(),
                        Stored {
                            value: Some(value),
                            range: None,
                        },
                    )
                }
                None => {
                    let Some(index) = node.parameter_index(param) else {
                        return Err(format!("{} has no parameter `{param}`", node.descriptor.name));
                    };
                    let spec = &node.descriptor.params[index];
                    if spec.is_transient() {
                        return Err(format!("{} is a live gesture, not a value to expose", spec.label));
                    }
                    (
                        spec.label.to_string(),
                        Stored {
                            value: Some(node.descriptor.saved_values(&node.params)[index]),
                            range: stored_range(node, index),
                        },
                    )
                }
            }
        };
        Ok(self.expose_on(&key, member, param, &label, None, stored))
    }

    /// Takes `param` of the member `member` off the surface of the
    /// definition at `level`, and off every surface above that re-exposed
    /// it. Exposures below, inside a nested member, stay. Returns whether
    /// it was exposed. The caller removes the assignments that reached it
    /// from other levels, syncs, and commits.
    pub fn unexpose(&mut self, level: &ModulePath, member: u32, param: &str) -> bool {
        let exposed = self
            .definition_at(level)
            .is_some_and(|d| d.exposed.iter().any(|e| e.module == member && e.param == param));
        if exposed {
            self.drop_exposed(level, member, Some(&[param.to_string()]));
        }
        exposed
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
        removed.tiles.push(id);
        removed
    }

    /// Drops the enclosing definition's exposed entries that name `member`
    /// (all of them, or only those on `names`), and the entries above that
    /// re-exposed them, level by level.
    fn drop_exposed(&mut self, level: &ModulePath, member: u32, names: Option<&[String]>) {
        let Some(key) = self.instances.get(level).map(|i| i.key.clone()) else {
            return;
        };
        let Some(definition) = self.definitions.get_mut(&key) else {
            return;
        };
        let goes = |e: &Exposed| e.module == member && names.is_none_or(|names| names.contains(&e.param));
        let gone: Vec<String> = definition
            .exposed
            .iter()
            .filter(|e| goes(e))
            .map(|e| e.name.clone())
            .collect();
        if gone.is_empty() {
            return;
        }
        definition.exposed.retain(|e| !goes(e));
        definition
            .defaults
            .retain(|(m, param), _| !(*m == member && names.is_none_or(|names| names.contains(param))));
        if let Some(parent) = level.parent() {
            self.drop_exposed(&parent, level.last(), Some(&gone));
        }
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

    /// Collapses the selected tiles of `level` into a new sub-patch
    /// (Collapsing, points 1 to 7). The Transport, the ports of this
    /// level, and Patch input and output modules at the top count as
    /// outside. Nothing in the graph changes; the caller places the new
    /// tiles and syncs.
    pub fn collapse(&mut self, graph: &Graph, level: &ModulePath, selection: &[u32]) -> Result<Collapsed, String> {
        let is_outside_kind = |id: ModuleId| {
            graph.node(id).is_some_and(|node| {
                node.descriptor.id == TRANSPORT_MODULE
                    || (level.is_root()
                        && (node.descriptor.kind() == INPUT_KIND || node.descriptor.kind() == OUTPUT_KIND))
            })
        };
        let mut members: BTreeSet<u32> = BTreeSet::new();
        for &id in selection {
            let id = ModuleId(id);
            if self.level_of(id) != *level || is_outside_kind(id) {
                continue;
            }
            let path = self.path_of(id);
            if self.slots.contains_key(&path) || self.slot_of_part(id).is_some() {
                return Err(NO_SLOT_IN_COLLAPSE.to_string());
            }
            if graph.node(id).is_some() || self.instances.contains_key(&path) {
                members.insert(path.last());
            }
        }
        if members.is_empty() {
            return Err(NOTHING_TO_COLLAPSE.to_string());
        }

        // The definition and its instance.
        let name = unique_name(DEFAULT_NAME, |name| {
            self.definitions.values().any(|d| d.metadata.name == name)
        });
        let key = unique_name(&name.to_lowercase(), |key| self.definitions.contains_key(key));
        let (instance_path, instance_id) = self.new_module(level).map_err(|e| e.to_string())?;
        let ilid = instance_path.last();
        self.instances.insert(
            instance_path.clone(),
            Instance {
                key: key.clone(),
                label: None,
            },
        );
        self.definitions.insert(
            key.clone(),
            Definition {
                metadata: Metadata {
                    name,
                    ..Metadata::default()
                },
                exposed: Vec::new(),
                defaults: BTreeMap::new(),
                library: None,
                roles: Vec::new(),
                stored: None,
            },
        );

        // What the level showed, before anything moves.
        let shown = self.level_cables(graph, level);
        let member_ids: Vec<ModuleId> = members.iter().filter_map(|lid| self.id_at(level, *lid)).collect();

        // Members move inside, keeping their level ids.
        for lid in &members {
            self.rename_prefix(&level.child(*lid), &instance_path.child(*lid))
                .map_err(|e| e.to_string())?;
        }

        // Cables: inside stays inside, a crossing cable gets a port.
        let mut inputs: BTreeMap<End, (u32, String)> = BTreeMap::new();
        let mut outputs: BTreeMap<End, (u32, String)> = BTreeMap::new();
        let mut input_tiles = Vec::new();
        let mut output_tiles = Vec::new();
        for cable in shown {
            let (from_in, to_in) = (members.contains(&cable.from.0), members.contains(&cable.to.0));
            match (from_in, to_in) {
                (true, true) => {
                    if cable.link {
                        self.links.remove(&Link {
                            level: level.clone(),
                            to: cable.to.clone(),
                            from: cable.from.clone(),
                        });
                        self.links.insert(Link {
                            level: instance_path.clone(),
                            to: cable.to,
                            from: cable.from,
                        });
                    }
                }
                (false, true) => {
                    if cable.link {
                        self.links.remove(&Link {
                            level: level.clone(),
                            to: cable.to.clone(),
                            from: cable.from.clone(),
                        });
                    }
                    let port = match inputs.get(&cable.from) {
                        Some(port) => port.clone(),
                        None => {
                            let label = self.inner_port_label(graph, &instance_path, &cable.to, Direction::Input);
                            let id = self
                                .add_port(&instance_path, Direction::Input, &label)
                                .map_err(|e| e.to_string())?;
                            input_tiles.push(id);
                            let path = self.ids.path_of(id).cloned().expect("just added");
                            let port = (path.last(), self.ports[&path].name.clone());
                            inputs.insert(cable.from.clone(), port.clone());
                            port
                        }
                    };
                    self.links.insert(Link {
                        level: level.clone(),
                        to: (ilid, port.1.clone()),
                        from: cable.from,
                    });
                    self.links.insert(Link {
                        level: instance_path.clone(),
                        to: cable.to,
                        from: (port.0, "out".to_string()),
                    });
                }
                (true, false) => {
                    if cable.link {
                        self.links.remove(&Link {
                            level: level.clone(),
                            to: cable.to.clone(),
                            from: cable.from.clone(),
                        });
                    }
                    let port = self
                        .output_port_for(graph, &instance_path, &cable.from, &mut outputs, &mut output_tiles)
                        .map_err(|e| e.to_string())?;
                    self.links.insert(Link {
                        level: level.clone(),
                        to: cable.to,
                        from: (ilid, port.1.clone()),
                    });
                }
                (false, false) => {}
            }
        }

        // Assignments across the new boundary (point 4): each target is
        // exposed once, however many assignments reach it.
        for (_, assignment) in graph.assignments() {
            let source_inside = self.path_of(assignment.source.module).starts_with(&instance_path);
            let target_inside = self.path_of(assignment.target).starts_with(&instance_path);
            let param = crate::param_name(graph, assignment.target, assignment.param);
            if !source_inside && target_inside && self.level_of(assignment.source.module) == *level {
                let target_path = self.path_of(assignment.target);
                let member = instance_path.child(target_path.0[instance_path.0.len()]);
                let (inner_param, stored) = if self.instances.contains_key(&member) {
                    let Some(name) = self.exposed_name_for(&member, &(assignment.target, param.clone())) else {
                        continue;
                    };
                    let value = self
                        .definition_at(&member)
                        .and_then(|d| d.exposed.iter().find(|e| e.name == name).cloned())
                        .and_then(|e| self.exposed_value(graph, &member, &e));
                    (name, Stored { value, range: None })
                } else {
                    let node = graph.node(assignment.target);
                    let value = node.map(|n| n.descriptor.saved_values(&n.params)[assignment.param]);
                    let range = node.and_then(|n| stored_range(n, assignment.param));
                    (param.clone(), Stored { value, range })
                };
                let label = graph
                    .node(assignment.target)
                    .and_then(|n| n.descriptor.params.get(assignment.param))
                    .map_or(inner_param.clone(), |spec| spec.label.to_string());
                self.expose_on(&key, member.last(), &inner_param, &label, None, stored);
            } else if source_inside && !target_inside && self.level_of(assignment.target) == *level {
                let source_path = self.path_of(assignment.source.module);
                let member = instance_path.child(source_path.0[instance_path.0.len()]);
                let output = crate::port_name(graph, assignment.source);
                let end = if self.instances.contains_key(&member) {
                    match self.port_for_output(graph, &member, &(assignment.source.module, output)) {
                        Some(port) => (member.last(), port),
                        None => continue,
                    }
                } else {
                    (member.last(), output)
                };
                self.output_port_for(graph, &instance_path, &end, &mut outputs, &mut output_tiles)
                    .map_err(|e| e.to_string())?;
            }
        }

        // What the enclosing definition exposed on a member now goes through
        // the new sub-patch: exposed there under the same name, and the
        // enclosing entry points at the new instance.
        if let Some(outer_key) = self.instances.get(level).map(|i| i.key.clone()) {
            let outer = self.definitions[&outer_key].clone();
            for entry in outer.exposed.iter().filter(|e| members.contains(&e.module)) {
                let stored = outer
                    .defaults
                    .get(&(entry.module, entry.param.clone()))
                    .cloned()
                    .unwrap_or_default();
                let name = self.expose_on(&key, entry.module, &entry.param, &entry.name, entry.range, stored);
                let outer = self.definitions.get_mut(&outer_key).expect("the enclosing definition");
                if let Some(e) = outer.exposed.iter_mut().find(|e| e.name == entry.name) {
                    e.module = ilid;
                    e.param = name.clone();
                }
                outer.defaults.remove(&(entry.module, entry.param.clone()));
                outer.defaults.insert((ilid, name), Stored::default());
            }
        } else if level.is_root() {
            // The top level's own surface (a library file opened on its
            // own) goes through the new sub-patch the same way. An entry
            // the member cannot carry is dropped.
            let entries: Vec<Exposed> = self
                .top_exposed
                .iter()
                .filter(|e| members.contains(&e.module))
                .cloned()
                .collect();
            for entry in entries {
                let carried = self.expose(graph, &instance_path, entry.module, &entry.param).ok();
                let index = self
                    .top_exposed
                    .iter()
                    .position(|e| e.module == entry.module && e.param == entry.param)
                    .expect("the entry is still there");
                match carried {
                    Some(name) => {
                        self.top_exposed[index].module = ilid;
                        self.top_exposed[index].param = name;
                    }
                    None => {
                        self.top_exposed.remove(index);
                    }
                }
            }
        }

        Ok(Collapsed {
            instance: instance_id,
            inputs: input_tiles,
            outputs: output_tiles,
            members: member_ids,
        })
    }

    /// The output port of the new definition at `instance` that `from`
    /// (an end inside it) feeds, made when `from` has none yet. Returns the
    /// port's level id and name.
    fn output_port_for(
        &mut self,
        graph: &Graph,
        instance: &ModulePath,
        from: &End,
        made: &mut BTreeMap<End, (u32, String)>,
        tiles: &mut Vec<ModuleId>,
    ) -> Result<(u32, String), PatchError> {
        if let Some(port) = made.get(from) {
            return Ok(port.clone());
        }
        let label = self.inner_port_label(graph, instance, from, Direction::Output);
        let id = self.add_port(instance, Direction::Output, &label)?;
        tiles.push(id);
        let path = self.ids.path_of(id).cloned().expect("just added");
        let port = (path.last(), self.ports[&path].name.clone());
        made.insert(from.clone(), port.clone());
        self.links.insert(Link {
            level: instance.clone(),
            to: (port.0, "in".to_string()),
            from: from.clone(),
        });
        Ok(port)
    }

    /// Exposes `param` of the member `lid` on the definition `key`, or
    /// finds it already exposed, and returns the exposed name: `label` made
    /// unique among the definition's names. `stored` is the member's own
    /// value and range, kept for the file.
    fn expose_on(
        &mut self,
        key: &str,
        lid: u32,
        param: &str,
        label: &str,
        range: Option<(f32, f32)>,
        stored: Stored,
    ) -> String {
        let definition = self.definitions.get_mut(key).expect("the definition exists");
        if let Some(entry) = definition.exposed.iter().find(|e| e.module == lid && e.param == param) {
            return entry.name.clone();
        }
        let name = unique_name(label, |name| definition.exposed.iter().any(|e| e.name == name));
        definition.exposed.push(Exposed {
            name: name.clone(),
            module: lid,
            param: param.to_string(),
            range,
        });
        definition.defaults.insert((lid, param.to_string()), stored);
        name
    }

    /// The label a new port takes from the inner end it stands for
    /// (Collapsing, point 3): a module's port label, or a nested
    /// instance's port name.
    fn inner_port_label(&self, graph: &Graph, level: &ModulePath, end: &End, direction: Direction) -> String {
        let path = level.child(end.0);
        if self.instances.contains_key(&path) {
            return end.1.clone();
        }
        let Some(node) = self.ids.get(&path).and_then(|id| graph.node(id)) else {
            return end.1.clone();
        };
        let spec = match direction {
            Direction::Input => node.input_index(&end.1).and_then(|i| node.descriptor.inputs.get(i)),
            Direction::Output => node.output_index(&end.1).and_then(|i| node.descriptor.outputs.get(i)),
        };
        spec.map_or_else(|| end.1.clone(), |spec| spec.label.to_string())
    }

    /// Puts an instance's inside back into its level (the reverse of
    /// collapsing). Cables through its ports become cables of the level.
    /// Nothing in the graph changes; the caller places the tiles and
    /// syncs.
    pub fn dissolve(&mut self, graph: &Graph, instance: ModuleId) -> Result<Dissolved, String> {
        let Some(instance_path) = self.ids.path_of(instance).cloned() else {
            return Err("Select a sub-patch to dissolve".to_string());
        };
        let Some(record) = self.instances.get(&instance_path).cloned() else {
            return Err("Select a sub-patch to dissolve".to_string());
        };
        let name = self
            .definitions
            .get(&record.key)
            .map(|d| d.metadata.name.clone())
            .unwrap_or_default();
        let level = instance_path.parent().unwrap_or_else(ModulePath::root);
        let ilid = instance_path.last();
        if self.slots.keys().any(|p| p.parent().as_ref() == Some(&instance_path)) {
            return Err(NO_SLOT_IN_DISSOLVE.to_string());
        }

        // The members and the ports of the inside.
        let mut members: Vec<ModulePath> = Vec::new();
        let mut ports: Vec<(ModulePath, Port)> = Vec::new();
        for (path, id) in self.ids.paths() {
            if path.parent().as_ref() != Some(&instance_path) {
                continue;
            }
            if let Some(port) = self.ports.get(path) {
                ports.push((path.clone(), port.clone()));
            } else if self.instances.contains_key(path) || graph.node(id).is_some() {
                members.push(path.clone());
            }
        }

        // The cables each port joins, in the ids of the two levels.
        let outside_source = |port: &Port| -> Option<End> {
            self.link_into(&level, &(ilid, port.name.clone()))
                .map(|l| l.from.clone())
        };
        let outside_dests = |port: &Port| -> Vec<End> {
            self.links_from(&level, &(ilid, port.name.clone()))
                .map(|l| l.to.clone())
                .collect()
        };
        let mut joins: Vec<(End, End)> = Vec::new();
        for (path, port) in &ports {
            match port.direction {
                Direction::Input => {
                    let Some(source) = outside_source(port) else { continue };
                    for link in self.links_from(&instance_path, &(path.last(), "out".to_string())) {
                        // Straight into an output port: a pass-through.
                        if let Some(out) = ports
                            .iter()
                            .find(|(p, q)| p.last() == link.to.0 && q.direction == Direction::Output)
                        {
                            for dest in outside_dests(&out.1) {
                                joins.push((source.clone(), dest));
                            }
                        } else {
                            joins.push((source.clone(), link.to.clone()));
                        }
                    }
                }
                Direction::Output => {
                    let Some(link) = self.link_into(&instance_path, &(path.last(), "in".to_string())) else {
                        continue;
                    };
                    if ports
                        .iter()
                        .any(|(p, q)| p.last() == link.from.0 && q.direction == Direction::Input)
                    {
                        continue; // Handled from the input side.
                    }
                    for dest in outside_dests(port) {
                        joins.push((link.from.clone(), dest));
                    }
                }
            }
        }
        let inside_links: Vec<Link> = self
            .links_at(&instance_path)
            .filter(|link| {
                !ports
                    .iter()
                    .any(|(p, _)| p.last() == link.from.0 || p.last() == link.to.0)
            })
            .cloned()
            .collect();

        // New level ids: kept when the level never had them, else the next.
        let mut renamed: BTreeMap<u32, u32> = BTreeMap::new();
        let mut next = self.next_level_id(&level);
        for path in &members {
            let lid = path.last();
            let new = if level.is_root() {
                self.ids.get(path).map_or(lid, |id| id.0)
            } else if self.ids.get(&level.child(lid)).is_none() && !renamed.values().any(|v| *v == lid) {
                lid
            } else {
                next += 1;
                next - 1
            };
            renamed.insert(lid, new);
        }
        let rename = |end: &End| -> End { (renamed.get(&end.0).copied().unwrap_or(end.0), end.1.clone()) };

        // Take the ports, their links, and the instance out.
        let mut removed = vec![instance];
        for (path, _) in &ports {
            if let Some(id) = self.ids.get(path) {
                removed.push(id);
            }
            self.ports.remove(path);
        }
        self.links.retain(|link| link.level != instance_path);
        self.links
            .retain(|link| !(link.level == level && (link.from.0 == ilid || link.to.0 == ilid)));
        self.instances.remove(&instance_path);
        let inner = self.definitions.remove(&record.key);

        // Move the members out.
        let mut member_ids = Vec::new();
        for path in &members {
            if let Some(id) = self.ids.get(path) {
                member_ids.push(id);
            }
            let new = level.child(renamed[&path.last()]);
            self.rename_prefix(path, &new).map_err(|e| e.to_string())?;
        }
        for link in inside_links {
            self.links.insert(Link {
                level: level.clone(),
                to: rename(&link.to),
                from: rename(&link.from),
            });
        }
        // What the enclosing definition exposed through the dissolved
        // sub-patch now names the member itself; at the top level, the
        // patch's own surface does the same.
        if let (Some(inner), true) = (&inner, level.is_root()) {
            for entry in self.top_exposed.iter_mut().filter(|e| e.module == ilid) {
                let Some(through) = inner.exposed.iter().find(|e| e.name == entry.param) else {
                    continue;
                };
                entry.module = renamed.get(&through.module).copied().unwrap_or(through.module);
                entry.param = through.param.clone();
                entry.range = entry.range.or(through.range);
            }
            self.top_exposed.retain(|e| e.module != ilid);
        }
        if let (Some(inner), Some(outer_key)) = (inner, self.instances.get(&level).map(|i| i.key.clone())) {
            let outer = self.definitions.get_mut(&outer_key).expect("the enclosing definition");
            let mut moved = Vec::new();
            for entry in outer.exposed.iter_mut().filter(|e| e.module == ilid) {
                let Some(through) = inner.exposed.iter().find(|e| e.name == entry.param) else {
                    continue;
                };
                let stored = inner
                    .defaults
                    .get(&(through.module, through.param.clone()))
                    .cloned()
                    .unwrap_or_default();
                entry.module = renamed.get(&through.module).copied().unwrap_or(through.module);
                entry.param = through.param.clone();
                entry.range = entry.range.or(through.range);
                moved.push(((entry.module, entry.param.clone()), stored));
            }
            outer.exposed.retain(|e| e.module != ilid);
            outer.defaults.retain(|(member, _), _| *member != ilid);
            outer.defaults.extend(moved);
        }
        for (from, to) in joins {
            let (from, to) = (rename(&from), rename(&to));
            if self.is_module_end(graph, &level, &from) && self.is_module_end(graph, &level, &to) {
                // The graph holds it already; it becomes a direct cable.
                continue;
            }
            self.links.insert(Link {
                level: level.clone(),
                to,
                from,
            });
        }
        Ok(Dissolved {
            name,
            members: member_ids,
            removed,
        })
    }

    /// True when an end of `level` is a module of the graph.
    fn is_module_end(&self, graph: &Graph, level: &ModulePath, end: &End) -> bool {
        self.id_at(level, end.0).is_some_and(|id| graph.node(id).is_some())
    }

    /// Inserts a patch file as a new sub-patch tile at `level`, as a copy
    /// or, with `library`, linked to that path (sub-patches.md, Library).
    /// The file becomes a definition ([`SubPatch::from_patch`]): its
    /// patch-wide modules are left out and their cables become ports. Its
    /// modules are added to the controller with ids from the table, and
    /// the tile, its ports, and the definitions it brings are recorded.
    /// Refused, with nothing added, when the file does not flatten, or
    /// when `library` is the file an enclosing linked sub-patch follows,
    /// which would make that file contain itself (Patch file). The caller
    /// syncs and commits; on a later refusal it removes `modules`.
    pub fn insert(
        &mut self,
        controller: &mut Controller,
        level: &ModulePath,
        file: &Patch,
        library: Option<&str>,
    ) -> Result<Inserted, PatchError> {
        if let Some(path) = library {
            for n in 1..=level.0.len() {
                let prefix = level.prefix(n);
                if self
                    .definition_at(&prefix)
                    .is_some_and(|d| d.library.as_deref() == Some(path))
                {
                    let mut loop_path = self.path_names(level).split_off(n - 1);
                    loop_path.push(file.metadata.name.clone());
                    return Err(PatchError::SubPatchLoop { path: loop_path });
                }
            }
        }
        let (mut definition, left_out) = SubPatch::from_patch(file, controller.registry(), library);
        let name = definition.metadata.name.clone();
        let mut nested = file.subpatches.clone();
        let key = unique_name(&name.to_lowercase(), |key| {
            self.definitions.contains_key(key) || nested.contains_key(key)
        });
        rekey(&mut definition, &mut nested, |candidate| {
            candidate == key || self.definitions.contains_key(candidate)
        });
        self.adopt(controller.graph());
        let (instance_path, instance_id) = self.new_module(level)?;

        // The file as a patch of one instance, flattened on a scratch table
        // whose paths start at the instance's level id.
        let mut temp = Patch::new("");
        temp.subpatches = nested;
        temp.subpatches.insert(key.clone(), definition);
        temp.modules.push(PatchModule {
            id: instance_path.last(),
            kind: SUBPATCH_KIND.to_string(),
            subpatch: Some(key),
            slot: None,
            label: None,
            params: BTreeMap::new(),
            position: None,
            size: None,
            ranges: BTreeMap::new(),
        });
        let flat = temp.flatten(controller.registry(), &mut FlatIds::new())?;
        let mut real: BTreeMap<ModuleId, ModuleId> = BTreeMap::new();
        for module in &flat.modules {
            let mut path = level.clone();
            path.0.extend_from_slice(&module.path.0);
            real.insert(module.id, self.ids.id_of(&path)?);
        }
        let added: Vec<ModuleId> = real.values().copied().collect();
        if let Err(error) = flat.add_to(controller, &|id| real.get(&id).copied().unwrap_or(id)) {
            for id in &added {
                let _ = controller.remove_module(*id);
            }
            return Err(error);
        }
        if let Err(error) = self.read_level(&temp.modules, &[], level, &temp.subpatches, controller.registry()) {
            for id in &added {
                let _ = controller.remove_module(*id);
            }
            return Err(error);
        }
        let mut layouts = Vec::new();
        for (path, instance) in &self.instances {
            if !path.starts_with(&instance_path) {
                continue;
            }
            let Some(definition) = temp.subpatches.get(&instance.key) else {
                continue;
            };
            for module in &definition.modules {
                if let Some(id) = self.ids.get(&path.child(module.id)) {
                    layouts.push((
                        id,
                        Layout {
                            position: module.position,
                            size: module.size,
                        },
                    ));
                }
            }
        }
        Ok(Inserted {
            instance: instance_id,
            name,
            modules: added,
            layouts,
            left_out,
            notes: flat.warnings,
        })
    }

    /// The library path the sub-patch tile `instance` follows, if it is
    /// linked.
    pub fn library_of(&self, instance: ModuleId) -> Option<&str> {
        self.definition_of(instance)?.library.as_deref()
    }

    /// True when any sub-patch in the patch follows a library file.
    pub fn has_links(&self) -> bool {
        self.definitions.values().any(|d| d.library.is_some())
    }

    /// Unlink: makes the linked sub-patch tile `instance` a plain copy
    /// (sub-patches.md, Linked library items). Returns its name. Refused
    /// for a tile that is not a linked sub-patch.
    pub fn unlink(&mut self, instance: ModuleId) -> Result<String, String> {
        let key = self
            .ids
            .path_of(instance)
            .and_then(|path| self.instances.get(path))
            .map(|i| i.key.clone())
            .ok_or_else(|| "select a sub-patch to unlink".to_string())?;
        let definition = self
            .definitions
            .get_mut(&key)
            .expect("every instance has its definition");
        if definition.library.take().is_none() {
            return Err(format!("{} does not follow the library", definition.metadata.name));
        }
        Ok(definition.metadata.name.clone())
    }

    /// Takes the link off the definition `key`, for the first edit inside
    /// a linked sub-patch. Returns its name, or `None` when it was not
    /// linked.
    pub fn unlink_key(&mut self, key: &str) -> Option<String> {
        let definition = self.definitions.get_mut(key)?;
        definition.library.take()?;
        Some(definition.metadata.name.clone())
    }

    /// What each linked definition and everything nested in it would be
    /// saved as, by key: the text an edit inside changes, and the refresh
    /// on load compares against the library file. Empty with no link, at
    /// no cost.
    pub fn linked_fingerprints(&self, graph: &Graph, layout: impl Fn(u32) -> Layout) -> BTreeMap<String, String> {
        if !self.has_links() {
            return BTreeMap::new();
        }
        let patch = self.unflatten(graph, layout);
        patch
            .subpatches
            .iter()
            .filter(|(_, definition)| definition.library.is_some())
            .map(|(key, definition)| (key.clone(), crate::library::fingerprint(definition, &patch.subpatches)))
            .collect()
    }

    /// The sub-patch tile `instance` as a library file: its definition at
    /// the top level, with the definitions nested in it (sub-patches.md,
    /// Library). `None` for a tile that is not a sub-patch.
    pub fn definition_file(&self, graph: &Graph, layout: impl Fn(u32) -> Layout, instance: ModuleId) -> Option<Patch> {
        let key = &self.instances.get(self.ids.path_of(instance)?)?.key;
        let patch = self.unflatten(graph, layout);
        let definition = patch.subpatches.get(key)?;
        Some(Patch::from_definition(definition, &patch.subpatches))
    }

    /// The definition key of the instance shown at `level`. `None` at the
    /// top.
    pub fn key_at(&self, level: &ModulePath) -> Option<&str> {
        self.instances.get(level).map(|i| i.key.as_str())
    }

    /// The keys of the definitions the one at `key` uses, through any
    /// depth, as a file written from it would carry them.
    pub fn nested_keys(&self, graph: &Graph, key: &str) -> Vec<String> {
        let patch = self.unflatten(graph, |_| Layout::default());
        patch
            .subpatches
            .get(key)
            .map(|definition| reachable_keys(definition, &patch.subpatches))
            .unwrap_or_default()
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
        *self = snapshot.clone();
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

/// Puts the live assignments on each parameter in the order `canonical`
/// lists them, which is the order the file loads them: each instance's
/// inner ones first, then the level's own (flattening rule 11). The engine
/// sums assignments on one parameter in id order, so an assignment whose
/// id is below the one before it in that order gets a fresh id; every
/// other one keeps its id. Two assignments from one source onto one
/// parameter, which only a hand-written file makes, are matched to the
/// file's entries in id order, so a settled order stays settled. A live
/// assignment the file does not hold is left alone. Only the graph
/// changes, so a full command queue cannot stop this part-way; the
/// caller's commit carries it to the engine. Returns the ids that changed,
/// as (old, new).
fn order_assignments(
    controller: &mut Controller,
    canonical: &[PatchModulation],
) -> Result<Vec<(AssignmentId, AssignmentId)>, loom_core::LoomError> {
    let graph = controller.graph();
    let mut live: BTreeMap<(End, End), Vec<AssignmentId>> = BTreeMap::new();
    for (id, a) in graph.assignments() {
        let source = (a.source.module.0, crate::port_name(graph, a.source));
        let target = (a.target.0, crate::param_name(graph, a.target, a.param));
        live.entry((source, target)).or_default().push(id);
    }
    let mut groups: BTreeMap<End, Vec<AssignmentId>> = BTreeMap::new();
    for entry in canonical {
        let ids = live.get_mut(&(entry.source.clone(), entry.target.clone()));
        if let Some(id) = ids.and_then(|ids| (!ids.is_empty()).then(|| ids.remove(0))) {
            groups.entry(entry.target.clone()).or_default().push(id);
        }
    }
    let mut renamed = Vec::new();
    for ids in groups.into_values() {
        let mut last: Option<AssignmentId> = None;
        for id in ids {
            let placed = match last {
                Some(last) if id <= last => {
                    let fresh = controller.renumber_assignment(id)?;
                    renamed.push((id, fresh));
                    fresh
                }
                _ => id,
            };
            last = Some(placed);
        }
    }
    Ok(renamed)
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
