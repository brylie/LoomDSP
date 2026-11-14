//! The modules the flattener adds for sub-patch modes
//! (`docs/design/sub-patches.md`, The modules the flattener adds): Sum for
//! layer and mono stack outputs, Stereo spread sum for a stack with stereo
//! spread, Member for a select's Member control, and Select for each output
//! of a select. All are internal: never in the browser, never in a file,
//! never on the canvas. Each counts per sample, so it gives the same output
//! at any block size, and holds no buffer. The math is in
//! [`loom_dsp::blend`]; these are thin wrappers that add ports and
//! parameters.

use loom_core::signal::hold_finite;
use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec,
    ProcessContext, SignalType,
};
use loom_dsp::blend::{self, Law, MemberFade};
use loom_rt::Smoothed;

/// The most members or copies a mode runs, and so the inputs of each added
/// module (sub-patches.md, Members are nested instances).
pub const MOST_MEMBERS: usize = 16;

/// How long a gain change of Sum or Stereo spread sum glides, and how long a
/// select fade takes, in seconds.
pub const MODE_FADE_SECONDS: f32 = 0.010;

/// `value`, held at the largest finite value when it overflowed from finite
/// inputs (ADR-0037): a sum of sixteen large inputs is the module's own
/// doing. An infinite or NaN input passes through. The inputs are checked
/// only when the value is not finite, so the usual frame costs nothing.
#[inline]
fn held(value: f32, inputs_finite: impl Fn() -> bool) -> f32 {
    if !value.is_finite() && inputs_finite() {
        hold_finite(value)
    } else {
        value
    }
}

/// An input of one member or copy.
const fn member_input(name: &'static str, label: &'static str, channels: Channels) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Audio,
        description: "The same output of one member or copy.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels,
    }
}

/// The sixteen member inputs, `in1` to `in16`.
macro_rules! member_inputs {
    ($channels:expr $(, $extra:expr)*) => {
        &[
            member_input("in1", "In 1", $channels),
            member_input("in2", "In 2", $channels),
            member_input("in3", "In 3", $channels),
            member_input("in4", "In 4", $channels),
            member_input("in5", "In 5", $channels),
            member_input("in6", "In 6", $channels),
            member_input("in7", "In 7", $channels),
            member_input("in8", "In 8", $channels),
            member_input("in9", "In 9", $channels),
            member_input("in10", "In 10", $channels),
            member_input("in11", "In 11", $channels),
            member_input("in12", "In 12", $channels),
            member_input("in13", "In 13", $channels),
            member_input("in14", "In 14", $channels),
            member_input("in15", "In 15", $channels),
            member_input("in16", "In 16", $channels),
            $($extra,)*
        ]
    };
}

/// A count parameter: 1 to 16, whole numbers.
const fn count_param(name: &'static str, label: &'static str, description: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description,
        min: 1.0,
        max: MOST_MEMBERS as f32,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
}

/// `loom.struct.sum`: the sum of a layer's or a mono stack's outputs.
/// Internal.
pub static SUM: ModuleDescriptor = ModuleDescriptor {
    id: loom_core::cable_fade::SUM_MODULE,
    version: 1,
    name: "Sum",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "Adds the same output of every member or copy of a sub-patch, times a gain. The flattener adds it.",
    inputs: member_inputs!(Channels::PerChannel),
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The sum of the inputs, times the gain.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "gain",
        label: "Gain",
        description: "1 for a layer, 1/√N for a stack of N copies.",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .smooth(MODE_FADE_SECONDS)],
    role: ModuleRole::Dsp(|config| {
        Box::new(Sum {
            gain: Smoothed::new(1.0, config.sample_rate, MODE_FADE_SECONDS),
        })
    }),
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// Sum: the inputs added, times a gliding gain.
#[derive(Clone)]
struct Sum {
    gain: Smoothed,
}

impl Sum {
    const GAIN: usize = SUM.param_index("gain").unwrap();
}

