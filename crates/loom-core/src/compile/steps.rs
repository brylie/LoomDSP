//! Steps 2 to 4: the order modules run in, and the one-sample delays that
//! break feedback loops.

use super::*;

/// A unit delay's channel, or a delay the compiler inserted, by buffer index.
pub(super) struct TapSpec {
    pub(super) key: u64,
    pub(super) source: Option<OutputRef>,
    pub(super) channel: usize,
    pub(super) input: Option<usize>,
    pub(super) output: usize,
}

/// One step of the schedule, with modules and taps by index.
pub(super) enum StepSpec {
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

impl Dep {
    fn is_cable(&self) -> bool {
        matches!(self, Self::Cable(_))
    }

    fn is_assignment(&self) -> bool {
        matches!(self, Self::Assignment(_))
    }
}

/// Each module's successors over cables and assignments, sorted, and whether
/// the module feeds itself.
pub(super) fn successors(layout: &Layout, compiled: &[CompiledAssignment]) -> (Vec<Vec<usize>>, Vec<bool>) {
    let n = layout.ids.len();
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut self_loop = vec![false; n];
    let cables = layout
        .graph
        .cables
        .iter()
        .map(|(to, from)| (layout.index(from.module), layout.index(to.module)));
    let assignments = compiled
        .iter()
        .map(|assignment| (layout.index(assignment.source.module), assignment.target_node));
    for (u, v) in cables.chain(assignments) {
        succ[u].push(v);
        if u == v {
            self_loop[u] = true;
        }
    }
    for s in &mut succ {
        s.sort_unstable();
        s.dedup();
    }
    (succ, self_loop)
}

/// The steps the schedule runs and the delays it needs (steps 3 and 4).
#[derive(Default)]
pub(super) struct Plan {
    pub(super) taps: Vec<TapSpec>,
    /// Input and channel to the delay buffer it reads instead of its cable.
    /// A delay on a mono cable is listed for channel 0 only.
    pub(super) overrides: BTreeMap<(InputRef, usize), usize>,
    /// Assignment id and channel to the delay buffer the fill should read.
    pub(super) delayed: BTreeMap<(AssignmentId, usize), usize>,
    pub(super) steps: Vec<StepSpec>,
    pub(super) output_nodes: Vec<usize>,
    pub(super) loops: Vec<FeedbackLoop>,
}

impl Plan {
    /// Plans the steps from the SCCs in topological order. Modules outside
    /// loops run a whole block at a time, and each loop one sample at a time.
    pub(super) fn new(
        layout: &Layout,
        compiled: &[CompiledAssignment],
        sccs: &[Vec<usize>],
        self_loop: &[bool],
        live: &[bool],
        buffers: &mut Buffers,
    ) -> Self {
        let mut plan = Self::default();
        for scc in sccs {
            // Every member of an SCC reaches every other, so they share one
            // answer.
            let is_live = live[scc[0]];
            if scc.len() > 1 || self_loop[scc[0]] {
                plan.add_loop(layout, compiled, scc, is_live, buffers);
            } else {
                plan.add_module(layout, scc[0], is_live);
            }
        }
        plan
    }

    /// The buffer channel `ch` of `input` reads: a delay's if the compiler
    /// broke its cable, otherwise the cable's. `None` when unconnected.
    pub(super) fn input_buffer(&self, layout: &Layout, input: InputRef, ch: usize) -> Option<usize> {
        if let Some(&buffer) = self
            .overrides
            .get(&(input, ch))
            .or_else(|| self.overrides.get(&(input, 0)))
        {
            return Some(buffer);
        }
        layout
            .graph
            .cables
            .get(&input)
            .map(|&from| layout.channel_buffer(from, ch))
    }

    /// The buffer channel `ch` of an assignment's source gives, through its
    /// delay if the assignment was broken.
    pub(super) fn assignment_buffer(&self, layout: &Layout, assignment: &CompiledAssignment, ch: usize) -> usize {
        self.delayed
            .get(&(assignment.id, ch))
            .or_else(|| self.delayed.get(&(assignment.id, 0)))
            .copied()
            .unwrap_or_else(|| layout.channel_buffer(assignment.source, ch))
    }

