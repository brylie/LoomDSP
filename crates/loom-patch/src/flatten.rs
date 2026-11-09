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

use crate::hierarchy::fold_ranges;
use crate::level::LevelRef;
use crate::modes::{
    MEMBER_KIND, MEMBER_PARAM, MOST_MEMBERS, Mode, SELECT_KIND, SPREAD_SOURCE_KIND, SPREAD_SUM_KIND, SUM_KIND,
};
use crate::{
    INPUT_KIND, LoadWarning, OUTPUT_KIND, Patch, PatchCable, PatchError, PatchMidiMapping, PatchModulation,
    PatchModule, ResolveOptions, ResolvedModule, SLOT_KIND, SUBPATCH_KIND, SubPatch,
};
use loom_core::SignalType;
use loom_core::{
    Controller, FillsRole, Graph, LoomError, MidiControl, ModuleId, ModuleNames, ParamRange, Registry, RoleDescriptor,
    TRANSPORT_MODULE,
};
use loom_dsp::blend;
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

    /// The path of copy `k` (from 1) of the stacked instance at this path:
    /// a step no module id can take ([`FIRST_RESERVED_STEP`]). Copy 0 is the
    /// instance's own path (sub-patches.md, Copies in the editor).
    pub fn copy(&self, k: u32) -> Self {
        self.child(COPY_STEP + k)
    }

    /// The path of the `n`th module the flattener adds for the mode of the
    /// instance at this path (sub-patches.md, The modules the flattener
    /// adds): a step no module id can take.
    pub fn added(&self, n: u32) -> Self {
        self.child(ADDED_STEP + n)
    }

    /// True for a path the flattener makes for a mode: a stack copy past
    /// the first, or a module it adds. Such a path is implied (ADR-0040):
    /// never a tile, never saved.
    pub fn is_implied(&self) -> bool {
        // The first step is a top-level module's own flat id, which may be
        // any id; reserved steps come only after an instance.
        self.0.iter().skip(1).any(|&step| step >= FIRST_RESERVED_STEP)
    }

    /// The path with every copy step left out and the copy index of each
    /// stack it passes through, outer first; `None` for a path with an added
    /// step. Copy 0 of every stack has no copy step, so its path is the
    /// path the editor shows.
    pub fn without_copies(&self) -> Option<(Self, Vec<u32>)> {
        let mut ids = Vec::new();
        let mut copies = Vec::new();
        for (n, &step) in self.0.iter().enumerate() {
            if n == 0 {
                ids.push(step);
                continue;
            }
            if (ADDED_STEP..COPY_STEP).contains(&step) {
                return None;
            }
            if step >= COPY_STEP {
                copies.push(step - COPY_STEP);
            } else {
                ids.push(step);
            }
        }
        Some((Self(ids), copies))
    }
}

/// The first path step a module id cannot take. A file whose level ids reach
/// it is refused, so copies and added modules never meet a real id.
pub const FIRST_RESERVED_STEP: u32 = ADDED_STEP;
/// The base of the steps of added modules ([`ModulePath::added`]).
pub const ADDED_STEP: u32 = 0xE000_0000;
/// The base of the steps of stack copies ([`ModulePath::copy`]).
pub const COPY_STEP: u32 = 0xF000_0000;
/// The added step of a select's Member module.
pub const MEMBER_STEP: u32 = 0x0FFF_FFFF;
/// The added step of a stack's spread source.
pub const SPREAD_STEP: u32 = 0x0FFF_FFFE;

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

    /// Forgets `path` and its id. The id is never handed out again, since
    /// `next` is not lowered (rule 4).
    pub fn forget(&mut self, path: &ModulePath) {
        if let Some(id) = self.by_path.remove(path) {
            self.by_id.remove(&id);
        }
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
    /// What the stack copies it is in added to a `seed` parameter: 0 in
    /// copy 0 of every stack (sub-patches.md, Copies in the editor).
    pub seed_offset: u32,
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
    /// The range MIDI sweeps on each flat parameter a sub-patch surface
    /// exposes with a range: every level's range on the way in, outer first,
    /// as the surface control spans it (sub-patches.md, Exposing). Not in
    /// the file as such; the loader hands it to
    /// [`Controller::set_limits`].
    pub limits: BTreeMap<(ModuleId, String), (f32, f32)>,
    /// What flattened differently from the file.
    pub warnings: Vec<LoadWarning>,
    /// The top-level Transport, if the file has one.
    pub transport: Option<ModuleId>,
    /// For a mapped parameter of copy 0 of a stack, the same parameter of
    /// every other copy, which takes every value the mapping sets: a control
    /// has one mapping ([`Controller::set_midi_followers`]; ADR-0040).
    pub followers: BTreeMap<(ModuleId, String), Vec<(ModuleId, String)>>,
    /// For each module of copy 0 of a stack, the same module in every other
    /// copy, with the seed offset that copy adds to a `seed` parameter
    /// (sub-patches.md, Copies in the editor). A parameter write reaches
    /// them all.
    pub copies: BTreeMap<ModuleId, Vec<(ModuleId, u32)>>,
    /// Every module the flattener added for a mode, with what it stands for.
    pub added: BTreeMap<ModuleId, Added>,
}

