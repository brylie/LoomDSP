//! Display modules: modules whose job is to show a signal on their panel
//! (FR-LIB-17, ADR-0013).

use loom_core::{Module, ModuleDescriptor, ModuleRole, Panel, PortIo, PortSpec, ProcessContext, SignalType, Widget};

/// `loom.display.meter`: a mono level meter. Passes its input through, so it
/// can sit inline or hang off an output, and shows the level of that output
/// on its panel.
pub static METER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.display.meter",
    version: 1,
    name: "Level Meter",
    category: "Display",
    description: "Shows the peak and RMS level of its input. Passes the input through unchanged.",
    inputs: &[PortSpec::new("in", SignalType::Audio)],
    outputs: &[PortSpec::new("out", SignalType::Audio)],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Meter)),
    panel: Panel::new(3, 6, &[Widget::meter(0, 0, "out")]),
    displays: &[],
};

struct Meter;

impl Module for Meter {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([input], [out]) = io.split::<1, 1>();
        out.copy_from_slice(input);
    }
}
