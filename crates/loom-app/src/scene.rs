//! Builds the canvas scene from the engine graph.

use iced::{Color, Point};
use loom_canvas::{
    CableView, HoverText, ModulationView, ModuleView, PanelValues, PanelView, PortShape, PortView, Scene, WidgetValue,
    WidgetView, WidgetViewKind,
};
use loom_core::{
    Controller, DisplayKind, InputRef, ModuleDescriptor, ModuleId, OutputRef, ParamSpec, PortSpec, SignalType,
    WidgetKind,
};
use std::collections::{BTreeMap, BTreeSet};

/// Port and cable colour for a signal type (ADR-0007).
pub fn signal_colour(signal: SignalType) -> Color {
    let [r, g, b] = signal.colour();
    Color::from_rgb8(r, g, b)
}

/// Marker shape for a signal type. Pitch and gate are square, so they differ
/// from audio and modulation by shape as well as colour (NFR-UX-03).
fn signal_shape(signal: SignalType) -> PortShape {
    match signal {
        SignalType::Pitch | SignalType::Gate => PortShape::Square,
        SignalType::Audio | SignalType::Modulation | SignalType::Phase => PortShape::Round,
    }
}

/// Header colour by module category. Muted, so cable colours stand out.
pub fn category_colour(category: &str) -> Color {
    match category {
        "Phase" => Color::from_rgb8(0x5A, 0x4A, 0x82),
        "Oscillator" => Color::from_rgb8(0x8A, 0x45, 0x45),
        "Filter" => Color::from_rgb8(0x3D, 0x68, 0x86),
        "Envelope" => Color::from_rgb8(0x3E, 0x7A, 0x58),
        "MIDI" => Color::from_rgb8(0x86, 0x6A, 0x3A),
        "I/O" => Color::from_rgb8(0x6E, 0x6E, 0x78),
        "Time" => Color::from_rgb8(0x3E, 0x76, 0x78),
        "Display" => Color::from_rgb8(0x4A, 0x6E, 0x5A),
        _ => Color::from_rgb8(0x50, 0x54, 0x60),
    }
}

fn port_view(spec: &PortSpec) -> PortView {
    PortView {
        label: spec.label.to_string(),
        hover: HoverText::new(spec.label, spec.description, spec.name),
        colour: signal_colour(spec.signal),
        shape: signal_shape(spec.signal),
        badge: 0,
    }
}

/// The canvas view of a module's panel (ADR-0013). Controls show their
/// parameter's label, meters their output's, and lights their display
/// value's, each with its description as hover text
/// (port-and-parameter-text.md, Display values).
fn panel_view(d: &ModuleDescriptor) -> PanelView {
    let widgets = d
        .panel
        .widgets
        .iter()
        .map(|w| {
            let kind = match w.kind {
                WidgetKind::Knob => {
                    let steps = d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind)
                        .filter(|p| p.options.integer)
                        .map_or(0, |p| (p.max - p.min) as u32);
                    WidgetViewKind::Knob { steps }
                }
                WidgetKind::Toggle => WidgetViewKind::Toggle,
                WidgetKind::Meter => WidgetViewKind::Meter,
                // The names come from the parameter (panels.md, Choice names).
                WidgetKind::Radio => WidgetViewKind::Radio {
                    options: d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind)
                        .map_or_else(Vec::new, |p| p.options.choices.iter().map(|o| o.to_string()).collect()),
                },
                WidgetKind::Button { .. } => WidgetViewKind::Button,
                // Light, and any kind this app does not draw yet.
                _ => WidgetViewKind::Light,
            };
            let (label, hover) = if w.is_control() {
                let param = d.params.iter().find(|p| p.name == w.bind);
                param.map_or((w.bind, HoverText::default()), |p| {
                    (p.label, HoverText::new(p.label, p.description, p.name))
                })
            } else if w.kind == WidgetKind::Meter {
                // A meter's text is its output's: two texts for one signal
                // could disagree.
                let output = d.outputs.iter().find(|p| p.name == w.bind);
                output.map_or((w.bind, HoverText::default()), |p| {
                    (p.label, HoverText::new(p.label, p.description, p.name))
                })
            } else {
                let display = d.displays.iter().find(|v| v.name == w.bind);
                display.map_or((w.bind, HoverText::default()), |v| {
                    (v.label, HoverText::new(v.label, v.description, v.name))
                })
            };
            WidgetView {
                kind,
                x: w.x,
                y: w.y,
                label: label.to_string(),
                hover,
            }
        })
        .collect();
    PanelView {
        width: d.panel.width,
        height: d.panel.height,
        widgets,
    }
}

