//! Port and parameter names of a module that may be a placeholder.
//!
//! A registered module takes its names from its descriptor. A placeholder
//! for an unknown module (`docs/design/unknown-modules.md`) takes them from
//! the patch file instead. The graph, the patch loader, and the clipboard
//! all need the same lookups for both, so they live here once (#441).
//!
//! Control thread only. Nothing here runs on the audio thread.

use crate::{ModuleDescriptor, UnknownModule};

/// The names, display labels, indices, and counts of a module's inputs,
/// outputs, and parameters, for a registered module or a placeholder.
///
/// Implementors give the descriptor and the placeholder data. Every lookup
/// is provided, so the choice between the two is made in one place.
pub trait ModuleNames {
    /// The module's descriptor. For a placeholder, the shared silent one.
    fn descriptor(&self) -> &'static ModuleDescriptor;

    /// The placeholder data, if this module's kind is unknown.
    fn unknown(&self) -> Option<&UnknownModule>;

    /// Number of inputs, including placeholder inputs.
    fn input_count(&self) -> usize {
        self.unknown()
            .map_or(self.descriptor().inputs.len(), |unknown| unknown.inputs.len())
    }

    /// Number of outputs, including placeholder outputs.
    fn output_count(&self) -> usize {
        self.unknown()
            .map_or(self.descriptor().outputs.len(), |unknown| unknown.outputs.len())
    }

    /// Number of parameters, including placeholder parameters.
    fn parameter_count(&self) -> usize {
        self.unknown()
            .map_or(self.descriptor().params.len(), |unknown| unknown.parameters.len())
    }

    /// The saved name of an input, as patch files write it.
    fn input_name(&self, index: usize) -> Option<&str> {
        match self.unknown() {
            Some(unknown) => unknown.inputs.get(index).map(String::as_str),
            None => self.descriptor().inputs.get(index).map(|p| p.name),
        }
    }

    /// The saved name of an output, as patch files write it.
    fn output_name(&self, index: usize) -> Option<&str> {
        match self.unknown() {
            Some(unknown) => unknown.outputs.get(index).map(String::as_str),
            None => self.descriptor().outputs.get(index).map(|p| p.name),
        }
    }

    /// The saved name of a parameter, as patch files write it.
    fn parameter_name(&self, index: usize) -> Option<&str> {
        match self.unknown() {
            Some(unknown) => unknown.parameters.get(index).map(String::as_str),
            None => self.descriptor().params.get(index).map(|p| p.name),
        }
    }

    /// The label an output shows. A placeholder has only its saved name.
    fn output_label(&self, index: usize) -> Option<&str> {
        match self.unknown() {
            Some(unknown) => unknown.outputs.get(index).map(String::as_str),
            None => self.descriptor().outputs.get(index).map(|p| p.label),
        }
    }

    /// The label a parameter shows. A placeholder has only its saved name.
    fn parameter_label(&self, index: usize) -> Option<&str> {
        match self.unknown() {
            Some(unknown) => unknown.parameters.get(index).map(String::as_str),
            None => self.descriptor().params.get(index).map(|p| p.label),
        }
    }

    /// The index of the input with this saved name.
    fn input_index(&self, name: &str) -> Option<usize> {
        match self.unknown() {
            Some(unknown) => unknown.inputs.iter().position(|port| port == name),
            None => self.descriptor().input_index(name),
        }
    }

    /// The index of the output with this saved name.
    fn output_index(&self, name: &str) -> Option<usize> {
        match self.unknown() {
            Some(unknown) => unknown.outputs.iter().position(|port| port == name),
            None => self.descriptor().output_index(name),
        }
    }

    /// The index of the parameter with this saved name.
    fn parameter_index(&self, name: &str) -> Option<usize> {
        match self.unknown() {
            Some(unknown) => unknown.parameters.iter().position(|param| param == name),
            None => self.descriptor().param_index(name),
        }
    }
}

#[cfg(test)]
mod tests;
