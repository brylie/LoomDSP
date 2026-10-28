//! Built-in DSP modules for LoomDSP.
//!
//! The Phase 0 set is small on purpose: enough to build a playable synth from
//! primitives and to test feedback loops. Each module follows the same rules
//! as third-party modules (FR-SDK-01).
//!
//! Module IDs are stable. Never rename a released ID, port, or parameter.
//! Add a new version instead.

#![warn(missing_docs)]

use loom_core::{LoomError, ModuleDescriptor, Registry};
use std::sync::Arc;

pub mod clock;
pub mod display;
pub mod env;
pub mod filter;
pub mod io;
pub mod level;
pub mod lfo;
pub mod logic;
pub mod math;
pub mod midi;
pub mod mix;
pub mod osc;
pub mod phase;
pub mod rand;
pub mod time;
pub mod ui;

/// Every built-in module.
pub static ALL: &[&ModuleDescriptor] = &[
    &phase::PHASE,
    &phase::PHASE_TRIGGER,
    &phase::WRAP,
    &phase::SCALE,
    &lfo::LFO,
    &osc::SINE,
    &osc::SAW,
    &osc::PULSE,
    &osc::TRIANGLE,
    &osc::NOISE,
    &filter::SVF,
    &filter::ONEPOLE,
    &env::ADSR,
    &math::ADD,
    &math::MUL,
    &math::SUB,
    &math::DIV,
    &math::CONSTANT,
    &math::PITCH_CONSTANT,
    &math::CONVERT,
    &level::AMP,
    &level::CONSTANT,
    &level::CLAMP,
    &level::SCALE,
    &mix::MIXER,
    &mix::CROSSFADE,
    &io::UNIT_DELAY,
    &io::AUDIO_OUT,
    &midi::NOTE_IN,
    &display::METER,
    &display::SCOPE,
    &clock::CLOCK,
    &logic::DIVIDE,
    &logic::COMPARE,
    &logic::LOGIC,
    &logic::MEMORY,
    &rand::SAMPLE_HOLD,
    &level::SLEW,
    &ui::BUTTON,
    &time::DELAY,
    &time::ALLPASS,
];

// Every built-in port and parameter has a label and a description, or this
// crate does not compile.
const _: () = loom_core::assert_documented(ALL);

/// Adds every built-in module to a registry.
pub fn register_all(registry: &mut Registry) -> Result<(), LoomError> {
    for descriptor in ALL {
        registry.register(descriptor)?;
    }
    Ok(())
}

/// A registry with every built-in module.
pub fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    register_all(&mut registry).expect("built-in modules are valid and unique");
    Arc::new(registry)
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_modules_register() {
        assert_eq!(super::registry().len(), super::ALL.len());
    }

    #[test]
    fn panels_follow_the_spec() {
        // docs/design/panels.md, Module panels on tiles: a knob for every
        // parameter except these, and lights on MIDI Note and ADSR. Button's
        // `on` is the button itself. Its `mode` stays in the inspector.
        let inspector_only = [
            ("loom.osc.noise", "seed"),
            ("loom.midi.note", "channel"),
            ("loom.midi.note", "bend-range"),
            ("loom.ui.button", "mode"),
        ];
        for descriptor in super::ALL {
            let panel = &descriptor.panel;
            for param in descriptor.params {
                let widget = panel.widgets.iter().find(|w| w.bind == param.name);
                if descriptor.id == "loom.ui.button" && param.name == "on" {
                    assert!(
                        matches!(
                            widget.map(|w| w.kind),
                            Some(loom_core::WidgetKind::Button {
                                mode: "mode",
                                hold: "hold"
                            })
                        ),
                        "button.on"
                    );
                    continue;
                }
                // A transient parameter is reached through its widget only
                // (transient-params.md): Button's `hold` is the button.
                if param.is_transient() {
                    continue;
                }
                // The scope's `auto` is a toggle (panels.md, Scope module).
                if descriptor.id == "loom.display.scope" && param.name == "auto" {
                    assert_eq!(
                        widget.map(|w| w.kind),
                        Some(loom_core::WidgetKind::Toggle),
                        "scope.auto"
                    );
                    continue;
                }
                // The amplifier's response is a radio, not a knob.
                if descriptor.id == "loom.level.amp" && param.name == "response" {
                    assert!(
                        matches!(widget.map(|w| w.kind), Some(loom_core::WidgetKind::Radio)),
                        "amp.response"
                    );
                    continue;
                }
                // Convert's mode is a radio, not a knob.
                if descriptor.id == "loom.math.convert" && param.name == "mode" {
                    assert!(
                        matches!(widget.map(|w| w.kind), Some(loom_core::WidgetKind::Radio)),
                        "convert.mode"
                    );
                    continue;
                }
                // Radios and toggles that stand in for a knob on these modules.
                let non_knob = [
                    ("loom.phase.wrap", "mode"),
                    ("loom.mix.crossfade", "law"),
                    ("loom.mix.mixer", "mute1"),
                    ("loom.mix.mixer", "mute2"),
                    ("loom.mix.mixer", "mute3"),
                    ("loom.mix.mixer", "mute4"),
                    ("loom.logic.compare", "mode"),
                    ("loom.logic.logic", "op"),
                    ("loom.logic.logic", "invert"),
                    ("loom.logic.memory", "mode"),
                    ("loom.rand.sample-hold", "mode"),
                    ("loom.osc.noise", "colour"),
                    ("loom.mod.lfo", "sync"),
                    ("loom.mod.lfo", "shape"),
                ];
                if non_knob.contains(&(descriptor.id, param.name)) {
                    assert!(
                        matches!(
                            widget.map(|w| w.kind),
                            Some(loom_core::WidgetKind::Radio | loom_core::WidgetKind::Toggle)
                        ),
                        "{}.{}",
                        descriptor.id,
                        param.name
                    );
                    continue;
                }
                let knob = widget.is_some_and(|w| w.kind == loom_core::WidgetKind::Knob);
                let expected = !inspector_only.contains(&(descriptor.id, param.name));
                assert_eq!(knob, expected, "{}.{}", descriptor.id, param.name);
            }
            let lights: Vec<&str> = panel
                .widgets
                .iter()
                .filter(|w| w.kind == loom_core::WidgetKind::Light)
                .map(|w| w.bind)
                .collect();
            let expected: &[&str] = match descriptor.id {
                "loom.midi.note" => &["gate"],
                "loom.env.adsr" => &["level"],
                "loom.logic.memory" => &["state"],
                // Trigger lights (panels.md, Lights for one-sample triggers).
                "loom.logic.divide" | "loom.phase.trigger" | "loom.clock.clock" => &["fired"],
                _ => &[],
            };
            assert_eq!(lights, expected, "{}", descriptor.id);
            let meters: Vec<&str> = panel
                .widgets
                .iter()
                .filter(|w| w.kind == loom_core::WidgetKind::Meter)
                .map(|w| w.bind)
                .collect();
            let expected: &[&str] = if descriptor.id == "loom.display.meter" {
                &["out"]
            } else {
                &[]
            };
            assert_eq!(meters, expected, "{}", descriptor.id);
        }
    }

    #[test]
    fn parameters_in_hz_and_seconds_are_logarithmic() {
        use loom_core::ParamMapping;
        for descriptor in super::ALL {
            for param in descriptor.params {
                let expected = if matches!(param.unit, "Hz" | "s") {
                    ParamMapping::Logarithmic
                } else {
                    ParamMapping::Linear
                };
                assert_eq!(param.options.mapping, expected, "{}.{}", descriptor.id, param.name);
            }
        }
    }
}
