//! Wave shapers and noise (FR-LIB-02).
//!
//! Shapers take a phase input instead of a frequency. Connect a Phase module
//! to drive them, or any other signal to bend the wave shape.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;
use std::f32::consts::TAU;

/// `loom.osc.sine`: sine of the phase input.
pub static SINE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.sine",
    version: 1,
    name: "Sine",
    category: "Oscillator",
    description: "Sine wave shaped from a phase input.",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "Position in the wave, from 0 to 1. One ramp is one cycle.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Sine wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Sine)),
    panel: Panel::EMPTY,
    displays: &[],
};

/// State for `loom.osc.sine`.
struct Sine;

impl Module for Sine {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase], [out]) = io.split::<1, 1>();
        for (out, &p) in out.iter_mut().zip(phase) {
            *out = (TAU * (p - p.floor())).sin();
        }
    }
}

/// `loom.osc.saw`: band-limited saw shaped from a phase input.
pub static SAW: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.saw",
    version: 1,
    name: "Saw",
    category: "Oscillator",
    description: "Band-limited saw wave shaped from a phase input (polyBLEP).",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "Position in the wave, from 0 to 1. One ramp is one cycle.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Saw wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Saw { previous: 0.0 })),
    panel: Panel::EMPTY,
    displays: &[],
};

/// State for `loom.osc.saw`.
struct Saw {
    previous: f32,
}

/// Polynomial band-limited step. Smooths the jump at the wrap point, which
/// removes most of the aliasing a naive saw produces (NFR-SND-01).
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

impl Module for Saw {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase], [out]) = io.split::<1, 1>();
        for (out, &p) in out.iter_mut().zip(phase) {
            let t = p - p.floor();
            // The phase step tells us the frequency, so the shaper does not
            // need a separate frequency input.
            let mut dt = t - self.previous;
            dt -= dt.floor();
            self.previous = t;
            let mut y = 2.0 * t - 1.0;
            if dt > 0.0 && dt < 0.5 {
                y -= poly_blep(t, dt);
            }
            *out = y;
        }
    }

    fn reset(&mut self) {
        self.previous = 0.0;
    }
}

/// `loom.osc.pulse`: band-limited pulse shaped from a phase input.
pub static PULSE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.pulse",
    version: 1,
    name: "Pulse",
    category: "Oscillator",
    description: "Band-limited pulse wave shaped from a phase input (polyBLEP).",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "Position in the wave, from 0 to 1. One ramp is one cycle.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Pulse wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "width",
        label: "Width",
        description: "Share of the cycle spent high, from 0.01 to 0.99. 0.5 is a square wave.",
        min: 0.01,
        max: 0.99,
        default: 0.5,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)],
    role: ModuleRole::Dsp(|config| {
        Box::new(Pulse {
            previous: 0.0,
            previous_fall: 0.5,
            width: Smoothed::new(0.5, config.sample_rate, PULSE.params[0].options.smooth_seconds),
        })
    }),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "width")]),
    displays: &[],
};

/// State for `loom.osc.pulse`.
struct Pulse {
    previous: f32,
    previous_fall: f32,
    width: Smoothed,
}

impl Module for Pulse {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.modulated(0);
        let ([phase], [out]) = io.split::<1, 1>();
        for (i, (out, &p)) in out.iter_mut().zip(phase).enumerate() {
            let width = if let Some(samples) = modulated {
                let _ = self.width.tick();
                samples[i]
            } else {
                self.width.tick()
            }
            .clamp(0.01, 0.99);
            let t = p - p.floor();
            let mut dt = t - self.previous;
            dt -= dt.floor();
            self.previous = t;

            let mut t_fall = t - width;
            t_fall -= t_fall.floor();
            let mut dt_fall = t_fall - self.previous_fall;
            dt_fall -= dt_fall.floor();
            self.previous_fall = t_fall;

            let mut y = if t < width { 1.0 } else { -1.0 };
            if dt > 0.0 && dt < 0.5 {
                // Rising jump (+2) at the start of the cycle.
                y += poly_blep(t, dt);
            }
            // Falling jump (-2) when `t` crosses `width`, or rising jump (+2)
            // when `width` sweeps past a slower or stationary `t`.
            let step_fall = dt_fall.min(1.0 - dt_fall);
            if step_fall > 0.0 && step_fall < 0.5 {
                y -= poly_blep(t_fall, step_fall);
            }
            *out = y;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.width.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 { self.width.gliding() } else { None }
    }

