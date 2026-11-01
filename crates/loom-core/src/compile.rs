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

use crate::descriptor::{Channels, ModuleRole, ParamSpec, Polarity, PortDefault};
use crate::graph::{AssignmentId, Graph, InputRef, ModuleId, OutputRef};
use crate::midi_map::{MidiMapOp, MidiMappingId};
use crate::module::RightPtr;
use crate::signal::SignalType;
use crate::telemetry::{ProbeId, ProbeKind};
use crate::width::{self, Width};
use loom_rt::Smoothed;
use std::collections::{BTreeMap, BTreeSet};

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

pub(crate) struct ModuleOp {
    pub slot: u32,
    pub inputs: (u32, u32),
    pub outputs: (u32, u32),
    pub connected: u64,
    /// Range into [`Schedule::fills`].
    pub fills: (u32, u32),
    /// Range into [`Schedule::param_ptrs`], one entry per modulated parameter.
    pub params: (u32, u32),
    /// Range into [`Schedule::downmixes`], filled before the module runs.
    pub downmixes: (u32, u32),
    /// Range into [`Schedule::right_ptrs`], for `Stereo` ports.
    pub right: (u32, u32),
    /// 0 for a module that runs as one, or its left copy; 1 for its right.
    pub channel: u8,
}

/// How a downmix makes one channel from two (stereo spec, rule 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DownmixKind {
    /// `(left + right) / 2`, for audio, modulation, and pitch.
    Mean,
    /// `max(left, right)`, for gates: high when either side is high.
    Max,
    /// The mean the shorter way round the circle, for phases.
    Phase,
}

impl DownmixKind {
    /// The method the input's signal type picks.
    pub fn of(signal: SignalType) -> Self {
        match signal {
            SignalType::Gate => Self::Max,
            SignalType::Phase => Self::Phase,
            SignalType::Audio | SignalType::Modulation | SignalType::Pitch => Self::Mean,
        }
    }

    /// One sample of the downmix.
    #[inline]
    pub fn apply(self, left: f32, right: f32) -> f32 {
        match self {
            Self::Mean => (left + right) * 0.5,
            Self::Max => left.max(right),
            Self::Phase => {
                // The difference the short way round, from -0.5 (included)
                // up to 0.5 (not included). Exactly half a cycle apart, it
                // is -0.5, so the tie always goes the same way.
                let difference = right - left;
                let difference = difference - (difference + 0.5).floor();
                let mean = left + difference * 0.5;
                mean - mean.floor()
            }
        }
    }
}

/// Fills `output` from two channels just before a module runs.
pub(crate) struct DownmixOp {
    pub kind: DownmixKind,
    pub left: *const f32,
    pub right: *const f32,
    pub output: *mut f32,
}

/// One source of a modulated parameter. `state` indexes [`Schedule::assignments`].
pub(crate) struct ModSource {
    pub state: u32,
    pub buffer: *const f32,
}

/// A parameter the engine fills before its module runs.
pub(crate) struct ParamFill {
    /// `(module id << 32) | param index`. Matches smoother state across swaps.
    pub key: u64,
    pub slot: u32,
    pub param: u16,
    /// The parameter limited to its selected range, so modulation maps and
    /// clamps inside it (control-ranges.md, rules 4, 5, and 8).
    pub spec: ParamSpec,
    pub base: f32,
    pub smooth: Option<Smoothed>,
    pub buffer: *mut f32,
    pub sources: Vec<ModSource>,
    /// Cell in the live value table, or [`crate::live::NO_CELL`]. Set by
    /// the controller after compiling.
    pub live: u32,
}

impl ParamFill {
    /// True for the fill of a module's right copy. Its live value is not
    /// shown: the knob shows copy 0 (stereo spec, rule 16).
    pub fn is_right_copy(&self) -> bool {
        self.key & RIGHT_FILL != 0
    }
}

/// Depth, polarity, and enabled flag. The audio thread updates these without
/// recompiling. Sorted by [`AssignmentRuntime::id`].
pub(crate) struct AssignmentRuntime {
    pub id: AssignmentId,
    pub depth: f32,
    pub enabled: bool,
    pub polarity: Option<Polarity>,
    pub source_polarity: Polarity,
    /// A missing source must remain inert even under a polarity override.
    pub silent_source: bool,
}

pub(crate) struct Tap {
    pub key: u64,
    /// [`output_key`] of the output this delay reads, or [`NO_SOURCE`].
    /// A new delay starts with that output's last value (live patching
    /// spec, rule 5).
    pub source: u64,
    pub input: *const f32,
    pub output: *mut f32,
    pub state: f32,
}

pub(crate) struct OutputOp {
    pub left: *const f32,
    pub right: *const f32,
}

/// A probed output buffer. Read after each block for telemetry.
pub(crate) struct ProbeOp {
    pub id: ProbeId,
    pub buffer: *const f32,
    pub right: Option<*const f32>,
    pub kind: ProbeKind,
}

/// Buffers for transport signals that at least one unconnected input reads.
/// `None` when nothing reads that signal, so it costs nothing.
#[derive(Default)]
pub(crate) struct TransportBuffers {
    pub bar_phase: Option<*mut f32>,
    pub beat_phase: Option<*mut f32>,
    pub playing: Option<*mut f32>,
    /// Engine slot of the Transport module, if the patch has one.
    pub module: Option<u32>,
    /// True when the Transport's `run` or `reset` input has a cable: the
    /// engine then splits blocks on the transport grid (rule 11).
    pub grid: bool,
    /// True when the Transport's `run` input has a cable, so a fresh
    /// Transport waits for the gate instead of sending its Run parameter.
    pub run_cable: bool,
}

/// Identifies one shared default buffer. Value defaults are keyed by their
/// bits, so each distinct value gets one buffer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DefaultKey {
    Value(u32),
    BarPhase,
    BeatPhase,
    Playing,
}

impl DefaultKey {
    /// `None` for defaults that read the shared zero buffer. The pattern
    /// compares with `==`, so -0.0 reads it too.
    fn of(default: PortDefault) -> Option<Self> {
        match default {
            PortDefault::Value(0.0) => None,
            PortDefault::Value(value) => Some(Self::Value(value.to_bits())),
            PortDefault::BarPhase => Some(Self::BarPhase),
            PortDefault::BeatPhase => Some(Self::BeatPhase),
            PortDefault::Playing => Some(Self::Playing),
        }
    }
}

pub(crate) enum Step {
    /// Run a module over the whole block.
    Module(u32),
    /// Run a unit delay over the whole block.
    Tap(u32),
    /// Run a feedback loop one sample at a time.
    Loop { ops: (u32, u32), taps: (u32, u32) },
}

