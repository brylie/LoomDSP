//! Display modules: modules whose job is to show a signal on their panel
//! (FR-LIB-17, ADR-0013).

use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo, PortSpec,
    ProcessContext, SignalType, Widget,
};

/// `loom.display.meter`: a mono level meter. Passes its input through, so it
/// can sit inline or hang off an output, and shows the level of that output
/// on its panel.
pub static METER: ModuleDescriptor = ModuleDescriptor {
    id: "loom.display.meter",
    version: 1,
    name: "Level Meter",
    category: "Display",
    family: loom_core::Family::Interface,
    description: "Shows the peak and RMS level of its input. Passes the input through unchanged.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal to measure.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The input, unchanged.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Meter)),
    panel: Panel::new(3, 6, &[Widget::meter(0, 0, "out")]),
    displays: &[],
};

/// Copies the input to the output. The meter and the scope share it: both
/// only pass the signal through for a probe to read.
struct Meter;

impl Module for Meter {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([input], [out]) = io.split::<1, 1>();
        out.copy_from_slice(input);
    }
}

/// `loom.display.scope`: a mono oscilloscope (MT-VIEW-01, scope mode).
/// Passes its input through, as the level meter does, so the app reads the
/// signal from a scope probe on `out`. The parameters only tell the app how
/// to draw the trace; they never change the audio. Spec:
/// `docs/design/panels.md`, Scope module.
pub static SCOPE: ModuleDescriptor = ModuleDescriptor {
    id: "loom.display.scope",
    version: 1,
    name: "Scope",
    category: "Display",
    family: loom_core::Family::Interface,
    description: "Shows its input over time. Passes the input through unchanged.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal to show.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The input, unchanged.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "window",
            label: "Window",
            description: "The time shown across the display.",
            min: 0.001,
            max: 1.0,
            default: 0.02,
            unit: "s",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic(),
        ParamSpec {
            name: "auto",
            label: "Auto",
            description: "Triggers halfway between the signal's lowest and highest values.",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "level",
            label: "Level",
            description: "The trigger level while Auto is off.",
            min: -1.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|_| Box::new(Meter)),
    panel: Panel::new(
        9,
        8,
        &[
            Widget::knob(0, 0, "window"),
            Widget::toggle(3, 0, "auto"),
            Widget::knob(6, 0, "level"),
            Widget::scope(0, 3, "out"),
        ],
    ),
    displays: &[],
};
