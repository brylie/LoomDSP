//! The last step: allocate the buffers and turn buffer indices into the
//! pointers the engine reads.

use super::*;
use crate::names::ModuleNames;

/// Builds the [`Schedule`] from the plan. Operations are made the first time
/// a step needs them, so they are stored in the order they run.
pub(super) struct Builder<'l, 'a> {
    layout: &'l Layout<'a>,
    plan: &'l Plan,
    inputs: &'l Inputs,
    sample_rate: f32,
    /// The schedule so far. It owns the buffers the pointers point into.
    schedule: Schedule,
    /// A pointer to each buffer, by index.
    ptrs: Vec<*mut f32>,
    zero: *const f32,
    /// The operation of each module and copy, once made.
    op_of_node: Vec<Vec<u32>>,
}

impl<'l, 'a> Builder<'l, 'a> {
    /// Allocates `buffers` buffers of `max_block` samples.
    pub(super) fn new(
        layout: &'l Layout<'a>,
        plan: &'l Plan,
        inputs: &'l Inputs,
        buffers: Buffers,
        max_block: usize,
        sample_rate: f32,
    ) -> Self {
        let mut schedule = Schedule::empty();
        schedule._buffers = (0..buffers.0)
            .map(|_| vec![0.0; max_block].into_boxed_slice())
            .collect();
        for (key, &buf) in &inputs.defaults {
            if let DefaultKey::Value(bits) = *key {
                // Filled once here. Inputs are read-only, so nothing changes it.
                schedule._buffers[buf].fill(f32::from_bits(bits));
            }
        }
        schedule._zero = vec![0.0; max_block].into_boxed_slice();
        let zero = schedule._zero.as_mut_ptr();
        let ptrs = schedule._buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
        Self {
            layout,
            plan,
            inputs,
            sample_rate,
            schedule,
            ptrs,
            zero,
            op_of_node: (0..layout.ids.len())
                .map(|v| vec![u32::MAX; layout.channels(v)])
                .collect(),
        }
    }

    /// Resolves every step and returns the finished schedule.
    pub(super) fn finish(
        mut self,
        compiled: &[CompiledAssignment],
        probes: &BTreeMap<ProbeId, (OutputRef, ProbeKind)>,
        pairs: &BTreeMap<ProbeId, (PairProbe, bool)>,
        midi_synced: &BTreeMap<MidiMappingId, u32>,
    ) -> Schedule {
        let layout = self.layout;
        self.schedule.transport = self.transport();
        let tap_index = self.add_taps();
        self.add_steps(&tap_index);
        let start = self.schedule.downmixes.len() as u32;
        for spec in &self.inputs.output_downmixes {
            let downmix = self.downmix(spec);
            self.schedule.downmixes.push(downmix);
        }
        self.schedule.output_downmixes = (start, self.schedule.downmixes.len() as u32);
        self.schedule.outputs = self.outputs();
        self.schedule.probes = self.probes(probes);
        self.schedule.pairs = self.pairs(pairs);
        self.schedule.sources = self.sources();

        let fills = &self.schedule.fills;
        let mut fill_order: Vec<u32> = (0..fills.len() as u32).collect();
        fill_order.sort_unstable_by_key(|&i| fills[i as usize].key);
        self.schedule.fill_order = fill_order;
        let mut copies = Vec::new();
        let mut stereo_copies = Vec::new();
        for (v, node) in layout.nodes.iter().enumerate() {
            if let Some(original) = node.slot {
                let id = layout.ids[v];
                let sides = if layout.widths.per_channel.contains(&id) { 2 } else { 1 };
                for (&(voice, side), &slot) in &node.copy_slots {
                    if voice >= layout.widths.voices[&id] || side >= sides {
                        continue;
                    }
                    copies.push((original as u32, slot as u32));
                    if side == 1 {
                        let left = if voice == 0 {
                            original
                        } else {
                            node.copy_slots[&(voice, 0)]
                        };
                        stereo_copies.push((left as u32, slot as u32));
                    }
                }
            }
        }
        copies.sort_unstable();
        stereo_copies.sort_unstable();
        self.schedule.copies = copies;
        self.schedule.stereo_copies = stereo_copies;
        for node in &layout.nodes {
            if let Some(slot) = node.slot {
                for (index, spec) in node.descriptor.params.iter().enumerate() {
                    if spec.options.shapes_graph {
                        self.schedule
                            .structural_params
                            .push((slot as u32, index as u16, node.params[index]));
                    }
                }
            }
        }
        self.schedule.assignments = compiled
            .iter()
            .map(|assignment| AssignmentRuntime {
                id: assignment.id,
                depth: assignment.depth,
                enabled: assignment.enabled,
                polarity: assignment.polarity,
                source_polarity: assignment.source_polarity,
                silent_source: assignment.silent_source,
            })
            .collect();
        self.schedule.midi_maps = compile_midi_maps(layout.graph, midi_synced);
        self.schedule
    }