/// A compiled, ready-to-run graph. Built on the control thread, then moved to
/// the audio thread whole.
pub(crate) struct Schedule {
    // Owned buffer memory. The raw pointers below point into these boxes.
    // Moving the `Schedule` moves the boxes, not the heap data, so the
    // pointers stay valid.
    _buffers: Vec<Box<[f32]>>,
    _zero: Box<[f32]>,
    pub input_ptrs: Vec<*const f32>,
    pub output_ptrs: Vec<*mut f32>,
    pub ops: Vec<ModuleOp>,
    /// Sorted by key, so tap state can be carried across schedule swaps.
    pub taps: Vec<Tap>,
    /// Every output port's buffer, sorted by [`output_key`]. The next
    /// schedule reads it at the swap to start its new delays.
    pub sources: Vec<(u64, *const f32)>,
    pub loop_ops: Vec<u32>,
    pub loop_taps: Vec<u32>,
    pub steps: Vec<Step>,
    pub outputs: Vec<OutputOp>,
    pub probes: Vec<ProbeOp>,
    pub transport: TransportBuffers,
    pub fills: Vec<ParamFill>,
    /// Indices into `fills`, sorted by fill key. Built on the control thread
    /// so [`Schedule::take_smooth_state_from`] can merge two sorted lists.
    pub fill_order: Vec<u32>,
    pub assignments: Vec<AssignmentRuntime>,
    pub param_ptrs: Vec<(u16, *const f32)>,
    /// Downmixes, run by the module operation that owns them, or before the
    /// outputs are summed (`output_downmixes`).
    pub downmixes: Vec<DownmixOp>,
    pub output_downmixes: (u32, u32),
    pub right_ptrs: Vec<RightPtr>,
    /// `(left slot, right slot)` of every module that runs as two copies,
    /// sorted by left slot. A parameter change or reset on the left slot
    /// also goes to the right one.
    pub copies: Vec<(u32, u32)>,
    /// MIDI mappings, sorted by control.
    pub midi_maps: Vec<MidiMapOp>,
    /// Live value cells the previous schedule used and this one does not.
    /// The engine releases them when it swaps this schedule in.
    pub live_release: Vec<u32>,
}

// SAFETY: the raw pointers only point into buffers owned by this schedule.
// The schedule is only ever used by one thread at a time (built on the control
// thread, then moved to the audio thread, then moved back for dropping).
unsafe impl Send for Schedule {}

impl Schedule {
    pub fn empty() -> Self {
        Self {
            _buffers: Vec::new(),
            _zero: Box::new([]),
            input_ptrs: Vec::new(),
            output_ptrs: Vec::new(),
            ops: Vec::new(),
            taps: Vec::new(),
            sources: Vec::new(),
            loop_ops: Vec::new(),
            loop_taps: Vec::new(),
            steps: Vec::new(),
            outputs: Vec::new(),
            probes: Vec::new(),
            transport: TransportBuffers::default(),
            fills: Vec::new(),
            fill_order: Vec::new(),
            assignments: Vec::new(),
            param_ptrs: Vec::new(),
            downmixes: Vec::new(),
            output_downmixes: (0, 0),
            right_ptrs: Vec::new(),
            copies: Vec::new(),
            midi_maps: Vec::new(),
            live_release: Vec::new(),
        }
    }

    /// Copies unit delay state from the previous schedule, matched by key.
    /// A delay with no state to carry starts with the last value its source
    /// output wrote in the old schedule's last block of `frames` frames, or
    /// 0 if that output did not run (live patching spec, rule 5). Then the
    /// delay holds what it would hold had it always been there.
    /// Runs on the audio thread: no allocation, linear time plus a binary
    /// search per new delay.
    pub fn take_tap_state_from(&mut self, old: &Schedule, frames: usize) {
        let mut j = 0;
        for tap in &mut self.taps {
            while j < old.taps.len() && old.taps[j].key < tap.key {
                j += 1;
            }
            if j < old.taps.len() && old.taps[j].key == tap.key {
                tap.state = old.taps[j].state;
            } else if tap.key & RIGHT_TAP != 0
                && let Ok(i) = old
                    .taps
                    .binary_search_by_key(&(tap.key & !RIGHT_TAP), |old_tap| old_tap.key)
            {
                // A new right channel inherits the running mono delay.
                tap.state = old.taps[i].state;
            } else if let Some(last) = frames.checked_sub(1)
                && tap.source != NO_SOURCE
                && let Some(i) = old
                    .sources
                    .binary_search_by_key(&tap.source, |&(key, _)| key)
                    .ok()
                    .or_else(|| {
                        (tap.source & RIGHT_TAP != 0)
                            .then(|| {
                                old.sources
                                    .binary_search_by_key(&(tap.source & !RIGHT_TAP), |&(key, _)| key)
                                    .ok()
                            })
                            .flatten()
                    })
            {
                // SAFETY: every buffer in `sources` is `max_block` floats
                // long and owned by `old`, which is alive here. The last
                // block filled its first `frames <= max_block` floats.
                tap.state = unsafe { *old.sources[i].1.add(last) };
            }
        }
    }

    /// Copies in-flight base smoothing onto the new schedule, matched by key.
    /// A fill that is new, because its parameter just got its first
    /// assignment, starts from `glide(slot, param)` instead: the value the
    /// module itself was gliding through, so the parameter does not step.
    /// Fills sit in execution order, which is not key order, so both
    /// schedules keep a `fill_order` index sorted by key. Walking the two
    /// indices together matches every key in one pass, like
    /// [`Schedule::take_tap_state_from`]. The order matters: a merge only
    /// works if both lists are sorted by the same key.
    /// Runs on the audio thread: no allocation, linear time.
    pub fn take_smooth_state_from(&mut self, old: &Schedule, glide: impl Fn(usize, usize) -> Option<f32>) {
        let mut j = 0;
        for &i in &self.fill_order {
            let fill = &mut self.fills[i as usize];
            while j < old.fill_order.len() && old.fills[old.fill_order[j] as usize].key < fill.key {
                j += 1;
            }
            let Some(smooth) = fill.smooth.as_mut() else {
                continue;
            };
            let previous = old
                .fill_order
                .get(j)
                .map(|&k| &old.fills[k as usize])
                .filter(|previous| previous.key == fill.key)
                .map(|previous| previous.smooth);
            match previous {
                // The range changed: the glide goes on from the value it had
                // reached, at that value's place in the new range, so the
                // sound does not jump and no value is passed that was never
                // set (control-ranges.md).
                Some(Some(mut previous)) if previous_spec(old, j) != Some((fill.spec.min, fill.spec.max)) => {
                    let old_spec = old.fills[old.fill_order[j] as usize].spec;
                    let value = old_spec.denormalize(previous.current());
                    previous.reset(fill.spec.normalize(value));
                    previous.set_target(fill.spec.normalize(fill.base));
                    *smooth = previous;
                }
                Some(Some(previous)) => *smooth = previous,
                Some(None) => {}
                None => {
                    if let Some(value) = glide(fill.slot as usize, usize::from(fill.param)) {
                        smooth.reset(fill.spec.normalize(value));
                        smooth.set_target(fill.spec.normalize(fill.base));
                    }
                }
            }
        }
    }
}

