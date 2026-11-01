//! The last step: allocate the buffers and turn buffer indices into the
//! pointers the engine reads.

use super::*;

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
    op_of_node: Vec<[u32; 2]>,
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
            op_of_node: vec![[u32::MAX; 2]; layout.ids.len()],
        }
    }

    /// Resolves every step and returns the finished schedule.
    pub(super) fn finish(
        mut self,
        compiled: &[CompiledAssignment],
        probes: &BTreeMap<ProbeId, (OutputRef, ProbeKind)>,
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
        self.schedule.sources = self.sources();

        let fills = &self.schedule.fills;
        let mut fill_order: Vec<u32> = (0..fills.len() as u32).collect();
        fill_order.sort_unstable_by_key(|&i| fills[i as usize].key);
        self.schedule.fill_order = fill_order;
        let mut copies: Vec<(u32, u32)> = (0..layout.ids.len())
            .filter(|&v| layout.copied[v] && layout.is_dsp(v))
            .map(|v| {
                let slot = |slot: Option<usize>| slot.expect("copied DSP modules have both slots") as u32;
                (slot(layout.nodes[v].slot), slot(layout.nodes[v].copy_slot))
            })
            .collect();
        copies.sort_unstable();
        self.schedule.copies = copies;
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
        if let Some(&buf) = self.inputs.downmixed.get(&input) {
            (self.ptr(buf), connected)
        } else if let Some(buf) = self.plan.input_buffer(self.layout, input, ch) {
            (self.ptr(buf), connected)
        } else if let Some(key) = DefaultKey::of(self.layout.default_of(input)) {
            (self.ptr(self.inputs.defaults[&key]), false)
        } else {
            (self.zero, false)
        }
    }

    /// The operation of copy `ch` of the module at `v`, made on first use.
    fn op(&mut self, v: usize, ch: usize) -> u32 {
        if self.op_of_node[v][ch] != u32::MAX {
            return self.op_of_node[v][ch];
        }
        let node = self.layout.nodes[v];
        let in_start = self.schedule.input_ptrs.len() as u32;
        let right_start = self.schedule.right_ptrs.len() as u32;
        let connected = self.add_inputs(v, ch);
        let out_start = self.schedule.output_ptrs.len() as u32;
        self.add_outputs(v, ch);
        // Only a module that runs as one has downmixes (rules 11 and 17).
        let downmix_start = self.schedule.downmixes.len() as u32;
        if ch == 0 {
            for spec in &self.inputs.downmixes[v] {
                let downmix = self.downmix(spec);
                self.schedule.downmixes.push(downmix);
            }
        }
        let slot = if ch == 1 {
            node.copy_slot
                .expect("the controller gives every copied DSP module a copy slot")
        } else {
            node.slot.expect("DSP modules have a slot")
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
            fills: (fill_start, s.fills.len() as u32),
            params: (param_start, s.param_ptrs.len() as u32),
            downmixes: (downmix_start, s.downmixes.len() as u32),
            right: (right_start, s.right_ptrs.len() as u32),
            channel: ch as u8,
        });
        self.op_of_node[v][ch] = (s.ops.len() - 1) as u32;
        self.op_of_node[v][ch]
    }

    /// Adds the input pointers of copy `ch` of the module at `v`. Returns a
    /// bit for each input with a cable.
    fn add_inputs(&mut self, v: usize, ch: usize) -> u64 {
        let layout = self.layout;
        let node = layout.nodes[v];
        let mut connected = 0u64;
        for port in 0..node.input_count() {
            let input = InputRef {
                module: layout.ids[v],
                port,
            };
            let (ptr, is_connected) = self.resolve_input(input, ch);
            self.schedule.input_ptrs.push(ptr);
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
                let right = self.plan.input_buffer(layout, input, 1).expect("connected");
                let ptr = self.ptr(right);
                self.schedule.right_ptrs.push(RightPtr {
                    port: port as u16,
                    output: false,
                    ptr,
                });
            }
        }
        connected
    }

    /// Adds the output pointers of copy `ch` of the module at `v`.
    fn add_outputs(&mut self, v: usize, ch: usize) {
        let layout = self.layout;
        let node = layout.nodes[v];
        for port in 0..node.output_count() {
            let right = layout.right_buf[v][port];
            let buffer = if ch == 1 {
                right.expect("a copied module's outputs are stereo")
            } else {
                layout.out_buf[v][port]
            };
            let ptr = self.ptr(buffer);
            self.schedule.output_ptrs.push(ptr);
            if node
                .descriptor
                .outputs
                .get(port)
                .is_some_and(|spec| spec.channels == Channels::Stereo)
            {
                let ptr = self.ptr(right.expect("a Stereo output is stereo"));
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
                key: (u64::from(layout.ids[v].0) << 32) | (spec.param as u64) | if ch == 1 { RIGHT_FILL } else { 0 },
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
                    output_key(source)
                        | if taps[i].channel == 1 && self.layout.is_stereo(source) {
                            RIGHT_TAP
                        } else {
                            0
                        }
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
        self.plan
            .output_nodes
            .iter()
            .filter_map(|&v| {
                let from = *layout.graph.cables.get(&InputRef {
                    module: layout.ids[v],
                    port: 0,
                })?;
                Some(OutputOp {
                    left: self.ptr(layout.channel_buffer(from, 0)) as *const f32,
                    right: self.ptr(layout.channel_buffer(from, 1)) as *const f32,
                })
            })
            .collect()
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
        sources.sort_unstable_by_key(|&(key, _)| key);
        sources
    }
}
