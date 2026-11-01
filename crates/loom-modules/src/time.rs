//! Delay line and all-pass filter (`FR-LIB-06`, `MT-DELAY-02`, `MT-DELAY-03`).
//!
//! Spec: `docs/design/delay.md`. Theory: `docs/learn/delays-and-interpolation.md`.
//!
//! Both modules allocate their circular buffer on the control thread when the
//! module is created (`NFR-RT-02`), never in `process`, and share a 4-point
//! Catmull-Rom (cubic Hermite) fractional-delay reader.

use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};
use loom_rt::Smoothed;

/// `loom.time.delay`: interpolated delay line (`MT-DELAY-02`).
pub static DELAY: ModuleDescriptor = ModuleDescriptor {
    id: "loom.time.delay",
    version: 1,
    name: "Delay Line",
    category: "Time",
    family: loom_core::Family::FiltersAndEffects,
    description: "Plays its input back after a set time, for echoes, chorus, and feedback. The time input adds seconds to the time knob.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The signal to delay.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "time",
            label: "Time",
            signal: SignalType::Modulation,
            description: "Adds to the delay time, in seconds.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The delayed signal.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "time",
        label: "Time",
        description: "How long the signal is delayed.",
        min: 0.0001,
        max: 2.0,
        default: 0.2,
        unit: "s",
        options: ParamOptions::DEFAULT,
    }
    .logarithmic()
    .smooth(0.01)],
    role: ModuleRole::Dsp(|config| Box::new(Delay::new(config))),
    panel: Panel::new(3, 3, &[Widget::knob(0, 0, "time")]),
    displays: &[],
};

/// `loom.time.allpass`: Schroeder delay-line all-pass filter (`MT-DELAY-03`).
pub static ALLPASS: ModuleDescriptor = ModuleDescriptor {
    id: "loom.time.allpass",
    version: 1,
    name: "All-Pass",
    category: "Time",
    family: loom_core::Family::FiltersAndEffects,
    description: "A delay-based filter that passes every frequency at the same level but smears their timing. Chains of them build reverbs and diffusers.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "The signal to pass through the filter.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "time",
            label: "Time",
            signal: SignalType::Modulation,
            description: "Adds to the delay time, in seconds.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The all-pass filtered signal.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "time",
            label: "Time",
            description: "Delay time of the all-pass loop.",
            min: 0.0001,
            max: 0.2,
            default: 0.01,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .smooth(0.01),
        ParamSpec {
            name: "gain",
            label: "Gain",
            description: "Feedback and feed-forward coefficient. 0 is a plain delay.",
            min: -0.95,
            max: 0.95,
            default: 0.5,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .smooth(0.005),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Allpass::new(config))),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "time"), Widget::knob(3, 0, "gain")]),
    displays: &[],
};

/// Circular buffer with a 4-point Catmull-Rom fractional-delay reader.
#[derive(Clone)]
struct DelayBuffer {
    samples: Box<[f32]>,
    write: usize,
    max_samples: usize,
}

impl DelayBuffer {
    /// Copies `other` into this buffer, with no allocation. False when the
    /// two have different lengths, which only happens when they were built
    /// with a different `ModuleConfig`.
    fn copy_from(&mut self, other: &Self) -> bool {
        if self.samples.len() != other.samples.len() {
            return false;
        }
        self.samples.copy_from_slice(&other.samples);
        self.write = other.write;
        self.max_samples = other.max_samples;
        true
    }

    /// Allocates a zeroed buffer for `max_seconds` at `sample_rate`, plus four
    /// guard samples for the 4-point cubic stencil.
    fn new(max_seconds: f32, sample_rate: f32) -> Self {
        let max_samples = (f64::from(max_seconds) * f64::from(sample_rate)).ceil().max(2.0) as usize;
        // Four extra frames so `k + 2` at `k == max_samples` stays in bounds
        // without overlapping the current write slot.
        Self {
            samples: vec![0.0; max_samples + 4].into_boxed_slice(),
            write: 0,
            max_samples,
        }
    }

