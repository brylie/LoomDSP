//! Steps 5 and 6: what each module reads besides its cables. A modulated
//! parameter reads a buffer filled from its sources, an input where stereo
//! meets mono reads a downmix, and an unconnected input reads its default.

use super::*;

/// A modulation assignment the graph can schedule.
pub(super) struct CompiledAssignment {
    pub(super) id: AssignmentId,
    pub(super) source: OutputRef,
    pub(super) target_node: usize,
    pub(super) param: usize,
    pub(super) depth: f32,
    pub(super) enabled: bool,
    pub(super) polarity: Option<Polarity>,
    pub(super) source_polarity: Polarity,
    /// The source is an unknown placeholder.
    pub(super) silent_source: bool,
}

/// Assignments the graph can actually schedule. Dangling ones are dropped.
pub(super) fn compile_assignments(layout: &Layout) -> Vec<CompiledAssignment> {
    layout
        .graph
        .assignments()
        .filter_map(|(id, assignment)| {
            let &target_node = layout.index_of.get(&assignment.target)?;
            let &source_node = layout.index_of.get(&assignment.source.module)?;
            let source = layout.nodes[source_node];
            let target = layout.nodes[target_node];
            let param_count = target
                .unknown
                .as_ref()
                .map_or(target.descriptor.params.len(), |unknown| unknown.parameters.len());
            if assignment.source.port >= source.output_count() || assignment.param >= param_count {
                return None;
            }
            Some(CompiledAssignment {
                id,
                source: assignment.source,
                target_node,
                param: assignment.param,
                depth: assignment.depth,
                enabled: assignment.enabled,
                polarity: assignment.polarity,
                source_polarity: source
                    .descriptor
                    .outputs
                    .get(assignment.source.port)
                    .map_or(Polarity::Bipolar, |port| port.polarity()),
                silent_source: source.unknown.is_some(),
            })
        })
        .collect()
}

/// Identifies one shared default buffer. Value defaults are keyed by their
/// bits, so each distinct value gets one buffer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum DefaultKey {
    Value(u32),
    BarPhase,
    BeatPhase,
    Playing,
}

impl DefaultKey {
    /// `None` for defaults that read the shared zero buffer. The pattern
    /// compares with `==`, so -0.0 reads it too.
    pub(super) fn of(default: PortDefault) -> Option<Self> {
        match default {
            PortDefault::Value(0.0) => None,
            PortDefault::Value(value) => Some(Self::Value(value.to_bits())),
            PortDefault::BarPhase => Some(Self::BarPhase),
            PortDefault::BeatPhase => Some(Self::BeatPhase),
            PortDefault::Playing => Some(Self::Playing),
        }
    }
}

/// One source of a modulated parameter.
pub(super) struct SourceSpec {
    /// Index into the compiled assignment list.
    pub(super) assignment: usize,
    /// Buffer index the fill reads. A delay tap when the assignment was broken.
    pub(super) buffer: usize,
}

/// The buffer of one modulated parameter, and the sources that fill it.
pub(super) struct FillSpec {
    pub(super) param: usize,
    pub(super) buffer: usize,
    pub(super) sources: Vec<SourceSpec>,
}

/// Mixes the two channels in `left` and `right` into `output`.
pub(super) struct DownmixSpec {
    pub(super) kind: DownmixKind,
    pub(super) left: usize,
    pub(super) right: usize,
    pub(super) output: usize,
}

/// What each module reads besides its cables, by buffer index.
pub(super) struct Inputs {
    /// Fills per module and copy.
    pub(super) fills_of: Vec<[Vec<FillSpec>; 2]>,
    /// Downmixes each module fills before it runs.
    pub(super) downmixes: Vec<Vec<DownmixSpec>>,
    /// Downmixes for the audio output, which run before the outputs are
    /// summed.
    pub(super) output_downmixes: Vec<DownmixSpec>,
    /// The downmix buffer an input reads instead of its stereo cable.
    pub(super) downmixed: BTreeMap<InputRef, usize>,
    /// One buffer per distinct default that an unconnected input reads.
    pub(super) defaults: BTreeMap<DefaultKey, usize>,
}

