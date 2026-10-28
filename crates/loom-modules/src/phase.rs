//! Phase generator: the timing source for oscillators (FR-LIB-01).
//!
//! Why a separate phase module: separating timing from wave shape lets one
//! ramp drive any shaper, and lets users bend time (sync, phase distortion,
//! step indexing) before it becomes sound.

use loom_core::signal::{EdgeDetector, WrapDetector, pitch, trigger};
use loom_core::{
    Channels, DisplayKind, DisplaySpec, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions,
    ParamSpec, Polarity, PortDefault, PortIo, PortSpec, ProcessContext, SignalType, TriggerFlag, Widget,
};

/// `loom.phase.generator`: a ramp from 0 to 1 at the frequency set by pitch.
pub static PHASE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.generator",
    version: 1,
    name: "Phase",
    category: "Phase",
    description: "Ramp from 0 to 1 at the pitch frequency. Drives oscillators and LFOs.",
    inputs: &[
        PortSpec {
            name: "pitch",
            label: "Pitch",
            signal: SignalType::Pitch,
            description: "Sets the frequency. 0 is middle C, and +0.1 is one octave up.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "A rising edge sets the phase back to 0.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "A ramp from 0 up to 1, once per cycle.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "tune",
            label: "Tune",
            description: "Shifts the pitch in semitones.",
            min: -48.0,
            max: 48.0,
            default: 0.0,
            unit: "st",
            options: ParamOptions::DEFAULT,
        },
        ParamSpec {
            name: "octave",
            label: "Octave",
            description: "Shifts the pitch in octaves.",
            min: -10.0,
            max: 10.0,
            default: 0.0,
            unit: "oct",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|config| Box::new(Phase::new(config))),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "tune"), Widget::knob(3, 0, "octave")]),
    displays: &[],
};

#[derive(Clone)]
struct Phase {
    phase: f64,
    sample_rate: f64,
    tune: f32,
    octave: f32,
    reset_in: EdgeDetector,
}

impl Phase {
    fn new(config: &ModuleConfig) -> Self {
        Self {
            phase: 0.0,
            sample_rate: f64::from(config.sample_rate),
            tune: 0.0,
            octave: 0.0,
            reset_in: EdgeDetector::new(),
        }
    }
}

/// Largest phase step per sample: just under half a cycle (Nyquist).
///
/// Not exactly 0.5: then the phase would read 0, 0.5, 0, 0.5, and no step
/// would be more than half a cycle, so `WrapDetector` would see no wraps
/// and a Phase Trigger would stop firing. At 0.49 every wrap is a jump of
/// 0.51, clear of `f32` rounding.
const MAX_STEP: f64 = 0.49;

/// Phase step per sample for a frequency, at most [`MAX_STEP`].
///
/// A high pitch overflows `to_hz` to infinity, and `inf - floor(inf)` is
/// NaN. Without this limit the phase would stay NaN for good, so a patch
/// stays silent even after the pitch comes back (#121). Frequencies near
/// Nyquist and above cannot be heard as themselves anyway. NaN pitch stops
/// the phase.
fn increment(hz: f32, sample_rate: f64) -> f64 {
    let step = f64::from(hz) / sample_rate;
    if step.is_nan() { 0.0 } else { step.min(MAX_STEP) }
}

impl Module for Phase {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let tune = io.param(0);
        let octave = io.param(1);
        let ([pitch_in, reset], [out]) = io.split::<2, 1>();
        for (i, ((out, &p), &r)) in out.iter_mut().zip(pitch_in).zip(reset).enumerate() {
            let offset =
                tune.at_or(i, self.tune) * pitch::PER_SEMITONE + octave.at_or(i, self.octave) * pitch::PER_OCTAVE;
            if self.reset_in.rising(r) {
                self.phase = 0.0;
            }

            *out = self.phase as f32;
            self.phase += increment(pitch::to_hz(p + offset), self.sample_rate);
            self.phase -= self.phase.floor();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.tune = value,
            1 => self.octave = value.round(),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.reset_in.reset();
    }
}

/// `loom.phase.trigger`: fires a trigger each time its input phase wraps.
///
/// It turns any phase into triggers. With Phase Scale in front, it is a
/// clock multiplier that stays locked to its input. The wrap rule is
/// [`WrapDetector`]'s, shared with every end-of-cycle trigger.
pub static PHASE_TRIGGER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.trigger",
    version: 1,
    name: "Phase Trigger",
    category: "Phase",
    description: "Fires a trigger each time the phase wraps, forward or backward.",
    inputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The phase to watch. Each wrap fires a trigger.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "trigger",
        label: "Trigger",
        signal: SignalType::Gate,
        description: "Fires once each time the phase wraps.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| {
        Box::new(PhaseTrigger {
            wrap: WrapDetector::new(),
            fired: TriggerFlag::default(),
        })
    }),
    panel: Panel::new(3, 2, &[Widget::light(0, 0, "fired")]),
    displays: &[DisplaySpec {
        name: "fired",
        label: "Fired",
        description: "Flashes each time the phase wraps.",
        kind: DisplayKind::Trigger,
    }],
};

