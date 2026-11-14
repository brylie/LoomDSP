//! The control-thread side of the engine.
//!
//! [`Controller`] has one flat list of methods, grouped by what they edit.
//! Each group is an `impl Controller` block in its own child module, with a
//! heading that the API docs show as a section:
//!
//! | Group | File |
//! |---|---|
//! | The graph, commit, and engine state | this file |
//! | Modules | `controller/modules.rs` |
//! | Cables | `controller/cables.rs` |
//! | Modulation assignments | `controller/assignments.rs` |
//! | Parameters | `controller/params.rs` |
//! | MIDI mappings, learn, and inputs | `controller/midi.rs` |
//! | Transport | `controller/transport.rs` |
//! | Display values and probes | `controller/displays.rs` |
//! | Audio output and input | `controller/audio.rs` |
//!
//! State with rules of its own is a typed part with private fields, owned
//! by the controller: `Slots` (in `modules.rs`), `MidiSync` (`midi.rs`), and
//! `Probes` and `LiveValues` (`displays.rs`). The methods coordinate parts
//! and the graph, borrowing them as separate fields. The graph and the
//! command queue stay shared, because nearly every edit uses both. Helpers
//! that more than one group uses, such as `send` and the `check_*`
//! functions, stay in this file, because a child module cannot see its
//! siblings' private items. Why: ADR-0034.

use crate::compile::{CompileReport, FadeKey, Fades, Wiring, compile_with_fades};
use crate::descriptor::{ModuleConfig, ModuleDescriptor, ModuleRole, ParamRange, Polarity};
use crate::display::{DisplayReader, DisplaySource, DisplayTable};
use crate::engine::{Command, Engine, EngineConfig, EngineStats, Garbage};
use crate::error::LoomError;
use crate::graph::{Assignment, AssignmentId, Graph, InputRef, ModuleId, Node, OutputRef};
use crate::live::{LiveCells, LiveTable, NO_CELL};
use crate::midi::MidiEvent;
use crate::midi_map::{
    ControlChangeSeen, ControlOutcome, ControlState, MidiControl, MidiFeedback, MidiMapping, MidiMappingId,
    MidiMappingOptions, MidiReader, MidiWaiting, later,
};
use crate::names::ModuleNames;
use crate::registry::Registry;
use crate::safety::OutputSafety;
use crate::telemetry::{PairProbe, ProbeId, ProbeKind, Telemetry, TelemetrySender};
use crate::transport::{TimeSignature, TransportSettings, clamp_tempo};
use crate::width;
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// The handles returned by [`create_engine`].
pub struct EngineParts {
    /// Edits the graph. Keep it on a control thread.
    pub controller: Controller,
    /// Renders audio. Move it to the audio thread.
    pub engine: Engine,
    /// Sends MIDI to the engine. Move it to the MIDI input thread.
    pub midi: MidiSender,
    /// Meter and scope data from the engine. Read it on a control thread.
    pub telemetry: Telemetry,
}

/// Creates a connected controller, engine, MIDI sender, and telemetry reader.
pub fn create_engine(registry: Arc<Registry>, config: EngineConfig) -> EngineParts {
    let (command_tx, command_rx) = RingBuffer::new(config.command_capacity);
    // Every command can produce at most one piece of garbage, so a garbage
    // queue as large as the command queue cannot overflow as long as the
    // controller drains it before sending more commands.
    let (garbage_tx, garbage_rx) = RingBuffer::new(config.command_capacity);
    let (midi_tx, midi_rx) = RingBuffer::new(config.midi_capacity);
    let (meter_tx, meter_rx) = RingBuffer::new(config.meter_capacity);
    let (scope_tx, scope_rx) = RingBuffer::new(config.scope_capacity);
    let (pair_tx, pair_rx) = RingBuffer::new(config.pair_capacity);
    let stats = Arc::new(EngineStats::default());
    let displays = Arc::new(DisplayTable::new(config.max_modules));
    let live = Arc::new(LiveTable::new(config.max_live_values));
    let midi_feedback = Arc::new(MidiFeedback::new());
    let engine = Engine::new(
        config,
        command_rx,
        garbage_tx,
        (midi_rx, midi_feedback.clone()),
        TelemetrySender {
            meters: meter_tx,
            scope: scope_tx,
            pairs: pair_tx,
        },
        (displays.clone(), live.clone()),
        stats.clone(),
    );
    let controller = Controller {
        registry,
        graph: Graph::default(),
        commands: command_tx,
        garbage: garbage_rx,
        slots: Slots::new(config.max_modules),
        midi: MidiSync::new(midi_feedback),
        probes: Probes::new(),
        live: LiveValues::new(displays, live, config.max_live_values),
        next_id: 1,
        next_assignment: 1,
        dirty: false,
        revision: 0,
        last_report: CompileReport::default(),
        transport: TransportSettings::default(),
        config,
        stats,
        cable_fades: CableFades::default(),
    };
    EngineParts {
        controller,
        engine,
        midi: MidiSender { tx: midi_tx },
        telemetry: Telemetry::new(meter_rx, scope_rx, pair_rx),
    }
}

