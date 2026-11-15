//! MIDI input modules (FR-LIB-07).

use loom_core::signal::pitch;
use loom_core::{
    Channels, DisplayKind, DisplayRange, DisplaySpec, MidiMessage, Module, ModuleDescriptor, ModuleRole, Panel,
    ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};

/// `loom.midi.note`: polyphonic MIDI notes with per-voice expression.
pub static NOTE_IN: ModuleDescriptor = ModuleDescriptor {
    id: "loom.midi.note",
    version: 1,
    name: "MIDI Note",
    category: "MIDI",
    family: loom_core::Family::Interface,
    description: "Allocates MIDI notes to voices, with pitch bend, pressure, and timbre.",
    inputs: &[PortSpec {
        name: "level",
        label: "Level",
        description: "Per-voice envelope level for Quietest stealing.",
        signal: SignalType::Modulation,
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Voices,
    }],
    outputs: &[
        PortSpec {
            name: "pitch",
            label: "Pitch",
            signal: SignalType::Pitch,
            description: "Pitch of this voice, with pitch bend. 0 is middle C.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Voices,
        },
        PortSpec {
            name: "gate",
            label: "Gate",
            signal: SignalType::Gate,
            description: "High while this voice holds a note.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Voices,
        },
        PortSpec {
            name: "velocity",
            label: "Velocity",
            signal: SignalType::Modulation,
            description: "How hard this voice was played, from 0 to 1.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Span(0.0, 1.0),
            channels: Channels::Voices,
        },
        PortSpec {
            name: "pressure",
            label: "Pressure",
            description: "Per-note pressure, from 0 to 1.",
            signal: SignalType::Modulation,
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Span(0.0, 1.0),
            channels: Channels::Voices,
        },
        PortSpec {
            name: "timbre",
            label: "Timbre",
            description: "Per-note CC74 expression, from 0 to 1.",
            signal: SignalType::Modulation,
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Span(0.0, 1.0),
            channels: Channels::Voices,
        },
    ],
    params: &[
        ParamSpec {
            name: "voices",
            label: "Voices",
            description: "Number of simultaneous voices.",
            min: 1.0,
            max: 16.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .shapes_graph(),
        ParamSpec {
            name: "mode",
            label: "Mode",
            description: "How notes steal voices, or how a single voice articulates notes.",
            min: 0.0,
            max: 3.0,
            default: 2.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Oldest", "Quietest", "Legato", "Retrigger"])
        .choices_by_count("voices", 2),
        ParamSpec {
            name: "channel",
            label: "Channel",
            description: "The MIDI channel to listen to, from 1 to 16. 0 listens to all.",
            min: 0.0,
            max: 16.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "bend-range",
            label: "Bend Range",
            description: "How far a full pitch bend moves the pitch.",
            min: 0.0,
            max: 48.0,
            default: 2.0,
            unit: "st",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|_| Box::new(NoteIn::default())),
    panel: Panel::new(
        12,
        5,
        &[
            Widget::segmented(0, 0, 12, "mode"),
            Widget::number(0, 2, "voices"),
            Widget::light(9, 2, "gate"),
        ],
    ),
    displays: &[DisplaySpec {
        name: "gate",
        label: "Gate",
        description: "Lit while a key is held.",
        kind: DisplayKind::Level,
        range: DisplayRange::UNIT,
    }],
    roles: &[],
};

/// One remembered key's raw expression, before manager/channel expression is added.
#[derive(Clone, Copy)]
struct Key {
    id: loom_dsp::voice::NoteId,
    velocity: f32,
    generation: u64,
    zone: Option<u8>,
    pressure: u8,
    serial: u64,
}

/// Channel expression belongs to an input, not just a MIDI channel number.
#[derive(Clone, Copy, Default)]
struct Expression {
    zone: Option<u8>,
    bend: i16,
    pressure: u8,
    timbre: u8,
}

/// MIDI source state; the engine owns zone configuration, even while unscheduled.
#[derive(Clone)]
struct NoteIn {
    allocator: loom_dsp::voice::Allocator,
    keys: [Option<Key>; 16],
    serial: u64,
    mono_velocity: f32,
    expression: [[Expression; 16]; loom_core::MAX_MIDI_INPUTS as usize],
    generations: [u64; loom_core::MAX_MIDI_INPUTS as usize],
    channel: u8,
    bend_range: f32,
    previous_range: Option<f32>,
    mode: usize,
}

impl Default for NoteIn {
    fn default() -> Self {
        Self {
            allocator: Default::default(),
            keys: [None; 16],
            serial: 0,
            mono_velocity: 0.0,
            expression: [[Expression::default(); 16]; loom_core::MAX_MIDI_INPUTS as usize],
            generations: [0; loom_core::MAX_MIDI_INPUTS as usize],
            channel: 0,
            bend_range: 2.0,
            previous_range: None,
            mode: 2,
        }
    }
}

impl NoteIn {
    /// Ordinary MIDI filtering used by Controller input.
    fn accepts(listen: u8, channel: u8) -> bool {
        listen == 0 || listen == channel + 1
    }

    const CHANNEL: usize = NOTE_IN.param_index("channel").unwrap();
    const BEND_RANGE: usize = NOTE_IN.param_index("bend-range").unwrap();
    const MODE: usize = NOTE_IN.param_index("mode").unwrap();
    const LEVEL: usize = NOTE_IN.input_index("level").unwrap();
    const PITCH: usize = NOTE_IN.output_index("pitch").unwrap();
    const GATE: usize = NOTE_IN.output_index("gate").unwrap();
    const VELOCITY: usize = NOTE_IN.output_index("velocity").unwrap();
    const PRESSURE: usize = NOTE_IN.output_index("pressure").unwrap();
    const TIMBRE: usize = NOTE_IN.output_index("timbre").unwrap();

    /// Packs all three identity parts without collisions, including untracked input 0.
    fn identity(input: u16, channel: u8, note: u8) -> loom_dsp::voice::NoteId {
        loom_dsp::voice::NoteId((u64::from(input) << 11) | (u64::from(channel) << 7) | u64::from(note))
    }

    /// Stored choice values remain stable even when their effective mode falls back.
    fn mode(value: usize) -> loom_dsp::voice::Mode {
        use loom_dsp::voice::Mode;
        match value {
            0 => Mode::Oldest,
            1 => Mode::Quietest,
            3 => Mode::Retrigger,
            _ => Mode::Legato,
        }
    }

    /// Converts raw expression using configuration at this exact sample.
    fn note(
        &self,
        key: Key,
        ctx: &ProcessContext<'_>,
        event_position: Option<usize>,
        ordinary_range: f32,
    ) -> loom_dsp::voice::Note {
        let input = (key.id.0 >> 11) as u16;
        let channel = ((key.id.0 >> 7) & 15) as u8;
        let config = ctx.midi_config_at_event(input, event_position);
        let member = self.expression[usize::from(input)][usize::from(channel)];
        let mut bend = f32::from(member.bend) / 8192.0 * config.bend_range(channel, ordinary_range);
        let mut pressure = member.pressure.max(key.pressure);
        let mut timbre = member.timbre;
        if let Some(manager) = config.manager(channel).filter(|m| *m != channel) {
            let manager_expression = self.expression[usize::from(input)][usize::from(manager)];
            bend += f32::from(manager_expression.bend) / 8192.0 * config.bend_range(manager, ordinary_range);
            pressure = pressure.max(manager_expression.pressure);
            timbre = timbre.max(manager_expression.timbre);
        }
        loom_dsp::voice::Note {
            id: key.id,
            pitch: pitch::from_midi_note((key.id.0 & 127) as f32) + bend * pitch::PER_SEMITONE,
            velocity: if self.allocator.voices().len() == 1 {
                self.mono_velocity
            } else {
                key.velocity
            },
            pressure: f32::from(pressure) / 127.0,
            timbre: f32::from(timbre) / 127.0,
        }
    }

    /// RP-053 2.1.4 resets only channels entering or leaving MPE control.
    fn sync_input(&mut self, input: usize, config: loom_core::MidiInputConfig) {
        if config.generation != self.generations[input] {
            self.expression[input].fill(Expression::default());
            self.generations[input] = config.generation;
        }
        for channel in 0..16u8 {
            let zone = config.manager(channel);
            let expression = &mut self.expression[input][usize::from(channel)];
            if expression.zone != zone {
                *expression = Expression {
                    zone,
                    ..Expression::default()
                };
            }
            for key in &mut self.keys {
                if key.is_some_and(|key| {
                    key.generation == config.generation
                        && (key.id.0 >> 11) as usize == input
                        && ((key.id.0 >> 7) & 15) as u8 == channel
                        && key.zone != zone
                }) {
                    self.allocator.note_off(key.take().expect("matching key exists").id);
                }
            }
        }
    }

    /// Refreshes held keys only. Released voices retain their last expression.
    fn refresh(&mut self, ctx: &ProcessContext<'_>, event_position: Option<usize>, range: f32) {
        let mut visited = 0u64;
        for key in self.keys.into_iter().flatten() {
            let input = (key.id.0 >> 11) as u16;
            if visited & (1u64 << input) == 0 {
                visited |= 1u64 << input;
                self.sync_input(usize::from(input), ctx.midi_config_at_event(input, event_position));
            }
        }
        for key in self.keys.into_iter().flatten() {
            let input = (key.id.0 >> 11) as u16;
            if key.generation == ctx.midi_config_at_event(input, event_position).generation {
                self.allocator.update_note(self.note(key, ctx, event_position, range));
            }
        }
    }

    /// Reclaims expression slots for stolen or removed voices.
    /// Mono's held-key history is retained until an actual capacity change.
    fn retain_sounding_keys(&mut self) {
        for key in &mut self.keys {
            if key.is_some_and(|key| self.allocator.voices_for(key.id).next().is_none()) {
                *key = None;
            }
        }
    }

    /// Consumes one event. Releases do not depend on the current channel filter.
    fn handle(
        &mut self,
        event: &loom_core::MidiEvent,
        listen: u8,
        range: f32,
        ctx: &ProcessContext<'_>,
        event_position: Option<usize>,
    ) {
        let input = usize::from(event.input);
        if input >= self.expression.len() {
            return;
        }
        let config = ctx.midi_config_at_event(event.input, event_position);
        self.sync_input(input, config);
        match event.message() {
            MidiMessage::NoteOn {
                channel,
                note,
                velocity,
            } if config.accepts(listen, channel) => {
                let id = Self::identity(event.input, channel, note);
                self.mono_velocity = f32::from(velocity) / 127.0;
                self.serial = self.serial.saturating_add(1);
                let key = Key {
                    id,
                    velocity: f32::from(velocity) / 127.0,
                    generation: config.generation,
                    zone: config.manager(channel),
                    pressure: 0,
                    serial: self.serial,
                };
                self.allocator.note_on(self.note(key, ctx, event_position, range));
                if self.allocator.voices().len() > 1 {
                    self.retain_sounding_keys();
                }
                let slot = self
                    .keys
                    .iter()
                    .position(|key| key.is_some_and(|key| key.id == id))
                    .or_else(|| self.keys.iter().position(Option::is_none))
                    .unwrap_or_else(|| {
                        self.keys
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, key)| key.map(|k| k.serial))
                            .unwrap()
                            .0
                    });
                self.keys[slot] = Some(key);
            }
            MidiMessage::NoteOff { channel, note } => {
                self.refresh(ctx, event_position, range);
                let id = Self::identity(event.input, channel, note);
                self.allocator.note_off(id);
                for key in &mut self.keys {
                    if key.is_some_and(|key| key.id == id) {
                        *key = None;
                    }
                }
            }
            MidiMessage::PitchBend { channel, value } => self.expression[input][usize::from(channel)].bend = value,
            MidiMessage::ChannelPressure { channel, value } => {
                self.expression[input][usize::from(channel)].pressure = value
            }
            MidiMessage::ControlChange {
                channel,
                controller: 74,
                value,
            } => self.expression[input][usize::from(channel)].timbre = value,
            MidiMessage::PolyPressure { channel, note, value } => {
                let id = Self::identity(event.input, channel, note);
                for key in self.keys.iter_mut().flatten() {
                    if key.id == id {
                        key.pressure = value;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Module for NoteIn {
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let capacity = io.output_width(Self::PITCH).voices;
        let channel = io.param(Self::CHANNEL);
        let bend_range = io.param(Self::BEND_RANGE);
        let mode = io.param(Self::MODE);
        let mut events = ctx.midi_indexed().peekable();
        let mut event_position = None;
        for frame in 0..io.frames() {
            let listen = channel.map_or(frame, self.channel, |v| v.round().clamp(0.0, 16.0) as u8);
            let range = bend_range.at_or(frame, self.bend_range);
            let mode = mode.map_or(frame, self.mode, |v| v.round().clamp(0.0, 3.0) as usize);
            let resized = self.allocator.voices().len() != capacity;
            let mut refresh = resized || self.previous_range != Some(range);
            self.allocator.configure(capacity, Self::mode(mode), 1);
            if resized {
                self.retain_sounding_keys();
                // Preserve the surviving output when crossing mono's last-note velocity rule.
                for key in self.keys.iter_mut().flatten() {
                    if let Some(note) = self
                        .allocator
                        .voices()
                        .iter()
                        .filter_map(|voice| voice.note)
                        .find(|note| note.id == key.id)
                    {
                        key.velocity = note.velocity;
                    }
                }
                if capacity == 1
                    && let Some(note) = self.allocator.voices()[0].note
                {
                    self.mono_velocity = note.velocity;
                }
            }
            let mut levels = [0.0; 16];
            for (voice, level) in levels[..capacity].iter_mut().enumerate() {
                let left = io.voice_input(Self::LEVEL, voice, 0)[frame];
                *level = if io.input_width(Self::LEVEL).sides == 2 {
                    left * 0.5 + io.voice_input(Self::LEVEL, voice, 1)[frame] * 0.5
                } else {
                    left
                };
            }
            self.allocator
                .begin_frame((!io.uses_default(Self::LEVEL)).then_some(&levels[..capacity]));
            while let Some((at, index, event)) = events.peek() {
                if *at > frame {
                    break;
                }
                event_position = Some(*index);
                self.handle(event, listen, range, ctx, event_position);
                refresh = true;
                events.next();
            }
            if refresh {
                self.refresh(ctx, event_position, range);
            }
            self.previous_range = Some(range);
            for (voice, state) in self.allocator.voices().iter().enumerate() {
                let note = state.note;
                io.voice_output(Self::PITCH, voice, 0)[frame] = note.map_or(-0.5, |note| note.pitch);
                io.voice_output(Self::GATE, voice, 0)[frame] = if state.gate { 1.0 } else { 0.0 };
                io.voice_output(Self::VELOCITY, voice, 0)[frame] = note.map_or(0.0, |note| note.velocity);
                io.voice_output(Self::PRESSURE, voice, 0)[frame] = note.map_or(0.0, |note| note.pressure);
                io.voice_output(Self::TIMBRE, voice, 0)[frame] = note.map_or(0.0, |note| note.timbre);
            }
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::CHANNEL => self.channel = value.round() as u8,
            Self::BEND_RANGE => self.bend_range = value,
            Self::MODE => self.mode = value.round() as usize,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.allocator.reset();
        self.keys.fill(None);
        self.serial = 0;
        self.mono_velocity = 0.0;
        self.previous_range = None;
        self.expression.fill([Expression::default(); 16]);
        self.generations.fill(0);
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = if self.allocator.voices().iter().any(|voice| voice.gate) {
            1.0
        } else {
            0.0
        };
    }
}

/// `loom.midi.controller`: one MIDI controller, the pitch bend, or channel
/// aftertouch as a signal (MT-IO-04).
///
/// Spec: `docs/design/p1-modules.md`, Controller input.
pub static CONTROLLER_IN: ModuleDescriptor = ModuleDescriptor {
    id: "loom.midi.controller",
    version: 1,
    name: "MIDI Controller",
    category: "MIDI",
    family: loom_core::Family::Interface,
    description: "Turns a MIDI controller, the pitch bend, or channel aftertouch into a signal. Controllers and aftertouch go from 0 to 1, bend from -1 to 1.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The chosen source: 0 to 1 for a controller or aftertouch, -1 to 1 for pitch bend. 0 until a message arrives.",
        default: PortDefault::Value(0.0),
        // Bipolar, as the widest case: a port's polarity is static.
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "source",
            label: "Source",
            description: "What to follow: a controller number, the pitch bend wheel, or channel aftertouch.",
            min: 0.0,
            max: 2.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Controller", "Pitch bend", "Aftertouch"]),
        ParamSpec {
            name: "controller",
            label: "Controller",
            description: "The controller number to follow, from 0 to 127, when the source is Controller.",
            min: 0.0,
            max: 127.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .shown_when("source", &[0]),
        ParamSpec {
            name: "channel",
            label: "Channel",
            description: "The MIDI channel to listen to, from 1 to 16. 0 listens to all.",
            min: 0.0,
            max: 16.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "smoothing",
            label: "Smoothing",
            description: "About how long the output takes to reach a new value. 0 jumps at once.",
            min: 0.0,
            max: 0.5,
            default: 0.005,
            unit: "s",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|config| Box::new(ControllerIn::new(config.sample_rate))),
    panel: Panel::new(
        11,
        5,
        &[
            Widget::select(0, 0, "source"),
            // Shown only for the Controller source (ADR-0030).
            Widget::number(6, 0, "controller"),
            Widget::knob(0, 2, "channel"),
            Widget::knob(3, 2, "smoothing"),
        ],
    ),
    displays: &[],
    roles: &[],
};

/// State for `loom.midi.controller`.
#[derive(Clone)]
struct ControllerIn {
    sample_rate: f32,
    /// The knob values. Modulated values count for one frame only.
    source: u8,
    controller: u8,
    channel: u8,
    /// The smoothing coefficient for the knob's `smoothing`.
    coefficient: f32,
    /// The last value of every controller number, from 0 to 1. Kept for
    /// all 128, so turning the `controller` knob shows that controller's
    /// last value at once.
    controllers: [f32; 128],
    /// The last pitch bend, from -1 to 1.
    bend: f32,
    /// The last channel pressure, from 0 to 1.
    pressure: f32,
    /// The smoothed output.
    out: f32,
}

impl ControllerIn {
    const SOURCE: usize = CONTROLLER_IN.param_index("source").unwrap();
    const CONTROLLER: usize = CONTROLLER_IN.param_index("controller").unwrap();
    const CHANNEL: usize = CONTROLLER_IN.param_index("channel").unwrap();
    const SMOOTHING: usize = CONTROLLER_IN.param_index("smoothing").unwrap();

    /// A module that has seen no messages, with the default parameters.
    fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            source: 0,
            controller: 1,
            channel: 0,
            coefficient: Self::coefficient(0.005, sample_rate),
            controllers: [0.0; 128],
            bend: 0.0,
            pressure: 0.0,
            out: 0.0,
        }
    }

    /// The one-pole coefficient that reaches about 99% of a new value in
    /// `seconds`, the same law as parameter smoothing. 0 seconds is 0: no
    /// smoothing.
    fn coefficient(seconds: f32, sample_rate: f32) -> f32 {
        if seconds.is_finite() && seconds > 0.0 {
            (-5.0 / (seconds * sample_rate).max(1.0)).exp()
        } else {
            0.0
        }
    }

    /// A 14-bit bend from -8192 to 8191 as -1 to 1, with the centre at 0
    /// and both ends reached exactly.
    fn bend_value(value: i16) -> f32 {
        if value < 0 {
            f32::from(value) / 8192.0
        } else {
            f32::from(value) / 8191.0
        }
    }

    /// Records one message, if it is on a channel `listen` accepts.
    fn handle(&mut self, message: MidiMessage, listen: u8) {
        match message {
            MidiMessage::ControlChange {
                channel,
                controller,
                value,
            } if NoteIn::accepts(listen, channel) => {
                self.controllers[usize::from(controller & 0x7F)] = f32::from(value) / 127.0;
            }
            MidiMessage::PitchBend { channel, value } if NoteIn::accepts(listen, channel) => {
                self.bend = Self::bend_value(value);
            }
            MidiMessage::ChannelPressure { channel, value } if NoteIn::accepts(listen, channel) => {
                self.pressure = f32::from(value) / 127.0;
            }
            _ => {}
        }
    }
}

impl Module for ControllerIn {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let source = io.param(Self::SOURCE);
        let controller = io.param(Self::CONTROLLER);
        let channel = io.param(Self::CHANNEL);
        let smoothing = io.param(Self::SMOOTHING);
        let ([], [out]) = io.split::<0, 1>();
        let mut events = ctx.midi().peekable();
        for (i, out) in out.iter_mut().enumerate() {
            let listen = channel.map_or(i, self.channel, |v| v.round().clamp(0.0, 16.0) as u8);
            // Every message at or before this frame lands on this frame,
            // so the output is the same at any block size.
            while let Some((frame, event)) = events.peek() {
                if *frame > i {
                    break;
                }
                self.handle(event.message(), listen);
                events.next();
            }
            let source = source.map_or(i, self.source, |v| v.round().clamp(0.0, 2.0) as u8);
            let number = controller.map_or(i, self.controller, |v| v.round().clamp(0.0, 127.0) as u8);
            let target = match source {
                0 => self.controllers[usize::from(number)],
                1 => self.bend,
                _ => self.pressure,
            };
            let coefficient = smoothing.map_or(i, self.coefficient, |seconds| {
                Self::coefficient(seconds, self.sample_rate)
            });
            self.out = target + (self.out - target) * coefficient;
            if (self.out - target).abs() < 1e-7 {
                // Land exactly, and keep denormals out.
                self.out = target;
            }
            *out = self.out;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::SOURCE => self.source = value.round().clamp(0.0, 2.0) as u8,
            Self::CONTROLLER => self.controller = value.round().clamp(0.0, 127.0) as u8,
            Self::CHANNEL => self.channel = value.round().clamp(0.0, 16.0) as u8,
            Self::SMOOTHING => self.coefficient = Self::coefficient(value, self.sample_rate),
            _ => {}
        }
    }

    fn reset(&mut self) {
        // Forget every message: the output is 0 again, as before the first.
        self.controllers = [0.0; 128];
        self.bend = 0.0;
        self.pressure = 0.0;
        self.out = 0.0;
    }
}

#[cfg(test)]
mod tests;