    /// Clamps `raw_samples` to `[min_samples, max_samples]`, mapping `NaN` to
    /// `min_samples`.
    #[inline]
    fn clamp_delay(&self, raw_samples: f64, min_samples: f64) -> f64 {
        if raw_samples.is_nan() {
            min_samples
        } else {
            raw_samples.clamp(min_samples, self.max_samples as f64)
        }
    }

    /// Sample `age` frames behind `self.write`. `age == 0` is the current
    /// write slot; `age == 1` is the sample written on the previous frame.
    #[inline]
    fn at(&self, age: usize) -> f32 {
        let index = if self.write >= age {
            self.write - age
        } else {
            self.samples.len() + self.write - age
        };
        self.samples[index]
    }

    /// 4-point Catmull-Rom (cubic Hermite) read at `delay_samples` behind
    /// `self.write`. Caller guarantees `1.0 <= delay_samples <= max_samples`.
    #[inline]
    fn read_catmull_rom(&self, delay_samples: f64) -> f32 {
        let k = delay_samples.floor() as usize;
        let t = (delay_samples - k as f64) as f32;
        let y_m1 = self.at(k - 1);
        let y0 = self.at(k);
        let y1 = self.at(k + 1);
        let y2 = self.at(k + 2);
        let c0 = y0;
        let c1 = 0.5 * (y1 - y_m1);
        let c2 = y_m1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let c3 = 0.5 * (y2 - y_m1) + 1.5 * (y0 - y1);
        ((c3 * t + c2) * t + c1) * t + c0
    }

    /// Steps the write index forward by one sample, wrapping at the end of the
    /// buffer.
    #[inline]
    fn advance(&mut self) {
        self.write += 1;
        if self.write == self.samples.len() {
            self.write = 0;
        }
    }

    /// Delay-line step: writes `input` at age 0, reads at `raw_samples`
    /// clamped to `[1.0, max_samples]`, and advances. A non-finite `input`
    /// is written as 0, so a bad sample does not come out again one delay
    /// time later (#127).
    #[inline]
    fn step_delay(&mut self, input: f32, raw_samples: f64) -> f32 {
        let delay_samples = self.clamp_delay(raw_samples, 1.0);
        self.samples[self.write] = if input.is_finite() { input } else { 0.0 };
        let out = self.read_catmull_rom(delay_samples);
        self.advance();
        out
    }

    /// Schroeder all-pass step: reads at `raw_samples` clamped to
    /// `[2.0, max_samples]` (so `k - 1 >= 1` only reads past `w` values),
    /// flushes denormals on the feedback product, sanitizes non-finite state
    /// so a bad sample cannot circulate forever, writes `w`, and advances.
    #[inline]
    fn step_allpass(&mut self, input: f32, raw_samples: f64, gain: f32) -> f32 {
        let delay_samples = self.clamp_delay(raw_samples, 2.0);
        let delayed = self.read_catmull_rom(delay_samples);
        let mut feedback = gain * delayed;
        if feedback.abs() < 1e-20 {
            feedback = 0.0;
        }
        let mut w = input + feedback;
        if !w.is_finite() {
            w = 0.0;
        }
        let out = delayed - gain * w;
        self.samples[self.write] = w;
        self.advance();
        out
    }

    /// Zeroes all buffered samples and resets the write index.
    fn clear(&mut self) {
        self.samples.fill(0.0);
        self.write = 0;
    }
}

/// Runtime state for `loom.time.delay`.
struct Delay {
    sample_rate: f64,
    /// Smoothed in normalised 0..1 knob position, matching `ParamFill`.
    time: Smoothed,
    buffer: DelayBuffer,
}

impl Delay {
    /// Creates a `Delay` with its buffer sized for `DELAY.params[0].max`.
    fn new(config: &ModuleConfig) -> Self {
        let spec = &DELAY.params[0];
        Self {
            sample_rate: f64::from(config.sample_rate),
            time: Smoothed::new(
                spec.normalize(spec.default),
                config.sample_rate,
                spec.options.smooth_seconds,
            ),
            buffer: DelayBuffer::new(spec.max, config.sample_rate),
        }
    }
}

