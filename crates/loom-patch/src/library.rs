//! Library items and linked sub-patches (`docs/design/sub-patches.md`,
//! Library and Linked library items; ADR-0031).
//!
//! A library item is an ordinary patch file. Made into a sub-patch
//! definition ([`SubPatch::from_patch`]), its Patch inputs and outputs are
//! the ports, its `exposed` list is the surface, and its patch-wide modules
//! (the Transport, the Scale and tuning module) are left out, each of their
//! cables turned into a port, as if they had been outside when the
//! sub-patch was collapsed (rule 8). The other way, [`Patch::from_definition`]
//! writes a definition as a file, for Save to library.
//!
//! A linked instance keeps the library path beside its own copy of the
//! definition, and takes a fresh copy from the library only when the patch
//! loads ([`Patch::refresh_linked`]). A definition that contains itself
//! through a chain of links is refused, naming the loop.
//!
//! Nothing here opens a file (AGENTS.md, hard rule 6). Linked items are read
//! through the [`Library`] trait: the desktop app reads its library folder,
//! a test passes a map, and the command line passes [`NoLibrary`].

use crate::flatten::port_name;
use crate::{
    Exposed, INPUT_KIND, LoadWarning, OUTPUT_KIND, Patch, PatchCable, PatchError, PatchModule, SubPatch, unique_name,
};
use loom_core::{Registry, TRANSPORT_MODULE};
use std::collections::{BTreeMap, BTreeSet};

/// The Scale and tuning module's id (`docs/design/scale-and-tuning.md`,
/// MT-PITCH-03). A patch-wide module like the Transport: it lives only at
/// the top level (sub-patches.md, rule 8). Named here before the module is
/// built, so a file that holds one is already left out of a sub-patch.
pub const SCALE_MODULE: &str = "loom.pitch.scale";

/// The canvas grid, in world units, as `loom_canvas::GRID`: the ports made
/// from a dropped module's cables are placed on it. Repeated here because
/// the UI crates are desktop only and this crate builds for the web.
const GRID: f32 = 12.0;
/// A tile's width in world units, as `loom_canvas::TILE_WIDTH`.
const TILE_WIDTH: f32 = 132.0;

/// Why a library item could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryError {
    /// There is no file at the path.
    Missing,
    /// The file is there but is not a patch this build loads.
    Unreadable(String),
}

/// Where linked library items are read from (flattening rule 7). A path is
/// relative to the library folder, with `/` between its parts, as the
/// `library` field of a definition writes it.
pub trait Library {
    /// The patch file at `path`.
    fn read(&self, path: &str) -> Result<Patch, LibraryError>;
}

/// A library with nothing in it: every linked instance keeps its stored
/// copy. For the command line and the web runtime, which have no library
/// folder.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLibrary;

impl Library for NoLibrary {
    fn read(&self, _path: &str) -> Result<Patch, LibraryError> {
        Err(LibraryError::Missing)
    }
}

/// A library in memory, by path: for tests.
impl Library for BTreeMap<String, Patch> {
    fn read(&self, path: &str) -> Result<Patch, LibraryError> {
        self.get(path).cloned().ok_or(LibraryError::Missing)
    }
}

/// A patch-wide module left out of a definition made from a file, with the
/// ports made from its cables (sub-patches.md, Library).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    /// The module's kind id without its version: `loom.clock.transport` or
    /// `loom.pitch.scale`.
    pub kind: String,
    /// The names of the ports made from its cables, inputs then outputs.
    pub ports: Vec<String>,
}

impl LeftOut {
    /// The note for the status bar, for the sub-patch called `name`:
    /// `Arp: its Transport was left out; connect its clock ports.`
    pub fn warning(&self, name: &str) -> LoadWarning {
        let (module, carries) = if self.kind == SCALE_MODULE {
            ("Scale and tuning", "pitch")
        } else {
            ("Transport", "clock")
        };
        LoadWarning::PatchWideLeftOut {
            name: name.to_string(),
            module: module.to_string(),
            ports: (!self.ports.is_empty()).then_some(carries),
        }
    }
}

