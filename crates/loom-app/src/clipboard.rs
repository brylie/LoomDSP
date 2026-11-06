//! Copy, cut, paste, and duplicate (`docs/design/clipboard.md`).
//!
//! A [`Group`] is the modules being copied, with the cables and
//! assignments that go with them. Copy turns the selection into a group and
//! writes it as patch text. Paste reads patch text back into a group.
//! Duplicate takes a group straight from the graph, so it can also keep the
//! incoming cables and assignments, which patch text cannot hold.
//!
//! Ids in a group only link its parts. A paste gives every module a new id
//! from the controller and remaps the cables and assignments to it.

use loom_canvas::GRID;
use loom_core::ModuleNames;
use loom_core::{Controller, Graph, InputRef, LoomError, ModuleId, ModuleRole, OutputRef, Polarity, Registry};
use loom_patch::{
    Layout, Patch, PatchCable, PatchError, PatchModulation, PatchPolarity, ResolveError, ResolveOptions, ResolvedModule,
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

/// One module in a group: the same resolved form a patch loads with, so
/// the clipboard holds no name lookups of its own (#651).
type Member = ResolvedModule;

/// Where a cable or an assignment starts.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    /// An output of a module in the group: module id and output index.
    Inside(u32, usize),
    /// An output of a module outside the group (duplicate only).
    Outside(OutputRef),
}

/// A cable into a module of the group.
#[derive(Clone, Debug)]
struct Cable {
    from: Source,
    /// Module id in the group, and input index.
    to: (u32, usize),
}

/// A modulation assignment onto a parameter of the group.
#[derive(Clone, Debug)]
struct Modulation {
    source: Source,
    /// Module id in the group, and parameter index.
    target: (u32, usize),
    depth: f32,
    polarity: Option<Polarity>,
    enabled: bool,
}

/// Modules with the cables and assignments that go with them.
#[derive(Clone, Debug, Default)]
pub struct Group {
    /// In id order, which is the order new ids are given in (rule 2).
    modules: Vec<Member>,
    cables: Vec<Cable>,
    /// In the order they are added back, which decides how assignments on
    /// one parameter are summed (rule 3).
    modulation: Vec<Modulation>,
}

impl Group {
    /// Includes the selected tiles' saved dimensions in a copied group.
    pub fn with_sizes(mut self, sizes: &BTreeMap<u32, (u8, u8)>) -> Self {
        for member in &mut self.modules {
            member.layout.size = sizes.get(&member.id).copied();
        }
        self
    }

    /// Stores copied dimensions under the newly assigned module IDs.
    pub fn apply_sizes(&self, added: &[u32], sizes: &mut BTreeMap<u32, (u8, u8)>) {
        for (member, &id) in self.modules.iter().zip(added) {
            if let Some(size) = member.layout.size {
                sizes.insert(id, size);
            }
        }
    }

    /// Takes the selected modules from the graph, with their inside cables
    /// and assignments. With `incoming`, cables and assignments from outside
    /// modules into the selection come too (duplicate). Outgoing ones and
    /// MIDI mappings never do. The Transport is left out: a patch has
    /// exactly one, so a copy of it could never be added (clock.md).
    pub fn from_selection(
        graph: &Graph,
        positions: &BTreeMap<u32, (f32, f32)>,
        selection: &BTreeSet<u32>,
        incoming: bool,
    ) -> Self {
        let selection: BTreeSet<u32> = selection
            .iter()
            .copied()
            .filter(|&id| {
                graph
                    .node(loom_core::ModuleId(id))
                    .is_some_and(|node| node.descriptor.id != loom_core::TRANSPORT_MODULE)
            })
            .collect();
        let selection = &selection;
        let modules: Vec<Member> = graph
            .nodes()
            .filter(|(id, _)| selection.contains(&id.0))
            .map(|(id, node)| {
                let layout = Layout {
                    position: positions.get(&id.0).copied(),
                    size: None,
                };
                ResolvedModule::from_node(id, node, layout)
            })
            .collect();
        let source = |from: OutputRef| {
            if selection.contains(&from.module.0) {
                Some(Source::Inside(from.module.0, from.port))
            } else {
                incoming.then_some(Source::Outside(from))
            }
        };
        let cables = graph
            .cables()
            .filter(|(_, to)| selection.contains(&to.module.0))
            .filter_map(|(from, to)| {
                Some(Cable {
                    from: source(from)?,
                    to: (to.module.0, to.port),
                })
            })
            .collect();
        // Assignments come in id order, the order the engine sums them in.
        let modulation = graph
            .assignments()
            .filter(|(_, a)| selection.contains(&a.target.0))
            .filter_map(|(_, a)| {
                Some(Modulation {
                    source: source(a.source)?,
                    target: (a.target.0, a.param),
                    depth: a.depth,
                    polarity: a.polarity,
                    enabled: a.enabled,
                })
            })
            .collect();
        Self {
            modules,
            cables,
            modulation,
        }
    }