/// Sends MIDI events to the engine from any single thread.
pub struct MidiSender {
    tx: Producer<MidiEvent>,
}

impl MidiSender {
    /// Queues an event. Returns false if the queue is full (event dropped).
    pub fn send(&mut self, event: MidiEvent) -> bool {
        self.tx.push(event).is_ok()
    }
}

/// A module added since the last commit, with what the engine needs to
/// take it in.
struct Fresh {
    module: Box<dyn crate::Module>,
    /// The module's ID, which owns its display values.
    id: u32,
    /// How many display values it publishes.
    displays: u8,
    /// Bit `i` set when display value `i` is a trigger.
    triggers: u8,
}

/// Edits the graph and keeps the engine in sync.
///
/// Structural edits (add, remove, connect, disconnect, add or remove a
/// modulation assignment) take effect when you call [`Controller::commit`].
/// Batch several edits, then commit once. Parameter changes, and changes to
/// an assignment's depth, polarity, or enabled flag, take effect at the next
/// buffer without a recompile.
pub struct Controller {
    registry: Arc<Registry>,
    graph: Graph,
    commands: Producer<Command>,
    garbage: Consumer<Garbage>,
    /// The engine's module slots: free, held by new modules, or retiring.
    slots: Slots,
    /// MIDI mapping ids, and the order of screen edits and control changes.
    midi: MidiSync,
    /// The ports the UI watches.
    probes: Probes,
    /// Display values and live modulated values from the engine.
    live: LiveValues,
    next_id: u32,
    next_assignment: u32,
    dirty: bool,
    /// Goes up by one with every structural edit; see
    /// [`Controller::revision`].
    revision: u64,
    last_report: CompileReport,
    /// What the engine's transport is set to, as of the last command sent.
    transport: TransportSettings,
    config: EngineConfig,
    stats: Arc<EngineStats>,
    /// What the next compile needs to fade the changes since the last one.
    cable_fades: CableFades,
}

/// What the last schedule sent was compiled from, and the fades that may
/// still run in it (live patching spec, rule 9). Updated only after the
/// engine was sent the schedule.
#[derive(Default)]
struct CableFades {
    /// `None` before the first schedule, so nothing fades after a load.
    wiring: Option<Wiring>,
    /// The generation of the last schedule sent.
    generation: u64,
    /// Every fade and contribution ramp in that schedule.
    fading: Vec<FadeKey>,
}

impl CableFades {
    /// The fades the next schedule carries: none once the engine says the
    /// last schedule's fades have all ended.
    fn carried(&self, stats: &EngineStats) -> &[FadeKey] {
        if stats.cable_fades_settled.load(Ordering::Relaxed) >= self.generation {
            &[]
        } else {
            &self.fading
        }
    }
}

/// # The graph, commit, and engine state
///
/// What the controller holds, [`commit`](Controller::commit), which sends
/// structural edits to the engine, and the helpers every group uses.
impl Controller {
    /// The module registry.
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// How many fades and contribution ramps of the last schedule sent the
    /// controller still keeps, because the engine has not said they ended
    /// (live patching spec, rule 9). For tests and diagnostics.
    pub fn cable_fades_in_flight(&self) -> usize {
        self.cable_fades.carried(&self.stats).len()
    }

    /// The current graph (including uncommitted edits).
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Engine settings.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Engine counters.
    pub fn stats(&self) -> &Arc<EngineStats> {
        &self.stats
    }

    /// The report from the last successful [`Controller::commit`].
    pub fn last_report(&self) -> &CompileReport {
        &self.last_report
    }

    /// True if there are structural or probe edits not yet committed.
    pub fn has_uncommitted_changes(&self) -> bool {
        self.dirty
    }

    /// A count that goes up with every structural or probe edit, committed
    /// or not, including graph-shaping parameter edits. Ordinary parameter
    /// values do not change it. Two equal readings mean
    /// the graph's structure did not change between them, so a caller that
    /// derives something from it (the editor's sync of stack copies,
    /// ADR-0040) can skip the work.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Marks the graph changed: it needs a commit, and its revision moves on.
    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    fn send(&mut self, command: Command) -> Result<(), LoomError> {
        self.commands.push(command).map_err(|_| LoomError::QueueFull)
    }