/// A control's position for the canvas: the knob position from 0 to 1, the
/// toggle state, or the radio option index.
pub fn control_position(kind: WidgetKind, spec: &ParamSpec, value: f32) -> f32 {
    match kind {
        WidgetKind::Knob => spec.normalize(value),
        _ => value - spec.min,
    }
}

/// The parameter value for a control position from the canvas.
pub fn control_value(kind: WidgetKind, spec: &ParamSpec, position: f32) -> f32 {
    match kind {
        WidgetKind::Knob => spec.denormalize(position),
        _ => spec.clamp(spec.min + position.round()),
    }
}

/// Updates the live value of every panel widget: parameter values and
/// modulation marks for controls, and `shown` (position and peak, by module
/// and widget) for lights and meters. Values that did not change keep the generation, so the
/// canvas uploads nothing new.
pub fn update_panel_values(
    values: &mut PanelValues,
    controller: &Controller,
    shown: &BTreeMap<(u32, usize), (f32, f32)>,
) {
    update_panel_values_dragging(values, controller, shown, None);
}

/// Updates only the live dots, for the refresh on every frame: the knobs
/// bound to a parameter with an enabled assignment. Values that did not
/// change leave the panel generation alone, so the canvas rebuilds nothing.
pub fn update_live_values(values: &mut PanelValues, controller: &Controller) {
    let graph = controller.graph();
    let mut done = BTreeSet::new();
    for (_, a) in graph.assignments().filter(|(_, a)| a.enabled) {
        if !done.insert((a.target, a.param)) {
            continue;
        }
        let Some(d) = graph.node(a.target).map(|n| n.descriptor) else {
            continue;
        };
        let name = d.params[a.param].name;
        let live = controller.modulated(a.target, a.param);
        for (index, w) in d.panel.widgets.iter().enumerate() {
            if w.kind != WidgetKind::Knob || w.bind != name {
                continue;
            }
            if let Some(value) = values.get(a.target.0, index).filter(|v| v.live != live) {
                let value = WidgetValue { live, ..value.clone() };
                values.set(a.target.0, index, value);
            }
        }
    }
}

/// As [`update_panel_values`], while the arc of a knob is dragged: that
/// knob's arc shows only `dragging`, the assignment being changed, so the
/// user sees what moves (`docs/design/modulation.md`, Arc drag).
pub fn update_panel_values_dragging(
    values: &mut PanelValues,
    controller: &Controller,
    shown: &BTreeMap<(u32, usize), (f32, f32)>,
    dragging: Option<loom_core::AssignmentId>,
) {
    let reaches = crate::modulation::reaches(controller.graph());
    // The dragged knob's arc shows only the dragged assignment. Whether it
    // shows the live dot still depends on all of them.
    let dragged = dragging
        .and_then(|id| controller.graph().assignment(id))
        .and_then(|a| Some(((a.target, a.param), crate::modulation::reach(controller.graph(), a)?)));
    for (id, node) in controller.graph().nodes() {
        let d = node.descriptor;
        for (index, w) in d.panel.widgets.iter().enumerate() {
            let value = match d.control_param(w, &node.params) {
                Some(p) => {
                    // The knob spans the selected range (control-ranges.md).
                    let spec = &node.spec(p).expect("the index is a parameter");
                    let value = node.params[p];
                    let all = reaches.get(&(id, p)).map_or(&[][..], Vec::as_slice);
                    let assigned = match &dragged {
                        Some((key, reach)) if *key == (id, p) => std::slice::from_ref(reach),
                        _ => all,
                    };
                    // Only knobs show the live value, and only while an
                    // assignment is on (modulation.md, Live value).
                    let live = (w.kind == WidgetKind::Knob && all.iter().any(|r| r.enabled))
                        .then(|| controller.modulated(id, p))
                        .flatten();
                    WidgetValue {
                        position: control_position(w.kind, spec, value),
                        peak: 0.0,
                        text: crate::param::format(spec, value),
                        modulation: crate::modulation::mark(w.kind, spec, value, assigned),
                        live,
                    }
                }
                _ => {
                    let (position, peak) = shown.get(&(id.0, index)).copied().unwrap_or_default();
                    WidgetValue {
                        position,
                        peak,
                        text: display_text(d, w, position),
                        ..WidgetValue::default()
                    }
                }
            };
            values.set(id.0, index, value);
        }
    }
}