    /// The pointer to buffer `buffer`.
    fn ptr(&self, buffer: usize) -> *mut f32 {
        self.ptrs[buffer]
    }

    fn downmix(&self, spec: &DownmixSpec) -> DownmixOp {
        DownmixOp {
            kind: spec.kind,
            left: self.ptr(spec.left),
            right: self.ptr(spec.right),
            output: self.ptr(spec.output),
        }
    }

    /// The shared transport buffers, and what the Transport module needs.
    fn transport(&self) -> TransportBuffers {
        let graph = self.layout.graph;
        let buffer = |key: DefaultKey| self.inputs.defaults.get(&key).map(|&buf| self.ptr(buf));
        // The Transport module, if any. The controller keeps a patch to one.
        let transport_node = graph
            .nodes
            .iter()
            .find(|(_, node)| node.descriptor.id == crate::transport::TRANSPORT_MODULE);
        TransportBuffers {
            bar_phase: buffer(DefaultKey::BarPhase),
            beat_phase: buffer(DefaultKey::BeatPhase),
            playing: buffer(DefaultKey::Playing),
            module: transport_node.and_then(|(_, node)| node.slot).map(|slot| slot as u32),
            grid: transport_node.is_some_and(|(&id, _)| graph.cables.keys().any(|to| to.module == id)),
            run_cable: transport_node.is_some_and(|(&id, node)| {
                node.descriptor
                    .input_index("run")
                    .is_some_and(|port| graph.cables.contains_key(&InputRef { module: id, port }))
            }),
        }
    }

    /// The buffer channel `ch` of `input` reads, and whether a cable is
    /// plugged into it.
    fn resolve_input(&self, input: InputRef, ch: usize) -> (*const f32, bool) {
        let connected = self.layout.graph.cables.contains_key(&input);
        if let Some(&buf) = self.inputs.downmixed.get(&(input, ch / 2)) {
            (self.ptr(buf), connected)
        } else if let Some(buf) = self.plan.input_buffer(self.layout, input, ch) {
            (self.ptr(buf), connected)
        } else if let Some(key) = DefaultKey::of(self.layout.default_of(input)) {
            (self.ptr(self.inputs.defaults[&key]), connected)
        } else {
            (self.zero, connected)
        }
    }