    /// Reads a group from patch text. MIDI mappings in it are ignored.
    /// Fails, with the text for the status bar, if the text is not a patch
    /// or refers to something that does not exist.
    pub fn from_text(text: &str, registry: &Registry) -> Result<Self, String> {
        let patch = match Patch::from_ron(text) {
            Ok(patch) => patch,
            Err(error @ PatchError::TooNew { .. }) => return Err(format!("Cannot paste: {error}")),
            Err(_) => return Err(NOTHING_TO_PASTE.to_string()),
        };
        if patch.modules.is_empty() {
            return Err(NOTHING_TO_PASTE.to_string());
        }
        // An instance is not a registered module, so resolving would paste
        // it as a silent placeholder (#51, slice 2 adds the real thing).
        if !patch.subpatches.is_empty() || patch.modules.iter().any(|m| m.is_instance() || m.subpatch.is_some()) {
            return Err("Cannot paste: sub-patches are not supported yet".to_string());
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
        let (mut modules, _) = patch
            .resolve(registry, ResolveOptions::CLIPBOARD)
            .map_err(|error| match error {
                ResolveError::UnknownParam { module_name, param, .. } => {
                    format!("{module_name} has no parameter `{param}`")
                }
                ResolveError::Patch(error) => error.to_string(),
            })?;
        let mut ids = BTreeSet::new();
        for member in &mut modules {
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
            if !ids.insert(member.id) {
                return Err(format!("two modules have id {}", member.id));
            }
        }
        modules.sort_by_key(|m| m.id);
        let module = |id: u32| {
            modules
                .iter()
                .find(|m| m.id == id)
                .ok_or_else(|| format!("no module has id {id}"))
        };
        let output = |(id, name): &(u32, String)| {
            let member = module(*id)?;
            let port = member
                .output_index(name)
                .ok_or_else(|| format!("{} has no output `{name}`", member.descriptor.name))?;
            Ok::<_, String>(Source::Inside(*id, port))
        };
        let cables = patch
            .cables
            .iter()
            .map(|cable| {
                let member = module(cable.to.0)?;
                let port = member
                    .input_index(&cable.to.1)
                    .ok_or_else(|| format!("{} has no input `{}`", member.descriptor.name, cable.to.1))?;
                Ok(Cable {
                    from: output(&cable.from)?,
                    to: (cable.to.0, port),
                })
            })
            .collect::<Result<_, String>>()?;
        let modulation = patch
            .modulation
            .iter()
            .map(|assignment| {
                let member = module(assignment.target.0)?;
                let param = member
                    .parameter_index(&assignment.target.1)
                    .ok_or_else(|| format!("{} has no parameter `{}`", member.descriptor.name, assignment.target.1))?;
                Ok(Modulation {
                    source: output(&assignment.source)?,
                    target: (assignment.target.0, param),
                    depth: assignment.depth,
                    polarity: assignment.polarity.map(Polarity::from),
                    enabled: assignment.enabled,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            modules,
            cables,
            modulation,
        })
    }

    /// Writes the group as patch text: a whole patch with a version and an
    /// empty name. Outside cables and assignments are left out.
    pub fn to_text(&self) -> String {
        let mut patch = Patch::new("");
        for member in &self.modules {
            let mut module = member.write();
            module.position = module.position.map(|(x, y)| (x.round(), y.round()));
            patch.modules.push(module);
        }
        let output_name = |id: u32, port: usize| {
            let member = self.member(id).expect("a group's cables stay inside it");
            (
                id,
                member.output_name(port).expect("a group's outputs exist").to_string(),
            )
        };
        for cable in &self.cables {
            if let Source::Inside(id, port) = cable.from
                && let Some(member) = self.member(cable.to.0)
            {
                patch.cables.push(PatchCable {
                    from: output_name(id, port),
                    to: (
                        cable.to.0,
                        member
                            .input_name(cable.to.1)
                            .expect("a group's inputs exist")
                            .to_string(),
                    ),
                });
            }
        }
        for assignment in &self.modulation {
            if let Source::Inside(id, port) = assignment.source
                && let Some(member) = self.member(assignment.target.0)
            {
                patch.modulation.push(PatchModulation {
                    source: output_name(id, port),
                    target: (
                        assignment.target.0,
                        member
                            .parameter_name(assignment.target.1)
                            .expect("a group's parameters exist")
                            .to_string(),
                    ),
                    depth: assignment.depth,
                    polarity: assignment.polarity.map(PatchPolarity::from),
                    enabled: assignment.enabled,
                });
            }
        }
        patch.canonicalise();
        let mut text = patch.to_ron().expect("a group always serialises");
        let ports: BTreeMap<_, _> = self
            .modules
            .iter()
            .filter_map(|member| {
                member.unknown.as_ref().map(|unknown| {
                    (
                        member.id,
                        PlaceholderPorts {
                            inputs: unknown.inputs.clone(),
                            outputs: unknown.outputs.clone(),
                            parameters: unknown.parameters.clone(),
                        },
                    )
                })
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
        self.modules.iter().find(|m| m.id == id)
    }

    /// True when the group has no modules.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// "Filter" for one module, "3 modules" for more, for step names and
    /// the status bar.
    pub fn describe(&self) -> String {
        match self.modules.as_slice() {
            [one] => one.descriptor.name.to_string(),
            all => format!("{} modules", all.len()),
        }
    }

    /// The top-left corner of the modules that have a position.
    pub fn top_left(&self) -> Option<(f32, f32)> {
        self.modules
            .iter()
            .filter_map(|m| m.layout.position)
            .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))
    }

    /// Checks that the group can be added, before anything changes
    /// (rule 5). Module types and names were checked when the group was
    /// read, so what is left is room in the patch. Only normal modules take
    /// an engine slot, as in `undo::check`.
    pub fn check(&self, controller: &Controller) -> Result<(), String> {
        let needed = self
            .modules
            .iter()
            .filter(|m| matches!(m.descriptor.role, ModuleRole::Dsp(_)))
            .count();
        let free = controller.free_module_slots();
        if needed > free {
            return Err(format!(
                "the patch has room for {free} more {}",
                if free == 1 { "module" } else { "modules" }
            ));
        }
        Ok(())
    }

    /// Adds the group with new ids, its top-left corner at `origin`, and
    /// returns the new ids. The caller commits. On an error, everything
    /// added is taken away again, so a paste is whole or nothing. The caller
    /// commits then too: the slots of the removed modules come back only
    /// with a commit.
    pub fn add(
        &self,
        controller: &mut Controller,
        positions: &mut BTreeMap<u32, (f32, f32)>,
        origin: (f32, f32),
    ) -> Result<Vec<u32>, LoomError> {
        let mut added = Vec::with_capacity(self.modules.len());
        let result = self.add_into(controller, positions, origin, &mut added);
        if result.is_err() {
            for id in &added {
                let _ = controller.remove_module(ModuleId(*id));
                positions.remove(id);
            }
        }
        result.map(|()| added)
    }

    fn add_into(
        &self,
        controller: &mut Controller,
        positions: &mut BTreeMap<u32, (f32, f32)>,
        origin: (f32, f32),
        added: &mut Vec<u32>,
    ) -> Result<(), LoomError> {
        let top_left = self.top_left().unwrap_or((0.0, 0.0));
        // Modules without a position go in a row below the others.
        let below = self
            .modules
            .iter()
            .filter_map(|m| m.layout.position)
            .map(|p| p.1 - top_left.1 + 10.0 * GRID)
            .fold(0.0, f32::max);
        let mut unplaced = 0.0;
        let mut ids = BTreeMap::new();
        for member in &self.modules {
            let id = if let Some(unknown) = &member.unknown {
                controller.add_unknown(unknown.clone())?
            } else {
                controller.add_module(&member.descriptor.kind())?
            };
            added.push(id.0);
            ids.insert(member.id, id);
            // Label, ranges, then values, as a patch loads.
            member.apply_to(controller, id)?;
            let place = match member.layout.position {
                Some(p) => (origin.0 + p.0 - top_left.0, origin.1 + p.1 - top_left.1),
                None => {
                    let place = (origin.0 + unplaced, origin.1 + below);
                    unplaced += loom_canvas::TILE_WIDTH + 4.0 * GRID;
                    place
                }
            };
            positions.insert(id.0, place);
        }
        let source = |source: Source| match source {
            Source::Inside(id, port) => OutputRef { module: ids[&id], port },
            Source::Outside(output) => output,
        };
        for cable in &self.cables {
            let to = InputRef {
                module: ids[&cable.to.0],
                port: cable.to.1,
            };
            controller.connect(source(cable.from), to)?;
        }
        for assignment in &self.modulation {
            let id = controller.add_assignment(
                source(assignment.source),
                ids[&assignment.target.0],
                assignment.target.1,
                assignment.depth,
            )?;
            if assignment.polarity.is_some() {
                controller.set_assignment_polarity(id, assignment.polarity)?;
            }
            if !assignment.enabled {
                controller.set_assignment_enabled(id, false)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