#[derive(Clone)]
struct PhaseTrigger {
    wrap: WrapDetector,
    /// Whether the output fired since the light last read it.
    fired: TriggerFlag,
}

impl Module for PhaseTrigger {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase], [out]) = io.split::<1, 1>();
        for (out, &p) in out.iter_mut().zip(phase) {
            let fired = self.wrap.wrapped(p);
            self.fired.record(fired);
            *out = trigger(fired);
        }
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.fired.take();
    }

    fn reset(&mut self) {
        self.wrap.reset();
        self.fired.clear();
    }
}

/// `loom.phase.wrap` (MT-PHASE-08): brings any number into the phase range.
pub static WRAP: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.wrap",
    version: 1,
    name: "Phase wrap and fold",
    category: "Phase",
    description: "Brings any signal into the phase range, 0 to 1. Wrap goes round the edge; fold turns back at the edges.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Modulation,
        description: "The signal to bring into the phase range.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Phase,
        description: "A phase from 0 to 1.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "mode",
        label: "Mode",
        description: "Wrap goes round the edge. Fold turns back at the edges.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Wrap", "Fold"])],
    role: ModuleRole::Dsp(|_| Box::new(Wrap { fold: false })),
    panel: Panel::new(6, 3, &[Widget::radio(0, 0, "mode")]),
    displays: &[],
};

struct Wrap {
    fold: bool,
}

impl Wrap {
    /// `x - floor(x)`, always in `[0, 1)`.
    fn wrap(x: f32) -> f32 {
        let w = x - x.floor();
        // A tiny negative `x` can round the result up to exactly 1.
        if w >= 1.0 { 0.0 } else { w }
    }

    /// A triangle fold into `[0, 1]`: 1 stays 1, 1.25 comes back to 0.75.
    fn fold(x: f32) -> f32 {
        let t = x - 2.0 * (x / 2.0).floor();
        let folded = if t <= 1.0 { t } else { 2.0 - t };
        folded.clamp(0.0, 1.0)
    }
}

impl Module for Wrap {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let mode_mod = io.param(0);
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            // A modulated mode counts for this frame only; the base stays
            // in the field.
            let fold = mode_mod.map_or(i, self.fold, |v| v >= 0.5);
            // A phase module must not pass NaN on to oscillators.
            *out = if !x.is_finite() {
                0.0
            } else if fold {
                Self::fold(x)
            } else {
                Self::wrap(x)
            };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.fold = value >= 0.5;
        }
    }
}

/// `loom.phase.scale` (MT-PHASE-02): runs a phase faster or slower by a
/// whole-number ratio, and stays locked to it.
pub static SCALE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.phase.scale",
    version: 1,
    name: "Phase Scale",
    category: "Phase",
    description: "Runs a phase at multiply / divide times its speed, locked to the input. Divide counts the input's wraps from the first cycle or the last reset.",
    inputs: &[
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "The phase to scale.",
            // An unpatched Phase Scale follows the bar, so it works as a
            // clock multiplier with no cables. A flat 0 would never wrap.
            default: PortDefault::BarPhase,
            polarity: Polarity::Unipolar,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "A rising edge starts counting the input's wraps again from zero.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "phase",
        label: "Phase",
        signal: SignalType::Phase,
        description: "The scaled phase, from 0 to 1.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Unipolar,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "multiply",
            label: "Multiply",
            description: "How many times faster the output runs.",
            min: 1.0,
            max: 16.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "divide",
            label: "Divide",
            description: "How many times slower the output runs: it wraps once every this many input wraps.",
            min: 1.0,
            max: 16.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|_| Box::new(PhaseScale::new())),
    panel: Panel::new(6, 3, &[Widget::knob(0, 0, "multiply"), Widget::knob(3, 0, "divide")]),
    displays: &[],
};

#[derive(Clone)]
struct PhaseScale {
    multiply: i64,
    divide: i64,
    /// The previous input phase. `None` before the first frame.
    previous: Option<f32>,
    /// Wraps of the input since the first cycle or the last reset. Taken
    /// modulo `divide`, so a changed divide still lines up.
    wraps: i64,
    reset: EdgeDetector,
}

impl PhaseScale {
    fn new() -> Self {
        Self {
            multiply: 1,
            divide: 1,
            previous: None,
            wraps: 0,
            reset: EdgeDetector::new(),
        }
    }
}

