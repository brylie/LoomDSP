//! Turns a [`Graph`] into an execution [`Schedule`] (ADR-0002).
//!
//! This runs on the control thread and may allocate freely.
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
//!    where, so the UI can show them.

use crate::descriptor::ModuleRole;
use crate::graph::{Graph, InputRef, ModuleId, OutputRef};
use crate::telemetry::{ProbeId, ProbeKind};
use std::collections::{BTreeMap, BTreeSet};

/// Marks tap keys created by the compiler, so they never clash with user
/// unit delay keys (which are module IDs).
const AUTO_TAP: u64 = 1 << 63;

/// A feedback loop found by the compiler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackLoop {
    /// Every module in the loop, sorted by ID.
    pub modules: Vec<ModuleId>,
    /// Cables where the compiler inserted a one-sample delay. Empty when the
    /// loop already contains a user unit delay in every cycle.
    pub inserted_delays: Vec<(OutputRef, InputRef)>,
}

/// What the compiler found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompileReport {
    /// Number of modules in the schedule.
    pub modules: usize,
    /// Feedback loops, each run one sample at a time.
    pub loops: Vec<FeedbackLoop>,
}

impl CompileReport {
    /// All delays the compiler inserted, across all loops.
    pub fn inserted_delays(&self) -> impl Iterator<Item = &(OutputRef, InputRef)> {
        self.loops.iter().flat_map(|l| l.inserted_delays.iter())
    }
}

pub(crate) struct ModuleOp {
    pub slot: u32,
    pub inputs: (u32, u32),
    pub outputs: (u32, u32),
    pub connected: u64,
}

pub(crate) struct Tap {
    pub key: u64,
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
    pub kind: ProbeKind,
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
    pub loop_ops: Vec<u32>,
    pub loop_taps: Vec<u32>,
    pub steps: Vec<Step>,
    pub outputs: Vec<OutputOp>,
    pub probes: Vec<ProbeOp>,
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
            loop_ops: Vec::new(),
            loop_taps: Vec::new(),
            steps: Vec::new(),
            outputs: Vec::new(),
            probes: Vec::new(),
        }
    }

    /// Copies unit delay state from the previous schedule, matched by key.
    /// Runs on the audio thread: no allocation, linear time.
    pub fn take_tap_state_from(&mut self, old: &Schedule) {
        let mut j = 0;
        for tap in &mut self.taps {
            while j < old.taps.len() && old.taps[j].key < tap.key {
                j += 1;
            }
            if j < old.taps.len() && old.taps[j].key == tap.key {
                tap.state = old.taps[j].state;
            }
        }
    }
}

struct TapSpec {
    key: u64,
    input: Option<usize>,
    output: usize,
}

enum StepSpec {
    Module(usize),
    Tap(usize),
    Loop { modules: Vec<usize>, taps: Vec<usize> },
}

