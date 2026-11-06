//! The LoomDSP patch file format (ADR-0008).
//!
//! Patches are RON (Rusty Object Notation) text files. Why RON: it keeps
//! Rust's data model (structs, enums, tuples) exactly, allows comments, and
//! diffs well in git.
//!
//! ```ron
//! Patch(
//!     version: 1,
//!     name: "Simple sine",
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
    Controller, Graph, InputRef, LoomError, MidiControl, MidiEncoding, MidiMappingOptions, MidiMode, ModuleId,
    ModuleNames, OutputRef, Polarity, Registry, TRANSPORT_MODULE,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod metadata;
pub use metadata::Metadata;
pub mod resolve;
pub use resolve::{Layout, ResolveError, ResolveOptions, ResolvedModule};
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
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Free text for the patch author.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Who made it, in credit order (`docs/design/patch-metadata.md`).
    /// Omitted when empty, like the other metadata fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// Tags for finding the patch again. Kept as written; filters compare
    /// their tidy form ([`metadata::tidy_tag`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// An SPDX licence expression, kept as written even when it is not
    /// valid (patch-metadata.md, rule 1). Spelled as SPDX spells it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "metadata::serialize_licence",
        deserialize_with = "metadata::deserialize_licence"
    )]
    pub license: Option<String>,
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
}

/// A module in a patch file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchModule {
    /// Stable module ID within the patch.
    pub id: u32,
    /// Module type as `"id@version"`.
    pub kind: String,
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
    /// A parameter name does not exist on a module.
    #[error("module {module}: unknown parameter `{param}`")]
    UnknownParam {
        /// Module ID in the patch.
        module: u32,
        /// Parameter name.
        param: String,
    },
}