impl Inputs {
    pub(super) fn new(
        layout: &Layout,
        compiled: &[CompiledAssignment],
        plan: &Plan,
        live: &[bool],
        buffers: &mut Buffers,
    ) -> Self {
        let n = layout.ids.len();
        let mut inputs = Self {
            fills_of: (0..n).map(|_| [Vec::new(), Vec::new()]).collect(),
            downmixes: (0..n).map(|_| Vec::new()).collect(),
            output_downmixes: Vec::new(),
            downmixed: BTreeMap::new(),
            defaults: BTreeMap::new(),
        };
        inputs.add_fills(layout, compiled, plan, live, buffers);
        inputs.add_downmixes(layout, plan, buffers);
        inputs.add_defaults(layout, live, buffers);
        inputs
    }

    /// Step 5: one buffer per modulated parameter and copy. Sources in
    /// assignment id order.
    fn add_fills(
        &mut self,
        layout: &Layout,
        compiled: &[CompiledAssignment],
        plan: &Plan,
        live: &[bool],
        buffers: &mut Buffers,
    ) {
        let mut groups: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        for (i, assignment) in compiled.iter().enumerate() {
            let v = assignment.target_node;
            if live[v] && layout.nodes[v].unknown.is_none() && layout.is_dsp(v) {
                groups.entry((v, assignment.param)).or_default().push(i);
            }
        }
        for ((node, param), sources) in groups {
            for ch in 0..layout.channels(node) {
                let buffer = buffers.add();
                let sources = sources
                    .iter()
                    .map(|&i| {
                        let assignment = &compiled[i];
                        let buffer = if !layout.is_stereo(assignment.source) || layout.copied[node] {
                            plan.assignment_buffer(layout, assignment, ch)
                        } else {
                            // A stereo source into a module that runs as one
                            // gives the mean, because a parameter has no signal
                            // type (stereo spec, rule 17).
                            let output = buffers.add();
                            self.downmixes[node].push(DownmixSpec {
                                kind: DownmixKind::Mean,
                                left: plan.assignment_buffer(layout, assignment, 0),
                                right: plan.assignment_buffer(layout, assignment, 1),
                                output,
                            });
                            output
                        };
                        SourceSpec { assignment: i, buffer }
                    })
                    .collect();
                self.fills_of[node][ch].push(FillSpec { param, buffer, sources });
            }
        }
    }

    /// Where stereo meets an input of a module that runs as one, a downmix
    /// fills a buffer of its own (stereo spec, rule 11). A `Stereo` input
    /// reads both channels itself instead.
    fn add_downmixes(&mut self, layout: &Layout, plan: &Plan, buffers: &mut Buffers) {
        for v in (0..layout.ids.len()).filter(|&v| !layout.copied[v]) {
            for (port, spec) in layout.nodes[v].descriptor.inputs.iter().enumerate() {
                let input = InputRef {
                    module: layout.ids[v],
                    port,
                };
                let Some(&from) = layout.graph.cables.get(&input) else {
                    continue;
                };
                if !layout.is_stereo(from) || spec.channels == Channels::Stereo {
                    continue;
                }
                let output = buffers.add();
                let downmix = DownmixSpec {
                    kind: DownmixKind::of(spec.signal),
                    left: plan.input_buffer(layout, input, 0).expect("connected"),
                    right: plan.input_buffer(layout, input, 1).expect("connected"),
                    output,
                };
                match layout.nodes[v].descriptor.role {
                    ModuleRole::Dsp(_) => self.downmixes[v].push(downmix),
                    ModuleRole::AudioOutput => self.output_downmixes.push(downmix),
                    // A unit delay's input is `PerChannel`, so stereo there
                    // always copies it.
                    ModuleRole::UnitDelay => continue,
                }
                self.downmixed.insert(input, output);
            }
        }
    }

    /// Step 6: one buffer per distinct default that an unconnected input
    /// reads. Only DSP modules read their inputs through pointers.
    fn add_defaults(&mut self, layout: &Layout, live: &[bool], buffers: &mut Buffers) {
        for (v, node) in layout.nodes.iter().enumerate() {
            if !live[v] || !layout.is_dsp(v) {
                continue;
            }
            for port in 0..node.input_count() {
                let input = InputRef {
                    module: layout.ids[v],
                    port,
                };
                if layout.graph.cables.contains_key(&input) {
                    continue;
                }
                if let Some(key) = DefaultKey::of(layout.default_of(input)) {
                    self.defaults.entry(key).or_insert_with(|| buffers.add());
                }
            }
        }
    }
}
