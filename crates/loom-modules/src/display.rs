//! Display modules: modules whose job is to show a signal on their panel
//! (FR-LIB-17, ADR-0013).

use loom_core::{
    Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamRange, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
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
    params: &[
        ParamSpec {
            name: "view",
            label: "View",
            description: "Shows the meter alone, or with its level over the last seconds beside it.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Meter", "Meter and history"]),
        ParamSpec {
            name: "window",
            label: "Window",
            description: "How much time the level history shows.",
            min: 0.0,
            max: 2.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["1 s", "5 s", "30 s"]),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Meter)),
    panel: Panel::new(
        12,
        6,
        &[
            Widget::meter(0, 0, "out").captioned(""),
            Widget::level_history(3, 0, "out")
                .captioned("")
                .shown_when("view", &[1]),
        ],
    ),
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

/// `loom.display.spectrum`: the frequency content of a signal (MT-VIEW-03).
/// Passes its input through, as the level meter does, so the app reads the
/// signal from a scope probe on `out`. The FFT runs in the app, never here,
/// and the parameters only change the drawing. Spec:
/// `docs/design/p2-modules.md`, Spectrum.
pub static SPECTRUM: ModuleDescriptor = ModuleDescriptor {
    id: "loom.display.spectrum",
    version: 1,
    name: "Spectrum",
    category: "Display",
    family: loom_core::Family::Interface,
    description: "Shows the frequency content of its input. Passes the input through unchanged.",
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
            name: "tilt",
            label: "Tilt",
            description: "Lifts the high frequencies on the display, so a full mix reads closer to level.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Flat", "+4.5 dB per octave"]),
        ParamSpec {
            name: "hold",
            label: "Hold",
            description: "Keeps a line at each recent peak for two seconds.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Off", "On"]),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Meter)),
    panel: Panel::new(9, 5, &[Widget::spectrum(0, 0, "out").captioned("")]),
    displays: &[],
};

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
            min: -10.0,
            max: 10.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .ranges(
            &[ParamRange::new(-1.0, 1.0), ParamRange::new(-10.0, 10.0)],
            ParamRange::new(-1.0, 1.0),
        )
        .shown_when("auto", &[0]),
        ParamSpec {
            name: "slope",
            label: "Slope",
            description: "Which edge starts the trace.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Rising", "Falling"]),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Meter)),
    panel: Panel::new(
        9,
        8,
        &[
            Widget::scope(0, 0, "out"),
            Widget::knob(0, 5, "window"),
            Widget::toggle(3, 5, "auto"),
            Widget::knob(6, 5, "level"),
        ],
    ),
    displays: &[],
};
