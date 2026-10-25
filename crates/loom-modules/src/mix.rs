//! Mixing (MT-MIX-01, MT-MIX-02, FR-LIB-05).
//!
//! Spec: `docs/design/p1-modules.md`, Mixing and shaping.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, Param, ParamOptions, ParamSpec, Polarity,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;
use std::f32::consts::FRAC_PI_2;

/// One audio input of the mixer.
const fn mixer_input(name: &'static str, label: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Audio,
        description: "A signal to mix. Each side of a stereo cable is mixed on its own.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }
}

/// One level knob of the mixer.
const fn mixer_level(name: &'static str, label: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description: "How loud this input is in the mix.",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)
}

/// One mute toggle of the mixer.
const fn mixer_mute(name: &'static str, label: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description: "Silences this input. The input fades out over 5 ms.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .smooth(0.005)
}

/// Number of inputs on the mixer.
const MIXER_INPUTS: usize = 4;

/// The open state (1 open, 0 muted) for a modulated mute value.
///
/// From 0.5 up is muted, as a whole-number parameter rounds. NaN counts as
/// open. *Why:* a NaN handed to the smoother would stay in its target for
/// good, and the mixer would stay NaN until it was reset.
fn open_state(mute: f32) -> f32 {
    if mute >= 0.5 { 0.0 } else { 1.0 }
}

/// `loom.mix.mixer` (MT-MIX-01): four inputs, each with a level and a mute.
pub static MIXER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.mix.mixer",
    version: 1,
    name: "Mixer",
    category: "Mix",
    description: "Adds four signals, each with its own level and mute. Two mixers in a row make a bigger mix.",
    inputs: &[
        mixer_input("in1", "In 1"),
        mixer_input("in2", "In 2"),
        mixer_input("in3", "In 3"),
        mixer_input("in4", "In 4"),
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The sum of the inputs, each times its level, muted inputs silent.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[
        mixer_level("level1", "Level 1"),
        mixer_level("level2", "Level 2"),
        mixer_level("level3", "Level 3"),
        mixer_level("level4", "Level 4"),
        mixer_mute("mute1", "Mute 1"),
        mixer_mute("mute2", "Mute 2"),
        mixer_mute("mute3", "Mute 3"),
        mixer_mute("mute4", "Mute 4"),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Mixer::new(config))),
    // Four columns, each a knob over a toggle.
    panel: Panel::new(
        12,
        5,
        &[
            Widget::knob(0, 0, "level1"),
            Widget::knob(3, 0, "level2"),
            Widget::knob(6, 0, "level3"),
            Widget::knob(9, 0, "level4"),
            Widget::toggle(0, 3, "mute1"),
            Widget::toggle(3, 3, "mute2"),
            Widget::toggle(6, 3, "mute3"),
            Widget::toggle(9, 3, "mute4"),
        ],
    ),
    displays: &[],
};

struct Mixer {
    levels: [Smoothed; MIXER_INPUTS],
    /// 1 while the input is open and 0 while it is muted, so a mute fades.
    open: [Smoothed; MIXER_INPUTS],
    /// The open state the mute toggles are set to, 1 or 0. A modulated mute
    /// replaces it for as long as the assignment is there, and removing the
    /// assignment returns to it.
    open_base: [f32; MIXER_INPUTS],
}

impl Mixer {
    fn new(config: &ModuleConfig) -> Self {
        let level = |i: usize| Smoothed::new(1.0, config.sample_rate, MIXER.params[i].options.smooth_seconds);
        let open = |i: usize| {
            Smoothed::new(
                1.0,
                config.sample_rate,
                MIXER.params[MIXER_INPUTS + i].options.smooth_seconds,
            )
        };
        Self {
            levels: std::array::from_fn(level),
            open: std::array::from_fn(open),
            open_base: [1.0; MIXER_INPUTS],
        }
    }
}

impl Module for Mixer {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated: [Param<'_>; MIXER_INPUTS] = std::array::from_fn(|n| io.param(n));
        let mute_mod: [Param<'_>; MIXER_INPUTS] = std::array::from_fn(|n| io.param(MIXER_INPUTS + n));
        let (inputs, [out]) = io.split::<MIXER_INPUTS, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for n in 0..MIXER_INPUTS {
                // The smoother glides to the wanted state, so a modulated
                // mute fades like a knob mute does.
                let want = mute_mod[n].map_or(i, self.open_base[n], open_state);
                if want != self.open[n].target() {
                    self.open[n].set_target(want);
                }
                let level = modulated[n].at_or(i, self.levels[n].tick());
                sum += inputs[n][i] * level * self.open[n].tick();
            }
            *out = sum;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0..=3 => self.levels[index].set_target(value),
            4..=7 => {
                self.open_base[index - MIXER_INPUTS] = 1.0 - value;
                self.open[index - MIXER_INPUTS].set_target(1.0 - value);
            }
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            0..=3 => self.levels[index].gliding(),
            _ => None,
        }
    }

    fn reset(&mut self) {
        for smoothed in self.levels.iter_mut().chain(self.open.iter_mut()) {
            let target = smoothed.target();
            smoothed.reset(target);
        }
    }
}

/// `loom.mix.crossfade` (MT-MIX-02): a blend between two signals.
pub static CROSSFADE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.mix.crossfade",
    version: 1,
    name: "Crossfade",
    category: "Mix",
    description: "Blends two signals. Equal power keeps the loudness of audio steady; linear is exact for control signals.",
    inputs: &[
        PortSpec {
            name: "a",
            label: "A",
            signal: SignalType::Audio,
            description: "The signal heard when the mix is 0.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "b",
            label: "B",
            signal: SignalType::Audio,
            description: "The signal heard when the mix is 1.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "A blend of A and B.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "mix",
            label: "Mix",
            description: "0 is all A and 1 is all B.",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
        ParamSpec {
            name: "law",
            label: "Law",
            description: "How the two signals share the mix: equal power, or linear.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Crossfade {
            mix: Smoothed::new(0.5, config.sample_rate, CROSSFADE.params[0].options.smooth_seconds),
            linear: false,
        })
    }),
    panel: Panel::new(
        3,
        6,
        &[
            Widget::knob(0, 0, "mix"),
            Widget::radio(0, 3, "law", &["Equal power", "Linear"]),
        ],
    ),
    displays: &[],
};

