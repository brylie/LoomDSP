//! Flattening: expanding sub-patch instances into the flat graph the
//! engine runs (`docs/design/sub-patches.md`, Flattening; ADR-0031).
//!
//! The control side expands each instance into its definition's modules,
//! recursively, before a patch is applied. A Patch input or output inside a
//! definition is removed, and a cable into it continues from its other
//! side. An exposed parameter's instance value is written to the inner
//! parameter. Each flat module gets a [`ModuleId`] from a [`FlatIds`] table
//! keyed by its path, so a path keeps its id across edits and its state
//! survives. Flattening is deterministic: modules and instances are
//! visited in id order, and ids are handed out in visit order.
//!
//! Control thread only. Nothing here runs on the audio thread, and nothing
//! here opens a file.

use crate::level::LevelRef;
use crate::{
    INPUT_KIND, LoadWarning, OUTPUT_KIND, Patch, PatchCable, PatchError, PatchMidiMapping, PatchModulation,
    PatchModule, ResolveOptions, ResolvedModule, SUBPATCH_KIND, SubPatch,
};
use loom_core::{Controller, LoomError, MidiControl, ModuleId, ModuleNames, Registry, TRANSPORT_MODULE};
use std::collections::BTreeMap;
use std::fmt;

/// The most levels of sub-patch a patch may nest. Flattening recurses once
/// per level, so a bound keeps a hand-made file from exhausting the stack.
/// With one instance per definition, the flat graph is never larger than
/// the file's definitions put together, so no other bound is needed.
pub const MAX_DEPTH: usize = 64;

/// Where a flat module came from: the ids of the instances it is inside,
/// from the top level down, then its own id in its level. A top-level
/// module's path is its id alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModulePath(pub Vec<u32>);

impl ModulePath {
    /// The path of a top-level module.
    pub fn top(id: u32) -> Self {
        Self(vec![id])
    }

    /// The path of module `id` inside the instance at this path.
    pub fn child(&self, id: u32) -> Self {
        let mut ids = self.0.clone();
        ids.push(id);
        Self(ids)
    }

    /// True for a top-level module.
    pub fn is_top(&self) -> bool {
        self.0.len() == 1
    }

    /// The path of the top level itself: no instance at all.
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// True for the top level's own path.
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The level this path is on: the path without its last id. `None`
    /// for the root.
    pub fn parent(&self) -> Option<Self> {
        let (_, parent) = self.0.split_last()?;
        Some(Self(parent.to_vec()))
    }

    /// The id in its level: the last element. 0 for the root, which has
    /// no id.
    pub fn last(&self) -> u32 {
        self.0.last().copied().unwrap_or(0)
    }

    /// True when this path is `prefix` or lies under it.
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }

    /// The first `n` ids.
    pub fn prefix(&self, n: usize) -> Self {
        Self(self.0[..n.min(self.0.len())].to_vec())
    }

    /// This path with `old` at its start replaced by `new`.
    pub fn replace_prefix(&self, old: &Self, new: &Self) -> Self {
        let mut ids = new.0.clone();
        ids.extend_from_slice(&self.0[old.0.len().min(self.0.len())..]);
        Self(ids)
    }
}

impl fmt::Display for ModulePath {
    /// The ids joined by " › ", such as `7 › 3`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, id) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, " › ")?;
            }
            write!(f, "{id}")?;
        }
        Ok(())
    }
}

/// Flat ids by path (flattening rule 4).
///
/// A path keeps its id for the life of the table, and `next` is never
/// lowered, so a removed path's id is not reused while the table lives.
/// The editor will keep one table per session (slice 2 of #51). Every id
/// the controller holds must be known here: a top-level module's id is
/// reserved by [`Patch::flatten`], and an id handed out elsewhere, such
/// as the Transport that [`Flat::apply_to`] adds, is told to the table
/// with [`FlatIds::reserve_id`]. New top-level modules must take their ids
/// from the table, not from the controller, or a later flatten can hand
/// the same id to an inner module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlatIds {
    by_path: BTreeMap<ModulePath, ModuleId>,
    by_id: BTreeMap<ModuleId, ModulePath>,
    /// The next id to hand out. Starts above every id ever seen.
    next: u32,
}