/// The value line of a light's or meter's hover text: the label and the
/// value. A light shows its brightness as a number, or "lit" and "dark" for
/// a trigger. A meter shows its RMS level in dB.
fn display_text(d: &ModuleDescriptor, w: &loom_core::Widget, position: f32) -> String {
    if w.kind == WidgetKind::Meter {
        let label = d.outputs.iter().find(|p| p.name == w.bind).map_or(w.bind, |p| p.label);
        return if position > 0.0 {
            format!("{label}: {:.1} dB", crate::live::position_db(position))
        } else {
            format!("{label}: below -60 dB")
        };
    }
    let Some(display) = d.displays.iter().find(|v| v.name == w.bind) else {
        return String::new();
    };
    match display.kind {
        DisplayKind::Level => format!("{}: {position:.2}", display.label),
        DisplayKind::Trigger if position > 0.0 => format!("{}: lit", display.label),
        DisplayKind::Trigger => format!("{}: dark", display.label),
    }
}

/// The outputs that meters on these modules show, with the widget each
/// one feeds.
pub fn meter_outputs(controller: &Controller, modules: &[u32]) -> Vec<((u32, usize), OutputRef)> {
    modules
        .iter()
        .filter_map(|&id| Some((id, controller.graph().node(ModuleId(id))?.descriptor)))
        .flat_map(|(id, d)| {
            d.panel
                .widgets
                .iter()
                .enumerate()
                .filter(|(_, w)| w.kind == WidgetKind::Meter)
                .filter_map(move |(index, w)| {
                    let port = d.outputs.iter().position(|p| p.name == w.bind)?;
                    Some((
                        (id, index),
                        OutputRef {
                            module: ModuleId(id),
                            port,
                        },
                    ))
                })
        })
        .collect()
}

/// The parameter a control is bound to, limited to its selected range, if
/// the module and widget exist.
///
/// A button in momentary mode is bound to its hold, and otherwise to its
/// latch (clock.md, Button details).
pub fn control_param(controller: &Controller, module: u32, widget: usize) -> Option<(usize, WidgetKind, ParamSpec)> {
    let node = controller.graph().node(ModuleId(module))?;
    let d = node.descriptor;
    let w = d.panel.widgets.get(widget)?;
    let index = d.control_param(w, &node.params)?;
    // The control spans the selected range (control-ranges.md, rule 4).
    Some((index, w.kind, node.spec(index)?))
}