struct Crossfade {
    mix: Smoothed,
    linear: bool,
}

impl Module for Crossfade {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (modulated, law_mod) = (io.param(0), io.param(1));
        let ([a, b], [out]) = io.split::<2, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let linear = law_mod.map_or(i, self.linear, |v| v >= 0.5);
            let mix = modulated.at_or(i, self.mix.tick()).clamp(0.0, 1.0);
            // `clamp` passes NaN through. Use the middle for that frame.
            let mix = if mix.is_nan() { 0.5 } else { mix };
            let (gain_a, gain_b) = if linear {
                (1.0 - mix, mix)
            } else {
                // Audio from two sources adds in power, so the gains follow
                // a quarter circle and the sum of their squares stays 1.
                let (sin, cos) = (mix * FRAC_PI_2).sin_cos();
                (cos, sin)
            };
            *out = a[i] * gain_a + b[i] * gain_b;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.mix.set_target(value),
            _ => self.linear = value >= 0.5,
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.mix.gliding() } else { None }
    }

    fn reset(&mut self) {
        let target = self.mix.target();
        self.mix.reset(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;
    use std::f32::consts::FRAC_1_SQRT_2;

    const RATE: f32 = 48_000.0;

    /// Builds a module from a descriptor, settled at the given parameters.
    fn make(descriptor: &ModuleDescriptor, params: &[(usize, f32)]) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = descriptor.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate: RATE,
            max_block: 4_800,
        });
        for &(index, value) in params {
            m.set_param(index, value);
        }
        m.reset();
        m
    }

    /// Mixing sums inputs times their levels.
    #[test]
    fn mixer_sums_inputs_times_levels() {
        let mut mixer = make(&MIXER, &[(0, 0.5), (1, 2.0)]);
        let (a, b, c, d) = ([1.0; 8], [0.25; 8], [0.1; 8], [0.0; 8]);
        let mut out = [0.0; 8];
        run(
            mixer.as_mut(),
            RATE,
            &[&a, &b, &c, &d],
            &[true, true, true, true],
            &mut [&mut out],
            &[],
        );
        // 1.0 * 0.5 + 0.25 * 2.0 + 0.1 * 1.0 = 1.1
        assert!(out.iter().all(|v| (v - 1.1).abs() < 1e-6), "{out:?}");
    }

    /// A modulated mute from 0.5 up is muted; below it, and NaN, is open.
    #[test]
    fn modulated_mute_maps_nan_to_open() {
        assert_eq!(open_state(0.0), 1.0);
        assert_eq!(open_state(0.49), 1.0);
        assert_eq!(open_state(0.5), 0.0);
        assert_eq!(open_state(3.0), 0.0);
        assert_eq!(open_state(f32::NAN), 1.0);
        assert_eq!(open_state(f32::NEG_INFINITY), 1.0);
    }

    /// A mute fades over 5 ms instead of cutting (no click).
    #[test]
    fn mixer_mute_fades() {
        let mut mixer = make(&MIXER, &[]);
        mixer.set_param(4, 1.0);
        let one = vec![1.0; 480];
        let zero = vec![0.0; 480];
        let mut out = vec![0.0; 480];
        run(
            mixer.as_mut(),
            RATE,
            &[&one, &zero, &zero, &zero],
            &[true, false, false, false],
            &mut [&mut out],
            &[],
        );
        assert!(out[0] > 0.9, "the fade starts near full level: {}", out[0]);
        assert!(out[479] < 0.01, "and ends near silence: {}", out[479]);
        assert!(out.windows(2).all(|w| w[1] <= w[0]), "and only falls");
        let biggest_step = out.windows(2).map(|w| w[0] - w[1]).fold(0.0, f32::max);
        assert!(biggest_step < 0.05, "no click: {biggest_step}");
    }

    /// Runs a crossfade on constant inputs and returns the first output frame.
    fn crossfade_value(law: f32, mix: f32, a: f32, b: f32) -> f32 {
        let mut fade = make(&CROSSFADE, &[(0, mix), (1, law)]);
        let (a, b) = ([a; 4], [b; 4]);
        let mut out = [0.0; 4];
        run(fade.as_mut(), RATE, &[&a, &b], &[true, true], &mut [&mut out], &[]);
        out[0]
    }

    /// Equal power gives 0.7071 of each side at mix 0.5; linear gives 0.5.
    #[test]
    fn crossfade_laws_at_the_middle() {
        assert!((crossfade_value(0.0, 0.5, 1.0, 0.0) - FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((crossfade_value(0.0, 0.5, 0.0, 1.0) - FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(crossfade_value(1.0, 0.5, 1.0, 0.0), 0.5);
        assert_eq!(crossfade_value(1.0, 0.5, 0.0, 1.0), 0.5);
    }

    /// The ends of the mix are exactly one side, for both laws.
    #[test]
    fn crossfade_ends_are_one_side() {
        for law in [0.0, 1.0] {
            assert!((crossfade_value(law, 0.0, 0.8, -0.3) - 0.8).abs() < 1e-6);
            assert!((crossfade_value(law, 1.0, 0.8, -0.3) + 0.3).abs() < 1e-6);
        }
    }
}
