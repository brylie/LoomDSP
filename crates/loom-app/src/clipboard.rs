//! Copy, cut, paste, and duplicate (`docs/design/clipboard.md`).
//!
//! A [`Group`] is the tiles being copied, with the cables and assignments
//! that go with them: modules, and sub-patch tiles with their definitions
//! (`docs/design/sub-patches.md`, Copies and editing). Copy turns the
//! selection into a group and writes it as patch text. Paste reads patch
//! text back into a group. Duplicate takes a group straight from the
//! graph, so it can also keep the incoming cables and assignments, which
//! patch text cannot hold.
//!
//! Ids in a group only link its parts. A paste gives every tile a new id
//! from the hierarchy's table, inserts each sub-patch with a definition of
//! its own, and remaps the cables and assignments to the new ids. Ports
//! are named, not numbered, because a sub-patch tile's ports are its
//! definition's and a module's are its descriptor's.

use loom_canvas::GRID;
use loom_core::{AssignmentId, Controller, Graph, ModuleId, ModuleNames, ModuleRole, OutputRef, Polarity, Registry};
use loom_patch::{
    End, Hierarchy, INPUT_KIND, Layout, ModulePath, OUTPUT_KIND, Patch, PatchCable, PatchError, PatchModulation,
    PatchModule, PatchPolarity, ResolveError, ResolveOptions, ResolvedModule, SUBPATCH_KIND, SubPatch, Tile,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// How far a duplicate lands from its original, right and down, in world
/// units: 2 grid units.
pub const DUPLICATE_OFFSET: f32 = 2.0 * GRID;

/// The status bar text when the clipboard holds nothing to paste.
pub const NOTHING_TO_PASTE: &str = "The clipboard holds no LoomDSP modules";

/// A trailing RON comment retains placeholder ports that have no copied link.
const PORTS_COMMENT: &str = "// LoomDSP placeholder ports: ";

/// Clipboard-only names kept even when their cables are outside the selection.
#[derive(Debug, Serialize, Deserialize)]
struct PlaceholderPorts {
    inputs: Vec<String>,
    outputs: Vec<String>,
    parameters: Vec<String>,
}

/// A sub-patch tile in a group: its definition is in the group's table.
#[derive(Clone, Debug)]
struct InstanceMember {
    /// Its id in the group.
    id: u32,
    /// Its definition's key in the group's table.
    key: String,
    /// The tile's own label, if the file gave one.
    label: Option<String>,
    /// Its exposed values, by exposed name.
    params: BTreeMap<String, f32>,
    layout: Layout,
}

/// One tile in a group.
#[derive(Clone, Debug)]
enum Member {
    /// A module, in the same resolved form a patch loads with, so the
    /// clipboard holds no name lookups of its own (#651).
    Module(ResolvedModule),
    /// A sub-patch tile.
    Instance(InstanceMember),
}

impl Member {
    fn id(&self) -> u32 {
        match self {
            Self::Module(module) => module.id,
            Self::Instance(instance) => instance.id,
        }
    }

    fn layout(&self) -> &Layout {
        match self {
            Self::Module(module) => &module.layout,
            Self::Instance(instance) => &instance.layout,
        }
    }

    fn layout_mut(&mut self) -> &mut Layout {
        match self {
            Self::Module(module) => &mut module.layout,
            Self::Instance(instance) => &mut instance.layout,
        }
    }
}

/// Where a cable or an assignment starts.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    /// An output of a tile in the group: its id and the output's name.
    Inside(u32, String),
    /// An output outside the group, as the level names it: a module's, a
    /// sub-patch tile's, or a port tile's (duplicate only).
    Outside(End),
}

/// A cable into a tile of the group.
#[derive(Clone, Debug)]
struct Cable {
    from: Source,
    /// Tile id in the group, and input name.
    to: (u32, String),
}

/// A modulation assignment onto a parameter of the group: a module's, or
/// an exposed parameter of a sub-patch tile.
#[derive(Clone, Debug)]
struct Modulation {
    source: Source,
    /// Tile id in the group, and parameter or exposed name.
    target: (u32, String),
    depth: f32,
    polarity: Option<Polarity>,
    enabled: bool,
}