/// A mapped parameter and the parameters that follow its mapping, by
/// parameter index, as [`Controller::set_midi_followers`] takes them.
pub type Followers = ((ModuleId, usize), Vec<(ModuleId, usize)>);

/// What a module the flattener adds for a mode stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    /// The sum or select behind an output port of the instance at
    /// `instance`.
    Output {
        /// The instance's path.
        instance: ModulePath,
        /// The output port's name.
        port: String,
    },
    /// The Member module of the select instance at `instance`.
    Member {
        /// The instance's path.
        instance: ModulePath,
    },
    /// The constant source of the spreads of the stack at `instance`.
    Spread {
        /// The instance's path.
        instance: ModulePath,
    },
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
    /// Exposed name to every finite range on the way in, outer first.
    chains: BTreeMap<String, Vec<(f32, f32)>>,
    /// Flat parameter to the range MIDI sweeps on it. See [`Flat::limits`].
    limits: BTreeMap<(ModuleId, String), (f32, f32)>,
    /// See [`Flat::followers`].
    followers: BTreeMap<(ModuleId, String), Vec<(ModuleId, String)>>,
    /// See [`Flat::copies`].
    copies: BTreeMap<ModuleId, Vec<(ModuleId, u32)>>,
    /// See [`Flat::added`].
    added: BTreeMap<ModuleId, Added>,
    /// Each instance member of this level, by level id: its ports as they
    /// were before its modules joined this level's, for a layer or select,
    /// whose ports come from its members.
    children: BTreeMap<u32, ChildPorts>,
}

/// The ports of one flattened instance.
#[derive(Debug, Default, Clone)]
struct ChildPorts {
    inputs: BTreeMap<String, Vec<(ModuleId, String)>>,
    outputs: BTreeMap<String, Option<Source>>,
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
    /// What one step of copy index adds to [`Context::seed_offset`] at this
    /// depth of stacks: 1 outside any stack, times 16 inside each.
    seed_stride: u32,
    /// More than 0 while flattening a stack copy past the first, whose
    /// nested instances use the keys copy 0 used.
    in_copy: usize,
    /// What a `seed` parameter gains in the copy being flattened: 0 in
    /// copy 0 of every stack (sub-patches.md, Copies in the editor).
    seed_offset: u32,
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
    /// A slot: its role, and the level id of its current part, which is
    /// another member of the level (sub-patches.md, Slots).
    Slot(SlotMember),
}

