//! Builds the canvas scene from the engine graph.

use iced::{Color, Point};
use loom_canvas::{
    CableView, HoverText, ModulationView, ModuleView, PanelValues, PanelView, PortShape, PortView, Scene,
    TransportSignal, WidgetValue, WidgetView, WidgetViewKind,
};
use loom_core::{
    Controller, DisplayKind, InputRef, ModuleDescriptor, ModuleId, OutputRef, ParamSpec, PortDefault, PortSpec,
    SignalType, WidgetKind,
};
use std::collections::{BTreeMap, BTreeSet};

/// Segments across a result preview. About one world unit per segment on a
/// 12-grid-unit tile keeps its line smooth at close zoom.
const SHAPE_SEGMENTS: usize = 128;

/// Light and meter values by module and widget, including the right meter side.
pub type ShownValues = BTreeMap<(u32, usize), (f32, f32, Option<(f32, f32)>)>;

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

/// Header colour of a module's family, from the shared palette
/// (ADR-0029, visual-system.md, Module families).
pub fn family_colour(family: loom_core::Family) -> Color {
    loom_canvas::Palette::default().families[family.index()]
}

fn port_view(spec: &PortSpec, stereo: bool) -> PortView {
    let (min, max) = spec.nominal_range();
    let mut hover = HoverText::new(
        spec.label,
        &format!("{} Usually {min} to {max}.", spec.description),
        spec.name,
    );
    if stereo {
        hover.description.push_str(" Stereo.");
    }
    PortView {
        label: spec.label.to_string(),
        hover,
        colour: signal_colour(spec.signal),
        shape: signal_shape(spec.signal),
        badge: 0,
        follows_transport: None,
    }
}

/// The transport signal an input follows while it has no cable, if its
/// default is one (clock.md, Default inputs).
fn transport_default(spec: &PortSpec) -> Option<TransportSignal> {
    match spec.default {
        PortDefault::BarPhase => Some(TransportSignal::BarPhase),
        PortDefault::BeatPhase => Some(TransportSignal::BeatPhase),
        PortDefault::Playing => Some(TransportSignal::Playing),
        PortDefault::Value(_) => None,
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
                WidgetKind::Scope => WidgetViewKind::Scope,
                WidgetKind::Shape { width, .. } => WidgetViewKind::Shape { width },
                // The names come from the parameter (panels.md, Choice names).
                WidgetKind::Radio => WidgetViewKind::Radio {
                    options: d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind)
                        .map_or_else(Vec::new, |p| p.options.choices.iter().map(|o| o.to_string()).collect()),
                },
                WidgetKind::Select => WidgetViewKind::Select {
                    options: d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind)
                        .map_or_else(Vec::new, |p| p.options.choices.iter().map(|o| o.to_string()).collect()),
                },
                WidgetKind::Segmented { width } => WidgetViewKind::Segmented {
                    options: d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind)
                        .map_or_else(Vec::new, |p| p.options.choices.iter().map(|o| o.to_string()).collect()),
                    width,
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
            } else if let WidgetKind::Shape { label, .. } = w.kind {
                let output = d.outputs.iter().find(|p| p.name == w.bind);
                output.map_or((label, HoverText::default()), |p| {
                    (label, HoverText::new(label, p.description, p.name))
                })
            } else if w.shows_output() {
                // A meter's or scope's text is its output's: two texts for
                // one signal could disagree. A scope has no caption: its
                // frame fills the widget.
                let output = d.outputs.iter().find(|p| p.name == w.bind);
                let caption = |label| if w.kind == WidgetKind::Scope { "" } else { label };
                output.map_or((caption(w.bind), HoverText::default()), |p| {
                    (caption(p.label), HoverText::new(p.label, p.description, p.name))
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
                // A widget can caption itself, such as "1" under a mixer
                // strip; the hover text keeps the full label.
                label: w.caption.unwrap_or(label).to_string(),
                hover,
                hidden: false,
            }
        })
        .collect();
    PanelView {
        width: d.panel.width,
        height: d.panel.height,
        widgets,
    }
}

