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
pub mod transport;
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
    &filter::LADDER,
    &env::ENVELOPE,
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
    &mix::JOIN,
    &mix::SPLIT,
    &io::UNIT_DELAY,
    &io::AUDIO_OUT,
    &io::AUDIO_IN,
    &midi::NOTE_IN,
    &midi::CONTROLLER_IN,
    &display::METER,
    &display::SCOPE,
    &clock::CLOCK,
    &transport::TRANSPORT,
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
    /// Unipolar modulation outputs declare their range explicitly.
    #[test]
    fn envelope_and_velocity_are_unipolar() {
        for port in [&super::env::ENVELOPE.outputs[0], &super::midi::NOTE_IN.outputs[2]] {
            assert_eq!(port.range, loom_core::PortRange::Span(0.0, 1.0));
            assert_eq!(port.polarity(), loom_core::Polarity::Unipolar);
        }
    }

    #[test]
    fn all_modules_register() {
        assert_eq!(super::registry().len(), super::ALL.len());
    }

    #[test]
    fn panels_follow_the_spec() {
        // docs/design/panels.md, Module panels on tiles: a knob for every
        // parameter except these, and lights on MIDI Note and Envelope. Button's
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
                // The amplifier's response is a segmented control, not a knob.
                if descriptor.id == "loom.level.amp" && param.name == "response" {
                    assert!(
                        matches!(widget.map(|w| w.kind), Some(loom_core::WidgetKind::Segmented { .. })),
                        "amp.response"
                    );
                    continue;
                }
                // Convert's mode is a radio, not a knob.
                if descriptor.id == "loom.math.convert" && param.name == "mode" {
                    assert!(
                        matches!(widget.map(|w| w.kind), Some(loom_core::WidgetKind::Select)),
                        "convert.mode"
                    );
                    continue;
                }
                // Choice controls that stand in for a knob on these modules.
                let non_knob = [
                    ("loom.io.audio-in", "pair"),
                    ("loom.io.audio-in", "mute"),
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
                    ("loom.env.envelope", "mode"),
                    ("loom.filter.ladder", "slope"),
                    ("loom.midi.controller", "source"),
                    ("loom.clock.transport", "run"),
                ];
                if non_knob.contains(&(descriptor.id, param.name)) {
                    assert!(
                        matches!(
                            widget.map(|w| w.kind),
                            Some(
                                loom_core::WidgetKind::Radio
                                    | loom_core::WidgetKind::Toggle
                                    | loom_core::WidgetKind::Select
                                    | loom_core::WidgetKind::Segmented { .. }
                            )
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
                "loom.io.audio-in" => &["active"],
                "loom.env.envelope" => &["level"],
                "loom.logic.memory" => &["state"],
                // Trigger lights (panels.md, Lights for one-sample triggers).
                "loom.logic.divide" | "loom.phase.trigger" | "loom.clock.clock" => &["fired"],
                "loom.clock.transport" => &["beat"],
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
            let scopes: Vec<&str> = panel
                .widgets
                .iter()
                .filter(|w| w.kind == loom_core::WidgetKind::Scope)
                .map(|w| w.bind)
                .collect();
            let expected: &[&str] = if descriptor.id == "loom.display.scope" {
                &["out"]
            } else {
                &[]
            };
            assert_eq!(scopes, expected, "{}", descriptor.id);
        }
    }

    #[test]
    fn parameters_in_hz_and_seconds_are_logarithmic() {
        use loom_core::ParamMapping;
        for descriptor in super::ALL {
            for param in descriptor.params {
                // The controller's smoothing starts at 0 (none), and a
                // logarithmic range needs a minimum above 0 (p1-modules.md,
                // Controller input).
                if (descriptor.id, param.name) == ("loom.midi.controller", "smoothing") {
                    assert_eq!(param.options.mapping, ParamMapping::Linear);
                    continue;
                }
                let expected = if matches!(param.unit, "Hz" | "s") {
                    ParamMapping::Logarithmic
                } else {
                    ParamMapping::Linear
                };
                assert_eq!(param.options.mapping, expected, "{}.{}", descriptor.id, param.name);
            }
        }
    }

    /// Every built-in module follows the text rules (port-and-parameter-text.md,
    /// Module names and summaries; #458): Title Case names, summaries of at
    /// most two sentences, and no requirement IDs, method names, or sales
    /// words in any descriptor text.
    #[test]
    fn module_text_follows_the_rules() {
        const JOINING: [&str; 7] = ["and", "or", "of", "the", "a", "to", "in"];
        const DENIED: [&str; 9] = [
            "polyblep",
            "polyblamp",
            "trapezoidal",
            "nyquist",
            "warm",
            "rich",
            "classic",
            "pristine",
            "ideal",
        ];
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(str::to_lowercase)
                .collect()
        };
        let mut problems = Vec::new();
        for d in crate::registry().iter() {
            for (i, word) in d.name.split([' ', '-']).enumerate() {
                let first = word.chars().next().unwrap_or(' ');
                let joining = i > 0 && JOINING.contains(&word);
                if !(joining || first.is_uppercase() || first.is_ascii_digit()) {
                    problems.push(format!("{}: `{}` is not Title Case", d.id, d.name));
                }
            }
            let sentences = d
                .description
                .split_terminator(['.', '!', '?'])
                .filter(|s| s.trim_start().starts_with(|c: char| c.is_uppercase()))
                .count();
            if sentences > 2 {
                problems.push(format!("{}: the summary has {sentences} sentences", d.id));
            }
            let mut texts = vec![d.name, d.description];
            texts.extend(d.inputs.iter().chain(d.outputs).flat_map(|p| [p.label, p.description]));
            texts.extend(d.params.iter().flat_map(|p| [p.label, p.description]));
            texts.extend(d.displays.iter().flat_map(|v| [v.label, v.description]));
            for text in texts {
                if text.contains("FR-") {
                    problems.push(format!("{}: `{text}` names a requirement", d.id));
                }
                for word in words(text) {
                    if DENIED.contains(&word.as_str()) {
                        problems.push(format!("{}: `{text}` uses `{word}`", d.id));
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// A choice parameter's range and default read as its choice names.
    #[test]
    fn choice_ranges_read_as_names() {
        let env = crate::registry().resolve("loom.env.envelope").unwrap();
        let mode = env.params.iter().find(|p| p.name == "mode").unwrap();
        assert_eq!(mode.range_text(), "ADSR or AD");
        assert_eq!(mode.default_text(), "ADSR");
        let lfo = crate::registry().resolve("loom.mod.lfo").unwrap();
        let shape = lfo.params.iter().find(|p| p.name == "shape").unwrap();
        assert!(shape.range_text().contains(", or "), "{}", shape.range_text());
    }

    /// Every built-in panel reads top to bottom as view, then choices, then
    /// continuous controls (panels.md, Panel layout, principles 1 to 3): no
    /// result view starts below a choice or knob, and no choice starts
    /// below a knob.
    #[test]
    fn panels_put_the_view_first_then_choices_then_knobs() {
        use loom_core::WidgetKind as K;
        let rank = |kind: &K| match kind {
            K::Shape { .. } | K::Scope | K::Meter => Some(0),
            K::Segmented { .. } | K::Select | K::Radio => Some(1),
            K::Knob => Some(2),
            _ => None,
        };
        for d in crate::registry().iter() {
            let ranked: Vec<_> = d
                .panel
                .widgets
                .iter()
                .filter_map(|w| rank(&w.kind).map(|r| (r, w.y, w.bind)))
                .collect();
            for &(ra, ya, a) in &ranked {
                for &(rb, yb, b) in &ranked {
                    assert!(
                        !(ra < rb && ya > yb),
                        "{}: `{a}` sits below `{b}`, which should come after it",
                        d.id
                    );
                }
            }
        }
    }

    /// Two to four named choices use a segmented control or, for long
    /// names, a select list, never a column of radio buttons (principle 6).
    #[test]
    fn short_choice_lists_are_not_radio_columns() {
        for d in crate::registry().iter() {
            for w in d
                .panel
                .widgets
                .iter()
                .filter(|w| w.kind == loom_core::WidgetKind::Radio)
            {
                let choices = d.params[d.param_index(w.bind).unwrap()].options.choices.len();
                assert!(
                    choices > 4,
                    "{}: `{}` has {choices} choices as radio buttons",
                    d.id,
                    w.bind
                );
            }
        }
    }

    /// No built-in panel is more than one grid unit wider than its widest
    /// row of widgets needs (principle 7: no empty quarters).
    #[test]
    fn panels_are_no_wider_than_their_rows() {
        for d in crate::registry().iter() {
            let widest = d
                .panel
                .widgets
                .iter()
                .map(|w| w.x + d.widget_size(w).0)
                .max()
                .unwrap_or(0);
            assert!(
                d.panel.width <= widest + 1 || d.panel.widgets.is_empty(),
                "{}: the panel is {} wide, its widest row {widest}",
                d.id,
                d.panel.width
            );
        }
    }
}