    /// The operation of copy `ch` of the module at `v`, made on first use.
    fn op(&mut self, v: usize, ch: usize) -> u32 {
        if self.op_of_node[v][ch] != u32::MAX {
            return self.op_of_node[v][ch];
        }
        let node = self.layout.nodes[v];
        let channel = self.layout.copy_channel(v, ch);
        let bundle_start = self.schedule.bundle_ports.len() as u32;
        let in_start = self.schedule.input_ptrs.len() as u32;
        let right_start = self.schedule.right_ptrs.len() as u32;
        let (connected, defaulted) = self.add_inputs(v, channel);
        let out_start = self.schedule.output_ptrs.len() as u32;
        self.add_outputs(v, channel);
        // Only a module that runs as one has downmixes (rules 11 and 17).
        let downmix_start = self.schedule.downmixes.len() as u32;
        if ch == 0 {
            for spec in &self.inputs.downmixes[v] {
                let downmix = self.downmix(spec);
                self.schedule.downmixes.push(downmix);
            }
        }
        let slot = if channel == 0 {
            node.slot.expect("DSP modules have a slot")
        } else {
            node.copy_slots[&(channel / 2, channel % 2)]
        } as u32;
        let fill_start = self.schedule.fills.len() as u32;
        let param_start = self.schedule.param_ptrs.len() as u32;
        self.add_fills(v, ch, slot);
        let s = &mut self.schedule;
        s.ops.push(ModuleOp {
            slot,
            inputs: (in_start, s.input_ptrs.len() as u32),
            outputs: (out_start, s.output_ptrs.len() as u32),
            connected,
            defaulted,
            fills: (fill_start, s.fills.len() as u32),
            params: (param_start, s.param_ptrs.len() as u32),
            downmixes: (downmix_start, s.downmixes.len() as u32),
            right: (right_start, s.right_ptrs.len() as u32),
            channel: (channel % 2) as u8,
            voice: (channel / 2) as u8,
            voices: self.layout.widths.voices[&self.layout.ids[v]] as u8,
            bundles: (bundle_start, s.bundle_ports.len() as u32),
        });
        self.op_of_node[v][ch] = (s.ops.len() - 1) as u32;
        self.op_of_node[v][ch]
    }

    /// Adds the input pointers of copy `ch` of the module at `v`. Returns a
    /// bit for each input with a cable.
    fn add_inputs(&mut self, v: usize, ch: usize) -> (u64, u64) {
        let layout = self.layout;
        let node = layout.nodes[v];
        let mut connected = 0u64;
        let mut defaulted = 0u64;
        for port in 0..node.input_count() {
            let input = InputRef {
                module: layout.ids[v],
                port,
            };
            let (ptr, is_connected) = self.resolve_input(input, ch);
            if self.plan.input_buffer(layout, input, ch).is_none() {
                defaulted |= 1 << port;
            }
            self.schedule.input_ptrs.push(ptr);
            if node
                .descriptor
                .inputs
                .iter()
                .chain(node.descriptor.outputs)
                .any(|spec| spec.channels == Channels::Voices)
            {
                let width = if node.descriptor.inputs[port].channels == Channels::Voices {
                    layout
                        .graph
                        .source_of(input)
                        .and_then(|from| layout.widths.outputs.get(&from).copied())
                        .unwrap_or_default()
                } else {
                    Width::MONO
                };
                let mut channels = Vec::with_capacity(width.voices * width.sides);
                for voice in 0..width.voices {
                    for side in 0..width.sides {
                        let channel = if node.descriptor.inputs[port].channels == Channels::Voices {
                            voice * 2 + side
                        } else {
                            ch
                        };
                        channels.push(self.resolve_input(input, channel).0 as *mut f32);
                    }
                }
                self.schedule.bundle_ports.push(crate::module::BundlePort {
                    width,
                    channels: channels.into_boxed_slice(),
                });
            }
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
                && layout
                    .graph
                    .cables
                    .get(&input)
                    .is_some_and(|&from| layout.is_stereo(from))
            {
                let (ptr, _) = self.resolve_input(input, ch / 2 * 2 + 1);
                self.schedule.right_ptrs.push(RightPtr {
                    port: port as u16,
                    output: false,
                    ptr: ptr as *mut f32,
                });
            }
        }
        (connected, defaulted)
    }

    /// Adds the output pointers of copy `ch` of the module at `v`.
    fn add_outputs(&mut self, v: usize, ch: usize) {
        let layout = self.layout;
        let node = layout.nodes[v];
        for port in 0..node.output_count() {
            let from = OutputRef {
                module: layout.ids[v],
                port,
            };
            let buffer = layout.channel_buffer(from, ch);
            let ptr = self.ptr(buffer);
            self.schedule.output_ptrs.push(ptr);
            if node
                .descriptor
                .inputs
                .iter()
                .chain(node.descriptor.outputs)
                .any(|spec| spec.channels == Channels::Voices)
            {
                let width = layout.widths.outputs[&from];
                let channels = layout
                    .output_channels(from)
                    .map(|channel| self.ptr(layout.channel_buffer(from, channel)))
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                self.schedule
                    .bundle_ports
                    .push(crate::module::BundlePort { width, channels });
            }
            if node
                .descriptor
                .outputs
                .get(port)
                .is_some_and(|spec| spec.channels == Channels::Stereo)
            {
                let ptr = self.ptr(layout.channel_buffer(from, ch / 2 * 2 + 1));
                self.schedule.right_ptrs.push(RightPtr {
                    port: port as u16,
                    output: true,
                    ptr,
                });
            }
        }
    }

