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
mod tests {
    use super::*;
    use loom_core::testing::run;
    use std::f32::consts::TAU;

    /// Builds a `loom.time.delay` instance settled at `time` seconds.
    fn make_delay(sample_rate: f32, time: f32) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = DELAY.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate,
            max_block: 512,
        });
        m.set_param(0, time);
        m.reset();
        m
    }

    /// Builds a `loom.time.allpass` instance settled at `time` seconds and `gain`.
    fn make_allpass(sample_rate: f32, time: f32, gain: f32) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = ALLPASS.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate,
            max_block: 512,
        });
        m.set_param(0, time);
        m.set_param(1, gain);
        m.reset();
        m
    }

    /// Checks that an integer delay shifts an impulse by the exact number of samples.
    #[test]
    fn integer_delay_shifts_an_impulse_by_exact_samples() {
        let mut buf = DelayBuffer::new(0.1, 1_000.0);
        for i in 0..32 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = buf.step_delay(x, 10.0);
            let expected = if i == 10 { 1.0 } else { 0.0 };
            assert_eq!(y, expected, "step_delay frame {i}");
        }

        // Through Module::process: use dyadic floats so (base_time + time_in) * sample_rate
        // is an exact integer (2.0 - 1.984375 = 1/64 s; at 640 Hz that is 10.0 samples).
        let mut delay = make_delay(640.0, 2.0);
        let mut input = [0.0f32; 32];
        input[0] = 1.0;
        let time_in = [-1.984_375f32; 32];
        let mut out = [0.0f32; 32];
        run(
            delay.as_mut(),
            640.0,
            &[&input, &time_in],
            &[true, true],
            &mut [&mut out],
            &[],
        );
        for (i, &sample) in out.iter().enumerate() {
            let expected = if i == 10 { 1.0 } else { 0.0 };
            assert_eq!(sample, expected, "module frame {i}");
        }
    }

    /// Checks that 4-point Catmull-Rom reproduces a ramp and sweeps smoothly at 192 kHz.
    #[test]
    fn fractional_catmull_rom_matches_the_cubic_formula_and_sweeps_smoothly_at_long_delays() {
        // On a linear ramp x[n] = n, Catmull-Rom reproduces the line exactly
        // once all four stencil points (k - 1 ..= k + 2) lie on the ramp.
        let mut buf = DelayBuffer::new(0.1, 1_000.0);
        for n in 0..30 {
            let y = buf.step_delay(n as f32, 5.375);
            if n >= 8 {
                let expected = n as f32 - 5.375;
                assert!((y - expected).abs() < 1e-5, "frame {n}: got {y}, expected {expected}");
            }
        }

        // At 192 kHz near 2.0 s (384,000 samples), f32 has a step of 1/32
        // sample. Check that a fine f64 sweep of 0.001 samples per frame
        // changes the output on every frame rather than quantising into
        // 32-frame plateaus.
        let sample_rate = 192_000.0;
        let mut long_buf = DelayBuffer::new(2.0, sample_rate);
        let base_delay = 380_000usize;
        for n in 0..base_delay + 10 {
            let _ = long_buf.step_delay((n as f32 * 0.1).sin(), base_delay as f64);
        }
        let mut prev = long_buf.step_delay(0.5, base_delay as f64);
        let mut changed = 0;
        for step in 1..=16 {
            let d = base_delay as f64 + step as f64 * 0.001;
            let next = long_buf.step_delay(0.5, d);
            if next != prev {
                changed += 1;
            }
            prev = next;
        }
        assert_eq!(changed, 16, "f64 delay position must advance every frame at 192 kHz");
    }

    /// Checks delay bounds clamping and NaN/infinity sanitisation on `time` and `in`.
    #[test]
    fn clamps_limits_and_sanitizes_nan_and_infinity_on_time_input() {
        // Delay line clamps to [1, max_samples]. At 1,000 Hz, 1 sample is a
        // 1-frame shift; NaN and negative times must clamp to 1 sample.
        let mut delay = make_delay(1_000.0, 0.0001);
        let input = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let time_in = [
            -10.0,
            f32::NAN,
            f32::NEG_INFINITY,
            0.0,
            f32::INFINITY,
            -1.0,
            f32::NAN,
            0.0,
        ];
        let mut out = [0.0; 8];
        run(
            delay.as_mut(),
            1_000.0,
            &[&input, &time_in],
            &[true, true],
            &mut [&mut out],
            &[],
        );
        assert!(out.iter().all(|x| x.is_finite()), "{out:?}");
        // Frame 0 writes 1.0; frame 1 (time = NaN -> 1 sample) reads frame 0's 1.0.
        assert_eq!(out[1], 1.0);
        assert_eq!(out[2], 2.0);
        assert_eq!(out[3], 3.0);

        // All-pass clamps to [2, max_samples]. With gain = 0 and negative/NaN
        // time inputs, the impulse comes out at frame 2, not frame 1.
        let mut allpass = make_allpass(1_000.0, 0.0001, 0.0);
        let impulse = [1.0, 0.0, 0.0, 0.0, 0.0];
        let extreme_time = [-5.0, f32::NAN, f32::NEG_INFINITY, -1.0, f32::INFINITY];
        let mut ap_out = [0.0; 5];
        run(
            allpass.as_mut(),
            1_000.0,
            &[&impulse, &extreme_time],
            &[true, true],
            &mut [&mut ap_out],
            &[],
        );
        assert!(ap_out.iter().all(|x| x.is_finite()), "{ap_out:?}");
        assert_eq!(ap_out[0], 0.0);
        assert_eq!(ap_out[1], 0.0);
        assert_eq!(ap_out[2], 1.0);

        // A single non-finite audio sample into Allpass must not poison the
        // feedback buffer; output remains finite and subsequent impulses still pass.
        let mut allpass = make_allpass(1_000.0, 0.005, 0.6);
        let mut bad_then_good = [0.0f32; 16];
        bad_then_good[0] = f32::NAN;
        bad_then_good[1] = f32::INFINITY;
        bad_then_good[4] = 1.0;
        let zero_time = [0.0f32; 16];
        let mut rec_out = [0.0f32; 16];
        run(
            allpass.as_mut(),
            1_000.0,
            &[&bad_then_good, &zero_time],
            &[true, false],
            &mut [&mut rec_out],
            &[],
        );
        assert!(rec_out.iter().all(|x| x.is_finite()), "{rec_out:?}");
        assert!(
            (rec_out[9] - 0.64).abs() < 1e-4,
            "impulse after NaN recovers: {}",
            rec_out[9]
        );
    }

    /// Checks that `loom.time.allpass` with `gain = 0` equals `loom.time.delay` for the same delay in samples.
    #[test]
    fn allpass_with_zero_gain_equals_delay_line_for_same_delay_in_samples() {
        // 1. Step level: identical `delay_samples >= 2.0`, including fractional
        // sweeps, -0.0, and quiet 1e-25 inputs below the 1e-20 feedback flush.
        let mut delay_buf = DelayBuffer::new(0.2, 48_000.0);
        let mut ap_buf = DelayBuffer::new(0.2, 48_000.0);
        for i in 0..500 {
            let x = match i {
                0 => -0.0,
                1..=20 => 1e-25 * (i as f32),
                _ => (i as f32 * 0.13).sin(),
            };
            let d = 2.0 + 50.0 * (1.0 + (i as f64 * 0.03).sin());
            let y_delay = delay_buf.step_delay(x, d);
            let y_ap = ap_buf.step_allpass(x, d, 0.0);
            assert_eq!(y_ap, y_delay, "frame {i} at d = {d}");
        }

        // 2. Module level: with both knobs at their minimum (0.0001 s, where
        // normalize == 0.0 and denormalize(0.0) == 0.0001 on both descriptors)
        // and the delay driven by the `time` input port.
        let mut delay = make_delay(48_000.0, 0.0001);
        let mut allpass = make_allpass(48_000.0, 0.0001, 0.0);
        let input: Vec<f32> = (0..256)
            .map(|i| if i < 16 { 1e-25 } else { (i as f32 * 0.07).cos() })
            .collect();
        let time_in: Vec<f32> = (0..256).map(|i| 0.005 + 0.001 * (i as f32 * 0.05).sin()).collect();
        let mut out_delay = vec![0.0; 256];
        let mut out_ap = vec![0.0; 256];
        run(
            delay.as_mut(),
            48_000.0,
            &[&input, &time_in],
            &[true, true],
            &mut [&mut out_delay],
            &[],
        );
        run(
            allpass.as_mut(),
            48_000.0,
            &[&input, &time_in],
            &[true, true],
            &mut [&mut out_ap],
            &[],
        );
        assert_eq!(out_ap, out_delay);
    }

    /// Checks the Schroeder all-pass impulse response and flat steady-state magnitude across frequencies.
    #[test]
    fn allpass_has_expected_impulse_response_and_flat_steady_state_magnitude() {
        // Impulse at D = 5 samples (0.005 s at 1,000 Hz) and g = 0.6:
        // y[0] = -g = -0.6
        // y[5] = 1 - g^2 = 0.64
        // y[10] = (1 - g^2) * g = 0.384
        // y[15] = (1 - g^2) * g^2 = 0.2304
        let g = 0.6f32;
        let mut ap = make_allpass(1_000.0, 0.0001, g);
        let mut impulse = [0.0f32; 20];
        impulse[0] = 1.0;
        let time_in = [0.0049f32; 20];
        let mut out = [0.0f32; 20];
        run(
            ap.as_mut(),
            1_000.0,
            &[&impulse, &time_in],
            &[true, true],
            &mut [&mut out],
            &[],
        );
        assert!((out[0] - (-g)).abs() < 1e-6, "y[0] = {}", out[0]);
        assert!((out[5] - (1.0 - g * g)).abs() < 1e-6, "y[5] = {}", out[5]);
        assert!((out[10] - (1.0 - g * g) * g).abs() < 1e-6, "y[10] = {}", out[10]);
        assert!((out[15] - (1.0 - g * g) * g * g).abs() < 1e-6, "y[15] = {}", out[15]);

        // Steady-state flat magnitude response: with an integer sample delay
        // (D = 12 samples at 48 kHz: 0.0001 s knob + 0.00015 s input = 0.00025 s)
        // and g = 0.6, let each sine tone settle for 400 echo periods (4,800
        // frames) and measure RMS over the next 4,800 frames (an exact whole
        // number of cycles for every test frequency). Relative RMS error must
        // be within 1e-4.
        let sample_rate = 48_000.0;
        let frames = 9_600;
        let settle = 4_800;
        let time_in = vec![0.00015f32; frames];
        for freq in [110.0f32, 440.0, 1_760.0, 7_040.0] {
            let mut ap = make_allpass(sample_rate, 0.0001, 0.6);
            let input: Vec<f32> = (0..frames)
                .map(|n| (TAU * freq * n as f32 / sample_rate).sin())
                .collect();
            let mut out = vec![0.0f32; frames];
            run(
                ap.as_mut(),
                sample_rate,
                &[&input, &time_in],
                &[true, true],
                &mut [&mut out],
                &[],
            );
            let rms = |s: &[f32]| (s.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>() / s.len() as f64).sqrt();
            let in_rms = rms(&input[settle..]);
            let out_rms = rms(&out[settle..]);
            let rel_err = ((out_rms - in_rms) / in_rms).abs();
            assert!(
                rel_err < 1e-4,
                "{freq} Hz: in_rms = {in_rms}, out_rms = {out_rms}, rel_err = {rel_err}"
            );
        }
    }

    /// Checks that `reset` zeroes buffered echoes and settles parameter smoothers on both modules.
    #[test]
    fn reset_clears_buffered_echoes_and_settles_smoothers() {
        let mut delay = make_delay(1_000.0, 0.01);
        let loud = [1.0f32; 32];
        let zero = [0.0f32; 32];
        let mut out = [0.0f32; 32];
        run(
            delay.as_mut(),
            1_000.0,
            &[&loud, &zero],
            &[true, false],
            &mut [&mut out],
            &[],
        );
        assert!(out.iter().any(|&x| x != 0.0));
        delay.set_param(0, 0.05);
        assert!(delay.param_glide(0).is_some());
        delay.reset();
        assert!(delay.param_glide(0).is_none());
        run(
            delay.as_mut(),
            1_000.0,
            &[&zero, &zero],
            &[true, false],
            &mut [&mut out],
            &[],
        );
        assert!(out.iter().all(|&x| x == 0.0), "{out:?}");

        let mut allpass = make_allpass(1_000.0, 0.01, 0.7);
        run(
            allpass.as_mut(),
            1_000.0,
            &[&loud, &zero],
            &[true, false],
            &mut [&mut out],
            &[],
        );
        allpass.set_param(0, 0.02);
        allpass.set_param(1, -0.5);
        allpass.reset();
        assert!(allpass.param_glide(0).is_none());
        assert!(allpass.param_glide(1).is_none());
        run(
            allpass.as_mut(),
            1_000.0,
            &[&zero, &zero],
            &[true, false],
            &mut [&mut out],
            &[],
        );
        assert!(out.iter().all(|&x| x == 0.0), "{out:?}");
    }

    /// Checks that `loom.time.delay` and `loom.time.allpass` stay finite and bounded under
    /// audio-rate `time` modulation (both via the `time` input port and via parameter
    /// modulation) at extreme feedback gains (`±0.95`), and that the all-pass tail decays
    /// after the input stops.
    #[test]
    fn delay_and_allpass_stay_bounded_under_audio_rate_time_modulation() {
        let sample_rate = 48_000.0;
        let active_frames = 24_000;
        let tail_frames = 24_000;
        let total_frames = active_frames + tail_frames;

        // Multi-tone + deterministic pseudo-noise excitation bounded to [-1.0, 1.0],
        // followed by silence so we can verify tail decay under ongoing audio-rate modulation.
        let mut seed = 0x1234_5678u32;
        let mut next_noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        let input: Vec<f32> = (0..total_frames)
            .map(|n| {
                if n < active_frames {
                    let t = n as f32 / sample_rate;
                    let tones = 0.5 * (TAU * 220.0 * t).sin() + 0.3 * (TAU * 1_760.0 * t).cos();
                    (tones + 0.2 * next_noise()).clamp(-1.0, 1.0)
                } else {
                    0.0
                }
            })
            .collect();

        // Audio-rate `time` input combining 137 Hz, 1,100 Hz, and 6,000 Hz modulation
        // plus fast deterministic jitter around a 5 ms base delay.
        let time_in: Vec<f32> = (0..total_frames)
            .map(|n| {
                let t = n as f32 / sample_rate;
                0.002 * (TAU * 137.0 * t).sin()
                    + 0.0008 * (TAU * 1_100.0 * t).cos()
                    + 0.0002 * (TAU * 6_000.0 * t).sin()
                    + 0.0001 * next_noise()
            })
            .collect();

        // 1. Delay line: Catmull-Rom L1 weight sum is 1 + t*(1 - t) <= 1.25, so for
        // any input in [-1.0, 1.0] and any delay trajectory, |y[n]| <= 1.25.
        let mut delay = make_delay(sample_rate, 0.005);
        let mut delay_out = vec![0.0f32; total_frames];
        run(
            delay.as_mut(),
            sample_rate,
            &[&input, &time_in],
            &[true, true],
            &mut [&mut delay_out],
            &[],
        );
        assert!(delay_out.iter().all(|x| x.is_finite()));
        let delay_peak = delay_out.iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));
        assert!(
            delay_peak <= 1.25 + 1e-5,
            "delay output exceeded Catmull-Rom L1 bound: {delay_peak}"
        );

        // 2. All-pass at extreme positive and negative gains (±0.95) under audio-rate
        // time modulation: outputs stay finite and bounded, and the tail decays once
        // the input stops.
        for gain in [0.95f32, -0.95f32] {
            let mut ap = make_allpass(sample_rate, 0.005, gain);
            let mut ap_out = vec![0.0f32; total_frames];
            run(
                ap.as_mut(),
                sample_rate,
                &[&input, &time_in],
                &[true, true],
                &mut [&mut ap_out],
                &[],
            );
            assert!(
                ap_out.iter().all(|x| x.is_finite()),
                "non-finite sample for gain {gain}"
            );
            let peak = ap_out.iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));
            assert!(peak < 10.0, "allpass peak grew excessively for gain {gain}: {peak}");
            let active_rms = (ap_out[..active_frames]
                .iter()
                .map(|&x| f64::from(x) * f64::from(x))
                .sum::<f64>()
                / active_frames as f64)
                .sqrt();
            let late_tail_rms = (ap_out[total_frames - 4_800..]
                .iter()
                .map(|&x| f64::from(x) * f64::from(x))
                .sum::<f64>()
                / 4_800.0)
                .sqrt();
            assert!(
                late_tail_rms < active_rms * 0.05,
                "allpass tail did not decay for gain {gain}: active_rms = {active_rms}, late_tail_rms = {late_tail_rms}"
            );
        }

        // 3. Audio-rate sinusoidal and square-wave `time` modulation across frequencies
        // (including deep sweeps that hit the 2-sample minimum clamp) at gain = ±0.95.
        for mod_freq in [50.0f32, 440.0, 2_400.0, 9_600.0] {
            let sweep_time: Vec<f32> = (0..total_frames)
                .map(|n| {
                    let phase = TAU * mod_freq * n as f32 / sample_rate;
                    // Sweeps below -0.005 s to exercise the 2-sample minimum clamp at audio rate.
                    0.006 * phase.sin() + 0.001 * if phase.cos() >= 0.0 { 1.0 } else { -1.0 }
                })
                .collect();
            for gain in [0.95f32, -0.95f32] {
                let mut ap = make_allpass(sample_rate, 0.005, gain);
                let mut ap_out = vec![0.0f32; total_frames];
                run(
                    ap.as_mut(),
                    sample_rate,
                    &[&input, &sweep_time],
                    &[true, true],
                    &mut [&mut ap_out],
                    &[],
                );
                assert!(ap_out.iter().all(|x| x.is_finite()));
                let peak = ap_out.iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));
                assert!(
                    peak < 15.0,
                    "allpass peak unbounded at mod_freq = {mod_freq} Hz, gain = {gain}: {peak}"
                );
                let tail_peak = ap_out[total_frames - 2_400..]
                    .iter()
                    .fold(0.0f32, |acc, &x| acc.max(x.abs()));
                assert!(
                    tail_peak < 0.05,
                    "allpass tail did not decay at mod_freq = {mod_freq} Hz, gain = {gain}: {tail_peak}"
                );
            }
        }

        // 4. Fractional delays t in {0.1, 0.25, 0.5, 0.75, 0.9} at maximum gain (0.95):
        // because |F_t(e^{jw})| <= 1 for all frequencies, the steady-state output RMS
        // never exceeds the input RMS.
        let frames = 9_600;
        let settle = 4_800;
        for frac in [0.1f32, 0.25, 0.5, 0.75, 0.9] {
            // Base knob = 0.0001 s (4.8 samples at 48 kHz); choose time_in so total
            // delay is 12 + frac samples.
            let target_samples = 12.0 + frac;
            let time_in_val = (target_samples / sample_rate) - 0.0001;
            let time_in = vec![time_in_val; frames];
            for freq in [220.0f32, 1_760.0, 7_040.0, 14_080.0] {
                let mut ap = make_allpass(sample_rate, 0.0001, 0.95);
                let tone: Vec<f32> = (0..frames)
                    .map(|n| (TAU * freq * n as f32 / sample_rate).sin())
                    .collect();
                let mut out = vec![0.0f32; frames];
                run(
                    ap.as_mut(),
                    sample_rate,
                    &[&tone, &time_in],
                    &[true, true],
                    &mut [&mut out],
                    &[],
                );
                let rms =
                    |s: &[f32]| (s.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>() / s.len() as f64).sqrt();
                let in_rms = rms(&tone[settle..]);
                let out_rms = rms(&out[settle..]);
                assert!(
                    out_rms <= in_rms * (1.0 + 1e-4),
                    "fractional delay t = {frac} at {freq} Hz amplified steady-state tone: in_rms = {in_rms}, out_rms = {out_rms}"
                );
            }
        }
    }
}