/// True for a module kind that lives only at the top level of a patch: the
/// Transport and the Scale and tuning module (rule 8). The version is not
/// looked at, so a file from another build is still handled.
pub fn is_patch_wide(kind: &str) -> bool {
    let id = kind.split('@').next().unwrap_or(kind);
    id == TRANSPORT_MODULE || id == SCALE_MODULE
}

impl SubPatch {
    /// Makes a definition from a patch file, as inserting it as a sub-patch
    /// does (sub-patches.md, Library). The file's modules, cables,
    /// assignments, mappings, and exposed list become the definition's,
    /// and its metadata is the sub-patch's own. Patch-wide modules are
    /// left out: each cable from one becomes a Patch input, named after the
    /// inner input it fed (several inputs fed from one output share it),
    /// and each cable into one becomes a Patch output; an assignment from
    /// one exposes its target, and an assignment into one gets a Patch
    /// output for its source, as collapsing does (points 1 to 4). Their
    /// MIDI mappings go. Of two cables into one input, which a flat file
    /// may hold, only the later comes along, as the file played it.
    /// `library` is the path the definition follows, or
    /// `None` for a copy. Nested definitions stay in `file.subpatches`,
    /// under their own keys.
    pub fn from_patch(file: &Patch, registry: &Registry, library: Option<&str>) -> (Self, Vec<LeftOut>) {
        let (dropped, kept): (Vec<&PatchModule>, Vec<&PatchModule>) =
            file.modules.iter().partition(|m| is_patch_wide(&m.kind));
        let dropped_ids: BTreeSet<u32> = dropped.iter().map(|m| m.id).collect();
        let touches_dropped = |a: u32, b: u32| dropped_ids.contains(&a) || dropped_ids.contains(&b);
        // A flat file may list two cables into one input and keep the
        // later; a definition refuses that (flattening rule 17), so only
        // the later one comes along.
        let mut last_into: BTreeMap<&(u32, String), usize> = BTreeMap::new();
        for (i, cable) in file.cables.iter().enumerate() {
            last_into.insert(&cable.to, i);
        }
        let mut definition = SubPatch {
            metadata: file.metadata.clone(),
            modules: kept.iter().map(|m| (*m).clone()).collect(),
            cables: file
                .cables
                .iter()
                .enumerate()
                .filter(|(i, c)| last_into.get(&c.to) == Some(i) && !touches_dropped(c.from.0, c.to.0))
                .map(|(_, c)| c.clone())
                .collect(),
            modulation: file
                .modulation
                .iter()
                .filter(|a| !touches_dropped(a.source.0, a.target.0))
                .cloned()
                .collect(),
            midi: file
                .midi
                .iter()
                .filter(|m| !dropped_ids.contains(&m.target.0))
                .cloned()
                .collect(),
            exposed: file.exposed.clone(),
            library: library.map(str::to_string),
        };
        if dropped.is_empty() {
            return (definition, Vec::new());
        }

        let mut ports = Ports::new(&kept, registry);
        let mut left_out = Vec::new();
        for module in dropped {
            let mut made = Vec::new();
            // Cables out of it feed inner inputs: one Patch input per output.
            let mut inputs: BTreeMap<String, u32> = BTreeMap::new();
            for (i, cable) in file.cables.iter().enumerate().filter(|(_, c)| c.from.0 == module.id) {
                if dropped_ids.contains(&cable.to.0) || last_into.get(&cable.to) != Some(&i) {
                    continue;
                }
                let port = *inputs.entry(cable.from.1.clone()).or_insert_with(|| {
                    let (id, name) = ports.add(&mut definition, Direction::Input, &cable.to);
                    made.push(name);
                    id
                });
                definition.cables.push(PatchCable {
                    from: (port, "out".to_string()),
                    to: cable.to.clone(),
                });
            }
            // Cables into it come from inner outputs: one Patch output per
            // inner output, shared with an assignment from that output.
            let mut outputs: BTreeMap<(u32, String), u32> = BTreeMap::new();
            let mut output_for = |source: &(u32, String), definition: &mut SubPatch, made: &mut Vec<String>| {
                *outputs.entry(source.clone()).or_insert_with(|| {
                    let (id, name) = ports.add(definition, Direction::Output, source);
                    made.push(name);
                    id
                })
            };
            for cable in file.cables.iter().filter(|c| c.to.0 == module.id) {
                if dropped_ids.contains(&cable.from.0) {
                    continue;
                }
                let port = output_for(&cable.from, &mut definition, &mut made);
                definition.cables.push(PatchCable {
                    from: cable.from.clone(),
                    to: (port, "in".to_string()),
                });
            }
            for assignment in &file.modulation {
                if assignment.source.0 == module.id && !dropped_ids.contains(&assignment.target.0) {
                    // Collapsing point 4: the target is exposed, and the
                    // assignment waits for a source on the tile's level.
                    expose(&mut definition, registry, &assignment.target);
                } else if assignment.target.0 == module.id && !dropped_ids.contains(&assignment.source.0) {
                    output_for(&assignment.source, &mut definition, &mut made);
                }
            }
            left_out.push(LeftOut {
                kind: module.kind.split('@').next().unwrap_or(&module.kind).to_string(),
                ports: made,
            });
        }
        (definition, left_out)
    }
}