impl Module for PhaseScale {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (multiply_mod, divide_mod) = (io.param(0), io.param(1));
        let ([phase, reset], [out]) = io.split::<2, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            // A modulated value counts for this frame only. The base stays
            // in the field, so removing the assignment returns to it.
            let multiply = multiply_mod.map_or(i, self.multiply, |v| v.round().max(1.0) as i64);
            let divide = divide_mod.map_or(i, self.divide, |v| v.round().max(1.0) as i64);
            let p = if phase[i].is_finite() { phase[i] } else { 0.0 };
            // A jump of more than half a cycle is a wrap, as in Phase
            // Trigger. Going forward adds a cycle and going backward takes
            // one away. The first frame has nothing to compare with.
            if let Some(previous) = self.previous {
                let step = p - previous;
                if step < -0.5 {
                    self.wraps += 1;
                } else if step > 0.5 {
                    self.wraps -= 1;
                }
            }
            self.previous = Some(p);
            // Reset on the same frame as a wrap: the reset comes first, so
            // this frame is the first one of the new count.
            if self.reset.rising(reset[i]) {
                self.wraps = 0;
            }
            // The cycle we are in, among `divide` of them, plus how far
            // through the input cycle we are. Done in f64, so a long run
            // does not lose the fraction.
            let cycle = self.wraps.rem_euclid(divide) as f64;
            let scaled = (cycle + f64::from(p)) * multiply as f64 / divide as f64;
            let wrapped = (scaled - scaled.floor()) as f32;
            // An f32 can round up to exactly 1.
            *out = if wrapped >= 1.0 { 0.0 } else { wrapped };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.multiply = value.round().max(1.0) as i64,
            1 => self.divide = value.round().max(1.0) as i64,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.previous = None;
        self.wraps = 0;
        self.reset.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;

    fn phase_trigger(input: &[f32]) -> Vec<f32> {
        let mut module = PhaseTrigger {
            wrap: WrapDetector::new(),
            fired: TriggerFlag::default(),
        };
        let mut out = vec![0.0; input.len()];
        run(&mut module, 48_000.0, &[input], &[true], &mut [&mut out], &[]);
        out
    }

    #[test]
    fn fires_one_sample_on_each_wrap() {
        // Four frames per cycle, starting on a bar line.
        let input: Vec<f32> = (0..12).map(|i| (i % 4) as f32 / 4.0).collect();
        let out = phase_trigger(&input);
        let fired: Vec<usize> = (0..out.len()).filter(|&i| out[i] == 1.0).collect();
        assert_eq!(fired, [0, 4, 8]);
        assert!(out.iter().all(|&value| value == 0.0 || value == 1.0));
    }

    #[test]
    fn extreme_pitch_never_leaves_the_phase_stuck() {
        // #121: pitch 20 overflows `to_hz` to infinity. Then NaN, then a
        // normal pitch: the phase must stay in range and move again.
        let mut module = Phase::new(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 16,
        });
        let pitch = [20.0, 20.0, f32::INFINITY, f32::NAN, 0.0, 0.0, 0.0, 0.0];
        let reset = [0.0; 8];
        let mut out = [0.0; 8];
        run(
            &mut module,
            48_000.0,
            &[&pitch, &reset],
            &[true, true],
            &mut [&mut out],
            &[],
        );
        assert!(out.iter().all(|p| (0.0..1.0).contains(p)), "{out:?}");
        // Middle C after the extremes: the phase moves by 261.6 / 48000.
        let step = out[7] - out[6];
        assert!((step - pitch::to_hz(0.0) / 48_000.0).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn phase_trigger_keeps_firing_at_the_highest_pitch() {
        // Pitch 20 runs Phase at its largest step. Every cycle must still
        // end in a jump that Phase Trigger sees.
        let mut generator = Phase::new(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 100,
        });
        let pitch = [20.0; 100];
        let reset = [0.0; 100];
        let mut phase = [0.0; 100];
        run(
            &mut generator,
            48_000.0,
            &[&pitch, &reset],
            &[true, true],
            &mut [&mut phase],
            &[],
        );
        let fired = phase_trigger(&phase).iter().filter(|&&t| t == 1.0).count();
        // 100 frames at 0.49 cycles per frame: frame 0, then 48 wraps.
        assert_eq!(fired, 49, "{phase:?}");
    }