impl Module for Delay {
    /// Takes the running state of another instance, buffer included
    /// (ADR-0022). The buffer is copied into this one's, so nothing is
    /// allocated.
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if !self.buffer.copy_from(&other.buffer) {
            return false;
        }
        self.sample_rate = other.sample_rate;
        self.time = other.time;
        true
    }

    /// Processes a block of audio through the interpolated delay line.
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let spec = &DELAY.params[0];
        let time_modulated = io.param(0);
        let ([input, time_in], [out]) = io.split::<2, 1>();
        for i in 0..input.len() {
            let pos = self.time.tick();
            let base_time = time_modulated.at_or(i, spec.denormalize(pos));
            let raw_samples = (f64::from(base_time) + f64::from(time_in[i])) * self.sample_rate;
            out[i] = self.buffer.step_delay(input[i], raw_samples);
        }
    }

    /// Updates the target delay time in normalised 0..1 knob position.
    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.time.set_target(DELAY.params[0].normalize(value));
        }
    }

    /// Reports the current gliding delay time in seconds while the smoother is moving.
    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == 0 {
            self.time.gliding().map(|pos| DELAY.params[0].denormalize(pos))
        } else {
            None
        }
    }

    /// Clears the delay buffer and settles the delay-time smoother.
    fn reset(&mut self) {
        self.buffer.clear();
        self.time.reset(self.time.target());
    }
}

/// Runtime state for `loom.time.allpass`.
struct Allpass {
    sample_rate: f64,
    /// Smoothed in normalised 0..1 knob position, matching `ParamFill`.
    time: Smoothed,
    gain: Smoothed,
    buffer: DelayBuffer,
}

impl Allpass {
    /// Creates an `Allpass` with its buffer sized for `ALLPASS.params[0].max`.
    fn new(config: &ModuleConfig) -> Self {
        let time_spec = &ALLPASS.params[0];
        let gain_spec = &ALLPASS.params[1];
        Self {
            sample_rate: f64::from(config.sample_rate),
            time: Smoothed::new(
                time_spec.normalize(time_spec.default),
                config.sample_rate,
                time_spec.options.smooth_seconds,
            ),
            gain: Smoothed::new(gain_spec.default, config.sample_rate, gain_spec.options.smooth_seconds),
            buffer: DelayBuffer::new(time_spec.max, config.sample_rate),
        }
    }
}

impl Module for Allpass {
    /// Takes the running state of another instance, buffer included
    /// (ADR-0022). The buffer is copied into this one's, so nothing is
    /// allocated.
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        let Some(other) = other.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if !self.buffer.copy_from(&other.buffer) {
            return false;
        }
        self.sample_rate = other.sample_rate;
        self.time = other.time;
        self.gain = other.gain;
        true
    }

    /// Processes a block of audio through the Schroeder delay-line all-pass filter.
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let time_spec = &ALLPASS.params[0];
        let time_modulated = io.param(0);
        let gain_modulated = io.param(1);
        let ([input, time_in], [out]) = io.split::<2, 1>();
        for i in 0..input.len() {
            let pos = self.time.tick();
            let base_time = time_modulated.at_or(i, time_spec.denormalize(pos));
            let gain = gain_modulated.at_or(i, self.gain.tick()).clamp(-0.95, 0.95);
            let raw_samples = (f64::from(base_time) + f64::from(time_in[i])) * self.sample_rate;
            out[i] = self.buffer.step_allpass(input[i], raw_samples, gain);
        }
    }

    /// Updates the target delay time or all-pass gain.
    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.time.set_target(ALLPASS.params[0].normalize(value)),
            1 => self.gain.set_target(value),
            _ => {}
        }
    }

    /// Reports the current gliding parameter value while its smoother is moving.
    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            0 => self.time.gliding().map(|pos| ALLPASS.params[0].denormalize(pos)),
            1 => self.gain.gliding(),
            _ => None,
        }
    }

    /// Clears the delay buffer and settles the time and gain smoothers.
    fn reset(&mut self) {
        self.buffer.clear();
        self.time.reset(self.time.target());
        self.gain.reset(self.gain.target());
    }
}

#[cfg(test)]
mod tests;
