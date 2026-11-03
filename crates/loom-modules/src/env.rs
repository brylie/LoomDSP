//! Envelopes (FR-LIB-04, MT-ENV-01).
//!
//! Spec: `docs/design/p1-modules.md`, Envelope AD mode.

use loom_core::signal::gate_is_high;
use loom_core::{
    Channels, DisplayKind, DisplayRange, DisplaySpec, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel,
    ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec, ProcessContext, SignalType, Widget,
};
use loom_dsp::envelope::{self, Settings, fall, power};

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
        range: DisplayRange::UNIT,
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

/// State for `loom.env.envelope`.
#[derive(Clone)]
struct Envelope {
    envelope: envelope::Envelope,
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
            envelope: envelope::Envelope::new(config.sample_rate),
            attack: 0.01,
            decay: 0.2,
            sustain: 0.7,
            release: 0.3,
            ad: false,
            curve: 0.0,
        }
    }
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

            *out = self.envelope.tick(
                gate_is_high(g),
                Settings {
                    attack,
                    decay,
                    sustain,
                    release,
                    ad,
                    power: k,
                },
            );
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
        self.envelope.reset();
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.envelope.level();
    }
}

#[cfg(test)]
mod tests;
