//! Structure shared by the patch map, outline, and cable navigation.

use loom_canvas::{PortRef, Scene};
use loom_core::{AssignmentId, Controller, Graph, ModuleId, ModuleNames, ModuleRole, OutputRef, SignalType};
use loom_patch::{Direction, Hierarchy, ModulePath, Tile};
use loom_ui_core::Color;
use std::collections::{BTreeMap, BTreeSet};

/// A tile's identity and the words used to describe it.
#[derive(Debug, PartialEq)]
pub(crate) struct TileInfo {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) instance: bool,
    pub(crate) output: bool,
    pub(crate) inputs: Vec<PortInfo>,
    pub(crate) outputs: Vec<PortInfo>,
}

/// The stable key and visible label of one port.
#[derive(Debug, PartialEq)]
pub(crate) struct PortInfo {
    pub(crate) key: String,
    pub(crate) label: String,
}

/// A visible cable, including connections to and from boundary tiles.
#[derive(Debug, PartialEq)]
pub(crate) struct Cable {
    pub(crate) from: PortRef,
    pub(crate) to: PortRef,
    pub(crate) colour: Color,
    pub(crate) stereo: bool,
    pub(crate) signal: SignalType,
}

/// An assignment in the level, including switched-off assignments.
#[derive(Debug, PartialEq)]
pub(crate) struct Modulation {
    pub(crate) id: u32,
    pub(crate) from: PortRef,
    pub(crate) to: u32,
    pub(crate) parameter: String,
    pub(crate) depth: f32,
    pub(crate) enabled: bool,
}

/// The level tiles standing for a compiled loop and what breaks the cycle.
#[derive(Debug, PartialEq)]
pub(crate) struct Loop {
    pub(crate) members: Vec<u32>,
    pub(crate) delays: String,
}

/// Structural information. Geometry is read from the scene and camera.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Structure {
    pub(crate) tiles: BTreeMap<u32, TileInfo>,
    pub(crate) cables: Vec<Cable>,
    pub(crate) modulations: Vec<Modulation>,
    pub(crate) heard: BTreeSet<u32>,
    pub(crate) loops: Vec<Loop>,
}

/// One unit in signal-flow order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Unit {
    Tile(u32),
    Loop(usize),
}

/// The current level's structure and its signal-flow order.
#[derive(Debug, Default)]
pub(crate) struct PatchMap {
    /// A new value for every structural refresh, shared with the canvas.
    pub(crate) generation: u64,
    /// Changes to order or speech that leave the map drawing intact.
    pub(crate) row_revision: u64,
    pub(crate) structure: Structure,
    pub(crate) order: Vec<Unit>,
    pub(crate) drawing: loom_canvas::MapGraph,
    reading: Vec<u32>,
}

/// A visible owner for a flat module on the current level.
fn owner(scene: &Scene, hierarchy: &Hierarchy, level: &ModulePath, module: ModuleId) -> Option<u32> {
    if scene.module(module.0).is_some() {
        return Some(module.0);
    }
    let path = hierarchy.path_of(module);
    let next = path.0.get(level.0.len())?;
    if path.prefix(level.0.len()) != *level {
        return None;
    }
    let tile = hierarchy.id_at(level, *next)?;
    scene.module(tile.0).map(|_| tile.0)
}

/// Gets the source and target of a cable as canvas port references.
fn port(module: u32, index: usize, output: bool) -> PortRef {
    PortRef { module, index, output }
}

/// Walks backwards from audio outputs across both kinds of signal connection.
fn heard_modules(graph: &Graph) -> BTreeSet<ModuleId> {
    let mut feeders = BTreeMap::<_, Vec<_>>::new();
    for (from, to) in graph.cables() {
        feeders.entry(to.module).or_default().push(from.module);
    }
    for (_, assignment) in graph.assignments() {
        feeders
            .entry(assignment.target)
            .or_default()
            .push(assignment.source.module);
    }
    let mut pending: Vec<_> = graph
        .nodes()
        .filter_map(|(id, node)| matches!(node.descriptor.role, ModuleRole::AudioOutput).then_some(id))
        .collect();
    let mut heard = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if heard.insert(id) {
            pending.extend(feeders.get(&id).into_iter().flatten().copied());
        }
    }
    heard
}