impl FlatIds {
    /// An empty table. The first id handed out is 1.
    pub fn new() -> Self {
        Self {
            by_path: BTreeMap::new(),
            by_id: BTreeMap::new(),
            next: 1,
        }
    }

    /// Gives `path` the id `id`: a top-level module keeps the id in its
    /// file. Refused when another path owns that id.
    pub fn reserve(&mut self, path: ModulePath, id: ModuleId) -> Result<(), PatchError> {
        match self.by_id.get(&id) {
            Some(owner) if *owner != path => return Err(LoomError::DuplicateModuleId(id).into()),
            _ => {}
        }
        if let Some(old) = self.by_path.insert(path.clone(), id) {
            self.by_id.remove(&old);
        }
        self.by_id.insert(id, path);
        self.reserve_id(id);
        Ok(())
    }

    /// Notes an id handed out elsewhere, so it is never handed out here.
    pub fn reserve_id(&mut self, id: ModuleId) {
        self.next = self.next.max(id.0.saturating_add(1));
    }

    /// The id of `path`: the one it has, else the next free one.
    pub fn id_of(&mut self, path: &ModulePath) -> Result<ModuleId, PatchError> {
        if let Some(id) = self.by_path.get(path) {
            return Ok(*id);
        }
        let id = ModuleId(self.next);
        if id > ModuleId::MAX {
            return Err(LoomError::ModuleIdOutOfRange(id).into());
        }
        self.next += 1;
        self.by_path.insert(path.clone(), id);
        self.by_id.insert(id, path.clone());
        Ok(id)
    }

    /// The path that owns `id`, if any.
    pub fn path_of(&self, id: ModuleId) -> Option<&ModulePath> {
        self.by_id.get(&id)
    }

    /// Every path and its id, in path order.
    pub fn paths(&self) -> impl Iterator<Item = (&ModulePath, ModuleId)> {
        self.by_path.iter().map(|(path, id)| (path, *id))
    }

    /// The id `path` has, if it has one. Hands out nothing.
    pub fn get(&self, path: &ModulePath) -> Option<ModuleId> {
        self.by_path.get(path).copied()
    }

    /// The next id the table would hand out.
    pub fn next_free(&self) -> u32 {
        self.next
    }

    /// A new top-level path and its id, which are the same number: the
    /// editor gives a new top-level module its id here, never from the
    /// controller, so no inner module can take it later.
    pub fn next_top(&mut self) -> Result<ModuleId, PatchError> {
        let id = ModuleId(self.next);
        if id > ModuleId::MAX {
            return Err(LoomError::ModuleIdOutOfRange(id).into());
        }
        self.reserve(ModulePath::top(id.0), id)?;
        Ok(id)
    }

    /// Moves the id of `old` to `new`, which must be free: the module
    /// keeps its id and its state while it changes level (collapsing and
    /// dissolving).
    pub fn rename(&mut self, old: &ModulePath, new: ModulePath) -> Result<(), PatchError> {
        if let Some(taken) = self.by_path.get(&new) {
            return Err(LoomError::DuplicateModuleId(*taken).into());
        }
        let Some(id) = self.by_path.remove(old) else {
            return Ok(());
        };
        self.by_path.insert(new.clone(), id);
        self.by_id.insert(id, new);
        Ok(())
    }
}

/// One module of the flat graph. `id` is its flat id; `module.id` is its
/// own id in its level of the file, the last element of `path`.
#[derive(Debug, Clone)]
pub struct FlatModule {
    /// Where it came from.
    pub path: ModulePath,
    /// Its id in the flat graph.
    pub id: ModuleId,
    /// The module, resolved.
    pub module: ResolvedModule,
}

/// A patch with every instance expanded: what [`Flat::apply_to`] adds to
/// a controller. Cables, assignments, and mappings use flat ids.
#[derive(Debug, Clone)]
pub struct Flat {
    /// Every module, in visit order: depth first, ids ascending.
    pub modules: Vec<FlatModule>,
    /// Cables between flat modules.
    pub cables: Vec<PatchCable>,
    /// Assignments, in the order they are summed: each instance's inner
    /// ones first, instances in id order, then the level's own.
    pub modulation: Vec<PatchModulation>,
    /// MIDI mappings on flat modules.
    pub midi: Vec<PatchMidiMapping>,
    /// What flattened differently from the file.
    pub warnings: Vec<LoadWarning>,
    /// The top-level Transport, if the file has one.
    pub transport: Option<ModuleId>,
}

