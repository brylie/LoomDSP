//! The LFO (MT-MOD-01, FR-LIB-01).
//!
//! Spec: `docs/design/p1-modules.md`, LFO.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;
use std::f64::consts::TAU;

/// `loom.mod.lfo`: a slow periodic signal, free-running or locked to the
/// transport.
pub static LFO: ModuleDescriptor = ModuleDescriptor {
    id: "loom.mod.lfo",
    version: 1,
    name: "LFO",
    category: "Modulation",
    description: "A slow wave. Free mode runs at a rate in Hz; synced mode follows a phase, by default the bar phase.",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "In synced mode, the phase to follow. Follows the bar phase while no cable is connected. Free mode ignores it.",
        default: PortDefault::BarPhase,
        polarity: Polarity::Unipolar,
        channels: Channels::Downmix,
    }],
    outputs: &[
        PortSpec {
            name: "out",
            label: "Out",
            signal: SignalType::Modulation,
            description: "The wave, from -1 to 1.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "out90",
            label: "Out +90°",
            signal: SignalType::Modulation,
            description: "The same wave a quarter cycle ahead. With Out on an XY scope, a sine draws a circle.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "The phase, from 0 to 1, that the wave is using.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::PerChannel,
        },
    ],
    params: &[
        ParamSpec {
            name: "sync",
            label: "Sync",
            description: "Free: run at the rate. Synced: follow the phase input.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "rate",
            label: "Rate",
            description: "How many waves per second in free mode.",
            min: 0.01,
            max: 50.0,
            default: 1.0,
            unit: "Hz",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .smooth(0.005),
        ParamSpec {
            name: "cycles",
            label: "Cycles",
            description: "How many waves fit in one cycle of the phase input in synced mode.",
            min: 1.0,
            max: 16.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "shape",
            label: "Shape",
            description: "The wave: sine, triangle, ramp up, ramp down, or square.",
            min: 0.0,
            max: 4.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Lfo::new(config))),
    panel: Panel::new(
        9,
        6,
        &[
            Widget::radio(0, 0, "sync", &["Free", "Synced"]),
            Widget::radio(3, 0, "shape", &["Sine", "Triangle", "Ramp up", "Ramp down", "Square"]),
            Widget::knob(6, 0, "rate"),
            Widget::knob(6, 3, "cycles"),
        ],
    ),
    displays: &[],
};

struct Lfo {
    sample_rate: f64,
    synced: bool,
    cycles: f64,
    shape: u8,
    rate: Smoothed,
    /// The free-running phase, kept in `f64` so it does not drift.
    phase: f64,
}

impl Lfo {
    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: f64::from(config.sample_rate),
            synced: false,
            cycles: 1.0,
            shape: 0,
            rate: Smoothed::new(1.0, config.sample_rate, LFO.params[1].options.smooth_seconds),
            phase: 0.0,
        }
    }

    /// The wave at phase `p` in `[0, 1)`. Every shape crosses 0 going up at
    /// phase 0, so a sine and a triangle line up.
    fn wave(shape: u8, p: f64) -> f32 {
        let fract = |x: f64| x - x.floor();
        let value = match shape {
            0 => (TAU * p).sin(),
            1 => 1.0 - 4.0 * (fract(p + 0.25) - 0.5).abs(),
            2 => 2.0 * fract(p + 0.5) - 1.0,
            3 => -(2.0 * fract(p + 0.5) - 1.0),
            _ => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        };
        value as f32
    }
}