/// Which way a port made from a dropped module's cable faces.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Input,
    Output,
}

/// The ports of a definition being made from a file: the names taken on
/// each side, the next module id, and where new port tiles go.
struct Ports<'a> {
    registry: &'a Registry,
    input_names: BTreeSet<String>,
    output_names: BTreeSet<String>,
    next_id: u32,
    /// The bounds of the file's placed modules, if any has a position.
    bounds: Option<(f32, f32, f32, f32)>,
    /// How many ports each side has so far, for the column.
    inputs_made: usize,
    outputs_made: usize,
}

impl<'a> Ports<'a> {
    fn new(kept: &[&PatchModule], registry: &'a Registry) -> Self {
        let names = |kind: &str, fallback: &str| -> BTreeSet<String> {
            kept.iter()
                .filter(|m| m.kind == kind)
                .map(|m| port_name(m, fallback))
                .collect()
        };
        let inputs_made = kept.iter().filter(|m| m.kind == INPUT_KIND).count();
        let outputs_made = kept.iter().filter(|m| m.kind == OUTPUT_KIND).count();
        let bounds =
            kept.iter()
                .filter_map(|m| m.position)
                .fold(None, |bounds: Option<(f32, f32, f32, f32)>, (x, y)| {
                    Some(match bounds {
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                        None => (x, y, x, y),
                    })
                });
        Self {
            registry,
            input_names: names(INPUT_KIND, "in"),
            output_names: names(OUTPUT_KIND, "out"),
            next_id: kept.iter().map(|m| m.id).max().map_or(1, |max| max + 1),
            bounds,
            inputs_made,
            outputs_made,
        }
    }