/// What stands behind an output port of a definition.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    /// An inner module's output.
    Module(ModuleId, String),
    /// The definition's own input port of this name: a pass-through.
    Input(String),
}

/// One level, flattened: its modules and links, and how its ports and
/// exposed parameters map to flat modules.
#[derive(Debug, Default)]
struct Inner {
    modules: Vec<FlatModule>,
    cables: Vec<PatchCable>,
    modulation: Vec<PatchModulation>,
    midi: Vec<PatchMidiMapping>,
    /// Input port name to the inner inputs it feeds.
    inputs: BTreeMap<String, Vec<(ModuleId, String)>>,
    /// Output port name to what is behind it, if anything.
    outputs: BTreeMap<String, Option<Source>>,
    /// Exposed name to the flat module and parameter it stands for.
    exposed: BTreeMap<String, (ModuleId, String)>,
}

/// What flattening carries down the levels.
struct Context<'a> {
    registry: &'a Registry,
    ids: &'a mut FlatIds,
    table: &'a BTreeMap<String, SubPatch>,
    warnings: Vec<LoadWarning>,
    /// Unknown kinds in visit order, for one aggregated note.
    unknown_kinds: Vec<String>,
    /// The definitions being flattened, outermost first: keys and names.
    stack: Vec<(String, String)>,
    /// Each definition key and the first instance that used it. No two
    /// instances share a definition (sub-patches.md, Patch file).
    used: BTreeMap<String, ModulePath>,
}

/// A module of one level, sorted into what flattening does with it.
enum Member {
    /// An ordinary module, resolved, with its flat id.
    Module(ModuleId, ResolvedModule),
    /// An instance, flattened.
    Instance(Inner),
    /// A Patch input of a definition, with its port name.
    Input(String),
    /// A Patch output of a definition, with its port name.
    Output(String),
}

impl Patch {
    /// Expands every instance into its definition's modules (flattening
    /// rules 1 to 6). Top-level modules keep their file ids; inner modules
    /// take ids from `ids`. Nothing is added anywhere.
    pub fn flatten(&self, registry: &Registry, ids: &mut FlatIds) -> Result<Flat, PatchError> {
        let transports: Vec<u32> = self
            .modules
            .iter()
            .filter(|m| is_transport(registry, &m.kind))
            .map(|m| m.id)
            .collect();
        if let [first, second, ..] = transports[..] {
            return Err(PatchError::TwoTransports { first, second });
        }
        // Every top-level id first, instances included, so no inner module
        // can take the id of a top-level module visited later.
        for module in &self.modules {
            ids.reserve(ModulePath::top(module.id), ModuleId(module.id))?;
        }
        let mut ctx = Context {
            registry,
            ids,
            table: &self.subpatches,
            warnings: Vec::new(),
            unknown_kinds: Vec::new(),
            stack: Vec::new(),
            used: BTreeMap::new(),
        };
        let inner = flatten_level(&mut ctx, self.level(), &ModulePath(Vec::new()), None)?;
        let mut warnings = ctx.warnings;
        if !ctx.unknown_kinds.is_empty() {
            warnings.push(LoadWarning::UnknownModules {
                kinds: ctx.unknown_kinds,
            });
        }
        let flat = Flat {
            modules: inner.modules,
            cables: inner.cables,
            modulation: inner.modulation,
            midi: inner.midi,
            warnings,
            transport: transports.first().map(|&id| ModuleId(id)),
        };
        flat.check_midi(ctx.ids)?;
        Ok(flat)
    }

