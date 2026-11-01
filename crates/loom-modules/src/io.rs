//! Modules the engine runs itself: the unit delay and the audio output.

use loom_core::{Channels, ModuleDescriptor, ModuleRole, Panel, PortDefault, PortSpec, SignalType};

/// `loom.time.delay1`: delays its input by exactly one sample.
///
/// Put one in a feedback loop to choose where the loop's delay sits. A loop
/// without one gets a delay inserted automatically (ADR-0002). See
/// `docs/learn/feedback-loops.md`.
pub static UNIT_DELAY: ModuleDescriptor = ModuleDescriptor {
    id: "loom.time.delay1",
    version: 1,
    name: "1-Sample Delay",
    category: "Time",
    family: loom_core::Family::LevelAndRouting,
    description: "Delays the input by one sample. Marks where a feedback loop is broken.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal to delay.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The input, one sample later.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

/// `loom.io.audio-out`: sends audio to the speakers or host.
pub static AUDIO_OUT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.io.audio-out",
    version: 1,
    name: "Audio Out",
    category: "I/O",
    family: loom_core::Family::Interface,
    description: "Sends a mono or stereo signal to the audio output.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal to play. A mono cable plays on both sides.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Stereo,
    }],
    outputs: &[],
    params: &[],
    role: ModuleRole::AudioOutput,
    panel: Panel::EMPTY,
    displays: &[],
};

/// `loom.io.audio-in`: brings the selected input pair into a patch.
pub static AUDIO_IN: ModuleDescriptor = ModuleDescriptor {
    id: "loom.io.audio-in",
    version: 1,
    name: "Audio In",
    category: "I/O",
    family: loom_core::Family::Interface,
    description: "Brings a pair of channels from the selected audio input into the patch.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The selected input pair, scaled by gain and mute.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Stereo,
    }],
    params: &[
        loom_core::ParamSpec {
            name: "pair",
            label: "Pair",
            description: "The input channel pair to read.",
            min: 1.0,
            max: 4.0,
            default: 1.0,
            unit: "",
            options: loom_core::ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["1", "2", "3", "4"]),
        loom_core::ParamSpec {
            name: "gain",
            label: "Gain",
            description: "Scales the input level.",
            min: 0.0,
            max: 4.0,
            default: 1.0,
            unit: "",
            options: loom_core::ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        loom_core::ParamSpec {
            name: "mute",
            label: "Mute",
            description: "Fades the input to silence.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: loom_core::ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(AudioIn {
            pair: 1,
            gain: loom_rt::Smoothed::new(1.0, config.sample_rate, 0.005),
            open: loom_rt::Smoothed::new(1.0, config.sample_rate, 0.005),
            mute: false,
            active: false,
        })
    }),
    panel: Panel::new(
        12,
        6,
        &[
            loom_core::Widget::radio(0, 0, "pair"),
            loom_core::Widget::knob(6, 0, "gain"),
            loom_core::Widget::toggle(9, 0, "mute"),
            loom_core::Widget::light(9, 2, "active"),
        ],
    ),
    displays: &[loom_core::DisplaySpec {
        name: "active",
        label: "Active",
        description: "Lit when the selected input pair carries sound.",
        kind: loom_core::DisplayKind::Level,
    }],
};

/// Input source state; the engine supplies the same frames to every instance.
#[derive(Clone)]
struct AudioIn {
    pair: usize,
    gain: loom_rt::Smoothed,
    open: loom_rt::Smoothed,
    mute: bool,
    active: bool,
}

impl loom_core::Module for AudioIn {
    fn process(&mut self, ctx: &loom_core::ProcessContext<'_>, io: &mut loom_core::PortIo<'_>) {
        let pair = io.param(0);
        let gain = io.param(1);
        let mute = io.param(2);
        let (input, channels) = ctx.audio_input();
        let [left, right] = io.stereo_output(0);
        self.active = false;
        for (i, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let pair = pair.map_or(i, self.pair, |value| value.round().clamp(1.0, 4.0) as usize);
            let first = (pair - 1) * 2;
            let want = mute.map_or(i, self.mute, |value| value >= 0.5);
            let open = if want { 0.0 } else { 1.0 };
            if open != self.open.target() {
                self.open.set_target(open);
            }
            let gain = gain.at_or(i, self.gain.tick()) * self.open.tick();
            if let Some(frame) = input.get(i).filter(|_| first < channels) {
                let input_left = frame[first];
                let input_right = if channels == 1 {
                    input_left
                } else {
                    frame
                        .get(first + 1)
                        .filter(|_| first + 1 < channels)
                        .copied()
                        .unwrap_or(0.0)
                };
                *left = input_left * gain;
                *right = input_right * gain;
                self.active |= input_left != 0.0 || input_right != 0.0;
            } else {
                *left = 0.0;
                *right = 0.0;
            }
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.pair = value.round().clamp(1.0, 4.0) as usize,
            1 => self.gain.set_target(value),
            2 => {
                self.mute = value >= 0.5;
                self.open.set_target(if self.mute { 0.0 } else { 1.0 });
            }
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 1 { self.gain.gliding() } else { None }
    }

    fn reset(&mut self) {
        self.gain.reset(self.gain.target());
        self.open.reset(self.open.target());
        self.active = false;
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = f32::from(self.active);
    }

    fn copy_state_from(&mut self, other: &dyn loom_core::Module) -> bool {
        loom_core::copy_state(self, other)
    }
}
