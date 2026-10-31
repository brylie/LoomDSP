//! Wave shapers and noise (FR-LIB-02).
//!
//! Shapers take a phase input instead of a frequency. Connect a Phase module
//! to drive them, or any other signal to bend the wave shape.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
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
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Sine wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
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
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Saw wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Saw { previous: 0.0 })),
    panel: Panel::EMPTY,
    displays: &[],
};

/// State for `loom.osc.saw`.
#[derive(Clone)]
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
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

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
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Pulse wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
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
#[derive(Clone)]
struct Pulse {
    previous: f32,
    previous_fall: f32,
    width: Smoothed,
}

impl Module for Pulse {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let modulated = io.param(0);
        let ([phase], [out]) = io.split::<1, 1>();
        for (i, (out, &p)) in out.iter_mut().zip(phase).enumerate() {
            let width = modulated.at_or(i, self.width.tick()).clamp(0.01, 0.99);
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
            let step = dt.min(1.0 - dt);
            let step_fall = dt_fall.min(1.0 - dt_fall);
            // When the pulse or gap is narrower than `step + step_fall`, the
            // two BLEP windows overlap; use a shared step so their slopes cancel.
            let (step, step_fall) = if width < step + step_fall || 1.0 - width < step + step_fall {
                let shared = step.max(step_fall);
                (shared, shared)
            } else {
                (step, step_fall)
            };
            if step > 0.0 && step < 0.5 {
                // Jump at the start of the cycle (+2 ascending, -2 descending).
                y += poly_blep(t, step);
            }
            // Jump when `t` crosses `width` (-2 ascending, +2 descending).
            if step_fall > 0.0 && step_fall < 0.5 {
                y -= poly_blep(t_fall, step_fall);
            }
            *out = y.clamp(-1.0, 1.0);
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
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Triangle wave from -1 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Triangle { previous: 0.0 })),
    panel: Panel::EMPTY,
    displays: &[],
};

/// State for `loom.osc.triangle`.
#[derive(Clone)]
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
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

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
            let step = dt.min(1.0 - dt);
            if step > 0.0 && step < 0.5 {
                let mut t_bottom = t - 0.75;
                t_bottom -= t_bottom.floor();
                // Slope changes by -8*step at the top peak and +8*step at the bottom peak.
                y += 8.0 * step * (poly_blamp(t_bottom, step) - poly_blamp(t_top, step));
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
    description: "White, pink, or brown noise. The seed makes renders repeatable (NFR-SND-04).",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Noise with the same loudness in every colour. White stays within -1 to 1; pink and brown can peak a little above.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
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
    .integer(),
    ParamSpec {
        name: "colour",
        label: "Colour",
        description: "White falls flat across the spectrum, pink falls 3 dB per octave, and brown falls 6 dB per octave.",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
.integer()
    .choices(&["White", "Pink", "Brown"])],
    role: ModuleRole::Dsp(|config: &ModuleConfig| Box::new(Noise::new(config.sample_rate))),
    panel: Panel::new(6, 4, &[Widget::radio(0, 0, "colour")]),
    displays: &[],
};

/// Rows in the pink noise generator. Row `k` changes every `2^k` samples, so
/// 16 rows reach down to well under 1 Hz at audio rates.
const PINK_ROWS: usize = 16;

/// The corner of the brown noise leak, in Hz. Far below the audible slope, so
/// the -6 dB per octave fall holds above about 100 Hz.
const BROWN_LEAK_HZ: f32 = 10.0;

/// State for `loom.osc.noise`.
///
/// White noise comes from one xorshift stream, `state`. Pink and brown noise
/// are built from the same white samples, so every colour at one seed shares
/// its white stream. The pink rows draw from a separate stream, `aux`, so
/// they never move the white one.
///
/// Pink noise is the Voss-McCartney method (R. F. Voss and J. Clarke, "1/f
/// noise in music and speech", Nature 258, 1975; the row-update form is
/// from Gardner, "White and brown music, fractal curves and 1/f
/// fluctuations", Scientific American, 1978). Brown noise is a leaky
/// integral of white noise, so it cannot drift away.
struct Noise {
    state: u32,
    /// Channel whose seed is used for the current random streams.
    channel: usize,
    /// The knob's base seed. Only `set_param` writes it.
    seed: u32,
    /// The seed `state` was last started from: the modulated seed while an
    /// assignment is active, otherwise the base.
    active: u32,
    colour: u8,
    /// Stream for the pink rows.
    aux: u32,
    rows: [f32; PINK_ROWS],
    row_sum: f32,
    counter: u32,
    brown: f32,
    /// Brown leak coefficient, just under 1.
    leak: f32,
    /// Brown input gain that keeps the RMS equal to white noise.
    brown_gain: f32,
}