    /// Adds every module and cable to a controller and commits.
    ///
    /// The controller should be empty. Module IDs from the file are kept
    /// for top-level modules; modules inside sub-patches are given ids by
    /// flattening ([`Patch::flatten`]). Returns a [`LoadWarning`] for each
    /// thing that loaded differently from the file: an unknown module, an
    /// added Transport, or a saved control range that the parameter does
    /// not offer, or that does not hold the saved value. Values never
    /// change (control-ranges.md, rule 7).
    ///
    /// Every patch has exactly one Transport (`docs/design/clock.md`). Two
    /// fail with an error that names both. A patch without one gets one,
    /// with a note, when the registry has the Transport module.
    pub fn load_into(&self, controller: &mut Controller) -> Result<Vec<LoadWarning>, PatchError> {
        self.load_with_ids(controller, &mut FlatIds::new())
    }

    /// [`Patch::load_into`] with the id table kept: the editor keeps it
    /// for the session, so every tile has a flat id (ADR-0039).
    pub fn load_with_ids(
        &self,
        controller: &mut Controller,
        ids: &mut FlatIds,
    ) -> Result<Vec<LoadWarning>, PatchError> {
        let flat = self.flatten(controller.registry(), ids)?;
        flat.apply_to(controller, ids)
    }
}

impl Flat {
    /// Adds every module, cable, assignment, and mapping to `controller`
    /// and commits. Adds a Transport, with a note, when the patch has none
    /// and the registry has the module; its id comes from `ids`, above
    /// every id the file uses, instances included.
    pub fn apply_to(&self, controller: &mut Controller, ids: &mut FlatIds) -> Result<Vec<LoadWarning>, PatchError> {
        let mut notes = self.warnings.clone();
        for module in &self.modules {
            module.module.add_to(controller, module.id)?;
        }
        if self.transport.is_none() && controller.registry().resolve(TRANSPORT_MODULE).is_ok() {
            // From the table, not the controller: a top-level instance owns
            // its id without a graph module, and the controller would not
            // know to skip it.
            let id = ids.next_top()?;
            controller.add_module_with_id(id, TRANSPORT_MODULE)?;
            notes.push(LoadWarning::TransportAdded { module: id.0 });
        }
        for cable in &self.cables {
            controller.connect_named(ModuleId(cable.from.0), &cable.from.1, ModuleId(cable.to.0), &cable.to.1)?;
        }
        for assignment in &self.modulation {
            let id = controller.add_assignment_named(
                ModuleId(assignment.source.0),
                &assignment.source.1,
                ModuleId(assignment.target.0),
                &assignment.target.1,
                assignment.depth,
            )?;
            if let Some(polarity) = assignment.polarity {
                controller.set_assignment_polarity(id, Some(polarity.into()))?;
            }
            if !assignment.enabled {
                controller.set_assignment_enabled(id, false)?;
            }
        }
        for mapping in &self.midi {
            let control = MidiControl {
                channel: mapping.channel,
                controller: mapping.controller,
            };
            controller
                .map_midi_named_with(
                    control,
                    ModuleId(mapping.target.0),
                    &mapping.target.1,
                    mapping.options(),
                )
                .map_err(|error| match error {
                    LoomError::NoSuchParam { .. } => PatchError::UnknownParam {
                        module: mapping.target.0,
                        param: mapping.target.1.clone(),
                    },
                    LoomError::InvalidMidiOptions(reason) => PatchError::InvalidMidiMapping {
                        module: mapping.target.0,
                        param: mapping.target.1.clone(),
                        reason,
                    },
                    other => PatchError::Engine(other),
                })?;
        }
        controller.commit()?;
        Ok(notes)
    }

    /// Refuses two mappings on one flat parameter or one control. Each
    /// level was checked with file ids; this catches an outer mapping on an
    /// exposed name and an inner one on the same parameter, and names
    /// both by path.
    fn check_midi(&self, ids: &FlatIds) -> Result<(), PatchError> {
        let mut targets = std::collections::BTreeSet::new();
        let mut controls = std::collections::BTreeSet::new();
        let path = |id: u32| {
            ids.path_of(ModuleId(id))
                .map_or_else(|| id.to_string(), ToString::to_string)
        };
        for mapping in &self.midi {
            if !targets.insert(&mapping.target) {
                return Err(PatchError::DuplicateMidiMapping {
                    what: format!("parameter `{}` of module {}", mapping.target.1, path(mapping.target.0)),
                });
            }
            if !controls.insert((mapping.channel, mapping.controller)) {
                return Err(PatchError::DuplicateMidiMapping {
                    what: format!("controller {} on channel {}", mapping.controller, mapping.channel),
                });
            }
        }
        Ok(())
    }
}