/// Builds the scene for the controller's graph. Modules without a position
/// are placed at the origin; call [`crate::layout::auto_layout`] first.
pub fn build(controller: &Controller, positions: &BTreeMap<u32, Point>) -> Scene {
    let graph = controller.graph();
    let delayed: BTreeSet<InputRef> = controller.last_report().inserted_delays().map(|(_, to)| *to).collect();

    // Assignments each output drives, counting those that are off.
    let mut badges: BTreeMap<OutputRef, u32> = BTreeMap::new();
    for (_, a) in graph.assignments() {
        *badges.entry(a.source).or_default() += 1;
    }
    let modules = graph
        .nodes()
        .map(|(id, node)| {
            let d = node.descriptor;
            let outputs = d
                .outputs
                .iter()
                .enumerate()
                .map(|(port, spec)| PortView {
                    badge: badges.get(&OutputRef { module: id, port }).copied().unwrap_or(0),
                    ..port_view(spec)
                })
                .collect();
            ModuleView {
                id: id.0,
                title: d.name.to_string(),
                accent: category_colour(d.category),
                position: positions.get(&id.0).copied().unwrap_or(Point::ORIGIN),
                inputs: d.inputs.iter().map(port_view).collect(),
                outputs,
                panel: panel_view(d),
            }
        })
        .collect();

    let cables = graph
        .cables()
        .filter_map(|(from, to)| {
            let source = graph.node(from.module)?.descriptor.outputs.get(from.port)?;
            let target = graph.node(to.module)?.descriptor.inputs.get(to.port)?;
            Some(CableView {
                from: (from.module.0, from.port),
                to: (to.module.0, to.port),
                colour_from: signal_colour(source.signal),
                colour_to: signal_colour(target.signal),
                delayed: delayed.contains(&to),
            })
        })
        .collect();

    // One line per assignment, to the control bound to its parameter.
    let modulations = graph
        .assignments()
        .filter_map(|(_, a)| {
            let d = graph.node(a.target)?.descriptor;
            let name = d.params.get(a.param)?.name;
            Some(ModulationView {
                from: (a.source.module.0, a.source.port),
                to: a.target.0,
                widget: d.panel.widgets.iter().position(|w| w.is_control() && w.bind == name),
            })
        })
        .collect();

    Scene::new(modules, cables).with_modulations(modulations)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parameter for test descriptors.
    const fn param(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> loom_core::ParamSpec {
        loom_core::ParamSpec {
            name,
            label: name,
            description: "A test parameter.",
            min,
            max,
            default,
            unit,
            options: loom_core::ParamOptions::DEFAULT,
        }
    }

    const CUTOFF: ParamSpec = param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();

    /// A modulated knob carries the engine's live value while an
    /// assignment is on, and none when every assignment is off.
    #[test]
    fn modulated_knobs_carry_the_live_value() {
        let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let controller = &mut parts.controller;
        let source = controller.add_module("loom.math.constant").unwrap();
        controller.set_param_named(source, "value", 0.5).unwrap();
        let filter = controller.add_module("loom.filter.svf").unwrap();
        let id = controller
            .add_assignment_named(source, "out", filter, "cutoff", 0.2)
            .unwrap();
        let cutoff = controller.graph().node(filter).unwrap().descriptor;
        let knob = cutoff.panel.widgets.iter().position(|w| w.bind == "cutoff").unwrap();
        controller.commit().unwrap();
        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
        parts.engine.process(&mut left, &mut right);

        let mut values = PanelValues::new();
        update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
        let live = values.get(filter.0, knob).unwrap().live.expect("a live value");
        assert_eq!(Some(live), parts.controller.modulated(filter, 0));
        // The resonance knob is not modulated.
        let other = 1 - knob;
        assert_eq!(values.get(filter.0, other).unwrap().live, None);

        parts.controller.set_assignment_enabled(id, false).unwrap();
        update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
        assert_eq!(values.get(filter.0, knob).unwrap().live, None);
    }

    /// The frame refresh changes only the live dot, and only when it moved,
    /// so a still value keeps the panel generation.
    #[test]
    fn the_frame_refresh_touches_only_live_dots() {
        let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let controller = &mut parts.controller;
        let source = controller.add_module("loom.math.constant").unwrap();
        let filter = controller.add_module("loom.filter.svf").unwrap();
        controller
            .add_assignment_named(source, "out", filter, "cutoff", 0.2)
            .unwrap();
        let d = controller.graph().node(filter).unwrap().descriptor;
        let knob = d.panel.widgets.iter().position(|w| w.bind == "cutoff").unwrap();
        controller.commit().unwrap();

        let mut values = PanelValues::new();
        update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
        assert_eq!(values.get(filter.0, knob).unwrap().live, None, "not swapped in yet");
        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
        parts.engine.process(&mut left, &mut right);
        let before = values.get(filter.0, knob).unwrap().clone();
        update_live_values(&mut values, &parts.controller);
        let after = values.get(filter.0, knob).unwrap().clone();
        assert!(after.live.is_some());
        assert_eq!(WidgetValue { live: None, ..after }, before, "only the dot changed");
        let generation = values.generation();
        update_live_values(&mut values, &parts.controller);
        assert_eq!(values.generation(), generation, "a still value changes nothing");
    }

    /// Dragging an assignment that is off shows only its arc, but the knob
    /// keeps its live dot while another assignment is on.
    #[test]
    fn dragging_an_off_assignment_keeps_the_live_dot() {
        let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let controller = &mut parts.controller;
        let source = controller.add_module("loom.math.constant").unwrap();
        let filter = controller.add_module("loom.filter.svf").unwrap();
        controller
            .add_assignment_named(source, "out", filter, "cutoff", 0.2)
            .unwrap();
        let off = controller
            .add_assignment_named(filter, "lowpass", filter, "cutoff", 0.1)
            .unwrap();
        controller.set_assignment_enabled(off, false).unwrap();
        let d = controller.graph().node(filter).unwrap().descriptor;
        let knob = d.panel.widgets.iter().position(|w| w.bind == "cutoff").unwrap();
        controller.commit().unwrap();
        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
        parts.engine.process(&mut left, &mut right);

        let mut values = PanelValues::new();
        update_panel_values_dragging(&mut values, &parts.controller, &BTreeMap::new(), Some(off));
        let value = values.get(filter.0, knob).unwrap();
        assert!(value.live.is_some());
        assert_eq!(
            value.modulation,
            loom_canvas::ModulationMark::Ring,
            "only the off one's arc"
        );
    }

    #[test]
    fn halfway_up_the_cutoff_knob_is_632_hz() {
        let value = control_value(WidgetKind::Knob, &CUTOFF, 0.5);
        assert!((value - 632.46).abs() < 0.1, "{value}");
        let back = control_position(WidgetKind::Knob, &CUTOFF, value);
        assert!((back - 0.5).abs() < 1e-4);
    }

    /// A button shows its latch in toggle mode and its hold in momentary
    /// mode, so a latched `on` does not show as a hold (clock.md, Button
    /// details).
    #[test]
    fn a_button_shows_the_parameter_its_mode_chooses() {
        use loom_core::{EngineConfig, Registry, create_engine};
        use std::sync::Arc;

        let mut registry = Registry::new();
        loom_modules::register_all(&mut registry).unwrap();
        let parts = create_engine(Arc::new(registry), EngineConfig::default());
        let mut controller = parts.controller;
        let id = controller.add_module("loom.ui.button").unwrap();
        controller.set_param_named(id, "on", 1.0).unwrap();
        let (index, _, _) = control_param(&controller, id.0, 0).unwrap();
        assert_eq!(controller.graph().node(id).unwrap().descriptor.params[index].name, "on");
        controller.set_param_named(id, "mode", 1.0).unwrap();
        let (index, _, spec) = control_param(&controller, id.0, 0).unwrap();
        assert_eq!(spec.name, "hold");
        let node = controller.graph().node(id).unwrap();
        // The latch is kept, and the hold is released.
        assert_eq!(node.params[node.descriptor.param_index("on").unwrap()], 1.0);
        assert_eq!(node.params[index], 0.0);
    }

    /// Every built-in radio option shows in full on its tile: radio buttons
    /// are wide enough that no choice name is cut with "…" (#394).
    #[test]
    fn no_built_in_radio_option_is_cut() {
        let registry = loom_modules::registry();
        let mut checked = 0;
        for d in registry.iter() {
            if !d.panel.widgets.iter().any(|w| w.kind == WidgetKind::Radio) {
                continue;
            }
            let module = ModuleView {
                id: 0,
                title: d.name.to_string(),
                accent: category_colour(d.category),
                position: Point::ORIGIN,
                inputs: d.inputs.iter().map(port_view).collect(),
                outputs: d.outputs.iter().map(port_view).collect(),
                panel: panel_view(d),
            };
            loom_canvas::for_each_tile_text(&module, |item| {
                if matches!(item.kind, loom_canvas::TextKind::Option(..)) {
                    checked += 1;
                    assert!(!item.content.ends_with('…'), "{}: {} is cut", d.id, item.content);
                }
            });
        }
        assert!(checked > 20, "only {checked} radio options found");
    }

    #[test]
    fn toggles_and_radio_buttons_use_whole_values() {
        let on = param("on", 0.0, 1.0, 0.0, "").integer();
        assert_eq!(control_value(WidgetKind::Toggle, &on, 1.0), 1.0);
        assert_eq!(control_position(WidgetKind::Toggle, &on, 1.0), 1.0);
        let slope = param("slope", 1.0, 4.0, 1.0, "").integer();
        let radio = WidgetKind::Radio;
        assert_eq!(control_value(radio, &slope, 2.0), 3.0);
        assert_eq!(control_position(radio, &slope, 3.0), 2.0);
        assert_eq!(control_value(radio, &slope, 9.0), 4.0);
    }

    #[test]
    fn tiles_show_labels_not_names() {
        // panels.md, Labels and hover text: Clock Divide's `n` knob says
        // "Division", and its hover text keeps the name, which differs.
        let divide = &loom_modules::logic::DIVIDE;
        let knob = &panel_view(divide).widgets[0];
        assert_eq!(knob.label, "Division");
        assert_eq!(knob.hover.description, "Fires on every Nth trigger.");
        assert_eq!(knob.hover.name.as_deref(), Some("n"));
        let reset = port_view(&divide.inputs[1]);
        assert_eq!(reset.label, "Reset");
        assert_eq!(reset.hover.name, None, "`reset` reads the same as \"Reset\"");
    }

    /// port-and-parameter-text.md, Display values: a light's caption is its
    /// display value's label, and its hover text the description. A meter
    /// takes both from the output it shows.
    #[test]
    fn meters_and_lights_show_labels_and_descriptions() {
        let meter_module = &loom_modules::display::METER;
        let meter = &panel_view(meter_module).widgets[0];
        assert_eq!(meter.label, "Out");
        assert_eq!(meter.hover.description, meter_module.outputs[0].description);
        let light = &panel_view(&loom_modules::env::ADSR).widgets[4];
        assert_eq!(light.kind, WidgetViewKind::Light);
        assert_eq!(light.label, "Level");
        assert_eq!(
            light.hover,
            HoverText::new("Level", loom_modules::env::ADSR.displays[0].description, "level")
        );
        assert_eq!(light.hover.name, None, "`level` reads the same as \"Level\"");
        let gate = &panel_view(&loom_modules::midi::NOTE_IN)
            .widgets
            .last()
            .cloned()
            .unwrap();
        assert_eq!(gate.label, "Gate");
        assert_eq!(gate.hover.description, "Lit while a key is held.");
        let fired = &panel_view(&loom_modules::logic::DIVIDE).widgets[2];
        assert_eq!(fired.label, "Fired");
    }

    /// The value line of a light's or meter's hover text.
    #[test]
    fn lights_and_meters_say_their_value() {
        let adsr = &loom_modules::env::ADSR;
        assert_eq!(display_text(adsr, &adsr.panel.widgets[4], 0.5), "Level: 0.50");
        let divide = &loom_modules::logic::DIVIDE;
        assert_eq!(display_text(divide, &divide.panel.widgets[2], 0.4), "Fired: lit");
        assert_eq!(display_text(divide, &divide.panel.widgets[2], 0.0), "Fired: dark");
        let meter = &loom_modules::display::METER;
        assert_eq!(display_text(meter, &meter.panel.widgets[0], 0.8), "Out: -12.0 dB");
        assert_eq!(display_text(meter, &meter.panel.widgets[0], 0.0), "Out: below -60 dB");
        // A control has no value line here: its text is the parameter value.
        assert_eq!(display_text(divide, &divide.panel.widgets[0], 0.5), "");
    }

    /// A radio widget reads its names from the parameter (panels.md, Choice
    /// names), so it shows the same words as the inspector.
    #[test]
    fn radio_buttons_show_the_parameter_choice_names() {
        let lfo = &loom_modules::lfo::LFO;
        let view = panel_view(lfo);
        let WidgetViewKind::Radio { options } = &view.widgets[1].kind else {
            panic!("the shape is a radio");
        };
        let shape = &lfo.params[lfo.param_index("shape").unwrap()];
        assert_eq!(
            options,
            &shape.options.choices.iter().map(|o| o.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_built_in_panel_reaches_the_canvas() {
        for d in loom_modules::ALL {
            let view = panel_view(d);
            assert_eq!(view.widgets.len(), d.panel.widgets.len(), "{}", d.id);
            for (w, v) in d.panel.widgets.iter().zip(&view.widgets) {
                // Sizes must agree, or hit-testing would miss the drawing.
                assert_eq!(d.widget_size(w), v.size(), "{}.{}", d.id, w.bind);
            }
        }
    }
}
