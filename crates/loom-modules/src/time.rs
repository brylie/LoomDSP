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
use loom_core::{FillsRole, roles};
use loom_dsp::delay;
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
    roles: &[FillsRole::of(&roles::EFFECT)],
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
    roles: &[FillsRole::of(&roles::EFFECT)],
};

/// Runtime state for `loom.time.delay`.
struct Delay {
    sample_rate: f64,
    /// Smoothed in normalised 0..1 knob position, matching `ParamFill`.
    time: Smoothed,
    buffer: delay::DelayLine,
}

impl Delay {
    const TIME: usize = DELAY.param_index("time").unwrap();

    /// Creates a `Delay` with its buffer sized for `DELAY.params[Self::TIME].max`.
    fn new(config: &ModuleConfig) -> Self {
        let spec = &DELAY.params[Self::TIME];
        Self {
            sample_rate: f64::from(config.sample_rate),
            time: Smoothed::new(
                spec.normalize(spec.default),
                config.sample_rate,
                spec.options.smooth_seconds,
            ),
            buffer: delay::DelayLine::new(spec.max, config.sample_rate),
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
        let spec = &DELAY.params[Self::TIME];
        let time_modulated = io.param(Self::TIME);
        let ([input, time_in], [out]) = io.split::<2, 1>();
        for i in 0..input.len() {
            let pos = self.time.tick();
            let base_time = time_modulated.at_or(i, spec.denormalize(pos));
            let raw_samples = (f64::from(base_time) + f64::from(time_in[i])) * self.sample_rate;
            // A bad sample must not come out again one delay time later (#127).
            let x = loom_dsp::guard(input[i]);
            out[i] = self.buffer.tick(x, raw_samples);
        }
    }

    /// Updates the target delay time in normalised 0..1 knob position.
    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::TIME {
            self.time.set_target(DELAY.params[Self::TIME].normalize(value));
        }
    }

    /// Reports the current gliding delay time in seconds while the smoother is moving.
    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::TIME {
            self.time.gliding().map(|pos| DELAY.params[Self::TIME].denormalize(pos))
        } else {
            None
        }
    }

    /// Clears the delay buffer and settles the delay-time smoother.
    fn reset(&mut self) {
        self.buffer.reset();
        self.time.reset(self.time.target());
    }
}

/// Runtime state for `loom.time.allpass`.
struct Allpass {
    sample_rate: f64,
    /// Smoothed in normalised 0..1 knob position, matching `ParamFill`.
    time: Smoothed,
    gain: Smoothed,
    buffer: delay::Allpass,
}

impl Allpass {
    const TIME: usize = ALLPASS.param_index("time").unwrap();
    const GAIN: usize = ALLPASS.param_index("gain").unwrap();

    /// Creates an `Allpass` with its buffer sized for `ALLPASS.params[Self::TIME].max`.
    fn new(config: &ModuleConfig) -> Self {
        let time_spec = &ALLPASS.params[Self::TIME];
        let gain_spec = &ALLPASS.params[Self::GAIN];
        Self {
            sample_rate: f64::from(config.sample_rate),
            time: Smoothed::new(
                time_spec.normalize(time_spec.default),
                config.sample_rate,
                time_spec.options.smooth_seconds,
            ),
            gain: Smoothed::new(gain_spec.default, config.sample_rate, gain_spec.options.smooth_seconds),
            buffer: delay::Allpass::new(time_spec.max, config.sample_rate),
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
        let time_spec = &ALLPASS.params[Self::TIME];
        let time_modulated = io.param(Self::TIME);
        let gain_modulated = io.param(Self::GAIN);
        let ([input, time_in], [out]) = io.split::<2, 1>();
        for i in 0..input.len() {
            let pos = self.time.tick();
            let base_time = time_modulated.at_or(i, time_spec.denormalize(pos));
            let gain = gain_modulated.at_or(i, self.gain.tick()).clamp(-0.95, 0.95);
            let raw_samples = (f64::from(base_time) + f64::from(time_in[i])) * self.sample_rate;
            out[i] = self.buffer.tick(loom_dsp::guard(input[i]), raw_samples, gain);
        }
    }

    /// Updates the target delay time or all-pass gain.
    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::TIME => self.time.set_target(ALLPASS.params[Self::TIME].normalize(value)),
            Self::GAIN => self.gain.set_target(value),
            _ => {}
        }
    }

    /// Reports the current gliding parameter value while its smoother is moving.
    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            Self::TIME => self
                .time
                .gliding()
                .map(|pos| ALLPASS.params[Self::TIME].denormalize(pos)),
            Self::GAIN => self.gain.gliding(),
            _ => None,
        }
    }

    /// Clears the delay buffer and settles the time and gain smoothers.
    fn reset(&mut self) {
        self.buffer.reset();
        self.time.reset(self.time.target());
        self.gain.reset(self.gain.target());
    }
}

#[cfg(test)]
mod tests;
