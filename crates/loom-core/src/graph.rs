//! The editable patch graph, owned by the control thread.

use crate::descriptor::{ModuleDescriptor, ModuleRole, ParamRange, ParamSpec, Polarity};
use crate::midi_map::{MidiControl, MidiMapping, MidiMappingId};
use std::collections::BTreeMap;
use std::fmt;

/// Patch data for a module this build cannot identify.
///
/// Port and parameter names are owned on the control thread. The audio
/// thread sees only the compiled buffers and a silent module instance.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownModule {
    /// Original versioned module kind.
    pub kind: String,
    /// Original optional user label.
    pub label: Option<String>,
    /// Original parameter values, preserved even when they are defaults.
    pub params: BTreeMap<String, f32>,
    /// Original selected ranges, preserved without interpretation.
    pub ranges: BTreeMap<String, (f32, f32)>,
    /// Inputs named by cables, in first-seen order.
    pub inputs: Vec<String>,
    /// Outputs named by cables or assignments, in first-seen order.
    pub outputs: Vec<String>,
    /// Parameter names referenced by values, assignments, or MIDI mappings.
    pub parameters: Vec<String>,
}

impl UnknownModule {
    /// The shared execution descriptor for a placeholder.
    pub fn descriptor() -> &'static ModuleDescriptor {
        &UNKNOWN_DESCRIPTOR
    }
}

/// Silent engine instance shared by all unknown module kinds.
struct UnknownDsp;

impl crate::Module for UnknownDsp {
    fn process(&mut self, _ctx: &crate::ProcessContext<'_>, io: &mut crate::PortIo<'_>) {
        for index in 0..io.output_count() {
            io.output(index).fill(0.0);
        }
    }
}

/// Creates the stateless audio-thread half of a placeholder.
fn unknown_factory(_: &crate::ModuleConfig) -> Box<dyn crate::Module> {
    Box::new(UnknownDsp)
}

/// The shared execution descriptor; original names live in [`UnknownModule`].
pub(crate) static UNKNOWN_DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
    id: "loom.internal.unknown",
    version: 1,
    name: "Unknown module",
    category: "Unknown",
    description: "This version does not have this module.",
    inputs: &[],
    outputs: &[],
    params: &[],
    role: ModuleRole::Dsp(unknown_factory),
    panel: crate::Panel::EMPTY,
    displays: &[],
};

/// Stable identifier of a module in a patch. Saved in patch files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(pub u32);

impl ModuleId {
    /// The largest ID a module may have. `u32::MAX` is kept to mean "no
    /// module" in the display table, and the next free ID must still fit
    /// after the largest one (#44).
    pub const MAX: ModuleId = ModuleId(u32::MAX - 1);
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// An output port: module plus output index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputRef {
    /// The module.
    pub module: ModuleId,
    /// Output index.
    pub port: usize,
}

/// An input port: module plus input index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputRef {
    /// The module.
    pub module: ModuleId,
    /// Input index.
    pub port: usize,
}

/// Stable identifier of one modulation assignment. Saved only for the
/// session: patch files identify an assignment by its source and target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssignmentId(pub u32);

impl fmt::Display for AssignmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One modulation assignment: an output moving a parameter, without a cable.
///
/// Spec: `docs/design/modulation.md`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Assignment {
    /// The output that drives the parameter.
    pub source: OutputRef,
    /// The module whose parameter is moved.
    pub target: ModuleId,
    /// Parameter index on [`Self::target`].
    pub param: usize,
    /// Share of the control's full range, from -1 to 1.
    pub depth: f32,
    /// Forces a polarity. `None` follows the source output.
    pub polarity: Option<Polarity>,
    /// False skips this assignment without removing the scheduling edge.
    pub enabled: bool,
}

/// A module in the graph.
#[derive(Debug)]
pub struct Node {
    /// The module type.
    pub descriptor: &'static ModuleDescriptor,
    /// Original data for an unknown module, if this node is a placeholder.
    pub unknown: Option<UnknownModule>,
    /// Current parameter values, one per [`ModuleDescriptor::params`].
    pub params: Vec<f32>,
    /// Engine slot holding the instance. `None` for engine-handled roles.
    pub(crate) slot: Option<usize>,
    /// The selected control range of each parameter, or `None` for one that
    /// offers no ranges (`docs/design/control-ranges.md`).
    pub(crate) ranges: Vec<Option<ParamRange>>,
}

impl Node {
    /// A node with default parameter values and default ranges.
    pub(crate) fn new(descriptor: &'static ModuleDescriptor, slot: Option<usize>) -> Self {
        Self {
            descriptor,
            unknown: None,
            params: descriptor.default_params(),
            slot,
            ranges: descriptor.params.iter().map(ParamSpec::default_range).collect(),
        }
    }