/// True for the Transport module's kind, at the registry's version.
fn is_transport(registry: &Registry, kind: &str) -> bool {
    loom_core::ModuleKind::parse(kind).ok().is_some_and(|kind| {
        registry
            .get(&kind.id)
            .is_some_and(|descriptor| descriptor.id == TRANSPORT_MODULE && descriptor.version == kind.version)
    })
}

/// Flattens one level. `path` is the instance path of this level (empty at
/// the top), and `definition` is the definition it is the inside of, or
/// `None` at the top level, where Patch inputs and outputs are ordinary
/// modules.
fn flatten_level(
    ctx: &mut Context<'_>,
    level: LevelRef<'_>,
    path: &ModulePath,
    definition: Option<&SubPatch>,
) -> Result<Inner, PatchError> {
    let top = definition.is_none();
    let mut modules: Vec<&PatchModule> = level.modules.iter().collect();
    modules.sort_by_key(|m| m.id);
    let mut seen = std::collections::BTreeSet::new();
    for module in &modules {
        if !seen.insert(module.id) {
            return Err(LoomError::DuplicateModuleId(ModuleId(module.id)).into());
        }
    }
    level.check_midi()?;
    let mut members: BTreeMap<u32, Member> = BTreeMap::new();

    // Pass 1: each module becomes a flat module, a flattened instance, or
    // a port, in id order, so ids are handed out the same way every time.
    let mut inner = Inner::default();
    for module in &modules {
        let member = if module.is_instance() || module.subpatch.is_some() {
            Member::Instance(flatten_instance(ctx, module, path)?)
        } else if !top && module.kind == INPUT_KIND {
            Member::Input(port_name(module, "in"))
        } else if !top && module.kind == OUTPUT_KIND {
            Member::Output(port_name(module, "out"))
        } else {
            if !top && is_transport(ctx.registry, &module.kind) {
                return Err(PatchError::TransportInSubPatch { module: module.id });
            }
            let mut warnings = Vec::new();
            let resolved = level
                .resolve_module(module, ctx.registry, ResolveOptions::FILE, &mut warnings)
                .map_err(PatchError::from)?;
            ctx.warnings.extend(warnings);
            if let Some(unknown) = &resolved.unknown {
                ctx.unknown_kinds.push(unknown.kind.clone());
            }
            let id = if top {
                ModuleId(module.id)
            } else {
                ctx.ids.id_of(&path.child(module.id))?
            };
            Member::Module(id, resolved)
        };
        members.insert(module.id, member);
    }

    // A definition's ports, each named once.
    let mut input_ports: BTreeMap<String, u32> = BTreeMap::new();
    let mut output_ports: BTreeMap<String, u32> = BTreeMap::new();
    for (id, member) in &members {
        let (ports, name, direction) = match member {
            Member::Input(name) => (&mut input_ports, name, "input"),
            Member::Output(name) => (&mut output_ports, name, "output"),
            _ => continue,
        };
        if ports.insert(name.clone(), *id).is_some() {
            return Err(PatchError::DuplicatePort {
                direction,
                port: name.clone(),
            });
        }
    }
    for name in input_ports.keys() {
        inner.inputs.insert(name.clone(), Vec::new());
    }
    for name in output_ports.keys() {
        inner.outputs.insert(name.clone(), None);
    }

    // Exposed parameters, before the links, so an exposed name on a
    // placeholder is one of its parameters when an assignment reaches it.
    if let Some(definition) = definition {
        for exposed in &definition.exposed {
            if let Some((min, max)) = exposed.range
                && !(min.is_finite() && max.is_finite() && min < max)
            {
                return Err(PatchError::BadExposed {
                    name: exposed.name.clone(),
                    reason: format!("its range ({min}, {max}) is not a finite span"),
                });
            }
            let target = match members.get(&exposed.module) {
                Some(Member::Module(id, resolved)) => {
                    // A placeholder cannot say whether it has the parameter;
                    // the file says so (unknown-modules.md, rule 1).
                    if resolved.unknown.is_none() && resolved.parameter_index(&exposed.param).is_none() {
                        return Err(PatchError::BadExposed {
                            name: exposed.name.clone(),
                            reason: format!("module {} has no parameter `{}`", exposed.module, exposed.param),
                        });
                    }
                    (*id, exposed.param.clone())
                }
                Some(Member::Instance(child)) => match child.exposed.get(&exposed.param) {
                    Some(target) => target.clone(),
                    None => {
                        return Err(PatchError::BadExposed {
                            name: exposed.name.clone(),
                            reason: format!("sub-patch {} exposes no `{}`", exposed.module, exposed.param),
                        });
                    }
                },
                Some(Member::Input(_) | Member::Output(_)) => {
                    return Err(PatchError::BadExposed {
                        name: exposed.name.clone(),
                        reason: format!("module {} is a port and has no parameters", exposed.module),
                    });
                }
                None => {
                    return Err(PatchError::BadExposed {
                        name: exposed.name.clone(),
                        reason: format!("there is no module {}", exposed.module),
                    });
                }
            };
            if inner.exposed.insert(exposed.name.clone(), target).is_some() {
                return Err(PatchError::BadExposed {
                    name: exposed.name.clone(),
                    reason: "it is exposed twice".to_string(),
                });
            }
        }
        // A placeholder takes the exposed parameters that reach it.
        for (id, param) in inner.exposed.values() {
            for member in members.values_mut() {
                if let Member::Module(flat, resolved) = member
                    && flat == id
                    && let Some(unknown) = &mut resolved.unknown
                    && !unknown.parameters.contains(param)
                {
                    unknown.parameters.push(param.clone());
                }
            }
        }
    }

    // Pass 2: links. What feeds each instance input, for pass-through
    // ports, in last-cable-wins order as the controller connects.
    let mut feeds: BTreeMap<(u32, String), (u32, String)> = BTreeMap::new();
    for cable in level.cables {
        if matches!(members.get(&cable.to.0), Some(Member::Instance(_))) {
            feeds.insert(cable.to.clone(), cable.from.clone());
        }
    }
    let lookup = Lookup {
        members: &members,
        feeds: &feeds,
    };
    // Inside a definition, two cables into one input are refused: a flat
    // patch keeps the later one, and flattening would not keep that order
    // across the boundary.
    let mut fed = std::collections::BTreeSet::new();
    for cable in level.cables {
        if !top && !fed.insert(&cable.to) {
            return Err(PatchError::TwoCablesIntoOneInput {
                module: cable.to.0,
                port: cable.to.1.clone(),
            });
        }
        let source = lookup.source_of(&cable.from, &mut Vec::new())?;
        match members.get(&cable.to.0) {
            Some(Member::Output(name)) => {
                if cable.to.1 != "in" {
                    return Err(no_such_port(cable.to.0, "input", &cable.to.1));
                }
                inner.outputs.insert(name.clone(), source);
            }
            _ => {
                for (to, input) in lookup.dests_of(&cable.to)? {
                    match &source {
                        Some(Source::Module(from, output)) => inner.cables.push(PatchCable {
                            from: (from.0, output.clone()),
                            to: (to.0, input.clone()),
                        }),
                        Some(Source::Input(port)) => inner
                            .inputs
                            .get_mut(port)
                            .expect("every input port is listed")
                            .push((to, input)),
                        // Nothing behind the port: the input reads its default.
                        None => {}
                    }
                }
            }
        }
    }
    // This level's own assignments and mappings go after each instance's
    // inner ones (below): the engine sums assignments in this order.
    let mut own_modulation = Vec::new();
    let mut own_midi = Vec::new();
    for assignment in level.modulation {
        let source = match lookup.source_of(&assignment.source, &mut Vec::new())? {
            Some(Source::Module(id, output)) => (id.0, output),
            Some(Source::Input(_)) => {
                return Err(PatchError::AssignmentFromPort {
                    module: assignment.source.0,
                });
            }
            // Nothing behind the port: the assignment moves nothing.
            None => continue,
        };
        let (target, param) = lookup.param_of(&assignment.target)?;
        own_modulation.push(PatchModulation {
            source,
            target: (target.0, param),
            ..assignment.clone()
        });
    }
    for mapping in level.midi {
        let (target, param) = lookup.param_of(&mapping.target)?;
        own_midi.push(PatchMidiMapping {
            target: (target.0, param),
            ..mapping.clone()
        });
    }

    // The modules, depth first, in id order; each instance's inner
    // assignments first, instances in id order, then this level's own.
    for (_, member) in members {
        match member {
            Member::Module(id, module) => inner.modules.push(FlatModule {
                path: path.child(module.id),
                id,
                module,
            }),
            Member::Instance(mut child) => {
                inner.modules.append(&mut child.modules);
                inner.cables.append(&mut child.cables);
                inner.modulation.append(&mut child.modulation);
                inner.midi.append(&mut child.midi);
            }
            _ => {}
        }
    }
    inner.modulation.append(&mut own_modulation);
    inner.midi.append(&mut own_midi);
    Ok(inner)
}

