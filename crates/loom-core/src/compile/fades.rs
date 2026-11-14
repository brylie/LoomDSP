//! Which inputs and modulation contributions fade at a swap (live patching
//! spec, rule 9). Runs on the control thread and compares the graph the
//! last schedule was compiled from, kept as a [`Wiring`], with the new one.
//! `cable_fade.rs` holds the ramps and what the engine does with them.

use super::*;
use crate::cable_fade::{
    MEMBER_MODULE, MEMBER_PARAM, SELECT_MEMBER_INPUT, SELECT_MEMBERS_PARAM, SELECT_MODULE, SPREAD_SUM_MODULE,
    SUM_GAIN_PARAM, SUM_MODULE, SWAP_FADE_DONE, SWAP_FADE_MODULE, input_key,
};
use crate::display::DisplaySource;

/// A fade that may still run in the engine, named by what it fades. The
/// controller keeps these and gives each one a slot in the next schedule
/// too, so a swap while a fade runs does not cut it short (rule 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FadeKey {
    /// One channel of an input ([`input_key`]).
    Input(u64),
    /// One assignment's contribution on one copy's fill (fill key).
    Contribution(u64, AssignmentId),
}

/// The kind of a mixer the editor adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MixerKind {
    Sum,
    SpreadSum,
    Select,
    SwapFade,
}

impl MixerKind {
    fn of(id: &str) -> Option<Self> {
        match id {
            SUM_MODULE => Some(Self::Sum),
            SPREAD_SUM_MODULE => Some(Self::SpreadSum),
            SELECT_MODULE => Some(Self::Select),
            SWAP_FADE_MODULE => Some(Self::SwapFade),
            _ => None,
        }
    }
}

/// What a mixer plays, read from the graph.
#[derive(Debug, Clone)]
struct Mixer {
    kind: MixerKind,
    /// The output that feeds each of its signal inputs.
    inputs: Vec<Option<OutputRef>>,
    /// The one source it plays alone on its first frame, if any.
    first: Option<OutputRef>,
    /// The one source it plays alone once settled, if any. For a Swap
    /// fade it holds only once the fade is done, which the controller reads
    /// from `done`.
    last: Option<OutputRef>,
    /// Where a Swap fade publishes `done`.
    done: Option<DisplaySource>,
}

/// What the last compiled schedule was built from: enough of the graph to
/// tell, at the next compile, which cables and assignments changed.
#[derive(Debug, Default)]
pub(crate) struct Wiring {
    /// Every module, scheduled or not, so a module that resumes (rule 8)
    /// can be told from a new one.
    modules: BTreeSet<ModuleId>,
    /// Every cable.
    cables: BTreeMap<InputRef, OutputRef>,
    /// Every input channel the schedule read ([`input_key`]).
    reads: BTreeSet<u64>,
    /// The mixers the editor added.
    mixers: BTreeMap<ModuleId, Mixer>,
    /// Every assignment: source, target, and parameter.
    assignments: BTreeMap<AssignmentId, (OutputRef, ModuleId, usize)>,
}

impl Wiring {
    /// The Swap fades of this wiring that the new graph no longer has,
    /// with where each publishes `done`. The controller reads them before
    /// it compiles.
    pub(crate) fn leaving_fades(&self, graph: &Graph) -> Vec<(ModuleId, DisplaySource)> {
        self.mixers
            .iter()
            .filter(|(id, mixer)| mixer.kind == MixerKind::SwapFade && !graph.nodes.contains_key(id))
            .filter_map(|(&id, mixer)| Some((id, mixer.done?)))
            .collect()
    }
}

/// The signal types outputs carry past the mixers, worked out at most once
/// per output in a compile, so a patch of many mixers that share inputs
/// costs time linear in its cables, not exponential in its depth.
#[derive(Default)]
pub(super) struct Carried(BTreeMap<OutputRef, Option<SignalType>>);

