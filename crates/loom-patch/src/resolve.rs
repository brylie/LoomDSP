//! Patch modules resolved against a registry, as plain data (#651).
//!
//! A [`PatchModule`] names things: a kind, parameters, ranges. Resolving
//! looks those names up once and gives a [`ResolvedModule`]: the descriptor
//! or placeholder data, values and ranges in parameter order, the label, and
//! the layout. Then each caller applies it in its own way. Loading a patch
//! adds each module with the ID from the file; the clipboard renumbers them.
//! Flattening a sub-patch will add them with IDs from a table keyed by path
//! (`docs/design/sub-patches.md`, Flattening). The name lookups live here,
//! once, and never in a caller.
//!
//! Control thread only. Nothing here runs on the audio thread.

use crate::level::LevelRef;
use crate::{LoadWarning, Patch, PatchError, PatchModule, RangeReason};
use loom_core::{
    Controller, LoomError, ModuleDescriptor, ModuleId, ModuleNames, Node, ParamRange, ParamSpec, Registry,
    UnknownModule,
};

/// Where a module sits on the canvas. The editor's data, not the engine's.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Layout {
    /// Position on the canvas.
    pub position: Option<(f32, f32)>,
    /// Tile width and height in grid units.
    pub size: Option<(u8, u8)>,
}

/// How strict resolving is. A patch file and clipboard text differ in two
/// ways, kept as they were before the two shared this code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveOptions {
    /// Accept a kind with no `@version` when the registry has it. Text
    /// written by hand into the clipboard may leave the version out. A
    /// patch file may not (`docs/design/unknown-modules.md`).
    pub versionless_kinds: bool,
    /// Refuse a saved range on a parameter the module does not have. A
    /// patch file is refused; the clipboard ignores the range.
    pub refuse_ranges_on_missing_params: bool,
}

impl ResolveOptions {
    /// For a patch file: strict.
    pub const FILE: Self = Self {
        versionless_kinds: false,
        refuse_ranges_on_missing_params: true,
    };
    /// For clipboard text: what a person might type is accepted.
    pub const CLIPBOARD: Self = Self {
        versionless_kinds: true,
        refuse_ranges_on_missing_params: false,
    };
}

/// A module whose names were looked up in a registry.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    /// Its ID in the patch, the clipboard text, or the graph it came from.
    pub id: u32,
    /// The module type. For a placeholder, the shared silent descriptor.
    pub descriptor: &'static ModuleDescriptor,
    /// Saved data, if the kind is unknown to this build.
    pub unknown: Option<UnknownModule>,
    /// The user's label, exactly as written (clipboard.md, rule 9).
    pub label: Option<String>,
    /// One value per parameter of the descriptor. Transient parameters are
    /// released (transient-params.md).
    pub params: Vec<f32>,
    /// The control range of each parameter, or `None` for one that offers
    /// no ranges.
    pub ranges: Vec<Option<ParamRange>>,
    /// Where it sits on the canvas.
    pub layout: Layout,
}

/// Why a module could not be resolved.
#[derive(Debug)]
pub enum ResolveError {
    /// A parameter name the module does not have.
    UnknownParam {
        /// The module's ID in the patch.
        module: u32,
        /// The module type's display name, for messages.
        module_name: &'static str,
        /// The parameter name.
        param: String,
    },
    /// The kind is not valid, or the registry refuses it.
    Patch(PatchError),
}

impl From<PatchError> for ResolveError {
    fn from(error: PatchError) -> Self {
        Self::Patch(error)
    }
}

impl From<LoomError> for ResolveError {
    fn from(error: LoomError) -> Self {
        Self::Patch(error.into())
    }
}

impl From<ResolveError> for PatchError {
    fn from(error: ResolveError) -> Self {
        match error {
            ResolveError::UnknownParam { module, param, .. } => PatchError::UnknownParam { module, param },
            ResolveError::Patch(error) => error,
        }
    }
}

impl ModuleNames for ResolvedModule {
    fn descriptor(&self) -> &'static ModuleDescriptor {
        self.descriptor
    }

    fn unknown(&self) -> Option<&UnknownModule> {
        self.unknown.as_ref()
    }
}