/// A slot of one level, as the flattener routes through it.
struct SlotMember {
    role: &'static RoleDescriptor,
    part: Option<u32>,
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
            in_copy: 0,
            seed_offset: 0,
            seed_stride: 1,
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
            limits: inner.limits,
            warnings,
            transport: transports.first().map(|&id| ModuleId(id)),
            followers: inner.followers,
            copies: inner.copies,
            added: inner.added,
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
        let same = |id: ModuleId| id;
        self.add_modules(controller, &same)?;
        if self.transport.is_none() && controller.registry().resolve(TRANSPORT_MODULE).is_ok() {
            // From the table, not the controller: a top-level instance owns
            // its id without a graph module, and the controller would not
            // know to skip it.
            let id = ids.next_top()?;
            controller.add_module_with_id(id, TRANSPORT_MODULE)?;
            notes.push(LoadWarning::TransportAdded { module: id.0 });
        }
        self.add_links(controller, &same)?;
        controller.set_limits(self.limits_in(controller.graph()))?;
        controller.set_midi_followers(self.followers_in(controller.graph()))?;
        controller.commit()?;
        Ok(notes)
    }

    /// [`Flat::followers`] as [`Controller::set_midi_followers`] takes them:
    /// by parameter index, for the modules `graph` holds.
    pub fn followers_in(&self, graph: &Graph) -> Vec<Followers> {
        let index = |(module, param): &(ModuleId, String)| {
            let node = graph.node(*module)?;
            Some((*module, node.parameter_index(param)?))
        };
        self.followers
            .iter()
            .filter_map(|(target, list)| Some((index(target)?, list.iter().filter_map(index).collect())))
            .collect()
    }

    /// Adds every module, then every cable, assignment, and mapping, to a
    /// controller that holds a patch already, with `map` giving each flat
    /// id the id it takes there: a sub-patch inserted into the open patch
    /// ([`crate::Hierarchy::insert`]). No Transport is added and nothing
    /// is committed.
    pub fn add_to(&self, controller: &mut Controller, map: &dyn Fn(ModuleId) -> ModuleId) -> Result<(), PatchError> {
        self.add_modules(controller, map)?;
        self.add_links(controller, map)
    }

    fn add_modules(&self, controller: &mut Controller, map: &dyn Fn(ModuleId) -> ModuleId) -> Result<(), PatchError> {
        for module in &self.modules {
            module.module.add_to(controller, map(module.id))?;
        }
        Ok(())
    }

    fn add_links(&self, controller: &mut Controller, map: &dyn Fn(ModuleId) -> ModuleId) -> Result<(), PatchError> {
        for cable in &self.cables {
            controller.connect_named(
                map(ModuleId(cable.from.0)),
                &cable.from.1,
                map(ModuleId(cable.to.0)),
                &cable.to.1,
            )?;
        }
        for assignment in &self.modulation {
            let id = controller.add_assignment_named(
                map(ModuleId(assignment.source.0)),
                &assignment.source.1,
                map(ModuleId(assignment.target.0)),
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
                    map(ModuleId(mapping.target.0)),
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
        Ok(())
    }

    /// [`Flat::limits`] as [`Controller::set_limits`] takes them: by
    /// parameter index, for the modules `graph` holds. A placeholder for an
    /// unknown module is left out: it plays nothing, so there is nothing for
    /// MIDI to sweep. A limit is cut to the parameter's own bounds, and one
    /// with nothing left, which only a hand-written file can make, is left
    /// out: the value is clamped by the parameter anyway, and the file still
    /// loads.
    pub fn limits_in(&self, graph: &Graph) -> Vec<(ModuleId, usize, ParamRange)> {
        self.limits
            .iter()
            .filter_map(|((module, param), &(min, max))| {
                let node = graph.node(*module).filter(|node| node.unknown.is_none())?;
                let index = node.descriptor.param_index(param)?;
                let spec = node.descriptor.params.get(index)?;
                // `Controller::set_limits` cuts and checks the same way. Both
                // stay: the loader drops an empty limit so the file loads,
                // and the controller refuses one from any other caller.
                let cut = ParamRange::new(min.max(spec.min), max.min(spec.max));
                (cut.min < cut.max).then_some((*module, index, cut))
            })
            .collect()
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
        // A copy or an added module's path step is never a module id.
        if !top && module.id >= FIRST_RESERVED_STEP {
            return Err(LoomError::ModuleIdOutOfRange(ModuleId(module.id)).into());
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
        } else if module.is_slot() || module.slot.is_some() {
            Member::Slot(slot_member(ctx, module)?)
        } else if !top && module.kind == INPUT_KIND {
            Member::Input(port_name(module, "in"))
        } else if !top && module.kind == OUTPUT_KIND {
            Member::Output(port_name(module, "out"))
        } else {
            if !top && is_transport(ctx.registry, &module.kind) {
                return Err(PatchError::TransportInSubPatch { module: module.id });
            }
            let mut warnings = Vec::new();
            let mut resolved = level
                .resolve_module(module, ctx.registry, ResolveOptions::FILE, &mut warnings)
                .map_err(PatchError::from)?;
            offset_seed(&mut resolved, ctx.seed_offset);
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

    // Slots: each part is another member of this level that declares the
    // role, and no two slots share one (sub-patches.md, Patch file).
    let mut owned: BTreeMap<u32, u32> = BTreeMap::new();
    for module in &modules {
        let Some(Member::Slot(slot)) = members.get(&module.id) else {
            continue;
        };
        let bad = |reason: String| PatchError::BadSlot {
            module: module.id,
            reason,
        };
        if let Some(part) = slot.part {
            let fits = match members.get(&part) {
                Some(Member::Module(_, resolved)) => {
                    resolved.unknown.is_some() || resolved.descriptor.fills(slot.role.id).is_some()
                }
                Some(Member::Instance(_)) => modules
                    .iter()
                    .find(|m| m.id == part)
                    .and_then(|m| m.subpatch.as_ref())
                    .and_then(|key| ctx.table.get(key))
                    .is_some_and(|d| d.roles.iter().any(|r| crate::role_id(r) == slot.role.id)),
                Some(Member::Slot(_) | Member::Input(_) | Member::Output(_)) => {
                    return Err(bad(format!("its part, module {part}, is a slot or a port")));
                }
                None => return Err(bad(format!("its part, module {part}, is not in this level"))),
            };
            if !fits {
                return Err(bad(format!(
                    "its part, module {part}, does not fill the role {}",
                    slot.role.kind()
                )));
            }
            if let Some(other) = owned.insert(part, module.id) {
                return Err(bad(format!("module {part} is already the part of slot {other}")));
            }
        }
        if let Some(held) = module.slot.as_ref().map(|s| &s.held) {
            for key in held.keys() {
                if let crate::PartKey::SubPatch(key) = key
                    && !ctx.table.contains_key(key)
                {
                    return Err(bad(format!(
                        "it holds the sub-patch `{key}`, which the patch does not have"
                    )));
                }
            }
        }
    }

    // What feeds each instance or slot input, for pass-through ports, in
    // last-cable-wins order as the controller connects.
    let mut feeds: BTreeMap<(u32, String), (u32, String)> = BTreeMap::new();
    for cable in level.cables {
        if matches!(members.get(&cable.to.0), Some(Member::Instance(_) | Member::Slot(_))) {
            feeds.insert(cable.to.clone(), cable.from.clone());
        }
    }
    // Exposed parameters, before the links, so an exposed name on a
    // placeholder is one of its parameters when an assignment reaches it.
    // Their limits are kept until the instances' own are merged, because
    // this level's chain holds the inner ranges too and so replaces them.
    let mut surface_limits: Vec<((ModuleId, String), (f32, f32))> = Vec::new();
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
                // A slot's role parameter, routed to its part. Nothing behind
                // it (an empty slot, or a part without the optional
                // parameter) makes no entry.
                Some(Member::Slot(slot)) => {
                    if slot.role.param(&exposed.param).is_none() {
                        return Err(PatchError::BadExposed {
                            name: exposed.name.clone(),
                            reason: format!("slot {} has no role parameter `{}`", exposed.module, exposed.param),
                        });
                    }
                    match slot_param(&members, slot, &exposed.param) {
                        Some(target) => target,
                        None => continue,
                    }
                }
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
            // This level's range, then the ranges further in.
            let mut chain: Vec<(f32, f32)> = exposed.range.into_iter().collect();
            if let Some(Member::Instance(child)) = members.get(&exposed.module)
                && let Some(further) = child.chains.get(&exposed.param)
            {
                chain.extend(further);
            }
            if let Some(limit) = fold_ranges(chain.iter().copied()) {
                surface_limits.push((target.clone(), limit));
            }
            inner.chains.insert(exposed.name.clone(), chain);
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

    let lookup = Lookup {
        members: &members,
        feeds: &feeds,
        owned: &owned,
    };
    // Pass 2: links. Inside a definition, two cables into one input are refused: a flat
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
        // Nothing behind a slot's parameter: the assignment moves nothing.
        let Some((target, param)) = lookup.param_of(&assignment.target)? else {
            continue;
        };
        own_modulation.push(PatchModulation {
            source,
            target: (target.0, param),
            ..assignment.clone()
        });
    }
    for mapping in level.midi {
        let Some((target, param)) = lookup.param_of(&mapping.target)? else {
            continue;
        };
        own_midi.push(PatchMidiMapping {
            target: (target.0, param),
            ..mapping.clone()
        });
    }

    // The modules, depth first, in id order; each instance's inner
    // assignments first, instances in id order, then this level's own.
    for (lid, member) in members {
        match member {
            Member::Module(id, module) => inner.modules.push(FlatModule {
                path: path.child(module.id),
                id,
                module,
                seed_offset: ctx.seed_offset,
            }),
            Member::Instance(mut child) => {
                inner.children.insert(
                    lid,
                    ChildPorts {
                        inputs: child.inputs.clone(),
                        outputs: child.outputs.clone(),
                    },
                );
                inner.modules.append(&mut child.modules);
                inner.cables.append(&mut child.cables);
                inner.modulation.append(&mut child.modulation);
                inner.midi.append(&mut child.midi);
                inner.limits.append(&mut child.limits);
                inner.followers.append(&mut child.followers);
                inner.copies.append(&mut child.copies);
                inner.added.append(&mut child.added);
            }
            _ => {}
        }
    }
    inner.limits.extend(surface_limits);
    // An assignment or mapping into a stack reaches every copy: an
    // assignment as one per copy, a mapping as followers, since a control
    // has one mapping (ADR-0040).
    for assignment in own_modulation {
        let copies = inner
            .copies
            .get(&ModuleId(assignment.target.0))
            .cloned()
            .unwrap_or_default();
        let param = assignment.target.1.clone();
        inner.modulation.push(assignment.clone());
        for (copy, _) in copies {
            inner.modulation.push(PatchModulation {
                target: (copy.0, param.clone()),
                ..assignment.clone()
            });
        }
    }
    for mapping in &own_midi {
        let target = (ModuleId(mapping.target.0), mapping.target.1.clone());
        if let Some(copies) = inner.copies.get(&target.0) {
            let list: Vec<(ModuleId, String)> = copies.iter().map(|(copy, _)| (*copy, target.1.clone())).collect();
            inner.followers.entry(target).or_default().extend(list);
        }
    }
    inner.midi.append(&mut own_midi);
    Ok(inner)
}

/// Adds `offset` to a `seed` parameter, wrapped in its range, so copy `k`
/// of a stack plays its own random numbers (sub-patches.md, Copies in the
/// editor). Nothing changes for an offset of 0 or a module with no seed.
pub(crate) fn offset_seed(module: &mut ResolvedModule, offset: u32) {
    if offset == 0 {
        return;
    }
    let Some(index) = module.descriptor.params.iter().position(|p| p.name == SEED_PARAM) else {
        return;
    };
    let spec = &module.descriptor.params[index];
    if let Some(value) = module.params.get_mut(index) {
        *value = seeded(spec.min, spec.max, *value, offset);
    }
}

/// The name of a seed parameter (TR-07).
pub const SEED_PARAM: &str = "seed";

/// `value + offset`, wrapped into the whole numbers from `min` to `max`.
pub fn seeded(min: f32, max: f32, value: f32, offset: u32) -> f32 {
    let span = (f64::from(max) - f64::from(min)).floor() + 1.0;
    if !(span.is_finite() && span >= 1.0) {
        return value;
    }
    let shifted = (f64::from(value) - f64::from(min) + f64::from(offset)).rem_euclid(span);
    (f64::from(min) + shifted) as f32
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
    if ctx.in_copy == 0
        && let Some(first) = ctx.used.insert(key.clone(), child_path.clone())
    {
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
    let mut member = None;
    for (name, &value) in &module.params {
        if definition.mode == Mode::Select && name == MEMBER_PARAM {
            member = Some(value);
            continue;
        }
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
        let Some(inner) = modules.iter().find(|m| m.id == exposed.module) else {
            // Reported as a bad exposed entry when the definition is
            // flattened below.
            continue;
        };
        // Through a slot, the value lands on the part, by its own name
        // (sub-patches.md, Slots). A part without the optional parameter
        // takes nothing.
        let (target, param) = match &inner.slot {
            None => (exposed.module, exposed.param.clone()),
            Some(slot) => {
                let Some(part) = slot.part else { continue };
                let Some(part_module) = modules.iter().find(|m| m.id == part) else {
                    continue;
                };
                let role_id = crate::role_id(&slot.role);
                let own = if part_module.subpatch.is_some() {
                    Some(exposed.param.clone())
                } else {
                    part_own_param(ctx.registry, &part_module.kind, role_id, &exposed.param)
                };
                match own {
                    Some(own) => (part, own),
                    None => continue,
                }
            }
        };
        let inner = modules.iter_mut().find(|m| m.id == target).expect("found above");
        inner.params.insert(param.clone(), value);
        inner.ranges.remove(&param);
    }
    let inside = LevelRef {
        modules: &modules,
        cables: &definition.cables,
        modulation: &definition.modulation,
        midi: &definition.midi,
    };
    ctx.stack.push((key.clone(), name.clone()));
    let first_new_warning = ctx.warnings.len();
    let result = flatten_mode(ctx, inside, &child_path, definition, member);
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

/// Flattens the inside of the instance at `path` by its definition's mode
/// (sub-patches.md, Modes): as it is, as members mixed or selected, or as
/// copies summed. `member` is a select instance's Member value.
fn flatten_mode(
    ctx: &mut Context<'_>,
    inside: LevelRef<'_>,
    path: &ModulePath,
    definition: &SubPatch,
    member: Option<f32>,
) -> Result<Inner, PatchError> {
    if let Some(reason) = definition.mode_problem() {
        return Err(PatchError::BadMode { reason });
    }
    match definition.mode {
        Mode::Single => flatten_level(ctx, inside, path, Some(definition)),
        Mode::Layer | Mode::Select => {
            let inner = flatten_level(ctx, inside, path, Some(definition))?;
            mix_members(ctx, inner, path, definition, member)
        }
        Mode::Stack => flatten_stack(ctx, inside, path, definition),
    }
}

/// A module the flattener adds for a mode, at `path`, of `kind`, with
/// `params` set. Refused when this build lacks the kind, since a
/// placeholder would play silence where the mode needs a sum.
fn added_module(
    ctx: &mut Context<'_>,
    path: &ModulePath,
    kind: &str,
    params: &[(&str, f32)],
) -> Result<FlatModule, PatchError> {
    let mut module = PatchModule::new(path.last(), kind);
    for (name, value) in params {
        module.params.insert((*name).to_string(), *value);
    }
    let empty = LevelRef {
        modules: &[],
        cables: &[],
        modulation: &[],
        midi: &[],
    };
    let mut warnings = Vec::new();
    let resolved = empty
        .resolve_module(&module, ctx.registry, ResolveOptions::FILE, &mut warnings)
        .map_err(PatchError::from)?;
    if resolved.unknown.is_some() {
        return Err(PatchError::BadMode {
            reason: format!("this build has no `{kind}` module, which the mode needs"),
        });
    }
    Ok(FlatModule {
        path: path.clone(),
        id: ctx.ids.id_of(path)?,
        module: resolved,
        seed_offset: 0,
    })
}

/// Feeds input `input` of the added module `mix` from `source`: a cable
/// from an inner module, or a place in the instance input a pass-through
/// stands for. Nothing behind a port feeds nothing.
fn feed(inner: &mut Inner, source: Option<&Source>, mix: ModuleId, input: String) {
    match source {
        Some(Source::Module(from, output)) => inner.cables.push(PatchCable {
            from: (from.0, output.clone()),
            to: (mix.0, input),
        }),
        Some(Source::Input(port)) => inner.inputs.entry(port.clone()).or_default().push((mix, input)),
        None => {}
    }
}

/// The input of an added module for member or copy `k`, from 0: `in1` to
/// `in16`.
fn member_input(k: usize) -> String {
    format!("in{}", k + 1)
}

/// The signal type of the output behind `source`, or `None` for a
/// pass-through or nothing, which the modes treat as audio.
fn signal_of(inner: &Inner, source: Option<&Source>) -> Option<SignalType> {
    let Some(Source::Module(id, port)) = source else {
        return None;
    };
    inner
        .modules
        .iter()
        .find(|m| m.id == *id)
        .and_then(|m| m.module.descriptor.outputs.iter().find(|p| p.name == port))
        .map(|p| p.signal)
}

/// The Select's `fade` for an output whose members give `signals`: off when
/// any is a gate, trigger, or phase, which switch on the frame (live-patching
/// rule 9); linear when any is pitch or modulation, whose values must not
/// swell mid-fade; equal power for audio (sub-patches.md, The modules the
/// flattener adds, Select).
fn select_fade(signals: &[Option<SignalType>]) -> f32 {
    if signals
        .iter()
        .any(|s| matches!(s, Some(SignalType::Gate | SignalType::Phase)))
    {
        0.0
    } else if signals
        .iter()
        .any(|s| matches!(s, Some(SignalType::Pitch | SignalType::Modulation)))
    {
        1.0
    } else {
        2.0
    }
}

/// The gain a stack's Sum takes for an output of `signal` over `copies`:
/// 1/√N for audio, whose different copies add in power; 1/N for pitch and
/// modulation, so the output is the copies' mean; 1 for a gate, which stays
/// high while any copy is high (sub-patches.md, The modules the flattener
/// adds, Sum).
fn stack_gain(signal: Option<SignalType>, copies: u32) -> f32 {
    match signal {
        Some(SignalType::Pitch | SignalType::Modulation) => 1.0 / copies.max(1) as f32,
        Some(SignalType::Gate | SignalType::Phase) => 1.0,
        _ => blend::unison_gain(copies),
    }
}

/// A layer's or a select's ports and mixing (sub-patches.md, Members are
/// nested instances): each outer input feeds the same-named input of every
/// member; each outer output is a Sum of the members' (layer) or a Select
/// between them (select), all Selects reading one Member module.
fn mix_members(
    ctx: &mut Context<'_>,
    mut inner: Inner,
    path: &ModulePath,
    definition: &SubPatch,
    member: Option<f32>,
) -> Result<Inner, PatchError> {
    let children: Vec<ChildPorts> = definition
        .members
        .iter()
        .map(|id| inner.children.get(id).cloned().unwrap_or_default())
        .collect();
    let mut outputs: Vec<String> = Vec::new();
    for child in &children {
        for (name, dests) in &child.inputs {
            inner
                .inputs
                .entry(name.clone())
                .or_default()
                .extend(dests.iter().cloned());
        }
        for name in child.outputs.keys() {
            if !outputs.contains(name) {
                outputs.push(name.clone());
            }
        }
    }
    let count = children.len();
    if definition.mode == Mode::Layer && count == 1 {
        inner.outputs = children[0].outputs.clone();
        return Ok(inner);
    }
    let member_id = if definition.mode == Mode::Select {
        let value = member.unwrap_or(1.0);
        let value = if value.is_finite() {
            value.round().clamp(1.0, count as f32)
        } else {
            1.0
        };
        let module = added_module(ctx, &path.added(MEMBER_STEP), MEMBER_KIND, &[(MEMBER_PARAM, value)])?;
        let id = module.id;
        inner.modules.push(module);
        inner
            .exposed
            .insert(MEMBER_PARAM.to_string(), (id, MEMBER_PARAM.to_string()));
        if count > 1 {
            inner.limits.insert((id, MEMBER_PARAM.to_string()), (1.0, count as f32));
        }
        inner.added.insert(id, Added::Member { instance: path.clone() });
        Some(id)
    } else {
        None
    };
    for (i, name) in outputs.iter().enumerate() {
        let sources: Vec<Option<&Source>> = children
            .iter()
            .map(|child| child.outputs.get(name).and_then(Option::as_ref))
            .collect();
        let step = path.added(i as u32);
        let mix = match member_id {
            None => added_module(ctx, &step, SUM_KIND, &[("gain", 1.0)])?,
            Some(_) => {
                let signals: Vec<Option<SignalType>> = sources.iter().map(|s| signal_of(&inner, *s)).collect();
                let fade = select_fade(&signals);
                added_module(ctx, &step, SELECT_KIND, &[("members", count as f32), ("fade", fade)])?
            }
        };
        let id = mix.id;
        inner.modules.push(mix);
        for (k, source) in sources.iter().enumerate() {
            feed(&mut inner, *source, id, member_input(k));
        }
        if let Some(member_id) = member_id {
            inner.cables.push(PatchCable {
                from: (member_id.0, "out".to_string()),
                to: (id.0, MEMBER_PARAM.to_string()),
            });
        }
        inner
            .outputs
            .insert(name.clone(), Some(Source::Module(id, "out".to_string())));
        inner.added.insert(
            id,
            Added::Output {
                instance: path.clone(),
                port: name.clone(),
            },
        );
    }
    Ok(inner)
}

/// A stack's copies (sub-patches.md, Copies in the editor): the inside
/// flattened once per copy, copy 0 at the instance's own paths and copy `k`
/// under [`ModulePath::copy`], with every output summed at 1/√N (or spread
/// across stereo), each spread as hidden modulation from one constant, and
/// the copies' mappings turned into followers of copy 0's.
fn flatten_stack(
    ctx: &mut Context<'_>,
    inside: LevelRef<'_>,
    path: &ModulePath,
    definition: &SubPatch,
) -> Result<Inner, PatchError> {
    let n = definition.copy_count();
    let mut copies = Vec::with_capacity(n as usize);
    for k in 0..n {
        let copy_path = if k == 0 { path.clone() } else { path.copy(k) };
        let saved = (ctx.in_copy, ctx.seed_offset, ctx.seed_stride);
        if k > 0 {
            ctx.in_copy += 1;
        }
        ctx.seed_offset = saved.1.wrapping_add(k.wrapping_mul(saved.2));
        ctx.seed_stride = saved.2.wrapping_mul(MOST_MEMBERS as u32);
        let result = flatten_level(ctx, inside, &copy_path, Some(definition));
        (ctx.in_copy, ctx.seed_offset, ctx.seed_stride) = saved;
        copies.push(result?);
    }
    let mut rest = copies.split_off(1);
    let mut inner = copies.pop().expect("a stack has at least one copy");
    let base_modules: Vec<ModuleId> = inner.modules.iter().map(|m| m.id).collect();
    let base_midi: Vec<(ModuleId, String)> = inner
        .midi
        .iter()
        .map(|m| (ModuleId(m.target.0), m.target.1.clone()))
        .collect();
    let mut outputs: Vec<BTreeMap<String, Option<Source>>> = vec![inner.outputs.clone()];
    let mut exposed: Vec<BTreeMap<String, (ModuleId, String)>> = vec![inner.exposed.clone()];
    for copy in &mut rest {
        // The same definition flattens to the same modules in the same
        // order, so module `i` of copy `k` is module `i` of copy 0.
        for (i, module) in copy.modules.iter().enumerate() {
            let Some(&base) = base_modules.get(i) else { continue };
            let mut list = vec![(module.id, module.seed_offset)];
            list.extend(copy.copies.remove(&module.id).unwrap_or_default());
            inner.copies.entry(base).or_default().extend(list);
        }
        for (i, mapping) in copy.midi.iter().enumerate() {
            let Some(base) = base_midi.get(i) else { continue };
            let target = (ModuleId(mapping.target.0), mapping.target.1.clone());
            let mut list = vec![target.clone()];
            list.extend(copy.followers.remove(&target).unwrap_or_default());
            inner.followers.entry(base.clone()).or_default().extend(list);
        }
        inner.modules.append(&mut copy.modules);
        inner.cables.append(&mut copy.cables);
        inner.modulation.append(&mut copy.modulation);
        inner.limits.append(&mut copy.limits);
        inner.added.append(&mut copy.added);
        for (name, dests) in std::mem::take(&mut copy.inputs) {
            inner.inputs.entry(name).or_default().extend(dests);
        }
        outputs.push(std::mem::take(&mut copy.outputs));
        exposed.push(std::mem::take(&mut copy.exposed));
    }
    if n > 1 || definition.stereo_spread {
        let names: Vec<String> = outputs[0].keys().cloned().collect();
        for (i, name) in names.iter().enumerate() {
            let step = path.added(i as u32);
            let signal = signal_of(&inner, outputs[0].get(name).and_then(Option::as_ref));
            // Only audio is spread across stereo; a pitch or a gate has no side.
            let audio = !matches!(
                signal,
                Some(SignalType::Pitch | SignalType::Modulation | SignalType::Gate | SignalType::Phase)
            );
            let mix = if definition.stereo_spread && audio {
                added_module(ctx, &step, SPREAD_SUM_KIND, &[("copies", n as f32)])?
            } else {
                added_module(ctx, &step, SUM_KIND, &[("gain", stack_gain(signal, n))])?
            };
            let id = mix.id;
            inner.modules.push(mix);
            for (k, copy) in outputs.iter().enumerate() {
                feed(&mut inner, copy.get(name).and_then(Option::as_ref), id, member_input(k));
            }
            inner
                .outputs
                .insert(name.clone(), Some(Source::Module(id, "out".to_string())));
            inner.added.insert(
                id,
                Added::Output {
                    instance: path.clone(),
                    port: name.clone(),
                },
            );
        }
    }
    let spreads: Vec<(&String, f32)> = definition
        .spread
        .iter()
        .filter(|(_, amount)| **amount > 0.0)
        .map(|(name, amount)| (name, *amount))
        .collect();
    if n > 1 && !spreads.is_empty() {
        let source = added_module(ctx, &path.added(SPREAD_STEP), SPREAD_SOURCE_KIND, &[("value", 1.0)])?;
        let source_id = source.id;
        inner.modules.push(source);
        inner.added.insert(source_id, Added::Spread { instance: path.clone() });
        for (name, amount) in spreads {
            for (k, targets) in exposed.iter().enumerate() {
                let depth = amount * blend::spread_offset(k as u32, n);
                let Some(target) = targets.get(name) else { continue };
                if depth == 0.0 {
                    continue;
                }
                inner.modulation.push(PatchModulation {
                    source: (source_id.0, "out".to_string()),
                    target: (target.0.0, target.1.clone()),
                    depth,
                    polarity: None,
                    enabled: true,
                });
            }
        }
    }
    Ok(inner)
}

/// The slot member for a slot module: its role, resolved, and its part.
fn slot_member(ctx: &Context<'_>, module: &PatchModule) -> Result<SlotMember, PatchError> {
    let bad = |reason: String| PatchError::BadSlot {
        module: module.id,
        reason,
    };
    if module.kind != SLOT_KIND {
        return Err(bad(format!(
            "`{}` has a `slot` field, which only {SLOT_KIND} has",
            module.kind
        )));
    }
    let Some(slot) = &module.slot else {
        return Err(bad("it has no `slot` field".to_string()));
    };
    if !module.params.is_empty() || !module.ranges.is_empty() || module.subpatch.is_some() {
        return Err(bad(
            "a slot has no values of its own; they belong to its part".to_string()
        ));
    }
    let role = ctx.registry.resolve_role(&slot.role).map_err(|e| bad(e.to_string()))?;
    Ok(SlotMember { role, part: slot.part })
}

/// A module part's own name for the role parameter `param`: through its
/// declaration of the role, the same name for a module this build does not
/// know, or `None` when the module lacks the optional parameter.
pub(crate) fn part_own_param(registry: &Registry, kind: &str, role_id: &str, param: &str) -> Option<String> {
    let Ok(descriptor) = registry.resolve(kind) else {
        return Some(param.to_string());
    };
    let fills = descriptor.fills(role_id)?;
    let own = fills.own_param(param);
    descriptor.param_index(&own).map(|_| own)
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
    /// What feeds each instance or slot input in this level.
    feeds: &'a BTreeMap<(u32, String), (u32, String)>,
    /// Each slot's part, by the part's id: a cable or an assignment on a
    /// role name of one is refused, since it goes through the slot.
    owned: &'a BTreeMap<u32, u32>,
}

impl Lookup<'_> {
    /// What stands behind the output `from` of this level, or `None` when
    /// nothing does. `seen` guards a loop of pass-through ports, which
    /// carries nothing.
    fn source_of(&self, from: &(u32, String), seen: &mut Vec<(u32, String)>) -> Result<Option<Source>, PatchError> {
        let (id, port) = from;
        if let Some(slot) = self.owned.get(id) {
            return Err(PatchError::PartCabledDirectly {
                module: *id,
                slot: *slot,
            });
        }
        match self.members.get(id) {
            Some(Member::Slot(slot)) => {
                if slot.role.output(port).is_none() {
                    return Err(no_such_port(*id, "output", port));
                }
                match slot.part.and_then(|part| self.members.get(&part)) {
                    Some(Member::Module(flat, resolved)) => {
                        let own = own_name(resolved, slot.role, port, |f, n| f.own_output(n));
                        Ok(resolved.output_index(&own).map(|_| Source::Module(*flat, own)))
                    }
                    Some(Member::Instance(child)) => match child.outputs.get(port) {
                        Some(Some(Source::Module(flat, output))) => Ok(Some(Source::Module(*flat, output.clone()))),
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
                        _ => Ok(None),
                    },
                    // An empty slot: nothing behind the port.
                    _ => Ok(None),
                }
            }
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
        if let Some(slot) = self.owned.get(id) {
            return Err(PatchError::PartCabledDirectly {
                module: *id,
                slot: *slot,
            });
        }
        match self.members.get(id) {
            Some(Member::Slot(slot)) => {
                if slot.role.input(port).is_none() {
                    return Err(no_such_port(*id, "input", port));
                }
                Ok(match slot.part.and_then(|part| self.members.get(&part)) {
                    Some(Member::Module(flat, resolved)) => {
                        let own = own_name(resolved, slot.role, port, |f, n| f.own_input(n));
                        resolved
                            .input_index(&own)
                            .map(|_| vec![(*flat, own)])
                            .unwrap_or_default()
                    }
                    Some(Member::Instance(child)) => child.inputs.get(port).cloned().unwrap_or_default(),
                    _ => Vec::new(),
                })
            }
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

    /// The flat parameter the parameter `target` of this level stands for,
    /// or `None` for a slot's role parameter with nothing behind it: an
    /// empty slot, or a part without the optional parameter. On a slot's
    /// part itself, only a parameter outside the role is reached directly;
    /// a role parameter goes through the slot.
    fn param_of(&self, target: &(u32, String)) -> Result<Option<(ModuleId, String)>, PatchError> {
        let (id, param) = target;
        let no_such_param = || PatchError::UnknownParam {
            module: *id,
            param: param.clone(),
        };
        let through_slot = |slot: u32| PatchError::PartCabledDirectly { module: *id, slot };
        match self.members.get(id) {
            Some(Member::Slot(slot)) => {
                if slot.role.param(param).is_none() {
                    return Err(no_such_param());
                }
                Ok(slot_param(self.members, slot, param))
            }
            Some(Member::Module(flat, resolved)) => {
                if resolved.parameter_index(param).is_none() {
                    return Err(no_such_param());
                }
                if let Some(slot) = self.owned.get(id)
                    && let Some(Member::Slot(owner)) = self.members.get(slot)
                    && resolved
                        .descriptor
                        .fills(owner.role.id)
                        .is_some_and(|f| f.role_param_of(param).is_some())
                {
                    return Err(through_slot(*slot));
                }
                Ok(Some((*flat, param.clone())))
            }
            Some(Member::Instance(child)) => {
                if let Some(slot) = self.owned.get(id)
                    && let Some(Member::Slot(owner)) = self.members.get(slot)
                    && owner.role.param(param).is_some()
                {
                    return Err(through_slot(*slot));
                }
                child.exposed.get(param).cloned().map(Some).ok_or_else(no_such_param)
            }
            Some(Member::Input(_) | Member::Output(_)) => Err(no_such_param()),
            None => Err(LoomError::UnknownModule(ModuleId(*id)).into()),
        }
    }
}

/// A module part's own name for a role name: through its declaration of
/// the role, or the same name for a module this build does not know.
fn own_name(
    resolved: &ResolvedModule,
    role: &RoleDescriptor,
    name: &str,
    map: impl Fn(&FillsRole, &str) -> String,
) -> String {
    match resolved.descriptor.fills(role.id) {
        Some(fills) => map(fills, name),
        None => name.to_string(),
    }
}

/// The flat parameter behind a slot's role parameter `param`, or `None`
/// for an empty slot or a part without the optional parameter.
fn slot_param(members: &BTreeMap<u32, Member>, slot: &SlotMember, param: &str) -> Option<(ModuleId, String)> {
    match slot.part.and_then(|part| members.get(&part)) {
        Some(Member::Module(flat, resolved)) => {
            let own = own_name(resolved, slot.role, param, |f, n| f.own_param(n));
            resolved.parameter_index(&own).map(|_| (*flat, own))
        }
        Some(Member::Instance(child)) => child.exposed.get(param).cloned(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