    /// Adds a tap and returns its index.
    fn push_tap(&mut self, tap: TapSpec) -> usize {
        self.taps.push(tap);
        self.taps.len() - 1
    }

    /// Plans a module that is not part of a loop.
    fn add_module(&mut self, layout: &Layout, v: usize, is_live: bool) {
        let role = &layout.nodes[v].descriptor.role;
        if !is_live {
            // Keep the held sample even while the delay is idle. It is
            // copied into a later schedule if the cable reconnects.
            if matches!(role, ModuleRole::UnitDelay) {
                for ch in 0..layout.channels(v) {
                    self.push_tap(user_tap(layout, v, ch));
                }
            }
            return;
        }
        match role {
            ModuleRole::Dsp(_) => self.steps.push(StepSpec::Module(v)),
            ModuleRole::UnitDelay => {
                for ch in 0..layout.channels(v) {
                    let tap = self.push_tap(user_tap(layout, v, ch));
                    self.steps.push(StepSpec::Tap(tap));
                }
            }
            ModuleRole::AudioOutput => self.output_nodes.push(v),
        }
    }

    /// Plans a feedback loop: orders its DSP members and inserts a delay
    /// wherever a cycle has no unit delay. A skipped loop is still analysed,
    /// so the report shows its inserted delays on the canvas, and its taps
    /// are kept for state transfer.
    fn add_loop(
        &mut self,
        layout: &Layout,
        compiled: &[CompiledAssignment],
        scc: &[usize],
        is_live: bool,
        buffers: &mut Buffers,
    ) {
        let mut loop_taps = Vec::new();
        let mut members = BTreeSet::new();
        for &v in scc {
            match layout.nodes[v].descriptor.role {
                ModuleRole::Dsp(_) => {
                    members.insert(v);
                }
                ModuleRole::UnitDelay => {
                    for ch in 0..layout.channels(v) {
                        loop_taps.push(self.push_tap(user_tap(layout, v, ch)));
                    }
                }
                // Has no outputs, so it can never be part of a cycle.
                ModuleRole::AudioOutput => {}
            }
        }

        let mut order = LoopOrder::new(layout, compiled, members);
        let mut inserted = Vec::new();
        let mut inserted_assignments = Vec::new();
        loop {
            order.run_ready();
            if order.is_complete() {
                break;
            }
            // Prefer to delay an assignment. A modulation loop then keeps the
            // audio cables on their current sample. A cycle of cables only
            // needs a cable delay anyway, so break those first: that cable
            // may also resolve the modulation loop, and then no assignment
            // is delayed for nothing.
            let assignments_left = order.assignments_left();
            if assignments_left && !order.cable_cycle_left() {
                let v = order.fewest_assignments();
                for (id, from) in order.pending_assignments(v) {
                    for ch in 0..layout.output_channels(from) {
                        let output = buffers.add();
                        loop_taps.push(self.push_tap(TapSpec {
                            key: ASSIGN_TAP | u64::from(id.0) | right_tap(ch),
                            source: Some(from),
                            channel: ch,
                            input: Some(layout.channel_buffer(from, ch)),
                            output,
                        }));
                        self.delayed.insert((id, ch), output);
                    }
                    order.break_edge(v, Dep::Assignment(id));
                    inserted_assignments.push(id);
                }
                order.release(v);
                continue;
            }
            // Stuck on a cycle with no unit delay. Pick the remaining module
            // with the fewest unresolved inputs (lowest ID on ties) and put a
            // one-sample delay on each of those inputs. With assignments
            // left, only modules and cables on a cable-only cycle count, so
            // no cable is delayed where an assignment delay would do.
            let v = order.fewest_inputs(assignments_left);
            for (input, from) in order.cables_to_break(v, assignments_left) {
                for ch in 0..layout.output_channels(from) {
                    let output = buffers.add();
                    loop_taps.push(self.push_tap(TapSpec {
                        key: AUTO_TAP
                            | (u64::from(input.module.0) << 16)
                            | (input.port as u64 & 0xFFFF)
                            | right_tap(ch),
                        source: Some(from),
                        channel: ch,
                        input: Some(layout.channel_buffer(from, ch)),
                        output,
                    }));
                    self.overrides.insert((input, ch), output);
                }
                order.break_edge(v, Dep::Cable(input));
                inserted.push((from, input));
            }
            order.release(v);
        }

        let mut modules: Vec<ModuleId> = scc.iter().map(|&v| layout.ids[v]).collect();
        modules.sort_unstable();
        self.loops.push(FeedbackLoop {
            modules,
            inserted_delays: inserted,
            inserted_assignment_delays: inserted_assignments,
        });
        if is_live {
            self.steps.push(StepSpec::Loop {
                modules: order.ordered,
                taps: loop_taps,
            });
        }
    }
}

/// One tap per channel of a unit delay. The right channel's key is marked,
/// so each channel's state carries across swaps on its own.
fn user_tap(layout: &Layout, v: usize, ch: usize) -> TapSpec {
    let source = layout.graph.source_of(InputRef {
        module: layout.ids[v],
        port: 0,
    });
    TapSpec {
        key: u64::from(layout.ids[v].0) | right_tap(ch),
        source,
        channel: ch,
        input: source.map(|from| layout.channel_buffer(from, ch)),
        output: if ch == 1 {
            layout.right_buf[v][0].expect("a copied unit delay has a stereo output")
        } else {
            layout.out_buf[v][0]
        },
    }
}

/// Orders the DSP members of one feedback loop, a topological sort that
/// breaks an edge whenever a cycle leaves no member ready. Only edges between
/// DSP members count. Edges into or out of unit delays do not constrain the
/// order: delays emit before and capture after.
struct LoopOrder<'l, 'a> {
    layout: &'l Layout<'a>,
    members: BTreeSet<usize>,
    /// Unbroken edges into each member from members that have not run.
    indegree: BTreeMap<usize, usize>,
    incoming: BTreeMap<usize, Vec<(Dep, OutputRef)>>,
    outgoing: BTreeMap<usize, Vec<(Dep, usize)>>,
    /// Members in the order they run.
    ordered: Vec<usize>,
    done: BTreeSet<usize>,
    broken: BTreeSet<Dep>,
    ready: BTreeSet<usize>,
}

impl<'l, 'a> LoopOrder<'l, 'a> {
    fn new(layout: &'l Layout<'a>, compiled: &[CompiledAssignment], members: BTreeSet<usize>) -> Self {
        let mut order = Self {
            layout,
            indegree: members.iter().map(|&v| (v, 0)).collect(),
            ordered: Vec::with_capacity(members.len()),
            members,
            incoming: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            done: BTreeSet::new(),
            broken: BTreeSet::new(),
            ready: BTreeSet::new(),
        };
        for (to, from) in &layout.graph.cables {
            order.add_edge(Dep::Cable(*to), *from, layout.index(to.module));
        }
        for assignment in compiled {
            order.add_edge(
                Dep::Assignment(assignment.id),
                assignment.source,
                assignment.target_node,
            );
        }
        order.ready = order
            .indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(v, _)| *v)
            .collect();
        order
    }