impl Module for Sum {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (inputs, [out]) = io.split::<MOST_MEMBERS, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for input in &inputs {
                sum += input[i];
            }
            *out = held(self.gain.tick() * sum, || {
                inputs.iter().all(|input| input[i].is_finite())
            });
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::GAIN {
            self.gain.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == Self::GAIN { self.gain.gliding() } else { None }
    }

    fn reset(&mut self) {
        let target = self.gain.target();
        self.gain.reset(target);
    }

    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }
}

/// `loom.struct.spread_sum`: the stereo sum of a stack with stereo spread.
/// Internal.
pub static SPREAD_SUM: ModuleDescriptor = ModuleDescriptor {
    id: loom_core::cable_fade::SPREAD_SUM_MODULE,
    version: 1,
    name: "Stereo Spread Sum",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "Pans each copy of a stack by its place among the copies, equal power, and adds them at 1/√N. The flattener adds it.",
    inputs: member_inputs!(Channels::Downmix),
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The copies, spread from left to right.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Stereo,
    }],
    params: &[count_param("copies", "Copies", "How many copies the stack runs.")],
    role: ModuleRole::Dsp(|config| Box::new(SpreadSum::new(config.sample_rate, 1))),
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// Stereo spread sum: one gliding pair of gains per input.
#[derive(Clone)]
struct SpreadSum {
    left: [Smoothed; MOST_MEMBERS],
    right: [Smoothed; MOST_MEMBERS],
}

impl SpreadSum {
    const COPIES: usize = SPREAD_SUM.param_index("copies").unwrap();
    const OUT: usize = SPREAD_SUM.output_index("out").unwrap();

    fn new(sample_rate: f32, copies: u32) -> Self {
        let smoothed = Smoothed::new(0.0, sample_rate, MODE_FADE_SECONDS);
        let mut sum = Self {
            left: [smoothed; MOST_MEMBERS],
            right: [smoothed; MOST_MEMBERS],
        };
        sum.set_copies(copies);
        sum.reset();
        sum
    }

    /// The gains for `copies`: copy `k` panned by its offset, times
    /// 1/√copies; inputs past the last copy are silent.
    fn set_copies(&mut self, copies: u32) {
        let copies = copies.clamp(1, MOST_MEMBERS as u32);
        let gain = blend::unison_gain(copies);
        for k in 0..MOST_MEMBERS {
            let (left, right) = if (k as u32) < copies {
                let (left, right) = blend::pan(blend::spread_offset(k as u32, copies));
                (left * gain, right * gain)
            } else {
                (0.0, 0.0)
            };
            self.left[k].set_target(left);
            self.right[k].set_target(right);
        }
    }
}

impl Module for SpreadSum {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let inputs: [&[f32]; MOST_MEMBERS] = std::array::from_fn(|k| io.stereo_input(k)[0]);
        let [out_left, out_right] = io.stereo_output(Self::OUT);
        for i in 0..out_left.len() {
            let (mut left, mut right) = (0.0, 0.0);
            for (k, input) in inputs.iter().enumerate() {
                left += self.left[k].tick() * input[i];
                right += self.right[k].tick() * input[i];
            }
            let finite = || inputs.iter().all(|input| input[i].is_finite());
            out_left[i] = held(left, finite);
            out_right[i] = held(right, finite);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::COPIES {
            self.set_copies(value.round() as u32);
        }
    }

    fn reset(&mut self) {
        for smoothed in self.left.iter_mut().chain(self.right.iter_mut()) {
            let target = smoothed.target();
            smoothed.reset(target);
        }
    }

    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }
}

/// `loom.struct.member`: the home of a select's Member parameter. Internal.
pub static MEMBER: ModuleDescriptor = ModuleDescriptor {
    id: loom_core::cable_fade::MEMBER_MODULE,
    version: 1,
    name: "Member",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "Holds which member of a select sub-patch plays, and sends it to every output. The flattener adds it.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The Member value, per sample, modulation included.",
        default: PortDefault::Value(1.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[count_param("member", "Member", "Which member plays, from 1.")],
    role: ModuleRole::Dsp(|_| Box::new(MemberValue { member: 1.0 })),
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// Member: outputs its parameter, modulated per sample.
#[derive(Clone)]
struct MemberValue {
    member: f32,
}

impl MemberValue {
    const MEMBER: usize = MEMBER.param_index("member").unwrap();
}

impl Module for MemberValue {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let member = io.param(Self::MEMBER);
        let ([], [out]) = io.split::<0, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            *out = member.at_or(i, self.member);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::MEMBER {
            self.member = value;
        }
    }

    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }
}