impl PatchMap {
    /// Builds the shared structure for the level and orders its units.
    pub(crate) fn new(controller: &Controller, hierarchy: &Hierarchy, level: &ModulePath, scene: &Scene) -> Self {
        let graph = controller.graph();
        let report = controller.last_report();
        let visible = |module| owner(scene, hierarchy, level, module);
        let flat_heard = heard_modules(graph);
        let mut structure = Structure {
            heard: flat_heard.iter().filter_map(|id| visible(*id)).collect(),
            ..Structure::default()
        };
        for tile in scene.modules() {
            let id = ModuleId(tile.id);
            let node = graph.node(id);
            let kind = hierarchy.tile(graph, id);
            let instance = matches!(kind, Some(Tile::Instance(_)));
            let output = node.is_some_and(|n| matches!(n.descriptor.role, ModuleRole::AudioOutput))
                || matches!(kind, Some(Tile::Port(Direction::Output, _)));
            if let Some(Tile::Port(direction, _)) = kind {
                let end = (hierarchy.path_of(id).last(), direction.own_port().to_string());
                let heard = match direction {
                    Direction::Input => hierarchy
                        .level_cables(graph, level)
                        .iter()
                        .filter(|wire| wire.from == end)
                        .any(|wire| {
                            hierarchy
                                .inputs_behind(graph, level, &wire.to)
                                .iter()
                                .any(|(module, _)| flat_heard.contains(module))
                        }),
                    Direction::Output => level.parent().is_some_and(|parent| {
                        let Some(instance) = hierarchy.id_at(&parent, level.last()) else {
                            return false;
                        };
                        hierarchy
                            .instance_ports(instance)
                            .1
                            .iter()
                            .find(|(port, _)| *port == id)
                            .is_some_and(|(_, name)| {
                                hierarchy
                                    .level_cables(graph, &parent)
                                    .iter()
                                    .filter(|wire| wire.from == (level.last(), name.clone()))
                                    .any(|wire| {
                                        hierarchy
                                            .inputs_behind(graph, &parent, &wire.to)
                                            .iter()
                                            .any(|(module, _)| flat_heard.contains(module))
                                    })
                            })
                    }),
                };
                if heard {
                    structure.heard.insert(tile.id);
                }
            }
            let describe_ports = |output, ports: &[loom_canvas::PortView]| {
                ports
                    .iter()
                    .enumerate()
                    .filter_map(|(index, view)| {
                        let key = crate::targets::port_name(graph, hierarchy, port(tile.id, index, output))?;
                        Some(PortInfo {
                            key,
                            label: view.label.clone(),
                        })
                    })
                    .collect()
            };
            structure.tiles.insert(
                tile.id,
                TileInfo {
                    name: tile.title.clone(),
                    kind: hierarchy.definition_of(id).map_or_else(
                        || node.map_or_else(|| tile.title.clone(), |n| n.descriptor.family.label().to_string()),
                        |_| {
                            let inside = hierarchy.path_of(id);
                            let modules = graph
                                .nodes()
                                .filter(|(id, _)| hierarchy.level_of(*id) == inside && !hierarchy.is_hidden(*id))
                                .count();
                            format!(
                                "Sub-patch, {} modules inside",
                                modules + hierarchy.structural_tiles(&inside).len()
                            )
                        },
                    ),
                    instance,
                    output,
                    inputs: describe_ports(false, &tile.inputs),
                    outputs: describe_ports(true, &tile.outputs),
                },
            );
        }
        structure.cables = scene
            .cables()
            .iter()
            .map(|wire| {
                let from = port(wire.from.0, wire.from.1, true);
                let id = ModuleId(from.module);
                let source = if graph.node(id).is_some() {
                    Some(OutputRef {
                        module: id,
                        port: from.index,
                    })
                } else {
                    crate::targets::port_name(graph, hierarchy, from).and_then(|name| {
                        let (module, name) =
                            hierarchy.source_behind(graph, level, &(hierarchy.path_of(id).last(), name))?;
                        Some(OutputRef {
                            module,
                            port: graph.node(module)?.output_index(&name)?,
                        })
                    })
                };
                let stereo = source.is_some_and(|source| report.width(source).sides == 2);
                let signal = source
                    .and_then(|source| graph.node(source.module)?.descriptor.outputs.get(source.port))
                    .map_or(SignalType::Modulation, |spec| spec.signal);
                Cable {
                    from,
                    to: port(wire.to.0, wire.to.1, false),
                    colour: wire.colour_from,
                    stereo,
                    signal,
                }
            })
            .collect();
        structure.modulations = scene
            .modulations()
            .iter()
            .filter_map(|line| {
                let assignment = graph.assignment(AssignmentId(line.id))?;
                let spec = graph.node(assignment.target)?.descriptor.params.get(assignment.param)?;
                Some(Modulation {
                    id: line.id,
                    from: port(line.from.0, line.from.1, true),
                    to: line.to,
                    parameter: spec.label.to_string(),
                    depth: assignment.depth,
                    enabled: assignment.enabled,
                })
            })
            .collect();

        let name = |id| structure.tiles.get(&id).map_or("?", |tile| tile.name.as_str());
        for group in &report.loops {
            let mut seen = BTreeSet::new();
            let members: Vec<_> = group
                .run_order
                .iter()
                .filter_map(|id| visible(*id))
                .filter(|id| seen.insert(*id))
                .collect();
            if members.is_empty() {
                continue;
            }
            let mut delays = Vec::new();
            for (from, to) in &group.inserted_delays {
                if let (Some(a), Some(b)) = (visible(from.module), visible(to.module)) {
                    delays.push(format!("Delay inserted from {} to {}", name(a), name(b)));
                }
            }
            for id in &group.inserted_assignment_delays {
                if let Some(assignment) = structure.modulations.iter().find(|assignment| assignment.id == id.0) {
                    delays.push(format!(
                        "Delay inserted on {}'s modulation of {} {}",
                        name(assignment.from.module),
                        name(assignment.to),
                        assignment.parameter
                    ));
                }
            }
            if delays.is_empty() {
                for id in &group.run_order {
                    if graph
                        .node(*id)
                        .is_some_and(|node| matches!(node.descriptor.role, ModuleRole::UnitDelay))
                        && let Some(tile) = visible(*id)
                    {
                        delays.push(format!("Unit delay: {}", name(tile)));
                    }
                }
            }
            structure.loops.push(Loop {
                members,
                delays: delays.join(". "),
            });
        }
        let mut lines = BTreeMap::<_, loom_canvas::MapLine>::new();
        for wire in &structure.cables {
            let key = (wire.from.module, wire.to.module);
            let width = if wire.stereo { 2.0 } else { 1.25 };
            let line = lines.entry(key).or_insert(loom_canvas::MapLine {
                from: key.0,
                to: key.1,
                colour: wire.colour,
                width,
                dash: 0.0,
            });
            line.width = line.width.max(width);
        }
        let drawing = loom_canvas::MapGraph {
            lines: lines
                .into_values()
                .chain(structure.modulations.iter().map(|assignment| loom_canvas::MapLine {
                    from: assignment.from.module,
                    to: assignment.to,
                    colour: crate::scene::signal_colour(loom_core::SignalType::Modulation),
                    width: 1.25,
                    dash: if assignment.enabled { 4.0 } else { 1.0 },
                }))
                .collect(),
            loops: structure
                .loops
                .iter()
                .flat_map(|group| group.members.iter().copied())
                .collect(),
        };
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
        let mut map = Self {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            structure,
            drawing,
            ..Self::default()
        };
        map.reorder(scene);
        map
    }