    /// Adds the edge `dep` from `from` into `v`, if both ends are members.
    fn add_edge(&mut self, dep: Dep, from: OutputRef, v: usize) {
        let u = self.layout.index(from.module);
        if !self.members.contains(&u) || !self.members.contains(&v) {
            return;
        }
        *self.indegree.get_mut(&v).expect("member") += 1;
        self.incoming.entry(v).or_default().push((dep, from));
        self.outgoing.entry(u).or_default().push((dep, v));
    }

    /// Runs every ready member, and every member that becomes ready.
    fn run_ready(&mut self) {
        while let Some(v) = self.ready.pop_first() {
            self.ordered.push(v);
            self.done.insert(v);
            for (dep, w) in self.outgoing.get(&v).into_iter().flatten() {
                if self.broken.contains(dep) {
                    continue;
                }
                let d = self.indegree.get_mut(w).expect("member");
                *d -= 1;
                if *d == 0 && !self.done.contains(w) {
                    self.ready.insert(*w);
                }
            }
        }
    }

    fn is_complete(&self) -> bool {
        self.ordered.len() == self.members.len()
    }

    /// Members that have not run yet.
    fn remaining(&self) -> impl Iterator<Item = usize> + '_ {
        self.members.iter().copied().filter(|v| !self.done.contains(v))
    }

    fn incoming(&self, v: usize) -> impl Iterator<Item = &(Dep, OutputRef)> {
        self.incoming.get(&v).into_iter().flatten()
    }

    /// True when `dep` is not broken and `from` has not run yet.
    fn pending(&self, dep: &Dep, from: OutputRef) -> bool {
        !self.broken.contains(dep) && !self.done.contains(&self.layout.index(from.module))
    }

    fn has_pending_assignment(&self, v: usize) -> bool {
        self.incoming(v)
            .any(|(dep, from)| dep.is_assignment() && self.pending(dep, *from))
    }

    fn assignments_left(&self) -> bool {
        self.remaining().any(|v| self.has_pending_assignment(v))
    }

    /// True when `from` reaches `to` over cables that are not yet broken,
    /// through modules that have not run yet.
    fn cable_reaches(&self, from: usize, to: usize) -> bool {
        let mut seen = BTreeSet::from([from]);
        let mut stack = vec![from];
        while let Some(u) = stack.pop() {
            for (dep, w) in self.outgoing.get(&u).into_iter().flatten() {
                if !dep.is_cable() || self.broken.contains(dep) || self.done.contains(w) {
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
    }

    /// True for an unbroken cable into `v` that lies on a cycle of cables
    /// only. Only a cable delay can break such a cycle.
    fn on_cable_cycle(&self, v: usize, dep: &Dep, from: OutputRef) -> bool {
        let u = self.layout.index(from.module);
        dep.is_cable() && self.pending(dep, from) && (u == v || self.cable_reaches(v, u))
    }

    fn cable_cycle_at(&self, v: usize) -> bool {
        self.incoming(v).any(|(dep, from)| self.on_cable_cycle(v, dep, *from))
    }

    fn cable_cycle_left(&self) -> bool {
        self.remaining().any(|v| self.cable_cycle_at(v))
    }

    /// The remaining member with a pending assignment that has the fewest
    /// assignments in, lowest index on ties.
    fn fewest_assignments(&self) -> usize {
        self.remaining()
            .filter(|&v| self.has_pending_assignment(v))
            .min_by_key(|&v| (self.incoming(v).filter(|(dep, _)| dep.is_assignment()).count(), v))
            .expect("an assignment is left")
    }

    /// The pending assignments into `v`.
    fn pending_assignments(&self, v: usize) -> Vec<(AssignmentId, OutputRef)> {
        self.incoming(v)
            .filter_map(|&(dep, from)| match dep {
                Dep::Assignment(id) if self.pending(&dep, from) => Some((id, from)),
                _ => None,
            })
            .collect()
    }

    /// The remaining member with the fewest unresolved inputs, lowest index
    /// on ties. With assignments left, only members on a cable-only cycle.
    fn fewest_inputs(&self, assignments_left: bool) -> usize {
        self.remaining()
            .filter(|&v| !assignments_left || self.cable_cycle_at(v))
            .min_by_key(|&v| (self.indegree[&v], v))
            .expect("remaining member")
    }

    /// The pending cables into `v` to delay. With assignments left, only
    /// those on a cable-only cycle.
    fn cables_to_break(&self, v: usize, assignments_left: bool) -> Vec<(InputRef, OutputRef)> {
        self.incoming(v)
            .filter(|(dep, from)| {
                if assignments_left {
                    self.on_cable_cycle(v, dep, *from)
                } else {
                    dep.is_cable() && self.pending(dep, *from)
                }
            })
            .filter_map(|&(dep, from)| match dep {
                Dep::Cable(input) => Some((input, from)),
                Dep::Assignment(_) => None,
            })
            .collect()
    }

    /// Breaks the edge `dep` into `v`, which no longer waits for it.
    fn break_edge(&mut self, v: usize, dep: Dep) {
        self.broken.insert(dep);
        *self.indegree.get_mut(&v).expect("member") -= 1;
    }

    /// Makes `v` ready once nothing it waits for is left.
    fn release(&mut self, v: usize) {
        if self.indegree[&v] == 0 {
            self.ready.insert(v);
        }
    }
}
