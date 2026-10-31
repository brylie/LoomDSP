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

use iced::Point;
use loom_canvas::GRID;
use loom_core::{
    Controller, Graph, InputRef, LoomError, ModuleDescriptor, ModuleId, ModuleRole, OutputRef, ParamRange, Polarity,
    Registry,
};
use loom_patch::{Patch, PatchCable, PatchError, PatchModulation, PatchModule, PatchPolarity};
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

/// One module in a group.
#[derive(Clone, Debug)]
struct Member {
    /// Its id in the graph or in the clipboard text.
    id: u32,
    descriptor: &'static ModuleDescriptor,
    /// Original data if this member is an unknown placeholder.
    unknown: Option<loom_core::UnknownModule>,
    /// One value per parameter of the descriptor.
    params: Vec<f32>,
    /// The selected control range of each parameter, if it offers ranges.
    ranges: Vec<Option<ParamRange>>,
    position: Option<Point>,
    /// Optional saved tile dimensions in grid units.
    size: Option<(u8, u8)>,
}

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
            member.size = sizes.get(&member.id).copied();
        }
        self
    }

    /// Stores copied dimensions under the newly assigned module IDs.
    pub fn apply_sizes(&self, added: &[u32], sizes: &mut BTreeMap<u32, (u8, u8)>) {
        for (member, &id) in self.modules.iter().zip(added) {
            if let Some(size) = member.size {
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
        positions: &BTreeMap<u32, Point>,
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
            .map(|(id, node)| Member {
                id: id.0,
                descriptor: node.descriptor,
                unknown: node.unknown.clone(),
                params: node.descriptor.saved_values(&node.params),
                ranges: (0..node.params.len()).map(|index| node.range(index)).collect(),
                position: positions.get(&id.0).copied(),
                size: None,
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
        let mut modules = Vec::with_capacity(patch.modules.len());
        let mut ids = BTreeSet::new();
        for module in &patch.modules {
            let mut unknown = if !module.kind.contains('@') && registry.resolve(&module.kind).is_ok() {
                None
            } else {
                patch
                    .unknown_module(module, registry)
                    .map_err(|error| error.to_string())?
            };
            if let (Some(unknown), Some(saved)) = (&mut unknown, ports.get(&module.id)) {
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
            if unknown.as_ref().is_some_and(|unknown| unknown.inputs.len() > 64) {
                return Err("a placeholder cannot have more than 64 inputs".into());
            }
            let descriptor = if unknown.is_some() {
                loom_core::UnknownModule::descriptor()
            } else {
                registry.resolve(&module.kind).map_err(|error| error.to_string())?
            };
            if !ids.insert(module.id) {
                return Err(format!("two modules have id {}", module.id));
            }
            let mut params: Vec<f32> = descriptor.params.iter().map(|p| p.default).collect();
            for (name, &value) in module.params.iter().filter(|_| unknown.is_none()) {
                let index = descriptor
                    .param_index(name)
                    .ok_or_else(|| format!("{} has no parameter `{name}`", descriptor.name))?;
                params[index] = value;
            }
            // A pasted module starts released (transient-params.md).
            let params = descriptor.saved_values(&params);
            // As a patch loads: the range never changes the value
            // (control-ranges.md, rule 7).
            let ranges = descriptor
                .params
                .iter()
                .zip(&params)
                .map(|(spec, &value)| {
                    let saved = module
                        .ranges
                        .get(spec.name)
                        .map(|&(min, max)| ParamRange::new(min, max));
                    spec.load_range(saved, spec.clamp(value)).map(|(range, _)| range)
                })
                .collect();
            modules.push(Member {
                id: module.id,
                descriptor,
                unknown,
                params,
                ranges,
                position: module.position.map(|(x, y)| Point::new(x, y)),
                size: module.size,
            });
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
                .unknown
                .as_ref()
                .map_or_else(
                    || member.descriptor.output_index(name),
                    |unknown| unknown.outputs.iter().position(|port| port == name),
                )
                .ok_or_else(|| format!("{} has no output `{name}`", member.descriptor.name))?;
            Ok::<_, String>(Source::Inside(*id, port))
        };
        let cables = patch
            .cables
            .iter()
            .map(|cable| {
                let member = module(cable.to.0)?;
                let port = member
                    .unknown
                    .as_ref()
                    .map_or_else(
                        || member.descriptor.input_index(&cable.to.1),
                        |unknown| unknown.inputs.iter().position(|port| port == &cable.to.1),
                    )
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
                    .unknown
                    .as_ref()
                    .map_or_else(
                        || member.descriptor.param_index(&assignment.target.1),
                        |unknown| {
                            unknown
                                .parameters
                                .iter()
                                .position(|param| param == &assignment.target.1)
                        },
                    )
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
            let d = member.descriptor;
            if let Some(unknown) = &member.unknown {
                patch.modules.push(PatchModule {
                    id: member.id,
                    kind: unknown.kind.clone(),
                    label: unknown.label.clone(),
                    params: unknown.params.clone(),
                    position: member.position.map(|p| (p.x.round(), p.y.round())),
                    size: member.size,
                    ranges: unknown.ranges.clone(),
                });
                continue;
            }
            patch.modules.push(PatchModule {
                id: member.id,
                kind: d.kind(),
                label: None,
                params: d
                    .params
                    .iter()
                    .zip(&member.params)
                    .filter(|(spec, value)| **value != spec.default)
                    .map(|(spec, value)| (spec.name.to_string(), *value))
                    .collect(),
                position: member.position.map(|p| (p.x.round(), p.y.round())),
                size: member.size,
                ranges: d
                    .params
                    .iter()
                    .zip(&member.ranges)
                    .filter_map(|(spec, range)| {
                        let range = (*range)?;
                        (Some(range) != spec.default_range()).then(|| (spec.name.to_string(), (range.min, range.max)))
                    })
                    .collect(),
            });
        }
        let output_name = |id: u32, port: usize| {
            let member = self.member(id).expect("a group's cables stay inside it");
            (
                id,
                member.unknown.as_ref().map_or_else(
                    || member.descriptor.outputs[port].name.to_string(),
                    |unknown| unknown.outputs[port].clone(),
                ),
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
                        member.unknown.as_ref().map_or_else(
                            || member.descriptor.inputs[cable.to.1].name.to_string(),
                            |unknown| unknown.inputs[cable.to.1].clone(),
                        ),
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
                        member.unknown.as_ref().map_or_else(
                            || member.descriptor.params[assignment.target.1].name.to_string(),
                            |unknown| unknown.parameters[assignment.target.1].clone(),
                        ),
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
    pub fn top_left(&self) -> Option<Point> {
        self.modules
            .iter()
            .filter_map(|m| m.position)
            .reduce(|a, b| Point::new(a.x.min(b.x), a.y.min(b.y)))
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
        positions: &mut BTreeMap<u32, Point>,
        origin: Point,
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
        positions: &mut BTreeMap<u32, Point>,
        origin: Point,
        added: &mut Vec<u32>,
    ) -> Result<(), LoomError> {
        let top_left = self.top_left().unwrap_or(Point::ORIGIN);
        // Modules without a position go in a row below the others.
        let below = self
            .modules
            .iter()
            .filter_map(|m| m.position)
            .map(|p| p.y - top_left.y + 10.0 * GRID)
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
            // Ranges first, so they never clamp a pasted value.
            for (index, range) in member.ranges.iter().enumerate() {
                let current = controller.graph().node(id).and_then(|n| n.range(index));
                if let Some(range) = *range
                    && current != Some(range)
                {
                    controller.set_range(id, index, range)?;
                }
            }
            let changes: Vec<(usize, f32)> = member
                .descriptor
                .params
                .iter()
                .zip(&member.params)
                .enumerate()
                .filter(|(_, (spec, value))| **value != spec.default)
                .map(|(index, (_, value))| (index, *value))
                .collect();
            if !changes.is_empty() {
                controller.set_params(id, &changes)?;
            }
            let place = match member.position {
                Some(p) => Point::new(origin.x + p.x - top_left.x, origin.y + p.y - top_left.y),
                None => {
                    let place = Point::new(origin.x + unplaced, origin.y + below);
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
