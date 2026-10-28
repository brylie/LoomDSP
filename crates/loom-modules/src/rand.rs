//! Random and stepped sources (FR-LIB-01).

use loom_core::signal::{EdgeDetector, gate_is_high};
use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};

/// `loom.rand.sample-hold` (MT-RAND-02): takes a value and holds it.
pub static SAMPLE_HOLD: ModuleDescriptor = ModuleDescriptor {
    id: "loom.rand.sample-hold",
    version: 1,
    name: "Sample and hold",
    category: "Random",
    description: "Takes the input's value on a trigger and holds it. With noise on the input, it makes stepped random values.",
    inputs: &[
        PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Modulation,
            description: "The signal to sample.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "trigger",
            label: "Trigger",
            signal: SignalType::Gate,
            description: "A rising edge takes a new value (Sample), or lets the input through while high (Track).",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Modulation,
        description: "The held value. It starts at 0.",
        default: PortDefault::Value(0.0),
        polarity: Polarity::Bipolar,
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
    panel: Panel::new(6, 3, &[Widget::radio(0, 0, "mode")]),
    displays: &[],
};

/// State for `loom.rand.sample-hold`: `track` is the mode, `edge` finds the
/// trigger edges, and `held` is the value on the output.
struct SampleHold {
    track: bool,
    edge: EdgeDetector,
    held: f32,
}

impl Module for SampleHold {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let mode_mod = io.param(0);
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
        if index == 0 {
            self.track = value >= 0.5;
        }
    }

    fn reset(&mut self) {
        self.held = 0.0;
        self.edge.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::ModuleConfig;
    use loom_core::testing::run;

    /// Runs the module over the inputs in blocks of `block` frames.
    fn process(mode: f32, input: &[f32], trigger: &[f32], block: usize) -> Vec<f32> {
        let ModuleRole::Dsp(create) = SAMPLE_HOLD.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: block,
        });
        m.set_param(0, mode);
        m.reset();
        let mut out = vec![0.0; input.len()];
        for start in (0..input.len()).step_by(block) {
            let end = (start + block).min(input.len());
            run(
                m.as_mut(),
                48_000.0,
                &[&input[start..end], &trigger[start..end]],
                &[true, true],
                &mut [&mut out[start..end]],
                &[],
            );
        }
        out
    }

    /// Sample takes the input on the frame of the rising edge, at any block
    /// size; Track follows the input while the trigger is high.
    #[test]
    fn samples_on_the_edge_and_tracks_while_high() {
        let input: Vec<f32> = (0..12).map(|i| i as f32).collect();
        let mut trigger = vec![0.0; 12];
        trigger[3..6].fill(1.0);
        trigger[9] = 1.0;
        let sampled = [0.0, 0.0, 0.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 9.0, 9.0, 9.0];
        let tracked = [0.0, 0.0, 0.0, 3.0, 4.0, 5.0, 5.0, 5.0, 5.0, 9.0, 9.0, 9.0];
        for block in [1, 5, 12] {
            assert_eq!(process(0.0, &input, &trigger, block), sampled, "sample, block {block}");
            assert_eq!(process(1.0, &input, &trigger, block), tracked, "track, block {block}");
        }
    }
}