impl Patch {
    /// An empty patch.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: CURRENT_VERSION,
            name: name.into(),
            description: String::new(),
            authors: Vec::new(),
            tags: Vec::new(),
            license: None,
            modules: Vec::new(),
            cables: Vec::new(),
            modulation: Vec::new(),
            midi: Vec::new(),
        }
    }

    /// The patch's metadata: name, description, authors, tags, licence.
    pub fn metadata(&self) -> Metadata {
        Metadata {
            name: self.name.clone(),
            description: self.description.clone(),
            authors: self.authors.clone(),
            tags: self.tags.clone(),
            license: self.license.clone(),
        }
    }

    /// Replaces the patch's metadata, name included.
    pub fn set_metadata(&mut self, metadata: Metadata) {
        let Metadata {
            name,
            description,
            authors,
            tags,
            license,
        } = metadata;
        self.name = name;
        self.description = description;
        self.authors = authors;
        self.tags = tags;
        self.license = license;
    }

    /// Parses RON text and migrates it to the current version.
    pub fn from_ron(text: &str) -> Result<Self, PatchError> {
        let patch: Patch = ron::from_str(text)?;
        patch.migrate()
    }

    /// Canonical RON text.
    pub fn to_ron(&self) -> Result<String, PatchError> {
        let mut canonical = self.clone();
        canonical.canonicalise();
        let config = ron::ser::PrettyConfig::new()
            .struct_names(true)
            .depth_limit(2)
            .separate_tuple_members(false)
            .indentor("    ");
        let mut text = ron::ser::to_string_pretty(&canonical, config)?;
        text.push('\n');
        Ok(text)
    }

    /// Sorts modules, cables, and parameters into canonical order.
    ///
    /// Modulation is not sorted. Loading gives assignments ids in file order,
    /// and the engine adds them in id order, so a reloaded patch sums its
    /// assignments the same way as the patch that was saved.
    pub fn canonicalise(&mut self) {
        self.modules.sort_by_key(|m| m.id);
        self.cables.sort_by(|a, b| (a.to.0, &a.to.1).cmp(&(b.to.0, &b.to.1)));
        self.midi
            .sort_by(|a, b| (a.target.0, &a.target.1).cmp(&(b.target.0, &b.target.1)));
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
        if !module.kind.contains('@') {
            return Err(LoomError::InvalidKind(module.kind.clone()).into());
        }
        let kind = loom_core::ModuleKind::parse(&module.kind)?;
        if registry
            .get(&kind.id)
            .is_some_and(|descriptor| descriptor.version == kind.version)
        {
            return Ok(None);
        }
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        let mut parameters: Vec<String> = module.params.keys().cloned().collect();
        let push = |items: &mut Vec<String>, name: &str| {
            if !items.iter().any(|existing| existing == name) {
                items.push(name.to_string());
            }
        };
        for cable in &self.cables {
            if cable.to.0 == module.id {
                push(&mut inputs, &cable.to.1);
            }
            if cable.from.0 == module.id {
                push(&mut outputs, &cable.from.1);
            }
        }
        for assignment in &self.modulation {
            if assignment.source.0 == module.id {
                push(&mut outputs, &assignment.source.1);
            }
            if assignment.target.0 == module.id {
                push(&mut parameters, &assignment.target.1);
            }
        }
        for mapping in &self.midi {
            if mapping.target.0 == module.id {
                push(&mut parameters, &mapping.target.1);
            }
        }
        Ok(Some(loom_core::UnknownModule {
            kind: module.kind.clone(),
            params: module.params.clone(),
            ranges: module.ranges.clone(),
            inputs,
            outputs,
            parameters,
        }))
    }

    /// Adds every module and cable to a controller and commits.
    ///
    /// The controller should be empty. Module IDs from the file are kept.
    /// Returns a [`LoadWarning`] for each thing that loaded differently from
    /// the file: an unknown module, an added Transport, or a saved
    /// control range that the parameter does not offer, or that does not
    /// hold the saved value. Values never change (control-ranges.md, rule 7).
    ///
    /// Every patch has exactly one Transport (`docs/design/clock.md`). Two
    /// fail with an error that names both. A patch without one gets one,
    /// with a note, when the registry has the Transport module.
    pub fn load_into(&self, controller: &mut Controller) -> Result<Vec<LoadWarning>, PatchError> {
        let mut notes = Vec::new();
        let mut unknown_kinds = Vec::new();
        let transports: Vec<u32> = self
            .modules
            .iter()
            .filter(|m| {
                loom_core::ModuleKind::parse(&m.kind).ok().is_some_and(|kind| {
                    controller.registry().get(&kind.id).is_some_and(|descriptor| {
                        descriptor.id == TRANSPORT_MODULE && descriptor.version == kind.version
                    })
                })
            })
            .map(|m| m.id)
            .collect();
        if let [first, second, ..] = transports[..] {
            return Err(PatchError::TwoTransports { first, second });
        }
        let (modules, warnings) = self.resolve(controller.registry(), ResolveOptions::FILE)?;
        notes.extend(warnings);
        for module in &modules {
            module.add_to(controller, ModuleId(module.id))?;
            if let Some(unknown) = &module.unknown {
                unknown_kinds.push(unknown.kind.clone());
            }
        }
        if !unknown_kinds.is_empty() {
            notes.push(LoadWarning::UnknownModules { kinds: unknown_kinds });
        }
        if transports.is_empty() && controller.registry().resolve(TRANSPORT_MODULE).is_ok() {
            let id = controller.add_module(TRANSPORT_MODULE)?;
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
        self.check_midi()?;
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

    /// The saved values that [`Patch::load_into`] ignores because their
    /// parameter is transient, as `(module id, parameter name)`. A module
    /// kind the registry does not know is skipped; loading reports it.
    pub fn transient_values(&self, registry: &Registry) -> Vec<(u32, String)> {
        self.modules
            .iter()
            .filter_map(|module| {
                let kind = loom_core::ModuleKind::parse(&module.kind).ok()?;
                let descriptor = registry.get(&kind.id)?;
                (kind.version == descriptor.version).then_some((module, descriptor))
            })
            .flat_map(|(module, descriptor)| {
                module
                    .params
                    .keys()
                    .filter(|name| {
                        descriptor
                            .param_index(name)
                            .is_some_and(|i| descriptor.params[i].is_transient())
                    })
                    .map(|name| (module.id, name.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Refuses two MIDI mappings on one parameter or one control. The
    /// controller would quietly keep the last one, which hides the mistake.
    fn check_midi(&self) -> Result<(), PatchError> {
        let mut targets = std::collections::BTreeSet::new();
        let mut controls = std::collections::BTreeSet::new();
        for mapping in &self.midi {
            if !targets.insert(&mapping.target) {
                return Err(PatchError::DuplicateMidiMapping {
                    what: format!("parameter `{}` of module {}", mapping.target.1, mapping.target.0),
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
            patch.midi.push(PatchMidiMapping {
                target: (mapping.target.0, param_name(graph, mapping.target, mapping.param)),
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
            });
        }
        patch.canonicalise();
        patch
    }
}

fn port_name(graph: &Graph, output: OutputRef) -> String {
    name_of(graph, output.module, |n| n.output_name(output.port))
}

fn input_name(graph: &Graph, input: InputRef) -> String {
    name_of(graph, input.module, |n| n.input_name(input.port))
}

fn param_name(graph: &Graph, module: ModuleId, index: usize) -> String {
    name_of(graph, module, |n| n.parameter_name(index))
}

/// A saved name from one of the module's [`ModuleNames`] lookups, or an
/// empty string when the module or the index is gone.
fn name_of(graph: &Graph, module: ModuleId, name: impl Fn(&loom_core::Node) -> Option<&str>) -> String {
    graph.node(module).and_then(name).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests;