/// Tiles with the cables and assignments that go with them.
#[derive(Clone, Debug, Default)]
pub struct Group {
    /// In id order, which is the order new ids are given in (rule 2).
    members: Vec<Member>,
    /// The sub-patch definitions the tiles use, with the ones nested in
    /// them, by key (sub-patches.md, Copies and editing).
    definitions: BTreeMap<String, SubPatch>,
    cables: Vec<Cable>,
    /// In the order they are added back, which decides how assignments on
    /// one parameter are summed (rule 3).
    modulation: Vec<Modulation>,
    /// The patch-wide modules the text held, which a paste leaves out
    /// (sub-patches.md, rule 8), as the player knows them.
    left_out: Vec<String>,
}

/// What [`Group::add`] made.
pub struct Added {
    /// The new tiles' ids, in the group's order.
    pub ids: Vec<u32>,
    /// Assignments the sync gave new ids, as (old, new).
    pub renamed: Vec<(AssignmentId, AssignmentId)>,
}

impl Group {
    /// Includes the selected tiles' saved dimensions in a copied group.
    pub fn with_sizes(mut self, sizes: &BTreeMap<u32, (u8, u8)>) -> Self {
        for member in &mut self.members {
            let id = member.id();
            member.layout_mut().size = sizes.get(&id).copied();
        }
        self
    }

