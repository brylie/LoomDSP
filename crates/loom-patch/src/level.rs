//! One level of a patch: the top patch, or a sub-patch definition
//! (`docs/design/sub-patches.md`, Terms).
//!
//! Resolving a module, building a placeholder's ports, checking MIDI
//! mappings, and listing transient values all read the level's own
//! modules, cables, assignments, and mappings. Both [`Patch`] and a
//! definition give a [`LevelRef`], so those rules live here once.

use crate::{Patch, PatchCable, PatchError, PatchMidiMapping, PatchModulation, PatchModule};
use loom_core::{LoomError, Registry, UnknownModule};

/// A borrowed view of one level's lists.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LevelRef<'a> {
    pub(crate) modules: &'a [PatchModule],
    pub(crate) cables: &'a [PatchCable],
    pub(crate) modulation: &'a [PatchModulation],
    pub(crate) midi: &'a [PatchMidiMapping],
}

impl Patch {
    /// The top level of this patch.
    pub(crate) fn level(&self) -> LevelRef<'_> {
        LevelRef {
            modules: &self.modules,
            cables: &self.cables,
            modulation: &self.modulation,
            midi: &self.midi,
        }
    }
}

impl LevelRef<'_> {
    /// Builds the owned data for a missing module from names in this level.
    /// A registered kind at the exact version returns `None`.
    pub(crate) fn unknown_module(
        &self,
        module: &PatchModule,
        registry: &Registry,
    ) -> Result<Option<UnknownModule>, PatchError> {
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
        for cable in self.cables {
            if cable.to.0 == module.id {
                push(&mut inputs, &cable.to.1);
            }
            if cable.from.0 == module.id {
                push(&mut outputs, &cable.from.1);
            }
        }
        for assignment in self.modulation {
            if assignment.source.0 == module.id {
                push(&mut outputs, &assignment.source.1);
            }
            if assignment.target.0 == module.id {
                push(&mut parameters, &assignment.target.1);
            }
        }
        for mapping in self.midi {
            if mapping.target.0 == module.id {
                push(&mut parameters, &mapping.target.1);
            }
        }
        Ok(Some(UnknownModule {
            kind: module.kind.clone(),
            params: module.params.clone(),
            ranges: module.ranges.clone(),
            inputs,
            outputs,
            parameters,
        }))
    }

    /// Refuses two MIDI mappings on one parameter or one control. The
    /// controller would quietly keep the last one, which hides the mistake.
    pub(crate) fn check_midi(&self) -> Result<(), PatchError> {
        let mut targets = std::collections::BTreeSet::new();
        let mut controls = std::collections::BTreeSet::new();
        for mapping in self.midi {
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

    /// The saved values that loading ignores because their parameter is
    /// transient, as `(module id, parameter name)`. A module kind the
    /// registry does not know is skipped; loading reports it.
    pub(crate) fn transient_values(&self, registry: &Registry) -> Vec<(u32, String)> {
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
}