/// The panel for a module with these parameter values: widgets whose mode
/// does not use them are hidden, and the panel closes up around them
/// (ADR-0030, panels.md, Panel layout, principle 5).
pub fn panel_for(d: &ModuleDescriptor, values: &[f32]) -> PanelView {
    let mut panel = panel_view(d);
    let hidden: Vec<bool> = d
        .panel
        .widgets
        .iter()
        .map(|w| w.is_control() && d.param_index(w.bind).is_some_and(|i| !d.is_shown(i, values)))
        .collect();
    if hidden.iter().any(|&h| h) {
        close_up(&mut panel, &hidden);
    }
    panel
}

/// Hides the marked widgets and closes the panel up around them, keeping
/// its width (ADR-0030). A row that loses a widget closes its gap and is
/// centred again in the space the row had. Grid rows that only hidden
/// widgets used are removed, so the panel gets shorter.
pub fn close_up(panel: &mut PanelView, hidden: &[bool]) {
    for (widget, &h) in panel.widgets.iter_mut().zip(hidden) {
        widget.hidden = h;
    }
    // Close up each row that lost a widget: rows are the widgets that start
    // on the same grid row.
    let rows: std::collections::BTreeSet<u8> = panel.widgets.iter().filter(|w| w.hidden).map(|w| w.y).collect();
    for row in rows {
        let all: Vec<usize> = (0..panel.widgets.len())
            .filter(|&i| panel.widgets[i].y == row)
            .collect();
        let start = all.iter().map(|&i| panel.widgets[i].x).min().unwrap_or(0);
        let end = all
            .iter()
            .map(|&i| panel.widgets[i].x + panel.widgets[i].size().0)
            .max()
            .unwrap_or(0);
        let mut shown: Vec<usize> = all.into_iter().filter(|&i| !panel.widgets[i].hidden).collect();
        shown.sort_by_key(|&i| panel.widgets[i].x);
        let used: u8 = shown.iter().map(|&i| panel.widgets[i].size().0).sum();
        let mut x = start + (end - start - used.min(end - start)) / 2;
        for i in shown {
            panel.widgets[i].x = x;
            x += panel.widgets[i].size().0;
        }
    }
    // Remove grid rows that no shown widget covers but a hidden one did.
    let covers = |w: &WidgetView, r: u8| w.y <= r && r < w.y + w.size().1;
    let mut removed = 0;
    for r in (0..panel.height).rev() {
        let by_hidden = panel.widgets.iter().any(|w| w.hidden && covers(w, r));
        let by_shown = panel.widgets.iter().any(|w| !w.hidden && covers(w, r));
        if by_hidden && !by_shown {
            for w in panel.widgets.iter_mut().filter(|w| !w.hidden && w.y > r) {
                w.y -= 1;
            }
            removed += 1;
        }
    }
    panel.height -= removed;
}

/// The values the panel layout depends on: each mode parameter that some
/// `shown_when` names, by module (ADR-0030). The app rebuilds the scene
/// when this changes, which is a mode click, never a knob turn.
pub fn mode_key(controller: &Controller) -> Vec<(u32, usize, i64)> {
    let mut key = Vec::new();
    for (id, node) in controller.graph().nodes() {
        let d = node.descriptor;
        for p in d.params {
            if let Some(when) = p.options.shown_when
                && let Some(mode) = d.param_index(when.param)
            {
                let value = node.params.get(mode).copied().unwrap_or(d.params[mode].default);
                key.push((id.0, mode, value.round() as i64));
            }
        }
    }
    key.dedup();
    key
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
pub fn update_panel_values(values: &mut PanelValues, controller: &Controller, shown: &ShownValues) {
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
    shown: &ShownValues,
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
            let value = if let WidgetKind::Shape { sample, .. } = w.kind {
                WidgetValue {
                    shape: (0..=SHAPE_SEGMENTS)
                        .map(|i| {
                            let y = sample(&node.params, i as f32 / SHAPE_SEGMENTS as f32);
                            if y.is_finite() { y.clamp(0.0, 1.0) } else { 0.0 }
                        })
                        .collect(),
                    ..WidgetValue::default()
                }
            } else {
                match d.control_param(w, &node.params) {
                    Some(p) => {
                        // The knob spans the selected range (control-ranges.md).
                        let spec = &node
                            .spec(p)
                            .expect("node.spec covers every parameter index of the node");
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
                        // While a cable drives the Transport's run input, the
                        // Run toggle shows the state it sets (clock.md).
                        let value = driven_run(controller, id, w).unwrap_or(value);
                        WidgetValue {
                            position: control_position(w.kind, spec, value),
                            peak: 0.0,
                            right: None,
                            text: crate::param::format(spec, value),
                            modulation: crate::modulation::mark(w.kind, spec, value, assigned),
                            live,
                            shape: Vec::new(),
                        }
                    }
                    _ => {
                        let (position, peak, right) = shown.get(&(id.0, index)).copied().unwrap_or_default();
                        WidgetValue {
                            position,
                            peak,
                            right,
                            text: display_text(d, w, position, right),
                            ..WidgetValue::default()
                        }
                    }
                }
            };
            values.set(id.0, index, value);
        }
    }
}

