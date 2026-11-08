//! Slots in a patch file (`docs/design/sub-patches.md`, Slots and Patch
//! file; ADR-0017, MT-STRUCT-03).
//!
//! A slot is a [`PatchModule`] of kind [`SLOT_KIND`] with a [`Slot`] field,
//! as an instance has a `subpatch` field. Its current part is an ordinary
//! module of the same level, named by id, which the editor hides behind
//! the slot's tile. Every cable, assignment, mapping, and exposure on the
//! part goes through the slot, by the role's names. [`Slot::held`] keeps
//! what every other part the slot has held would need to come back: its
//! values and control ranges by its own names, keyed by what the part is.
//!
//! Nothing here reaches the engine. The flattener routes the slot's ports
//! and parameters to the part's (`flatten.rs`) and adds nothing.

use crate::{Exposed, INPUT_KIND, OUTPUT_KIND, PatchModule, SubPatch};
use loom_core::{ParamKind, Registry, RoleDescriptor, RoleMismatch, SignalType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The kind of a slot (MT-STRUCT-03). Not a registered module: the
/// flattener routes through it.
pub const SLOT_KIND: &str = "loom.struct.slot@1";

/// The kind of the swap fade the editor adds during a swap
/// (`loom.struct.fade` in `loom-modules`, which a test there checks
/// against this). Never in a file.
pub const FADE_KIND: &str = "loom.struct.fade@1";

/// What a slot holds at one time: a module type, or a sub-patch
/// definition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PartKey {
    /// A module, by its `"id@version"` kind.
    Module(String),
    /// A sub-patch, by its key in the `subpatches` table. The definition
    /// stays in the table while the slot holds the part, used by no
    /// instance, so swapping back restores it whole.
    SubPatch(String),
}

impl std::fmt::Display for PartKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module(kind) => write!(f, "module {kind}"),
            Self::SubPatch(key) => write!(f, "sub-patch `{key}`"),
        }
    }
}

/// What a held part would need to come back: its values and control
/// ranges by its own names (for a sub-patch, its exposed names). Later its
/// data too ([module data](../../../docs/design/module-data.md)).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Held {
    /// Parameter values by the part's own names. Omitted when empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, f32>,
    /// Selected control ranges by the part's own names, as `(min, max)`.
    /// Omitted when empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ranges: BTreeMap<String, (f32, f32)>,
}

/// The `slot` field of a slot module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// The role's `"id@version"`.
    pub role: String,
    /// The level id of the module or instance that fills the slot now, or
    /// `None` for an empty slot, which plays as a module with nothing
    /// behind its ports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
    /// Every other part the slot has held, keyed by what it is. Omitted
    /// when empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub held: BTreeMap<PartKey, Held>,
}

impl Slot {
    /// An empty slot for `role`.
    pub fn new(role: &RoleDescriptor) -> Self {
        Self {
            role: role.kind(),
            part: None,
            held: BTreeMap::new(),
        }
    }
}

impl PatchModule {
    /// True for a slot.
    pub fn is_slot(&self) -> bool {
        self.kind == SLOT_KIND
    }
}

impl SubPatch {
    /// What this definition lacks for `role` (sub-patches.md, Roles): a
    /// Patch input or output of each required port's name, and an exposed
    /// item of each required parameter's name whose inner parameter is of
    /// the role's kind. `table` resolves an exposed item that reaches
    /// through a nested instance. Empty when the definition fits.
    pub fn role_mismatches(
        &self,
        role: &RoleDescriptor,
        registry: &Registry,
        table: &BTreeMap<String, SubPatch>,
    ) -> Vec<RoleMismatch> {
        let port = |kind: &str, name: &str| -> Option<SignalType> {
            self.modules
                .iter()
                .find(|m| {
                    m.kind == kind
                        && crate::flatten::port_name(m, if kind == INPUT_KIND { "in" } else { "out" }) == name
                })
                .map(|_| {
                    // A port tile carries whatever the cable behind it
                    // carries; the role's kind is taken on trust, as the
                    // cable's type is a hint and never a limit (ADR-0007).
                    role.input(name)
                        .or_else(|| role.output(name))
                        .map_or(SignalType::Audio, |p| p.signal)
                })
        };
        role.mismatches(
            |name| port(INPUT_KIND, name),
            |name| port(OUTPUT_KIND, name),
            |name| {
                let exposed = self.exposed.iter().find(|e| e.name == name)?;
                exposed_kind(self, exposed, registry, table, 0)
            },
            "exposed",
        )
    }

    /// True when this definition declares `role` and fits it.
    pub fn fills(&self, role: &RoleDescriptor, registry: &Registry, table: &BTreeMap<String, SubPatch>) -> bool {
        self.roles.iter().any(|r| crate::role_id(r) == role.id)
            && self.role_mismatches(role, registry, table).is_empty()
    }

    /// Drops every declared role this definition no longer fits, and
    /// returns `(role kind, why)` for each (sub-patches.md, Roles: a failed
    /// check removes the role and says why, and does not block the save).
    /// A role the registry does not know is dropped too.
    pub fn prune_roles(&mut self, registry: &Registry, table: &BTreeMap<String, SubPatch>) -> Vec<(String, String)> {
        let mut dropped = Vec::new();
        let roles = std::mem::take(&mut self.roles);
        for kind in roles {
            let Ok(role) = registry.resolve_role(&kind) else {
                dropped.push((kind.clone(), "this build does not know the role".to_string()));
                continue;
            };
            let mismatches = self.role_mismatches(role, registry, table);
            if mismatches.is_empty() {
                self.roles.push(kind);
            } else {
                let why: Vec<String> = mismatches.iter().map(ToString::to_string).collect();
                dropped.push((kind, why.join(", ")));
            }
        }
        dropped
    }
}

/// The kind of the inner parameter an exposed item stands for, through
/// any nesting, or `None` when the chain breaks. A placeholder for an
/// unknown module gives no kind.
fn exposed_kind(
    definition: &SubPatch,
    exposed: &Exposed,
    registry: &Registry,
    table: &BTreeMap<String, SubPatch>,
    depth: usize,
) -> Option<ParamKind> {
    if depth > crate::flatten::MAX_DEPTH {
        return None;
    }
    let member = definition.modules.iter().find(|m| m.id == exposed.module)?;
    if let Some(key) = &member.subpatch {
        let nested = table.get(key)?;
        let inner = nested.exposed.iter().find(|e| e.name == exposed.param)?;
        return exposed_kind(nested, inner, registry, table, depth + 1);
    }
    if member.is_slot() {
        // A slot's role parameter has the role's kind, whatever part is in.
        let slot = member.slot.as_ref()?;
        let role = registry.resolve_role(&slot.role).ok()?;
        let param = role.param(&exposed.param)?;
        return Some(ParamKind {
            unit: param.unit,
            whole: param.whole,
        });
    }
    let descriptor = registry.resolve(&member.kind).ok()?;
    let spec = descriptor.params.iter().find(|p| p.name == exposed.param)?;
    Some(ParamKind::of(spec))
}

#[cfg(test)]
pub(crate) mod tests;