    fn reset(&mut self) {
        self.previous = 0.0;
        let target = self.width.target();
        self.width.reset(target);
        self.previous_fall = 1.0 - target.clamp(0.01, 0.99);
    }
}

/// `loom.osc.triangle`: band-limited triangle shaped from a phase input.
pub static TRIANGLE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.triangle",
    version: 1,
    name: "Triangle",
    category: "Oscillator",
    description: "Band-limited triangle wave shaped from a phase input (polyBLAMP).",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "Position in the wave, from 0 to 1. One ramp is one cycle.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Triangle wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Triangle { previous: 0.0 })),
    panel: Panel::EMPTY,
    displays: &[],
};

/// State for `loom.osc.triangle`.
struct Triangle {
    previous: f32,
}

/// Polynomial band-limited ramp: the integral of a two-sample polynomial step.
/// Smooths a unit slope change at `t = 0`, rounding the sharp corners of a
/// triangle wave without a leaky integrator (NFR-SND-01).
#[inline]
fn poly_blamp(t: f32, dt: f32) -> f32 {
    if t < dt {
        let u = 1.0 - t / dt;
        (1.0 / 6.0) * u * u * u
    } else if t > 1.0 - dt {
        let u = (t - 1.0) / dt + 1.0;
        (1.0 / 6.0) * u * u * u
    } else {
        0.0
    }
}

impl Module for Triangle {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase], [out]) = io.split::<1, 1>();
        for (out, &p) in out.iter_mut().zip(phase) {
            let t = p - p.floor();
            let mut dt = t - self.previous;
            dt -= dt.floor();
            self.previous = t;
            // Sine-aligned: 0 at t = 0, +1 at t = 0.25, 0 at t = 0.5, -1 at t = 0.75.
            let mut t_top = t - 0.25;
            t_top -= t_top.floor();
            let mut y = 4.0 * (t_top - 0.5).abs() - 1.0;
            if dt > 0.0 && dt < 0.5 {
                let mut t_bottom = t - 0.75;
                t_bottom -= t_bottom.floor();
                // Slope changes by -8*dt at the top peak and +8*dt at the bottom peak.
                y += 8.0 * dt * (poly_blamp(t_bottom, dt) - poly_blamp(t_top, dt));
            }
            *out = y;
        }
    }

    fn reset(&mut self) {
        self.previous = 0.0;
    }
}

/// `loom.osc.noise`: white noise from a seeded generator.
pub static NOISE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.osc.noise",
    version: 1,
    name: "Noise",
    category: "Oscillator",
    description: "White noise. The seed makes renders repeatable (NFR-SND-04).",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "White noise from -1 to 1.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "seed",
        label: "Seed",
        description: "Picks the noise pattern. The same seed gives the same noise.",
        min: 1.0,
        max: 65_535.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()],
    role: ModuleRole::Dsp(|config: &ModuleConfig| {
        let _ = config;
        Box::new(Noise {
            state: 1,
            seed: 1,
            active: 1,
        })
    }),
    panel: Panel::EMPTY,
    displays: &[],
};

/// State for `loom.osc.noise`.
struct Noise {
    state: u32,
    /// The knob's base seed. Only `set_param` writes it.
    seed: u32,
    /// The seed `state` was last started from: the modulated seed while an
    /// assignment is active, otherwise the base.
    active: u32,
}