/// For the Transport's Run toggle while a cable drives its `run` input:
/// 1 or 0, from the module's `running` display value. `None` otherwise.
fn driven_run(controller: &Controller, id: ModuleId, w: &loom_core::Widget) -> Option<f32> {
    if controller.transport_module() != Some(id) || w.bind != "run" {
        return None;
    }
    let driven = controller
        .graph()
        .cables()
        .any(|(_, to)| to == InputRef { module: id, port: 0 });
    if !driven {
        return None;
    }
    let d = controller.graph().node(id)?.descriptor;
    let running = controller.display(id, d.display_index("running")?)?;
    Some(if running >= 0.5 { 1.0 } else { 0.0 })
}

/// The value line of a light's or meter's hover text: the label and the
/// value. A light shows its brightness as a number, or "lit" and "dark" for
/// a trigger. A meter shows its RMS level in dB.
fn display_text(d: &ModuleDescriptor, w: &loom_core::Widget, position: f32, right: Option<(f32, f32)>) -> String {
    if w.kind == WidgetKind::Scope {
        // The trace is the value.
        return String::new();
    }
    if w.kind == WidgetKind::Meter {
        let label = d.outputs.iter().find(|p| p.name == w.bind).map_or(w.bind, |p| p.label);
        let level = |position| {
            if position > 0.0 {
                format!("{:.1} dB", crate::live::position_db(position))
            } else {
                "below -60 dB".to_string()
            }
        };
        return match right {
            Some((rms, _)) => format!("{label}: L {}, R {}", level(position), level(rms)),
            None => format!("{label}: {}", level(position)),
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

/// A scope widget on screen and what it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScopeOutput {
    /// Module ID and widget index.
    pub widget: (u32, usize),
    /// The output it shows.
    pub output: OutputRef,
    /// Seconds across the frame: the module's `window`, or 20 ms.
    pub window: f32,
    /// Where the trace starts: from the module's `auto` and `level`, or
    /// automatic.
    pub trigger: crate::live::Trigger,
}

/// The scopes on these modules (`docs/design/panels.md`, Scope module).
/// The window and trigger come from the module's `window`, `auto`, and
/// `level` parameters, when it has them.
pub fn scope_outputs(controller: &Controller, modules: &[u32]) -> Vec<ScopeOutput> {
    let mut scopes = Vec::new();
    for &id in modules {
        let Some(node) = controller.graph().node(ModuleId(id)) else {
            continue;
        };
        let d = node.descriptor;
        let param = |name: &str, default: f32| d.param_index(name).map_or(default, |i| node.params[i]);
        for (index, w) in d.panel.widgets.iter().enumerate() {
            if w.kind != WidgetKind::Scope {
                continue;
            }
            let Some(port) = d.outputs.iter().position(|p| p.name == w.bind) else {
                continue;
            };
            scopes.push(ScopeOutput {
                widget: (id, index),
                output: OutputRef {
                    module: ModuleId(id),
                    port,
                },
                window: param("window", 0.02),
                trigger: crate::live::Trigger::from_params(param("auto", 1.0), param("level", 0.0)),
            });
        }
    }
    scopes
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
pub fn build(controller: &Controller, positions: &BTreeMap<u32, Point>, sizes: &BTreeMap<u32, (u8, u8)>) -> Scene {
    let graph = controller.graph();
    let delayed: BTreeSet<InputRef> = controller.last_report().inserted_delays().map(|(_, to)| *to).collect();

    // Assignments each output drives, counting those that are off.
    let mut badges: BTreeMap<OutputRef, u32> = BTreeMap::new();
    for (_, a) in graph.assignments() {
        *badges.entry(a.source).or_default() += 1;
    }
    // Inputs with a cable: they no longer follow their default, so they
    // show no clock mark (panels.md, Clock mark on transport inputs).
    let cabled: BTreeSet<InputRef> = graph.cables().map(|(_, to)| to).collect();
    let modules = graph
        .nodes()
        .map(|(id, node)| {
            let d = node.descriptor;
            let mut inputs: Vec<PortView> = d
                .inputs
                .iter()
                .enumerate()
                .map(|(port, spec)| PortView {
                    follows_transport: transport_default(spec)
                        .filter(|_| !cabled.contains(&InputRef { module: id, port })),
                    ..port_view(
                        spec,
                        graph
                            .source_of(InputRef { module: id, port })
                            .is_some_and(|source| controller.last_report().width(source) == loom_core::Width::Stereo),
                    )
                })
                .collect();
            let mut outputs: Vec<PortView> = d
                .outputs
                .iter()
                .enumerate()
                .map(|(port, spec)| PortView {
                    badge: badges.get(&OutputRef { module: id, port }).copied().unwrap_or(0),
                    ..port_view(
                        spec,
                        controller.last_report().width(OutputRef { module: id, port }) == loom_core::Width::Stereo,
                    )
                })
                .collect();
            if let Some(unknown) = &node.unknown {
                let port = |name: &str| PortView {
                    label: name.to_string(),
                    hover: HoverText::new(name, "Not known to this version of LoomDSP", name),
                    colour: signal_colour(SignalType::Modulation),
                    shape: PortShape::Round,
                    badge: 0,
                    follows_transport: None,
                };
                inputs = unknown.inputs.iter().map(|name| port(name)).collect();
                outputs = unknown
                    .outputs
                    .iter()
                    .enumerate()
                    .map(|(i, name)| PortView {
                        badge: badges.get(&OutputRef { module: id, port: i }).copied().unwrap_or(0),
                        ..port(name)
                    })
                    .collect();
            }
            ModuleView {
                size: sizes.get(&id.0).copied(),
                unknown: node.unknown.is_some(),
                id: id.0,
                title: node.unknown.as_ref().map_or_else(
                    || d.name.to_string(),
                    |unknown| unknown.label.clone().unwrap_or_else(|| unknown.kind.clone()),
                ),
                accent: family_colour(d.family),
                // The header's hover text: family first, so colour is
                // never the only cue (ADR-0029), and the type ID as the name.
                hover: loom_canvas::HoverText {
                    description: format!("{}. {}", d.family.label(), d.description),
                    name: Some(d.id.to_string()),
                },
                position: positions.get(&id.0).copied().unwrap_or(Point::ORIGIN),
                inputs,
                outputs,
                panel: if node.unknown.is_some() {
                    PanelView {
                        width: 10,
                        height: 4,
                        widgets: Vec::new(),
                    }
                } else {
                    panel_for(d, &node.params)
                },
            }
        })
        .collect();

    let cables = graph
        .cables()
        .filter_map(|(from, to)| {
            let source = graph.node(from.module)?;
            let target = graph.node(to.module)?;
            let from_signal = source
                .descriptor
                .outputs
                .get(from.port)
                .map_or(SignalType::Modulation, |port| port.signal);
            let to_signal = target
                .descriptor
                .inputs
                .get(to.port)
                .map_or(SignalType::Modulation, |port| port.signal);
            Some(CableView {
                from: (from.module.0, from.port),
                to: (to.module.0, to.port),
                colour_from: signal_colour(from_signal),
                colour_to: signal_colour(to_signal),
                delayed: delayed.contains(&to),
            })
        })
        .collect();

    // One line per assignment, to the control bound to its parameter.
    let modulations = graph
        .assignments()
        .filter_map(|(id, a)| {
            let d = graph.node(a.target)?.descriptor;
            let name = d.params.get(a.param)?.name;
            Some(ModulationView {
                id: id.0,
                from: (a.source.module.0, a.source.port),
                to: a.target.0,
                widget: d.panel.widgets.iter().position(|w| w.is_control() && w.bind == name),
                enabled: a.enabled,
            })
        })
        .collect();

    Scene::new(modules, cables).with_modulations(modulations)
}

#[cfg(test)]
mod tests;
