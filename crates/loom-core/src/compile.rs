//! Turns a [`Graph`] into an execution [`Schedule`] (ADR-0002).
//!
//! This runs on the control thread and may allocate freely.
//!
//! `docs/learn/feedback-loops.md` explains why loops need a one-sample delay
//! and run one sample at a time.
//!
//! Steps:
//! 1. Give every output port its own buffer.
//! 2. Find strongly connected components (SCCs) with Tarjan's algorithm.
//!    An SCC with more than one module, or a module wired to itself, is a
//!    feedback loop.
//! 3. Order the SCCs topologically. Modules outside loops run a whole block
//!    at a time. Each loop runs one sample at a time.
//! 4. Inside a loop, user-placed unit delays break the cycle. If cycles
//!    remain, the compiler inserts its own one-sample delays and reports
//!    where, so the UI can show them. A modulation assignment is broken
//!    before a cable, so a modulation loop does not delay the audio path.
//! 5. Give each modulated parameter a buffer, filled from its base value
//!    and its sources just before the target module runs.
//! 6. Bind unconnected inputs to their default (ADR-0015): the zero buffer,
//!    a constant buffer per distinct value, or one shared buffer per
//!    transport signal that the engine fills at the start of each block.
//! 7. Infer stereo widths (ADR-0020, `width.rs`) and allocate right-channel
//!    buffers, module operations and delay taps. Downmix where a mono input
//!    receives stereo.
//!
//! Before step 3, the compiler marks the modules that can reach something
//! the user hears or sees (FR-GRAPH-09, see [`reaches_a_root`]). The others
//! are left out of the schedule. Their instances stay in their engine
//! slots, so a cable that connects them again resumes their state.
//!
//! [`compile`] runs the steps in order. Step 1 is [`Layout::new`], steps 3
//! and 4 are in `steps.rs`, steps 5 and 6 in `inputs.rs`, and `build.rs`
//! allocates the buffers and resolves every index into a pointer. Steps work
//! with buffer indices, so only the last one touches memory.

use crate::descriptor::{Channels, ModuleRole, ParamSpec, Polarity, PortDefault};
use crate::graph::{AssignmentId, Graph, InputRef, ModuleId, Node, OutputRef};
use crate::midi_map::{MidiMapOp, MidiMappingId};
use crate::module::RightPtr;
use crate::signal::SignalType;
use crate::telemetry::{ProbeId, ProbeKind};
use crate::width::{self, Width, Widths};
use loom_rt::Smoothed;
use std::collections::{BTreeMap, BTreeSet};

mod build;
mod inputs;
mod midi_maps;
mod order;
mod schedule;
mod steps;

use build::Builder;
use inputs::{CompiledAssignment, DefaultKey, DownmixSpec, Inputs, compile_assignments};
use midi_maps::compile_midi_maps;
pub(crate) use order::partner;
use order::{reaches_a_root, tarjan};
pub(crate) use schedule::{
    AssignmentRuntime, DownmixKind, DownmixOp, ModSource, ModuleOp, OutputOp, ParamFill, ProbeOp, Schedule, Step, Tap,
    TransportBuffers,
};
use steps::{Plan, StepSpec, successors};

/// Marks tap keys created by the compiler, so they never clash with user
/// unit delay keys (which are module IDs).
const AUTO_TAP: u64 = 1 << 63;

/// [`Tap::source`] of a delay with nothing plugged into it.
pub(crate) const NO_SOURCE: u64 = u64::MAX;

/// A key for an output port, unique in a graph and sorted by module, then
/// port.
fn output_key(output: OutputRef) -> u64 {
    (u64::from(output.module.0) << 16) | (output.port as u64 & 0xFFFF)
}

/// Marks a tap inserted on a modulation assignment. Bit 62 keeps these clear
/// of cable taps, whose keys use the module id shifted by 16 under `AUTO_TAP`.
const ASSIGN_TAP: u64 = AUTO_TAP | (1 << 62);

/// Marks the right-channel tap of a stereo delay. Bit 61 is clear of user
/// keys (module IDs), cable taps (module id shifted by 16), and assignment
/// taps (assignment IDs).
const RIGHT_TAP: u64 = 1 << 61;

/// Marks the fill of a module's right copy. Bit 31 sits between the module
/// id (bits 32 and up) and the parameter index (16 bits).
const RIGHT_FILL: u64 = 1 << 31;