    /// Takes the selected tiles of `level` from the graph and the
    /// hierarchy, with their inside cables and assignments: modules, and
    /// sub-patch tiles with their definitions, as the file would write
    /// them. With `incoming`, cables and assignments from outside the
    /// selection into it come too (duplicate). Outgoing ones and MIDI
    /// mappings never do. The Transport is left out: a patch has exactly
    /// one, so a copy of it could never be added (clock.md). A port tile is
    /// left out too: the clipboard holds no ports.
    pub fn from_selection(
        graph: &Graph,
        hierarchy: &Hierarchy,
        level: &ModulePath,
        positions: &BTreeMap<u32, (f32, f32)>,
        selection: &BTreeSet<u32>,
        incoming: bool,
    ) -> Self {
        let tile_of = |id: u32| hierarchy.tile(graph, ModuleId(id));
        let selected: BTreeSet<u32> = selection
            .iter()
            .copied()
            .filter(|&id| hierarchy.level_of(ModuleId(id)) == *level)
            .filter(|&id| match tile_of(id) {
                // A patch-wide module is left out, also as a placeholder for
                // one this build does not have (sub-patches.md, rule 8).
                Some(Tile::Module) => graph.node(ModuleId(id)).is_some_and(|node| {
                    let kind = node
                        .unknown
                        .as_ref()
                        .map_or_else(|| node.descriptor.kind(), |unknown| unknown.kind.clone());
                    !loom_patch::library::is_patch_wide(&kind)
                }),
                Some(Tile::Instance(_)) => true,
                Some(Tile::Port(..)) | None => false,
            })
            .collect();
        let selected = &selected;
        let layout = |id: ModuleId| Layout {
            position: positions.get(&id.0).copied(),
            size: None,
        };

        // The level as the file writes it, for the sub-patch tiles.
        let written = hierarchy.unflatten(graph, |id| layout(ModuleId(id)));
        let level_modules: &[PatchModule] = match hierarchy.key_at(level) {
            Some(key) => written.subpatches.get(key).map_or(&[], |d| &d.modules),
            None => &written.modules,
        };
        let mut members: Vec<Member> = Vec::new();
        let mut definitions = BTreeMap::new();
        for (id, node) in graph.nodes().filter(|(id, _)| selected.contains(&id.0)) {
            members.push(Member::Module(ResolvedModule::from_node(id, node, layout(id))));
        }
        for (id, tile) in hierarchy.structural_tiles(level) {
            let Tile::Instance(key) = tile else { continue };
            if !selected.contains(&id.0) {
                continue;
            }
            let lid = hierarchy.path_of(id).last();
            let Some(module) = level_modules.iter().find(|m| m.id == lid) else {
                continue;
            };
            members.push(Member::Instance(InstanceMember {
                id: id.0,
                key: key.clone(),
                label: module.label.clone(),
                params: module.params.clone(),
                layout: layout(id),
            }));
            if let Some(definition) = written.subpatches.get(&key) {
                for nested in loom_patch::reachable_keys(definition, &written.subpatches) {
                    if let Some(nested_definition) = written.subpatches.get(&nested) {
                        definitions.insert(nested, nested_definition.clone());
                    }
                }
                definitions.insert(key, definition.clone());
            }
        }
        members.sort_by_key(Member::id);

        // The cables the level shows, direct and through ports, in its ids.
        let flat = |lid: u32| hierarchy.id_at(level, lid);
        let source = |from: &End| -> Option<Source> {
            match flat(from.0) {
                Some(id) if selected.contains(&id.0) => Some(Source::Inside(id.0, from.1.clone())),
                _ => incoming.then(|| Source::Outside(from.clone())),
            }
        };
        let cables = hierarchy
            .level_cables(graph, level)
            .into_iter()
            .filter_map(|cable| {
                let to = flat(cable.to.0).filter(|id| selected.contains(&id.0))?;
                Some(Cable {
                    from: source(&cable.from)?,
                    to: (to.0, cable.to.1),
                })
            })
            .collect();

        // Assignments come in id order, the order the engine sums them in.
        // Only the level's own come: one onto a parameter inside a selected
        // sub-patch tile is written onto the tile's exposed name, and one
        // from inside a tile through its output port, but one with both
        // ends inside the same tile is the definition's, and the copied
        // definition carries it already.
        let written_at = |source: ModuleId, target: ModuleId| -> ModulePath {
            let (source_level, target_level) = (hierarchy.level_of(source), hierarchy.level_of(target));
            if target_level.starts_with(&source_level) {
                source_level
            } else {
                target_level
            }
        };
        let target_of = |target: ModuleId, index: usize| -> Option<(u32, String)> {
            let name = graph.node(target)?.parameter_name(index)?.to_string();
            if selected.contains(&target.0) {
                return Some((target.0, name));
            }
            let (instance, exposed) = hierarchy.surface_for(level, &(target, name))?;
            if !selected.contains(&instance.0) {
                return None;
            }
            let name = hierarchy.definition_of(instance)?.exposed.get(exposed)?.name.clone();
            Some((instance.0, name))
        };
        let source_of = |output: OutputRef| -> Option<Source> {
            let name = loom_core::ModuleNames::output_name(graph.node(output.module)?, output.port)?.to_string();
            let source_level = hierarchy.level_of(output.module);
            let end = if source_level == *level {
                (hierarchy.path_of(output.module).last(), name)
            } else if source_level.starts_with(level) {
                // Inside a sub-patch tile of this level: through its port.
                let instance = source_level.prefix(level.0.len() + 1);
                let port = hierarchy.port_for_output(graph, &instance, &(output.module, name))?;
                (instance.last(), port)
            } else {
                return None;
            };
            source(&end)
        };
        let modulation = graph
            .assignments()
            .filter(|(_, a)| written_at(a.source.module, a.target) == *level)
            .filter_map(|(_, a)| {
                Some(Modulation {
                    target: target_of(a.target, a.param)?,
                    source: source_of(a.source)?,
                    depth: a.depth,
                    polarity: a.polarity,
                    enabled: a.enabled,
                })
            })
            .collect();
        Self {
            members,
            definitions,
            cables,
            modulation,
            left_out: Vec::new(),
        }
    }

    /// Reads a group from patch text. MIDI mappings in it are ignored, and
    /// so are its patch-wide modules, which a paste leaves out with their
    /// cables (sub-patches.md, rule 8). Fails, with the text for the status
    /// bar, if the text is not a patch or refers to something that does
    /// not exist.
    pub fn from_text(text: &str, registry: &Registry) -> Result<Self, String> {
        let patch = match Patch::from_ron(text) {
            Ok(patch) => patch,
            Err(error @ PatchError::TooNew { .. }) => return Err(format!("Cannot paste: {error}")),
            Err(_) => return Err(NOTHING_TO_PASTE.to_string()),
        };
        if patch.modules.is_empty() {
            return Err(NOTHING_TO_PASTE.to_string());
        }
        let ports = text
            .lines()
            .find_map(|line| line.strip_prefix(PORTS_COMMENT))
            .map(serde_json::from_str::<BTreeMap<u32, PlaceholderPorts>>)
            .transpose()
            .map_err(|_| "Cannot paste: invalid placeholder ports".to_string())?
            .unwrap_or_default();
        Self::from_patch(&patch, registry, &ports).map_err(|why| format!("Cannot paste: {why}"))
    }