/// The bounds of fill `j` in `old`'s fill order.
fn previous_spec(old: &Schedule, j: usize) -> Option<(f32, f32)> {
    let fill = &old.fills[*old.fill_order.get(j)? as usize];
    Some((fill.spec.min, fill.spec.max))
}

struct TapSpec {
    key: u64,
    source: Option<OutputRef>,
    channel: usize,
    input: Option<usize>,
    output: usize,
}

enum StepSpec {
    Module(usize),
    Tap(usize),
    Loop { modules: Vec<usize>, taps: Vec<usize> },
}

/// An edge inside a feedback loop. Assignment edges are broken before cables.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Dep {
    Cable(InputRef),
    Assignment(AssignmentId),
}

struct CompiledAssignment {
    id: AssignmentId,
    source: OutputRef,
    target_node: usize,
    param: usize,
    depth: f32,
    enabled: bool,
    polarity: Option<Polarity>,
    source_polarity: Polarity,
    /// The source is an unknown placeholder.
    silent_source: bool,
}

struct SourceSpec {
    /// Index into the compiled assignment list.
    assignment: usize,
    /// Buffer index the fill reads. A delay tap when the assignment was broken.
    buffer: usize,
}

struct FillSpec {
    param: usize,
    buffer: usize,
    sources: Vec<SourceSpec>,
}

struct DownmixSpec {
    kind: DownmixKind,
    left: usize,
    right: usize,
    output: usize,
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
    let ids: Vec<ModuleId> = graph.nodes.keys().copied().collect();
    let index_of: BTreeMap<ModuleId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let nodes: Vec<&crate::graph::Node> = graph.nodes.values().collect();
    let n = ids.len();

    // Stereo widths (ADR-0020). A copied DSP module runs a second operation
    // on its copy's slot, which the controller allocated.
    let widths = width::infer(graph);
    let is_stereo = |from: OutputRef| widths.stereo_outputs.contains(&from);
    let copied: Vec<bool> = ids.iter().map(|id| widths.per_channel.contains(id)).collect();
    let channels = |v: usize| if copied[v] { 2 } else { 1 };

    // 1. One buffer per output port.
    let mut buffer_count = 0usize;
    let out_buf: Vec<Vec<usize>> = nodes
        .iter()
        .map(|node| {
            (0..node.output_count())
                .map(|_| {
                    buffer_count += 1;
                    buffer_count - 1
                })
                .collect()
        })
        .collect();
    // A second buffer for the right channel of every stereo output.
    let right_buf: Vec<Vec<Option<usize>>> = (0..n)
        .map(|v| {
            (0..nodes[v].output_count())
                .map(|port| {
                    is_stereo(OutputRef { module: ids[v], port }).then(|| {
                        buffer_count += 1;
                        buffer_count - 1
                    })
                })
                .collect()
        })
        .collect();
    let source_buffer = |from: OutputRef| out_buf[index_of[&from.module]][from.port];
    // The buffer that channel `ch` of a cable from `from` carries. A mono
    // output carries the same buffer on both channels.
    let channel_buffer = |from: OutputRef, ch: usize| {
        right_buf[index_of[&from.module]][from.port]
            .filter(|_| ch == 1)
            .unwrap_or_else(|| source_buffer(from))
    };

    // Assignments the graph can actually schedule. Dangling ones are dropped.
    let mut compiled = Vec::new();
    for (id, assignment) in graph.assignments() {
        let Some(&target_node) = index_of.get(&assignment.target) else {
            continue;
        };
        let Some(&source_node) = index_of.get(&assignment.source.module) else {
            continue;
        };
        let outputs = nodes[source_node].descriptor.outputs;
        let params = nodes[target_node].descriptor.params;
        let target = nodes[target_node];
        let param_count = target
            .unknown
            .as_ref()
            .map_or(params.len(), |unknown| unknown.parameters.len());
        if assignment.source.port >= nodes[source_node].output_count() || assignment.param >= param_count {
            continue;
        }
        compiled.push(CompiledAssignment {
            id,
            source: assignment.source,
            target_node,
            param: assignment.param,
            depth: assignment.depth,
            enabled: assignment.enabled,
            polarity: assignment.polarity,
            source_polarity: outputs
                .get(assignment.source.port)
                .map_or(Polarity::Bipolar, |port| port.polarity()),
            silent_source: nodes[source_node].unknown.is_some(),
        });
    }

    // 2. SCCs. Cables and assignments are both edges.
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut self_loop = vec![false; n];
    for (to, from) in &graph.cables {
        let u = index_of[&from.module];
        let v = index_of[&to.module];
        succ[u].push(v);
        if u == v {
            self_loop[u] = true;
        }
    }
    for assignment in &compiled {
        let u = index_of[&assignment.source.module];
        let v = assignment.target_node;
        succ[u].push(v);
        if u == v {
            self_loop[u] = true;
        }
    }
    for s in &mut succ {
        s.sort_unstable();
        s.dedup();
    }
    let mut sccs = tarjan(&succ);
    // Tarjan emits SCCs in reverse topological order.
    sccs.reverse();
    let probed: BTreeSet<ModuleId> = probes.values().map(|(from, _)| from.module).collect();
    let modulated: BTreeSet<usize> = compiled.iter().map(|a| a.target_node).collect();
    let live = reaches_a_root(&nodes, &succ, |v| probed.contains(&ids[v]) || modulated.contains(&v));