    /// Adds a port named after the inner end `end` it stands for, made
    /// unique on its side. Returns its module id and name.
    fn add(&mut self, definition: &mut SubPatch, direction: Direction, end: &(u32, String)) -> (u32, String) {
        let label = inner_label(&definition.modules, self.registry, direction, end);
        let (taken, made, x) = match direction {
            Direction::Input => (
                &mut self.input_names,
                &mut self.inputs_made,
                self.bounds.map(|(x0, ..)| x0 - TILE_WIDTH - 4.0 * GRID),
            ),
            Direction::Output => (
                &mut self.output_names,
                &mut self.outputs_made,
                self.bounds.map(|(_, _, x1, _)| x1 + TILE_WIDTH + 4.0 * GRID),
            ),
        };
        let name = unique_name(&label, |name| taken.contains(name));
        taken.insert(name.clone());
        let position = x.map(|x| {
            let y = self.bounds.map_or(0.0, |(_, y0, ..)| y0) + *made as f32 * 6.0 * GRID;
            ((x / GRID).round() * GRID, (y / GRID).round() * GRID)
        });
        *made += 1;
        let id = self.next_id;
        self.next_id += 1;
        definition.modules.push(PatchModule {
            id,
            kind: match direction {
                Direction::Input => INPUT_KIND,
                Direction::Output => OUTPUT_KIND,
            }
            .to_string(),
            subpatch: None,
            label: Some(name.clone()),
            params: BTreeMap::new(),
            position,
            size: None,
            ranges: BTreeMap::new(),
        });
        (id, name)
    }
}

/// The label a new port takes from the inner end it stands for (collapsing
/// point 3): a module's port label from the registry, a nested instance's
/// port name, or the port's own name when neither is known.
fn inner_label(modules: &[PatchModule], registry: &Registry, direction: Direction, end: &(u32, String)) -> String {
    let Some(module) = modules.iter().find(|m| m.id == end.0) else {
        return end.1.clone();
    };
    if module.subpatch.is_some() {
        return end.1.clone();
    }
    let Ok(descriptor) = registry.resolve(&module.kind) else {
        return end.1.clone();
    };
    let label = match direction {
        Direction::Input => descriptor
            .input_index(&end.1)
            .and_then(|i| descriptor.inputs.get(i))
            .map(|spec| spec.label),
        Direction::Output => descriptor
            .output_index(&end.1)
            .and_then(|i| descriptor.outputs.get(i))
            .map(|spec| spec.label),
    };
    label.map_or_else(|| end.1.clone(), str::to_string)
}

/// Exposes the parameter `target` of a module in `definition`, under its
/// label made unique, unless it is exposed already.
fn expose(definition: &mut SubPatch, registry: &Registry, target: &(u32, String)) {
    if definition
        .exposed
        .iter()
        .any(|e| e.module == target.0 && e.param == target.1)
    {
        return;
    }
    let label = definition
        .modules
        .iter()
        .find(|m| m.id == target.0)
        .filter(|m| m.subpatch.is_none())
        .and_then(|m| registry.resolve(&m.kind).ok())
        .and_then(|d| d.param_index(&target.1).map(|i| d.params[i].label.to_string()))
        .unwrap_or_else(|| target.1.clone());
    let name = unique_name(&label, |name| definition.exposed.iter().any(|e| e.name == name));
    definition.exposed.push(Exposed {
        name,
        module: target.0,
        param: target.1.clone(),
        range: None,
    });
}

impl Patch {
    /// Writes a definition as a patch file, for Save to library: its
    /// modules, cables, assignments, mappings, and exposed list at the top
    /// level, its metadata as the file's, and the nested definitions it
    /// uses from `table`, under their keys. The definition's own `library`
    /// path is not a file's to keep.
    pub fn from_definition(definition: &SubPatch, table: &BTreeMap<String, SubPatch>) -> Self {
        let mut patch = Patch::new("");
        patch.metadata = definition.metadata.clone();
        patch.modules = definition.modules.clone();
        patch.cables = definition.cables.clone();
        patch.modulation = definition.modulation.clone();
        patch.midi = definition.midi.clone();
        patch.exposed = definition.exposed.clone();
        for key in reachable_keys(definition, table) {
            if let Some(nested) = table.get(&key) {
                patch.subpatches.insert(key, nested.clone());
            }
        }
        patch
    }

