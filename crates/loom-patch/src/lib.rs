//! The LoomDSP patch file format (ADR-0008).
//!
//! Patches are RON (Rusty Object Notation) text files. Why RON: it keeps
//! Rust's data model (structs, enums, tuples) exactly, allows comments, and
//! diffs well in git.
//!
//! ```ron
//! Patch(
//!     version: 1,
//!     metadata: Metadata(name: "Simple sine"),
//!     modules: [
//!         (id: 1, kind: "loom.midi.note@1"),
//!         (id: 2, kind: "loom.phase.generator@1"),
//!         (id: 3, kind: "loom.osc.sine@1"),
//!         (id: 4, kind: "loom.io.audio-out@1"),
//!     ],
//!     cables: [
//!         (from: (1, "pitch"), to: (2, "pitch")),
//!         (from: (2, "phase"), to: (3, "phase")),
//!         (from: (3, "out"), to: (4, "in")),
//!     ],
//! )
//! ```
//!
//! Saved files are canonical: modules sorted by ID, cables sorted by input,
//! parameters sorted by name, MIDI mappings sorted by target. This keeps git diffs small. Modulation keeps
//! the order the assignments were made in, because the engine adds several
//! assignments on one parameter in that order, and float sums depend on it.

#![warn(missing_docs)]

use loom_core::{
    Graph, InputRef, LoomError, MidiEncoding, MidiMappingOptions, MidiMode, ModuleId, ModuleNames, OutputRef, Polarity,
    Registry,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod level;
pub mod metadata;
pub use metadata::Metadata;
pub mod resolve;
pub use resolve::{Layout, ResolveError, ResolveOptions, ResolvedModule};
pub mod subpatch;
pub use subpatch::{Exposed, INPUT_KIND, OUTPUT_KIND, SUBPATCH_KIND, SubPatch};
pub mod modes;
pub use modes::{MOST_MEMBERS, Mode};
pub mod slot;
pub use slot::{Held, PartKey, SLOT_KIND, Slot};
pub mod flatten;
pub use flatten::{Flat, FlatIds, FlatModule, ModulePath};
pub mod hierarchy;
pub use hierarchy::{
    Collapsed, Definition, Direction, Dissolved, End, Hierarchy, Inserted, Instance, LevelCable, Link, Port, Removed,
    Stored, Tile,
};
pub mod library;
pub use library::{LeftOut, Library, LibraryError, NoLibrary, SCALE_MODULE, reachable_keys};
pub mod warning;
pub use warning::{LoadLevel, LoadWarning, RangeReason};

/// The patch format version this build writes.
pub const CURRENT_VERSION: u32 = 1;

/// A patch file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Patch")]
pub struct Patch {
    /// Format version. See [`CURRENT_VERSION`].
    pub version: u32,
    /// Name, description, authors, tags, and licence
    /// (`docs/design/patch-metadata.md`). Always written, so a file names
    /// its patch.
    #[serde(default)]
    pub metadata: Metadata,
    /// Sub-patch definitions by key (`docs/design/sub-patches.md`, Patch
    /// file). A key is made from the definition's name and is not shown.
    /// Omitted when empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subpatches: BTreeMap<String, SubPatch>,
    /// Modules.
    #[serde(default)]
    pub modules: Vec<PatchModule>,
    /// Cables.
    #[serde(default)]
    pub cables: Vec<PatchCable>,
    /// Modulation assignments. Omitted when empty, so older files still load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modulation: Vec<PatchModulation>,
    /// MIDI mappings (MIDI learn). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub midi: Vec<PatchMidiMapping>,
    /// The parameters this patch exposes when it is inserted as a sub-patch
    /// (`docs/design/sub-patches.md`, Library): a library item is an
    /// ordinary patch file, and its surface travels here. Opened on its own,
    /// the list is kept and saved back, and nothing shows it. Omitted when
    /// empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exposed: Vec<Exposed>,
    /// The roles this patch fills when it is inserted as a sub-patch
    /// (FR-ENCAP-09), as `SubPatch::roles`. A library item carries them
    /// here. Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

/// A module in a patch file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchModule {
    /// Stable module ID within the patch.
    pub id: u32,
    /// Module type as `"id@version"`.
    pub kind: String,
    /// For a sub-patch instance ([`SUBPATCH_KIND`]), the key of its
    /// definition in [`Patch::subpatches`]. Then `params` holds the values
    /// of the definition's exposed parameters, by exposed name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpatch: Option<String>,
    /// For a slot ([`SLOT_KIND`]), its role, current part, and held parts
    /// (`docs/design/sub-patches.md`, Slots). Then `params` and `ranges`
    /// are empty: the part holds the values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<Slot>,
    /// Optional user label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Parameter values by name. Missing parameters use their defaults.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, f32>,
    /// Position on the canvas, for the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<(f32, f32)>,
    /// Optional tile width and height in grid units (panels spec, #61).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_tile_size",
        deserialize_with = "deserialize_tile_size"
    )]
    pub size: Option<(u8, u8)>,
    /// Selected control ranges by parameter name, as `(min, max)`
    /// (`docs/design/control-ranges.md`). Only ranges other than the
    /// default are saved. A missing one gets the default range if the
    /// value fits, else the smallest range that holds it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ranges: BTreeMap<String, (f32, f32)>,
}

