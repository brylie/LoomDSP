//! Panel controls that put a performer's gesture on a cable (FR-LIB-27).
//!
//! The button's mode decides which parameter the gesture writes: the saved
//! latch `on` in Toggle mode, the transient `hold` in Momentary mode. The
//! module sends the one its mode says. Spec: `docs/design/clock.md`, Button
//! details, and `docs/design/transient-params.md`.

use loom_core::{
    ButtonMode, Channels, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};

/// `loom.ui.button`: a gate from a large button.
///
/// Toggle keeps the gate where the last press left it. Momentary is high
/// only while the button is held. The canvas reports the gesture; the editor
/// writes `on` or `hold` from `mode`, and the module sends the one `mode`
/// chooses.
pub static BUTTON: ModuleDescriptor = ModuleDescriptor {
    id: "loom.ui.button",
    version: 1,
    name: "Button",
    category: "Control",
    family: loom_core::Family::Interface,
    description: "Sends a gate from a large button. Toggle keeps it on; Momentary is high only while held.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "gate",
        label: "Gate",
        signal: SignalType::Gate,
        description: "High while the button is on.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        ParamSpec {
            name: "mode",
            label: "Mode",
            description: "Toggle keeps the gate. Momentary is high only while the button is held.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(&["Toggle", "Momentary"]),
        ParamSpec {
            name: "on",
            label: "On",
            description: "The gate the button keeps in Toggle mode.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "hold",
            label: "Hold",
            description: "High while the button is held in Momentary mode. Never saved.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .transient(),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Button::default())),
    panel: Panel::new(6, 3, &[Widget::button(0, 0, "on", "hold", "mode").captioned("Gate")]),
    displays: &[],
};

/// Index of `mode`.
const MODE: usize = 0;
/// Index of `on`, the toggle latch.
const ON: usize = 1;
/// Index of `hold`, the momentary hold.
const HOLD: usize = 2;

/// The latest value of each parameter. The gate is `on` or `hold`.
#[derive(Default)]
struct Button {
    mode: f32,
    on: f32,
    hold: f32,
}

impl Module for Button {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        // Whole-number parameters arrive already rounded (modulation rule 5),
        // so the gate is 0 or 1. There is no second threshold here.
        let momentary = ButtonMode::from_value(self.mode) == ButtonMode::Momentary;
        let (index, base) = if momentary { (HOLD, self.hold) } else { (ON, self.on) };
        let modulated = io.param(index);
        let ([], [out]) = io.split::<0, 1>();
        for (i, sample) in out.iter_mut().enumerate() {
            *sample = modulated.at_or(i, base);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            MODE => self.mode = value,
            ON => self.on = value,
            HOLD => self.hold = value,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