    // 3 and 4. Steps.
    let mut taps: Vec<TapSpec> = Vec::new();
    // Input and channel to the delay buffer it reads instead of its cable.
    // A delay on a mono cable is listed for channel 0 only.
    let mut overrides: BTreeMap<(InputRef, usize), usize> = BTreeMap::new();
    // Assignment id and channel to the delay buffer the fill should read.
    let mut delayed: BTreeMap<(AssignmentId, usize), usize> = BTreeMap::new();
    let mut steps: Vec<StepSpec> = Vec::new();
    let mut output_nodes: Vec<usize> = Vec::new();
    let mut report = CompileReport {
        modules: n,
        skipped: (0..n).filter(|&v| !live[v]).map(|v| ids[v]).collect(),
        loops: Vec::new(),
        without_live_value: 0,
        per_channel: widths.per_channel.clone(),
        stereo_outputs: widths.stereo_outputs.clone(),
    };

    // One tap per channel of a unit delay. The right channel's key is
    // marked, so each channel's state carries across swaps on its own.
    let user_tap = |v: usize, ch: usize| TapSpec {
        key: u64::from(ids[v].0) | if ch == 1 { RIGHT_TAP } else { 0 },
        source: graph.source_of(InputRef {
            module: ids[v],
            port: 0,
        }),
        channel: ch,
        input: graph
            .source_of(InputRef {
                module: ids[v],
                port: 0,
            })
            .map(|from| channel_buffer(from, ch)),
        output: if ch == 1 {
            right_buf[v][0].expect("a copied unit delay has a stereo output")
        } else {
            out_buf[v][0]
        },
    };

    for scc in &sccs {
        let is_loop = scc.len() > 1 || self_loop[scc[0]];
        // Every member of an SCC reaches every other, so they share one
        // answer.
        let is_live = live[scc[0]];
        if !is_loop {
            let v = scc[0];
            if !is_live {
                // Keep the held sample even while the delay is idle. It is
                // copied into a later schedule if the cable reconnects.
                if matches!(nodes[v].descriptor.role, ModuleRole::UnitDelay) {
                    for ch in 0..channels(v) {
                        taps.push(user_tap(v, ch));
                    }
                }
                continue;
            }
            match nodes[v].descriptor.role {
                ModuleRole::Dsp(_) => steps.push(StepSpec::Module(v)),
                ModuleRole::UnitDelay => {
                    for ch in 0..channels(v) {
                        taps.push(user_tap(v, ch));
                        steps.push(StepSpec::Tap(taps.len() - 1));
                    }
                }
                ModuleRole::AudioOutput => output_nodes.push(v),
            }
            continue;
        }

        // A skipped loop is still analysed, so the report shows its
        // inserted delays on the canvas. Keep its taps for state transfer.
        let mut loop_taps = Vec::new();
        let members: BTreeSet<usize> = scc
            .iter()
            .copied()
            .filter(|&v| match nodes[v].descriptor.role {
                ModuleRole::Dsp(_) => true,
                ModuleRole::UnitDelay => {
                    for ch in 0..channels(v) {
                        taps.push(user_tap(v, ch));
                        loop_taps.push(taps.len() - 1);
                    }
                    false
                }
                // Has no outputs, so it can never be part of a cycle.
                ModuleRole::AudioOutput => false,
            })
            .collect();

        // Edges between DSP members only. Edges into or out of unit delays
        // do not constrain the order: delays emit before and capture after.
        let mut indegree: BTreeMap<usize, usize> = members.iter().map(|&v| (v, 0)).collect();
        let mut incoming: BTreeMap<usize, Vec<(Dep, OutputRef)>> = BTreeMap::new();
        let mut outgoing: BTreeMap<usize, Vec<(Dep, usize)>> = BTreeMap::new();
        let add_edge = |indegree: &mut BTreeMap<usize, usize>,
                        incoming: &mut BTreeMap<usize, Vec<(Dep, OutputRef)>>,
                        outgoing: &mut BTreeMap<usize, Vec<(Dep, usize)>>,
                        dep: Dep,
                        from: OutputRef,
                        v: usize| {
            *indegree.get_mut(&v).expect("member") += 1;
            incoming.entry(v).or_default().push((dep, from));
            outgoing.entry(index_of[&from.module]).or_default().push((dep, v));
        };
        for (to, from) in &graph.cables {
            let u = index_of[&from.module];
            let v = index_of[&to.module];
            if members.contains(&u) && members.contains(&v) {
                add_edge(&mut indegree, &mut incoming, &mut outgoing, Dep::Cable(*to), *from, v);
            }
        }
        for assignment in &compiled {
            let u = index_of[&assignment.source.module];
            let v = assignment.target_node;
            if members.contains(&u) && members.contains(&v) {
                add_edge(
                    &mut indegree,
                    &mut incoming,
                    &mut outgoing,
                    Dep::Assignment(assignment.id),
                    assignment.source,
                    v,
                );
            }
        }

        let mut ordered = Vec::with_capacity(members.len());
        let mut done: BTreeSet<usize> = BTreeSet::new();
        let mut broken: BTreeSet<Dep> = BTreeSet::new();
        let mut ready: BTreeSet<usize> = indegree.iter().filter(|(_, d)| **d == 0).map(|(v, _)| *v).collect();
        let mut inserted = Vec::new();
        let mut inserted_assignments = Vec::new();

        let unresolved = |v: usize, dep_is: fn(&Dep) -> bool, done: &BTreeSet<usize>, broken: &BTreeSet<Dep>| {
            incoming
                .get(&v)
                .into_iter()
                .flatten()
                .any(|(dep, from)| dep_is(dep) && !broken.contains(dep) && !done.contains(&index_of[&from.module]))
        };

        loop {
            while let Some(v) = ready.pop_first() {
                ordered.push(v);
                done.insert(v);
                for (dep, w) in outgoing.get(&v).into_iter().flatten() {
                    if broken.contains(dep) {
                        continue;
                    }
                    let d = indegree.get_mut(w).expect("member");
                    *d -= 1;
                    if *d == 0 && !done.contains(w) {
                        ready.insert(*w);
                    }
                }
            }
            if ordered.len() == members.len() {
                break;
            }
            let remaining = || members.iter().copied().filter(|v| !done.contains(v));
            let is_cable = |dep: &Dep| matches!(dep, Dep::Cable(_));
            let is_assignment = |dep: &Dep| matches!(dep, Dep::Assignment(_));
            // True when `from` reaches `to` over cables that are not yet
            // broken, through modules that have not run yet.
            let cable_reaches = |from: usize, to: usize, done: &BTreeSet<usize>, broken: &BTreeSet<Dep>| {
                let mut seen = BTreeSet::from([from]);
                let mut stack = vec![from];
                while let Some(u) = stack.pop() {
                    for (dep, w) in outgoing.get(&u).into_iter().flatten() {
                        if !is_cable(dep) || broken.contains(dep) || done.contains(w) {
                            continue;
                        }
                        if *w == to {
                            return true;
                        }
                        if seen.insert(*w) {
                            stack.push(*w);
                        }
                    }
                }
                false
            };
            // An unbroken cable into `v` that lies on a cycle of cables only.
            // Only a cable delay can break such a cycle.
            let on_cable_cycle =
                |v: usize, dep: &Dep, from: &OutputRef, done: &BTreeSet<usize>, broken: &BTreeSet<Dep>| {
                    let u = index_of[&from.module];
                    is_cable(dep)
                        && !broken.contains(dep)
                        && !done.contains(&u)
                        && (u == v || cable_reaches(v, u, done, broken))
                };
            let assignments_left = remaining().any(|v| unresolved(v, is_assignment, &done, &broken));
            let cable_cycle_at = |v: usize| {
                incoming
                    .get(&v)
                    .into_iter()
                    .flatten()
                    .any(|(dep, from)| on_cable_cycle(v, dep, from, &done, &broken))
            };
            // Prefer to delay an assignment. A modulation loop then keeps the
            // audio cables on their current sample. A cycle of cables only
            // needs a cable delay anyway, so break those first: that cable
            // may also resolve the modulation loop, and then no assignment
            // is delayed for nothing.
            if assignments_left && !remaining().any(cable_cycle_at) {
                let v = remaining()
                    .filter(|&v| unresolved(v, is_assignment, &done, &broken))
                    .min_by_key(|v| {
                        let count = incoming
                            .get(v)
                            .into_iter()
                            .flatten()
                            .filter(|(dep, _)| is_assignment(dep))
                            .count();
                        (count, *v)
                    })
                    .expect("an assignment is left");
                for (dep, from) in incoming.get(&v).into_iter().flatten() {
                    let Dep::Assignment(id) = *dep else {
                        continue;
                    };
                    if done.contains(&index_of[&from.module]) || broken.contains(dep) {
                        continue;
                    }
                    for ch in 0..if is_stereo(*from) { 2 } else { 1 } {
                        buffer_count += 1;
                        let output = buffer_count - 1;
                        taps.push(TapSpec {
                            key: ASSIGN_TAP | u64::from(id.0) | if ch == 1 { RIGHT_TAP } else { 0 },
                            source: Some(*from),
                            channel: ch,
                            input: Some(channel_buffer(*from, ch)),
                            output,
                        });
                        loop_taps.push(taps.len() - 1);
                        delayed.insert((id, ch), output);
                    }
                    broken.insert(*dep);
                    inserted_assignments.push(id);
                    *indegree.get_mut(&v).expect("member") -= 1;
                }
                if indegree[&v] == 0 {
                    ready.insert(v);
                }
                continue;
            }
            // Stuck on a cycle with no unit delay. Pick the remaining module
            // with the fewest unresolved inputs (lowest ID on ties) and put a
            // one-sample delay on each of those inputs. With assignments
            // left, only modules and cables on a cable-only cycle count, so
            // no cable is delayed where an assignment delay would do.
            let v = remaining()
                .filter(|&v| !assignments_left || cable_cycle_at(v))
                .min_by_key(|v| (indegree[v], *v))
                .expect("remaining member");
            let victims: Vec<(Dep, OutputRef)> = incoming
                .get(&v)
                .into_iter()
                .flatten()
                .filter(|(dep, from)| {
                    if assignments_left {
                        on_cable_cycle(v, dep, from, &done, &broken)
                    } else {
                        is_cable(dep) && !broken.contains(dep) && !done.contains(&index_of[&from.module])
                    }
                })
                .copied()
                .collect();
            for (dep, from) in victims {
                let Dep::Cable(input) = dep else {
                    continue;
                };
                for ch in 0..if is_stereo(from) { 2 } else { 1 } {
                    buffer_count += 1;
                    let output = buffer_count - 1;
                    taps.push(TapSpec {
                        key: AUTO_TAP
                            | (u64::from(input.module.0) << 16)
                            | (input.port as u64 & 0xFFFF)
                            | if ch == 1 { RIGHT_TAP } else { 0 },
                        source: Some(from),
                        channel: ch,
                        input: Some(channel_buffer(from, ch)),
                        output,
                    });
                    loop_taps.push(taps.len() - 1);
                    overrides.insert((input, ch), output);
                }
                broken.insert(dep);
                inserted.push((from, input));
                *indegree.get_mut(&v).expect("member") -= 1;
            }
            if indegree[&v] == 0 {
                ready.insert(v);
            }
        }

        let mut loop_modules: Vec<ModuleId> = scc.iter().map(|&v| ids[v]).collect();
        loop_modules.sort_unstable();
        report.loops.push(FeedbackLoop {
            modules: loop_modules,
            inserted_delays: inserted,
            inserted_assignment_delays: inserted_assignments,
        });
        if is_live {
            steps.push(StepSpec::Loop {
                modules: ordered,
                taps: loop_taps,
            });
        }
    }

