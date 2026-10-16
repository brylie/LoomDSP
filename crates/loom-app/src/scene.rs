//! Builds the canvas scene from the engine graph.

use iced::{Color, Point};
use loom_canvas::{
    CableView, ModuleView, PanelValues, PanelView, PortShape, PortView, Scene, WidgetValue, WidgetView, WidgetViewKind,
};
use loom_core::{
    Controller, InputRef, ModuleDescriptor, ModuleId, OutputRef, ParamSpec, PortSpec, SignalType, WidgetKind,
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
        label: spec.name.to_string(),
        colour: signal_colour(spec.signal),
        shape: signal_shape(spec.signal),
    }
}

/// The canvas view of a module's panel (ADR-0013).
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
                        .filter(|p| p.integer)
                        .map_or(0, |p| (p.max - p.min) as u32);
                    WidgetViewKind::Knob { steps }
                }
                WidgetKind::Toggle => WidgetViewKind::Toggle,
                WidgetKind::Meter => WidgetViewKind::Meter,
                WidgetKind::Radio { options } => WidgetViewKind::Radio {
                    options: options.iter().map(|o| o.to_string()).collect(),
                },
                // Light, and any kind this app does not draw yet.
                _ => WidgetViewKind::Light,
            };
            WidgetView {
                kind,
                x: w.x,
                y: w.y,
                label: w.bind.to_string(),
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

/// Updates the live value of every panel widget: parameter values for
/// controls, and `shown` (position and peak, by module and widget) for
/// lights and meters. Values that did not change keep the generation, so the
/// canvas uploads nothing new.
pub fn update_panel_values(
    values: &mut PanelValues,
    controller: &Controller,
    shown: &BTreeMap<(u32, usize), (f32, f32)>,
) {
    for (id, node) in controller.graph().nodes() {
        let d = node.descriptor;
        for (index, w) in d.panel.widgets.iter().enumerate() {
            let value = match d.param_index(w.bind) {
                Some(p) if w.is_control() => {
                    let spec = &d.params[p];
                    let value = node.params[p];
                    WidgetValue {
                        position: control_position(w.kind, spec, value),
                        peak: 0.0,
                        text: crate::param::format(spec, value),
                    }
                }
                _ => {
                    let (position, peak) = shown.get(&(id.0, index)).copied().unwrap_or_default();
                    WidgetValue {
                        position,
                        peak,
                        text: String::new(),
                    }
                }
            };
            values.set(id.0, index, value);
        }
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

/// The parameter a control is bound to, if the module and widget exist.
pub fn control_param(controller: &Controller, module: u32, widget: usize) -> Option<(usize, WidgetKind, ParamSpec)> {
    let d = controller.graph().node(ModuleId(module))?.descriptor;
    let w = d.panel.widgets.get(widget).filter(|w| w.is_control())?;
    let index = d.param_index(w.bind)?;
    Some((index, w.kind, d.params[index]))
}

/// Builds the scene for the controller's graph. Modules without a position
/// are placed at the origin; call [`crate::layout::auto_layout`] first.
pub fn build(controller: &Controller, positions: &BTreeMap<u32, Point>) -> Scene {
    let graph = controller.graph();
    let delayed: BTreeSet<InputRef> = controller.last_report().inserted_delays().map(|(_, to)| *to).collect();

    let modules = graph
        .nodes()
        .map(|(id, node)| {
            let d = node.descriptor;
            ModuleView {
                id: id.0,
                title: d.name.to_string(),
                accent: category_colour(d.category),
                position: positions.get(&id.0).copied().unwrap_or(Point::ORIGIN),
                inputs: d.inputs.iter().map(port_view).collect(),
                outputs: d.outputs.iter().map(port_view).collect(),
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

    Scene::new(modules, cables)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: ParamSpec = ParamSpec::new("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();

    #[test]
    fn halfway_up_the_cutoff_knob_is_632_hz() {
        let value = control_value(WidgetKind::Knob, &CUTOFF, 0.5);
        assert!((value - 632.46).abs() < 0.1, "{value}");
        let back = control_position(WidgetKind::Knob, &CUTOFF, value);
        assert!((back - 0.5).abs() < 1e-4);
    }

    #[test]
    fn toggles_and_radio_buttons_use_whole_values() {
        let on = ParamSpec::new("on", 0.0, 1.0, 0.0, "").integer();
        assert_eq!(control_value(WidgetKind::Toggle, &on, 1.0), 1.0);
        assert_eq!(control_position(WidgetKind::Toggle, &on, 1.0), 1.0);
        let slope = ParamSpec::new("slope", 1.0, 4.0, 1.0, "").integer();
        let radio = WidgetKind::Radio {
            options: &["6", "12", "18", "24"],
        };
        assert_eq!(control_value(radio, &slope, 2.0), 3.0);
        assert_eq!(control_position(radio, &slope, 3.0), 2.0);
        assert_eq!(control_value(radio, &slope, 9.0), 4.0);
    }

    #[test]
    fn every_built_in_panel_reaches_the_canvas() {
        for d in loom_modules::ALL {
            let view = panel_view(d);
            assert_eq!(view.widgets.len(), d.panel.widgets.len(), "{}", d.id);
            for (w, v) in d.panel.widgets.iter().zip(&view.widgets) {
                // Sizes must agree, or hit-testing would miss the drawing.
                assert_eq!(w.size(), v.size(), "{}.{}", d.id, w.bind);
            }
        }
    }
}
