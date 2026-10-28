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
//!         (from: (3, "out"), to: (4, "left")),
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
    Controller, Graph, InputRef, LoomError, MidiControl, ModuleId, OutputRef, ParamRange, Polarity, Registry,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

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
    /// Selected control ranges by parameter name, as `(min, max)`
    /// (`docs/design/control-ranges.md`). Only ranges other than the
    /// default are saved. A missing one gets the default range if the
    /// value fits, else the smallest range that holds it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ranges: BTreeMap<String, (f32, f32)>,
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
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// File path.
        path: String,
        /// Underlying error.
        source: std::io::Error,
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
            modules: Vec::new(),
            cables: Vec::new(),
            modulation: Vec::new(),
            midi: Vec::new(),
        }
    }

    /// Parses RON text and migrates it to the current version.
    pub fn from_ron(text: &str) -> Result<Self, PatchError> {
        let patch: Patch = ron::from_str(text)?;
        patch.migrate()
    }

    /// Reads a patch file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PatchError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| PatchError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_ron(&text)
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

    /// Writes a patch file.
    ///
    /// The text goes to a new temporary file in the same folder first, then
    /// replaces the target by a rename. A full disk or a failed write cannot
    /// leave a half-written patch where a good one was. It does not promise
    /// the data survives a power loss. The temporary name is
    /// unique to this process and is never an existing file, so another
    /// save or a backup is never overwritten. A read-only target is refused,
    /// and the target keeps its permissions.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), PatchError> {
        use std::io::Write;
        let path = path.as_ref();
        let text = self.to_ron()?;
        // Write through a symlink to its target, as a plain write would. A
        // rename over the link would replace the link itself.
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let target = resolved.as_path();
        let io = |source| PatchError::Io {
            path: path.display().to_string(),
            source,
        };
        let existing = std::fs::metadata(target).ok();
        if existing.as_ref().is_some_and(|m| m.permissions().readonly()) {
            return Err(io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the file is read-only",
            )));
        }
        let name = target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let temp = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .and_then(|()| match &existing {
                Some(meta) => std::fs::set_permissions(&temp, meta.permissions()),
                None => Ok(()),
            })
            .and_then(|()| std::fs::rename(&temp, target));
        written.map_err(|source| {
            let _ = std::fs::remove_file(&temp);
            io(source)
        })
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

    /// Adds every module and cable to a controller and commits.
    ///
    /// The controller should be empty. Module IDs from the file are kept.
    /// Returns notes about what loaded differently from the file: a saved
    /// control range that the parameter does not offer, or that does not
    /// hold the saved value. Values never change (control-ranges.md, rule 7).
    pub fn load_into(&self, controller: &mut Controller) -> Result<Vec<String>, PatchError> {
        let mut notes = Vec::new();
        for module in &self.modules {
            let id = ModuleId(module.id);
            controller.add_module_with_id(id, &module.kind)?;
            notes.extend(Self::load_ranges(module, controller)?);
            let descriptor = controller.graph().node(id).map(|node| node.descriptor);
            for (name, &value) in &module.params {
                // A saved value for a transient parameter is ignored: the
                // module starts released (transient-params.md).
                // `transient_values` lists them for `loom check`.
                if descriptor
                    .and_then(|d| d.param_index(name).map(|i| d.params[i]))
                    .is_some_and(|spec| spec.is_transient())
                {
                    continue;
                }
                controller
                    .set_param_named(id, name, value)
                    .map_err(|error| match error {
                        LoomError::NoSuchParam { .. } => PatchError::UnknownParam {
                            module: module.id,
                            param: name.clone(),
                        },
                        other => PatchError::Engine(other),
                    })?;
            }
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
                .map_midi_named(control, ModuleId(mapping.target.0), &mapping.target.1)
                .map_err(|error| match error {
                    LoomError::NoSuchParam { .. } => PatchError::UnknownParam {
                        module: mapping.target.0,
                        param: mapping.target.1.clone(),
                    },
                    other => PatchError::Engine(other),
                })?;
        }
        controller.commit()?;
        Ok(notes)
    }

    /// Selects each parameter's control range before its value is set, so
    /// the range never clamps a saved value (rule 7). Returns a note for
    /// each saved range that was not kept.
    fn load_ranges(module: &PatchModule, controller: &mut Controller) -> Result<Vec<String>, PatchError> {
        let id = ModuleId(module.id);
        let descriptor = controller
            .graph()
            .node(id)
            .map(|n| n.descriptor)
            .ok_or(LoomError::UnknownModule(id))?;
        let unknown = |name: &str| PatchError::UnknownParam {
            module: module.id,
            param: name.to_string(),
        };
        let mut notes = Vec::new();
        for name in module.ranges.keys() {
            let index = descriptor.param_index(name).ok_or_else(|| unknown(name))?;
            if descriptor.params[index].options.ranges.is_empty() {
                notes.push(format!(
                    "module {}: parameter `{name}` offers no ranges, so its saved range is ignored",
                    module.id
                ));
            }
        }
        for (index, spec) in descriptor.params.iter().enumerate() {
            let value = spec.clamp(module.params.get(spec.name).copied().unwrap_or(spec.default));
            let saved = module
                .ranges
                .get(spec.name)
                .map(|&(min, max)| ParamRange::new(min, max));
            let Some((range, reported)) = spec.load_range(saved, value) else {
                continue;
            };
            if reported && let Some(saved) = saved {
                notes.push(format!(
                    "module {}: parameter `{}` was saved with range {saved}, which {}; it loads with {range}",
                    module.id,
                    spec.name,
                    if spec.options.ranges.contains(&saved) {
                        format!("does not hold its value {value}")
                    } else {
                        "it does not offer".to_string()
                    }
                ));
            }
            if controller.graph().node(id).and_then(|n| n.range(index)) != Some(range) {
                controller.set_range(id, index, range)?;
            }
        }
        Ok(notes)
    }

    /// The saved values that [`Patch::load_into`] ignores because their
    /// parameter is transient, as `(module id, parameter name)`. A module
    /// kind the registry does not know is skipped; loading reports it.
    pub fn transient_values(&self, registry: &Registry) -> Vec<(u32, String)> {
        self.modules
            .iter()
            .filter_map(|module| Some((module, registry.resolve(&module.kind).ok()?)))
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
    pub fn from_graph(name: impl Into<String>, graph: &Graph) -> Self {
        let mut patch = Patch::new(name);
        for (id, node) in graph.nodes() {
            let params = node
                .descriptor
                .params
                .iter()
                .zip(&node.params)
                // A transient value is a live gesture, never saved
                // (transient-params.md).
                .filter(|(spec, value)| **value != spec.default && !spec.is_transient())
                .map(|(spec, value)| (spec.name.to_string(), *value))
                .collect();
            patch.modules.push(PatchModule {
                id: id.0,
                kind: node.descriptor.kind(),
                label: None,
                params,
                position: None,
                ranges: ranges_to_save(node),
            });
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
            });
        }
        patch.canonicalise();
        patch
    }
}