/// Flattens the instance `module` of this level: its definition with the
/// instance's exposed values written in. Errors inside are wrapped with the
/// definition's name.
fn flatten_instance(ctx: &mut Context<'_>, module: &PatchModule, path: &ModulePath) -> Result<Inner, PatchError> {
    let key = match (&module.subpatch, module.kind == SUBPATCH_KIND) {
        (Some(key), true) => key,
        _ => {
            return Err(PatchError::NotAnInstance {
                module: module.id,
                kind: module.kind.clone(),
            });
        }
    };
    let Some(definition) = ctx.table.get(key) else {
        return Err(PatchError::UnknownSubPatch {
            module: module.id,
            key: key.clone(),
        });
    };
    let name = definition.metadata.name.clone();
    if ctx.stack.len() >= MAX_DEPTH {
        return Err(PatchError::TooDeep { max: MAX_DEPTH });
    }
    if let Some(start) = ctx.stack.iter().position(|(k, _)| k == key) {
        let mut loop_path: Vec<String> = ctx.stack[start..].iter().map(|(_, n)| n.clone()).collect();
        loop_path.push(name);
        return Err(PatchError::SubPatchLoop { path: loop_path });
    }
    let child_path = path.child(module.id);
    if let Some(first) = ctx.used.insert(key.clone(), child_path.clone()) {
        return Err(PatchError::SharedSubPatch {
            key: key.clone(),
            first: first.to_string(),
            second: child_path.to_string(),
        });
    }
    if !module.ranges.is_empty() {
        let param = module.ranges.keys().next().expect("not empty").clone();
        return Err(PatchError::UnknownParam {
            module: module.id,
            param,
        });
    }
    // The instance's values, written into the definition's modules before
    // they are resolved, clamped to the exposed range. A saved control
    // range on that parameter is dropped, so the loader picks one that
    // holds the value (control-ranges.md, rule 7).
    let mut modules = definition.modules.clone();
    for (name, &value) in &module.params {
        let Some(exposed) = definition.exposed.iter().find(|e| &e.name == name) else {
            return Err(PatchError::UnknownParam {
                module: module.id,
                param: name.clone(),
            });
        };
        let value = match exposed.range {
            Some((min, max)) if min.is_finite() && max.is_finite() && min < max => value.clamp(min, max),
            _ => value,
        };
        let Some(inner) = modules.iter_mut().find(|m| m.id == exposed.module) else {
            // Reported as a bad exposed entry when the definition is
            // flattened below.
            continue;
        };
        inner.params.insert(exposed.param.clone(), value);
        inner.ranges.remove(&exposed.param);
    }
    let inside = LevelRef {
        modules: &modules,
        cables: &definition.cables,
        modulation: &definition.modulation,
        midi: &definition.midi,
    };
    ctx.stack.push((key.clone(), name.clone()));
    let first_new_warning = ctx.warnings.len();
    let result = flatten_level(ctx, inside, &child_path, Some(definition));
    ctx.stack.pop();
    // What the inside reports says which sub-patch it is in.
    for warning in ctx.warnings.drain(first_new_warning..).collect::<Vec<_>>() {
        ctx.warnings.push(LoadWarning::InSubPatch {
            name: name.clone(),
            warning: Box::new(warning),
        });
    }
    result.map_err(|source| PatchError::InSubPatch {
        name,
        source: Box::new(source),
    })
}

