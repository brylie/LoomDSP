//! Bundle routing. Spec: `docs/design/polyphony.md`.

use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortRange,
    PortSpec, ProcessContext, SignalType, Widget,
};

/// `loom.voice.merge`: sums, averages, or selects a voice from a bundle.
pub static MERGE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.voice.merge",
    version: 1,
    name: "Voice Merge",
    category: "Voice",
    family: loom_core::Family::LevelAndRouting,
    description: "Sums or averages voices, or follows one voice selected by its gate.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            description: "The voices to merge.",
            signal: SignalType::Audio,
            default: PortDefault::Value(0.0),
            range: PortRange::Signal,
            channels: Channels::Voices,
        },
        PortSpec {
            name: "gate",
            label: "Gate",
            description: "Which voices are playing. A gate edge updates the selected voice.",
            signal: SignalType::Gate,
            default: PortDefault::Value(1.0),
            range: PortRange::Signal,
            channels: Channels::Voices,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        description: "The merged signal, with the input's stereo sides.",
        signal: SignalType::Audio,
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "mode",
        label: "Mode",
        description: "Sum or average all voices, or select the last, lowest, or highest playing voice.",
        min: 0.0,
        max: 4.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Sum", "Average", "Last", "Lowest", "Highest"])],
    role: ModuleRole::Dsp(|_| Box::new(Merge::default())),
    panel: Panel::new(20, 2, &[Widget::segmented(0, 0, 20, "mode")]),
    displays: &[],
    roles: &[],
};

/// Gate history and the selection held between edges.
#[derive(Clone, Default)]
struct Merge {
    high: [bool; 16],
    order: [u64; 16],
    clock: u64,
    selected: usize,
    mode: usize,
    previous_mode: Option<usize>,
}

impl Merge {
    const IN: usize = MERGE.input_index("in").unwrap();
    const GATE: usize = MERGE.input_index("gate").unwrap();
    const OUT: usize = MERGE.output_index("out").unwrap();
    const MODE: usize = MERGE.param_index("mode").unwrap();

    /// Compares stereo voices by their mean while retaining their two sides.
    fn value(io: &PortIo<'_>, voice: usize, frame: usize) -> f32 {
        let left = io.voice_input(Self::IN, voice, 0)[frame];
        if io.input_width(Self::IN).sides == 2 {
            left * 0.5 + io.voice_input(Self::IN, voice, 1)[frame] * 0.5
        } else {
            left
        }
    }
}

impl Module for Merge {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let voices = io.input_width(Self::IN).voices;
        let sides = io.output_width(Self::OUT).sides;
        let mode = io.param(Self::MODE);
        let selected_removed = self.selected >= voices;
        self.selected = self.selected.min(voices - 1);
        // Removed voices have no gate history if the bundle later grows again.
        self.high[voices..].fill(false);
        self.order[voices..].fill(0);
        for frame in 0..io.frames() {
            let mode = mode.map_or(frame, self.mode, |value| value.round().clamp(0.0, 4.0) as usize);
            let mut edge = (frame == 0 && selected_removed) || self.previous_mode != Some(mode);
            self.previous_mode = Some(mode);
            // Simultaneous rises share an age, so ties retain the lowest index.
            self.clock = self.clock.wrapping_add(1);
            for voice in 0..voices {
                let high =
                    io.voice_input(Self::GATE, voice, 0)[frame].max(io.voice_input(Self::GATE, voice, 1)[frame]) > 0.5;
                if high != self.high[voice] {
                    edge = true;
                    self.high[voice] = high;
                    if high {
                        self.order[voice] = self.clock;
                    }
                }
            }
            if edge && mode >= 2 {
                let mut picked = None;
                for voice in 0..voices {
                    if !self.high[voice] {
                        continue;
                    }
                    let better = picked.is_none_or(|previous| match mode {
                        2 => self.order[voice] > self.order[previous],
                        3 => Self::value(io, voice, frame) < Self::value(io, previous, frame),
                        _ => Self::value(io, voice, frame) > Self::value(io, previous, frame),
                    });
                    if better {
                        picked = Some(voice);
                    }
                }
                if let Some(picked) = picked {
                    self.selected = picked;
                }
            }
            for side in 0..sides {
                let value = if mode <= 1 {
                    let mut sum = 0.0;
                    for voice in 0..voices {
                        sum += io.voice_input(Self::IN, voice, side)[frame];
                    }
                    if mode == 1 { sum / voices as f32 } else { sum }
                } else {
                    io.voice_input(Self::IN, self.selected, side)[frame]
                };
                io.voice_output(Self::OUT, 0, side)[frame] = value;
            }
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::MODE {
            self.mode = value.round().clamp(0.0, 4.0) as usize;
        }
    }

    fn reset(&mut self) {
        *self = Self {
            mode: self.mode,
            ..Self::default()
        };
    }

    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }
}
