//! Sub-patch definitions in a patch file (`docs/design/sub-patches.md`,
//! Patch file; ADR-0031).
//!
//! A patch has one `subpatches` table. A definition is the inside of a
//! sub-patch: its own modules, cables, assignments, mappings, and the
//! parameters it exposes. An instance is a [`PatchModule`] of kind
//! [`SUBPATCH_KIND`] whose `subpatch` names a key of the table; its
//! `params` hold the values of the definition's exposed parameters, by
//! exposed name. Every instance has its own definition (rule 3: no action
//! at a distance). Module IDs are unique within one definition, not across
//! the patch.
//!
//! Nothing here reaches the engine. Flattening (`flatten.rs`) expands each
//! instance into ordinary modules before a patch is applied.

use crate::level::LevelRef;
use crate::{Metadata, PatchCable, PatchMidiMapping, PatchModulation, PatchModule, canonicalise_lists};
use loom_core::Registry;
use serde::{Deserialize, Serialize};

/// The kind of a sub-patch instance (MT-STRUCT-01). Not a registered
/// module: the flattener expands it.
pub const SUBPATCH_KIND: &str = "loom.struct.subpatch@1";
/// The kind of a Patch input module (MT-STRUCT-02). Inside a definition it
/// becomes one input port of the instance, named by its label.
pub const INPUT_KIND: &str = "loom.struct.input@1";
/// The kind of a Patch output module (MT-STRUCT-02). Inside a definition
/// it becomes one output port of the instance, named by its label.
pub const OUTPUT_KIND: &str = "loom.struct.output@1";

/// The inside of a sub-patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubPatch {
    /// Its name, and the credit for a library item
    /// (`docs/design/patch-metadata.md`, credit for parts).
    #[serde(default)]
    pub metadata: Metadata,
    /// Modules. A `loom.struct.input@1` or `loom.struct.output@1` here is a
    /// port of the sub-patch.
    #[serde(default)]
    pub modules: Vec<PatchModule>,
    /// Cables.
    #[serde(default)]
    pub cables: Vec<PatchCable>,
    /// Modulation assignments. Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modulation: Vec<PatchModulation>,
    /// MIDI mappings. Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub midi: Vec<PatchMidiMapping>,
    /// Inner parameters shown as the sub-patch's own (FR-ENCAP-03). Omitted
    /// when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exposed: Vec<Exposed>,
    /// For a linked instance (`docs/design/sub-patches.md`, Linked library
    /// items): the library file this definition follows, as a path
    /// relative to the library folder with `/` between its parts. The
    /// definition itself is the stored copy, used when the file is
    /// missing. Omitted for a plain copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    /// The roles this sub-patch fills (FR-ENCAP-09), as role kinds such as
    /// `"loom.role.filter@1"`, so it can be the part of a slot of that
    /// role. Checked against its ports and exposed items when it is saved
    /// to the library, inserted, or placed in a slot
    /// ([`SubPatch::prune_roles`]). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

/// An inner parameter exposed on the sub-patch (`docs/design/sub-patches.md`,
/// Exposing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Exposed {
    /// The name on the sub-patch. An instance's `params` use it.
    pub name: String,
    /// The inner module's ID in the definition.
    pub module: u32,
    /// The inner parameter's name.
    pub param: String,
    /// A narrower range in the same unit, as `(min, max)`. The value is
    /// clamped to it, not rescaled. Absent means the parameter's own range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<(f32, f32)>,
}

impl SubPatch {
    /// An empty definition called `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            metadata: Metadata {
                name: name.into(),
                ..Metadata::default()
            },
            modules: Vec::new(),
            cables: Vec::new(),
            modulation: Vec::new(),
            midi: Vec::new(),
            exposed: Vec::new(),
            library: None,
            roles: Vec::new(),
        }
    }

    /// Sorts modules, cables, and mappings into canonical order, as
    /// [`crate::Patch::canonicalise`] does. Modulation keeps its order.
    pub fn canonicalise(&mut self) {
        canonicalise_lists(&mut self.modules, &mut self.cables, &mut self.midi);
    }

    /// The saved values that loading ignores because their parameter is
    /// transient, as `(module id, parameter name)` in this definition, as
    /// [`crate::Patch::transient_values`] lists them for the top level.
    pub fn transient_values(&self, registry: &Registry) -> Vec<(u32, String)> {
        self.level().transient_values(registry)
    }

    /// This definition as one level.
    fn level(&self) -> LevelRef<'_> {
        LevelRef {
            modules: &self.modules,
            cables: &self.cables,
            modulation: &self.modulation,
            midi: &self.midi,
        }
    }
}

impl PatchModule {
    /// True for a sub-patch instance.
    pub fn is_instance(&self) -> bool {
        self.kind == SUBPATCH_KIND
    }
}

#[cfg(test)]
mod tests;