impl ResolvedModule {
    /// A module in a graph, as saving or copying sees it: transient values
    /// released, and every value and range as they are now.
    pub fn from_node(id: ModuleId, node: &Node, layout: Layout) -> Self {
        Self {
            id: id.0,
            descriptor: node.descriptor,
            unknown: node.unknown.clone(),
            label: node.label.clone(),
            params: node.descriptor.saved_values(&node.params),
            ranges: (0..node.params.len()).map(|index| node.range(index)).collect(),
            layout,
        }
    }

    /// Adds this module to `controller` as `id`, then [`Self::apply_to`].
    /// Loading a patch keeps the file's IDs this way.
    pub fn add_to(&self, controller: &mut Controller, id: ModuleId) -> Result<(), PatchError> {
        match &self.unknown {
            Some(unknown) => controller.add_unknown_with_id(id, unknown.clone())?,
            None => controller.add_module_with_id(id, &self.descriptor.kind())?,
        }
        Ok(self.apply_to(controller, id)?)
    }

    /// Gives module `id`, just added with this module's kind, its label,
    /// its ranges, then its values. Ranges come first, so a range never
    /// clamps a value (control-ranges.md, rule 7). A new module starts at
    /// its defaults, so only values that differ are set, and never a
    /// transient one (transient-params.md). Loading, pasting, and
    /// flattening all add modules through this.
    pub fn apply_to(&self, controller: &mut Controller, id: ModuleId) -> Result<(), LoomError> {
        controller.set_label(id, self.label.clone());
        for (index, range) in self.ranges.iter().enumerate() {
            if let Some(range) = *range
                && controller.graph().node(id).and_then(|n| n.range(index)) != Some(range)
            {
                controller.set_range(id, index, range)?;
            }
        }
        let changes: Vec<(usize, f32)> = self
            .descriptor
            .params
            .iter()
            .zip(&self.params)
            .enumerate()
            .filter(|(_, (spec, value))| **value != spec.default && !spec.is_transient())
            .map(|(index, (_, value))| (index, *value))
            .collect();
        if !changes.is_empty() {
            controller.set_params(id, &changes)?;
        }
        Ok(())
    }

    /// The module as the patch format writes it. Only values and ranges
    /// that differ from their defaults are written, and never a transient
    /// value. A placeholder writes back the data it was loaded with.
    pub fn write(&self) -> PatchModule {
        let Layout { position, size } = self.layout;
        if let Some(unknown) = &self.unknown {
            return PatchModule {
                id: self.id,
                kind: unknown.kind.clone(),
                label: self.label.clone(),
                params: unknown.params.clone(),
                position,
                size,
                ranges: unknown.ranges.clone(),
            };
        }
        let specs = self.descriptor.params;
        PatchModule {
            id: self.id,
            kind: self.descriptor.kind(),
            label: self.label.clone(),
            params: specs
                .iter()
                .zip(&self.params)
                // A transient value is a live gesture, never saved
                // (transient-params.md).
                .filter(|(spec, value)| **value != spec.default && !spec.is_transient())
                .map(|(spec, value)| (spec.name.to_string(), *value))
                .collect(),
            position,
            size,
            ranges: specs
                .iter()
                .zip(&self.ranges)
                .filter_map(|(spec, range)| {
                    let range = (*range)?;
                    (Some(range) != spec.default_range()).then(|| (spec.name.to_string(), (range.min, range.max)))
                })
                .collect(),
        }
    }
}

impl Patch {
    /// Resolves every module, in file order. Returns them with what
    /// resolved differently from the file, such as a saved range that was
    /// not kept. Nothing is added anywhere.
    pub fn resolve(
        &self,
        registry: &Registry,
        options: ResolveOptions,
    ) -> Result<(Vec<ResolvedModule>, Vec<LoadWarning>), ResolveError> {
        self.level().resolve(registry, options)
    }

    /// Resolves one module of this patch. A placeholder takes its ports
    /// from this patch's cables, assignments, and MIDI mappings.
    pub fn resolve_module(
        &self,
        module: &PatchModule,
        registry: &Registry,
        options: ResolveOptions,
        warnings: &mut Vec<LoadWarning>,
    ) -> Result<ResolvedModule, ResolveError> {
        self.level().resolve_module(module, registry, options, warnings)
    }
}