/// The selected control ranges of a module that differ from the default,
/// by parameter name, for saving.
pub fn ranges_to_save(node: &loom_core::Node) -> BTreeMap<String, (f32, f32)> {
    node.descriptor
        .params
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| {
            let range = node.range(index)?;
            (Some(range) != spec.default_range()).then(|| (spec.name.to_string(), (range.min, range.max)))
        })
        .collect()
}

fn port_name(graph: &Graph, output: OutputRef) -> String {
    graph
        .node(output.module)
        .map(|n| n.descriptor.outputs[output.port].name.to_string())
        .unwrap_or_default()
}

fn input_name(graph: &Graph, input: InputRef) -> String {
    graph
        .node(input.module)
        .map(|n| n.descriptor.inputs[input.port].name.to_string())
        .unwrap_or_default()
}

fn param_name(graph: &Graph, module: ModuleId, index: usize) -> String {
    graph
        .node(module)
        .and_then(|n| n.descriptor.params.get(index))
        .map(|param| param.name.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{EngineConfig, create_engine};

    const SIMPLE: &str = r#"
// Comments are allowed.
Patch(
    version: 1,
    name: "Simple sine",
    modules: [
        (id: 3, kind: "loom.osc.sine@1"),
        (id: 1, kind: "loom.midi.note@1"),
        (id: 2, kind: "loom.phase.generator@1", params: {"tune": 12.0}),
        (id: 4, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        (from: (2, "phase"), to: (3, "phase")),
        (from: (1, "pitch"), to: (2, "pitch")),
        (from: (3, "out"), to: (4, "left")),
    ],
)
"#;

    /// Loads `text` into a new engine. Returns the controller and the notes.
    fn load_text(text: &str) -> (Controller, Vec<String>) {
        let patch = Patch::from_ron(text).unwrap();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let notes = patch.load_into(&mut parts.controller).unwrap();
        (parts.controller, notes)
    }

    /// Value and selected range of Multiply #1's gain.
    fn gain(controller: &Controller) -> (f32, Option<ParamRange>) {
        let node = controller.graph().node(ModuleId(1)).unwrap();
        (node.params[0], node.range(0))
    }

    /// control-ranges.md: a selected range saves next to the values and
    /// loads back. The default range is not written.
    #[test]
    fn ranges_save_and_reload() {
        let (mut c, notes) = load_text(
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1"), (id: 2, kind: "loom.math.mul@1")])"#,
        );
        assert!(notes.is_empty());
        assert_eq!(gain(&c), (1.0, Some(ParamRange::new(-1.0, 1.0))), "the default range");
        c.set_range(ModuleId(1), 0, ParamRange::new(-0.1, 0.1)).unwrap();
        c.set_param(ModuleId(1), 0, 0.08).unwrap();
        let text = Patch::from_graph("ranges", c.graph()).to_ron().unwrap();
        assert!(text.contains(r#"ranges: {"gain": (-0.1, 0.1)}"#), "{text}");
        assert_eq!(
            text.matches("ranges: {").count(),
            1,
            "module 2 keeps the default range: {text}"
        );

        let (reloaded, notes) = load_text(&text);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(gain(&reloaded), (0.08, Some(ParamRange::new(-0.1, 0.1))));
        // Two instances keep separate ranges.
        let other = reloaded.graph().node(ModuleId(2)).unwrap();
        assert_eq!(other.range(0), Some(ParamRange::new(-1.0, 1.0)));
    }

    /// Rule 7: a value outside the default range loads unchanged, with the
    /// smallest range that holds it. A saved range that does not hold the
    /// value does the same, with a note.
    #[test]
    fn a_range_never_changes_a_loaded_value() {
        let (c, notes) =
            load_text(r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 5.0})])"#);
        assert!(notes.is_empty());
        assert_eq!(gain(&c), (5.0, Some(ParamRange::new(-5.0, 5.0))));

        let (c, notes) = load_text(
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 5.0}, ranges: {"gain": (-0.1, 0.1)})])"#,
        );
        assert_eq!(gain(&c), (5.0, Some(ParamRange::new(-5.0, 5.0))));
        assert_eq!(notes.len(), 1);
        assert!(
            notes[0].contains("-0.1 to 0.1") && notes[0].contains("-5 to 5"),
            "{notes:?}"
        );

        let (c, notes) = load_text(
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 0.5}, ranges: {"gain": (-2.0, 2.0)})])"#,
        );
        assert_eq!(gain(&c), (0.5, Some(ParamRange::new(-1.0, 1.0))));
        assert!(notes[0].contains("does not offer"), "{notes:?}");

        // A range for a parameter the module does not have is an error, as
        // for a value.
        let patch = Patch::from_ron(
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", ranges: {"nope": (0.0, 1.0)})])"#,
        )
        .unwrap();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let error = patch.load_into(&mut parts.controller).unwrap_err();
        assert!(matches!(error, PatchError::UnknownParam { module: 1, .. }), "{error}");

        // A range on a parameter that offers none is ignored, with a note.
        let (_, notes) = load_text(
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.convert@1", ranges: {"mode": (0.0, 1.0)})])"#,
        );
        assert!(notes.iter().any(|n| n.contains("offers no ranges")), "{notes:?}");
    }

    /// Every example patch loads with the values in its file.
    #[test]
    fn example_patches_keep_their_values() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "ron") {
                continue;
            }
            let patch = Patch::load(&path).unwrap();
            let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
            let notes = patch.load_into(&mut parts.controller).unwrap();
            assert!(notes.is_empty(), "{}: {notes:?}", path.display());
            for module in &patch.modules {
                let node = parts.controller.graph().node(ModuleId(module.id)).unwrap();
                for (name, &value) in &module.params {
                    let index = node.descriptor.param_index(name).unwrap();
                    let expected = node.descriptor.params[index].clamp(value);
                    assert_eq!(node.params[index], expected, "{}: {} {name}", path.display(), module.id);
                }
            }
        }
    }

    /// A held momentary Button saves no `hold`, and a saved `hold` loads as
    /// released and is listed for `loom check` (transient-params.md).
    #[test]
    fn a_transient_hold_is_never_saved_or_loaded() {
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let c = &mut parts.controller;
        let id = c.add_module("loom.ui.button").unwrap();
        c.set_param_named(id, "mode", 1.0).unwrap();
        c.set_param_named(id, "hold", 1.0).unwrap();
        c.commit().unwrap();
        let saved = Patch::from_graph("held", c.graph());
        let text = saved.to_ron().unwrap();
        assert!(!text.contains("hold"), "{text}");
        assert!(text.contains("mode"), "{text}");

        let file =
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.ui.button@1", params: {"hold": 1.0, "on": 1.0})])"#;
        let patch = Patch::from_ron(file).unwrap();
        let registry = loom_modules::registry();
        assert_eq!(patch.transient_values(&registry), vec![(1, "hold".to_string())]);
        let mut parts = create_engine(registry, EngineConfig::default());
        patch.load_into(&mut parts.controller).unwrap();
        let node = parts.controller.graph().node(ModuleId(1)).unwrap();
        let hold = node.descriptor.param_index("hold").unwrap();
        let on = node.descriptor.param_index("on").unwrap();
        assert_eq!(node.params[hold], 0.0);
        assert_eq!(node.params[on], 1.0);
    }

    #[test]
    fn loads_and_round_trips() {
        let patch = Patch::from_ron(SIMPLE).unwrap();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        patch.load_into(&mut parts.controller).unwrap();

        let saved = Patch::from_graph("Simple sine", parts.controller.graph());
        let mut expected = patch.clone();
        expected.canonicalise();
        assert_eq!(saved, expected);

        // Canonical text is stable across a parse/print cycle.
        let text = saved.to_ron().unwrap();
        assert_eq!(Patch::from_ron(&text).unwrap().to_ron().unwrap(), text);
    }

    #[test]
    fn rejects_newer_versions() {
        let text = SIMPLE.replace("version: 1", "version: 99");
        assert!(matches!(
            Patch::from_ron(&text),
            Err(PatchError::TooNew { found: 99, .. })
        ));
    }

    const MODULATED: &str = r#"