/// Writes a present tile size as a plain tuple in the patch format.
fn serialize_tile_size<S: serde::Serializer>(size: &Option<(u8, u8)>, serializer: S) -> Result<S::Ok, S::Error> {
    size.expect("absent sizes are skipped").serialize(serializer)
}

/// Reads a plain tile-size tuple, leaving a missing field to its default.
fn deserialize_tile_size<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<(u8, u8)>, D::Error> {
    <(u8, u8)>::deserialize(deserializer).map(Some)
}

/// A cable in a patch file: `(module id, port name)` to `(module id, port name)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchCable {
    /// Output side.
    pub from: (u32, String),
    /// Input side.
    pub to: (u32, String),
}

/// A modulation assignment in a patch file.
///
/// `source` is `(module id, output name)`. `target` is `(module id, parameter name)`.
/// `polarity` is omitted when the assignment follows the source output.
/// `enabled: false` is written only when the assignment is off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchModulation {
    /// Output that drives the parameter.
    pub source: (u32, String),
    /// Parameter that is moved.
    pub target: (u32, String),
    /// Share of the control's range, from -1 to 1.
    pub depth: f32,
    /// Forces a polarity. Absent means follow the source output.
    /// Written as `Unipolar` or `Bipolar`, not as `Some(...)`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_polarity",
        deserialize_with = "deserialize_polarity"
    )]
    pub polarity: Option<PatchPolarity>,
    /// False skips the assignment. Missing means on.
    #[serde(default = "enabled_by_default", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
}

/// A MIDI mapping in a patch file: a hardware control that sets a parameter.
///
/// `target` is `(module id, parameter name)`. `channel` is 0 to 15, as in
/// MIDI. Spec: `docs/design/midi-learn.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchMidiMapping {
    /// Parameter that is set.
    pub target: (u32, String),
    /// MIDI channel, 0 to 15.
    pub channel: u8,
    /// Controller number, 0 to 127.
    pub controller: u8,
    /// What a control change does. Omitted for Absolute.
    #[serde(default, skip_serializing_if = "is_default_mode")]
    pub mode: PatchMidiMode,
    /// The span of the parameter's position the control covers, as
    /// `(low, high)`. Omitted for the whole travel.
    #[serde(default = "whole_travel", skip_serializing_if = "is_whole_travel")]
    pub range: (f32, f32),
    /// Swaps the ends of the span. Omitted when off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
}

/// A MIDI mapping's mode in a patch file. Matches [`MidiMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatchMidiMode {
    /// Sets the position.
    #[default]
    Absolute,
    /// Waits for the control to reach the parameter, then follows it.
    Pickup,
    /// Reads the value as a step up or down.
    Relative(PatchMidiEncoding),
}

/// A relative encoding in a patch file. Matches [`MidiEncoding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatchMidiEncoding {
    /// 64 is no step, and steps count from it.
    Offset64,
    /// Down steps count back from 128.
    TwosComplement,
    /// The top bit says down.
    SignBit,
}

/// True for the mode a mapping has when the file does not name one.
fn is_default_mode(mode: &PatchMidiMode) -> bool {
    *mode == PatchMidiMode::Absolute
}

/// The range a mapping has when the file does not name one.
fn whole_travel() -> (f32, f32) {
    (0.0, 1.0)
}

