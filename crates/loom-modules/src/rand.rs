//! Random and stepped sources (FR-LIB-01).

use loom_core::signal::{EdgeDetector, gate_is_high};
use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec,
    ProcessContext, SignalType, Widget,
};

/// `loom.rand.sample-hold` (MT-RAND-02): takes a value and holds it.
pub static SAMPLE_HOLD: ModuleDescriptor = ModuleDescriptor {
    id: "loom.rand.sample-hold",
    version: 1,
    name: "Sample and Hold",
    category: "Random",
    family: loom_core::Family::Modulation,
    description: "Takes the input's value on a trigger and holds it. With noise on the input, it makes stepped random values.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Modulation,
            description: "The signal to sample.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "trigger",
            label: "Trigger",
            signal: SignalType::Gate,
            description: "A rising edge takes a new value (Sample), or lets the input through while high (Track).",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The held value. It starts at 0.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        name: "mode",
        label: "Mode",
        description: "Sample: take the value on the rising edge. Track: follow the input while the trigger is high.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Sample", "Track"])],
    role: ModuleRole::Dsp(|_| {
        Box::new(SampleHold {
            track: false,
            edge: EdgeDetector::new(),
            held: 0.0,
        })
    }),
    panel: Panel::new(7, 2, &[Widget::segmented(0, 0, 7, "mode")]),
    displays: &[],
};

/// State for `loom.rand.sample-hold`: `track` is the mode, `edge` finds the
/// trigger edges, and `held` is the value on the output.
#[derive(Clone)]
struct SampleHold {
    track: bool,
    edge: EdgeDetector,
    held: f32,
}

impl SampleHold {
    const MODE: usize = SAMPLE_HOLD.param_index("mode").unwrap();
}

impl Module for SampleHold {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let mode_mod = io.param(Self::MODE);
        let ([input, trigger], [out]) = io.split::<2, 1>();
        for (i, out) in out.iter_mut().enumerate() {
            let track = mode_mod.map_or(i, self.track, |v| v >= 0.5);
            // The detector sees every frame, so a mode change is no event.
            let rising = self.edge.rising(trigger[i]);
            let take = if track { gate_is_high(trigger[i]) } else { rising };
            if take && input[i].is_finite() {
                self.held = input[i];
            }
            *out = self.held;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::MODE {
            self.track = value >= 0.5;
        }
    }

    fn reset(&mut self) {
        self.held = 0.0;
        self.edge.reset();
    }
}

#[cfg(test)]
mod tests;
