//! Structure: the modules that give a sub-patch its ports (MT-STRUCT-02).
//! Spec: `docs/design/sub-patches.md`.
//!
//! Inside a sub-patch definition, a Patch input becomes one input port of
//! the sub-patch, and a Patch output one output port, each named by the
//! module's label. Flattening removes them and runs the cables straight
//! through (`loom-patch`, Flattening rule 2), so they never reach the
//! engine from inside a sub-patch. At the top level of a patch, such as a
//! library file opened on its own, they are ordinary modules: the input
//! outputs silence, and the output reads its input and does nothing.
//!
//! The swap fade ([`FADE`]) is the one module here that plays inside a
//! sub-patch: the editor adds one per role output when a slot swaps its
//! part, so the leaving and the new part both run for a 10 ms equal-power
//! crossfade (`docs/design/sub-patches.md`, How a swap crossfades). It is
//! registered as internal: never in the browser, never in a file.

use loom_core::{
    Channels, DisplayKind, DisplayRange, DisplaySpec, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions,
    ParamSpec, PortDefault, PortIo, PortSpec, ProcessContext, SignalType,
};
use std::f32::consts::FRAC_PI_2;

pub mod modes;

/// `loom.struct.input` (MT-STRUCT-02): an input port of a sub-patch.
pub static INPUT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.struct.input",
    version: 1,
    name: "Patch Input",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "An input port of a sub-patch. Inside a sub-patch, cables into the port continue from here; at the top level it outputs silence.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The signal that arrives at the sub-patch's input port.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(PatchInput)),
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// `loom.struct.output` (MT-STRUCT-02): an output port of a sub-patch.
pub static OUTPUT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.struct.output",
    version: 1,
    name: "Patch Output",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "An output port of a sub-patch. Inside a sub-patch, the cable into it continues out of the port; at the top level it does nothing.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal that leaves through the sub-patch's output port.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(PatchOutput)),
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// How long a slot swap crossfades, in seconds (sub-patches.md, Slots).
pub const SWAP_FADE_SECONDS: f32 = 0.010;

/// The kind of the swap fade, as the editor adds it.
pub const FADE_KIND: &str = "loom.struct.fade@1";

/// A port of the swap fade.
const fn fade_port(name: &'static str, label: &'static str, description: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Audio,
        description,
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// `loom.struct.fade`: the crossfade of a slot swap. Internal.
pub static FADE: ModuleDescriptor = ModuleDescriptor {
    id: loom_core::cable_fade::SWAP_FADE_MODULE,
    version: 1,
    name: "Swap Fade",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "Fades from the part leaving a slot to the part taking its place over 10 ms, equal power or linear. The editor adds and removes it.",
    inputs: &[
        fade_port("a", "A", "The output of the part that is leaving."),
        fade_port("b", "B", "The same output of the part that takes its place."),
    ],
    outputs: &[fade_port(
        "out",
        "Out",
        "A, then a blend, then B alone once the fade is done.",
    )],
    params: &[ParamSpec {
        name: "law",
        label: "Law",
        description: "Equal power for audio, linear for pitch and modulation and for a slot that passes its input through. The swap sets it.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Equal power", "Linear"])],
    role: ModuleRole::Dsp(|config| {
        Box::new(SwapFade {
            length: (SWAP_FADE_SECONDS * config.sample_rate).round().max(1.0) as u32,
            count: 0,
            linear: false,
        })
    }),
    panel: Panel::EMPTY,
    displays: &[DisplaySpec {
        name: "done",
        label: "Done",
        description: "1 once the fade has finished and the output is B alone.",
        kind: DisplayKind::Level,
        range: DisplayRange::UNIT,
    }],
    roles: &[],
};

/// The swap fade: counts samples from its first processed frame to the
/// fade length. Under Equal power its gains sit on a quarter circle, so the
/// sum of their squares stays 1 and the loudness of two different sounds
/// holds. Under Linear they sum to 1, so a value both sides share stays put
/// (sub-patches.md, How a swap crossfades, point 6). Per sample, so it is
/// the same at every block size.
#[derive(Clone)]
struct SwapFade {
    /// Frames the fade takes.
    length: u32,
    /// Frames processed so far, up to `length`.
    count: u32,
    /// True for the Linear law.
    linear: bool,
}

impl SwapFade {
    /// The index of the `done` display value.
    pub const DONE: usize = 0;
    /// The index of the `law` parameter.
    const LAW: usize = FADE.param_index("law").unwrap();

    /// True once the fade has finished.
    fn is_done(&self) -> bool {
        self.count >= self.length
    }
}

impl Module for SwapFade {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([a, b], [out]) = io.split::<2, 1>();
        for i in 0..out.len() {
            if self.is_done() {
                out[i] = b[i];
                continue;
            }
            let (gain_a, gain_b) = if self.linear {
                let p = self.count as f32 / self.length as f32;
                (1.0 - p, p)
            } else {
                // Kept as written before the Linear law, so equal-power
                // fades stay bit-identical to their references.
                let theta = FRAC_PI_2 * self.count as f32 / self.length as f32;
                let (sin, cos) = theta.sin_cos();
                (cos, sin)
            };
            out[i] = a[i] * gain_a + b[i] * gain_b;
            self.count += 1;
        }
    }

    fn displays(&self, values: &mut [f32]) {
        values[Self::DONE] = if self.is_done() { 1.0 } else { 0.0 };
    }

    /// Takes the law: 1 is Linear, and any other value, NaN included,
    /// Equal power, so a value from a hand-edited file always picks one of
    /// the two.
    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::LAW {
            self.linear = value.round() as i32 == 1;
        }
    }

    fn reset(&mut self) {
        self.count = 0;
    }

    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }
}

/// A Patch input at the top level: silence.
struct PatchInput;

impl Module for PatchInput {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([], [out]) = io.split::<0, 1>();
        out.fill(0.0);
    }
}

/// A Patch output at the top level: a sink.
struct PatchOutput;

impl Module for PatchOutput {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([_], []) = io.split::<1, 0>();
    }
}

#[cfg(test)]
mod tests;