Patch(
    version: 1,
    name: "Modulated",
    modules: [
        (id: 1, kind: "loom.math.constant@1", params: {"value": 1.0}),
        (id: 2, kind: "loom.env.envelope@1"),
        (id: 3, kind: "loom.filter.svf@1"),
    ],
    modulation: [
        (source: (2, "out"), target: (3, "resonance"), depth: 0.4, polarity: Unipolar, enabled: false),
        (source: (1, "out"), target: (3, "cutoff"), depth: 0.25),
    ],
)
"#;

    #[test]
    fn modulation_round_trips_and_old_patches_load() {
        let old = Patch::from_ron(SIMPLE).unwrap();
        assert!(old.modulation.is_empty());
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        old.load_into(&mut parts.controller).unwrap();
        let saved = Patch::from_graph("Simple sine", parts.controller.graph());
        assert!(saved.modulation.is_empty());
        assert!(!saved.to_ron().unwrap().contains("modulation"));

        let patch = Patch::from_ron(MODULATED).unwrap();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        patch.load_into(&mut parts.controller).unwrap();
        let saved = Patch::from_graph("Modulated", parts.controller.graph());
        let mut expected = patch.clone();
        expected.canonicalise();
        assert_eq!(saved, expected);
        // File order is kept, not sorted by target.
        assert_eq!(saved.modulation[0].target.1, "resonance");
        assert_eq!(saved.modulation[0].polarity, Some(PatchPolarity::Unipolar));
        assert!(!saved.modulation[0].enabled);
        assert_eq!(saved.modulation[1].target.1, "cutoff");
        assert!(saved.modulation[1].polarity.is_none());
        assert!(saved.modulation[1].enabled);

        let text = saved.to_ron().unwrap();
        assert!(text.contains("modulation:"));
        assert!(text.contains("enabled: false"));
        assert!(!text.contains("enabled: true"));
        assert_eq!(Patch::from_ron(&text).unwrap().to_ron().unwrap(), text);
    }

    /// The engine adds assignments on one parameter in id order. Float sums
    /// depend on order, so a reloaded patch must keep it (review on #133).
    #[test]
    fn save_replaces_the_file_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("loom-patch-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.ron");
        std::fs::write(&path, "old").unwrap();
        let patch = Patch::new("Saved");
        patch.save(&path).unwrap();
        assert_eq!(Patch::load(&path).unwrap().name, "Saved");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file is left");
        // A target that cannot be written reports the error and keeps the old file.
        let missing = dir.join("no-such-folder").join("p.ron");
        assert!(matches!(patch.save(&missing), Err(PatchError::Io { .. })));
        // A read-only target is refused and keeps its content.
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms.clone()).unwrap();
        assert!(matches!(Patch::new("Other").save(&path), Err(PatchError::Io { .. })));
        assert_eq!(Patch::load(&path).unwrap().name, "Saved");
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_through_a_symlink_updates_the_target() {
        let dir = std::env::temp_dir().join(format!("loom-patch-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.ron");
        let link = dir.join("link.ron");
        Patch::new("First").save(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        Patch::new("Second").save(&link).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(Patch::load(&real).unwrap().name, "Second");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn assignment_order_survives_save_and_load() {
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let controller = &mut parts.controller;
        let low = controller.add_module("loom.math.constant").unwrap();
        let high = controller.add_module("loom.math.constant").unwrap();
        let filter = controller.add_module("loom.filter.svf").unwrap();
        // Made from the higher module id first, so sorting by source would
        // swap them.
        controller
            .add_assignment_named(high, "out", filter, "cutoff", 0.5)
            .unwrap();
        controller
            .add_assignment_named(low, "out", filter, "cutoff", -0.25)
            .unwrap();
        let order = |graph: &Graph| -> Vec<(OutputRef, f32)> {
            graph.assignments().map(|(_, a)| (a.source, a.depth)).collect()
        };
        let before = order(controller.graph());

        let text = Patch::from_graph("Order", controller.graph()).to_ron().unwrap();
        let mut reloaded = create_engine(loom_modules::registry(), EngineConfig::default());
        Patch::from_ron(&text)
            .unwrap()
            .load_into(&mut reloaded.controller)
            .unwrap();
        assert_eq!(order(reloaded.controller.graph()), before);
    }

    const MAPPED: &str = r#"
Patch(
    version: 1,
    name: "Mapped",
    modules: [
        (id: 1, kind: "loom.filter.svf@1"),
    ],
    midi: [
        (target: (1, "resonance"), channel: 0, controller: 71),
        (target: (1, "cutoff"), channel: 2, controller: 74),
    ],
)
"#;

    #[test]
    fn midi_mappings_round_trip_and_old_patches_load() {
        let old = Patch::from_ron(SIMPLE).unwrap();
        assert!(old.midi.is_empty());
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        old.load_into(&mut parts.controller).unwrap();
        assert_eq!(parts.controller.graph().midi_mappings().count(), 0);
        let text = Patch::from_graph("Simple sine", parts.controller.graph())
            .to_ron()
            .unwrap();
        assert!(!text.contains("midi:"));

        let patch = Patch::from_ron(MAPPED).unwrap();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        patch.load_into(&mut parts.controller).unwrap();
        let saved = Patch::from_graph("Mapped", parts.controller.graph());
        let mut expected = patch.clone();
        expected.canonicalise();
        assert_eq!(saved, expected);
        // Sorted by target.
        assert_eq!(saved.midi[0].target.1, "cutoff");
        assert_eq!((saved.midi[0].channel, saved.midi[0].controller), (2, 74));
        let text = saved.to_ron().unwrap();
        assert_eq!(Patch::from_ron(&text).unwrap().to_ron().unwrap(), text);
    }

    #[test]
    fn rejects_bad_midi_mappings() {
        let load = |text: &str| {
            let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
            Patch::from_ron(text).unwrap().load_into(&mut parts.controller)
        };
        let same_param = MAPPED.replace("\"resonance\"", "\"cutoff\"");
        assert!(matches!(
            load(&same_param),
            Err(PatchError::DuplicateMidiMapping { .. })
        ));
        let same_control = MAPPED.replace("channel: 2, controller: 74", "channel: 0, controller: 71");
        assert!(matches!(
            load(&same_control),
            Err(PatchError::DuplicateMidiMapping { .. })
        ));
        let unknown = MAPPED.replace("\"cutoff\"", "\"cut\"");
        assert!(matches!(
            load(&unknown),
            Err(PatchError::UnknownParam { module: 1, .. })
        ));
        let bad_channel = MAPPED.replace("channel: 2", "channel: 16");
        assert!(matches!(
            load(&bad_channel),
            Err(PatchError::Engine(LoomError::InvalidMidiControl(_)))
        ));
    }

    #[test]
    fn rejects_unknown_params() {
        let text = SIMPLE.replace("\"tune\"", "\"tuning\"");
        let patch = Patch::from_ron(&text).unwrap();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        assert!(matches!(
            patch.load_into(&mut parts.controller),
            Err(PatchError::UnknownParam { module: 2, .. })
        ));
    }
}
