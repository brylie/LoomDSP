//! The audio-thread side of the engine.
//!
//! Real-time rules (NFR-RT-01): nothing in this file allocates, frees, locks,
//! or waits once the engine is running. Memory arrives pre-allocated through
//! the command queue and leaves through the garbage queue.
//! `docs/learn/real-time-safety.md` explains why. Telemetry leaves
//! through its own queues and is dropped when they are full (NFR-RT-07).

use crate::cable_fade::InputFade;
use crate::compile::{AssignmentRuntime, DownmixOp, ModSource, ModuleOp, ParamFill, Schedule, Step, partner, partners};
use crate::descriptor::{MAX_DISPLAYS, Polarity};
use crate::display::DisplayTable;
use crate::graph::AssignmentId;
use crate::live::{LiveTable, NO_CELL};
use crate::midi::{HeldSet, MAX_MIDI_INPUTS, MidiEvent, MidiMessage};
use crate::midi_map::{
    ControlOutcome, ControlState, MIDI_CONTROLS, MidiControl, MidiFeedback, MidiMapOp, MidiMode, later,
};
use crate::module::{Module, PortIo, ProcessContext};
use crate::panic::{PanicState, PanicTrigger, TriggerWatch};
use crate::safety::{OutputSafety, SafetyStage};
use crate::telemetry::{MeterReading, PAIR_CHUNK, PairChunk, SCOPE_CHUNK, ScopeChunk, TelemetrySender, measure};
use crate::transport::{
    MidiClock, TRANSPORT_GRID, TimeSignature, Transport, TransportRequest, TransportSettings, clamp_tempo,
};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A host's bounded command channel for restarting the output fade.
///
/// Create this on the control thread before starting the engine. The host
/// sends a request before resuming a paused stream; the audio thread reads
/// it at the next buffer boundary. Repeated requests coalesce while one is
/// pending, so a full queue is not an error.
pub struct RestartFadeSender(Producer<()>);

impl RestartFadeSender {
    /// Requests a fresh fade on the next engine buffer.
    pub fn request(&mut self) {
        let _ = self.0.push(());
    }
}

/// Engine settings, fixed for the engine's lifetime.
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    /// Sample rate in Hz.
    pub sample_rate: f32,
    /// Internal block size. Host buffers of any size are split into blocks of
    /// at most this many frames.
    pub max_block: usize,
    /// Maximum number of DSP module instances.
    pub max_modules: usize,
    /// Cells in the live value table: how many modulated parameters show
    /// their live value on screen. More still sound right, without a dot.
    pub max_live_values: usize,
    /// Capacity of the control-to-audio command queue.
    pub command_capacity: usize,
    /// Capacity of the MIDI input queue and per-buffer MIDI event list.
    pub midi_capacity: usize,
    /// Capacity of the meter queue, in readings. One reading per metered
    /// probe per block.
    pub meter_capacity: usize,
    /// Capacity of the scope queue, in chunks of
    /// [`SCOPE_CHUNK`](crate::SCOPE_CHUNK) samples.
    pub scope_capacity: usize,
    /// Capacity of the pair queue, in chunks of
    /// [`PAIR_CHUNK`](crate::PAIR_CHUNK) frames.
    pub pair_capacity: usize,
    /// The safety stage's mode when the engine starts (ADR-0019). Change it
    /// later with [`Controller::set_output_safety`](crate::Controller::set_output_safety).
    pub output_safety: OutputSafety,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            max_block: 128,
            max_modules: 1024,
            max_live_values: 256,
            command_capacity: 1024,
            midi_capacity: 1024,
            meter_capacity: 1024,
            scope_capacity: 1024,
            pair_capacity: 1024,
            output_safety: OutputSafety::LowLatency,
        }
    }
}

impl EngineConfig {
    /// Default settings at the given sample rate.
    pub fn with_sample_rate(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            ..Self::default()
        }
    }
}

/// Counters the engine updates and the control thread reads.
#[derive(Debug, Default)]
pub struct EngineStats {
    /// Buffers processed.
    pub buffers: AtomicU64,
    /// Batches of commands drained, used to wake a producer waiting for room.
    pub command_drains: AtomicU64,
    /// Schedules swapped in.
    pub schedule_swaps: AtomicU64,
    /// MIDI events dropped because the per-buffer list was full.
    pub dropped_midi: AtomicU64,
    /// Effective tempo in BPM, encoded as f64 bits.
    pub transport_tempo: AtomicU64,
    /// The selected MIDI clock is free running after a loss.
    pub clock_lost: std::sync::atomic::AtomicBool,
    /// Objects the engine could not return for freeing (garbage queue full).
    /// They are leaked rather than freed on the audio thread.
    pub leaked: AtomicU64,
    /// Meter readings dropped because the meter queue was full.
    pub dropped_meters: AtomicU64,
    /// Scope chunks dropped because the scope queue was full.
    pub dropped_scope: AtomicU64,
    /// Pair chunks dropped because the queue was full.
    pub dropped_pairs: AtomicU64,
    /// Input underrun corrections.
    pub input_underruns: AtomicU64,
    /// Input overrun corrections.
    pub input_overruns: AtomicU64,
    /// Output samples that were NaN or infinity, and became silence.
    pub output_silenced: AtomicU64,
    /// Output samples clipped to -1 to 1, or finite samples above the safety
    /// stage's input limit when they came in. Each sample counts once.
    pub output_clipped: AtomicU64,
    /// The generation of the latest schedule whose cable fades have all
    /// ended (live patching spec, rule 9). The controller reads it to know
    /// when a fade can no longer be running.
    pub cable_fades_settled: AtomicU64,
}

pub(crate) enum Command {
    /// Replaces the input ring at a buffer boundary.
    SetAudioInput(Option<crate::audio_input::InputReader>),
    InsertModule {
        slot: usize,
        module: Box<dyn Module>,
        /// The module's ID, which owns its display values.
        id: u32,
        /// How many display values it publishes.
        displays: u8,
        /// Bit `i` set when display value `i` is a trigger.
        triggers: u8,
    },
    RemoveModule {
        slot: usize,
    },
    SwapSchedule(Box<Schedule>),
    SetParam {
        slot: usize,
        index: usize,
        value: f32,
        /// For a mapped parameter: its control and the edit's token, so the
        /// controller can order this edit against control changes.
        edit: Option<(MidiControl, u32)>,
    },
    /// Depth, polarity, and enabled flag of one assignment. Ignored when the
    /// running schedule has no assignment with this id.
    UpdateAssignment {
        id: AssignmentId,
        depth: f32,
        enabled: bool,
        polarity: Option<Polarity>,
    },
    ResetModule {
        slot: usize,
    },
    /// Already clamped by the controller.
    SetTempo(f64),
    SelectMidiClock(Option<crate::MidiInputId>, bool),
    /// Already checked by the controller.
    SetTimeSignature(TimeSignature),
    SetRunning(bool),
    ResetTransport,
    SetOutputSafety(OutputSafety),
    /// Fade the output to silence and keep it there (output safety, rule 14).
    FadeOut,
    /// Fade out, reset every module, and fade back in (panic.md).
    Panic,
    /// The MIDI message that runs panic, from the player's preferences.
    SetPanicTrigger(Option<PanicTrigger>),
    /// Start tracking MIDI input `input`, preserving its retained configuration.
    OpenInput {
        input: crate::MidiInputId,
        held: Box<HeldSet>,
    },
    /// Release every note MIDI input `input` holds, and stop tracking it.
    ReleaseInput(crate::MidiInputId),
}