    /// Number of named input ports, including placeholder ports.
    pub fn input_count(&self) -> usize {
        self.unknown
            .as_ref()
            .map_or(self.descriptor.inputs.len(), |unknown| unknown.inputs.len())
    }

    /// Number of named output ports, including placeholder ports.
    pub fn output_count(&self) -> usize {
        self.unknown
            .as_ref()
            .map_or(self.descriptor.outputs.len(), |unknown| unknown.outputs.len())
    }

    /// The saved name of an input, including a placeholder input.
    pub fn input_name(&self, index: usize) -> Option<&str> {
        self.unknown.as_ref().map_or_else(
            || self.descriptor.inputs.get(index).map(|p| p.name),
            |unknown| unknown.inputs.get(index).map(String::as_str),
        )
    }

    /// The saved name of an output, including a placeholder output.
    pub fn output_name(&self, index: usize) -> Option<&str> {
        self.unknown.as_ref().map_or_else(
            || self.descriptor.outputs.get(index).map(|p| p.name),
            |unknown| unknown.outputs.get(index).map(String::as_str),
        )
    }

    /// The saved name of a parameter, including a placeholder parameter.
    pub fn parameter_name(&self, index: usize) -> Option<&str> {
        self.unknown.as_ref().map_or_else(
            || self.descriptor.params.get(index).map(|p| p.name),
            |unknown| unknown.parameters.get(index).map(String::as_str),
        )
    }

    /// The selected control range of parameter `index`, if it offers ranges.
    pub fn range(&self, index: usize) -> Option<ParamRange> {
        self.ranges.get(index).copied().flatten()
    }

    /// Parameter `index` as its control maps it: limited to the selected
    /// range, if it has one (control-ranges.md, rule 4). `None` if there is
    /// no such parameter.
    pub fn spec(&self, index: usize) -> Option<ParamSpec> {
        let spec = self.descriptor.params.get(index)?;
        Some(match self.range(index) {
            Some(range) => spec.in_range(range),
            None => *spec,
        })
    }

    /// True for modules the engine runs itself (unit delay, audio output).
    pub fn is_intrinsic(&self) -> bool {
        !matches!(self.descriptor.role, ModuleRole::Dsp(_))
    }
}

/// Modules, cables, modulation assignments, and MIDI mappings. Each input has
/// at most one cable.
#[derive(Debug, Default)]
pub struct Graph {
    pub(crate) nodes: BTreeMap<ModuleId, Node>,
    /// Cables, keyed by the input they feed.
    pub(crate) cables: BTreeMap<InputRef, OutputRef>,
    /// Modulation assignments, in id order. That order is the order their
    /// depths are added.
    pub(crate) assignments: BTreeMap<AssignmentId, Assignment>,
    /// MIDI mappings. A parameter has at most one, and so does a control.
    pub(crate) midi_mappings: BTreeMap<MidiMappingId, MidiMapping>,
}

impl Graph {
    /// All modules, sorted by ID.
    pub fn nodes(&self) -> impl Iterator<Item = (ModuleId, &Node)> {
        self.nodes.iter().map(|(id, node)| (*id, node))
    }

    /// One module.
    pub fn node(&self, id: ModuleId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// All cables as `(from, to)`, sorted by input.
    pub fn cables(&self) -> impl Iterator<Item = (OutputRef, InputRef)> + '_ {
        self.cables.iter().map(|(to, from)| (*from, *to))
    }

    /// The cable feeding an input, if any.
    pub fn source_of(&self, input: InputRef) -> Option<OutputRef> {
        self.cables.get(&input).copied()
    }

    /// All modulation assignments, sorted by id.
    pub fn assignments(&self) -> impl Iterator<Item = (AssignmentId, &Assignment)> {
        self.assignments.iter().map(|(id, assignment)| (*id, assignment))
    }

    /// One modulation assignment.
    pub fn assignment(&self, id: AssignmentId) -> Option<&Assignment> {
        self.assignments.get(&id)
    }

    /// All MIDI mappings, sorted by id.
    pub fn midi_mappings(&self) -> impl Iterator<Item = (MidiMappingId, &MidiMapping)> {
        self.midi_mappings.iter().map(|(id, mapping)| (*id, mapping))
    }

    /// The MIDI mapping on a parameter, if any.
    pub fn midi_mapping_of(&self, module: ModuleId, param: usize) -> Option<(MidiMappingId, &MidiMapping)> {
        self.midi_mappings()
            .find(|(_, mapping)| mapping.target == module && mapping.param == param)
    }

    /// The MIDI mapping on a hardware control, if any.
    pub fn midi_mapping_on(&self, control: MidiControl) -> Option<(MidiMappingId, &MidiMapping)> {
        self.midi_mappings().find(|(_, mapping)| mapping.control == control)
    }

    /// Number of modules.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True when there are no modules.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
