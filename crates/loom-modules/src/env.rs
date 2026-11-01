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
        }
        .shown_when("mode", &[0]),
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
        .logarithmic()
        .shown_when("mode", &[0]),
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
            Widget::shape(0, 0, 15, "out", "Shape", envelope_preview),
            Widget::segmented(0, 5, 6, "mode"),
            Widget::light(12, 5, "level"),
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
    if params.len() != ENVELOPE.params.len() {
        return 0.0;
    }
    let value = |index: usize| params[index];
    let (attack, decay, sustain) = (
        value(Envelope::ATTACK),
        value(Envelope::DECAY),
        value(Envelope::SUSTAIN),
    );
    let (release, mode, curve) = (value(Envelope::RELEASE), value(Envelope::MODE), value(Envelope::CURVE));
    let ad = mode >= 0.5;
    // AD never releases, so its stored Release must not change the view.
    let timed_total = attack + decay + if ad { 0.0 } else { release };
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
mod preview_tests;

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
    const ATTACK: usize = ENVELOPE.param_index("attack").unwrap();
    const DECAY: usize = ENVELOPE.param_index("decay").unwrap();
    const SUSTAIN: usize = ENVELOPE.param_index("sustain").unwrap();
    const RELEASE: usize = ENVELOPE.param_index("release").unwrap();
    const MODE: usize = ENVELOPE.param_index("mode").unwrap();
    const CURVE: usize = ENVELOPE.param_index("curve").unwrap();

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
        let attack = io.param(Self::ATTACK);
        let decay = io.param(Self::DECAY);
        let sustain = io.param(Self::SUSTAIN);
        let release = io.param(Self::RELEASE);
        let mode = io.param(Self::MODE);
        let curve = io.param(Self::CURVE);
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
            Self::ATTACK => self.attack = value,
            Self::DECAY => self.decay = value,
            Self::SUSTAIN => self.sustain = value,
            Self::RELEASE => self.release = value,
            Self::MODE => self.ad = value >= 0.5,
            Self::CURVE => self.curve = value,
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
mod tests;