    fn from_patch(patch: &Patch, registry: &Registry, ports: &BTreeMap<u32, PlaceholderPorts>) -> Result<Self, String> {
        let (dropped, kept): (Vec<&PatchModule>, Vec<&PatchModule>) = patch
            .modules
            .iter()
            .partition(|m| loom_patch::library::is_patch_wide(&m.kind));
        let dropped_ids: BTreeSet<u32> = dropped.iter().map(|m| m.id).collect();
        let left_out: Vec<String> = dropped
            .iter()
            .map(|m| {
                registry
                    .resolve(&m.kind)
                    .map_or_else(|_| m.kind.clone(), |d| d.name.to_string())
            })
            .collect();
        if kept.is_empty() {
            let verb = if left_out.len() == 1 { "was" } else { "were" };
            return Err(format!(
                "only the {} {verb} copied, and a patch has one, at the top level",
                left_out.join(" and ")
            ));
        }
        let mut members = Vec::with_capacity(kept.len());
        let mut warnings = Vec::new();
        let mut ids = BTreeSet::new();
        for module in kept {
            if !ids.insert(module.id) {
                return Err(format!("two modules have id {}", module.id));
            }
            if let Some(key) = &module.subpatch {
                if !patch.subpatches.contains_key(key) {
                    return Err(format!("there is no sub-patch `{key}`"));
                }
                members.push(Member::Instance(InstanceMember {
                    id: module.id,
                    key: key.clone(),
                    label: module.label.clone(),
                    params: module.params.clone(),
                    layout: Layout {
                        position: module.position,
                        size: module.size,
                    },
                }));
                continue;
            }
            let mut member = patch
                .resolve_module(module, registry, ResolveOptions::CLIPBOARD, &mut warnings)
                .map_err(|error| match error {
                    ResolveError::UnknownParam { module_name, param, .. } => {
                        format!("{module_name} has no parameter `{param}`")
                    }
                    ResolveError::Patch(error) => error.to_string(),
                })?;
            if let (Some(unknown), Some(saved)) = (&mut member.unknown, ports.get(&member.id)) {
                for (items, preserved) in [
                    (&mut unknown.inputs, &saved.inputs),
                    (&mut unknown.outputs, &saved.outputs),
                    (&mut unknown.parameters, &saved.parameters),
                ] {
                    let mut merged = preserved.clone();
                    for item in items.iter() {
                        if !merged.contains(item) {
                            merged.push(item.clone());
                        }
                    }
                    *items = merged;
                }
            }
            if member.unknown.as_ref().is_some_and(|unknown| unknown.inputs.len() > 64) {
                return Err("a placeholder cannot have more than 64 inputs".into());
            }
            members.push(Member::Module(member));
        }
        members.sort_by_key(Member::id);
        let group = Self {
            members,
            definitions: patch.subpatches.clone(),
            cables: Vec::new(),
            modulation: Vec::new(),
            left_out,
        };
        let touches_dropped = |a: u32, b: u32| dropped_ids.contains(&a) || dropped_ids.contains(&b);
        let output = |(id, name): &(u32, String)| -> Result<Source, String> {
            if !group.has_output(*id, name) {
                return Err(format!("{} has no output `{name}`", group.name_of(*id)?));
            }
            Ok(Source::Inside(*id, name.clone()))
        };
        let cables = patch
            .cables
            .iter()
            .filter(|cable| !touches_dropped(cable.from.0, cable.to.0))
            .map(|cable| {
                if !group.has_input(cable.to.0, &cable.to.1) {
                    return Err(format!("{} has no input `{}`", group.name_of(cable.to.0)?, cable.to.1));
                }
                Ok(Cable {
                    from: output(&cable.from)?,
                    to: cable.to.clone(),
                })
            })
            .collect::<Result<_, String>>()?;
        let modulation = patch
            .modulation
            .iter()
            .filter(|a| !touches_dropped(a.source.0, a.target.0))
            .map(|assignment| {
                if !group.has_param(assignment.target.0, &assignment.target.1) {
                    return Err(format!(
                        "{} has no parameter `{}`",
                        group.name_of(assignment.target.0)?,
                        assignment.target.1
                    ));
                }
                Ok(Modulation {
                    source: output(&assignment.source)?,
                    target: assignment.target.clone(),
                    depth: assignment.depth,
                    polarity: assignment.polarity.map(Polarity::from),
                    enabled: assignment.enabled,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            cables,
            modulation,
            ..group
        })
    }

    /// Writes the group as patch text: a whole patch with a version and an
    /// empty name, holding the tiles, their definitions, the inside cables,
    /// and the inside assignments. Outside cables and assignments are left
    /// out.
    pub fn to_text(&self) -> String {
        let mut patch = Patch::new("");
        patch.subpatches = self.definitions.clone();
        for member in &self.members {
            let mut module = match member {
                Member::Module(module) => module.write(),
                Member::Instance(instance) => PatchModule {
                    id: instance.id,
                    kind: SUBPATCH_KIND.to_string(),
                    subpatch: Some(instance.key.clone()),
                    label: instance.label.clone(),
                    params: instance.params.clone(),
                    position: instance.layout.position,
                    size: instance.layout.size,
                    ranges: BTreeMap::new(),
                },
            };
            module.position = module.position.map(|(x, y)| (x.round(), y.round()));
            patch.modules.push(module);
        }
        for cable in &self.cables {
            if let Source::Inside(id, port) = &cable.from {
                patch.cables.push(PatchCable {
                    from: (*id, port.clone()),
                    to: cable.to.clone(),
                });
            }
        }
        for assignment in &self.modulation {
            if let Source::Inside(id, port) = &assignment.source {
                patch.modulation.push(PatchModulation {
                    source: (*id, port.clone()),
                    target: assignment.target.clone(),
                    depth: assignment.depth,
                    polarity: assignment.polarity.map(PatchPolarity::from),
                    enabled: assignment.enabled,
                });
            }
        }
        patch.canonicalise();
        let mut text = patch.to_ron().expect("a group always serialises");
        let ports: BTreeMap<_, _> = self
            .members
            .iter()
            .filter_map(|member| match member {
                Member::Module(module) => module.unknown.as_ref().map(|unknown| {
                    (
                        module.id,
                        PlaceholderPorts {
                            inputs: unknown.inputs.clone(),
                            outputs: unknown.outputs.clone(),
                            parameters: unknown.parameters.clone(),
                        },
                    )
                }),
                Member::Instance(_) => None,
            })
            .collect();
        if !ports.is_empty() {
            text.push_str(PORTS_COMMENT);
            text.push_str(&serde_json::to_string(&ports).expect("placeholder names serialise"));
            text.push('\n');
        }
        text
    }

    fn member(&self, id: u32) -> Option<&Member> {
        self.members.iter().find(|m| m.id() == id)
    }

    /// The name of a member, for a message: a module's, or a sub-patch's.
    fn name_of(&self, id: u32) -> Result<String, String> {
        match self.member(id) {
            Some(Member::Module(module)) => Ok(module.descriptor.name.to_string()),
            Some(Member::Instance(instance)) => Ok(self.definition_name(&instance.key)),
            None => Err(format!("no module has id {id}")),
        }
    }

    /// The name of the definition `key`, or the key when it is missing.
    fn definition_name(&self, key: &str) -> String {
        self.definitions
            .get(key)
            .map_or_else(|| key.to_string(), |d| d.metadata.name.clone())
    }

    /// The names of a definition's ports facing one way, in the order its
    /// tile shows them: by module id.
    fn port_names(&self, key: &str, kind: &str) -> Vec<String> {
        let Some(definition) = self.definitions.get(key) else {
            return Vec::new();
        };
        let mut ports: Vec<&PatchModule> = definition.modules.iter().filter(|m| m.kind == kind).collect();
        ports.sort_by_key(|m| m.id);
        let fallback = if kind == INPUT_KIND { "in" } else { "out" };
        ports
            .iter()
            .map(|m| {
                m.label
                    .as_deref()
                    .filter(|label| !label.is_empty())
                    .unwrap_or(fallback)
                    .to_string()
            })
            .collect()
    }

    fn has_input(&self, id: u32, name: &str) -> bool {
        match self.member(id) {
            Some(Member::Module(module)) => module.input_index(name).is_some(),
            Some(Member::Instance(instance)) => self.port_names(&instance.key, INPUT_KIND).iter().any(|n| n == name),
            None => false,
        }
    }

    fn has_output(&self, id: u32, name: &str) -> bool {
        match self.member(id) {
            Some(Member::Module(module)) => module.output_index(name).is_some(),
            Some(Member::Instance(instance)) => self.port_names(&instance.key, OUTPUT_KIND).iter().any(|n| n == name),
            None => false,
        }
    }

    fn has_param(&self, id: u32, name: &str) -> bool {
        match self.member(id) {
            Some(Member::Module(module)) => module.parameter_index(name).is_some(),
            Some(Member::Instance(instance)) => self
                .definitions
                .get(&instance.key)
                .is_some_and(|d| d.exposed.iter().any(|e| e.name == name)),
            None => false,
        }
    }

    /// True when the group has no tiles.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// The ids of the group's tiles in the patch they came from: what cut
    /// deletes, so a tile the clipboard left out stays.
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.members.iter().map(Member::id)
    }

    /// "Filter" for one module, "Voice" for one sub-patch, "3 modules" for
    /// more, for step names and the status bar.
    pub fn describe(&self) -> String {
        match self.members.as_slice() {
            [Member::Module(one)] => one.descriptor.name.to_string(),
            [Member::Instance(one)] => self.definition_name(&one.key),
            all => format!("{} modules", all.len()),
        }
    }

    /// The patch-wide modules a paste of this group leaves out, for the
    /// status bar: `The Transport was left out: it lives at the top level.`
    /// Empty when the text had none.
    pub fn left_out_note(&self) -> Option<String> {
        if self.left_out.is_empty() {
            return None;
        }
        let names = self.left_out.join(" and ");
        let verb = if self.left_out.len() == 1 { "was" } else { "were" };
        Some(format!(
            "The {names} {verb} left out: a patch has one, at the top level."
        ))
    }

    /// The top-left corner of the tiles that have a position.
    pub fn top_left(&self) -> Option<(f32, f32)> {
        self.members
            .iter()
            .filter_map(|m| m.layout().position)
            .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))
    }

    /// Checks that the group can be added, before anything changes
    /// (rule 5). Module types and names were checked when the group was
    /// read, so what is left is room in the patch. Only normal modules take
    /// an engine slot, as in `undo::check`; a sub-patch takes one for each
    /// such module inside it, at every depth.
    pub fn check(&self, controller: &Controller) -> Result<(), String> {
        let registry = controller.registry();
        let takes_slot = |kind: &str| {
            registry
                .resolve(kind)
                .map_or(true, |d| matches!(d.role, ModuleRole::Dsp(_)))
        };
        let needed: usize = self
            .members
            .iter()
            .map(|member| match member {
                Member::Module(module) => usize::from(matches!(module.descriptor.role, ModuleRole::Dsp(_))),
                Member::Instance(instance) => {
                    let Some(definition) = self.definitions.get(&instance.key) else {
                        return 0;
                    };
                    std::iter::once(definition)
                        .chain(
                            loom_patch::reachable_keys(definition, &self.definitions)
                                .iter()
                                .filter_map(|key| self.definitions.get(key)),
                        )
                        .flat_map(|d| d.modules.iter())
                        .filter(|m| m.subpatch.is_none() && m.kind != INPUT_KIND && m.kind != OUTPUT_KIND)
                        .filter(|m| takes_slot(&m.kind))
                        .count()
                }
            })
            .sum();
        let free = controller.free_module_slots();
        if needed > free {
            return Err(format!(
                "the patch has room for {free} more {}",
                if free == 1 { "module" } else { "modules" }
            ));
        }
        Ok(())
    }

    /// Adds the group at `level` with new ids, its top-left corner at
    /// `origin`, and makes the controller's cables follow (a sync). Returns
    /// the new ids. The caller commits. On an error, everything added is
    /// taken away again, so a paste is whole or nothing, and the caller
    /// drops `hierarchy`, a candidate. The caller commits then too: the
    /// slots of the removed modules come back only with a commit.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        controller: &mut Controller,
        hierarchy: &mut Hierarchy,
        level: &ModulePath,
        positions: &mut BTreeMap<u32, (f32, f32)>,
        sizes: &mut BTreeMap<u32, (u8, u8)>,
        origin: (f32, f32),
    ) -> Result<Added, String> {
        let mut added = Vec::with_capacity(self.members.len());
        let mut modules = Vec::new();
        let mut tiles = Vec::new();
        let result = self.add_into(
            controller,
            hierarchy,
            level,
            positions,
            sizes,
            origin,
            &mut added,
            &mut modules,
            &mut tiles,
        );
        match result {
            Ok(renamed) => Ok(Added { ids: added, renamed }),
            Err(why) => {
                for id in &modules {
                    let _ = controller.remove_module(*id);
                }
                for id in &tiles {
                    positions.remove(&id.0);
                    sizes.remove(&id.0);
                }
                Err(why)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_into(
        &self,
        controller: &mut Controller,
        hierarchy: &mut Hierarchy,
        level: &ModulePath,
        positions: &mut BTreeMap<u32, (f32, f32)>,
        sizes: &mut BTreeMap<u32, (u8, u8)>,
        origin: (f32, f32),
        added: &mut Vec<u32>,
        modules: &mut Vec<ModuleId>,
        tiles: &mut Vec<ModuleId>,
    ) -> Result<Vec<(AssignmentId, AssignmentId)>, String> {
        let top_left = self.top_left().unwrap_or((0.0, 0.0));
        // Tiles without a position go in a row below the others.
        let below = self
            .members
            .iter()
            .filter_map(|m| m.layout().position)
            .map(|p| p.1 - top_left.1 + 10.0 * GRID)
            .fold(0.0, f32::max);
        let mut unplaced = 0.0;
        let mut ids: BTreeMap<u32, ModuleId> = BTreeMap::new();
        hierarchy.adopt(controller.graph());
        for member in &self.members {
            let id = match member {
                Member::Module(module) => {
                    // Ids come from the hierarchy's table, at the shown level
                    // (ADR-0039, invariant 1).
                    let (_, id) = hierarchy.new_module(level).map_err(|e| e.to_string())?;
                    if let Some(unknown) = &module.unknown {
                        controller
                            .add_unknown_with_id(id, unknown.clone())
                            .map_err(|e| e.to_string())?;
                    } else {
                        controller
                            .add_module_with_id(id, &module.descriptor.kind())
                            .map_err(|e| e.to_string())?;
                    }
                    modules.push(id);
                    // Label, ranges, then values, as a patch loads.
                    module.apply_to(controller, id).map_err(|e| e.to_string())?;
                    id
                }
                Member::Instance(instance) => {
                    let Some(definition) = self.definitions.get(&instance.key) else {
                        return Err(format!("there is no sub-patch `{}`", instance.key));
                    };
                    // Its own definition, as a file inserted (sub-patches.md,
                    // Copies and editing): a copy of a linked tile follows the
                    // same file.
                    let file = Patch::from_definition(definition, &self.definitions);
                    let inserted = hierarchy
                        .insert(controller, level, &file, definition.library.as_deref())
                        .map_err(|e| e.to_string())?;
                    modules.extend(inserted.modules.iter().copied());
                    hierarchy.set_label(inserted.instance, instance.label.clone());
                    for (inner, layout) in &inserted.layouts {
                        tiles.push(*inner);
                        if let Some(position) = layout.position {
                            positions.insert(inner.0, position);
                        }
                        if let Some(size) = layout.size {
                            sizes.insert(inner.0, size);
                        }
                    }
                    // The exposed values are the instance's, clamped to the
                    // surface's range (flattening rules 3 and 12).
                    for (index, exposed) in definition.exposed.iter().enumerate() {
                        let Some(&value) = instance.params.get(&exposed.name) else {
                            continue;
                        };
                        let Some(target) = hierarchy.exposed_target(inserted.instance, index) else {
                            continue;
                        };
                        let value = target.range.map_or(value, |(min, max)| value.clamp(min, max));
                        controller
                            .set_param_named(target.module, &target.param, value)
                            .map_err(|e| e.to_string())?;
                    }
                    inserted.instance
                }
            };
            tiles.push(id);
            added.push(id.0);
            ids.insert(member.id(), id);
            let place = match member.layout().position {
                Some(p) => (origin.0 + p.0 - top_left.0, origin.1 + p.1 - top_left.1),
                None => {
                    let place = (origin.0 + unplaced, origin.1 + below);
                    unplaced += loom_canvas::TILE_WIDTH + 4.0 * GRID;
                    place
                }
            };
            positions.insert(id.0, place);
            if let Some(size) = member.layout().size {
                sizes.insert(id.0, size);
            }
        }

        // Cables: between two modules, straight in the graph; with a
        // sub-patch tile or a port at either end, a boundary record the
        // sync turns into flat cables (ADR-0039, invariant 2). The ends are
        // worked out first, since connecting borrows both sides.
        let end_of = |source: &Source| -> End {
            match source {
                Source::Inside(id, name) => (hierarchy.path_of(ids[id]).last(), name.clone()),
                Source::Outside(end) => end.clone(),
            }
        };
        let module_at = |end: &End| -> Option<ModuleId> {
            hierarchy
                .id_at(level, end.0)
                .filter(|id| controller.graph().node(*id).is_some())
        };
        let cables: Vec<(Option<ModuleId>, End, ModuleId, End)> = self
            .cables
            .iter()
            .map(|cable| {
                let to = ids[&cable.to.0];
                let from = end_of(&cable.from);
                let to_end = (hierarchy.path_of(to).last(), cable.to.1.clone());
                (module_at(&from), from, to, to_end)
            })
            .collect();
        // Assignments: onto a module's parameter, or through a sub-patch
        // tile's exposed name to the inner parameter; from a module's
        // output, or from what stands behind a tile's or a port's output.
        let assignments: Vec<(ModuleId, String, ModuleId, String, &Modulation)> = self
            .modulation
            .iter()
            .filter_map(|assignment| {
                let target = ids[&assignment.target.0];
                let (module, param) = match controller.graph().node(target) {
                    Some(_) => (target, assignment.target.1.clone()),
                    None => {
                        let index = hierarchy
                            .definition_of(target)
                            .and_then(|d| d.exposed.iter().position(|e| e.name == assignment.target.1))?;
                        let exposed = hierarchy.exposed_target(target, index)?;
                        (exposed.module, exposed.param)
                    }
                };
                let source = end_of(&assignment.source);
                let (from, output) = hierarchy.source_behind(controller.graph(), level, &source)?;
                Some((from, output, module, param, assignment))
            })
            .collect();
        for (from_module, from, to, to_end) in cables {
            match (from_module, controller.graph().node(to).is_some()) {
                (Some(from_module), true) => {
                    controller
                        .connect_named(from_module, &from.1, to, &to_end.1)
                        .map_err(|e| e.to_string())?;
                }
                _ => hierarchy.connect(level, from, to_end),
            }
        }
        for (from, output, module, param, assignment) in assignments {
            let id = controller
                .add_assignment_named(from, &output, module, &param, assignment.depth)
                .map_err(|e| e.to_string())?;
            if assignment.polarity.is_some() {
                controller
                    .set_assignment_polarity(id, assignment.polarity)
                    .map_err(|e| e.to_string())?;
            }
            if !assignment.enabled {
                controller
                    .set_assignment_enabled(id, false)
                    .map_err(|e| e.to_string())?;
            }
        }
        hierarchy.sync(controller).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