    #[test]
    fn unconnected_input_fires_once_then_stays_low() {
        // An unconnected input reads zeros: a phase standing still at 0.
        assert_eq!(phase_trigger(&[0.0; 4]), [1.0, 0.0, 0.0, 0.0]);
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::*;

    /// Wrap and fold give the values in the spec for the edges and for
    /// negative input, and 0 for NaN and infinity.
    #[test]
    fn wrap_and_fold_values() {
        let inputs = [-0.25, 0.0, 0.25, 1.0, 1.25, 2.0];
        let wrapped = [0.75, 0.0, 0.25, 0.0, 0.25, 0.0];
        let folded = [0.25, 0.0, 0.25, 1.0, 0.75, 0.0];
        for ((x, w), f) in inputs.iter().zip(wrapped).zip(folded) {
            assert_eq!(Wrap::wrap(*x), w, "wrap {x}");
            assert_eq!(Wrap::fold(*x), f, "fold {x}");
        }
        // A tiny negative number must not give exactly 1.
        assert!(Wrap::wrap(-1e-9) < 1.0);
    }

    /// NaN and infinity give 0 in both modes, through the module.
    #[test]
    fn non_finite_input_gives_zero() {
        use loom_core::testing::run;
        let input = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.25];
        for fold in [0.0, 1.0] {
            let ModuleRole::Dsp(create) = WRAP.role else {
                unreachable!()
            };
            let mut m = create(&ModuleConfig {
                sample_rate: 48_000.0,
                max_block: 4,
            });
            m.set_param(0, fold);
            let mut out = [9.0; 4];
            run(m.as_mut(), 48_000.0, &[&input], &[true], &mut [&mut out], &[]);
            assert_eq!(out[..3], [0.0, 0.0, 0.0]);
            assert!(out[3] > 0.0 && out[3] <= 1.0);
        }
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;
    use loom_core::signal::WrapDetector;
    use loom_core::testing::run;

    /// Runs Phase Scale over a phase that wraps every 100 frames, for
    /// `cycles` input cycles, in blocks of `block` frames.
    fn scaled(multiply: f32, divide: f32, cycles: usize, block: usize, reset: &[f32]) -> Vec<f32> {
        let ModuleRole::Dsp(create) = SCALE.role else {
            unreachable!()
        };
        let frames = cycles * 100;
        let phase: Vec<f32> = (0..frames).map(|i| (i % 100) as f32 / 100.0).collect();
        let mut reset_in = vec![0.0; frames];
        reset_in[..reset.len()].copy_from_slice(reset);
        let mut m = create(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: block,
        });
        m.set_param(0, multiply);
        m.set_param(1, divide);
        m.reset();
        let mut out = vec![0.0; frames];
        for start in (0..frames).step_by(block) {
            let end = (start + block).min(frames);
            run(
                m.as_mut(),
                48_000.0,
                &[&phase[start..end], &reset_in[start..end]],
                &[true, false],
                &mut [&mut out[start..end]],
                &[],
            );
        }
        out
    }

    /// How many times an output phase wraps.
    fn wraps(phase: &[f32]) -> usize {
        let mut detector = WrapDetector::new();
        // The first frame counts as a wrap when it is 0; skip it.
        phase.iter().skip(1).filter(|&&p| detector.wrapped(p)).count()
    }

    /// Divide 2 wraps once per two input wraps; multiply 3 divide 2 wraps
    /// three times per two input wraps; multiply 4 wraps four times per
    /// cycle. All at block sizes 1, 32, and 1024.
    #[test]
    fn ratios_hold_at_any_block_size() {
        for block in [1, 32, 1024] {
            // Four input cycles, 400 frames. The wrap on the first frame is
            // not counted, and the one that would land on frame 400 is past
            // the end. So divide 2 gives 2 - 1 = 1 wrap, 3 over 2 gives
            // 6 - 1 = 5, and multiply 4 gives 16 - 1 = 15.
            assert_eq!(wraps(&scaled(1.0, 2.0, 4, block, &[])), 1, "divide 2, block {block}");
            assert_eq!(wraps(&scaled(3.0, 2.0, 4, block, &[])), 5, "3 over 2, block {block}");
            assert_eq!(wraps(&scaled(4.0, 1.0, 4, block, &[])), 15, "multiply 4, block {block}");
        }
    }

    /// The output is the same at every block size, and reset starts the
    /// count again.
    #[test]
    fn block_size_does_not_change_the_output_and_reset_restarts() {
        let mut reset = vec![0.0; 150];
        reset[150 - 1] = 1.0;
        let reference = scaled(1.0, 2.0, 4, 1024, &reset);
        assert_eq!(scaled(1.0, 2.0, 4, 1, &reset), reference);
        assert_eq!(scaled(1.0, 2.0, 4, 32, &reset), reference);
        // Without a reset, cycle 1 is the second half of the divided cycle.
        let plain = scaled(1.0, 2.0, 2, 1024, &[]);
        assert!((plain[100] - 0.5).abs() < 1e-6, "{}", plain[100]);
        // A reset on frame 149 sends the count back, so frame 149 reads like
        // the middle of the first of the two cycles.
        assert!((reference[149] - 0.245).abs() < 1e-6, "{}", reference[149]);
    }
}
