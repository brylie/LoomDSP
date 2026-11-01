//! Envelopes (FR-LIB-04, MT-ENV-01).
//!
//! Spec: `docs/design/p1-modules.md`, Envelope AD mode.

use loom_core::signal::gate_is_high;
use loom_core::{
    Channels, DisplayKind, DisplaySpec, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions,
    ParamSpec, PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};

/// `loom.env.envelope`: an envelope with an ADSR mode that follows a gate
/// and an AD mode that runs once from a gate or a trigger.
pub static ENVELOPE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.env.envelope",
    version: 1,
    name: "Envelope",
    category: "Envelope",
    family: loom_core::Family::Modulation,
    description: "A level from 0 to 1 that rises and falls when a gate starts it. ADSR holds while the gate is high; AD runs once, so a trigger also starts it.",
    inputs: &[PortSpec {
        name: "gate",
        label: "Gate",
        signal: SignalType::Gate,
        description: "Going high starts the envelope. In ADSR mode, going low releases it.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The envelope level, from 0 to 1.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Span(0.0, 1.0),
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "attack",
            label: "Attack",
            description: "Time to rise to 1 after the gate goes high.",
            min: 0.0005,
            max: 10.0,
            default: 0.01,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic(),
        ParamSpec {
            name: "decay",
            label: "Decay",
            description: "Time to fall from 1 to the sustain level, or to 0 in AD mode.",
            min: 0.0005,
            max: 10.0,
            default: 0.2,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic(),
        ParamSpec {
            name: "sustain",
            label: "Sustain",
            description: "The level held while the gate stays high. AD mode ignores it.",
            min: 0.0,
            max: 1.0,
            default: 0.7,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
        ParamSpec {
            name: "release",
            label: "Release",
            description: "Time to fall to 0 after the gate goes low. AD mode ignores it.",
            min: 0.0005,
            max: 10.0,
            default: 0.3,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic(),
        ParamSpec {
            name: "mode",
            label: "Mode",
            description: "ADSR follows the gate. AD rises and falls once, whatever the gate does next.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["ADSR", "AD"]),
        ParamSpec {
            name: "curve",
            label: "Curve",
            description: "Bends every stage. 0 is straight, above 0 starts fast with a slow tail, below 0 starts slow.",
            min: -1.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|config| Box::new(Envelope::new(config))),
    panel: Panel::new(
        15,
        10,
        &[
            Widget::select(0, 0, "mode"),
            Widget::light(12, 0, "level"),
            Widget::shape(0, 2, 15, "out", "Shape", envelope_preview),
            Widget::knob(0, 7, "attack"),
            Widget::knob(3, 7, "decay"),
            Widget::knob(6, 7, "sustain"),
            Widget::knob(9, 7, "release"),
            Widget::knob(12, 7, "curve"),
        ],
    ),
    displays: &[DisplaySpec {
        name: "level",
        label: "Level",
        description: "The envelope's level, from dark at 0 to full at 1.",
        kind: DisplayKind::Level,
    }],
};

/// The envelope's schematic result for the tile preview. Each timed stage
/// has a minimum schematic width so a short attack remains visible at tile size.
fn envelope_preview(params: &[f32], x: f32) -> f32 {
    if x <= 0.0 || x >= 1.0 {
        return 0.0;
    }
    let &[attack, decay, sustain, release, mode, curve] = params else {
        return 0.0;
    };
    let ad = mode >= 0.5;
    let timed_total = attack + decay + release;
    let hold = if ad { 0.0 } else { timed_total * 0.2 };
    let durations = if ad {
        [attack, decay, 0.0, 0.0]
    } else {
        [attack, decay, hold, release]
    };
    let minimum = (timed_total * 0.12).max(0.03);
    let weights = durations.map(|time| if time > 0.0 { time.max(minimum) } else { 0.0 });
    let total: f32 = weights.iter().sum();
    let mut at = x.clamp(0.0, 1.0) * total;
    let bend = power(curve);
    if at <= weights[0] {
        return 1.0 - fall(at / weights[0], bend);
    }
    at -= weights[0];
    let target = if ad { 0.0 } else { sustain };
    if at <= weights[1] {
        return target + (1.0 - target) * fall(at / weights[1], bend);
    }
    at -= weights[1];
    if at <= weights[2] {
        return target;
    }
    at -= weights[2];
    target * fall(at / weights[3].max(f32::EPSILON), bend)
}

#[cfg(test)]
mod preview_tests {
    use super::envelope_preview;

    /// The preview follows mode and sustain without changing the engine.
    #[test]
    fn mode_and_sustain_shape() {
        let adsr = [0.1, 0.1, 0.6, 0.1, 0.0, 0.0];
        let ad = [0.1, 0.1, 0.6, 0.1, 1.0, 0.0];
        assert_eq!(envelope_preview(&adsr, 0.0), 0.0);
        assert_eq!(envelope_preview(&adsr, 1.0), 0.0);
        assert!((envelope_preview(&adsr, 0.7) - 0.6).abs() < 0.01);
        assert_eq!(envelope_preview(&ad, 1.0), 0.0);
        assert_ne!(envelope_preview(&adsr, 0.5), envelope_preview(&ad, 0.5));
    }
}

/// Where the envelope is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    /// At 0, waiting for a rising gate.
    Idle,
    /// Rising from the level it started at to 1.
    Attack,
    /// Falling from 1 to the sustain level (ADSR) or to 0 (AD).
    Decay,
    /// Holding the sustain level while the gate is high (ADSR only).
    Sustain,
    /// Falling from the level it started at to 0 (ADSR only).
    Release,
}

/// State for `loom.env.envelope`.
#[derive(Clone)]
struct Envelope {
    sample_rate: f64,
    stage: Stage,
    /// The output level.
    level: f32,
    /// The level when the current stage started.
    start: f32,
    /// Progress through the current stage, from 0 to 1. Kept in `f64`, so
    /// a 10 s stage at a high sample rate still ends on time.
    progress: f64,
    gate_was_high: bool,
    /// Whether the current decay falls to 0 (AD) rather than to the
    /// sustain level, so a mode change mid-decay can restart it.
    decay_ad: bool,
    /// The knob values. Modulated values live only for one frame, so these
    /// keep the base and removing an assignment returns to it.
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    ad: bool,
    curve: f32,
}

impl Envelope {
    /// An idle envelope with the default parameters.
    fn new(config: &ModuleConfig) -> Self {
        Self {
            sample_rate: f64::from(config.sample_rate),
            stage: Stage::Idle,
            level: 0.0,
            start: 0.0,
            progress: 0.0,
            gate_was_high: false,
            decay_ad: false,
            attack: 0.01,
            decay: 0.2,
            sustain: 0.7,
            release: 0.3,
            ad: false,
            curve: 0.0,
        }
    }

    /// Starts `stage` from the current level.
    fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        self.start = self.level;
        self.progress = 0.0;
    }

    /// Moves the progress on by one frame of a stage that lasts `seconds`,
    /// and returns the new progress, at most 1.
    fn advance(&mut self, seconds: f32) -> f32 {
        let frames = (f64::from(seconds) * self.sample_rate).max(1.0);
        self.progress = (self.progress + 1.0 / frames).min(1.0);
        self.progress as f32
    }
}

/// The shaping power for `curve` from -1 to 1: `2^(3 × curve)`, from 1/8
/// to 8.
fn power(curve: f32) -> f32 {
    (3.0 * curve.clamp(-1.0, 1.0)).exp2()
}

/// How far a falling stage has gone: `(1 - x)^k`, from 1 at `x` = 0 to 0
/// at `x` = 1. A rising stage uses `1 - fall(x, k)`.
fn fall(x: f32, k: f32) -> f32 {
    let rest = (1.0 - x).max(0.0);
    // Skip the `powf` for the straight line, the default.
    if k == 1.0 { rest } else { rest.powf(k) }
}

impl Module for Envelope {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let attack = io.param(0);
        let decay = io.param(1);
        let sustain = io.param(2);
        let release = io.param(3);
        let mode = io.param(4);
        let curve = io.param(5);
        let ([gate], [out]) = io.split::<1, 1>();
        for (i, (out, &g)) in out.iter_mut().zip(gate).enumerate() {
            let attack = attack.at_or(i, self.attack);
            let decay = decay.at_or(i, self.decay);
            let sustain = sustain.at_or(i, self.sustain).clamp(0.0, 1.0);
            let release = release.at_or(i, self.release);
            let ad = mode.map_or(i, self.ad, |v| v >= 0.5);
            let k = power(curve.at_or(i, self.curve));

            let high = gate_is_high(g);
            if high && !self.gate_was_high {
                // Retrigger from the current level: no click. The attack
                // still takes the full attack time.
                self.enter(Stage::Attack);
            } else if !high && self.gate_was_high && !ad && self.stage != Stage::Idle {
                self.enter(Stage::Release);
            }
            self.gate_was_high = high;
            // AD has no sustain. A switch to AD while holding falls to 0.
            if ad && self.stage == Stage::Sustain {
                self.enter(Stage::Decay);
                self.decay_ad = true;
            }
            // A mode change mid-decay moves the decay's end. Restart the
            // stage from the current level, so the output does not jump.
            // With the gate already low, ADSR goes on to the release.
            if self.stage == Stage::Decay && self.decay_ad != ad {
                self.enter(if ad || high { Stage::Decay } else { Stage::Release });
                self.decay_ad = ad;
            }

            match self.stage {
                Stage::Idle => self.level = 0.0,
                Stage::Attack => {
                    let x = self.advance(attack);
                    self.level = self.start + (1.0 - self.start) * (1.0 - fall(x, k));
                    if x >= 1.0 {
                        self.level = 1.0;
                        self.enter(Stage::Decay);
                        self.decay_ad = ad;
                    }
                }
                Stage::Decay => {
                    let x = self.advance(decay);
                    let end = if ad { 0.0 } else { sustain };
                    self.level = end + (self.start - end) * fall(x, k);
                    if x >= 1.0 {
                        self.level = end;
                        if ad {
                            self.stage = Stage::Idle;
                        } else if high {
                            self.stage = Stage::Sustain;
                        } else {
                            // The gate fell while the mode was AD, so no
                            // falling edge will come: release from here.
                            self.enter(Stage::Release);
                        }
                    }
                }
                Stage::Sustain => self.level = sustain,
                Stage::Release => {
                    let x = self.advance(release);
                    self.level = self.start * fall(x, k);
                    if x >= 1.0 {
                        self.level = 0.0;
                        self.stage = Stage::Idle;
                    }
                }
            }
            *out = self.level;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            0 => self.attack = value,
            1 => self.decay = value,
            2 => self.sustain = value,
            3 => self.release = value,
            4 => self.ad = value >= 0.5,
            5 => self.curve = value,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
        self.start = 0.0;
        self.progress = 0.0;
        self.gate_was_high = false;
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.level;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run;

    /// Sample rate for these tests: 1 kHz, so one frame is 1 ms.
    const RATE: f32 = 1_000.0;

    /// An envelope at 1 kHz with its defaults and then `params` set.
    fn make(params: &[(&str, f32)]) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = ENVELOPE.role else {
            unreachable!()
        };
        let mut env = create(&ModuleConfig {
            sample_rate: RATE,
            max_block: 4_000,
        });
        for (i, p) in ENVELOPE.params.iter().enumerate() {
            env.set_param(i, p.default);
        }
        for &(name, value) in params {
            let index = ENVELOPE.params.iter().position(|p| p.name == name).unwrap();
            env.set_param(index, value);
        }
        env.reset();
        env
    }

    /// Runs `env` over `gate` and returns its output.
    fn play(env: &mut dyn Module, gate: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; gate.len()];
        run(env, RATE, &[gate], &[true], &mut [&mut out], &[]);
        out
    }

    #[test]
    fn follows_gate() {
        let mut env = make(&[]);
        let mut gate = vec![1.0; 2_000];
        gate[1_000..].fill(0.0);
        let out = play(env.as_mut(), &gate);
        assert!((out[999] - 0.7).abs() < 1e-6, "sustain level, got {}", out[999]);
        assert_eq!(out[1_999], 0.0, "released");
        assert!(out[5] > 0.0 && out[5] < 1.0, "attack ramps");
    }

    /// In ADSR mode with curve 0, every stage is a straight line that takes
    /// its knob's time: 10 frames up, 20 down to sustain, 30 down to 0.
    #[test]
    fn adsr_stages_take_their_times() {
        let mut env = make(&[("attack", 0.01), ("decay", 0.02), ("sustain", 0.5), ("release", 0.03)]);
        let mut gate = vec![1.0; 100];
        gate[50..].fill(0.0);
        let out = play(env.as_mut(), &gate);
        // Frame 0 is the first step of the attack.
        assert!((out[0] - 0.1).abs() < 1e-6, "{}", out[0]);
        assert!((out[4] - 0.5).abs() < 1e-6, "{}", out[4]);
        assert_eq!(out[9], 1.0, "the attack ends after 10 frames");
        assert!((out[19] - 0.75).abs() < 1e-6, "half way down: {}", out[19]);
        assert_eq!(out[29], 0.5, "the decay ends after 20 frames");
        assert_eq!(out[49], 0.5, "sustain");
        assert!((out[64] - 0.25).abs() < 1e-6, "half way released: {}", out[64]);
        assert_eq!(out[79], 0.0, "the release ends after 30 frames");
    }

    /// AD mode measures its attack and decay times, ignores the gate going
    /// low, and runs from a one-frame trigger.
    #[test]
    fn ad_mode_runs_once_from_a_trigger() {
        let mut env = make(&[("mode", 1.0), ("attack", 0.01), ("decay", 0.04), ("sustain", 0.8)]);
        let mut gate = vec![0.0; 100];
        gate[0] = 1.0;
        let out = play(env.as_mut(), &gate);
        assert_eq!(out[9], 1.0, "the attack ends after 10 frames");
        assert!((out[29] - 0.5).abs() < 1e-6, "half way down: {}", out[29]);
        assert_eq!(out[49], 0.0, "the decay ends at 0, not at sustain");
        assert!(out[50..].iter().all(|&v| v == 0.0));

        // A held gate changes nothing: the decay still falls to 0.
        let mut env = make(&[("mode", 1.0), ("attack", 0.01), ("decay", 0.04)]);
        let out = play(env.as_mut(), &[1.0; 100]);
        assert_eq!(out[49], 0.0);
        assert_eq!(out[99], 0.0);
    }

    /// A rising gate in the attack or in the decay starts a new attack from
    /// the current level, so the output never jumps. The new attack takes
    /// the full attack time.
    #[test]
    fn retrigger_starts_from_the_current_level() {
        for mode in [0.0, 1.0] {
            let mut env = make(&[("mode", mode), ("attack", 0.02), ("decay", 0.04), ("curve", 0.5)]);
            let mut gate = vec![0.0; 200];
            // In the attack at frame 10, and in the decay at frame 60.
            gate[0..5].fill(1.0);
            gate[10..15].fill(1.0);
            gate[60..65].fill(1.0);
            if mode == 0.0 {
                // ADSR: keep the gate high so the low edges do not release.
                gate[0..100].fill(1.0);
                gate[9] = 0.0;
                gate[59] = 0.0;
            }
            let out = play(env.as_mut(), &gate);
            let biggest = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
            // A full attack of 20 frames moves at most about 1/20 per
            // frame at the start of a fast curve (k = 2.83).
            assert!(biggest < 0.2, "mode {mode}: step {biggest}");
            assert_eq!(out[79], 1.0, "mode {mode}: the retriggered attack takes 20 frames");
            assert!(out[78] < 1.0, "mode {mode}");
        }
    }

    /// Curve 0 is a straight line. Positive curve rises fast and falls fast
    /// at first; negative curve starts slowly.
    #[test]
    fn curve_bends_the_stages() {
        let half = |curve: f32| {
            let mut env = make(&[("mode", 1.0), ("attack", 0.01), ("decay", 0.01), ("curve", curve)]);
            let mut gate = vec![0.0; 30];
            gate[0] = 1.0;
            let out = play(env.as_mut(), &gate);
            (out[4], out[14])
        };
        let (rise, fall) = half(0.0);
        assert!((rise - 0.5).abs() < 1e-6 && (fall - 0.5).abs() < 1e-6);
        let (rise, fall) = half(1.0);
        // k = 8: 1 - 0.5^8 and 0.5^8.
        assert!((rise - (1.0 - 0.5f32.powi(8))).abs() < 1e-5, "{rise}");
        assert!((fall - 0.5f32.powi(8)).abs() < 1e-5, "{fall}");
        let (rise, fall) = half(-1.0);
        assert!(rise < 0.2 && fall > 0.8, "{rise} {fall}");
    }

    /// Switching to AD while the ADSR holds its sustain falls to 0 over
    /// the decay time, without a jump.
    #[test]
    fn switching_to_ad_while_holding_decays() {
        let mut env = make(&[("attack", 0.01), ("decay", 0.01), ("sustain", 0.6)]);
        let out = play(env.as_mut(), &[1.0; 50]);
        assert_eq!(out[49], 0.6);
        env.set_param(4, 1.0);
        let out = play(env.as_mut(), &[1.0; 20]);
        assert!((out[0] - 0.54).abs() < 1e-6, "{}", out[0]);
        assert_eq!(out[9], 0.0);
    }

    /// Switching the mode partway through a decay restarts it from the
    /// current level, so the output does not jump (#411 review). With the
    /// gate high, ADSR falls on to the sustain level; with it low, ADSR
    /// releases to 0.
    #[test]
    fn switching_mode_mid_decay_does_not_jump() {
        for (gate, after) in [(1.0, 0.5), (0.0, 0.0)] {
            let mut env = make(&[
                ("mode", 1.0),
                ("attack", 0.01),
                ("decay", 0.02),
                ("sustain", 0.5),
                ("release", 0.02),
            ]);
            let mut gates = [gate; 15];
            gates[0] = 1.0;
            let before = play(env.as_mut(), &gates);
            env.set_param(4, 0.0);
            let out = play(env.as_mut(), &[gate; 100]);
            let last = before[14];
            assert!(last > 0.5 && last < 1.0, "mid-decay, got {last}");
            assert!((out[0] - last).abs() < 0.06, "jump from {last} to {}", out[0]);
            let steps = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
            assert!(steps < 0.06, "largest step {steps}");
            assert_eq!(out[99], after, "gate {gate}");
        }
        // From ADSR to AD mid-decay, the fall goes on to 0 without a jump.
        let mut env = make(&[("attack", 0.01), ("decay", 0.02), ("sustain", 0.5)]);
        let before = play(env.as_mut(), &[1.0; 15]);
        env.set_param(4, 1.0);
        let out = play(env.as_mut(), &[1.0; 100]);
        assert!(
            (out[0] - before[14]).abs() < 0.06,
            "jump from {} to {}",
            before[14],
            out[0]
        );
        assert_eq!(out[99], 0.0);
    }

    /// A trigger in AD mode, then a switch to ADSR during the attack, does
    /// not hold the sustain with the gate low: the decay ends in the
    /// release (#411 review).
    #[test]
    fn switching_to_adsr_after_a_trigger_releases() {
        let mut env = make(&[("mode", 1.0), ("attack", 0.01), ("decay", 0.01), ("release", 0.01)]);
        let mut gates = [0.0; 5];
        gates[0] = 1.0;
        play(env.as_mut(), &gates);
        env.set_param(4, 0.0);
        let out = play(env.as_mut(), &[0.0; 100]);
        assert_eq!(out[99], 0.0, "no stuck sustain");
    }

    /// The `level` display value is the output's last sample after the
    /// attack, and 0 once the release has ended (#106).
    #[test]
    fn display_follows_the_level() {
        let mut env = make(&[]);
        let mut display = [f32::NAN];

        // The default attack is 10 ms, 10 frames at 1 kHz.
        let out = play(env.as_mut(), &[1.0; 50]);
        env.displays(&mut display);
        assert!(out[49] > 0.0, "past the attack, got {}", out[49]);
        assert_eq!(display[0], out[49]);

        let out = play(env.as_mut(), &[0.0; 2_000]);
        env.displays(&mut display);
        assert_eq!(out[1_999], 0.0, "released");
        assert_eq!(display[0], 0.0);
    }
}