/// True when a flag has its default, off.
fn is_false(flag: &bool) -> bool {
    !*flag
}

/// True for [`whole_travel`].
fn is_whole_travel(range: &(f32, f32)) -> bool {
    *range == whole_travel()
}

impl PatchMidiMapping {
    /// The options this entry names, as the controller takes them.
    fn options(&self) -> MidiMappingOptions {
        MidiMappingOptions {
            mode: match self.mode {
                PatchMidiMode::Absolute => MidiMode::Absolute,
                PatchMidiMode::Pickup => MidiMode::Pickup,
                PatchMidiMode::Relative(encoding) => MidiMode::Relative(match encoding {
                    PatchMidiEncoding::Offset64 => MidiEncoding::Offset64,
                    PatchMidiEncoding::TwosComplement => MidiEncoding::TwosComplement,
                    PatchMidiEncoding::SignBit => MidiEncoding::SignBit,
                }),
            },
            low: self.range.0,
            high: self.range.1,
            invert: self.invert,
        }
    }
}

/// Polarity stored in a patch file. Matches [`Polarity`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatchPolarity {
    /// Nominally 0 to 1.
    Unipolar,
    /// Nominally -1 to 1.
    Bipolar,
}

fn enabled_by_default() -> bool {
    true
}

fn is_enabled(enabled: &bool) -> bool {
    *enabled
}

fn serialize_polarity<S>(value: &Option<PatchPolarity>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(polarity) => polarity.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

fn deserialize_polarity<'de, D>(deserializer: D) -> Result<Option<PatchPolarity>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    PatchPolarity::deserialize(deserializer).map(Some)
}

impl From<Polarity> for PatchPolarity {
    fn from(polarity: Polarity) -> Self {
        match polarity {
            Polarity::Unipolar => Self::Unipolar,
            Polarity::Bipolar => Self::Bipolar,
        }
    }
}

impl From<PatchPolarity> for Polarity {
    fn from(polarity: PatchPolarity) -> Self {
        match polarity {
            PatchPolarity::Unipolar => Self::Unipolar,
            PatchPolarity::Bipolar => Self::Bipolar,
        }
    }
}