/// The port name a Patch input or output gives its sub-patch: its label,
/// else `fallback`.
pub(crate) fn port_name(module: &PatchModule, fallback: &str) -> String {
    module
        .label
        .as_deref()
        .filter(|label| !label.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn no_such_port(module: u32, direction: &'static str, port: &str) -> PatchError {
    LoomError::NoSuchPort {
        module: ModuleId(module),
        direction,
        port: port.to_string(),
    }
    .into()
}

/// Name lookups over one level's members, for pass 2.
struct Lookup<'a> {
    members: &'a BTreeMap<u32, Member>,
    /// What feeds each instance input in this level.
    feeds: &'a BTreeMap<(u32, String), (u32, String)>,
}

impl Lookup<'_> {
    /// What stands behind the output `from` of this level, or `None` when
    /// nothing does. `seen` guards a loop of pass-through ports, which
    /// carries nothing.
    fn source_of(&self, from: &(u32, String), seen: &mut Vec<(u32, String)>) -> Result<Option<Source>, PatchError> {
        let (id, port) = from;
        match self.members.get(id) {
            Some(Member::Module(flat, resolved)) => {
                if resolved.output_index(port).is_none() {
                    return Err(no_such_port(*id, "output", port));
                }
                Ok(Some(Source::Module(*flat, port.clone())))
            }
            Some(Member::Input(name)) => {
                if port != "out" {
                    return Err(no_such_port(*id, "output", port));
                }
                Ok(Some(Source::Input(name.clone())))
            }
            Some(Member::Instance(child)) => match child.outputs.get(port) {
                None => Err(no_such_port(*id, "output", port)),
                Some(None) => Ok(None),
                Some(Some(Source::Module(flat, output))) => Ok(Some(Source::Module(*flat, output.clone()))),
                // A pass-through: whatever feeds the instance's input.
                Some(Some(Source::Input(input))) => {
                    let key = (*id, input.clone());
                    if seen.contains(&key) {
                        return Ok(None);
                    }
                    seen.push(key.clone());
                    match self.feeds.get(&key) {
                        Some(feed) => self.source_of(feed, seen),
                        None => Ok(None),
                    }
                }
            },
            Some(Member::Output(_)) => Err(no_such_port(*id, "output", port)),
            None => Err(LoomError::UnknownModule(ModuleId(*id)).into()),
        }
    }

    /// The flat inputs the input `to` of this level stands for: one for a
    /// module, several for an instance port that fans out inside.
    fn dests_of(&self, to: &(u32, String)) -> Result<Vec<(ModuleId, String)>, PatchError> {
        let (id, port) = to;
        match self.members.get(id) {
            Some(Member::Module(flat, resolved)) => {
                if resolved.input_index(port).is_none() {
                    return Err(no_such_port(*id, "input", port));
                }
                Ok(vec![(*flat, port.clone())])
            }
            Some(Member::Instance(child)) => child
                .inputs
                .get(port)
                .cloned()
                .ok_or_else(|| no_such_port(*id, "input", port)),
            Some(Member::Input(_) | Member::Output(_)) => Err(no_such_port(*id, "input", port)),
            None => Err(LoomError::UnknownModule(ModuleId(*id)).into()),
        }
    }

    /// The flat parameter the parameter `target` of this level stands for.
    fn param_of(&self, target: &(u32, String)) -> Result<(ModuleId, String), PatchError> {
        let (id, param) = target;
        let no_such_param = || PatchError::UnknownParam {
            module: *id,
            param: param.clone(),
        };
        match self.members.get(id) {
            Some(Member::Module(flat, resolved)) => {
                if resolved.parameter_index(param).is_none() {
                    return Err(no_such_param());
                }
                Ok((*flat, param.clone()))
            }
            Some(Member::Instance(child)) => child.exposed.get(param).cloned().ok_or_else(no_such_param),
            Some(Member::Input(_) | Member::Output(_)) => Err(no_such_param()),
            None => Err(LoomError::UnknownModule(ModuleId(*id)).into()),
        }
    }
}

#[cfg(test)]
mod tests;