    /// Takes a fresh copy of every linked definition from `library`
    /// (sub-patches.md, Linked library items). A definition whose file
    /// differs from its stored copy is replaced, with its nested
    /// definitions; its instances keep their exposed values by exposed
    /// name, and a value whose name is gone is dropped. A file's own
    /// linked definitions are refreshed first, so a chain of links is
    /// followed to its end, and a chain that comes back to a file already
    /// on it is refused as a loop naming the sub-patches along it. A file
    /// that is missing or does not load leaves the stored copy, with a
    /// note (rule 5). Returns the notes: what was refreshed, what was left
    /// as it was, and what a refreshed file left out (rule 8).
    pub fn refresh_linked(
        &mut self,
        library: &dyn Library,
        registry: &Registry,
    ) -> Result<Vec<LoadWarning>, PatchError> {
        let mut notes = Vec::new();
        let mut refreshed = Vec::new();
        refresh_table(self, library, registry, &mut Vec::new(), &mut notes, &mut refreshed)?;
        if !refreshed.is_empty() {
            notes.push(LoadWarning::LibraryRefreshed { names: refreshed });
        }
        Ok(notes)
    }

    /// True when a definition in this patch follows a library file.
    pub fn has_links(&self) -> bool {
        self.subpatches.values().any(|d| d.library.is_some())
    }

    /// The modules of every level, top first then each definition in key
    /// order, for an edit that touches every instance.
    fn modules_at_every_level(&mut self) -> impl Iterator<Item = &mut PatchModule> {
        self.modules
            .iter_mut()
            .chain(self.subpatches.values_mut().flat_map(|d| d.modules.iter_mut()))
    }
}

/// Refreshes the linked definitions of `patch`'s table. `stack` holds the
/// library paths being read, outermost first, with the name each is known
/// by, for the loop check. `refreshed` collects the names of the
/// definitions whose copy was replaced.
fn refresh_table(
    patch: &mut Patch,
    library: &dyn Library,
    registry: &Registry,
    stack: &mut Vec<(String, String)>,
    notes: &mut Vec<LoadWarning>,
    refreshed: &mut Vec<String>,
) -> Result<(), PatchError> {
    let mut done: BTreeSet<String> = BTreeSet::new();
    loop {
        // The next linked definition not yet handled, in key order. The
        // table changes under a replacement, so it is looked up each time.
        let next = patch
            .subpatches
            .iter()
            .filter(|(key, definition)| !done.contains(*key) && definition.library.is_some())
            .map(|(key, definition)| {
                (
                    key.clone(),
                    definition.library.clone().unwrap_or_default(),
                    definition.metadata.name.clone(),
                )
            })
            .next();
        let Some((key, path, name)) = next else {
            return Ok(());
        };
        done.insert(key.clone());
        if let Some(start) = stack.iter().position(|(p, _)| *p == path) {
            let mut loop_path: Vec<String> = stack[start..].iter().map(|(_, n)| n.clone()).collect();
            loop_path.push(name);
            return Err(PatchError::SubPatchLoop { path: loop_path });
        }
        let mut file = match library.read(&path) {
            Ok(file) => file,
            Err(LibraryError::Missing) => {
                notes.push(LoadWarning::LibraryMissing { name, path });
                continue;
            }
            Err(LibraryError::Unreadable(reason)) => {
                notes.push(LoadWarning::LibraryUnreadable { name, path, reason });
                continue;
            }
        };
        // The file's own links first, so what comes in is current, and a
        // link back to a file on the way in is a loop.
        stack.push((path.clone(), name.clone()));
        let inner = refresh_table(&mut file, library, registry, stack, notes, &mut Vec::new());
        stack.pop();
        inner?;
        let (mut fresh, left_out) = SubPatch::from_patch(&file, registry, Some(&path));
        let old = &patch.subpatches[&key];
        if fingerprint(old, &patch.subpatches) == fingerprint(&fresh, &file.subpatches) {
            continue;
        }
        // The old copy's nested definitions go with it; the fresh ones
        // come in under keys the table does not use.
        for nested in reachable_keys(old, &patch.subpatches) {
            patch.subpatches.remove(&nested);
        }
        let mut nested = file.subpatches;
        rekey(&mut fresh, &mut nested, |candidate| {
            candidate == key || patch.subpatches.contains_key(candidate)
        });
        for (nested_key, definition) in nested {
            // Refreshed with the file above: not to be read again.
            done.insert(nested_key.clone());
            patch.subpatches.insert(nested_key, definition);
        }
        let names: BTreeSet<String> = fresh.exposed.iter().map(|e| e.name.clone()).collect();
        let fresh_name = fresh.metadata.name.clone();
        patch.subpatches.insert(key.clone(), fresh);
        for module in patch.modules_at_every_level() {
            if module.subpatch.as_deref() == Some(key.as_str()) {
                module.params.retain(|name, _| names.contains(name));
            }
        }
        for dropped in &left_out {
            notes.push(dropped.warning(&fresh_name));
        }
        refreshed.push(fresh_name);
    }
}