    // 5. One buffer per modulated parameter. Sources in assignment id order.
    let mut groups: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for (i, assignment) in compiled.iter().enumerate() {
        if live[assignment.target_node]
            && nodes[assignment.target_node].unknown.is_none()
            && matches!(nodes[assignment.target_node].descriptor.role, ModuleRole::Dsp(_))
        {
            groups
                .entry((assignment.target_node, assignment.param))
                .or_default()
                .push(i);
        }
    }
    // Downmixes each module fills before it runs, and those for the audio
    // output, which run before the outputs are summed.
    let mut downmix_specs: Vec<Vec<DownmixSpec>> = (0..n).map(|_| Vec::new()).collect();
    let mut output_downmix_specs: Vec<DownmixSpec> = Vec::new();
    // Fills per module and copy.
    let mut fills_of: Vec<[Vec<FillSpec>; 2]> = (0..n).map(|_| [Vec::new(), Vec::new()]).collect();
    // The buffer channel `ch` of an assignment's source gives, through its
    // delay if the assignment was broken.
    let assignment_buffer = |assignment: &CompiledAssignment, ch: usize| {
        delayed
            .get(&(assignment.id, ch))
            .or_else(|| delayed.get(&(assignment.id, 0)))
            .copied()
            .unwrap_or_else(|| channel_buffer(assignment.source, ch))
    };
    for ((node, param), sources) in groups {
        let copies = channels(node);
        for (ch, fills) in fills_of[node].iter_mut().enumerate().take(copies) {
            buffer_count += 1;
            let buffer = buffer_count - 1;
            let sources = sources
                .iter()
                .map(|&i| {
                    let assignment = &compiled[i];
                    let buffer = if !is_stereo(assignment.source) || copied[node] {
                        assignment_buffer(assignment, ch)
                    } else {
                        // A stereo source into a module that runs as one
                        // gives the mean, because a parameter has no signal
                        // type (stereo spec, rule 17).
                        buffer_count += 1;
                        let output = buffer_count - 1;
                        downmix_specs[node].push(DownmixSpec {
                            kind: DownmixKind::Mean,
                            left: assignment_buffer(assignment, 0),
                            right: assignment_buffer(assignment, 1),
                            output,
                        });
                        output
                    };
                    SourceSpec { assignment: i, buffer }
                })
                .collect();
            fills.push(FillSpec { param, buffer, sources });
        }
    }