pub(crate) enum Garbage {
    Module(#[expect(dead_code, reason = "dropped on the control thread")] Box<dyn Module>),
    Schedule(#[expect(dead_code, reason = "dropped on the control thread")] Box<Schedule>),
    HeldSet(#[expect(dead_code, reason = "dropped on the control thread")] Box<HeldSet>),
    Input(#[expect(dead_code, reason = "dropped on the control thread")] crate::audio_input::InputReader),
    MidiConfig(#[expect(dead_code, reason = "dropped on the control thread")] Box<crate::MidiConfigTable>),
}

/// Notes held per tracked MIDI input
/// (`docs/design/device-changes.md`, Held notes belong to an input). All
/// memory is allocated when the engine is built or arrives by command.
struct InputNotes {
    /// One set per open input, by id. `None` for an input that is not open.
    inputs: Box<[Option<Box<HeldSet>>]>,
    /// Note offs from released inputs, sent at the start of the next
    /// buffer.
    releases: Vec<MidiEvent>,
}

impl InputNotes {
    fn new() -> Self {
        Self {
            inputs: (0..MAX_MIDI_INPUTS).map(|_| None).collect(),
            releases: Vec::with_capacity(usize::from(MAX_MIDI_INPUTS - 1) * 16 * 128),
        }
    }

    /// Follows each source independently. Every genuine note off remains visible.
    fn follow(&mut self, event: &MidiEvent) -> bool {
        if event.input == 0 {
            return true;
        }
        let Some(Some(held)) = self.inputs.get_mut(usize::from(event.input)) else {
            return false;
        };
        match event.message() {
            MidiMessage::NoteOn { channel, note, .. } => held.set(channel, note, true),
            MidiMessage::NoteOff { channel, note } => held.set(channel, note, false),
            _ => {}
        }
        true
    }

    /// Queues tagged releases for every held identity on this input.
    fn release(&mut self, input: crate::MidiInputId) -> Option<Box<HeldSet>> {
        let index = usize::from(input.0);
        let mut held = self.inputs.get_mut(index)?.take()?;
        held.drain(|channel, note| {
            if self.releases.len() < self.releases.capacity() {
                self.releases
                    .push(MidiEvent::note_off(0, channel, note).from_input(input));
            }
        });
        Some(held)
    }

    /// Forgets every held note, as panic does: the modules have just let
    /// go of them all. Inputs stay open.
    fn clear(&mut self) {
        for held in self.inputs.iter_mut().flatten() {
            held.drain(|_, _| {});
        }
        self.releases.clear();
    }
}

/// A mapped control change waiting for its absolute grid frame.
#[derive(Clone, Copy)]
struct MappedControl {
    frame: u64,
    control: MidiControl,
    value: u8,
}

/// The audio-thread half of the engine. Call [`Engine::process`] from the
/// audio callback.
pub struct Engine {
    slots: Box<[Option<Box<dyn Module>>]>,
    /// Display values per slot (0 for most modules), and where they go.
    display_counts: Box<[u8]>,
    displays: Arc<DisplayTable>,
    live: Arc<LiveTable>,
    schedule: Box<Schedule>,
    commands: Consumer<Command>,
    /// Host restart commands, installed before an output stream starts.
    restart_fade: Option<Consumer<()>>,
    garbage: Producer<Garbage>,
    midi_in: Consumer<MidiEvent>,
    midi_config_in: Consumer<Box<crate::MidiConfigTable>>,
    midi_config: crate::MidiConfigTimeline,
    midi: Vec<MidiEvent>,
    /// Mapped control changes due within this callback or on a later grid line.
    mapped_controls: Vec<MappedControl>,
    /// What MIDI mappings did, for the controller (MIDI learn).
    midi_feedback: Arc<MidiFeedback>,
    /// What each mapped control's mapping knows between control changes.
    /// Indexed by [`MidiControl::index`].
    midi_states: Box<[ControlState]>,
    /// Control changes and note ons received, for [`MidiFeedback::record`]
    /// and [`MidiFeedback::record_note`]. Never 0
    /// after the first one.
    cc_serial: u32,
    /// One sequence for mapped control changes and screen edits, so the
    /// controller can tell which came last.
    feedback_sequence: u32,
    telemetry: TelemetrySender,
    /// The patch transport (ADR-0015). Not part of the schedule, so editing
    /// the patch never moves it.
    transport: Transport,
    midi_clock: MidiClock,
    local_transport: TransportSettings,
    run_gate: bool,
    restore_run_gate: bool,
    clock_output: Option<crate::clock_output::ClockOutput>,
    /// Frames rendered since the engine started. Stamps telemetry and
    /// anchors the transport.
    frame: u64,
    /// Length of the last block the current schedule ran, so a swap can
    /// read the last value of each of its outputs. 0 before it has run.
    last_frames: usize,
    /// True once the current schedule's cable fades have all ended and
    /// [`EngineStats::cable_fades_settled`] says so.
    fades_settled: bool,
    /// DC blocker, limiter, and fade on the summed output (ADR-0019).
    safety: SafetyStage,
    /// Where a panic is (panic.md).
    panic: PanicState,
    /// The MIDI trigger for panic, and the state of its control.
    panic_trigger: TriggerWatch,
    /// The notes each tracked MIDI input holds.
    input_notes: InputNotes,
    input: Option<crate::audio_input::InputReader>,
    input_buffer: Box<[crate::audio_input::InputFrame]>,
    input_channels: usize,
    config: EngineConfig,
    stats: Arc<EngineStats>,
}

impl Engine {
    pub(crate) fn new(
        config: EngineConfig,
        commands: Consumer<Command>,
        garbage: Producer<Garbage>,
        (midi_in, midi_config_in, midi_feedback): (
            Consumer<MidiEvent>,
            Consumer<Box<crate::MidiConfigTable>>,
            Arc<MidiFeedback>,
        ),
        telemetry: TelemetrySender,
        (displays, live): (Arc<DisplayTable>, Arc<LiveTable>),
        stats: Arc<EngineStats>,
    ) -> Self {
        Self {
            slots: (0..config.max_modules).map(|_| None).collect(),
            display_counts: vec![0; config.max_modules].into_boxed_slice(),
            displays,
            live,
            schedule: Box::new(Schedule::empty()),
            commands,
            restart_fade: None,
            garbage,
            midi_in,
            midi_config_in,
            midi_config: crate::MidiConfigTimeline::new(config.midi_capacity),
            midi: Vec::with_capacity(config.midi_capacity),
            mapped_controls: Vec::with_capacity(config.midi_capacity.saturating_mul(2)),
            midi_feedback,
            midi_states: vec![ControlState::NONE; MIDI_CONTROLS].into_boxed_slice(),
            cc_serial: 0,
            feedback_sequence: 0,
            telemetry,
            transport: Transport::new(config.sample_rate),
            midi_clock: MidiClock::default(),
            local_transport: TransportSettings::default(),
            run_gate: false,
            restore_run_gate: false,
            clock_output: None,
            frame: 0,
            last_frames: 0,
            fades_settled: true,
            safety: SafetyStage::new(config.output_safety, config.sample_rate),
            panic: PanicState::Idle,
            panic_trigger: TriggerWatch::default(),
            input_notes: InputNotes::new(),
            input: None,
            input_buffer: vec![[0.0; crate::audio_input::MAX_INPUT_CHANNELS]; config.max_block].into_boxed_slice(),
            input_channels: 0,
            config,
            stats,
        }
    }

    /// Creates a restart-fade command channel before moving the engine to
    /// an audio callback. This allocates and must run on the control thread.
    pub fn restart_fade_sender(&mut self) -> RestartFadeSender {
        let (sender, receiver) = RingBuffer::new(1);
        self.restart_fade = Some(receiver);
        RestartFadeSender(sender)
    }

    /// Applies a pending host restart without allocating on the audio thread.
    fn apply_restart_fade(&mut self) {
        if self
            .restart_fade
            .as_mut()
            .is_some_and(|receiver| receiver.pop().is_ok())
        {
            if matches!(self.panic, PanicState::Fading(_)) {
                // A paused device is silent: finish the panic reset before
                // the resumed stream can play another sample.
                self.panic = PanicState::Fading(0);
            } else {
                self.safety.restart_fade();
            }
        }
    }

    /// Engine settings.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// How many notes MIDI input `input` holds, or `None` if the engine does
    /// not track it. For tests and diagnostics.
    pub fn held_notes(&self, input: crate::MidiInputId) -> Option<usize> {
        self.input_notes
            .inputs
            .get(usize::from(input.0))?
            .as_ref()
            .map(|held| held.count())
    }

    /// True once the engine was asked to fade out
    /// ([`Controller::fade_out`](crate::Controller::fade_out)) and the last
    /// sample it rendered was silent. Every later sample is silent too, so
    /// the host may stop the stream once the device has played what is
    /// rendered (output safety, rule 14).
    pub fn is_silent(&self) -> bool {
        self.safety.is_silent()
    }

    /// Applies pending changes without rendering audio, for an engine that
    /// no device plays. The command queue then never fills, and replaced
    /// memory goes back to the controller to be freed (#220).
    ///
    /// It does only what [`process`](Self::process) does first: no MIDI is
    /// read, no module runs, the transport does not move, and no telemetry,
    /// display, or live value is published. Commands are applied in order
    /// either way, so the next `process` call gives the same output as if
    /// this call had not happened. Real-time safe, like `process`.
    pub fn apply_pending(&mut self) {
        self.apply_commands();
        self.apply_restart_fade();
    }

    /// Renders `left.len()` frames (both slices must have the same length).
    ///
    /// Applies pending changes first, at this buffer boundary (FR-GRAPH-02).
    /// MIDI events are stamped relative to the start of this buffer.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        self.apply_commands();
        self.apply_restart_fade();
        self.collect_midi(frames);
        self.watch_panic_trigger();
        self.collect_mapped_controls();

        left[..frames].fill(0.0);
        right[..frames].fill(0.0);

        // A panic that is already silent resets before any frame.
        if self.panic == PanicState::Fading(0) {
            self.panic_reset();
        }
        let mut position = 0;
        let mut midi_cursor = 0;
        let mut mapped_cursor = 0;
        while position < frames {
            let mut n = (frames - position).min(self.config.max_block);
            // The reset is sample-exact: the block ends at the frame where
            // the panic fade reaches 0 (panic.md, rule 2).
            if let PanicState::Fading(remaining) = self.panic {
                n = n.min(remaining);
            }
            // With a cable on the Transport's run or reset input, blocks end
            // on the transport grid, so what the module reads in one step
            // takes effect at the start of the next at any block size
            // (clock.md, rule 11).
            if self.schedule.transport.grid {
                let into = (self.frame % TRANSPORT_GRID as u64) as usize;
                n = n.min(TRANSPORT_GRID - into);
            }
            while let Some(change) = self.mapped_controls.get(mapped_cursor).copied() {
                if change.frame > self.frame {
                    n = n.min((change.frame - self.frame).min(n as u64) as usize);
                    break;
                }
                mapped_cursor += 1;
                self.apply_mapped_control(change);
            }
            let mut sent_jump = false;
            if self.midi_clock.input.is_some() {
                while let Some(event) = self.midi.get(midi_cursor).copied() {
                    if event.frame as usize != position {
                        n = n.min(event.frame as usize - position);
                        break;
                    }
                    midi_cursor += 1;
                    let before = self.transport.info(self.frame);
                    let jumped = self.midi_clock.receive(&mut self.transport, self.frame, event);
                    let after = self.transport.info(self.frame);
                    if (jumped || before.playing() != after.playing())
                        && let Some(output) = &mut self.clock_output
                    {
                        output.transition(self.frame, after.with_jump(jumped));
                        sent_jump |= jumped;
                    }
                }
                self.midi_clock.check_loss(&mut self.transport, self.frame);
                if let Some(deadline) = self.midi_clock.deadline(&self.transport) {
                    n = n.min((deadline - self.frame).min(n as u64) as usize);
                }
            }
            self.input_channels = 0;
            if let Some(input) = &mut self.input {
                let underruns = input.underruns();
                let overruns = input.overruns();
                if input.read_block(&mut self.input_buffer[..n]) {
                    self.input_channels = input.channels();
                }
                self.stats
                    .input_underruns
                    .fetch_add(input.underruns() - underruns, Ordering::Relaxed);
                self.stats
                    .input_overruns
                    .fetch_add(input.overruns() - overruns, Ordering::Relaxed);
            }
            self.apply_transport_request();
            if let Some(output) = &mut self.clock_output {
                let transport = self.transport.info(self.frame);
                output.render(self.frame, n, transport.with_jump(transport.jumped() && !sent_jump));
            }
            self.run_block(
                position,
                n,
                &mut left[position..position + n],
                &mut right[position..position + n],
            );
            self.send_telemetry(n);
            self.publish_displays();
            self.publish_live(n);
            self.frame += n as u64;
            self.last_frames = n;
            self.publish_fades_settled();
            position += n;
            match self.panic {
                PanicState::Fading(remaining) if remaining == n => self.panic_reset(),
                PanicState::Fading(remaining) => self.panic = PanicState::Fading(remaining - n),
                PanicState::Rising if self.safety.is_open() => self.panic = PanicState::Idle,
                _ => {}
            }
        }
        self.mapped_controls.drain(..mapped_cursor);
        self.stats
            .transport_tempo
            .store(self.transport.info(self.frame).tempo().to_bits(), Ordering::Relaxed);
        self.stats.clock_lost.store(self.midi_clock.lost, Ordering::Relaxed);
        self.stats.buffers.fetch_add(1, Ordering::Relaxed);
    }

    /// Tells the controller once every cable fade of the current schedule
    /// has ended. Scans the fades only until then.
    fn publish_fades_settled(&mut self) {
        if !self.fades_settled && crate::cable_fade::settled(&self.schedule) {
            self.fades_settled = true;
            self.stats
                .cable_fades_settled
                .store(self.schedule.generation, Ordering::Relaxed);
        }
    }

    /// Asks the Transport module, if the patch has one, for the changes to
    /// make before the next block, and makes them (clock.md, Controls live
    /// on the Transport module).
    fn apply_transport_request(&mut self) {
        let Some(slot) = self.schedule.transport.module else {
            return;
        };
        let Some(module) = self.slots[slot as usize].as_mut() else {
            return;
        };
        let grid = self.frame.is_multiple_of(TRANSPORT_GRID as u64);
        let cable = self.schedule.transport.run_cable;
        let mut request = module.transport_request(grid, cable);
        if let Some(run) = request.local_running.or(if cable { None } else { request.running }) {
            self.local_transport.running = run;
        }
        if let Some(run) = request.running.filter(|_| cable) {
            self.run_gate = run;
        }
        if self.midi_clock.input.is_none() && self.restore_run_gate && (grid || !cable) {
            self.restore_run_gate = false;
            if cable && request.running.is_none() {
                request.running = Some(self.run_gate);
            }
        }
        self.apply_local_transport(request);
    }

    /// Records local settings and applies the ones the active source allows.
    fn apply_local_transport(&mut self, request: TransportRequest) {
        if let Some(tempo) = request.tempo {
            self.local_transport.tempo = clamp_tempo(tempo);
        }
        let request = if self.midi_clock.input.is_some() {
            TransportRequest {
                time_signature: request.time_signature,
                ..TransportRequest::default()
            }
        } else {
            request
        };
        self.transport.apply(self.frame, request);
    }

    /// Selects the clock source and restores the local settings on return.
    fn select_midi_clock(&mut self, input: Option<crate::MidiInputId>, reconnect: bool) {
        let following = self.midi_clock.input.is_some();
        self.midi_clock
            .select(input, reconnect, &mut self.transport, self.frame);
        if following && input.is_none() {
            self.restore_run_gate = self.schedule.transport.run_cable;
            self.transport.set_tempo(self.frame, self.local_transport.tempo);
            self.transport.set_running(self.frame, self.local_transport.running);
        }
    }

    /// Frames rendered so far, for the host's playback time mapping.
    pub fn rendered_frames(&self) -> u64 {
        self.frame
    }

    /// Attaches an output queue before the engine starts. Allocates on the caller's thread.
    pub fn attach_clock_output(&mut self) -> crate::ClockReader {
        let (writer, reader) = crate::clock_output::channel();
        self.clock_output = Some(writer);
        reader
    }

    /// Starts a panic, unless one is running (panic.md, rule 3) or the
    /// output is stopping.
    fn start_panic(&mut self) {
        if self.panic != PanicState::Idle {
            return;
        }
        if let Some(frames) = self.safety.panic_fade() {
            self.panic = PanicState::Fading(frames);
        }
    }

    /// The silent frame of a panic (panic.md, step 2): every module is
    /// reset, which also lets go of every held note, unit delays and
    /// smoothing are cleared, and the output stage starts fresh and fades
    /// back in. Parameters, cables, and the transport are kept.
    fn panic_reset(&mut self) {
        for module in self.slots.iter_mut().flatten() {
            module.reset();
        }
        for tap in &mut self.schedule.taps {
            tap.state = 0.0;
        }
        for fill in &mut self.schedule.fills {
            if let Some(smooth) = fill.smooth.as_mut() {
                smooth.reset(smooth.target());
            }
        }
        // Cable fades end too, so nothing from before the panic reaches the
        // reset modules (rule 9).
        crate::cable_fade::finish_all(&mut self.schedule);
        self.fades_settled = false;
        self.publish_fades_settled();
        self.input_notes.clear();
        self.safety.panic_reset();
        self.panic = PanicState::Rising;
    }

    /// Starts a panic if a MIDI message in this buffer is the panic
    /// trigger. The message still reaches the modules (panic.md, MIDI
    /// trigger).
    fn watch_panic_trigger(&mut self) {
        let mut fired = false;
        for event in &self.midi {
            fired |= self.panic_trigger.fires(event.message());
        }
        if fired {
            self.start_panic();
        }
    }

    /// Asks every module that has display values for them, after a block.
    fn publish_displays(&self) {
        for (slot, &count) in self.display_counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            if let Some(module) = self.slots[slot].as_ref() {
                let mut values = [0.0; MAX_DISPLAYS];
                let values = &mut values[..usize::from(count)];
                module.displays(values);
                self.displays.write(slot, values);
            }
        }
    }

    /// Stores the last frame of each modulated parameter with a live cell,
    /// as a position from 0 to 1, after a block of `frames`.
    fn publish_live(&self, frames: usize) {
        if frames == 0 {
            return;
        }
        for fill in &self.schedule.fills {
            if fill.live == NO_CELL {
                continue;
            }
            // SAFETY: the fill buffer holds `max_block` floats, and the
            // block just filled its first `frames`.
            let last = unsafe { *fill.buffer.add(frames - 1) };
            self.live.write(fill.live, fill.spec.normalize(last));
        }
    }

    fn apply_commands(&mut self) {
        let mut drained = false;
        while let Ok(command) = self.commands.pop() {
            drained = true;
            match command {
                Command::SetAudioInput(input) => {
                    if let Some(old) = std::mem::replace(&mut self.input, input) {
                        self.discard(Garbage::Input(old));
                    }
                    self.input_channels = 0;
                }
                Command::InsertModule {
                    slot,
                    module,
                    id,
                    displays,
                    triggers,
                } => {
                    if let Some(old) = self.slots[slot].replace(module) {
                        self.discard(Garbage::Module(old));
                    }
                    self.display_counts[slot] = displays;
                    self.displays.claim(slot, id, triggers);
                }
                Command::RemoveModule { slot } => {
                    if let Some(old) = self.slots[slot].take() {
                        self.discard(Garbage::Module(old));
                    }
                    self.display_counts[slot] = 0;
                    self.displays.release(slot);
                }
                Command::SwapSchedule(mut schedule) => {
                    // Structural values arrive with the schedule, never as
                    // independently ordered SetParam commands. Every copy
                    // receives them, including instances already in slots.
                    for &(slot, index, value) in &schedule.structural_params {
                        set_base(
                            &mut self.slots,
                            &mut schedule.fills,
                            &schedule.copies,
                            slot as usize,
                            usize::from(index),
                            value,
                        );
                    }
                    // A newly created right copy takes the running left
                    // instance's state at the swap boundary (ADR-0022).
                    for &(left, right) in &schedule.stereo_copies {
                        if partner(&self.schedule.stereo_copies, left as usize) == Some(right as usize) {
                            continue;
                        }
                        let (left, right) = (left as usize, right as usize);
                        let (source, target) = if left < right {
                            let (before, after) = self.slots.split_at_mut(right);
                            (&before[left], &mut after[0])
                        } else {
                            let (before, after) = self.slots.split_at_mut(left);
                            (&after[0], &mut before[right])
                        };
                        if let (Some(source), Some(target)) = (source.as_ref(), target.as_mut()) {
                            target.copy_state_from(source.as_ref());
                        }
                    }
                    schedule.take_tap_state_from(&self.schedule, self.last_frames);
                    crate::cable_fade::take_state_from(&mut schedule, &self.schedule, self.last_frames);
                    // Before the engine has played a block nobody heard a
                    // step, so nothing fades: a patch plays the same however
                    // many commits it took to load (rule 9).
                    if self.frame == 0 {
                        crate::cable_fade::finish_all(&mut schedule);
                    }
                    self.fades_settled = false;
                    // The new schedule has not run yet, so a second swap
                    // before the next block has no last values to read.
                    self.last_frames = 0;
                    let slots = &self.slots;
                    schedule.take_smooth_state_from(&self.schedule, |slot, param| {
                        slots[slot].as_ref().and_then(|module| module.param_glide(param))
                    });
                    // Cells the old schedule used and this one does not go
                    // first, so a cell that changes hands is never claimed
                    // and then released.
                    for &cell in &schedule.live_release {
                        self.live.release(cell);
                    }
                    for fill in &schedule.fills {
                        if fill.live != NO_CELL {
                            self.live.claim(fill.live, fill.key);
                        }
                    }
                    let old = std::mem::replace(&mut self.schedule, schedule);
                    self.discard(Garbage::Schedule(old));
                    // A schedule with no fades running settles at once.
                    self.publish_fades_settled();
                    self.seed_midi_maps();
                    self.stats.schedule_swaps.fetch_add(1, Ordering::Relaxed);
                }
                Command::SetParam {
                    slot,
                    index,
                    value,
                    edit,
                } => {
                    set_base(
                        &mut self.slots,
                        &mut self.schedule.fills,
                        &self.schedule.copies,
                        slot,
                        index,
                        value,
                    );
                    if let Some((control, token)) = edit {
                        // The mapping on it starts again from this value. Its
                        // id tells an edit on a mapping learned just now, and
                        // not in this schedule yet, from one that plays.
                        let maps = &self.schedule.midi_maps;
                        let start = maps.partition_point(|map| map.control < control);
                        if let Some(map) = maps[start..].first().filter(|map| {
                            map.control == control && map.slot as usize == slot && usize::from(map.param) == index
                        }) {
                            let state = &mut self.midi_states[control.index()];
                            if state.id == map.id {
                                state.changed_elsewhere(&map.options, &map.spec, value);
                            }
                        }
                        self.feedback_sequence = self.feedback_sequence.wrapping_add(1);
                        self.midi_feedback.write_edit(control, token, self.feedback_sequence);
                    }
                }
                Command::UpdateAssignment {
                    id,
                    depth,
                    enabled,
                    polarity,
                } => {
                    if let Ok(index) = self
                        .schedule
                        .assignments
                        .binary_search_by_key(&id, |assignment| assignment.id)
                    {
                        // Only the settings change here. The pass after the
                        // batch starts the ramps, so a batch that ends where
                        // it began starts none (rule 9).
                        let assignment = &mut self.schedule.assignments[index];
                        assignment.depth = depth;
                        assignment.enabled = enabled;
                        assignment.polarity = polarity;
                        assignment.changed = true;
                        self.schedule.assignments_changed = true;
                    }
                }
                Command::ResetModule { slot } => {
                    // Reset every voice and side of the module.
                    for slot in std::iter::once(slot).chain(partners(&self.schedule.copies, slot)) {
                        if let Some(module) = self.slots[slot].as_mut() {
                            module.reset();
                        }
                    }
                }
                Command::SelectMidiClock(input, reconnect) => self.select_midi_clock(input, reconnect),
                Command::SetTempo(tempo) => self.apply_local_transport(TransportRequest {
                    tempo: Some(tempo),
                    ..TransportRequest::default()
                }),
                Command::SetTimeSignature(time_signature) => self.transport.set_time_signature(time_signature),
                Command::SetRunning(running) => {
                    self.local_transport.running = running;
                    self.apply_local_transport(TransportRequest {
                        running: Some(running),
                        ..TransportRequest::default()
                    });
                }
                Command::ResetTransport => self.apply_local_transport(TransportRequest {
                    reset: true,
                    ..TransportRequest::default()
                }),
                Command::SetOutputSafety(mode) => self.safety.set_mode(mode),
                Command::FadeOut => self.safety.fade_out(),
                Command::Panic => self.start_panic(),
                Command::SetPanicTrigger(trigger) => self.panic_trigger.set(trigger),
                Command::OpenInput { input, held } => {
                    let index = usize::from(input.0);
                    // Tracking an input again releases its old held notes.
                    // Device configuration can already be seeded by the rig.
                    if let Some(old) = self.input_notes.release(input) {
                        self.discard(Garbage::HeldSet(old));
                    }
                    match self.input_notes.inputs.get_mut(index) {
                        Some(slot) => *slot = Some(held),
                        None => self.discard(Garbage::HeldSet(held)),
                    }
                }
                Command::ReleaseInput(input) => {
                    self.midi_config.clear(input.0);
                    if let Some(old) = self.input_notes.release(input) {
                        self.discard(Garbage::HeldSet(old));
                    }
                }
            }
        }
        if drained {
            self.stats.command_drains.fetch_add(1, Ordering::Release);
        }
        // After the whole batch, so several changes act as one (rule 9).
        crate::cable_fade::start_setting_ramps(&mut self.schedule, self.frame > 0);
    }

    /// Hands memory back to the control thread for freeing.
    fn discard(&mut self, garbage: Garbage) {
        if let Err(rtrb::PushError::Full(garbage)) = self.garbage.push(garbage) {
            // Never free on the audio thread. Leaking is the lesser evil, and
            // the counter makes it visible. The garbage queue covers the
            // command queue plus the initial MIDI snapshot, so this should not happen.
            std::mem::forget(garbage);
            self.stats.leaked.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Reads the probed buffers after a block of `frames` frames and queues
    /// meter readings and scope chunks. Drops what does not fit.
    fn send_telemetry(&mut self, frames: usize) {
        for probe in &self.schedule.probes {
            // SAFETY: probe buffers are output buffers of the schedule,
            // `max_block` floats long, and `frames <= max_block`. No module
            // is running, so nothing writes to them now.
            let samples = unsafe { std::slice::from_raw_parts(probe.buffer, frames) };
            // SAFETY: the optional right buffer has the same length and
            // lifetime as the left buffer described above.
            let right = probe
                .right
                .map(|ptr| unsafe { std::slice::from_raw_parts(ptr, frames) });
            // Skip the work for a queue that is already full. The push below
            // still counts a drop if the queue fills in between.
            if probe.kind.meter() && self.telemetry.meters.is_full() {
                self.stats.dropped_meters.fetch_add(1, Ordering::Relaxed);
            } else if probe.kind.meter() {
                let (peak, rms) = measure(samples);
                let reading = MeterReading {
                    probe: probe.id,
                    frame: self.frame,
                    frames: frames as u32,
                    peak,
                    rms,
                    right: right.map(measure),
                };
                if self.telemetry.meters.push(reading).is_err() {
                    self.stats.dropped_meters.fetch_add(1, Ordering::Relaxed);
                }
            }
            if probe.kind.scope() {
                for (i, part) in samples.chunks(SCOPE_CHUNK).enumerate() {
                    if self.telemetry.scope.is_full() {
                        self.stats.dropped_scope.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let mut chunk = ScopeChunk {
                        probe: probe.id,
                        frame: self.frame + (i * SCOPE_CHUNK) as u64,
                        len: part.len() as u32,
                        samples: [0.0; SCOPE_CHUNK],
                    };
                    if let Some(right) = right {
                        let start = i * SCOPE_CHUNK;
                        for (out, (&left, &right)) in chunk.samples.iter_mut().zip(part.iter().zip(&right[start..])) {
                            *out = (left + right) * 0.5;
                        }
                    } else {
                        chunk.samples[..part.len()].copy_from_slice(part);
                    }
                    if self.telemetry.scope.push(chunk).is_err() {
                        self.stats.dropped_scope.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        for pair in &self.schedule.pairs {
            // SAFETY: as for the probes above: these are output buffers of
            // the schedule, `max_block` floats long, `frames <= max_block`, and
            // no module is running, so nothing writes to them now.
            let read = |ptr: *const f32| unsafe { std::slice::from_raw_parts(ptr, frames) };
            let (x, y) = (read(pair.x), read(pair.y));
            let (x_right, y_right) = (pair.x_right.map(read), pair.y_right.map(read));
            for start in (0..frames).step_by(PAIR_CHUNK) {
                if self.telemetry.pairs.is_full() {
                    self.stats.dropped_pairs.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                let len = PAIR_CHUNK.min(frames - start);
                let mut chunk = PairChunk {
                    probe: pair.id,
                    frame: self.frame + start as u64,
                    len: len as u32,
                    values: [0.0; PAIR_CHUNK * 2],
                };
                let side = |left: &[f32], right: Option<&[f32]>, i: usize| match right {
                    Some(right) => (left[i] + right[i]) * 0.5,
                    None => left[i],
                };
                for (i, out) in chunk.values[..len * 2].as_chunks_mut::<2>().0.iter_mut().enumerate() {
                    out[0] = side(x, x_right, start + i);
                    out[1] = side(y, y_right, start + i);
                }
                if self.telemetry.pairs.push(chunk).is_err() {
                    self.stats.dropped_pairs.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    fn collect_midi(&mut self, frames: usize) {
        self.midi.clear();
        while let Ok(mut config) = self.midi_config_in.pop() {
            self.midi_config.swap(&mut config);
            self.discard(Garbage::MidiConfig(config));
        }
        // Note offs from inputs released since the last buffer go first.
        for event in self
            .input_notes
            .releases
            .drain(..self.input_notes.releases.len().min(self.midi.capacity()))
        {
            self.midi.push(event);
        }
        // Finish synthetic releases before accepting new notes under a reused id.
        // The bounded release list may span several buffers after a mass disconnect.
        if !self.input_notes.releases.is_empty() {
            self.midi_config.begin_block(&self.midi);
            return;
        }
        let last = frames.saturating_sub(1) as u32;
        // Releases share this buffer with ingress. Leave accepted events queued
        // once it fills, rather than consuming a note that cannot be delivered.
        while self.midi.len() < self.midi.capacity() {
            let Ok(mut event) = self.midi_in.pop() else {
                break;
            };
            if !self.input_notes.follow(&event) {
                continue;
            }
            event.frame = event.frame.min(last);
            // Insertion sort by frame (stable). Lists are short, and this
            // keeps the audio thread free of allocation.
            let at = self.midi.partition_point(|e| e.frame <= event.frame);
            self.midi.insert(at, event);
        }
        self.midi_config.begin_block(&self.midi);
    }

    /// Records incoming events for MIDI learn and schedules mapped changes
    /// for the next absolute grid frame (MIDI learn, rules 4 and 6).
    fn collect_mapped_controls(&mut self) {
        for event in &self.midi {
            if let MidiMessage::NoteOn { channel, note, .. } = event.message() {
                self.cc_serial = self.cc_serial.wrapping_add(1).max(1);
                self.midi_feedback.record_note(channel, note, self.cc_serial);
                continue;
            }
            let MidiMessage::ControlChange {
                channel,
                controller,
                value,
            } = event.message()
            else {
                continue;
            };
            let control = MidiControl { channel, controller };
            self.cc_serial = self.cc_serial.wrapping_add(1).max(1);
            self.midi_feedback.record(control, value, self.cc_serial);
            let maps = &self.schedule.midi_maps;
            let start = maps.partition_point(|map| map.control < control);
            if maps.get(start).is_none_or(|map| map.control != control) {
                continue;
            }
            if self.mapped_controls.len() == self.mapped_controls.capacity() {
                self.stats.dropped_midi.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            // Ingress is sorted by frame. Changes at the same grid line
            // retain that order, including across two callbacks.
            let frame = (self.frame + u64::from(event.frame)).next_multiple_of(TRANSPORT_GRID as u64);
            self.mapped_controls.push(MappedControl { frame, control, value });
        }
    }

    /// Applies the mapped value before rendering its grid frame.
    fn apply_mapped_control(&mut self, MappedControl { control, value, .. }: MappedControl) {
        let maps = &self.schedule.midi_maps;
        let start = maps.partition_point(|map| map.control < control);
        for map in maps[start..].iter().take_while(|map| map.control == control) {
            match self.midi_states[control.index()].control_change(&map.options, &map.spec, value) {
                ControlOutcome::Set(base) => {
                    set_mapped(
                        &mut self.slots,
                        &mut self.schedule.fills,
                        &self.schedule.copies,
                        map,
                        base,
                    );
                    self.feedback_sequence = self.feedback_sequence.wrapping_add(1);
                    self.midi_feedback
                        .write(control, map.id, self.feedback_sequence, value, base);
                }
                ControlOutcome::Waiting => {
                    self.feedback_sequence = self.feedback_sequence.wrapping_add(1);
                    self.midi_feedback
                        .write_waiting(control, map.id, self.feedback_sequence, value);
                }
                ControlOutcome::Nothing => {}
            }
        }
    }

    /// After a swap, brings each control's state and the new schedule into
    /// step (MIDI learn, rule 2, and Mapping modes):
    ///
    /// - A new mapping id starts its state from the compiled value, and the
    ///   module takes that value, so a control change a renewed mapping
    ///   dropped stops playing as well as showing.
    /// - A control change applied to the old schedule after the new one was
    ///   compiled is missing from the new one's base values. Sets its result
    ///   again, unless a screen edit came after it. The state already has it,
    ///   so a step is not taken twice. Screen edits made after the compile are
    ///   queued behind the swap, so they still win.
    /// - Otherwise a compiled value that differs was changed from somewhere
    ///   else. An unchanged one keeps the state's unrounded position, so an
    ///   unrelated commit does not undo an encoder's small steps.
    fn seed_midi_maps(&mut self) {
        for map in &self.schedule.midi_maps {
            let state = &mut self.midi_states[map.control.index()];
            if state.id != map.id {
                let waiting = map.options.mode == MidiMode::Pickup && !map.caught;
                *state = ControlState::seeded(map.id, &map.spec, map.base, waiting);
                // A renewed mapping (a new range) drops a control change the
                // old one applied after the graph last synced: the controller
                // never copies it, so the module goes back to the compiled
                // value too, and what plays is what shows. For a mapping that
                // is new, the module holds this value already.
                set_mapped(
                    &mut self.slots,
                    &mut self.schedule.fills,
                    &self.schedule.copies,
                    map,
                    map.base,
                );
                continue;
            }
            let entry = self.midi_feedback.read(map.control);
            let (token, edited) = self.midi_feedback.read_edit(map.control);
            let newest = token == 0 || later(entry.sequence, edited);
            if entry.id == map.id && !entry.waiting && Some(entry.sequence) != map.synced && newest {
                let value = self.midi_feedback.result(map.control);
                set_mapped(
                    &mut self.slots,
                    &mut self.schedule.fills,
                    &self.schedule.copies,
                    map,
                    value,
                );
            } else if state.value != map.base {
                state.changed_elsewhere(&map.options, &map.spec, map.base);
            }
        }
    }

    fn run_block(&mut self, position: usize, frames: usize, left: &mut [f32], right: &mut [f32]) {
        let sample_rate = self.config.sample_rate;
        let input_channels = self.input_channels;
        let audio_input = &self.input_buffer[..frames];
        let midi = &self.midi[..];
        let slots = &mut self.slots;
        let transport = self.transport.info(self.frame);
        let Schedule {
            input_ptrs,
            output_ptrs,
            param_ptrs,
            ops,
            taps,
            loop_ops,
            loop_taps,
            steps,
            outputs,
            transport: transport_buffers,
            fills,
            assignments,
            downmixes,
            output_downmixes,
            right_ptrs,
            bundle_ports,
            cable_fades,
            output_fades,
            ..
        } = &mut *self.schedule;

        // Transport signals first, once per block, for every input that
        // follows them. Each sample's value comes from the anchor, so it is
        // the same for any block size (rule 5).
        if let Some(buffer) = transport_buffers.bar_phase {
            // SAFETY: transport buffers are schedule buffers of `max_block`
            // floats, `frames <= max_block`, modules only read them, and no
            // module is running yet.
            let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, frames) };
            for (i, value) in buffer.iter_mut().enumerate() {
                *value = transport.bar_phase_at(i);
            }
        }
        if let Some(buffer) = transport_buffers.beat_phase {
            // SAFETY: as for the bar phase buffer above.
            let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, frames) };
            for (i, value) in buffer.iter_mut().enumerate() {
                *value = transport.beat_phase_at(i);
            }
        }
        if let Some(buffer) = transport_buffers.playing {
            // SAFETY: as for the bar phase buffer above.
            let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, frames) };
            buffer.fill(if transport.playing() { 1.0 } else { 0.0 });
        }

        let mut run_op = |op: &ModuleOp, offset: usize, len: usize, cable_fades: &mut [InputFade]| {
            for downmix in &downmixes[op.downmixes.0 as usize..op.downmixes.1 as usize] {
                run_downmix(downmix, offset, len);
            }
            for index in op.fills.0..op.fills.1 {
                fill_param(&mut fills[index as usize], assignments, offset, len);
            }
            // Cable fades read what the module would read, so they run
            // after its downmixes, right before it (rule 9).
            crate::cable_fade::run(
                &mut cable_fades[op.cable_fades.0 as usize..op.cable_fades.1 as usize],
                offset,
                len,
            );
            if let Some(module) = slots[op.slot as usize].as_mut() {
                let ctx = ProcessContext {
                    sample_rate,
                    midi,
                    midi_config: Some(&self.midi_config),
                    midi_base: position + offset,
                    frames: len,
                    transport: transport.later(offset),
                    channel: op.channel,
                    voice: op.voice,
                    voices: op.voices,
                    audio_input: if input_channels == 0 {
                        &[]
                    } else {
                        &audio_input[offset..offset + len]
                    },
                    input_channels,
                };
                // SAFETY: the compiler gives every output its own buffer of
                // `max_block` floats. Inputs point at other modules' outputs,
                // a unit delay buffer, the zero buffer, or a default buffer.
                // Cycles always pass through a unit delay, so no module reads
                // its own output. Parameter pointers address the buffers just
                // filled above. `offset + len <= frames <= max_block`.
                let mut io = unsafe {
                    PortIo::new(
                        &input_ptrs[op.inputs.0 as usize..op.inputs.1 as usize],
                        &output_ptrs[op.outputs.0 as usize..op.outputs.1 as usize],
                        &param_ptrs[op.params.0 as usize..op.params.1 as usize],
                        &right_ptrs[op.right.0 as usize..op.right.1 as usize],
                        &bundle_ports[op.bundles.0 as usize..op.bundles.1 as usize],
                        op.connected,
                        offset,
                        len,
                    )
                    .with_defaults(op.defaulted)
                };
                module.process(&ctx, &mut io);
            }
        };

        for step in steps.iter() {
            match *step {
                Step::Module(op) => run_op(&ops[op as usize], 0, frames, cable_fades),
                Step::Tap(t) => {
                    let tap = &mut taps[t as usize];
                    if let Some(fade) = tap.fade {
                        crate::cable_fade::run(&mut cable_fades[fade as usize..=fade as usize], 0, frames);
                    }
                    // SAFETY: unit delay buffers are `max_block` long, and the
                    // input and output buffers are distinct.
                    unsafe {
                        let input = std::slice::from_raw_parts(tap.input, frames);
                        let output = std::slice::from_raw_parts_mut(tap.output, frames);
                        output[0] = tap.state;
                        output[1..].copy_from_slice(&input[..frames - 1]);
                        tap.state = input[frames - 1];
                    }
                }
                Step::Loop {
                    ops: op_range,
                    taps: tap_range,
                } => {
                    let region_ops = &loop_ops[op_range.0 as usize..op_range.1 as usize];
                    let region_taps = &loop_taps[tap_range.0 as usize..tap_range.1 as usize];
                    for i in 0..frames {
                        // Unit delays emit last sample's value first...
                        for &t in region_taps {
                            let tap = &taps[t as usize];
                            // SAFETY: `i < frames <= max_block`.
                            unsafe { *tap.output.add(i) = tap.state };
                        }
                        // ...then the loop's modules run for one sample...
                        for &op in region_ops {
                            run_op(&ops[op as usize], i, 1, cable_fades);
                        }
                        // ...then unit delays capture this sample's value.
                        for &t in region_taps {
                            let tap = &mut taps[t as usize];
                            if let Some(fade) = tap.fade {
                                crate::cable_fade::run(&mut cable_fades[fade as usize..=fade as usize], i, 1);
                            }
                            // SAFETY: `i < frames <= max_block`.
                            tap.state = unsafe { *tap.input.add(i) };
                        }
                    }
                }
            }
        }

        for downmix in &downmixes[output_downmixes.0 as usize..output_downmixes.1 as usize] {
            run_downmix(downmix, 0, frames);
        }
        crate::cable_fade::run(
            &mut cable_fades[output_fades.0 as usize..output_fades.1 as usize],
            0,
            frames,
        );
        for output in outputs.iter() {
            // SAFETY: output pointers reference schedule buffers of
            // `max_block` floats.
            let (l, r) = unsafe {
                (
                    std::slice::from_raw_parts(output.left, frames),
                    std::slice::from_raw_parts(output.right, frames),
                )
            };
            for (out, value) in left.iter_mut().zip(l) {
                *out += value;
            }
            for (out, value) in right.iter_mut().zip(r) {
                *out += value;
            }
        }
        let counts = self.safety.process(left, right);
        // One add per block, only when there is something to add.
        if counts.silenced > 0 {
            self.stats.output_silenced.fetch_add(counts.silenced, Ordering::Relaxed);
        }
        if counts.clipped > 0 {
            self.stats.output_clipped.fetch_add(counts.clipped, Ordering::Relaxed);
        }

        self.transport.end_block();
    }
}

/// Sets the base value of a mapped parameter and of every parameter that
/// follows its mapping (the copies of a stacked sub-patch).
fn set_mapped(
    slots: &mut [Option<Box<dyn Module>>],
    fills: &mut [ParamFill],
    copies: &[(u32, u32)],
    map: &MidiMapOp,
    value: f32,
) {
    set_base(slots, fills, copies, map.slot as usize, usize::from(map.param), value);
    for &(slot, param) in &map.followers {
        set_base(slots, fills, copies, slot as usize, usize::from(param), value);
    }
}

/// Sets a parameter's base value: on the module, and on its fill if it is
/// modulated, where a smoothed base glides to it. A module that runs per
/// voices and sides gets the value on every copy.
fn set_base(
    slots: &mut [Option<Box<dyn Module>>],
    fills: &mut [ParamFill],
    copies: &[(u32, u32)],
    slot: usize,
    index: usize,
    value: f32,
) {
    for slot in std::iter::once(slot).chain(partners(copies, slot)) {
        if let Some(module) = slots[slot].as_mut() {
            module.set_param(index, value);
        }
        let slot = slot as u32;
        let index = index as u16;
        for fill in fills.iter_mut() {
            if fill.slot == slot && fill.param == index {
                fill.base = value;
                if let Some(smooth) = fill.smooth.as_mut() {
                    smooth.set_target(fill.spec.normalize(value));
                }
                break;
            }
        }
    }
}

/// Fills a downmix buffer for `len` frames at `offset` (stereo spec, rule 11).
fn run_downmix(downmix: &DownmixOp, offset: usize, len: usize) {
    // SAFETY: the compiler gives every downmix its own output buffer of
    // `max_block` floats, and its inputs are other schedule buffers of the
    // same length that nothing writes while it runs. `offset + len` stays
    // inside `max_block`.
    let (left, right, output) = unsafe {
        (
            std::slice::from_raw_parts(downmix.left.add(offset), len),
            std::slice::from_raw_parts(downmix.right.add(offset), len),
            std::slice::from_raw_parts_mut(downmix.output.add(offset), len),
        )
    };
    for ((out, &l), &r) in output.iter_mut().zip(left).zip(right) {
        *out = downmix.kind.apply(l, r);
    }
}

/// Writes the modulated value of one parameter for `len` frames at `offset`.
///
/// A settled base with nothing added keeps the base bits, so depth 0 matches
/// no assignment. Otherwise the sum is mapped back through the parameter.
fn fill_param(fill: &mut ParamFill, assignments: &[AssignmentRuntime], offset: usize, len: usize) {
    let spec = fill.spec;
    let base = fill.base;
    // Constant for the call. Used when the base has no smoother.
    let base_position = spec.normalize(base);
    // SAFETY: the compiler allocated `buffer` for `max_block` floats, and
    // `offset + len` stays inside that. The audio thread does not alias it
    // with a source buffer: sources are output buffers or delay taps.
    let buf = unsafe { std::slice::from_raw_parts_mut(fill.buffer.add(offset), len) };
    for (i, value) in buf.iter_mut().enumerate() {
        let mut added = 0.0;
        let mut contributed = false;
        for source in &mut fill.sources {
            match source {
                ModSource::Live {
                    state,
                    buffer,
                    ramp,
                    last,
                    ..
                } => {
                    let assignment = &assignments[*state as usize];
                    let silent = assignment.is_silent();
                    // A contribution that fades in, moves, or blends to a
                    // new setting (rule 9). The ramp steps every sample,
                    // even while the source adds nothing, so it always
                    // ends. A silent source past its ramp is skipped, not
                    // multiplied by 0, because 0 · NaN is NaN.
                    let fading = ramp.as_ref().is_some_and(|ramp| !ramp.is_done());
                    if silent && !fading {
                        *last = 0.0;
                        continue;
                    }
                    // A ramp from a held value does not read the source, so
                    // a NaN source never reaches it.
                    let reads = !silent || ramp.as_ref().is_some_and(|ramp| ramp.reads_source());
                    let sample = if reads {
                        // SAFETY: `buffer` is an output or delay tap of
                        // `max_block` floats, and `offset + i < max_block`.
                        unsafe { *buffer.add(offset + i) }
                    } else {
                        0.0
                    };
                    let live = if silent { 0.0 } else { assignment.contribution(sample) };
                    let value = match ramp {
                        Some(ramp) => ramp.next(sample, live),
                        None => live,
                    };
                    *last = value;
                    added += value;
                    contributed = true;
                }
                // The last contribution of an assignment that left,
                // ramping to 0. It counts until the ramp ends.
                ModSource::Leaving { ramp, .. } => {
                    if !ramp.is_done() {
                        added += ramp.next(0.0);
                        contributed = true;
                    }
                }
            }
        }
        let position = if let Some(smooth) = fill.smooth.as_mut() {
            smooth.tick()
        } else {
            base_position
        };
        let settled = fill
            .smooth
            .as_ref()
            .is_none_or(|smooth| smooth.current() == smooth.target());
        *value = if !contributed && settled {
            base
        } else {
            spec.denormalize(position + added)
        };
    }
}

#[cfg(test)]
mod tests;