impl LevelRef<'_> {
    /// Resolves every module of this level, in file order.
    pub(crate) fn resolve(
        &self,
        registry: &Registry,
        options: ResolveOptions,
    ) -> Result<(Vec<ResolvedModule>, Vec<LoadWarning>), ResolveError> {
        let mut modules = Vec::with_capacity(self.modules.len());
        let mut warnings = Vec::new();
        for module in self.modules {
            modules.push(self.resolve_module(module, registry, options, &mut warnings)?);
        }
        Ok((modules, warnings))
    }

    /// Resolves one module of this level. A placeholder takes its ports
    /// from this level's cables, assignments, and MIDI mappings.
    pub(crate) fn resolve_module(
        &self,
        module: &PatchModule,
        registry: &Registry,
        options: ResolveOptions,
        warnings: &mut Vec<LoadWarning>,
    ) -> Result<ResolvedModule, ResolveError> {
        let layout = Layout {
            position: module.position,
            size: module.size,
        };
        let versionless =
            options.versionless_kinds && !module.kind.contains('@') && registry.resolve(&module.kind).is_ok();
        let unknown = if versionless {
            None
        } else {
            self.unknown_module(module, registry)?
        };
        if let Some(unknown) = unknown {
            return Ok(ResolvedModule {
                id: module.id,
                descriptor: UnknownModule::descriptor(),
                unknown: Some(unknown),
                label: module.label.clone(),
                params: Vec::new(),
                ranges: Vec::new(),
                layout,
            });
        }
        let descriptor = registry.resolve(&module.kind)?;
        let unknown_param = |param: &str| ResolveError::UnknownParam {
            module: module.id,
            module_name: descriptor.name,
            param: param.to_string(),
        };
        for name in module.ranges.keys() {
            match descriptor.param_index(name) {
                None if options.refuse_ranges_on_missing_params => return Err(unknown_param(name)),
                None => {}
                Some(index) if descriptor.params[index].options.ranges.is_empty() => {
                    warnings.push(LoadWarning::RangeIgnored {
                        module: module.id,
                        param: name.clone(),
                    });
                }
                Some(_) => {}
            }
        }
        let mut values = descriptor.default_params();
        for (name, &value) in &module.params {
            let index = descriptor.param_index(name).ok_or_else(|| unknown_param(name))?;
            values[index] = value;
        }
        // A saved value for a transient parameter is ignored: the module
        // starts released (transient-params.md). `transient_values` lists
        // them for `loom check`.
        let params = descriptor.saved_values(&values);
        let ranges = (0..descriptor.params.len())
            .map(|index| resolve_range(module, descriptor, &params, index, warnings))
            .collect();
        Ok(ResolvedModule {
            id: module.id,
            descriptor,
            unknown: None,
            label: module.label.clone(),
            params,
            ranges,
            layout,
        })
    }
}

/// The range parameter `index` loads with, chosen so it holds the value
/// (control-ranges.md, rule 7), with a warning when the saved range was
/// not kept. `None` for a parameter that offers no ranges.
fn resolve_range(
    module: &PatchModule,
    descriptor: &'static ModuleDescriptor,
    params: &[f32],
    index: usize,
    warnings: &mut Vec<LoadWarning>,
) -> Option<ParamRange> {
    let saved_of = |spec: &ParamSpec| {
        module
            .ranges
            .get(spec.name)
            .map(|&(min, max)| ParamRange::new(min, max))
    };
    let value_of = |spec: &ParamSpec| {
        descriptor
            .param_index(spec.name)
            .map_or(spec.default, |at| spec.clamp(params[at]))
    };
    let spec = &descriptor.params[index];
    let (range, reported) = descriptor.load_range(index, saved_of, value_of)?;
    match (reported, descriptor.range_partner(index), saved_of(spec)) {
        // Said once for the pair (control-ranges.md, Ranges on a range
        // widget).
        (true, Some(partner), _) if index < partner => warnings.push(LoadWarning::SharedRangeNotKept {
            module: module.id,
            param: spec.name.to_string(),
            partner: descriptor.params[partner].name.to_string(),
            loaded: range,
        }),
        (true, None, Some(saved)) => warnings.push(LoadWarning::RangeNotKept {
            module: module.id,
            param: spec.name.to_string(),
            saved,
            reason: if spec.options.ranges.contains(&saved) {
                RangeReason::DoesNotHold { value: value_of(spec) }
            } else {
                RangeReason::NotOffered
            },
            loaded: range,
        }),
        _ => {}
    }
    Some(range)
}

#[cfg(test)]
mod tests;