    // Where stereo meets an input of a module that runs as one, a downmix
    // fills a buffer of its own (stereo spec, rule 11). A `Stereo` input
    // reads both channels itself instead.
    let input_buffer = |input: InputRef, ch: usize| -> Option<usize> {
        if let Some(&buffer) = overrides.get(&(input, ch)).or_else(|| overrides.get(&(input, 0))) {
            return Some(buffer);
        }
        graph.cables.get(&input).map(|&from| channel_buffer(from, ch))
    };
    let mut input_downmix: BTreeMap<InputRef, usize> = BTreeMap::new();
    for v in (0..n).filter(|&v| !copied[v]) {
        for (port, spec) in nodes[v].descriptor.inputs.iter().enumerate() {
            let input = InputRef { module: ids[v], port };
            let Some(&from) = graph.cables.get(&input) else {
                continue;
            };
            if !is_stereo(from) || spec.channels == Channels::Stereo {
                continue;
            }
            buffer_count += 1;
            let output = buffer_count - 1;
            let downmix = DownmixSpec {
                kind: DownmixKind::of(spec.signal),
                left: input_buffer(input, 0).expect("connected"),
                right: input_buffer(input, 1).expect("connected"),
                output,
            };
            match nodes[v].descriptor.role {
                ModuleRole::Dsp(_) => downmix_specs[v].push(downmix),
                ModuleRole::AudioOutput => output_downmix_specs.push(downmix),
                // A unit delay's input is `PerChannel`, so stereo there
                // always copies it.
                ModuleRole::UnitDelay => continue,
            }
            input_downmix.insert(input, output);
        }
    }

    // 6. One buffer per distinct default that an unconnected input reads.
    // Only DSP modules read their inputs through pointers.
    let default_of = |input: InputRef| {
        nodes[index_of[&input.module]]
            .descriptor
            .inputs
            .get(input.port)
            .map_or(PortDefault::Value(0.0), |port| port.default)
    };
    let mut default_buf: BTreeMap<DefaultKey, usize> = BTreeMap::new();
    for (v, node) in nodes.iter().enumerate() {
        if !live[v] || !matches!(node.descriptor.role, ModuleRole::Dsp(_)) {
            continue;
        }
        for port in 0..node.input_count() {
            let input = InputRef { module: ids[v], port };
            if graph.cables.contains_key(&input) {
                continue;
            }
            if let Some(key) = DefaultKey::of(default_of(input)) {
                default_buf.entry(key).or_insert_with(|| {
                    buffer_count += 1;
                    buffer_count - 1
                });
            }
        }
    }

    // Allocate buffers and resolve pointers.
    let mut buffers: Vec<Box<[f32]>> = (0..buffer_count)
        .map(|_| vec![0.0; max_block].into_boxed_slice())
        .collect();
    for (key, &buf) in &default_buf {
        if let DefaultKey::Value(bits) = *key {
            // Filled once here. Inputs are read-only, so nothing changes it.
            buffers[buf].fill(f32::from_bits(bits));
        }
    }
    let mut zero: Box<[f32]> = vec![0.0; max_block].into_boxed_slice();
    let zero_ptr: *const f32 = zero.as_mut_ptr();
    let buf_ptrs: Vec<*mut f32> = buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
    let transport_buffer = |key: DefaultKey| default_buf.get(&key).map(|&buf| buf_ptrs[buf]);
    // The Transport module, if any. The controller keeps a patch to one.
    let transport_node = graph
        .nodes
        .iter()
        .find(|(_, node)| node.descriptor.id == crate::transport::TRANSPORT_MODULE);
    let transport = TransportBuffers {
        bar_phase: transport_buffer(DefaultKey::BarPhase),
        beat_phase: transport_buffer(DefaultKey::BeatPhase),
        playing: transport_buffer(DefaultKey::Playing),
        module: transport_node.and_then(|(_, node)| node.slot).map(|slot| slot as u32),
        grid: transport_node.is_some_and(|(&id, _)| graph.cables.keys().any(|to| to.module == id)),
        run_cable: transport_node.is_some_and(|(&id, node)| {
            node.descriptor
                .input_index("run")
                .is_some_and(|port| graph.cables.contains_key(&InputRef { module: id, port }))
        }),
    };

    let resolve_input = |input: InputRef, ch: usize| -> (*const f32, bool) {
        let connected = graph.cables.contains_key(&input);
        if let Some(&buf) = input_downmix.get(&input) {
            (buf_ptrs[buf], connected)
        } else if let Some(buf) = input_buffer(input, ch) {
            (buf_ptrs[buf], connected)
        } else if let Some(key) = DefaultKey::of(default_of(input)) {
            (buf_ptrs[default_buf[&key]], false)
        } else {
            (zero_ptr, false)
        }
    };