impl Carried {
    /// What `from` carries: its own type, or for a mixer's output the type
    /// of the first source upstream that is not a mixer, following the
    /// mixer's inputs in order. `None` when nothing upstream has a type.
    fn signal(&mut self, layout: &Layout, from: OutputRef) -> Option<SignalType> {
        if let Some(&known) = self.0.get(&from) {
            return known;
        }
        let v = *layout.index_of.get(&from.module)?;
        let node = layout.nodes[v];
        if MixerKind::of(node.descriptor.id).is_none() {
            let signal = node.descriptor.outputs.get(from.port).map(|port| port.signal);
            self.0.insert(from, signal);
            return signal;
        }
        // Marked before the walk, so a loop of mixers ends.
        self.0.insert(from, None);
        let signal = (0..node.input_count())
            .filter(|&port| node.descriptor.inputs[port].name != SELECT_MEMBER_INPUT)
            .filter_map(|port| {
                layout.graph.source_of(InputRef {
                    module: from.module,
                    port,
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .find_map(|source| self.signal(layout, source));
        self.0.insert(from, signal);
        signal
    }
}

/// True when a signal of this type fades (rule 9): audio, modulation, and
/// pitch. Gates, triggers, and phases switch.
fn fades(signal: SignalType) -> bool {
    matches!(signal, SignalType::Audio | SignalType::Modulation | SignalType::Pitch)
}

/// True when the module is one of the mixers the editor adds.
fn is_mixer(layout: &Layout, module: ModuleId) -> bool {
    layout
        .index_of
        .get(&module)
        .is_some_and(|&v| MixerKind::of(layout.nodes[v].descriptor.id).is_some())
}

/// True when input `port` of the module at `v` fades on a cable change.
/// The port's own type decides (rule 9). Into or out of a mixer, whose
/// ports carry any signal, the type the cable carries past the mixers
/// decides too, so a gate through a Sum switches.
pub(super) fn input_fades(
    layout: &Layout,
    carried: &mut Carried,
    v: usize,
    port: usize,
    from: Option<OutputRef>,
) -> bool {
    let node = layout.nodes[v];
    if node.unknown.is_some() {
        return false;
    }
    let Some(spec) = node.descriptor.inputs.get(port) else {
        return false;
    };
    if !fades(spec.signal) {
        return false;
    }
    let mixer = MixerKind::of(node.descriptor.id).is_some();
    if mixer && spec.name == SELECT_MEMBER_INPUT {
        // Read as a whole member number: a ramp would walk through the
        // wrong members.
        return false;
    }
    match from {
        Some(from) if mixer || is_mixer(layout, from.module) => carried.signal(layout, from).is_none_or(fades),
        _ => true,
    }
}

/// The mixer `id` of `graph`, if it is one.
fn mixer_of(graph: &Graph, id: ModuleId, node: &Node) -> Option<Mixer> {
    let kind = MixerKind::of(node.descriptor.id)?;
    let signal_ports: Vec<usize> = (0..node.input_count())
        .filter(|&port| node.descriptor.inputs[port].name != SELECT_MEMBER_INPUT)
        .collect();
    let inputs: Vec<Option<OutputRef>> = signal_ports
        .iter()
        .map(|&port| graph.source_of(InputRef { module: id, port }))
        .collect();
    let connected: Vec<OutputRef> = inputs.iter().flatten().copied().collect();
    let alone = match kind {
        // One input at gain 1 plays that input exactly.
        MixerKind::Sum => (connected.len() == 1 && sum_gain(node) == Some(1.0)).then(|| connected[0]),
        // A spread sum pans, so it never plays a source unchanged.
        MixerKind::SpreadSum => None,
        MixerKind::Select => select_choice(graph, id, node, &inputs),
        MixerKind::SwapFade => None,
    };
    let (first, last) = match kind {
        // A new fade starts at `a` alone; a finished one plays `b` alone.
        MixerKind::SwapFade => (inputs.first().copied().flatten(), inputs.get(1).copied().flatten()),
        _ => (alone, alone),
    };
    let done = (kind == MixerKind::SwapFade)
        .then(|| {
            let slot = node.slot?;
            let index = node.descriptor.display_index(SWAP_FADE_DONE)?;
            let spec = node.descriptor.displays.get(index)?;
            Some(DisplaySource::new(slot, id.0, index, spec.kind))
        })
        .flatten();
    Some(Mixer {
        kind,
        inputs,
        first,
        last,
        done,
    })
}

/// The input a Select plays, from its Member module's value in the graph.
/// `None` when the member is modulated, so the compiler cannot know.
fn select_choice(graph: &Graph, id: ModuleId, node: &Node, inputs: &[Option<OutputRef>]) -> Option<OutputRef> {
    let member_port = node.descriptor.input_index(SELECT_MEMBER_INPUT)?;
    let value = match graph.source_of(InputRef {
        module: id,
        port: member_port,
    }) {
        None => match node.descriptor.inputs[member_port].default {
            PortDefault::Value(value) => value,
            _ => return None,
        },
        Some(from) => {
            let member = graph.nodes.get(&from.module)?;
            if member.descriptor.id != MEMBER_MODULE {
                return None;
            }
            if graph.assignments.values().any(|a| a.target == from.module) {
                return None;
            }
            *member.params.get(member.descriptor.param_index(MEMBER_PARAM)?)?
        }
    };
    let members = node
        .descriptor
        .param_index(SELECT_MEMBERS_PARAM)
        .and_then(|i| node.params.get(i).copied())
        .unwrap_or(inputs.len() as f32);
    let pick = value.round().clamp(1.0, members.max(1.0)) as usize;
    inputs.get(pick.checked_sub(1)?).copied().flatten()
}

/// A Sum's gain, by name.
fn sum_gain(node: &Node) -> Option<f32> {
    node.params.get(node.descriptor.param_index(SUM_GAIN_PARAM)?).copied()
}

/// What fades in the next schedule.
#[derive(Debug, Default)]
pub(crate) struct FadePlan {
    /// Inputs whose cable changed: each of their channels fades from what
    /// it read last.
    fresh: BTreeSet<InputRef>,
    /// Channels carried from the last schedule.
    carried: BTreeSet<u64>,
    /// Channels the last schedule read, for those that only it had.
    pub(super) old_reads: BTreeSet<u64>,
    /// Contributions that fade: (target, param, assignment) and whether it
    /// is fresh.
    live: BTreeMap<(ModuleId, usize, AssignmentId), bool>,
    /// Contributions that are leaving their parameter: (target, param) and
    /// each leaving assignment with whether it left in this compile.
    leaving: BTreeMap<(ModuleId, usize), BTreeMap<AssignmentId, bool>>,
    /// Contributions carried by fill key.
    carried_contributions: BTreeSet<(u64, AssignmentId)>,
    /// Fade length in frames.
    pub(super) length: u32,
}

impl FadePlan {
    /// Nothing fades: the first schedule after a load or a new engine.
    pub(crate) fn none(length: u32) -> Self {
        Self {
            length,
            ..Self::default()
        }
    }

    /// Compares `previous` with the graph in `layout`. `carried` are fades
    /// that may still run, and `done` the leaving Swap fades that have
    /// finished.
    pub(super) fn new(
        previous: &Wiring,
        layout: &Layout,
        compiled: &[CompiledAssignment],
        carried: &[FadeKey],
        done: &BTreeSet<ModuleId>,
        length: u32,
    ) -> Self {
        let mut plan = Self::none(length);
        let graph = layout.graph;
        let new_mixers: BTreeMap<ModuleId, Mixer> = graph
            .nodes
            .iter()
            .filter(|(id, _)| !previous.modules.contains(id))
            .filter_map(|(&id, node)| Some((id, mixer_of(graph, id, node)?)))
            .collect();
        // A cable that only rearranges: onto a new mixer that plays the old
        // source alone, or off a leaving one that played the new source
        // alone (rule A).
        let exact = |old: Option<OutputRef>, new: Option<OutputRef>| -> bool {
            let onto = new
                .and_then(|new| new_mixers.get(&new.module))
                .is_some_and(|mixer| old.is_some() && mixer.first == old);
            let off = old.is_some_and(|old| {
                !graph.nodes.contains_key(&old.module)
                    && previous.mixers.get(&old.module).is_some_and(|mixer| {
                        new.is_some()
                            && mixer.last == new
                            && (mixer.kind != MixerKind::SwapFade || done.contains(&old.module))
                    })
            });
            onto || off
        };
        // A Layer that adds members (rule B): a cable moved onto a new Sum
        // at gain 1 whose inputs hold its old source. The cable switches,
        // and the Sum's other inputs fade in below. Only cables: an
        // assignment moved onto such a Sum fades as any moved source does.
        let layer_sum = |old: Option<OutputRef>, new: Option<OutputRef>| -> Option<&Mixer> {
            let new = new?;
            new_mixers.get(&new.module).filter(|mixer| {
                mixer.kind == MixerKind::Sum
                    && old.is_some()
                    && mixer.inputs.contains(&old)
                    && sum_gain(&graph.nodes[&new.module]) == Some(1.0)
            })
        };
        let mut carried_types = Carried::default();
        for (v, node) in layout.nodes.iter().enumerate() {
            let id = layout.ids[v];
            if !previous.modules.contains(&id) {
                continue;
            }
            for port in 0..node.input_count() {
                let input = InputRef { module: id, port };
                let old = previous.cables.get(&input).copied();
                let new = graph.source_of(input);
                if old == new || exact(old, new) || layer_sum(old, new).is_some() {
                    continue;
                }
                if input_fades(layout, &mut carried_types, v, port, new)
                    && old.is_none_or(|old| input_fades_old(layout, &mut carried_types, v, port, old))
                {
                    plan.fresh.insert(input);
                }
            }
        }
        // A Layer that adds members (rule B): the consumer switches onto a
        // new Sum at gain 1 that holds its old source, and the Sum's other
        // inputs fade in from 0.
        for (&to, &from) in &graph.cables {
            let old = previous.cables.get(&to).copied();
            let Some(mixer) = layer_sum(old, Some(from)) else {
                continue;
            };
            for (port, source) in mixer.inputs.iter().enumerate() {
                if source.is_some() && *source != old {
                    let v = layout.index(from.module);
                    if input_fades(layout, &mut carried_types, v, port, *source) {
                        plan.fresh.insert(InputRef {
                            module: from.module,
                            port,
                        });
                    }
                }
            }
        }
        plan.old_reads = previous
            .reads
            .iter()
            .copied()
            .filter(|&key| {
                plan.fresh.contains(&InputRef {
                    module: ModuleId((key >> 32) as u32),
                    port: ((key >> 16) & 0xFFFF) as usize,
                })
            })
            .collect();
        for key in carried {
            match *key {
                FadeKey::Input(key) => {
                    plan.carried.insert(key);
                }
                FadeKey::Contribution(fill, id) => {
                    plan.carried_contributions.insert((fill, id));
                }
            }
        }
        plan.add_contributions(previous, layout, compiled, &exact);
        plan
    }

    /// Assignments added, removed, or moved (rule 9, Modulation
    /// assignments).
    fn add_contributions(
        &mut self,
        previous: &Wiring,
        layout: &Layout,
        compiled: &[CompiledAssignment],
        exact: &dyn Fn(Option<OutputRef>, Option<OutputRef>) -> bool,
    ) {
        let graph = layout.graph;
        // A parameter that takes a ramp: continuous, on a module that is
        // not new in this swap.
        let continuous = |module: ModuleId, param: usize| {
            previous.modules.contains(&module)
                && graph.nodes.get(&module).is_some_and(|node| {
                    node.unknown.is_none()
                        && node.descriptor.params.get(param).is_some_and(|spec| {
                            !spec.options.integer && spec.options.choices.is_empty() && !spec.options.shapes_graph
                        })
                })
        };
        for assignment in compiled {
            let target = layout.ids[assignment.target_node];
            let key = (target, assignment.param, assignment.id);
            let ramps = continuous(target, assignment.param);
            match previous.assignments.get(&assignment.id) {
                None if ramps => {
                    self.live.insert(key, true);
                }
                None => {}
                Some(&(_, old_target, param)) if (old_target, param) != (target, assignment.param) => {
                    // Moved to another target: out on the old one, even
                    // when the new one is a new part that applies it at
                    // once (a slot swap), and in on the new one.
                    if continuous(old_target, param) {
                        self.leaving
                            .entry((old_target, param))
                            .or_default()
                            .insert(assignment.id, true);
                    }
                    if ramps {
                        self.live.insert(key, true);
                    }
                }
                Some(&(source, ..))
                    if ramps && source != assignment.source && !exact(Some(source), Some(assignment.source)) =>
                {
                    self.live.insert(key, true);
                }
                Some(_) => {}
            }
        }
        let current: BTreeSet<AssignmentId> = compiled.iter().map(|a| a.id).collect();
        for (&id, &(_, target, param)) in &previous.assignments {
            if !current.contains(&id) && continuous(target, param) {
                self.leaving.entry((target, param)).or_default().insert(id, true);
            }
        }
    }

    /// True when channel `key` of `input` fades in the next schedule, and
    /// whether it starts fresh.
    pub(super) fn input(&self, input: InputRef, key: u64) -> Option<bool> {
        if self.fresh.contains(&input) {
            Some(true)
        } else if self.carried.contains(&key) {
            Some(false)
        } else {
            None
        }
    }

    /// The channels of `input` that the last schedule read or that may
    /// still be fading, so a voice only the old cable had keeps a reader
    /// while it fades out.
    pub(super) fn old_channels(&self, input: InputRef) -> impl Iterator<Item = usize> + '_ {
        let start = key_of(input, 0);
        let end = start | 0xFFFF;
        self.old_reads
            .range(start..=end)
            .chain(self.carried.range(start..=end))
            .map(|&key| (key & 0xFFFF) as usize)
    }

    /// How the contribution of assignment `id` on fill `fill` of
    /// `(target, param)` fades: `Some(fresh)` or `None` for no ramp.
    pub(super) fn contribution(&self, target: ModuleId, param: usize, id: AssignmentId, fill: u64) -> Option<bool> {
        match self.live.get(&(target, param, id)) {
            Some(&fresh) => Some(fresh),
            None => self.carried_contributions.contains(&(fill, id)).then_some(false),
        }
    }

    /// The assignments leaving `(target, param)`, with whether each starts
    /// fresh, for fill `fill`. Includes ones carried from the last
    /// schedule.
    pub(super) fn leaving(
        &self,
        target: ModuleId,
        param: usize,
        fill: u64,
        current: &BTreeSet<AssignmentId>,
    ) -> Vec<(AssignmentId, bool)> {
        let mut leaving: BTreeMap<AssignmentId, bool> = self.leaving.get(&(target, param)).cloned().unwrap_or_default();
        for &(key, id) in &self.carried_contributions {
            if key == fill && !current.contains(&id) {
                leaving.entry(id).or_insert(false);
            }
        }
        leaving.into_iter().collect()
    }

    /// Every parameter that has leaving contributions, so a fill is built
    /// for it even with no assignment left.
    pub(super) fn leaving_params(&self) -> impl Iterator<Item = (ModuleId, usize)> + '_ {
        self.leaving.keys().copied()
    }

    /// Fill keys with carried contributions.
    pub(super) fn carried_fills(&self) -> impl Iterator<Item = u64> + '_ {
        self.carried_contributions.iter().map(|&(fill, _)| fill)
    }
}

/// True when the old cable `old` into input `port` of the module at `v`
/// carried a signal that fades. When its source is gone from the graph,
/// only the input's own type is left to read.
fn input_fades_old(layout: &Layout, carried: &mut Carried, v: usize, port: usize, old: OutputRef) -> bool {
    if layout.index_of.contains_key(&old.module) {
        input_fades(layout, carried, v, port, Some(old))
    } else {
        input_fades(layout, carried, v, port, None)
    }
}

impl Wiring {
    /// The wiring of `layout`'s graph, as the schedule just built reads it.
    pub(super) fn of(layout: &Layout, reads: &[crate::compile::Read]) -> Self {
        let graph = layout.graph;
        Self {
            modules: layout.ids.iter().copied().collect(),
            cables: graph.cables.clone(),
            reads: reads.iter().map(|read| read.key).collect(),
            mixers: graph
                .nodes
                .iter()
                .filter_map(|(&id, node)| Some((id, mixer_of(graph, id, node)?)))
                .collect(),
            assignments: graph
                .assignments
                .iter()
                .map(|(&id, a)| (id, (a.source, a.target, a.param)))
                .collect(),
        }
    }
}

/// The input-channel key of channel `ch` of `input`.
pub(super) fn key_of(input: InputRef, ch: usize) -> u64 {
    input_key(input.module.0, input.port, ch)
}

#[cfg(test)]
mod tests;