    /// Compiles the graph and sends it to the engine. The engine switches to
    /// it at the start of its next buffer, with no dropout (FR-GRAPH-02).
    pub fn commit(&mut self) -> Result<&CompileReport, LoomError> {
        self.collect_garbage();
        // The compiled base values come from the graph, so it must hold what
        // MIDI mappings set in the engine first.
        self.sync_midi();
        self.commit_synced()
    }

    /// Compiles a graph whose MIDI feedback was already reconciled before
    /// staging edits. Importing feedback again would overwrite staged values.
    fn commit_synced(&mut self) -> Result<&CompileReport, LoomError> {
        if !self.dirty {
            return Ok(&self.last_report);
        }
        // Prepare voice/side copies before the swap. Removed copies retire
        // after the new schedule no longer names their slots.
        let widths = width::infer(&self.graph);
        let mut starting = Vec::new();
        let mut stopping = Vec::new();
        for (&id, node) in &self.graph.nodes {
            if !matches!(node.descriptor.role, ModuleRole::Dsp(_)) {
                continue;
            }
            let sides = if widths.per_channel.contains(&id) { 2 } else { 1 };
            for voice in 0..widths.voices[&id] {
                for side in 0..sides {
                    let key = (voice, side);
                    if key != (0, 0) && !node.copy_slots.contains_key(&key) {
                        starting.push((id, key));
                    }
                }
            }
            for (&key, &slot) in &node.copy_slots {
                if key.0 >= widths.voices[&id] || key.1 >= sides {
                    stopping.push((id, key, slot));
                }
            }
        }
        if self.slots.free_count() < starting.len() {
            return Err(LoomError::TooManyModules(self.config.max_modules));
        }
        // New modules go in first, after `reset()` snaps their smoothing to
        // the values set since they were added (live patching spec, rule
        // 7). An instance the schedule does not use yet changes nothing, so
        // when they do not all fit with the swap, the ones that fit go now
        // and the rest wait for the next commit. Their later edits are
        // commands, as for any module in the engine.
        let room = self.commands.slots().saturating_sub(1 + self.slots.retiring().len());
        let waiting = self.slots.waiting().saturating_sub(room);
        for (slot, mut fresh) in self.slots.send_fresh(room) {
            fresh.module.reset();
            let insert = Command::InsertModule {
                slot,
                module: fresh.module,
                id: fresh.id,
                displays: fresh.displays,
                triggers: fresh.triggers,
            };
            let pushed = self.commands.push(insert).is_ok();
            debug_assert!(pushed, "room was checked above");
        }
        if waiting > 0 || self.commands.slots() < starting.len() + 1 + self.slots.retiring().len() + stopping.len() {
            return Err(LoomError::QueueFull);
        }
        for (id, key) in starting {
            let slot = self.slots.take().expect("checked above");
            let node = self.graph.nodes.get_mut(&id).expect("listed above");
            let ModuleRole::Dsp(factory) = node.descriptor.role else {
                unreachable!("only DSP modules are listed");
            };
            // A new instance starts from reset, with the module's current
            // parameters. It has no display values: lights read copy 0.
            let mut module = factory(&ModuleConfig {
                sample_rate: self.config.sample_rate,
                max_block: self.config.max_block,
            });
            for (index, &value) in node.params.iter().enumerate() {
                module.set_param(index, value);
            }
            module.reset();
            let insert = Command::InsertModule {
                slot,
                module,
                id: id.0,
                displays: 0,
                triggers: 0,
            };
            let pushed = self.commands.push(insert).is_ok();
            debug_assert!(pushed, "capacity was checked above");
            node.copy_slots.insert(key, slot);
        }
        for (id, key, slot) in stopping {
            self.graph
                .nodes
                .get_mut(&id)
                .expect("listed above")
                .copy_slots
                .remove(&key);
            self.slots.retire(slot);
        }
        // What the graph holds per mapping. With a screen edit on its way,
        // every control change so far is older than the graph's value.
        let synced = self.midi.synced(&self.graph);
        // Swap fades that leave with this edit and had finished: a cable
        // moved off one onto its new part switches (rule 9).
        let reader = self.display_reader();
        let done: BTreeSet<ModuleId> = self
            .cable_fades
            .wiring
            .as_ref()
            .map(|wiring| {
                wiring
                    .leaving_fades(&self.graph)
                    .into_iter()
                    .filter(|&(_, source)| reader.read(source) >= 0.5)
                    .map(|(id, _)| id)
                    .collect()
            })
            .unwrap_or_default();
        let generation = self.cable_fades.generation + 1;
        let (mut schedule, mut report, wiring) = compile_with_fades(
            &self.graph,
            (self.probes.as_map(), self.probes.pairs(), &synced),
            self.config.sample_rate,
            self.config.max_block,
            Fades {
                previous: self.cable_fades.wiring.as_ref(),
                carried: self.cable_fades.carried(&self.stats),
                done,
            },
        );
        schedule.generation = generation;
        let fading = schedule.fade_keys();
        // Live value cells, kept from the last compile where the parameter
        // is still modulated.
        // A right copy's fills get none: the knob shows copy 0. Nor does a
        // fill kept only while an assignment ramps out (rule 9).
        let keys: Vec<u64> = schedule
            .fills
            .iter()
            .filter(|fill| !fill.is_extra_copy() && fill.is_modulated())
            .map(|fill| fill.key)
            .collect();
        let (released, missing) = self.live.assign(&keys);
        for fill in &mut schedule.fills {
            fill.live = if fill.is_extra_copy() || !fill.is_modulated() {
                NO_CELL
            } else {
                self.live.cell(fill.key).unwrap_or(NO_CELL)
            };
        }
        schedule.live_release = released;
        report.without_live_value = missing;
        // Order matters: swap first (the new schedule no longer uses the
        // removed slots), then remove the instances.
        let swapped = self.commands.push(Command::SwapSchedule(Box::new(schedule))).is_ok();
        // The engine runs the schedule once it is sent, so what the next
        // compile fades from follows that push alone (rule 9).
        if swapped {
            self.cable_fades = CableFades {
                wiring: Some(wiring),
                generation,
                fading,
            };
        }
        let pushed = swapped
            && self
                .slots
                .retiring()
                .iter()
                .all(|&slot| self.commands.push(Command::RemoveModule { slot }).is_ok());
        debug_assert!(pushed, "capacity was checked above");
        self.slots.free_retired();
        self.dirty = false;
        self.last_report = report;
        Ok(&self.last_report)
    }