    /// Updates only the depth announced in the outline.
    pub(crate) fn update_depth(&mut self, id: u32, depth: f32) {
        if let Some(assignment) = self.structure.modulations.iter_mut().find(|a| a.id == id)
            && assignment.depth != depth
        {
            assignment.depth = depth;
            self.row_revision += 1;
        }
    }

    /// Recomputes the tie order after a tile move ends.
    pub(crate) fn reorder(&mut self, scene: &Scene) {
        let reading = scene
            .reading_order()
            .into_iter()
            .map(|tile| tile.id)
            .collect::<Vec<_>>();
        if reading == self.reading {
            return;
        }
        self.row_revision += 1;
        let units: Vec<_> = reading
            .iter()
            .map(|id| {
                self.structure
                    .loops
                    .iter()
                    .position(|group| group.members.contains(id))
                    .map_or(Unit::Tile(*id), Unit::Loop)
            })
            .collect();
        let by_module: BTreeMap<_, _> = reading.iter().copied().zip(units.iter().copied()).collect();
        let mut ranks = BTreeMap::new();
        for (rank, unit) in units.into_iter().enumerate() {
            ranks.entry(unit).or_insert(rank);
        }
        let mut feeders: BTreeMap<_, BTreeSet<_>> = ranks.keys().map(|unit| (*unit, BTreeSet::new())).collect();
        for (from, to) in self.structure.edges() {
            if let (Some(&a), Some(&b)) = (by_module.get(&from), by_module.get(&to))
                && a != b
            {
                feeders.get_mut(&b).expect("one set per unit").insert(a);
            }
        }
        let mut followers: BTreeMap<Unit, Vec<Unit>> = BTreeMap::new();
        for (&unit, incoming) in &feeders {
            for &feeder in incoming {
                followers.entry(feeder).or_default().push(unit);
            }
        }
        let mut ready: BTreeSet<_> = feeders
            .iter()
            .filter(|(_, incoming)| incoming.is_empty())
            .map(|(unit, _)| (ranks[unit], *unit))
            .collect();
        self.order.clear();
        while let Some((_, unit)) = ready.pop_first() {
            self.order.push(unit);
            for follower in followers.get(&unit).into_iter().flatten() {
                let incoming = feeders.get_mut(follower).expect("one set per unit");
                incoming.remove(&unit);
                if incoming.is_empty() {
                    ready.insert((ranks[follower], *follower));
                }
            }
        }
        // Collapsing a sub-patch can make a cycle between its ports even
        // when the flat graph has none. Keep these tiles in reading order.
        let placed: BTreeSet<_> = self.order.iter().copied().collect();
        let mut tail: Vec<_> = ranks.into_iter().filter(|(unit, _)| !placed.contains(unit)).collect();
        tail.sort_by_key(|(_, rank)| *rank);
        self.order.extend(tail.into_iter().map(|(unit, _)| unit));
        self.reading = reading;
    }
}