impl Noise {
    fn new(sample_rate: f32) -> Self {
        let leak = (-std::f32::consts::TAU * BROWN_LEAK_HZ / sample_rate).exp();
        let mut noise = Self {
            state: 1,
            channel: 0,
            seed: 1,
            active: 1,
            colour: 0,
            aux: 1,
            rows: [0.0; PINK_ROWS],
            row_sum: 0.0,
            counter: 0,
            brown: 0.0,
            leak,
            // The leaky integral of unit-variance noise has variance
            // 1 / (1 - leak^2), so this gain restores a variance of 1.
            brown_gain: (1.0 - leak * leak).sqrt(),
        };
        noise.restart(1);
        noise
    }

    /// Starts every stream again from `seed`.
    fn restart(&mut self, seed: u32) {
        let seed = if self.channel == 0 { seed } else { seed ^ 0x85EB_CA6B };
        self.state = seed.max(1);
        self.aux = (seed ^ 0x9E37_79B9).max(1);
        for row in 0..PINK_ROWS {
            self.rows[row] = Self::next_white(&mut self.aux);
        }
        self.row_sum = self.rows.iter().sum();
        self.counter = 0;
        self.brown = 0.0;
    }

    /// One xorshift32 step, as a value from -1 to 1.
    ///
    /// xorshift32 is fast and deterministic, and good enough for audio noise.
    fn next_white(state: &mut u32) -> f32 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// The next pink sample, given the next white one.
    fn pink(&mut self, white: f32) -> f32 {
        // Row k changes when bit k is the lowest set bit of the counter, so
        // it changes every 2^k samples.
        self.counter = self.counter.wrapping_add(1);
        let row = self.counter.trailing_zeros() as usize;
        if row < PINK_ROWS {
            let value = Self::next_white(&mut self.aux);
            self.row_sum += value - self.rows[row];
            self.rows[row] = value;
        }
        // The running sum drifts by rounding. Summing again every 65,536
        // samples keeps it exact enough, at no cost per sample.
        if self.counter & 0xFFFF == 0 {
            self.row_sum = self.rows.iter().sum();
        }
        // One white sample on top fills in the top of the spectrum. The sum
        // of PINK_ROWS + 1 independent values has that many times the
        // variance, so dividing by the square root restores white's RMS.
        (self.row_sum + white) / ((PINK_ROWS + 1) as f32).sqrt()
    }
}

impl Module for Noise {
    /// Takes the settings and the brown filter memory of another instance,
    /// then starts the random streams again from the seed, as `reset` does
    /// (stereo spec, rule 27). A copied generator would play the other
    /// side's numbers. The first call on the right mixes in its channel.
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Self>() else {
            return false;
        };
        self.seed = other.seed;
        self.channel = other.channel;
        self.active = other.active;
        self.colour = other.colour;
        self.leak = other.leak;
        self.brown_gain = other.brown_gain;
        self.restart(self.active);
        self.brown = other.brown;
        true
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        if self.channel != ctx.channel() {
            self.channel = ctx.channel();
            let brown = self.brown;
            self.restart(self.active);
            self.brown = brown;
        }
        let (seed, colour_mod) = (io.param(0), io.param(1));
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            // The colour counts for this frame only; the base stays in the
            // field, so removing an assignment returns to it.
            let colour = colour_mod.map_or(i, self.colour, |v| v.round().max(0.0) as u8);
            // Restart the pattern when the seed in use changes, including
            // back to the base when an assignment is removed.
            let next = seed.map_or(i, self.seed, |v| v.round().max(1.0) as u32);
            if next != self.active {
                self.active = next;
                self.restart(next);
            }
            let white = Self::next_white(&mut self.state);
            *out = match colour {
                0 => white,
                1 => self.pink(white),
                _ => {
                    // Uniform noise has variance 1/3, so scaling to the
                    // same RMS as white is the same gain.
                    self.brown = self.leak * self.brown + self.brown_gain * white;
                    self.brown
                }
            };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => {
                self.seed = value.round().max(1.0) as u32;
                self.active = self.seed;
                self.restart(self.seed);
            }
            1 => self.colour = value as u8,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.restart(self.active);
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

