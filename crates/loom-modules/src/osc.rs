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
    family: loom_core::Family::Sources,
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
    family: loom_core::Family::Sources,
    description: "A sawtooth wave, with every harmonic, shaped from a phase input. Its sharp edges are smoothed so high notes stay clean.",
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
    family: loom_core::Family::Sources,
    description: "A pulse wave with a set width, shaped from a phase input. Its sharp edges are smoothed so high notes stay clean.",
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
    family: loom_core::Family::Sources,
    description: "A triangle wave with few harmonics, shaped from a phase input. Its corners are smoothed so high notes stay clean.",
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
    family: loom_core::Family::Sources,
    description: "White, pink, or brown noise. The seed makes every render the same.",
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
    panel: Panel::new(9, 2, &[Widget::segmented(0, 0, 9, "colour")]),
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
mod tests;

#[cfg(test)]
mod noise_colour_tests;