impl Module for Noise {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let seed = io.modulated(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            // Restart the pattern when the seed in use changes, including
            // back to the base when an assignment is removed.
            let next = seed.map_or(self.seed, |samples| samples[i].round().max(1.0) as u32);
            if next != self.active {
                self.active = next;
                self.state = next;
            }
            // xorshift32: fast, deterministic, good enough for audio noise.
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.state = x;
            *out = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.seed = value.round().max(1.0) as u32;
            self.active = self.seed;
            self.state = self.seed;
        }
    }

    fn reset(&mut self) {
        self.state = self.active;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;

    /// Checks that the saw wave stays within `[-1, 1]` and rises across the cycle.
    #[test]
    fn saw_is_bounded_and_rises() {
        let config = ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 64,
        };
        let ModuleRole::Dsp(create) = SAW.role else {
            unreachable!()
        };
        let mut saw = create(&config);
        let phase: Vec<f32> = (0..64).map(|i| (i as f32 * 0.01) % 1.0).collect();
        let mut out = vec![0.0; 64];
        run(saw.as_mut(), 48_000.0, &[&phase], &[true], &mut [&mut out], &[]);
        assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6));
        assert!(out[10] > out[5]);
    }

    /// Checks that pulse wave output stays within `[-1, 1]` and matches the expected duty cycle mean.
    #[test]
    fn pulse_duty_cycle_and_bounds() {
        let config = ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 200,
        };
        let ModuleRole::Dsp(create) = PULSE.role else {
            unreachable!()
        };
        let phase: Vec<f32> = (0..200).map(|i| (i % 100) as f32 / 100.0).collect();
        for &(width, expected_mean) in &[(0.5, 0.0), (0.25, -0.5), (0.75, 0.5)] {
            let mut pulse = create(&config);
            pulse.set_param(0, width);
            pulse.reset();
            let mut out = vec![0.0; 200];
            run(pulse.as_mut(), 48_000.0, &[&phase], &[true], &mut [&mut out], &[]);
            assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6), "width {width}: {out:?}");
            // Steady second cycle: average equals 2 * width - 1.
            let mean: f32 = out[100..].iter().sum::<f32>() / 100.0;
            assert!((mean - expected_mean).abs() < 0.02, "width {width}: mean {mean}");
        }
    }

    /// Checks that sweeping `width` across a stationary phase still smooths the transition with polyBLEP.
    #[test]
    fn pulse_width_sweep_at_stationary_phase_is_smoothed() {
        let config = ModuleConfig {
            sample_rate: 4_000.0,
            max_block: 64,
        };
        let ModuleRole::Dsp(create) = PULSE.role else {
            unreachable!()
        };
        let phase = vec![0.5; 64];
        for &(start, end) in &[(0.75, 0.25), (0.25, 0.75)] {
            let mut pulse = create(&config);
            pulse.set_param(0, start);
            pulse.reset();
            pulse.set_param(0, end);
            let mut out = vec![0.0; 64];
            run(pulse.as_mut(), 4_000.0, &[&phase], &[true], &mut [&mut out], &[]);
            assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6), "{start}->{end}: {out:?}");
            assert!(
                out.iter().any(|v| v.abs() < 0.99),
                "expected polyBLEP-smoothed intermediate samples on {start}->{end} sweep, got {out:?}"
            );
        }
    }

    /// Checks that the triangle wave stays within `[-1, 1]` and matches sine phase alignment.
    #[test]
    fn triangle_is_bounded_and_sine_aligned() {
        let config = ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 200,
        };
        let ModuleRole::Dsp(create) = TRIANGLE.role else {
            unreachable!()
        };
        let mut tri = create(&config);
        let phase: Vec<f32> = (0..200).map(|i| (i % 100) as f32 / 100.0).collect();
        let mut out = vec![0.0; 200];
        run(tri.as_mut(), 48_000.0, &[&phase], &[true], &mut [&mut out], &[]);
        assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6), "{out:?}");
        // Second cycle: 0 at start, rises to +1 at quarter cycle, 0 at half, -1 at three-quarters.
        assert!(out[100].abs() < 0.05, "start of cycle: {}", out[100]);
        assert!(out[125] > 0.95, "quarter cycle peak: {}", out[125]);
        assert!(out[150].abs() < 0.05, "half cycle zero: {}", out[150]);
        assert!(out[175] < -0.95, "three-quarter trough: {}", out[175]);
    }

    /// NFR-SND-01: measures aliasing at a high note (~2859 Hz, near F7 at 48 kHz).
    ///
    /// Using N = 1024 samples and prime cycle count k0 = 61 makes the phase
    /// strictly periodic over N samples with zero window leakage, and places all
    /// true harmonics below Nyquist on bins 61 * m (m = 1..=8). Every other bin
    /// holds only aliased energy.
    #[test]
    fn aliasing_at_high_note_is_suppressed() {
        const N: usize = 1024;
        const WARMUP: usize = 16;
        const K0: usize = 61;

        let phase: Vec<f32> = (0..WARMUP + N).map(|i| ((i * K0) % N) as f32 / N as f32).collect();
        let config = ModuleConfig {
            sample_rate: 48_000.0,
            max_block: WARMUP + N,
        };

        // Returns (worst sub-fundamental alias in dB below the fundamental,
        // total alias RMS in dB below the fundamental).
        let measure_aliasing = |samples: &[f32]| -> (f32, f32) {
            let bin_mag_sq = |bin: usize| -> f32 {
                let mut re = 0.0f64;
                let mut im = 0.0f64;
                for (n, &x) in samples.iter().enumerate() {
                    let angle = std::f64::consts::TAU * (bin * n) as f64 / N as f64;
                    re += f64::from(x) * angle.cos();
                    im -= f64::from(x) * angle.sin();
                }
                ((re * re + im * im) / (N * N) as f64) as f32
            };
            let fund_sq = bin_mag_sq(K0);
            assert!(fund_sq > 0.05, "fundamental must be present, got {fund_sq}");
            let mut max_sub_sq = 0.0f32;
            let mut total_alias_sq = 0.0f32;
            for bin in 1..N / 2 {
                if bin.is_multiple_of(K0) {
                    continue;
                }
                let mag_sq = bin_mag_sq(bin);
                if bin < K0 {
                    max_sub_sq = max_sub_sq.max(mag_sq);
                }
                total_alias_sq += mag_sq;
            }
            let sub_db = 10.0 * (max_sub_sq / fund_sq).log10();
            let total_db = 10.0 * (total_alias_sq / fund_sq).log10();
            (sub_db, total_db)
        };

        let render_module = |descriptor: &ModuleDescriptor| -> Vec<f32> {
            let ModuleRole::Dsp(create) = descriptor.role else {
                unreachable!()
            };
            let mut module = create(&config);
            let mut out = vec![0.0; WARMUP + N];
            run(module.as_mut(), 48_000.0, &[&phase], &[true], &mut [&mut out], &[]);
            out[WARMUP..].to_vec()
        };

        // Naive shapes at the same high note for comparison.
        let naive_saw: Vec<f32> = phase[WARMUP..].iter().map(|&t| 2.0 * t - 1.0).collect();
        let naive_pulse: Vec<f32> = phase[WARMUP..]
            .iter()
            .map(|&t| if t < 0.5 { 1.0 } else { -1.0 })
            .collect();
        let naive_tri: Vec<f32> = phase[WARMUP..]
            .iter()
            .map(|&t| {
                let mut t_top = t - 0.25;
                t_top -= t_top.floor();
                4.0 * (t_top - 0.5).abs() - 1.0
            })
            .collect();

        let (saw_sub_db, saw_total_db) = measure_aliasing(&render_module(&SAW));
        let (naive_saw_sub_db, naive_saw_total_db) = measure_aliasing(&naive_saw);
        assert!(
            saw_sub_db < -40.0 && saw_total_db < naive_saw_total_db - 15.0,
            "saw aliasing: sub {saw_sub_db:.1} dB (naive {naive_saw_sub_db:.1}), total {saw_total_db:.1} dB (naive {naive_saw_total_db:.1})"
        );

        let (pulse_sub_db, pulse_total_db) = measure_aliasing(&render_module(&PULSE));
        let (naive_pulse_sub_db, naive_pulse_total_db) = measure_aliasing(&naive_pulse);
        assert!(
            pulse_sub_db < -40.0 && pulse_total_db < naive_pulse_total_db - 15.0,
            "pulse aliasing: sub {pulse_sub_db:.1} dB (naive {naive_pulse_sub_db:.1}), total {pulse_total_db:.1} dB (naive {naive_pulse_total_db:.1})"
        );

        let (tri_sub_db, tri_total_db) = measure_aliasing(&render_module(&TRIANGLE));
        let (naive_tri_sub_db, naive_tri_total_db) = measure_aliasing(&naive_tri);
        assert!(
            tri_sub_db < -65.0 && tri_total_db < -40.0 && tri_total_db < naive_tri_total_db - 10.0,
            "triangle aliasing: sub {tri_sub_db:.1} dB (naive {naive_tri_sub_db:.1}), total {tri_total_db:.1} dB (naive {naive_tri_total_db:.1})"
        );
    }
}