    /// Frees memory the engine has handed back. Called automatically by
    /// edits and commits. Call it now and then when idle.
    pub fn collect_garbage(&mut self) {
        while let Ok(garbage) = self.garbage.pop() {
            drop(garbage);
        }
    }

    fn node(&self, id: ModuleId) -> Result<&Node, LoomError> {
        self.graph.nodes.get(&id).ok_or(LoomError::UnknownModule(id))
    }

    fn check_output(&self, output: OutputRef) -> Result<(), LoomError> {
        let node = self.node(output.module)?;
        if output.port >= node.output_count() {
            return Err(LoomError::NoSuchPort {
                module: output.module,
                direction: "output",
                port: output.port.to_string(),
            });
        }
        Ok(())
    }

    fn param_index(&self, module: ModuleId, name: &str) -> Result<usize, LoomError> {
        let node = self.node(module)?;
        node.parameter_index(name).ok_or_else(|| LoomError::NoSuchParam {
            module,
            param: name.to_string(),
        })
    }

    /// Checks a parameter that modulation or a MIDI mapping will drive. A
    /// transient parameter is a live gesture, so it is refused
    /// (transient-params.md).
    fn check_target(&self, module: ModuleId, index: usize) -> Result<(), LoomError> {
        self.check_param(module, index)?;
        if self.node(module)?.unknown.is_some() {
            return Ok(());
        }
        let spec = &self.node(module)?.descriptor.params[index];
        if spec.options.shapes_graph {
            return Err(LoomError::StructuralParam {
                module,
                param: spec.name.to_string(),
            });
        }
        if spec.is_transient() {
            return Err(LoomError::TransientParam {
                module,
                param: spec.name.to_string(),
            });
        }
        Ok(())
    }

    fn check_param(&self, module: ModuleId, index: usize) -> Result<(), LoomError> {
        let node = self.node(module)?;
        let count = node.parameter_count();
        if index >= count {
            return Err(LoomError::NoSuchParam {
                module,
                param: index.to_string(),
            });
        }
        Ok(())
    }

    fn check_input(&self, input: InputRef) -> Result<(), LoomError> {
        let node = self.node(input.module)?;
        if input.port >= node.input_count() {
            return Err(LoomError::NoSuchPort {
                module: input.module,
                direction: "input",
                port: input.port.to_string(),
            });
        }
        Ok(())
    }
}

mod assignments;
mod audio;
mod cables;
mod displays;
mod midi;
mod modules;
mod params;
mod transport;

use displays::{LiveValues, Probes};
use midi::MidiSync;
use modules::Slots;

#[cfg(test)]
mod tests;