    /// Adds the parameter fills of copy `ch` of the module at `v`.
    fn add_fills(&mut self, v: usize, ch: usize, slot: u32) {
        let layout = self.layout;
        let node = layout.nodes[v];
        for spec in &self.inputs.fills_of[v][ch] {
            let param_spec = node.spec(spec.param).expect("fills name real parameters");
            let base = node.params.get(spec.param).copied().unwrap_or(param_spec.default);
            let smooth = (param_spec.options.smooth_seconds > 0.0).then(|| {
                Smoothed::new(
                    param_spec.normalize(base),
                    self.sample_rate,
                    param_spec.options.smooth_seconds,
                )
            });
            let sources = spec
                .sources
                .iter()
                .map(|source| ModSource {
                    state: source.assignment as u32,
                    buffer: self.ptr(source.buffer) as *const f32,
                })
                .collect();
            let buffer = self.ptr(spec.buffer);
            self.schedule.param_ptrs.push((spec.param as u16, buffer as *const f32));
            self.schedule.fills.push(ParamFill {
                key: (u64::from(layout.ids[v].0) << 32)
                    | (spec.param as u64)
                    | ((layout.copy_channel(v, ch) / 2) as u64) << 24
                    | if layout.copy_channel(v, ch) % 2 == 1 {
                        RIGHT_FILL
                    } else {
                        0
                    },
                slot,
                param: spec.param as u16,
                spec: param_spec,
                base,
                smooth,
                buffer,
                sources,
                live: crate::live::NO_CELL,
            });
        }
    }

    /// Adds the taps, sorted by key. Returns each planned tap's index in the
    /// sorted list.
    fn add_taps(&mut self) -> Vec<u32> {
        let taps = &self.plan.taps;
        let mut tap_order: Vec<usize> = (0..taps.len()).collect();
        tap_order.sort_by_key(|&i| taps[i].key);
        let mut tap_index = vec![0u32; taps.len()];
        for (sorted, &original) in tap_order.iter().enumerate() {
            tap_index[original] = sorted as u32;
        }
        self.schedule.taps = tap_order
            .iter()
            .map(|&i| Tap {
                key: taps[i].key,
                source: taps[i].source.map_or(NO_SOURCE, |source| {
                    let width = self.layout.widths.outputs.get(&source).copied().unwrap_or_default();
                    output_key(source)
                        | copy_tap(
                            (if width.voices == 1 { 0 } else { taps[i].channel / 2 }) * 2
                                + if width.sides == 2 { taps[i].channel % 2 } else { 0 },
                        )
                }),
                input: taps[i].input.map_or(self.zero, |b| self.ptr(b) as *const f32),
                output: self.ptr(taps[i].output),
                state: 0.0,
            })
            .collect();
        tap_index
    }

    /// Resolves the planned steps, making each operation as it is reached.
    fn add_steps(&mut self, tap_index: &[u32]) {
        let layout = self.layout;
        for step in &self.plan.steps {
            match step {
                // A module that runs per channel runs its left copy, then its
                // right. They share no state, so the order does not matter.
                StepSpec::Module(v) => {
                    for ch in 0..layout.channels(*v) {
                        let op = self.op(*v, ch);
                        self.schedule.steps.push(Step::Module(op));
                    }
                }
                StepSpec::Tap(t) => self.schedule.steps.push(Step::Tap(tap_index[*t])),
                StepSpec::Loop { modules, taps } => {
                    let ops_start = self.schedule.loop_ops.len() as u32;
                    for &v in modules {
                        for ch in 0..layout.channels(v) {
                            let op = self.op(v, ch);
                            self.schedule.loop_ops.push(op);
                        }
                    }
                    let s = &mut self.schedule;
                    let taps_start = s.loop_taps.len() as u32;
                    s.loop_taps.extend(taps.iter().map(|&t| tap_index[t]));
                    s.steps.push(Step::Loop {
                        ops: (ops_start, s.loop_ops.len() as u32),
                        taps: (taps_start, s.loop_taps.len() as u32),
                    });
                }
            }
        }
    }