/// The keys of the definitions `root` uses, through any depth, in the
/// order they are first met: the instances in id order, depth first.
pub(crate) fn reachable_keys(root: &SubPatch, table: &BTreeMap<String, SubPatch>) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending: Vec<&SubPatch> = vec![root];
    while let Some(definition) = pending.pop() {
        let mut instances: Vec<&PatchModule> = definition.modules.iter().filter(|m| m.subpatch.is_some()).collect();
        instances.sort_by_key(|m| m.id);
        // Depth first: the last pushed is visited next.
        for module in instances.iter().rev() {
            let key = module.subpatch.clone().unwrap_or_default();
            if found.contains(&key) {
                continue;
            }
            if let Some(nested) = table.get(&key) {
                found.push(key);
                pending.push(nested);
            }
        }
    }
    found
}

/// What a definition and everything nested in it hold, as text, with the
/// nested keys replaced by their place in the tree, so two copies of one
/// library file compare equal whatever keys their tables gave them.
pub(crate) fn fingerprint(root: &SubPatch, table: &BTreeMap<String, SubPatch>) -> String {
    let mut file = Patch::from_definition(root, table);
    let keys = reachable_keys(root, table);
    let renamed: BTreeMap<String, String> = keys
        .iter()
        .enumerate()
        .map(|(i, key)| (key.clone(), format!("s{}", i + 1)))
        .collect();
    let table = std::mem::take(&mut file.subpatches);
    for (key, mut definition) in table {
        for module in &mut definition.modules {
            if let Some(key) = &mut module.subpatch
                && let Some(new) = renamed.get(key)
            {
                *key = new.clone();
            }
        }
        file.subpatches
            .insert(renamed.get(&key).cloned().unwrap_or(key), definition);
    }
    for module in &mut file.modules {
        if let Some(key) = &mut module.subpatch
            && let Some(new) = renamed.get(key)
        {
            *key = new.clone();
        }
    }
    file.to_ron().unwrap_or_default()
}

/// Gives every definition in `nested` a key that `taken` does not report
/// and no other entry has, renaming the references in `root` and in the
/// nested definitions to match.
pub(crate) fn rekey(root: &mut SubPatch, nested: &mut BTreeMap<String, SubPatch>, taken: impl Fn(&str) -> bool) {
    let mut renamed: BTreeMap<String, String> = BTreeMap::new();
    for key in nested.keys() {
        if taken(key) {
            let new = unique_name(key, |candidate| {
                taken(candidate) || nested.contains_key(candidate) || renamed.values().any(|v| v == candidate)
            });
            renamed.insert(key.clone(), new);
        }
    }
    if renamed.is_empty() {
        return;
    }
    let fix = |modules: &mut [PatchModule]| {
        for module in modules {
            if let Some(key) = &mut module.subpatch
                && let Some(new) = renamed.get(key)
            {
                *key = new.clone();
            }
        }
    };
    fix(&mut root.modules);
    let table = std::mem::take(nested);
    for (key, mut definition) in table {
        fix(&mut definition.modules);
        nested.insert(renamed.get(&key).cloned().unwrap_or(key), definition);
    }
}

#[cfg(test)]
mod tests;
