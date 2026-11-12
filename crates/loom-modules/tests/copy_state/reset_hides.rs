//! Proves that busy resets can hide lost state in the shared copy scenario.

use super::{AT, harness};
use loom_core::{Family, Module, ModuleDescriptor, ModuleRole, Panel, PortIo, ProcessContext, signal::EdgeDetector};
use loom_modules::route::SELECTOR;

/// Uses Selector's actual input layout so a port reorder or rename also changes
/// this proof. A faulty copy forgets the count but keeps both edge detectors.
static DROPPED_COUNT: ModuleDescriptor = ModuleDescriptor {
    id: "test.dropped-count",
    version: 1,
    name: "Dropped count",
    category: "Test",
    family: Family::Interface,
    description: "Counts step edges but forgets the count when copied.",
    inputs: SELECTOR.inputs,
    outputs: SELECTOR.outputs,
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(DroppedCount::default())),
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// A counter whose count is visible only on a step edge, like a random pick.
#[derive(Clone, Default)]
struct DroppedCount {
    /// Steps since the most recent reset.
    count: u32,
    /// The step input's previous gate value.
    step: EdgeDetector,
    /// The reset input's previous gate value.
    reset: EdgeDetector,
}

impl DroppedCount {
    /// The clock input, found by name even if Selector's ports move.
    const STEP_IN: usize = DROPPED_COUNT.input_index("step").unwrap();
    /// The reset input, which the reset-inactive busy scenario leaves off.
    const RESET_IN: usize = DROPPED_COUNT.input_index("reset").unwrap();
    /// The count pulse output.
    const OUT: usize = DROPPED_COUNT.output_index("out").unwrap();
}

impl Module for DroppedCount {
    /// Keeps the edge detectors but deliberately loses the accumulated count.
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        let copied = loom_core::copy_state(self, other);
        if copied {
            self.count = 0;
        }
        copied
    }

    /// Emits the count only on a step, after applying any reset on that frame.
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for frame in 0..io.input(Self::STEP_IN).len() {
            if self.reset.rising(io.input(Self::RESET_IN)[frame]) {
                self.count = 0;
            }
            let value = if self.step.rising(io.input(Self::STEP_IN)[frame]) {
                self.count = self.count.saturating_add(1);
                self.count as f32
            } else {
                0.0
            };
            io.output(Self::OUT)[frame] = value;
        }
    }
}

/// Frequent resets before each step make a copy's missing count invisible.
#[test]
fn busy_resets_hide_the_lost_count() {
    assert!(harness(&DROPPED_COUNT, true).check_copy_state(AT));
}

/// The suite's reset-inactive builder must expose that same faulty copy.
#[test]
#[should_panic(expected = "test.dropped-count: output `out` at frame")]
fn without_busy_resets_the_lost_count_is_detected() {
    harness(&DROPPED_COUNT, false).check_copy_state(AT);
}