    /// Regression test: overlapping BLEP windows with unequal edge velocities under audio-rate
    /// width modulation must stay within `[-1, 1]`.
    #[test]
    fn pulse_overlapping_edges_with_unequal_velocities_stay_bounded() {
        // At 1 Hz, the 5 ms smoother reaches its target in a single sample while preserving
        // `previous` and `previous_fall`, reproducing audio-rate width jumps.
        let config = ModuleConfig {
            sample_rate: 1.0,
            max_block: 64,
        };
        let ModuleRole::Dsp(create) = PULSE.role else {
            unreachable!()
        };
        let mut pulse = create(&config);
        // Step 1 sets `previous = 0.75` and `width = 0.80` (`previous_fall = 0.95`).
        // Step 2 advances phase to `0.95` (`step = 0.20`) and width to `0.99` (`step_fall = 0.01`).
        let mut out = [0.0; 1];
        pulse.set_param(0, 0.80);
        pulse.reset();
        run(pulse.as_mut(), 1.0, &[&[0.75]], &[true], &mut [&mut out], &[]);
        assert!(out[0].abs() <= 1.0 + 1e-6, "step 1: {}", out[0]);

        pulse.set_param(0, 0.99);
        run(pulse.as_mut(), 1.0, &[&[0.95]], &[true], &mut [&mut out], &[]);
        assert!(out[0].abs() <= 1.0 + 1e-6, "step 2: {}", out[0]);
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

        let render_with_phase = |descriptor: &ModuleDescriptor, phase_in: &[f32]| -> Vec<f32> {
            let ModuleRole::Dsp(create) = descriptor.role else {
                unreachable!()
            };
            let mut module = create(&config);
            let mut out = vec![0.0; WARMUP + N];
            run(module.as_mut(), 48_000.0, &[phase_in], &[true], &mut [&mut out], &[]);
            out[WARMUP..].to_vec()
        };
        let render_module = |descriptor: &ModuleDescriptor| render_with_phase(descriptor, &phase);

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

        // Descending phase ramp at the same frequency must also suppress aliasing.
        let desc_phase: Vec<f32> = phase.iter().map(|&t| (1.0 - t) % 1.0).collect();
        let (desc_pulse_sub_db, desc_pulse_total_db) = measure_aliasing(&render_with_phase(&PULSE, &desc_phase));
        assert!(
            desc_pulse_sub_db < -40.0 && desc_pulse_total_db < naive_pulse_total_db - 15.0,
            "descending pulse aliasing: sub {desc_pulse_sub_db:.1} dB, total {desc_pulse_total_db:.1} dB"
        );
        let (desc_tri_sub_db, desc_tri_total_db) = measure_aliasing(&render_with_phase(&TRIANGLE, &desc_phase));
        assert!(
            desc_tri_sub_db < -65.0 && desc_tri_total_db < -40.0 && desc_tri_total_db < naive_tri_total_db - 10.0,
            "descending triangle aliasing: sub {desc_tri_sub_db:.1} dB, total {desc_tri_total_db:.1} dB"
        );
    }
}

#[cfg(test)]
mod noise_colour_tests {
    use super::*;
    use loom_core::testing::run;

    const RATE: f32 = 48_000.0;

    /// Renders `frames` samples of noise in the given colour (0 white, 1
    /// pink, 2 brown) at the given seed, in blocks of `block` frames.
    fn render(colour: f32, seed: f32, frames: usize, block: usize) -> Vec<f32> {
        let ModuleRole::Dsp(create) = NOISE.role else {
            unreachable!()
        };
        let mut noise = create(&ModuleConfig {
            sample_rate: RATE,
            max_block: block,
        });
        noise.set_param(0, seed);
        noise.set_param(1, colour);
        noise.reset();
        let mut out = vec![0.0; frames];
        for chunk in out.chunks_mut(block) {
            run(noise.as_mut(), RATE, &[], &[], &mut [chunk], &[]);
        }
        out
    }

    /// A copied noise stream mixes in the right channel and stays deterministic.
    #[test]
    fn copied_noise_uses_the_channel_in_its_seed() {
        fn run_copy(channel: usize, block: usize) -> Vec<f32> {
            let mut left = Noise::new(RATE);
            let mut output = [0.0; 64];
            run(&mut left, RATE, &[], &[], &mut [&mut output], &[]);
            let mut copy = Noise::new(RATE);
            assert!(copy.copy_state_from(&left));
            let mut result = vec![0.0; 512];
            for chunk in result.chunks_mut(block) {
                loom_core::testing::run_with_transport_and_channel(
                    &mut copy,
                    RATE,
                    loom_core::TransportInfo::default(),
                    &[],
                    &[],
                    &mut [chunk],
                    &[],
                    channel,
                );
            }
            result
        }
        let left = run_copy(0, 77);
        let right = run_copy(1, 77);
        assert_ne!(left, right);
        assert_eq!(right, run_copy(1, 1));
        assert_eq!(right, run_copy(1, 512));
    }

