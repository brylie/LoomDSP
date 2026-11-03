//! Rhythm patterns as gates and triggers (FR-LIB-08). Spec:
//! `docs/design/p2-modules.md`. Background: `docs/learn/euclidean-rhythms.md`.

use loom_core::signal::{EdgeDetector, WrapDetector, trigger};
use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec,
    ProcessContext, SignalType, Widget,
};
use loom_dsp::euclid::Pattern;
use loom_dsp::phase::wrap;

/// A gate input of Euclidean rhythm.
const fn gate_input(name: &'static str, label: &'static str, description: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Gate,
        description,
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Downmix,
    }
}

/// A gate output of Euclidean rhythm.
const fn gate_output(name: &'static str, label: &'static str, description: &'static str) -> PortSpec {
    PortSpec {
        name,
        label,
        signal: SignalType::Gate,
        description,
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// `loom.gate.euclid` (MT-GATE-02): spreads a number of hits as evenly as
/// possible over a number of steps, with rotation.
pub static EUCLID: ModuleDescriptor = ModuleDescriptor {
    id: "loom.gate.euclid",
    version: 1,
    name: "Euclidean Rhythm",
    category: "Gate",
    family: loom_core::Family::TimeAndRhythm,
    description: "Spreads a number of hits as evenly as possible over a number of steps, driven by a phase or a clock.",
    inputs: &[
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "One cycle plays the whole pattern, in Phase mode. With no cable, it follows the bar.",
            default: PortDefault::BarPhase,
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
        gate_input("clock", "Clock", "Each rising edge moves one step, in Clock mode."),
        gate_input(
            "reset",
            "Reset",
            "A rising edge makes the next clock play the first step, in Clock mode.",
        ),
    ],
    outputs: &[
        gate_output("trigger", "Trigger", "Fires once on entering each hit."),
        gate_output("gate", "Gate", "High for the whole of each hit step."),
        gate_output("accent", "Accent", "Fires once on the pattern's first hit."),
    ],
    params: &[
        ParamSpec {
            name: "source",
            label: "Source",
            description: "Phase: the step comes from the phase. Clock: each clock moves one step.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Phase", "Clock"]),
        ParamSpec {
            name: "steps",
            label: "Steps",
            description: "How many steps the pattern has.",
            min: 1.0,
            max: 32.0,
            default: 16.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "hits",
            label: "Hits",
            description: "How many of the steps are hits. Above the steps, every step is one.",
            min: 0.0,
            max: 32.0,
            default: 4.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "rotation",
            label: "Rotation",
            description: "How many steps later the pattern plays.",
            min: 0.0,
            max: 31.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Euclid::new())),
    panel: Panel::new(
        9,
        5,
        &[
            Widget::segmented(0, 0, 9, "source"),
            Widget::knob(0, 2, "steps"),
            Widget::knob(3, 2, "hits"),
            Widget::knob(6, 2, "rotation"),
        ],
    ),
    displays: &[],
};

/// A whole-number parameter's value as a count.
fn count(value: f32) -> u32 {
    value.round().max(0.0) as u32
}

/// State for `loom.gate.euclid`.
#[derive(Clone)]
struct Euclid {
    source: bool,
    steps: u32,
    hits: u32,
    rotation: u32,
    /// The step playing now. `None` before a step is entered: in Clock mode
    /// that is the wait for the first clock, and in Phase mode it lasts until
    /// the first frame. Keeping it in one value means the step number and the
    /// wait cannot disagree.
    step: Option<u32>,
    /// The steps when the current step was entered. It says when that step
    /// ends, so turning a knob never cuts it in half.
    entered_steps: u32,
    /// The gate's level, held for the whole step.
    gate: bool,
    clock: EdgeDetector,
    reset: EdgeDetector,
    wrap: WrapDetector,
}

impl Euclid {
    const SOURCE: usize = EUCLID.param_index("source").unwrap();
    const STEPS: usize = EUCLID.param_index("steps").unwrap();
    const HITS: usize = EUCLID.param_index("hits").unwrap();
    const ROTATION: usize = EUCLID.param_index("rotation").unwrap();

    /// A module with every parameter at its default, before its first step.
    fn new() -> Self {
        Self {
            source: false,
            steps: 16,
            hits: 4,
            rotation: 0,
            step: None,
            entered_steps: 16,
            gate: false,
            clock: EdgeDetector::new(),
            reset: EdgeDetector::new(),
            wrap: WrapDetector::new(),
        }
    }

    /// The step a phase points at, with `steps` steps.
    fn step_at(phase: f32, steps: u32) -> u32 {
        ((phase * steps as f32) as u32).min(steps - 1)
    }
}

impl Module for Euclid {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let source = io.param(Self::SOURCE);
        let steps = io.param(Self::STEPS);
        let hits = io.param(Self::HITS);
        let rotation = io.param(Self::ROTATION);
        let ([phase, clock, reset], [trigger_out, gate_out, accent_out]) = io.split::<3, 3>();
        for i in 0..phase.len() {
            let clocked = source.map_or(i, self.source, |v| v >= 0.5);
            let steps = steps.map_or(i, self.steps, count).clamp(1, 32);
            // The detectors see every frame, so a change of source is no event.
            let clock_rose = self.clock.rising(clock[i]);
            let reset_rose = self.reset.rising(reset[i]);
            // A bad phase holds the step, and must not reach the wrap detector.
            let wrapped_phase = phase[i].is_finite().then(|| wrap(phase[i]));
            let wrapped = wrapped_phase.is_some_and(|phase| self.wrap.wrapped(phase));

            let enter = if clocked {
                if reset_rose {
                    self.step = None;
                    self.gate = false;
                }
                clock_rose.then(|| match self.step {
                    Some(step) if step + 1 < steps => step + 1,
                    _ => 0,
                })
            } else {
                // The current step is judged with the steps it was entered
                // with, so a turn of the knob waits for the next step.
                wrapped_phase.and_then(|phase| match self.step {
                    Some(step) if !wrapped && Self::step_at(phase, self.entered_steps) == step => None,
                    _ => Some(Self::step_at(phase, steps)),
                })
            };

            let (mut fired, mut accent) = (false, false);
            if let Some(step) = enter {
                let pattern = Pattern::new(
                    steps,
                    hits.map_or(i, self.hits, count),
                    rotation.map_or(i, self.rotation, count),
                );
                self.entered_steps = steps;
                self.step = Some(step);
                // The pattern is read only here, and the gate holds the result,
                // so a knob changes the next step and not this one.
                self.gate = pattern.is_hit(step);
                fired = self.gate;
                accent = pattern.is_first_hit(step);
            }
            trigger_out[i] = trigger(fired);
            gate_out[i] = trigger(self.gate);
            accent_out[i] = trigger(accent);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::SOURCE => self.source = value >= 0.5,
            Self::STEPS => self.steps = count(value),
            Self::HITS => self.hits = count(value),
            Self::ROTATION => self.rotation = count(value),
            _ => {}
        }
    }

    /// Back to before the first step.
    fn reset(&mut self) {
        self.step = None;
        self.gate = false;
        self.clock.reset();
        self.reset.reset();
        self.wrap.reset();
    }
}

#[cfg(test)]
mod tests;