    /// What each audio output module plays, from the cable into it.
    fn outputs(&self) -> Vec<OutputOp> {
        let layout = self.layout;
        let mut outputs = Vec::new();
        for &v in &self.plan.output_nodes {
            let Some(from) = layout.graph.source_of(InputRef {
                module: layout.ids[v],
                port: 0,
            }) else {
                continue;
            };
            let width = layout.widths.outputs.get(&from).copied().unwrap_or_default();
            for voice in 0..width.voices {
                outputs.push(OutputOp {
                    left: self.ptr(layout.channel_buffer(from, voice * 2)) as *const f32,
                    right: self.ptr(layout.channel_buffer(from, voice * 2 + 1)) as *const f32,
                });
            }
        }
        outputs
    }

    /// The probes on outputs that exist.
    fn probes(&self, probes: &BTreeMap<ProbeId, (OutputRef, ProbeKind)>) -> Vec<ProbeOp> {
        let layout = self.layout;
        probes
            .iter()
            .filter_map(|(&id, &(from, kind))| {
                let v = *layout.index_of.get(&from.module)?;
                let buffer = *layout.out_buf.get(v)?.get(from.port)?;
                Some(ProbeOp {
                    id,
                    buffer: self.ptr(buffer) as *const f32,
                    right: layout.right_buf[v][from.port].map(|buffer| self.ptr(buffer) as *const f32),
                    kind,
                })
            })
            .collect()
    }

    /// The paired probes on outputs that exist. The flag says `y` has no cable
    /// in it, so a stereo `x` is read as its two sides.
    fn pairs(&self, pairs: &BTreeMap<ProbeId, (PairProbe, bool)>) -> Vec<PairOp> {
        let layout = self.layout;
        let side = |from: OutputRef| {
            let v = *layout.index_of.get(&from.module)?;
            let buffer = *layout.out_buf.get(v)?.get(from.port)?;
            let right = layout.right_buf[v][from.port];
            Some((
                self.ptr(buffer) as *const f32,
                right.map(|buffer| self.ptr(buffer) as *const f32),
            ))
        };
        pairs
            .iter()
            .filter_map(|(&id, &(pair, y_empty))| {
                let (x, x_right) = side(pair.x)?;
                let (y, y_right) = side(pair.y)?;
                Some(match x_right {
                    Some(right) if y_empty => PairOp {
                        id,
                        x,
                        x_right: None,
                        y: right,
                        y_right: None,
                    },
                    _ => PairOp {
                        id,
                        x,
                        x_right,
                        y,
                        y_right,
                    },
                })
            })
            .collect()
    }

    /// Every output's buffer by key, the right channel of a stereo output
    /// marked, sorted by key.
    fn sources(&self) -> Vec<(u64, *const f32)> {
        let layout = self.layout;
        let mut sources = Vec::new();
        for (v, ports) in layout.out_buf.iter().enumerate() {
            for (port, &buffer) in ports.iter().enumerate() {
                let key = output_key(OutputRef {
                    module: layout.ids[v],
                    port,
                });
                sources.push((key, self.ptr(buffer) as *const f32));
            }
        }
        for (v, ports) in layout.right_buf.iter().enumerate() {
            for (port, buffer) in ports.iter().enumerate() {
                if let Some(buffer) = buffer {
                    let key = output_key(OutputRef {
                        module: layout.ids[v],
                        port,
                    });
                    sources.push((key | RIGHT_TAP, self.ptr(*buffer) as *const f32));
                }
            }
        }
        for (&(from, channel), &buffer) in &layout.voice_buf {
            sources.push((output_key(from) | copy_tap(channel), self.ptr(buffer) as *const f32));
        }
        sources.sort_unstable_by_key(|&(key, _)| key);
        sources
    }
}