    let mut input_ptrs = Vec::new();
    let mut output_ptrs = Vec::new();
    let mut param_ptrs = Vec::new();
    let mut fills = Vec::new();
    let mut ops = Vec::new();
    let mut downmixes = Vec::new();
    let mut right_ptrs = Vec::new();
    let mut op_of_node = vec![[u32::MAX; 2]; n];
    let mut make_op = |v: usize, ch: usize| -> u32 {
        if op_of_node[v][ch] != u32::MAX {
            return op_of_node[v][ch];
        }
        let node = nodes[v];
        let in_start = input_ptrs.len() as u32;
        let right_start = right_ptrs.len() as u32;
        let mut connected = 0u64;
        for port in 0..node.input_count() {
            let input = InputRef { module: ids[v], port };
            let (ptr, is_connected) = resolve_input(input, ch);
            input_ptrs.push(ptr);
            if is_connected {
                connected |= 1 << port;
            }
            // A `Stereo` input on a stereo cable gets its right channel too.
            // A mono cable gives the same buffer on both sides (rule 12).
            if node
                .descriptor
                .inputs
                .get(port)
                .is_some_and(|spec| spec.channels == Channels::Stereo)
                && graph.cables.get(&input).is_some_and(|&from| is_stereo(from))
            {
                let right = input_buffer(input, 1).expect("connected");
                right_ptrs.push(RightPtr {
                    port: port as u16,
                    output: false,
                    ptr: buf_ptrs[right],
                });
            }
        }
        let out_start = output_ptrs.len() as u32;
        for port in 0..node.output_count() {
            let right = right_buf[v][port];
            let buffer = if ch == 1 {
                right.expect("a copied module's outputs are stereo")
            } else {
                out_buf[v][port]
            };
            output_ptrs.push(buf_ptrs[buffer]);
            if node
                .descriptor
                .outputs
                .get(port)
                .is_some_and(|spec| spec.channels == Channels::Stereo)
            {
                right_ptrs.push(RightPtr {
                    port: port as u16,
                    output: true,
                    ptr: buf_ptrs[right.expect("a Stereo output is stereo")],
                });
            }
        }
        // Only a module that runs as one has downmixes (rules 11 and 17).
        let downmix_start = downmixes.len() as u32;
        for spec in downmix_specs[v].iter().filter(|_| ch == 0) {
            downmixes.push(DownmixOp {
                kind: spec.kind,
                left: buf_ptrs[spec.left],
                right: buf_ptrs[spec.right],
                output: buf_ptrs[spec.output],
            });
        }
        let slot = if ch == 1 {
            node.copy_slot
                .expect("the controller gives every copied DSP module a copy slot")
        } else {
            node.slot.expect("DSP modules have a slot")
        } as u32;
        let fill_start = fills.len() as u32;
        let param_start = param_ptrs.len() as u32;
        for spec in &fills_of[v][ch] {
            let param_spec = node.spec(spec.param).expect("fills name real parameters");
            let base = node.params.get(spec.param).copied().unwrap_or(param_spec.default);
            let smooth = (param_spec.options.smooth_seconds > 0.0).then(|| {
                Smoothed::new(
                    param_spec.normalize(base),
                    sample_rate,
                    param_spec.options.smooth_seconds,
                )
            });
            let sources = spec
                .sources
                .iter()
                .map(|source| ModSource {
                    state: source.assignment as u32,
                    buffer: buf_ptrs[source.buffer] as *const f32,
                })
                .collect();
            param_ptrs.push((spec.param as u16, buf_ptrs[spec.buffer] as *const f32));
            fills.push(ParamFill {
                key: (u64::from(ids[v].0) << 32) | (spec.param as u64) | if ch == 1 { RIGHT_FILL } else { 0 },
                slot,
                param: spec.param as u16,
                spec: param_spec,
                base,
                smooth,
                buffer: buf_ptrs[spec.buffer],
                sources,
                live: crate::live::NO_CELL,
            });
        }
        ops.push(ModuleOp {
            slot,
            inputs: (in_start, input_ptrs.len() as u32),
            outputs: (out_start, output_ptrs.len() as u32),
            connected,
            fills: (fill_start, fills.len() as u32),
            params: (param_start, param_ptrs.len() as u32),
            downmixes: (downmix_start, downmixes.len() as u32),
            right: (right_start, right_ptrs.len() as u32),
            channel: ch as u8,
        });
        op_of_node[v][ch] = (ops.len() - 1) as u32;
        op_of_node[v][ch]
    };

    // Taps, sorted by key.
    let mut tap_order: Vec<usize> = (0..taps.len()).collect();
    tap_order.sort_by_key(|&i| taps[i].key);
    let mut tap_index = vec![0u32; taps.len()];
    for (sorted, &original) in tap_order.iter().enumerate() {
        tap_index[original] = sorted as u32;
    }
    let final_taps: Vec<Tap> = tap_order
        .iter()
        .map(|&i| Tap {
            key: taps[i].key,
            source: taps[i].source.map_or(NO_SOURCE, |source| {
                output_key(source)
                    | if taps[i].channel == 1 && is_stereo(source) {
                        RIGHT_TAP
                    } else {
                        0
                    }
            }),
            input: taps[i].input.map_or(zero_ptr, |b| buf_ptrs[b] as *const f32),
            output: buf_ptrs[taps[i].output],
            state: 0.0,
        })
        .collect();

    let mut loop_ops = Vec::new();
    let mut loop_taps = Vec::new();
    let mut final_steps = Vec::with_capacity(steps.len());
    for step in steps {
        match step {
            // A module that runs per channel runs its left copy, then its
            // right. They share no state, so the order does not matter.
            StepSpec::Module(v) => {
                for ch in 0..channels(v) {
                    final_steps.push(Step::Module(make_op(v, ch)));
                }
            }
            StepSpec::Tap(t) => final_steps.push(Step::Tap(tap_index[t])),
            StepSpec::Loop { modules, taps } => {
                let ops_start = loop_ops.len() as u32;
                for v in modules {
                    for ch in 0..channels(v) {
                        loop_ops.push(make_op(v, ch));
                    }
                }
                let taps_start = loop_taps.len() as u32;
                loop_taps.extend(taps.iter().map(|&t| tap_index[t]));
                final_steps.push(Step::Loop {
                    ops: (ops_start, loop_ops.len() as u32),
                    taps: (taps_start, loop_taps.len() as u32),
                });
            }
        }
    }

    let output_downmix_start = downmixes.len() as u32;
    downmixes.extend(output_downmix_specs.iter().map(|spec| DownmixOp {
        kind: spec.kind,
        left: buf_ptrs[spec.left],
        right: buf_ptrs[spec.right],
        output: buf_ptrs[spec.output],
    }));
    let output_downmixes = (output_downmix_start, downmixes.len() as u32);

    let outputs = output_nodes
        .iter()
        .filter_map(|&v| {
            let from = *graph.cables.get(&InputRef {
                module: ids[v],
                port: 0,
            })?;
            Some(OutputOp {
                left: buf_ptrs[channel_buffer(from, 0)] as *const f32,
                right: buf_ptrs[channel_buffer(from, 1)] as *const f32,
            })
        })
        .collect();

    let probes = probes
        .iter()
        .filter_map(|(&id, &(from, kind))| {
            let buffer = *out_buf.get(*index_of.get(&from.module)?)?.get(from.port)?;
            Some(ProbeOp {
                id,
                buffer: buf_ptrs[buffer] as *const f32,
                right: right_buf[*index_of.get(&from.module)?][from.port].map(|buffer| buf_ptrs[buffer] as *const f32),
                kind,
            })
        })
        .collect();