/// Compiles the graph. `max_block` is the buffer length. `probes` are output
/// ports to read for telemetry; probes on unknown modules or ports are
/// skipped.
pub(crate) fn compile(
    graph: &Graph,
    probes: &BTreeMap<ProbeId, (OutputRef, ProbeKind)>,
    max_block: usize,
) -> (Schedule, CompileReport) {
    let ids: Vec<ModuleId> = graph.nodes.keys().copied().collect();
    let index_of: BTreeMap<ModuleId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let nodes: Vec<&crate::graph::Node> = graph.nodes.values().collect();
    let n = ids.len();

    // 1. One buffer per output port.
    let mut buffer_count = 0usize;
    let out_buf: Vec<Vec<usize>> = nodes
        .iter()
        .map(|node| {
            (0..node.descriptor.outputs.len())
                .map(|_| {
                    buffer_count += 1;
                    buffer_count - 1
                })
                .collect()
        })
        .collect();
    let source_buffer = |from: OutputRef| out_buf[index_of[&from.module]][from.port];

    // 2. SCCs.
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
    for s in &mut succ {
        s.sort_unstable();
        s.dedup();
    }
    let mut sccs = tarjan(&succ);
    // Tarjan emits SCCs in reverse topological order.
    sccs.reverse();

    // 3 and 4. Steps.
    let mut taps: Vec<TapSpec> = Vec::new();
    let mut overrides: BTreeMap<InputRef, usize> = BTreeMap::new();
    let mut steps: Vec<StepSpec> = Vec::new();
    let mut output_nodes: Vec<usize> = Vec::new();
    let mut report = CompileReport {
        modules: n,
        loops: Vec::new(),
    };

    let user_tap = |v: usize| TapSpec {
        key: u64::from(ids[v].0),
        input: graph
            .source_of(InputRef {
                module: ids[v],
                port: 0,
            })
            .map(source_buffer),
        output: out_buf[v][0],
    };

    for scc in &sccs {
        let is_loop = scc.len() > 1 || self_loop[scc[0]];
        if !is_loop {
            let v = scc[0];
            match nodes[v].descriptor.role {
                ModuleRole::Dsp(_) => steps.push(StepSpec::Module(v)),
                ModuleRole::UnitDelay => {
                    taps.push(user_tap(v));
                    steps.push(StepSpec::Tap(taps.len() - 1));
                }
                ModuleRole::AudioOutput => output_nodes.push(v),
            }
            continue;
        }

        let mut loop_taps = Vec::new();
        let members: BTreeSet<usize> = scc
            .iter()
            .copied()
            .filter(|&v| match nodes[v].descriptor.role {
                ModuleRole::Dsp(_) => true,
                ModuleRole::UnitDelay => {
                    taps.push(user_tap(v));
                    loop_taps.push(taps.len() - 1);
                    false
                }
                // Has no outputs, so it can never be part of a cycle.
                ModuleRole::AudioOutput => false,
            })
            .collect();

        // Edges between DSP members only. Edges into or out of unit delays
        // do not constrain the order: delays emit before and capture after.
        let mut indegree: BTreeMap<usize, usize> = members.iter().map(|&v| (v, 0)).collect();
        let mut incoming: BTreeMap<usize, Vec<(InputRef, OutputRef)>> = BTreeMap::new();
        let mut outgoing: BTreeMap<usize, Vec<(InputRef, usize)>> = BTreeMap::new();
        for (to, from) in &graph.cables {
            let u = index_of[&from.module];
            let v = index_of[&to.module];
            if members.contains(&u) && members.contains(&v) {
                *indegree.get_mut(&v).expect("member") += 1;
                incoming.entry(v).or_default().push((*to, *from));
                outgoing.entry(u).or_default().push((*to, v));
            }
        }

        let mut ordered = Vec::with_capacity(members.len());
        let mut done: BTreeSet<usize> = BTreeSet::new();
        let mut broken: BTreeSet<InputRef> = BTreeSet::new();
        let mut ready: BTreeSet<usize> = indegree.iter().filter(|(_, d)| **d == 0).map(|(v, _)| *v).collect();
        let mut inserted = Vec::new();

        loop {
            while let Some(v) = ready.pop_first() {
                ordered.push(v);
                done.insert(v);
                for (input, w) in outgoing.get(&v).into_iter().flatten() {
                    if broken.contains(input) {
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
            // Stuck on a cycle with no unit delay. Pick the remaining module
            // with the fewest unresolved inputs (lowest ID on ties) and put a
            // one-sample delay on each of those inputs.
            let v = *members
                .iter()
                .filter(|v| !done.contains(v))
                .min_by_key(|v| (indegree[v], **v))
                .expect("remaining member");
            for (input, from) in incoming.get(&v).into_iter().flatten() {
                if done.contains(&index_of[&from.module]) || broken.contains(input) {
                    continue;
                }
                buffer_count += 1;
                let output = buffer_count - 1;
                taps.push(TapSpec {
                    key: AUTO_TAP | (u64::from(input.module.0) << 16) | (input.port as u64 & 0xFFFF),
                    input: Some(source_buffer(*from)),
                    output,
                });
                loop_taps.push(taps.len() - 1);
                overrides.insert(*input, output);
                broken.insert(*input);
                inserted.push((*from, *input));
                *indegree.get_mut(&v).expect("member") -= 1;
            }
            debug_assert_eq!(indegree[&v], 0);
            ready.insert(v);
        }

        let mut loop_modules: Vec<ModuleId> = scc.iter().map(|&v| ids[v]).collect();
        loop_modules.sort_unstable();
        report.loops.push(FeedbackLoop {
            modules: loop_modules,
            inserted_delays: inserted,
        });
        steps.push(StepSpec::Loop {
            modules: ordered,
            taps: loop_taps,
        });
    }

    // Allocate buffers and resolve pointers.
    let mut buffers: Vec<Box<[f32]>> = (0..buffer_count)
        .map(|_| vec![0.0; max_block].into_boxed_slice())
        .collect();
    let mut zero: Box<[f32]> = vec![0.0; max_block].into_boxed_slice();
    let zero_ptr: *const f32 = zero.as_mut_ptr();
    let buf_ptrs: Vec<*mut f32> = buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();

    let resolve_input = |input: InputRef| -> (*const f32, bool) {
        let connected = graph.cables.contains_key(&input);
        if let Some(&buf) = overrides.get(&input) {
            (buf_ptrs[buf], connected)
        } else if let Some(from) = graph.cables.get(&input) {
            (buf_ptrs[source_buffer(*from)], connected)
        } else {
            (zero_ptr, false)
        }
    };

    let mut input_ptrs = Vec::new();
    let mut output_ptrs = Vec::new();
    let mut ops = Vec::new();
    let mut op_of_node = vec![u32::MAX; n];
    let mut make_op = |v: usize| -> u32 {
        if op_of_node[v] != u32::MAX {
            return op_of_node[v];
        }
        let node = nodes[v];
        let in_start = input_ptrs.len() as u32;
        let mut connected = 0u64;
        for port in 0..node.descriptor.inputs.len() {
            let (ptr, is_connected) = resolve_input(InputRef { module: ids[v], port });
            input_ptrs.push(ptr);
            if is_connected {
                connected |= 1 << port;
            }
        }
        let out_start = output_ptrs.len() as u32;
        output_ptrs.extend(out_buf[v].iter().map(|&b| buf_ptrs[b]));
        ops.push(ModuleOp {
            slot: node.slot.expect("DSP modules have a slot") as u32,
            inputs: (in_start, input_ptrs.len() as u32),
            outputs: (out_start, output_ptrs.len() as u32),
            connected,
        });
        op_of_node[v] = (ops.len() - 1) as u32;
        op_of_node[v]
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
            input: taps[i].input.map_or(zero_ptr, |b| buf_ptrs[b] as *const f32),
            output: buf_ptrs[taps[i].output],
            state: 0.0,
        })
        .collect();

    let mut loop_ops = Vec::new();
    let mut loop_taps = Vec::new();
    let mut final_steps = Vec::with_capacity(steps.len());
    for step in steps {
        final_steps.push(match step {
            StepSpec::Module(v) => Step::Module(make_op(v)),
            StepSpec::Tap(t) => Step::Tap(tap_index[t]),
            StepSpec::Loop { modules, taps } => {
                let ops_start = loop_ops.len() as u32;
                for v in modules {
                    let op = make_op(v);
                    loop_ops.push(op);
                }
                let taps_start = loop_taps.len() as u32;
                loop_taps.extend(taps.iter().map(|&t| tap_index[t]));
                Step::Loop {
                    ops: (ops_start, loop_ops.len() as u32),
                    taps: (taps_start, loop_taps.len() as u32),
                }
            }
        });
    }

    let outputs = output_nodes
        .iter()
        .filter_map(|&v| {
            let left = graph.cables.get(&InputRef {
                module: ids[v],
                port: 0,
            });
            let right = graph.cables.get(&InputRef {
                module: ids[v],
                port: 1,
            });
            let ptr = |from: &OutputRef| buf_ptrs[source_buffer(*from)] as *const f32;
            match (left, right) {
                (Some(l), Some(r)) => Some(OutputOp {
                    left: ptr(l),
                    right: ptr(r),
                }),
                // Mono into one side plays on both sides.
                (Some(m), None) | (None, Some(m)) => Some(OutputOp {
                    left: ptr(m),
                    right: ptr(m),
                }),
                (None, None) => None,
            }
        })
        .collect();

    let probes = probes
        .iter()
        .filter_map(|(&id, &(from, kind))| {
            let buffer = *out_buf.get(*index_of.get(&from.module)?)?.get(from.port)?;
            Some(ProbeOp {
                id,
                buffer: buf_ptrs[buffer] as *const f32,
                kind,
            })
        })
        .collect();

    let schedule = Schedule {
        _buffers: buffers,
        _zero: zero,
        input_ptrs,
        output_ptrs,
        ops,
        taps: final_taps,
        loop_ops,
        loop_taps,
        steps: final_steps,
        outputs,
        probes,
    };
    (schedule, report)
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
mod tests {
    use super::*;

    #[test]
    fn tarjan_finds_cycles_in_topological_order() {
        // 0 -> 1 -> 2 -> 1, 2 -> 3
        let succ = vec![vec![1], vec![2], vec![1, 3], vec![]];
        let mut sccs = tarjan(&succ);
        sccs.reverse();
        assert_eq!(sccs, vec![vec![0], vec![1, 2], vec![3]]);
    }
}