impl Module for Lfo {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (sync_mod, rate_mod, cycles_mod, shape_mod) = (io.param(0), io.param(1), io.param(2), io.param(3));
        let ([phase_in], [out, out90, phase_out]) = io.split::<1, 3>();
        for i in 0..out.len() {
            // Modulated values count for this frame only, so removing an
            // assignment returns to the base value in the field.
            let synced = sync_mod.map_or(i, self.synced, |v| v >= 0.5);
            let cycles = cycles_mod.map_or(i, self.cycles, |v| f64::from(v.round().max(1.0)));
            let shape = shape_mod.map_or(i, self.shape, |v| v.round().max(0.0) as u8);
            let rate = f64::from(rate_mod.at_or(i, self.rate.tick()));
            let p = if synced {
                // `cycles` waves per input cycle. A whole number keeps the
                // wave continuous where the input wraps.
                let x = f64::from(phase_in[i]) * cycles;
                if x.is_finite() { x - x.floor() } else { 0.0 }
            } else {
                // The wave starts at phase 0, then the phase moves on.
                let p = self.phase;
                self.phase += rate / self.sample_rate;
                self.phase -= self.phase.floor();
                p
            };
            out[i] = Self::wave(shape, p);
            let ahead = p + 0.25;
            out90[i] = Self::wave(shape, ahead - ahead.floor());
            phase_out[i] = p as f32;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.synced = value >= 0.5,
            1 => self.rate.set_target(value),
            2 => self.cycles = f64::from(value.round().max(1.0)),
            3 => self.shape = value as u8,
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 1 { self.rate.gliding() } else { None }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        let target = self.rate.target();
        self.rate.reset(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;

    const RATE: f32 = 48_000.0;

    /// Runs the LFO with the given parameters (sync, rate, cycles, shape)
    /// over `phase_in`, in blocks of `block` frames. Returns the three outputs.
    fn lfo(params: [f32; 4], phase_in: &[f32], block: usize) -> [Vec<f32>; 3] {
        let ModuleRole::Dsp(create) = LFO.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate: RATE,
            max_block: block,
        });
        for (index, value) in params.into_iter().enumerate() {
            m.set_param(index, value);
        }
        m.reset();
        let frames = phase_in.len();
        let mut outs = [vec![0.0; frames], vec![0.0; frames], vec![0.0; frames]];
        for start in (0..frames).step_by(block) {
            let end = (start + block).min(frames);
            let [a, b, c] = &mut outs;
            run(
                m.as_mut(),
                RATE,
                &[&phase_in[start..end]],
                &[true],
                &mut [&mut a[start..end], &mut b[start..end], &mut c[start..end]],
                &[],
            );
        }
        outs
    }

    /// A free LFO starts at phase 0 and runs one wave per period, whatever
    /// is on the phase input, and the output is the same at any block size.
    #[test]
    fn free_lfo_ignores_the_phase_input_and_ignores_block_size() {
        let params = [0.0, 50.0, 1.0, 0.0];
        let ramp: Vec<f32> = (0..1_000).map(|i| i as f32 / 1_000.0).collect();
        let quiet = vec![0.0; 1_000];
        let [out, _, phase] = lfo(params, &ramp, 1_000);
        assert_eq!(phase[0], 0.0);
        assert_eq!(out[0], 0.0);
        let [other, _, _] = lfo(params, &quiet, 77);
        assert_eq!(out, other, "the phase input changes nothing");
        assert_eq!(lfo(params, &quiet, 1)[0], out, "block size 1");
    }

    /// 50 Hz is 960 frames a wave: check the rate of a free sine.
    #[test]
    fn free_rate_sets_the_wave_length() {
        let [out, _, _] = lfo([0.0, 50.0, 1.0, 0.0], &vec![0.0; 2_000], 512);
        // The sine peaks a quarter wave in and returns to 0 after a full wave.
        assert!((out[240] - 1.0).abs() < 1e-3, "{}", out[240]);
        assert!(out[960].abs() < 1e-3, "{}", out[960]);
    }

    /// Synced with `cycles` 2 gives two waves per input cycle, and out90 is
    /// the same wave a quarter ahead: on a sine, a cosine.
    #[test]
    fn synced_cycles_and_quadrature() {
        let phase_in: Vec<f32> = (0..400).map(|i| (i % 400) as f32 / 400.0).collect();
        let [out, out90, phase] = lfo([1.0, 1.0, 2.0, 0.0], &phase_in, 400);
        assert!(out[0].abs() < 1e-6 && (out90[0] - 1.0).abs() < 1e-6);
        // Two waves in 400 frames: the phase wraps back at 200.
        assert!(phase[199] > 0.98 && phase[200] == 0.0, "{} {}", phase[199], phase[200]);
        assert!(out[200].abs() < 1e-6);
    }

    /// Each shape is right at four points of the cycle.
    #[test]
    fn shapes() {
        let at = |shape: f32, p: f32| {
            let [out, _, _] = lfo([1.0, 1.0, 1.0, shape], &[p], 1);
            out[0]
        };
        // Phase 0, a quarter, a half, and three quarters.
        let table: [(f32, [f32; 4]); 5] = [
            (0.0, [0.0, 1.0, 0.0, -1.0]),
            (1.0, [0.0, 1.0, 0.0, -1.0]),
            (2.0, [0.0, 0.5, -1.0, -0.5]),
            (3.0, [0.0, -0.5, 1.0, 0.5]),
            (4.0, [1.0, 1.0, -1.0, -1.0]),
        ];
        for (shape, expected) in table {
            for (p, want) in [0.0, 0.25, 0.5, 0.75].into_iter().zip(expected) {
                assert!(
                    (at(shape, p) - want).abs() < 1e-5,
                    "shape {shape} at {p}: {}",
                    at(shape, p)
                );
            }
        }
    }
}