    // `ids` is sorted and ports count up, so this is sorted by key.
    let (ids, buf_ptrs) = (&ids, &buf_ptrs);
    let mut sources: Vec<(u64, *const f32)> = out_buf
        .iter()
        .enumerate()
        .flat_map(|(v, ports)| {
            ports.iter().enumerate().map(move |(port, &buf)| {
                let key = output_key(OutputRef { module: ids[v], port });
                (key, buf_ptrs[buf] as *const f32)
            })
        })
        .collect();
    for (v, ports) in right_buf.iter().enumerate() {
        for (port, buffer) in ports.iter().enumerate() {
            if let Some(buffer) = buffer {
                sources.push((
                    output_key(OutputRef { module: ids[v], port }) | RIGHT_TAP,
                    buf_ptrs[*buffer] as *const f32,
                ));
            }
        }
    }
    sources.sort_unstable_by_key(|&(key, _)| key);

    let mut fill_order: Vec<u32> = (0..fills.len() as u32).collect();
    fill_order.sort_unstable_by_key(|&i| fills[i as usize].key);
    let mut copies: Vec<(u32, u32)> = (0..n)
        .filter(|&v| copied[v] && matches!(nodes[v].descriptor.role, ModuleRole::Dsp(_)))
        .map(|v| {
            let slot = |slot: Option<usize>| slot.expect("copied DSP modules have both slots") as u32;
            (slot(nodes[v].slot), slot(nodes[v].copy_slot))
        })
        .collect();
    copies.sort_unstable();

    let schedule = Schedule {
        _buffers: buffers,
        _zero: zero,
        input_ptrs,
        output_ptrs,
        ops,
        taps: final_taps,
        sources,
        loop_ops,
        loop_taps,
        steps: final_steps,
        outputs,
        probes,
        transport,
        fills,
        fill_order,
        assignments: compiled
            .iter()
            .map(|assignment| AssignmentRuntime {
                id: assignment.id,
                depth: assignment.depth,
                enabled: assignment.enabled,
                polarity: assignment.polarity,
                source_polarity: assignment.source_polarity,
                silent_source: assignment.silent_source,
            })
            .collect(),
        param_ptrs,
        downmixes,
        output_downmixes,
        right_ptrs,
        copies,
        midi_maps: compile_midi_maps(graph, midi_synced),
        live_release: Vec::new(),
    };
    (schedule, report)
}

/// The right copy's slot of the module in `slot`, if it runs as two.
/// `copies` is [`Schedule::copies`].
pub(crate) fn partner(copies: &[(u32, u32)], slot: usize) -> Option<usize> {
    let slot = u32::try_from(slot).ok()?;
    let index = copies.binary_search_by_key(&slot, |&(left, _)| left).ok()?;
    Some(copies[index].1 as usize)
}

/// The graph's MIDI mappings, sorted by control so the engine can find a
/// control change's mapping with a binary search.
fn compile_midi_maps(graph: &Graph, synced: &BTreeMap<MidiMappingId, u32>) -> Vec<MidiMapOp> {
    let mut maps: Vec<MidiMapOp> = graph
        .midi_mappings
        .iter()
        .filter_map(|(&id, mapping)| {
            let node = graph.nodes.get(&mapping.target)?;
            Some(MidiMapOp {
                control: mapping.control,
                id,
                synced: synced.get(&id).copied(),
                slot: node.slot? as u32,
                param: u16::try_from(mapping.param).ok()?,
                spec: node.spec(mapping.param)?,
            })
        })
        .collect();
    maps.sort_by_key(|map| map.control);
    maps
}

/// True for each module that the user can hear or see, or that feeds one
/// over cables and assignments (FR-GRAPH-09). `succ` lists each module's
/// successors. The roots are:
///
/// - the audio output, which the user hears;
/// - a module that `shown` names: one with a probed output, whose meter or
///   scope the user watches (`docs/design/telemetry.md`), or one with a
///   modulated parameter, whose knob shows a moving live dot;
/// - a module that publishes display values or has a light or meter on its
///   panel (`docs/design/panels.md`). It keeps running
///   unconnected, so its lights tell the truth. MIDI Note is one, so it
///   follows every note on and off even with no cable, and a cable added
///   later never finds a stuck note.
///
/// Walks backwards from the roots, so the cost is linear in modules and
/// edges.
fn reaches_a_root(nodes: &[&crate::graph::Node], succ: &[Vec<usize>], shown: impl Fn(usize) -> bool) -> Vec<bool> {
    let n = nodes.len();
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (u, next) in succ.iter().enumerate() {
        for &v in next {
            pred[v].push(u);
        }
    }
    let is_root = |v: usize| {
        let descriptor = nodes[v].descriptor;
        matches!(descriptor.role, ModuleRole::AudioOutput)
            || shown(v)
            || !descriptor.displays.is_empty()
            || descriptor
                .panel
                .widgets
                .iter()
                .any(|w| matches!(w.kind, crate::WidgetKind::Light | crate::WidgetKind::Meter))
    };
    let mut live = vec![false; n];
    let mut stack: Vec<usize> = (0..n).filter(|&v| is_root(v)).collect();
    for &v in &stack {
        live[v] = true;
    }
    while let Some(v) = stack.pop() {
        for &u in &pred[v] {
            if !live[u] {
                live[u] = true;
                stack.push(u);
            }
        }
    }
    live
}

/// Tarjan's strongly connected components. Returns SCCs in reverse
/// topological order, each sorted.
fn tarjan(succ: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct State<'a> {
        succ: &'a [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }

    impl State<'_> {
        fn visit(&mut self, v: usize) {
            self.index[v] = Some(self.next);
            self.low[v] = self.next;
            self.next += 1;
            self.stack.push(v);
            self.on_stack[v] = true;
            for i in 0..self.succ[v].len() {
                let w = self.succ[v][i];
                match self.index[w] {
                    None => {
                        self.visit(w);
                        self.low[v] = self.low[v].min(self.low[w]);
                    }
                    Some(wi) if self.on_stack[w] => self.low[v] = self.low[v].min(wi),
                    Some(_) => {}
                }
            }
            if Some(self.low[v]) == self.index[v] {
                let mut component = Vec::new();
                loop {
                    let w = self.stack.pop().expect("tarjan stack");
                    self.on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                component.sort_unstable();
                self.out.push(component);
            }
        }
    }

    let n = succ.len();
    let mut state = State {
        succ,
        index: vec![None; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    for v in 0..n {
        if state.index[v].is_none() {
            state.visit(v);
        }
    }
    state.out
}

#[cfg(test)]
mod tests;