/// `loom.struct.select`: one output of a select, crossfading between
/// members. Internal.
pub static SELECT: ModuleDescriptor = ModuleDescriptor {
    id: loom_core::cable_fade::SELECT_MODULE,
    version: 1,
    name: "Select",
    category: "Structure",
    family: loom_core::Family::Interface,
    description: "Passes the same output of the chosen member of a select sub-patch, crossfading over 10 ms when the choice changes. The flattener adds it.",
    inputs: member_inputs!(
        Channels::PerChannel,
        PortSpec {
            name: "member",
            label: "Member",
            signal: SignalType::Modulation,
            description: "Which member plays, from 1, rounded to the nearest whole member.",
            default: PortDefault::Value(1.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        }
    ),
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The chosen member's output, or a blend of two during a fade.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        count_param("members", "Members", "How many members the select has."),
        ParamSpec {
            name: "fade",
            label: "Fade",
            description: "Equal power for audio, linear for pitch and modulation, off for gates, triggers, and phases, which switch on the frame.",
            min: 0.0,
            max: 2.0,
            default: 2.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Off", "Linear", "Equal power"]),
    ],
    role: ModuleRole::Dsp(|config| {
        Box::new(Select {
            fader: MemberFade::new((MODE_FADE_SECONDS * config.sample_rate).round() as u32, 0),
            members: 1,
            fade: Some(Law::EqualPower),
            fresh: true,
        })
    }),
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// Select: the member fade, per sample.
#[derive(Clone)]
struct Select {
    fader: MemberFade,
    /// How many member inputs count, 1 to 16.
    members: usize,
    /// The fade's law: equal power for audio, linear for pitch and
    /// modulation, `None` for gate, trigger, and phase outputs.
    fade: Option<Law>,
    /// True until the first frame, which starts on its member with no fade:
    /// a new Select must not fade in from member 1.
    fresh: bool,
}

impl Select {
    const MEMBERS: usize = SELECT.param_index("members").unwrap();
    const FADE: usize = SELECT.param_index("fade").unwrap();

    /// The member index, from 0, that a Member value picks: the nearest
    /// whole member, held to the ones there are. NaN picks the first.
    fn target(&self, member: f32) -> usize {
        if member.is_nan() {
            return 0;
        }
        (member.round().clamp(1.0, self.members as f32) as usize) - 1
    }
}

impl Module for Select {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let (inputs, [out]) = io.split::<{ MOST_MEMBERS + 1 }, 1>();
        let member = inputs[MOST_MEMBERS];
        for (i, out) in out.iter_mut().enumerate() {
            let target = self.target(member[i]);
            let blend = match self.fade {
                _ if self.fresh => {
                    self.fresh = false;
                    self.fader.cut(target)
                }
                Some(law) => self.fader.tick_with(target, law),
                None => self.fader.cut(target),
            };
            let (from, to) = (inputs[blend.from][i], inputs[blend.to][i]);
            *out = held(from * blend.gain_from + to * blend.gain_to, || {
                from.is_finite() && to.is_finite()
            });
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::MEMBERS => self.members = (value.round() as usize).clamp(1, MOST_MEMBERS),
            Self::FADE => {
                self.fade = match value.round() as i32 {
                    0 => None,
                    1 => Some(Law::Linear),
                    _ => Some(Law::EqualPower),
                }
            }
            _ => {}
        }
    }

    /// Back to a new Select: the next frame starts on its member, with no
    /// fade.
    fn reset(&mut self) {
        let member = self.fader.member().min(self.members - 1);
        self.fader.reset(member);
        self.fresh = true;
    }

    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }
}

#[cfg(test)]
mod tests;