/// A feedback loop found by the compiler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackLoop {
    /// Every module in the loop, sorted by ID.
    pub modules: Vec<ModuleId>,
    /// Cables where the compiler inserted a one-sample delay. Empty when the
    /// loop already contains a user unit delay in every cycle.
    pub inserted_delays: Vec<(OutputRef, InputRef)>,
    /// Modulation assignments where the compiler inserted a one-sample delay.
    pub inserted_assignment_delays: Vec<AssignmentId>,
}

/// What the compiler found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompileReport {
    /// Number of modules in the graph, including skipped ones.
    pub modules: usize,
    /// Modules left out of the schedule, because no path leads from them to
    /// something the user hears or sees (FR-GRAPH-09). Sorted by ID.
    pub skipped: BTreeSet<ModuleId>,
    /// Feedback loops, each run one sample at a time.
    pub loops: Vec<FeedbackLoop>,
    /// Modulated parameters with no live value, because the table set by
    /// [`EngineConfig::max_live_values`](crate::EngineConfig::max_live_values)
    /// is full. They sound right, but their knobs show no live dot.
    pub without_live_value: usize,
    /// Modules that run as two copies, one per channel (ADR-0020).
    pub per_channel: BTreeSet<ModuleId>,
    /// Outputs that carry stereo. Every other output is mono.
    pub stereo_outputs: BTreeSet<OutputRef>,
}

impl CompileReport {
    /// How many channels an output carries, for drawing its cables and for
    /// hover text (stereo spec, rules 19 and 23).
    pub fn width(&self, output: OutputRef) -> Width {
        if self.stereo_outputs.contains(&output) {
            Width::Stereo
        } else {
            Width::Mono
        }
    }

    /// True when the engine runs `module` as two copies, one per channel.
    pub fn runs_per_channel(&self, module: ModuleId) -> bool {
        self.per_channel.contains(&module)
    }

    /// All delays the compiler inserted, across all loops.
    pub fn inserted_delays(&self) -> impl Iterator<Item = &(OutputRef, InputRef)> {
        self.loops.iter().flat_map(|l| l.inserted_delays.iter())
    }

    /// All assignment delays the compiler inserted, across all loops.
    pub fn inserted_assignment_delays(&self) -> impl Iterator<Item = &AssignmentId> {
        self.loops.iter().flat_map(|l| l.inserted_assignment_delays.iter())
    }
}

/// The bounds of fill `j` in `old`'s fill order.
fn previous_spec(old: &Schedule, j: usize) -> Option<(f32, f32)> {
    let fill = &old.fills[*old.fill_order.get(j)? as usize];
    Some((fill.spec.min, fill.spec.max))
}

/// [`RIGHT_TAP`] for the right channel, nothing for the left.
fn right_tap(ch: usize) -> u64 {
    if ch == 1 { RIGHT_TAP } else { 0 }
}

/// Hands out buffer indices in order. The last step allocates them all.
#[derive(Default)]
struct Buffers(usize);

impl Buffers {
    /// The index of a new buffer.
    fn add(&mut self) -> usize {
        self.0 += 1;
        self.0 - 1
    }
}

/// The graph's modules in ID order, with their widths and output buffers,
/// which every step reads. The steps name a module by its index `v` here.
struct Layout<'a> {
    graph: &'a Graph,
    ids: Vec<ModuleId>,
    index_of: BTreeMap<ModuleId, usize>,
    nodes: Vec<&'a Node>,
    widths: Widths,
    /// True for each module that runs as two copies, one per channel.
    copied: Vec<bool>,
    /// The buffer of each output port.
    out_buf: Vec<Vec<usize>>,
    /// The right-channel buffer of each stereo output port.
    right_buf: Vec<Vec<Option<usize>>>,
}

