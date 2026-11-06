//! Structure: the modules that give a sub-patch its ports (MT-STRUCT-02).
//! Spec: `docs/design/sub-patches.md`.
//!
//! Inside a sub-patch definition, a Patch input becomes one input port of
//! the sub-patch, and a Patch output one output port, each named by the
//! module's label. Flattening removes them and runs the cables straight
//! through (`loom-patch`, Flattening rule 2), so they never reach the
//! engine from inside a sub-patch. At the top level of a patch, such as a
//! library file opened on its own, they are ordinary modules: the input
//! outputs silence, and the output reads its input and does nothing.

use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, PortDefault, PortIo, PortSpec, ProcessContext, SignalType,
};

/// `loom.struct.input` (MT-STRUCT-02): an input port of a sub-patch.
pub static INPUT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.struct.input",
    version: 1,
    name: "Patch Input",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "An input port of a sub-patch. Inside a sub-patch, cables into the port continue from here; at the top level it outputs silence.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The signal that arrives at the sub-patch's input port.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(PatchInput)),
    panel: Panel::EMPTY,
    displays: &[],
};

/// `loom.struct.output` (MT-STRUCT-02): an output port of a sub-patch.
pub static OUTPUT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.struct.output",
    version: 1,
    name: "Patch Output",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "An output port of a sub-patch. Inside a sub-patch, the cable into it continues out of the port; at the top level it does nothing.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal that leaves through the sub-patch's output port.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(PatchOutput)),
    panel: Panel::EMPTY,
    displays: &[],
};

/// A Patch input at the top level: silence.
struct PatchInput;

impl Module for PatchInput {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([], [out]) = io.split::<0, 1>();
        out.fill(0.0);
    }
}

/// A Patch output at the top level: a sink.
struct PatchOutput;

impl Module for PatchOutput {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([_], []) = io.split::<1, 0>();
    }
}