/// Patch loading and saving errors.
#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    /// A hand-edited file mapped an audio-thread control onto a structural value.
    #[error(
        "MIDI mapping CC {controller} on channel {channel} targets graph-shaping parameter `{param}` of module {module}"
    )]
    StructuralMidiMapping {
        /// Mapped module.
        module: u32,
        /// Mapped parameter or exposed name.
        param: String,
        /// Wire channel, from 0 to 15.
        channel: u8,
        /// Controller number.
        controller: u8,
    },
    /// A hand-edited file modulated a graph-shaping parameter.
    #[error("modulation targets graph-shaping parameter `{param}` of module {module}")]
    StructuralModulationTarget {
        /// Module named by the patch.
        module: u32,
        /// Parameter or exposed name named by the patch.
        param: String,
    },
    /// The text is not a valid patch.
    #[error("invalid patch: {0}")]
    Parse(#[from] ron::error::SpannedError),
    /// Serialisation failed.
    #[error("could not write patch: {0}")]
    Write(#[from] ron::Error),
    /// The file was written by a newer LoomDSP.
    #[error("patch format version {found} is newer than this build supports ({supported})")]
    TooNew {
        /// Version in the file.
        found: u32,
        /// Highest supported version.
        supported: u32,
    },
    /// The patch refers to something the engine rejects.
    #[error(transparent)]
    Engine(#[from] LoomError),
    /// Two MIDI mappings share a parameter or a control. Learning never makes
    /// that, so the file was edited by hand.
    #[error("two MIDI mappings use {what}")]
    DuplicateMidiMapping {
        /// The shared parameter or control.
        what: String,
    },
    /// A MIDI mapping's options cannot be used, so the file was edited by
    /// hand: edits in the app always give options that load.
    #[error("the MIDI mapping on parameter `{param}` of module {module} is invalid: {reason}")]
    InvalidMidiMapping {
        /// The mapped module's ID.
        module: u32,
        /// The mapped parameter.
        param: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The patch has more than one Transport module. A patch has exactly
    /// one (`docs/design/clock.md`).
    #[error("modules {first} and {second} are both a Transport, and a patch has only one")]
    TwoTransports {
        /// The first Transport's module ID.
        first: u32,
        /// The second Transport's module ID.
        second: u32,
    },
    /// A parameter name does not exist on a module. For a sub-patch
    /// instance, a name its definition does not expose, or any control
    /// range.
    #[error("module {module}: unknown parameter `{param}`")]
    UnknownParam {
        /// Module ID in the patch.
        module: u32,
        /// Parameter name.
        param: String,
    },
    /// An instance names a definition the `subpatches` table does not have.
    #[error("module {module}: there is no sub-patch `{key}`")]
    UnknownSubPatch {
        /// The instance's module ID.
        module: u32,
        /// The key it names.
        key: String,
    },
    /// A `subpatch` on a module that is not an instance, or an instance
    /// with no `subpatch`.
    #[error("module {module}: `{kind}` and its `subpatch` field disagree; only `loom.struct.subpatch@1` has one")]
    NotAnInstance {
        /// The module's ID.
        module: u32,
        /// Its kind.
        kind: String,
    },
    /// Two Patch inputs or two Patch outputs of one definition share a name.
    #[error("two {direction} ports are called `{port}`; give one another label")]
    DuplicatePort {
        /// "input" or "output".
        direction: &'static str,
        /// The shared name.
        port: String,
    },
    /// An `exposed` entry that cannot be used.
    #[error("exposed parameter `{name}`: {reason}")]
    BadExposed {
        /// The exposed name.
        name: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A Transport inside a sub-patch definition. A patch has one, at the
    /// top level (`docs/design/sub-patches.md`, rule 8).
    #[error("module {module} is a Transport, and a sub-patch cannot hold one")]
    TransportInSubPatch {
        /// The module's ID in the definition.
        module: u32,
    },
    /// A modulation assignment whose source is a Patch input's `out`.
    /// Collapsing never makes one; expose the parameter instead.
    #[error("module {module} is a Patch input, and an assignment cannot start at a port")]
    AssignmentFromPort {
        /// The Patch input's ID in the definition.
        module: u32,
    },
    /// Two instances name one definition. Every instance has its own
    /// (`docs/design/sub-patches.md`, rule 3: no action at a distance).
    #[error("sub-patch `{key}` is used by modules {first} and {second}, and each instance has a definition of its own")]
    SharedSubPatch {
        /// The shared key.
        key: String,
        /// The first instance's path.
        first: String,
        /// The second instance's path.
        second: String,
    },
    /// Two cables into one input inside a definition. A flat patch keeps
    /// the later one; a sub-patch must not depend on that.
    #[error("two cables go into input `{port}` of module {module}")]
    TwoCablesIntoOneInput {
        /// The module's ID in the definition.
        module: u32,
        /// The input's name.
        port: String,
    },
    /// Sub-patches nested deeper than [`flatten::MAX_DEPTH`] levels. A
    /// bound keeps a hand-made file from exhausting the stack.
    #[error("sub-patches nest more than {max} levels deep")]
    TooDeep {
        /// The most levels allowed.
        max: usize,
    },
    /// A definition that contains itself, directly or through others.
    #[error("sub-patch {} contains itself", .path.join(" › "))]
    SubPatchLoop {
        /// The definitions' names along the loop, first to last.
        path: Vec<String>,
    },
    /// A sub-patch whose mode fields do not fit together, or whose mode
    /// needs a module this build lacks (`docs/design/sub-patches.md`,
    /// Patch file).
    #[error("{reason}")]
    BadMode {
        /// What is wrong with it.
        reason: String,
    },
    /// A slot whose part is missing, is cabled directly, is not in
    /// its level or does not fill the role, or a held sub-patch the table
    /// lacks (`docs/design/sub-patches.md`, Patch file).
    #[error("slot {module}: {reason}")]
    BadSlot {
        /// The slot's module ID.
        module: u32,
        /// What is wrong with it.
        reason: String,
    },
    /// A cable, or an assignment or mapping on a role parameter, straight
    /// onto a slot's part. Everything on the part goes through the slot.
    #[error("module {module} is the part of slot {slot}; connect to the slot instead")]
    PartCabledDirectly {
        /// The part's module ID.
        module: u32,
        /// The slot's module ID.
        slot: u32,
    },
    /// An error inside a sub-patch definition, with the definition's name.
    #[error("in sub-patch \"{name}\": {source}")]
    InSubPatch {
        /// The definition's name.
        name: String,
        /// What went wrong inside it. Module IDs are the definition's own.
        source: Box<PatchError>,
    },
}

impl Patch {
    /// An empty patch.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: CURRENT_VERSION,
            metadata: Metadata {
                name: name.into(),
                ..Metadata::default()
            },
            subpatches: BTreeMap::new(),
            modules: Vec::new(),
            cables: Vec::new(),
            modulation: Vec::new(),
            midi: Vec::new(),
            exposed: Vec::new(),
            roles: Vec::new(),
        }
    }

    /// Parses RON text and migrates it to the current version.
    pub fn from_ron(text: &str) -> Result<Self, PatchError> {
        let patch: Patch = ron::from_str(text)?;
        patch.migrate()
    }

    /// Canonical RON text.
    ///
    /// Every module, cable, and mapping sits on one line, at the top level
    /// and inside each definition, so a diff shows one change per line.
    /// RON has one depth limit for a whole file, so the top level and each
    /// definition are printed on their own and joined.
    pub fn to_ron(&self) -> Result<String, PatchError> {
        let mut canonical = self.clone();
        canonical.canonicalise();
        let definitions = std::mem::take(&mut canonical.subpatches);
        let mut text = pretty(&canonical)?;
        if !definitions.is_empty() {
            let mut table = String::from("    subpatches: {\n");
            for (key, definition) in &definitions {
                table.push_str("        ");
                table.push_str(&ron::to_string(key)?);
                table.push_str(": ");
                for (i, line) in pretty(definition)?.lines().enumerate() {
                    if i > 0 {
                        table.push_str("        ");
                    }
                    table.push_str(line);
                    table.push('\n');
                }
                // The definition ended with ")\n"; it is one entry of the table.
                table.pop();
                table.push_str(",\n");
            }
            table.push_str("    },\n");
            // At a line start: a string value cannot hold a raw newline, so
            // this cannot match inside a name or description.
            let modules = text
                .find("\n    modules: [")
                .expect("a patch always writes its modules");
            text.insert_str(modules + 1, &table);
        }
        Ok(text)
    }

    /// Sorts modules, cables, and parameters into canonical order.
    ///
    /// Modulation is not sorted. Loading gives assignments ids in file order,
    /// and the engine adds them in id order, so a reloaded patch sums its
    /// assignments the same way as the patch that was saved.
    pub fn canonicalise(&mut self) {
        canonicalise_lists(&mut self.modules, &mut self.cables, &mut self.midi);
        for definition in self.subpatches.values_mut() {
            definition.canonicalise();
        }
    }

    /// Upgrades an older patch to [`CURRENT_VERSION`].
    ///
    /// Each format change adds one step here, plus a test that loads a
    /// fixture saved in the old format (FR-FILE-02).
    pub fn migrate(mut self) -> Result<Self, PatchError> {
        if self.version > CURRENT_VERSION {
            return Err(PatchError::TooNew {
                found: self.version,
                supported: CURRENT_VERSION,
            });
        }
        // Version 0 was never released. Treat it as version 1.
        if self.version == 0 {
            self.version = 1;
        }
        Ok(self)
    }

    /// Builds the owned data for a missing module from names in this patch.
    /// A registered kind at the exact version returns `None`.
    pub fn unknown_module(
        &self,
        module: &PatchModule,
        registry: &Registry,
    ) -> Result<Option<loom_core::UnknownModule>, PatchError> {
        self.level().unknown_module(module, registry)
    }

    /// The saved values that [`Patch::load_into`] ignores because their
    /// parameter is transient, as `(module id, parameter name)`. A module
    /// kind the registry does not know is skipped; loading reports it.
    pub fn transient_values(&self, registry: &Registry) -> Vec<(u32, String)> {
        self.level().transient_values(registry)
    }

    /// Builds a patch from a graph. Only non-default parameters are saved.
    /// Modules have no position or size; see [`Patch::from_graph_with`].
    pub fn from_graph(name: impl Into<String>, graph: &Graph) -> Self {
        Self::from_graph_with(name, graph, |_| Layout::default())
    }

    /// Builds a patch from a graph, with each module's layout from
    /// `layout`. The editor keeps positions and sizes outside the graph.
    pub fn from_graph_with(name: impl Into<String>, graph: &Graph, layout: impl Fn(ModuleId) -> Layout) -> Self {
        let mut patch = Patch::new(name);
        for (id, node) in graph.nodes() {
            patch
                .modules
                .push(ResolvedModule::from_node(id, node, layout(id)).write());
        }
        for (from, to) in graph.cables() {
            patch.cables.push(PatchCable {
                from: (from.module.0, port_name(graph, from)),
                to: (to.module.0, input_name(graph, to)),
            });
        }
        for (_, assignment) in graph.assignments() {
            patch.modulation.push(PatchModulation {
                source: (assignment.source.module.0, port_name(graph, assignment.source)),
                target: (
                    assignment.target.0,
                    param_name(graph, assignment.target, assignment.param),
                ),
                depth: assignment.depth,
                polarity: assignment.polarity.map(PatchPolarity::from),
                enabled: assignment.enabled,
            });
        }
        for (_, mapping) in graph.midi_mappings() {
            let target = (mapping.target.0, param_name(graph, mapping.target, mapping.param));
            patch.midi.push(midi_entry(mapping, target));
        }
        patch.canonicalise();
        patch
    }
}

/// One level as RON text, with each module, cable, and mapping on one
/// line and a newline at the end.
fn pretty<T: Serialize>(value: &T) -> Result<String, PatchError> {
    // One line ending everywhere: RON would write CRLF on Windows, and a
    // canonical file must not depend on where it was saved.
    let config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .depth_limit(2)
        .separate_tuple_members(false)
        .new_line("\n")
        .indentor("    ");
    let mut text = ron::ser::to_string_pretty(value, config)?;
    text.push('\n');
    Ok(text)
}

/// Sorts one level's lists into canonical order: modules by ID, cables by
/// input, MIDI mappings by target. Modulation is left in file order.
pub(crate) fn canonicalise_lists(
    modules: &mut [PatchModule],
    cables: &mut [PatchCable],
    midi: &mut [PatchMidiMapping],
) {
    modules.sort_by_key(|m| m.id);
    cables.sort_by(|a, b| (a.to.0, &a.to.1).cmp(&(b.to.0, &b.to.1)));
    midi.sort_by(|a, b| (a.target.0, &a.target.1).cmp(&(b.target.0, &b.target.1)));
}

/// The id of a role or module kind string: `"loom.role.filter@1"` gives
/// `"loom.role.filter"`.
pub fn role_id(kind: &str) -> &str {
    kind.rsplit_once('@').map_or(kind, |(id, _)| id)
}

/// A MIDI mapping as a file writes it, on `target`.
pub(crate) fn midi_entry(mapping: &loom_core::MidiMapping, target: (u32, String)) -> PatchMidiMapping {
    PatchMidiMapping {
        target,
        channel: mapping.control.channel,
        controller: mapping.control.controller,
        mode: match mapping.options.mode {
            MidiMode::Absolute => PatchMidiMode::Absolute,
            MidiMode::Pickup => PatchMidiMode::Pickup,
            MidiMode::Relative(encoding) => PatchMidiMode::Relative(match encoding {
                MidiEncoding::Offset64 => PatchMidiEncoding::Offset64,
                MidiEncoding::TwosComplement => PatchMidiEncoding::TwosComplement,
                MidiEncoding::SignBit => PatchMidiEncoding::SignBit,
            }),
        },
        range: (mapping.options.low, mapping.options.high),
        invert: mapping.options.invert,
    }
}

/// `base`, or `base 2`, `base 3`, and so on, the first that `taken` does
/// not report: how names are made unique everywhere in a patch.
pub fn unique_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken(name))
        .expect("some number is free")
}

pub(crate) fn port_name(graph: &Graph, output: OutputRef) -> String {
    name_of(graph, output.module, |n| n.output_name(output.port))
}

pub(crate) fn input_name(graph: &Graph, input: InputRef) -> String {
    name_of(graph, input.module, |n| n.input_name(input.port))
}

pub(crate) fn param_name(graph: &Graph, module: ModuleId, index: usize) -> String {
    name_of(graph, module, |n| n.parameter_name(index))
}

/// A saved name from one of the module's [`ModuleNames`] lookups, or an
/// empty string when the module or the index is gone.
fn name_of(graph: &Graph, module: ModuleId, name: impl Fn(&loom_core::Node) -> Option<&str>) -> String {
    graph.node(module).and_then(name).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests;