impl<'a> Layout<'a> {
    /// Step 1: one buffer per output port, and a second buffer for the right
    /// channel of every stereo output.
    fn new(graph: &'a Graph, buffers: &mut Buffers) -> Self {
        let ids: Vec<ModuleId> = graph.nodes.keys().copied().collect();
        let index_of = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let nodes: Vec<&Node> = graph.nodes.values().collect();
        // Stereo widths (ADR-0020). A copied DSP module runs a second
        // operation on its copy's slot, which the controller allocated.
        let widths = width::infer(graph);
        let copied = ids.iter().map(|id| widths.per_channel.contains(id)).collect();
        let out_buf = nodes
            .iter()
            .map(|node| (0..node.output_count()).map(|_| buffers.add()).collect())
            .collect();
        let right_buf = ids
            .iter()
            .zip(&nodes)
            .map(|(&module, node)| {
                (0..node.output_count())
                    .map(|port| {
                        widths
                            .stereo_outputs
                            .contains(&OutputRef { module, port })
                            .then(|| buffers.add())
                    })
                    .collect()
            })
            .collect();
        Self {
            graph,
            ids,
            index_of,
            nodes,
            widths,
            copied,
            out_buf,
            right_buf,
        }
    }

    /// The index of a module in the graph.
    fn index(&self, module: ModuleId) -> usize {
        self.index_of[&module]
    }

    /// True when the module at `v` is a DSP module, which runs as an operation.
    fn is_dsp(&self, v: usize) -> bool {
        matches!(self.nodes[v].descriptor.role, ModuleRole::Dsp(_))
    }

    /// True when `from` carries stereo.
    fn is_stereo(&self, from: OutputRef) -> bool {
        self.widths.stereo_outputs.contains(&from)
    }

    /// How many copies of the module at `v` run.
    fn channels(&self, v: usize) -> usize {
        if self.copied[v] { 2 } else { 1 }
    }

    /// How many channels `from` carries.
    fn output_channels(&self, from: OutputRef) -> usize {
        if self.is_stereo(from) { 2 } else { 1 }
    }

    /// The buffer that channel `ch` of a cable from `from` carries. A mono
    /// output carries the same buffer on both channels.
    fn channel_buffer(&self, from: OutputRef, ch: usize) -> usize {
        let v = self.index(from.module);
        self.right_buf[v][from.port]
            .filter(|_| ch == 1)
            .unwrap_or(self.out_buf[v][from.port])
    }

    /// What `input` reads when no cable is plugged into it.
    fn default_of(&self, input: InputRef) -> PortDefault {
        self.nodes[self.index(input.module)]
            .descriptor
            .inputs
            .get(input.port)
            .map_or(PortDefault::Value(0.0), |port| port.default)
    }
}

/// Compiles the graph. `max_block` is the buffer length. `probes` are output
/// ports to read for telemetry; probes on unknown modules or ports are
/// skipped. `sample_rate` sets the base-value smoother for modulated parameters.
pub(crate) fn compile(
    graph: &Graph,
    probes: &BTreeMap<ProbeId, (OutputRef, ProbeKind)>,
    midi_synced: &BTreeMap<MidiMappingId, u32>,
    sample_rate: f32,
    max_block: usize,
) -> (Schedule, CompileReport) {
    let mut buffers = Buffers::default();
    let layout = Layout::new(graph, &mut buffers);
    let compiled = compile_assignments(&layout);

    // 2. SCCs. Cables and assignments are both edges.
    let (succ, self_loop) = successors(&layout, &compiled);
    let mut sccs = tarjan(&succ);
    // Tarjan emits SCCs in reverse topological order.
    sccs.reverse();
    let probed: BTreeSet<ModuleId> = probes.values().map(|(from, _)| from.module).collect();
    let modulated: BTreeSet<usize> = compiled.iter().map(|a| a.target_node).collect();
    let live = reaches_a_root(&layout.nodes, &succ, |v| {
        probed.contains(&layout.ids[v]) || modulated.contains(&v)
    });

    // 3 and 4. Steps, and the delays that break feedback loops.
    let mut plan = Plan::new(&layout, &compiled, &sccs, &self_loop, &live, &mut buffers);
    // 5 and 6. Fills, downmixes, and defaults.
    let inputs = Inputs::new(&layout, &compiled, &plan, &live, &mut buffers);

    let report = CompileReport {
        modules: layout.ids.len(),
        skipped: (0..layout.ids.len())
            .filter(|&v| !live[v])
            .map(|v| layout.ids[v])
            .collect(),
        loops: std::mem::take(&mut plan.loops),
        without_live_value: 0,
        per_channel: layout.widths.per_channel.clone(),
        stereo_outputs: layout.widths.stereo_outputs.clone(),
    };
    let schedule =
        Builder::new(&layout, &plan, &inputs, buffers, max_block, sample_rate).finish(&compiled, probes, midi_synced);
    (schedule, report)
}

#[cfg(test)]
mod tests;
