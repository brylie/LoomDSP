//! MIDI input modules (FR-LIB-07).

use loom_core::signal::pitch;
use loom_core::{
    Channels, DisplayKind, DisplaySpec, MidiMessage, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions,
    ParamSpec, PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};

/// `loom.midi.note`: monophonic MIDI note input with last-note priority.
pub static NOTE_IN: ModuleDescriptor = ModuleDescriptor {
    id: "loom.midi.note",
    version: 1,
    name: "MIDI Note",
    category: "MIDI",
    family: loom_core::Family::Interface,
    description: "Monophonic note input (last-note priority). Pitch includes pitch bend.",
    inputs: &[],
    outputs: &[
        PortSpec {
            name: "pitch",
            label: "Pitch",
            signal: SignalType::Pitch,
            description: "Pitch of the last note held, with pitch bend. 0 is middle C.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "gate",
            label: "Gate",
            signal: SignalType::Gate,
            description: "High while any note is held.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "velocity",
            label: "Velocity",
            signal: SignalType::Modulation,
            description: "How hard the last note was played, from 0 to 1.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Span(0.0, 1.0),
            channels: Channels::PerChannel,
        },
    ],
    params: &[
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
    panel: Panel::new(3, 2, &[Widget::light(0, 0, "gate")]),
    displays: &[DisplaySpec {
        name: "gate",
        label: "Gate",
        description: "Lit while a key is held.",
        kind: DisplayKind::Level,
    }],
};

const MAX_HELD: usize = 16;

#[derive(Default, Clone)]
struct NoteIn {
    held: [u8; MAX_HELD],
    held_len: usize,
    note: u8,
    velocity: f32,
    bend: f32,
    channel: u8,
    bend_range: f32,
}

impl NoteIn {
    /// True when a message on `channel` (0 to 15) passes the `listen`
    /// setting, where 0 means every channel and 1 to 16 pick one.
    fn accepts(listen: u8, channel: u8) -> bool {
        listen == 0 || listen == channel + 1
    }

    fn release(&mut self, note: u8) {
        if let Some(i) = self.held[..self.held_len].iter().position(|&n| n == note) {
            self.held.copy_within(i + 1..self.held_len, i);
            self.held_len -= 1;
        }
    }

    fn press(&mut self, note: u8, velocity: u8) {
        self.release(note);
        if self.held_len == MAX_HELD {
            // Forget the oldest held note.
            self.held.copy_within(1.., 0);
            self.held_len -= 1;
        }
        self.held[self.held_len] = note;
        self.held_len += 1;
        self.note = note;
        self.velocity = f32::from(velocity) / 127.0;
    }

    fn handle(&mut self, message: MidiMessage, listen: u8) {
        match message {
            MidiMessage::NoteOn {
                channel,
                note,
                velocity,
            } if Self::accepts(listen, channel) => self.press(note, velocity),
            MidiMessage::NoteOff { channel, note } if Self::accepts(listen, channel) => {
                self.release(note);
                if self.held_len > 0 {
                    self.note = self.held[self.held_len - 1];
                }
            }
            MidiMessage::PitchBend { channel, value } if Self::accepts(listen, channel) => {
                self.bend = f32::from(value) / 8192.0;
            }
            _ => {}
        }
    }
}

impl Module for NoteIn {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let channel = io.param(0);
        let bend_range = io.param(1);
        let ([], [pitch_out, gate_out, velocity_out]) = io.split::<0, 3>();
        let mut events = ctx.midi().peekable();
        for i in 0..pitch_out.len() {
            // Modulated values live only for this frame. The fields keep the
            // knob's base, so removing an assignment returns to it.
            let listen = channel.map_or(i, self.channel, |v| v.round() as u8);
            let bend_range = bend_range.at_or(i, self.bend_range);
            while let Some((frame, event)) = events.peek() {
                if *frame > i {
                    break;
                }
                self.handle(event.message(), listen);
                events.next();
            }
            let bend = self.bend * bend_range * pitch::PER_SEMITONE;
            pitch_out[i] = pitch::from_midi_note(f32::from(self.note)) + bend;
            gate_out[i] = if self.held_len > 0 { 1.0 } else { 0.0 };
            velocity_out[i] = self.velocity;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.channel = value.round() as u8,
            1 => self.bend_range = value,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.held_len = 0;
        self.bend = 0.0;
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = if self.held_len > 0 { 1.0 } else { 0.0 };
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
        9,
        5,
        &[
            Widget::select(0, 0, "source"),
            // Shown only for the Controller source (ADR-0030).
            Widget::knob(0, 2, "controller"),
            Widget::knob(3, 2, "channel"),
            Widget::knob(6, 2, "smoothing"),
        ],
    ),
    displays: &[],
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
        let source = io.param(0);
        let controller = io.param(1);
        let channel = io.param(2);
        let smoothing = io.param(3);
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
            0 => self.source = value.round().clamp(0.0, 2.0) as u8,
            1 => self.controller = value.round().clamp(0.0, 127.0) as u8,
            2 => self.channel = value.round().clamp(0.0, 16.0) as u8,
            3 => self.coefficient = Self::coefficient(value, self.sample_rate),
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
mod tests {
    use super::*;
    use loom_core::testing::run;
    use loom_core::{MidiEvent, ModuleConfig};

    /// The `gate` display value is 1 while a note is held and 0 after its
    /// note-off (#106).
    #[test]
    fn display_shows_a_held_note() {
        let ModuleRole::Dsp(create) = NOTE_IN.role else {
            unreachable!()
        };
        let mut note_in = create(&ModuleConfig {
            sample_rate: 1_000.0,
            max_block: 64,
        });
        for (i, p) in NOTE_IN.params.iter().enumerate() {
            note_in.set_param(i, p.default);
        }
        let mut display = [f32::NAN];
        let (mut pitch, mut gate, mut velocity) = ([0.0; 64], [0.0; 64], [0.0; 64]);

        note_in.displays(&mut display);
        assert_eq!(display[0], 0.0, "no note yet");

        let on = [MidiEvent::note_on(10, 0, 60, 100)];
        run(
            note_in.as_mut(),
            1_000.0,
            &[],
            &[],
            &mut [&mut pitch, &mut gate, &mut velocity],
            &on,
        );
        note_in.displays(&mut display);
        assert_eq!(gate[63], 1.0);
        assert_eq!(display[0], 1.0, "held");

        let off = [MidiEvent::note_off(10, 0, 60)];
        run(
            note_in.as_mut(),
            1_000.0,
            &[],
            &[],
            &mut [&mut pitch, &mut gate, &mut velocity],
            &off,
        );
        note_in.displays(&mut display);
        assert_eq!(gate[63], 0.0);
        assert_eq!(display[0], 0.0, "released");
    }

    /// A controller module at 1 kHz with its defaults, then `params`.
    fn controller(params: &[(usize, f32)]) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = CONTROLLER_IN.role else {
            unreachable!()
        };
        let mut module = create(&ModuleConfig {
            sample_rate: 1_000.0,
            max_block: 512,
        });
        for (i, p) in CONTROLLER_IN.params.iter().enumerate() {
            module.set_param(i, p.default);
        }
        for &(i, value) in params {
            module.set_param(i, value);
        }
        module.reset();
        module
    }

    /// Plays `events` through `module` over `frames` frames, `block`
    /// frames at a time, and returns the output.
    fn play(module: &mut dyn Module, frames: usize, block: usize, events: &[MidiEvent]) -> Vec<f32> {
        let mut out = Vec::new();
        for start in (0..frames).step_by(block) {
            let len = block.min(frames - start);
            let in_block: Vec<MidiEvent> = events
                .iter()
                .filter(|e| (start..start + len).contains(&(e.frame as usize)))
                .map(|e| MidiEvent {
                    frame: e.frame - start as u32,
                    ..*e
                })
                .collect();
            let mut chunk = vec![f32::NAN; len];
            run(module, 1_000.0, &[], &[], &mut [&mut chunk], &in_block);
            out.extend(chunk);
        }
        out
    }

    /// With no smoothing, a controller change, a bend, and aftertouch land
    /// on the sample of their offset at block sizes 1, 77, and 512, with
    /// the ranges of the spec (p1-modules.md, Acceptance criteria).
    #[test]
    fn controller_events_land_on_their_sample() {
        /// A source, its events, and (frame, expected value) checks.
        type Case = (f32, Vec<MidiEvent>, Vec<(usize, f32)>);
        let cases: [Case; 3] = [
            (
                0.0,
                vec![
                    MidiEvent::control_change(100, 0, 1, 127),
                    MidiEvent::control_change(300, 0, 2, 50),
                    MidiEvent::control_change(450, 0, 1, 0),
                    MidiEvent::control_change(600, 3, 1, 64),
                ],
                vec![
                    (0, 0.0),
                    (99, 0.0),
                    (100, 1.0),
                    (300, 1.0),
                    (449, 1.0),
                    (450, 0.0),
                    (600, 64.0 / 127.0),
                ],
            ),
            (
                1.0,
                vec![
                    MidiEvent::pitch_bend(77, 0, 8191),
                    MidiEvent::pitch_bend(200, 5, -8192),
                    MidiEvent::pitch_bend(513, 0, 0),
                    MidiEvent::pitch_bend(700, 0, -4096),
                ],
                vec![
                    (0, 0.0),
                    (76, 0.0),
                    (77, 1.0),
                    (199, 1.0),
                    (200, -1.0),
                    (513, 0.0),
                    (700, -0.5),
                ],
            ),
            (
                2.0,
                vec![
                    MidiEvent::channel_pressure(1, 0, 127),
                    MidiEvent::channel_pressure(511, 0, 0),
                    MidiEvent::control_change(600, 0, 1, 127),
                ],
                vec![(0, 0.0), (1, 1.0), (510, 1.0), (511, 0.0), (700, 0.0)],
            ),
        ];
        for (source, events, checks) in cases {
            for block in [1, 77, 512] {
                let mut module = controller(&[(0, source), (3, 0.0)]);
                let out = play(module.as_mut(), 1_000, block, &events);
                for &(frame, value) in &checks {
                    assert!(
                        (out[frame] - value).abs() < 1e-6,
                        "source {source}, block {block}, frame {frame}: {} not {value}",
                        out[frame]
                    );
                }
            }
        }
    }

    /// Smoothing starts on the event's sample and gives the same output at
    /// every block size.
    #[test]
    fn controller_smoothing_is_block_size_invariant() {
        let events = [
            MidiEvent::control_change(100, 0, 1, 127),
            MidiEvent::control_change(140, 0, 1, 20),
        ];
        let reference = play(controller(&[(3, 0.05)]).as_mut(), 400, 400, &events);
        assert_eq!(reference[99], 0.0, "nothing before the event");
        assert!(reference[100] > 0.0 && reference[100] < 0.2, "{}", reference[100]);
        // About 99% of the way in 50 ms.
        assert!((reference[149 + 50] - 20.0 / 127.0).abs() < 0.01);
        for block in [1, 77, 512] {
            let out = play(controller(&[(3, 0.05)]).as_mut(), 400, block, &events);
            assert_eq!(out, reference, "block {block}");
        }
    }

    /// `channel` 0 listens to every channel, 1 to 16 to one. Changing the
    /// `controller` knob shows that controller's last value.
    #[test]
    fn controller_channel_and_number() {
        let events = [
            MidiEvent::control_change(0, 0, 7, 127),
            MidiEvent::control_change(1, 4, 1, 127),
        ];
        let out = play(controller(&[(2, 1.0), (3, 0.0)]).as_mut(), 10, 10, &events);
        assert_eq!(out[9], 0.0, "channel 1 ignores channel 5");
        let out = play(controller(&[(2, 5.0), (3, 0.0)]).as_mut(), 10, 10, &events);
        assert_eq!(out[9], 1.0, "channel 5");

        let mut module = controller(&[(3, 0.0)]);
        play(module.as_mut(), 10, 10, &events);
        module.set_param(1, 7.0);
        assert_eq!(play(module.as_mut(), 1, 1, &[])[0], 1.0, "controller 7 was set");
        module.reset();
        assert_eq!(play(module.as_mut(), 1, 1, &[])[0], 0.0, "reset forgets messages");
    }
}