    /// The two channels do not reuse either random stream at a shifted offset.
    #[test]
    fn stereo_noise_streams_do_not_overlap() {
        let mut left = Noise::new(RATE);
        let mut right = Noise::new(RATE);
        left.restart(42);
        right.channel = 1;
        right.restart(42);
        let streams = [left.state, left.aux, right.state, right.aux].map(|mut state| {
            (0..128)
                .map(|_| {
                    Noise::next_white(&mut state);
                    state
                })
                .collect::<std::collections::HashSet<_>>()
        });
        for i in 0..streams.len() {
            for j in i + 1..streams.len() {
                assert!(streams[i].is_disjoint(&streams[j]), "streams {i} and {j} overlap");
            }
        }
    }

    fn rms(samples: &[f32]) -> f64 {
        (samples.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    /// An in-place radix-2 FFT. `re.len()` must be a power of two.
    fn fft(re: &mut [f64], im: &mut [f64]) {
        let n = re.len();
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j |= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let angle = -std::f64::consts::TAU / len as f64;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (sin, cos) = (angle * k as f64).sin_cos();
                    let (a, b) = (start + k, start + k + len / 2);
                    let (tr, ti) = (re[b] * cos - im[b] * sin, re[b] * sin + im[b] * cos);
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }

    /// The slope of the power spectral density between 100 Hz and 10 kHz, in
    /// dB per octave. It averages the power per bin over one octave around
    /// each frequency, over many Hann-windowed segments.
    fn slope_db_per_octave(samples: &[f32]) -> f64 {
        const SEGMENT: usize = 1 << 16;
        let mut power = vec![0.0; SEGMENT / 2];
        for segment in samples.as_chunks::<SEGMENT>().0 {
            let mut re: Vec<f64> = segment
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / SEGMENT as f64).cos();
                    f64::from(v) * hann
                })
                .collect();
            let mut im = vec![0.0; SEGMENT];
            fft(&mut re, &mut im);
            for (bin, p) in power.iter_mut().enumerate() {
                *p += re[bin] * re[bin] + im[bin] * im[bin];
            }
        }
        let bin_hz = f64::from(RATE) / SEGMENT as f64;
        let mean_around = |centre: f64| {
            let (low, high) = (
                (centre / 2f64.sqrt() / bin_hz) as usize,
                (centre * 2f64.sqrt() / bin_hz) as usize,
            );
            power[low..high].iter().sum::<f64>() / (high - low) as f64
        };
        let octaves = (10_000.0f64 / 100.0).log2();
        10.0 * (mean_around(10_000.0) / mean_around(100.0)).log10() / octaves
    }

    /// Pink falls 3 dB per octave and brown 6, each within 1 dB; white is flat.
    #[test]
    fn slopes_follow_the_colour() {
        let frames = 16 << 16;
        for (colour, expected) in [(0.0, 0.0), (1.0, -3.0), (2.0, -6.0)] {
            let slope = slope_db_per_octave(&render(colour, 7.0, frames, 4096));
            assert!((slope - expected).abs() < 1.0, "colour {colour}: {slope} dB per octave");
        }
    }

    /// Every colour has about the same loudness as white (within 3 dB), and
    /// brown stays near zero over a minute.
    #[test]
    fn colours_match_white_in_level_and_brown_does_not_drift() {
        let frames = 60 * RATE as usize;
        let white = rms(&render(0.0, 3.0, frames, 4096));
        for colour in [1.0, 2.0] {
            let samples = render(colour, 3.0, frames, 4096);
            let db = 20.0 * (rms(&samples) / white).log10();
            assert!(db.abs() < 3.0, "colour {colour}: {db} dB from white");
            let mean = samples.iter().map(|&v| f64::from(v)).sum::<f64>() / frames as f64;
            assert!(mean.abs() < 0.05, "colour {colour}: mean {mean}");
        }
    }

    /// The same seed gives the same noise at block sizes 1, 77, and 512, and
    /// switching to white gives the same samples as before pink existed.
    #[test]
    fn same_seed_same_output_at_any_block_size() {
        for colour in [0.0, 1.0, 2.0] {
            let reference = render(colour, 5.0, 2_000, 512);
            assert_eq!(render(colour, 5.0, 2_000, 1), reference, "colour {colour}, block 1");
            assert_eq!(render(colour, 5.0, 2_000, 77), reference, "colour {colour}, block 77");
        }
    }
}