impl Structure {
    /// All feeder edges, including switched-off modulation.
    pub(crate) fn edges(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.cables
            .iter()
            .map(|wire| (wire.from.module, wire.to.module))
            .chain(self.modulations.iter().map(|line| (line.from.module, line.to)))
    }

    /// A module's name as shown on its tile.
    pub(crate) fn name(&self, id: u32) -> &str {
        self.tiles.get(&id).map_or("?", |tile| &tile.name)
    }

    /// A port's name as shown on its tile.
    pub(crate) fn port_label(&self, port: PortRef) -> &str {
        self.tiles
            .get(&port.module)
            .and_then(|tile| (if port.output { &tile.outputs } else { &tile.inputs }).get(port.index))
            .map_or("?", |port| port.label.as_str())
    }
}

/// Lists names in a short phrase for the outline and canvas descriptions.
pub(crate) fn names(words: impl IntoIterator<Item = String>, limit: usize) -> String {
    let words: Vec<_> = words.into_iter().collect();
    if words.len() > limit {
        return format!("{} and {} more", words[..limit].join(", "), words.len() - limit);
    }
    match words.as_slice() {
        [] => String::new(),
        [name] => name.clone(),
        [first, last] => format!("{first} and {last}"),
        [first @ .., last] => format!("{}, and {last}", first.join(", ")),
    }
}

impl crate::App {
    /// Refreshes structural data after a graph or level change.
    pub(crate) fn refresh_patch_map(&mut self) {
        let session = &mut self.session;
        session.patch_map = PatchMap::new(&session.controller, &session.hierarchy, &session.level, &session.scene);
        session.outline.reconcile_jump(&session.patch_map.structure);
    }

    /// The map overlay's options and the graph it draws.
    pub(crate) fn map_view(&self) -> Option<loom_canvas::MapView<'_>> {
        use crate::preferences::MapMode;
        let mode = self.rig.store.saved().patch_map;
        (mode != MapMode::Hidden).then_some(loom_canvas::MapView {
            graph: &self.session.patch_map.drawing,
            generation: self.session.patch_map.generation,
            auto: mode == MapMode::Auto,
            text_size: crate::style::SMALL,
        })
    }
}
