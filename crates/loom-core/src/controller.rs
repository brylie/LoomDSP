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
//! *Why flat, and not `controller.midi().map(..)` views or separate owned
//! types:* nearly every method edits the same graph and sends on the same
//! command queue, so separate parts would share both anyway, and views would
//! rename every call for no gain in safety (#472). Helpers that more than one
//! group uses, such as `send` and the `check_*` functions, stay in this file,
//! because a child module cannot see its siblings' private items.

use crate::compile::{CompileReport, compile};
use crate::descriptor::{ModuleConfig, ModuleDescriptor, ModuleRole, ParamRange, Polarity};
use crate::display::{DisplayReader, DisplaySource, DisplayTable};
use crate::engine::{Command, Engine, EngineConfig, EngineStats, Garbage};
use crate::error::LoomError;
use crate::graph::{Assignment, AssignmentId, Graph, InputRef, ModuleId, Node, OutputRef};
use crate::live::{LiveCells, LiveTable, NO_CELL};
use crate::midi::MidiEvent;
use crate::midi_map::{
    ControlChangeSeen, MidiControl, MidiFeedback, MidiMapping, MidiMappingId, MidiReader, control_value, later,
};
use crate::registry::Registry;
use crate::safety::OutputSafety;
use crate::telemetry::{ProbeId, ProbeKind, Telemetry, TelemetrySender};
use crate::transport::{TimeSignature, TransportSettings, clamp_tempo};
use crate::width;
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::BTreeMap;
use std::sync::Arc;

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
        },
        (displays.clone(), live.clone()),
        stats.clone(),
    );
    let controller = Controller {
        registry,
        graph: Graph::default(),
        commands: command_tx,
        garbage: garbage_rx,
        free_slots: (0..config.max_modules).rev().collect(),
        pending_removals: Vec::new(),
        fresh: BTreeMap::new(),
        probes: BTreeMap::new(),
        next_probe: 1,
        next_id: 1,
        next_assignment: 1,
        next_midi_mapping: 1,
        midi_seen: BTreeMap::new(),
        midi_edits: BTreeMap::new(),
        next_edit: 1,
        midi_feedback,
        dirty: false,
        last_report: CompileReport::default(),
        transport: TransportSettings::default(),
        displays,
        live,
        live_cells: LiveCells::new(config.max_live_values),
        config,
        stats,
    };
    EngineParts {
        controller,
        engine,
        midi: MidiSender { tx: midi_tx },
        telemetry: Telemetry::new(meter_rx, scope_rx),
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
    free_slots: Vec<usize>,
    pending_removals: Vec<usize>,
    /// Modules added since the last commit, by slot. They stay here, where
    /// their first values are set directly, until `commit` resets them and
    /// sends them to the engine (live patching spec, rule 7).
    fresh: BTreeMap<usize, Fresh>,
    probes: BTreeMap<ProbeId, (OutputRef, ProbeKind)>,
    next_probe: u32,
    next_id: u32,
    next_assignment: u32,
    next_midi_mapping: u32,
    /// The last feedback sequence number applied, per MIDI mapping.
    midi_seen: BTreeMap<MidiMappingId, u32>,
    /// Screen edits on mapped parameters that the engine has not confirmed
    /// yet: the token of the latest one, per mapping.
    midi_edits: BTreeMap<MidiMappingId, u32>,
    next_edit: u32,
    /// What the engine's MIDI mappings did (MIDI learn, rules 2 and 3).
    midi_feedback: Arc<MidiFeedback>,
    dirty: bool,
    last_report: CompileReport,
    /// What the engine's transport is set to, as of the last command sent.
    transport: TransportSettings,
    /// Display values from the engine (ADR-0013).
    displays: Arc<DisplayTable>,
    /// Live modulated values from the engine, and which cell each modulated
    /// parameter has (`docs/design/modulation.md`, Live value).
    live: Arc<LiveTable>,
    live_cells: LiveCells,
    config: EngineConfig,
    stats: Arc<EngineStats>,
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
        if !self.dirty {
            return Ok(&self.last_report);
        }
        // Right copies (ADR-0020, stereo spec rule 13). A module that starts
        // to run per channel needs a copy before the swap; one that stops
        // has its copy removed after it, like a removed module.
        let per_channel = width::infer(&self.graph).per_channel;
        let mut starting = Vec::new();
        let mut stopping = Vec::new();
        for (&id, node) in &self.graph.nodes {
            if !matches!(node.descriptor.role, ModuleRole::Dsp(_)) {
                continue;
            }
            match (per_channel.contains(&id), node.copy_slot) {
                (true, None) => starting.push(id),
                (false, Some(slot)) => stopping.push((id, slot)),
                _ => {}
            }
        }
        if self.free_slots.len() < starting.len() {
            return Err(LoomError::TooManyModules(self.config.max_modules));
        }
        // New modules go in first, after `reset()` snaps their smoothing to
        // the values set since they were added (live patching spec, rule
        // 7). An instance the schedule does not use yet changes nothing, so
        // when they do not all fit with the swap, the ones that fit go now
        // and the rest wait for the next commit. Their later edits are
        // commands, as for any module in the engine.
        let room = self.commands.slots().saturating_sub(1 + self.pending_removals.len());
        let waiting = self.fresh.len().saturating_sub(room);
        let sending: Vec<usize> = self.fresh.keys().copied().take(room).collect();
        for slot in sending {
            let mut fresh = self.fresh.remove(&slot).expect("listed above");
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
        if waiting > 0 || self.commands.slots() < starting.len() + 1 + self.pending_removals.len() + stopping.len() {
            return Err(LoomError::QueueFull);
        }
        for id in starting {
            let slot = self.free_slots.pop().expect("checked above");
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
            node.copy_slot = Some(slot);
        }
        for (id, slot) in stopping {
            self.graph.nodes.get_mut(&id).expect("listed above").copy_slot = None;
            self.pending_removals.push(slot);
        }
        // What the graph holds per mapping. With a screen edit on its way,
        // every control change so far is older than the graph's value.
        let synced: BTreeMap<MidiMappingId, u32> = self
            .graph
            .midi_mappings()
            .filter_map(|(id, mapping)| {
                let entry = self.midi_feedback.read(mapping.control);
                if self.midi_edits.contains_key(&id) && entry.id == id {
                    Some((id, entry.sequence))
                } else {
                    self.midi_seen.get(&id).map(|&sequence| (id, sequence))
                }
            })
            .collect();
        let (mut schedule, mut report) = compile(
            &self.graph,
            &self.probes,
            &synced,
            self.config.sample_rate,
            self.config.max_block,
        );
        // Live value cells, kept from the last compile where the parameter
        // is still modulated.
        // A right copy's fills get none: the knob shows copy 0.
        let keys: Vec<u64> = schedule
            .fills
            .iter()
            .filter(|fill| !fill.is_right_copy())
            .map(|fill| fill.key)
            .collect();
        let (released, missing) = self.live_cells.assign(&keys);
        for fill in &mut schedule.fills {
            fill.live = if fill.is_right_copy() {
                NO_CELL
            } else {
                self.live_cells.cell(fill.key).unwrap_or(NO_CELL)
            };
        }
        schedule.live_release = released;
        report.without_live_value = missing;
        // Order matters: swap first (the new schedule no longer uses the
        // removed slots), then remove the instances.
        let pushed = self.commands.push(Command::SwapSchedule(Box::new(schedule))).is_ok()
            && self
                .pending_removals
                .iter()
                .all(|&slot| self.commands.push(Command::RemoveModule { slot }).is_ok());
        debug_assert!(pushed, "capacity was checked above");
        self.free_slots.append(&mut self.pending_removals);
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
        node.unknown
            .as_ref()
            .map_or_else(
                || node.descriptor.param_index(name),
                |unknown| unknown.parameters.iter().position(|param| param == name),
            )
            .ok_or_else(|| LoomError::NoSuchParam {
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
        let count = node
            .unknown
            .as_ref()
            .map_or(node.descriptor.params.len(), |unknown| unknown.parameters.len());
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

#[cfg(test)]
mod tests;
