//! Builds the canvas scene from the engine graph.

use crate::clip::ClipHolds;
use loom_canvas::{
    CableView, HoverText, ModulationView, ModuleView, PanelValues, PanelView, PartText, PartValue, PortShape, PortView,
    Reading, Scene, TransportSignal, WidgetValue, WidgetView, WidgetViewKind,
};
use loom_core::ModuleNames;
use loom_core::{
    Controller, DisplayKind, InputRef, ModuleDescriptor, ModuleId, OutputRef, ParamSpec, PortDefault, PortSpec,
    SignalType, WidgetKind,
};
use loom_patch::{Direction, End, Hierarchy, ModulePath, Tile};
use loom_ui_core::{Color, Point};
use std::collections::{BTreeMap, BTreeSet};

/// Segments across a result preview. About one world unit per segment on a
/// 12-grid-unit tile keeps its line smooth at close zoom.
const SHAPE_SEGMENTS: usize = 128;

/// Light and meter values by module and widget, including the right meter side.
pub type ShownValues = BTreeMap<(u32, usize), (f32, f32, Option<(f32, f32)>)>;

/// Port and cable colour for a signal type (ADR-0007).
pub fn signal_colour(signal: SignalType) -> Color {
    let [r, g, b] = signal.colour();
    Color::rgb8(r, g, b)
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

fn port_view(spec: &PortSpec, width: loom_core::Width) -> PortView {
    let (min, max) = spec.nominal_range();
    let mut hover = HoverText::new(
        spec.label,
        &format!("{} Usually {min} to {max}.", spec.description),
        spec.name,
    );
    append_width(&mut hover, width);
    PortView {
        label: spec.label.to_string(),
        hover,
        colour: signal_colour(spec.signal),
        shape: signal_shape(spec.signal),
        badge: 0,
        follows_transport: None,
    }
}

/// Adds the same width hint to direct and sub-patch output ports.
fn append_width(hover: &mut HoverText, width: loom_core::Width) {
    if width.voices > 1 {
        hover.description.push_str(&format!(
            " {} voices{}.",
            width.voices,
            if width.sides == 2 { ", stereo" } else { "" }
        ));
    } else if width.sides == 2 {
        hover.description.push_str(" Stereo.");
    }
}

/// The transport signal an input follows while it has no cable, if its
/// default is one (clock.md, Default inputs).
fn transport_default(spec: &PortSpec) -> Option<TransportSignal> {
    match spec.default {
        PortDefault::BarPhase => Some(TransportSignal::BarPhase),
        PortDefault::BeatPhase => Some(TransportSignal::BeatPhase),
        PortDefault::Playing => Some(TransportSignal::Playing),
        PortDefault::Value(_) | PortDefault::OwnPhase => None,
    }
}

/// One parameter's text, for an edge of an XY pad or a handle of a range.
fn part_text(d: &ModuleDescriptor, name: &str) -> PartText {
    d.params
        .iter()
        .find(|p| p.name == name)
        .map_or_else(PartText::default, |p| PartText {
            label: p.label.to_string(),
            hover: HoverText::new(p.label, p.description, p.name),
        })
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
            // Whole-number values a number control steps through, or 0.
            let steps = || {
                d.params
                    .iter()
                    .find(|p| p.name == w.bind())
                    .filter(|p| p.options.integer)
                    .map_or(0, |p| (p.max - p.min) as u32)
            };
            let kind = match w.kind {
                WidgetKind::Knob => WidgetViewKind::Knob { steps: steps() },
                WidgetKind::Number => WidgetViewKind::Number {
                    steps: steps(),
                    key_step: d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind())
                        .and_then(|p| Some(p.options.key_step? / (p.max - p.min)))
                        .unwrap_or(0.0),
                },
                WidgetKind::Fader { vertical } => WidgetViewKind::Fader {
                    vertical,
                    steps: steps(),
                },
                WidgetKind::XyPad { y } => WidgetViewKind::XyPad {
                    parts: Box::new([part_text(d, w.bind()), part_text(d, y)]),
                },
                WidgetKind::Range { high, vertical } => WidgetViewKind::Range {
                    vertical,
                    parts: Box::new([part_text(d, w.bind()), part_text(d, high)]),
                },
                WidgetKind::Toggle => WidgetViewKind::Toggle,
                WidgetKind::Meter => WidgetViewKind::Meter {
                    zero: d.meter_display(w).map_or(0.0, |display| display.range.zero()),
                    clips: d.meter_display(w).is_none(),
                },
                WidgetKind::Readout { width } => WidgetViewKind::Readout { width },
                WidgetKind::Scope => WidgetViewKind::Scope,
                WidgetKind::LevelHistory => WidgetViewKind::LevelHistory,
                WidgetKind::Spectrum => WidgetViewKind::Spectrum,
                WidgetKind::XyScope { .. } => WidgetViewKind::XyScope,
                WidgetKind::Shape { width, .. } => WidgetViewKind::Shape { width },
                // The names come from the parameter (panels.md, Choice names).
                WidgetKind::Radio => WidgetViewKind::Radio {
                    options: d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind())
                        .map_or_else(Vec::new, |p| p.options.choices.iter().map(|o| o.to_string()).collect()),
                },
                WidgetKind::Select => WidgetViewKind::Select {
                    options: d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind())
                        .map_or_else(Vec::new, ParamSpec::value_names),
                },
                WidgetKind::Segmented { width } => WidgetViewKind::Segmented {
                    options: d
                        .params
                        .iter()
                        .find(|p| p.name == w.bind())
                        .map_or_else(Vec::new, |p| p.options.choices.iter().map(|o| o.to_string()).collect()),
                    width,
                },
                WidgetKind::Button { .. } => WidgetViewKind::Button,
                // Light, and any kind this app does not draw yet.
                _ => WidgetViewKind::Light,
            };
            let (label, hover) = if w.is_control() {
                let param = d.params.iter().find(|p| p.name == w.bind());
                param.map_or((w.bind(), HoverText::default()), |p| {
                    (p.label, HoverText::new(p.label, p.description, p.name))
                })
            } else if let WidgetKind::Shape { label, .. } = w.kind {
                let output = d.outputs.iter().find(|p| p.name == w.bind());
                output.map_or((label, HoverText::default()), |p| {
                    (label, HoverText::new(label, p.description, p.name))
                })
            } else if w.shows_output() && d.meter_display(w).is_none() {
                // A display's text is its output's: two texts for one signal
                // could disagree. A display drawn in a frame has no caption:
                // the frame fills the widget.
                let output = d.outputs.iter().find(|p| p.name == w.bind());
                let framed = matches!(
                    w.kind,
                    WidgetKind::Scope | WidgetKind::LevelHistory | WidgetKind::Spectrum | WidgetKind::XyScope { .. }
                );
                let caption = |label| if framed { "" } else { label };
                output.map_or((caption(w.bind()), HoverText::default()), |p| {
                    (caption(p.label), HoverText::new(p.label, p.description, p.name))
                })
            } else {
                let display = d.displays.iter().find(|v| v.name == w.bind());
                display.map_or((w.bind(), HoverText::default()), |v| {
                    (v.label, HoverText::new(v.label, v.description, v.name))
                })
            };
            WidgetView {
                kind,
                x: w.x,
                y: w.y,
                // A widget can caption itself, such as "1" under a mixer
                // strip; the hover text keeps the full label. A meter's peak
                // hold readout takes its caption's row.
                label: if w.kind == WidgetKind::Meter && d.meter_display(w).is_none() {
                    String::new()
                } else {
                    w.caption.unwrap_or(label).to_string()
                },
                hover,
                hidden: false,
                exposed: false,
                inert: false,
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
    let displays: Vec<bool> = d
        .panel
        .widgets
        .iter()
        .map(|w| !w.is_control() && !d.is_widget_shown(w, values))
        .collect();
    if displays.iter().any(|&h| h) {
        close_up_columns(&mut panel, &displays);
    }
    let hidden: Vec<bool> = d
        .panel
        .widgets
        .iter()
        .map(|w| w.is_control() && d.param_index(w.bind()).is_some_and(|i| !d.is_shown(i, values)))
        .collect();
    if hidden.iter().any(|&h| h) {
        close_up(&mut panel, &hidden);
    }
    panel
}

/// Hides the marked displays and removes the grid columns only they used, so
/// the panel gets narrower (panel-widgets.md, Level history). A hidden
/// display stays in the list, past the panel's new edge.
fn close_up_columns(panel: &mut PanelView, hidden: &[bool]) {
    for (widget, &h) in panel.widgets.iter_mut().zip(hidden) {
        widget.hidden = h;
    }
    let covers = |w: &WidgetView, c: u8| w.x <= c && c < w.x + w.size().0;
    let mut removed = 0;
    for c in (0..panel.width).rev() {
        let by_hidden = panel.widgets.iter().any(|w| w.hidden && covers(w, c));
        let by_shown = panel.widgets.iter().any(|w| !w.hidden && covers(w, c));
        if by_hidden && !by_shown {
            for w in panel.widgets.iter_mut().filter(|w| !w.hidden && w.x > c) {
                w.x -= 1;
            }
            removed += 1;
        }
    }
    panel.width -= removed;
    // Past the new edge, so nothing takes it for a widget beside another.
    for widget in panel.widgets.iter_mut().filter(|w| w.hidden) {
        widget.x = widget.x.max(panel.width);
    }
}

/// The width and height of a module's panel at its default values, in grid
/// units, which is the size a new tile gets.
pub fn default_panel_size(d: &ModuleDescriptor) -> (u8, u8) {
    let values: Vec<f32> = d.params.iter().map(|p| p.default).collect();
    let panel = panel_for(d, &values);
    (panel.width, panel.height)
}

/// Hides the marked widgets and closes the panel up around them, keeping
/// its width (ADR-0030). A row that loses a widget closes its gap and is
/// centred again in the space the row had. Grid rows that only hidden
/// widgets used are removed, so the panel gets shorter.
pub fn close_up(panel: &mut PanelView, hidden: &[bool]) {
    for (widget, &h) in panel.widgets.iter_mut().zip(hidden) {
        widget.hidden |= h;
    }
    // Close up each row that lost a widget: rows are the widgets that start
    // on the same grid row.
    let rows: std::collections::BTreeSet<u8> = panel
        .widgets
        .iter()
        .zip(hidden)
        .filter(|(_, h)| **h)
        .map(|(w, _)| w.y)
        .collect();
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
        for (index, p) in d.params.iter().enumerate() {
            if p.options.shapes_graph {
                key.push((id.0, index, node.params[index].round() as i64));
            }
            if let Some(when) = p.options.shown_when
                && let Some(mode) = d.param_index(when.param)
            {
                let value = node.params.get(mode).copied().unwrap_or(d.params[mode].default);
                key.push((id.0, mode, value.round() as i64));
            }
        }
        for w in d.panel.widgets {
            if let Some(when) = w.shown_when
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
    if kind.is_mapped() {
        spec.normalize(value)
    } else {
        value - spec.min
    }
}

/// The parameter value for a control position from the canvas.
pub fn control_value(kind: WidgetKind, spec: &ParamSpec, position: f32) -> f32 {
    if !kind.is_mapped() {
        return spec.clamp(spec.min + position.round());
    }
    let value = spec.denormalize(position);
    // A declared step lands on tenths of itself, because adding steps as
    // positions leaves rounding errors: 121 BPM would become 120.99998
    // (panel-widgets.md, Number). Multiplying and then dividing keeps tenths
    // exact where a multiply by 0.1 would not.
    match spec.options.key_step {
        Some(step) => {
            let per_unit = 10.0 / step;
            spec.clamp((value * per_unit).round() / per_unit)
        }
        None => value,
    }
}

/// Updates the live value of every panel widget: parameter values and
/// modulation marks for controls, and `shown` (position and peak, by module
/// and widget) for lights and meters. Values that did not change keep the generation, so the
/// canvas uploads nothing new.
pub fn update_panel_values(
    values: &mut PanelValues,
    controller: &Controller,
    hierarchy: &Hierarchy,
    shown: &ShownValues,
) {
    update_panel_values_dragging(values, controller, hierarchy, shown, &ClipHolds::default(), None);
}

/// Updates only the live dots, for the refresh on every frame: the knobs
/// bound to a parameter with an enabled assignment, on module tiles and on
/// the sub-patch surfaces that expose the parameter. Values that did not
/// change leave the panel generation alone, so the canvas rebuilds nothing.
pub fn update_live_values(values: &mut PanelValues, controller: &Controller, hierarchy: &Hierarchy) {
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
        crate::surface::surface_live_values(values, hierarchy, (a.target, name), live);
        for (index, w) in d.panel.widgets.iter().enumerate() {
            if !w.kind.has_range() || !w.binds(name) {
                continue;
            }
            let Some(value) = values.get(a.target.0, index) else {
                continue;
            };
            let mut next = value.clone();
            match next.second.as_mut().filter(|_| w.second() == Some(name)) {
                Some(second) => second.live = live,
                None => next.live = live,
            }
            if &next != value {
                if let Some(slot) = hierarchy.slot_of_part(a.target) {
                    values.set(slot.0, index, next.clone());
                }
                values.set(a.target.0, index, next);
            }
        }
    }
}

/// Puts each waiting pickup's ghost mark on its knob or fader, and on the
/// surface controls of every sub-patch level that exposes it, each at the
/// hardware's value on its own scale (sub-patches.md, Exposing). Other
/// controls have no single travel to mark (midi-learn.md, Mapping modes).
/// Call it after the values are updated, which clears the marks.
pub fn set_ghosts(
    values: &mut PanelValues,
    controller: &Controller,
    hierarchy: &Hierarchy,
    ghosts: &BTreeMap<(u32, usize), f32>,
) {
    let graph = controller.graph();
    let mark = |values: &mut PanelValues, tile: u32, widget: usize, kind: WidgetKind, at: f32| {
        if matches!(kind, WidgetKind::Knob | WidgetKind::Fader { .. })
            && let Some(value) = values.get(tile, widget)
        {
            let next = WidgetValue {
                ghost: Some(at.clamp(0.0, 1.0)),
                ..value.clone()
            };
            values.set(tile, widget, next);
        }
    };
    for (&(module, param), &value) in ghosts {
        let Some(node) = graph.node(ModuleId(module)) else {
            continue;
        };
        let (Some(spec), Some(knob)) = (node.descriptor.params.get(param), node.spec(param)) else {
            continue;
        };
        for (index, widget) in node.descriptor.panel.widgets.iter().enumerate() {
            if widget.binds(spec.name) {
                mark(
                    values,
                    module,
                    index,
                    widget.kind,
                    control_position(widget.kind, &knob, value),
                );
            }
        }
        // The surfaces above it, as `surface::surface_live_values` walks them.
        let mut at = (ModuleId(module), spec.name.to_string());
        while let Some((instance, name)) = hierarchy.exposure_of(at.0, &at.1) {
            let index = hierarchy
                .definition_of(instance)
                .and_then(|d| d.exposed.iter().position(|e| e.name == name));
            if let Some(index) = index
                && let Some(target) =
                    crate::surface::control_target(graph, hierarchy, instance.0, index, loom_canvas::ControlPart::Main)
            {
                let position = control_position(target.kind, &target.spec, value);
                mark(values, instance.0, index, target.kind, position);
            }
            at = (instance, name);
        }
    }
}

/// As [`update_panel_values`], while the arc of a knob is dragged: that
/// knob's arc shows only `dragging`, the assignment being changed, so the
/// user sees what moves (`docs/design/modulation.md`, Arc drag).
pub fn update_panel_values_dragging(
    values: &mut PanelValues,
    controller: &Controller,
    hierarchy: &Hierarchy,
    shown: &ShownValues,
    holds: &ClipHolds,
    dragging: Option<loom_core::AssignmentId>,
) {
    let reaches = crate::modulation::reaches(controller.graph());
    let sample_rate = controller.config().sample_rate;
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
                            let y = sample(&node.params, i as f32 / SHAPE_SEGMENTS as f32, sample_rate);
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
                        // Only a control with a range shows the live value,
                        // and only while an assignment is on (modulation.md,
                        // Live value).
                        let live = (w.kind.has_range() && all.iter().any(|r| r.enabled))
                            .then(|| controller.modulated(id, p))
                            .flatten();
                        // The second value of an XY pad or a range, worked out
                        // the same way from its own parameter.
                        let second = w.second().and_then(|name| d.param_index(name)).and_then(|q| {
                            let spec = node.spec(q)?;
                            let all = reaches.get(&(id, q)).map_or(&[][..], Vec::as_slice);
                            let assigned = match &dragged {
                                Some((key, reach)) if *key == (id, q) => std::slice::from_ref(reach),
                                _ => all,
                            };
                            let value = node.params[q];
                            Some(PartValue {
                                position: control_position(w.kind, &spec, value),
                                text: crate::param::format(&spec, value),
                                modulation: crate::modulation::mark(w.kind, &spec, value, assigned),
                                live: all
                                    .iter()
                                    .any(|r| r.enabled)
                                    .then(|| controller.modulated(id, q))
                                    .flatten(),
                            })
                        });
                        // While a cable drives the Transport's run input, the
                        // Run toggle shows the state it sets (clock.md).
                        let value = driven_run(controller, id, w).unwrap_or(value);
                        WidgetValue {
                            position: control_position(w.kind, spec, value),
                            peak: 0.0,
                            right: None,
                            text: both_values_text(w.kind, crate::param::format(spec, value), second.as_ref()),
                            modulation: crate::modulation::mark(w.kind, spec, value, assigned),
                            live,
                            ghost: None,
                            shape: Vec::new(),
                            second,
                            ..WidgetValue::default()
                        }
                    }
                    _ => {
                        let (stored, peak, right) = shown.get(&(id.0, index)).copied().unwrap_or_default();
                        // A display value arrives as stored. A meter draws its
                        // place in the range (panel-widgets.md), and its text
                        // says the value, even past the end of the range.
                        let position = d
                            .meter_display(w)
                            .map_or(stored, |display| display.range.position(stored));
                        let reading = meter_reading(d, w, stored, right);
                        let mut text = display_text(d, w, stored, right);
                        let clips = w.kind == WidgetKind::Meter && d.meter_display(w).is_none();
                        let hold = holds.get((id.0, index));
                        if clips && hold.lit {
                            text.push_str(". Would clip at the output");
                        }
                        WidgetValue {
                            position,
                            peak,
                            right,
                            text,
                            reading,
                            clip: clips && hold.lit,
                            hold: if clips { hold.text() } else { String::new() },
                            ..WidgetValue::default()
                        }
                    }
                }
            };
            // A slot tile draws its part's panel under its own id.
            if let Some(slot) = hierarchy.slot_of_part(id) {
                values.set(slot.0, index, value.clone());
            }
            values.set(id.0, index, value);
        }
    }
    crate::surface::surface_values(values, controller, hierarchy, &reaches, dragged.as_ref());
}

/// A control's value as text. A widget that binds two says both, as a screen
/// reader reads them: "0.20 to 0.70" on a range, and "0.20, 0.70" on an XY pad.
fn both_values_text(kind: WidgetKind, first: String, second: Option<&PartValue>) -> String {
    match (second, kind) {
        (Some(second), WidgetKind::Range { .. }) => format!("{first} to {}", second.text),
        (Some(second), _) => format!("{first}, {}", second.text),
        (None, _) => first,
    }
}

/// For the Transport's Run toggle while a cable drives its `run` input:
/// 1 or 0, from the module's `running` display value. `None` otherwise.
fn driven_run(controller: &Controller, id: ModuleId, w: &loom_core::Widget) -> Option<f32> {
    if controller.transport_module() != Some(id) || !w.binds("run") {
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

/// The one number a meter's text says, with the range the meter draws, or
/// `None` for any other widget and for a meter whose text names no single
/// number: a stereo meter says two, and a level under the meter's floor is
/// "below -60 dB". [`display_text`] writes its number from this, so the
/// text and the number a screen reader is given come from one place.
fn meter_reading(
    d: &ModuleDescriptor,
    w: &loom_core::Widget,
    stored: f32,
    right: Option<(f32, f32)>,
) -> Option<Reading> {
    if let Some(display) = d.meter_display(w) {
        // The text says the value even past the end of the range, so the
        // bounds reach out to it: a number outside its own bounds would be
        // an invalid reading.
        let value = shown_number(stored);
        return Some(Reading {
            value,
            min: display.range.min.min(value),
            max: display.range.max.max(value),
        });
    }
    if w.kind != WidgetKind::Meter || right.is_some() || stored <= 0.0 {
        return None;
    }
    Some(Reading {
        value: shown_number(crate::live::position_db(stored)),
        min: crate::live::position_db(0.0),
        max: crate::live::position_db(1.0),
    })
}

/// A meter's value as its text writes it, with one decimal: the number
/// read back from that text, so the two agree in every digit. A value
/// that is not a number is passed on as it is.
fn shown_number(value: f32) -> f32 {
    format!("{value:.1}").parse().unwrap_or(value)
}

/// The value line of a light's or meter's hover text: the label and the
/// value. A light shows its brightness as a number, or "lit" and "dark" for
/// a trigger. A meter shows its RMS level in dB, and a meter on a display value
/// shows that value. `position` is the value as stored. A meter's number
/// is the one [`meter_reading`] gives.
fn display_text(d: &ModuleDescriptor, w: &loom_core::Widget, position: f32, right: Option<(f32, f32)>) -> String {
    let reading = meter_reading(d, w, position, right);
    if let WidgetKind::Readout { .. } = w.kind {
        // A readout's text is what it shows. The registry checked its binding.
        return d
            .displays
            .iter()
            .find(|display| display.name == w.bind())
            .map_or_else(String::new, |display| display.range.text(position));
    }
    if let WidgetKind::XyScope { y } = w.kind {
        // What a screen reader says: the two inputs that feed it.
        let label = |output: &str| {
            let port = d.outputs.iter().position(|p| p.name == output);
            port.and_then(|port| d.inputs.get(port))
                .map_or(output, |input| input.label)
                .to_string()
        };
        return format!("XY scope of {} and {}", label(w.bind()), label(y));
    }
    if matches!(
        w.kind,
        WidgetKind::Scope | WidgetKind::LevelHistory | WidgetKind::Spectrum
    ) {
        // The trace is the value.
        return String::new();
    }
    if let Some(display) = d.meter_display(w) {
        let value = reading.map_or(position, |reading| reading.value);
        return format!("{}: {value:.1} {}", display.label, display.range.unit)
            .trim_end()
            .to_string();
    }
    if w.kind == WidgetKind::Meter {
        let label = d
            .outputs
            .iter()
            .find(|p| p.name == w.bind())
            .map_or(w.bind(), |p| p.label);
        let level = |position| {
            if position > 0.0 {
                format!("{:.1} dB", crate::live::position_db(position))
            } else {
                "below -60 dB".to_string()
            }
        };
        return match (right, reading) {
            (Some((rms, _)), _) => format!("{label}: L {}, R {}", level(position), level(rms)),
            (None, Some(reading)) => format!("{label}: {:.1} dB", reading.value),
            (None, None) => format!("{label}: {}", level(position)),
        };
    }
    let Some(display) = d.displays.iter().find(|v| v.name == w.bind()) else {
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
                    let port = d.outputs.iter().position(|p| p.name == w.bind())?;
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

/// The time a level history shows, by its `window` choice
/// (panel-widgets.md, Level history).
const HISTORY_WINDOWS: [f32; 3] = [1.0, 5.0, 30.0];

/// A level history on screen and what it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HistoryOutput {
    /// Module ID and widget index.
    pub widget: (u32, usize),
    /// The output it shows.
    pub output: OutputRef,
    /// Seconds across the frame.
    pub window: f32,
}

/// The level histories these modules show now: a hidden one needs no probe.
pub fn history_outputs(controller: &Controller, modules: &[u32]) -> Vec<HistoryOutput> {
    let mut histories = Vec::new();
    for &id in modules {
        let Some(node) = controller.graph().node(ModuleId(id)) else {
            continue;
        };
        let d = node.descriptor;
        let window = d.param_index("window").map_or(HISTORY_WINDOWS[1], |i| {
            HISTORY_WINDOWS[(node.params[i].round().max(0.0) as usize).min(HISTORY_WINDOWS.len() - 1)]
        });
        for (index, w) in d.panel.widgets.iter().enumerate() {
            if w.kind != WidgetKind::LevelHistory || !d.is_widget_shown(w, &node.params) {
                continue;
            }
            let Some(port) = d.outputs.iter().position(|p| p.name == w.bind()) else {
                continue;
            };
            histories.push(HistoryOutput {
                widget: (id, index),
                output: OutputRef {
                    module: ModuleId(id),
                    port,
                },
                window,
            });
        }
    }
    histories
}

/// The value at each edge of an XY scope, by its `range` choice
/// (panel-widgets.md, XY scope).
const XY_RANGES: [f32; 3] = [0.1, 1.0, 10.0];

/// An XY scope on screen and what it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XyOutput {
    /// Module ID and widget index.
    pub widget: (u32, usize),
    /// The output drawn across.
    pub x: OutputRef,
    /// The output drawn up.
    pub y: OutputRef,
    /// The input that feeds `y`, if the module has one at its port.
    pub y_input: Option<loom_core::InputRef>,
    /// Seconds of history.
    pub time: f32,
    /// The value at each edge of the frame.
    pub range: f32,
    /// True to turn the trace by 45 degrees.
    pub rotate: bool,
}

/// The XY scopes on these modules (panel-widgets.md, XY scope).
pub fn xy_outputs(controller: &Controller, modules: &[u32]) -> Vec<XyOutput> {
    let mut scopes = Vec::new();
    for &id in modules {
        let Some(node) = controller.graph().node(ModuleId(id)) else {
            continue;
        };
        let d = node.descriptor;
        let param = |name: &str, default: f32| d.param_index(name).map_or(default, |i| node.params[i]);
        let module = ModuleId(id);
        for (index, w) in d.panel.widgets.iter().enumerate() {
            let WidgetKind::XyScope { y } = w.kind else {
                continue;
            };
            let port = |name: &str| d.outputs.iter().position(|p| p.name == name);
            let (Some(x_port), Some(y_port)) = (port(w.bind()), port(y)) else {
                continue;
            };
            let choice = param("range", 1.0).round().max(0.0) as usize;
            scopes.push(XyOutput {
                widget: (id, index),
                x: OutputRef { module, port: x_port },
                y: OutputRef { module, port: y_port },
                y_input: (y_port < d.inputs.len()).then_some(loom_core::InputRef { module, port: y_port }),
                time: param("time", 0.02),
                range: XY_RANGES[choice.min(XY_RANGES.len() - 1)],
                rotate: param("rotate", 0.0) >= 0.5,
            });
        }
    }
    scopes
}

/// A spectrum on screen and what it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectrumOutput {
    /// Module ID and widget index.
    pub widget: (u32, usize),
    /// The output it shows.
    pub output: OutputRef,
    /// True to tilt the drawing by +4.5 dB per octave.
    pub tilt: bool,
    /// True to draw the peak hold line.
    pub hold: bool,
}

/// The spectra on these modules (panel-widgets.md, Spectrum). The drawing's
/// settings come from the module's `tilt` and `hold` parameters.
pub fn spectrum_outputs(controller: &Controller, modules: &[u32]) -> Vec<SpectrumOutput> {
    let mut spectra = Vec::new();
    for &id in modules {
        let Some(node) = controller.graph().node(ModuleId(id)) else {
            continue;
        };
        let d = node.descriptor;
        let on = |name: &str| d.param_index(name).is_some_and(|i| node.params[i] >= 0.5);
        for (index, w) in d.panel.widgets.iter().enumerate() {
            if w.kind != WidgetKind::Spectrum {
                continue;
            }
            let Some(port) = d.outputs.iter().position(|p| p.name == w.bind()) else {
                continue;
            };
            spectra.push(SpectrumOutput {
                widget: (id, index),
                output: OutputRef {
                    module: ModuleId(id),
                    port,
                },
                tilt: on("tilt"),
                hold: on("hold"),
            });
        }
    }
    spectra
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
    /// The edge that starts the trace: from the module's `slope`, or rising.
    pub slope: crate::live::Slope,
}

/// The scopes on these modules (`docs/design/panels.md`, Scope module).
/// The window and trigger come from the module's `window`, `auto`, `level`,
/// and `slope` parameters, when it has them.
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
            let Some(port) = d.outputs.iter().position(|p| p.name == w.bind()) else {
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
                slope: crate::live::Slope::from_param(param("slope", 0.0)),
            });
        }
    }
    scopes
}

/// Builds the scene of a flat patch at its top level, for tests.
#[cfg(test)]
pub fn build_flat(
    controller: &Controller,
    positions: &BTreeMap<u32, (f32, f32)>,
    sizes: &BTreeMap<u32, (u8, u8)>,
) -> Scene {
    let mut hierarchy = Hierarchy::flat();
    hierarchy.adopt(controller.graph());
    build(controller, &hierarchy, &ModulePath::root(), positions, sizes)
}

/// Builds the scene of one level: its modules as tiles, each nested
/// sub-patch as a tile with its name and ports, each Patch input and
/// output of the level as a port tile, and the cables between them as the
/// level shows them (sub-patches.md, Editor model). A modulation line is
/// drawn only between two modules of the level.
pub fn build(
    controller: &Controller,
    hierarchy: &Hierarchy,
    level: &ModulePath,
    positions: &BTreeMap<u32, (f32, f32)>,
    sizes: &BTreeMap<u32, (u8, u8)>,
) -> Scene {
    let graph = controller.graph();
    // Follows through slots and ports, kept for this build, so a long chain
    // of empty thru slots is walked once, not once per end on it.
    let behind = loom_patch::hierarchy::behind::Behind::new(hierarchy, graph);
    let delayed: BTreeSet<InputRef> = controller.last_report().inserted_delays().map(|(_, to)| *to).collect();
    // A slot's part, a part that is leaving, and a swap fade are hidden
    // behind the slot's tile (sub-patches.md, Slots).
    let hidden = hierarchy.hidden();
    let on_level = |id: ModuleId| hierarchy.level_of(id) == *level && !hidden.contains(id);

    // Assignments each output drives, counting those that are off.
    let mut badges: BTreeMap<OutputRef, u32> = BTreeMap::new();
    for (_, a) in graph.assignments() {
        *badges.entry(a.source).or_default() += 1;
    }
    // Inputs with a cable: they no longer follow their default, so they
    // show no clock mark (panels.md, Clock mark on transport inputs).
    let cabled: BTreeSet<InputRef> = graph.cables().map(|(_, to)| to).collect();
    let mut modules: Vec<ModuleView> = graph
        .nodes()
        .filter(|(id, _)| on_level(*id))
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
                            .map_or(loom_core::Width::MONO, |source| controller.last_report().width(source)),
                    )
                })
                .collect();
            let mut outputs: Vec<PortView> = d
                .outputs
                .iter()
                .enumerate()
                .map(|(port, spec)| PortView {
                    badge: badges.get(&OutputRef { module: id, port }).copied().unwrap_or(0),
                    ..port_view(spec, controller.last_report().width(OutputRef { module: id, port }))
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
                // Only a placeholder shows its label for now; known modules
                // keep theirs without showing it (clipboard.md, rule 9, #526).
                title: node.unknown.as_ref().map_or_else(
                    || d.name.to_string(),
                    |unknown| node.label.clone().unwrap_or_else(|| unknown.kind.clone()),
                ),
                accent: family_colour(d.family),
                // The header's hover text: family first, so colour is
                // never the only cue (ADR-0029), and the type ID as the name.
                hover: loom_canvas::HoverText {
                    description: format!("{}. {}", d.family.label(), d.description),
                    name: Some(d.id.to_string()),
                    note: None,
                },
                position: positions.get(&id.0).map_or(Point::ORIGIN, |&(x, y)| Point::new(x, y)),
                inputs,
                outputs,
                panel: if node.unknown.is_some() {
                    PanelView {
                        width: 10,
                        height: 4,
                        widgets: Vec::new(),
                    }
                } else {
                    let mut panel = panel_for(d, &node.params);
                    // A control exposed on the sub-patch's surface gets its
                    // mark and says so in its hover text (sub-patches.md,
                    // Exposing).
                    for (w, view) in d.panel.widgets.iter().zip(panel.widgets.iter_mut()) {
                        if w.is_control()
                            && let Some((_, surface_name)) = hierarchy.exposure_of(id, w.bind())
                        {
                            view.exposed = true;
                            view.hover.note = Some(format!("Exposed as {surface_name}"));
                        }
                    }
                    panel
                },
            }
        })
        .collect();

    let level_cables = hierarchy.level_cables(graph, level);
    // The sub-patch and port tiles of the level. A port is coloured by the
    // signal behind it: the inner input it feeds, or the output behind it.
    let signal_into = |end: &End| -> SignalType {
        behind
            .first_input(level, end)
            .and_then(|(id, name)| {
                let node = graph.node(id)?;
                node.input_index(&name).and_then(|i| node.descriptor.inputs.get(i))
            })
            .map_or(SignalType::Modulation, |spec| spec.signal)
    };
    let signal_from = |end: &End| -> SignalType {
        behind
            .source(level, end)
            .and_then(|(id, name)| {
                let node = graph.node(id)?;
                node.output_index(&name).and_then(|i| node.descriptor.outputs.get(i))
            })
            .map_or(SignalType::Modulation, |spec| spec.signal)
    };
    let tile_port = |name: &str, signal: SignalType, description: &str| PortView {
        label: name.to_string(),
        hover: HoverText::new(name, description, name),
        colour: signal_colour(signal),
        shape: signal_shape(signal),
        badge: 0,
        follows_transport: None,
    };
    let output_port = |name: &str, end: &End, description: &str| {
        let mut port = tile_port(name, signal_from(end), description);
        let width = behind
            .source(level, end)
            .and_then(|(id, name)| {
                Some(controller.last_report().width(OutputRef {
                    module: id,
                    port: graph.node(id)?.output_index(&name)?,
                }))
            })
            .unwrap_or_default();
        append_width(&mut port.hover, width);
        port
    };
    let place = |id: ModuleId| positions.get(&id.0).map_or(Point::ORIGIN, |&(x, y)| Point::new(x, y));
    let structure = family_colour(loom_core::Family::Interface);
    for (id, tile) in hierarchy.structural_tiles(level) {
        let lid = hierarchy.path_of(id).last();
        let view = match tile {
            Tile::Instance(_) => {
                let name = hierarchy.tile_name(id).unwrap_or_default();
                let (inputs, outputs) = hierarchy.instance_ports(id);
                ModuleView {
                    id: id.0,
                    title: name.clone(),
                    hover: HoverText {
                        description: format!("Sub-patch. {name} is a patch inside this one. Double-click to open it."),
                        name: Some(loom_patch::SUBPATCH_KIND.to_string()),
                        note: None,
                    },
                    accent: structure,
                    position: place(id),
                    size: sizes.get(&id.0).copied(),
                    unknown: false,
                    inputs: inputs
                        .iter()
                        .map(|(_, port)| {
                            tile_port(port, signal_into(&(lid, port.clone())), "An input of the sub-patch.")
                        })
                        .collect(),
                    outputs: outputs
                        .iter()
                        .map(|(_, port)| output_port(port, &(lid, port.clone()), "An output of the sub-patch."))
                        .collect(),
                    panel: crate::surface::surface_panel(graph, hierarchy, id),
                }
            }
            Tile::Port(direction, name) => {
                let d = controller.registry().resolve(direction.kind()).ok();
                let description = match direction {
                    Direction::Input => "A Patch input: what comes in through this port of the sub-patch.",
                    Direction::Output => "A Patch output: what goes out through this port of the sub-patch.",
                };
                let (inputs, outputs) = match direction {
                    Direction::Input => (
                        Vec::new(),
                        vec![tile_port("out", signal_into(&(lid, "out".to_string())), description)],
                    ),
                    Direction::Output => (
                        vec![
                            level_cables
                                .iter()
                                .find(|cable| cable.to == (lid, "in".to_string()))
                                .map_or_else(
                                    || tile_port("in", SignalType::Modulation, description),
                                    |cable| output_port("in", &cable.from, description),
                                ),
                        ],
                        Vec::new(),
                    ),
                };
                ModuleView {
                    id: id.0,
                    title: name,
                    hover: HoverText {
                        description: d.map_or_else(
                            || description.to_string(),
                            |d| format!("{}. {}", d.family.label(), d.description),
                        ),
                        name: Some(direction.kind().to_string()),
                        note: None,
                    },
                    accent: d.map_or(structure, |d| family_colour(d.family)),
                    position: place(id),
                    size: sizes.get(&id.0).copied(),
                    unknown: false,
                    inputs,
                    outputs,
                    panel: d.map_or_else(
                        || PanelView {
                            width: 10,
                            height: 2,
                            widgets: Vec::new(),
                        },
                        |d| panel_for(d, &[]),
                    ),
                }
            }
            Tile::Slot(_) => {
                let Some(state) = hierarchy.slot(id) else { continue };
                let name = hierarchy.tile_name(id).unwrap_or_default();
                let part = hierarchy.part_of_slot(id);
                let part_node = part.and_then(|part| graph.node(part));
                let part_name = part.and_then(|part| match part_node {
                    Some(node) => Some(node.descriptor.name.to_string()),
                    None => hierarchy.tile_name(part),
                });
                let note = loom_patch::hierarchy::no_part_text(state.role);
                let role_port = |port: &loom_core::RolePort, into: bool| PortView {
                    label: port.label.to_string(),
                    hover: HoverText::new(port.label, port.description, port.name),
                    colour: signal_colour(port.signal),
                    shape: signal_shape(port.signal),
                    badge: if into {
                        0
                    } else {
                        behind
                            .source(level, &(lid, port.name.to_string()))
                            .and_then(|(module, output)| {
                                let index = graph.node(module)?.output_index(&output)?;
                                badges.get(&OutputRef { module, port: index }).copied()
                            })
                            .unwrap_or(0)
                    },
                    follows_transport: None,
                };
                let panel = match (part, part_node) {
                    (Some(part), Some(node)) => {
                        let mut panel = panel_for(node.descriptor, &node.params);
                        for (w, view) in node.descriptor.panel.widgets.iter().zip(panel.widgets.iter_mut()) {
                            if w.is_control()
                                && let Some((_, surface_name)) = hierarchy.exposure_of(part, w.bind())
                            {
                                view.exposed = true;
                                view.hover.note = Some(format!("Exposed as {surface_name}"));
                            }
                        }
                        panel
                    }
                    (Some(part), None) => crate::surface::surface_panel(graph, hierarchy, part),
                    // An empty slot draws its note, one sentence a line, as
                    // text that is not a control (sub-patches.md, Empty slots).
                    (None, _) => {
                        let lines: Vec<&str> = note.split_inclusive(". ").map(str::trim).collect();
                        PanelView {
                            width: 10,
                            height: lines.len().max(3) as u8,
                            widgets: lines
                                .iter()
                                .enumerate()
                                .map(|(row, line)| WidgetView {
                                    kind: WidgetViewKind::Note { width: 10 },
                                    x: 0,
                                    y: row as u8,
                                    label: (*line).to_string(),
                                    hover: HoverText::default(),
                                    hidden: false,
                                    exposed: false,
                                    inert: false,
                                })
                                .collect(),
                        }
                    }
                };
                ModuleView {
                    id: id.0,
                    title: name.clone(),
                    hover: HoverText {
                        description: match &part_name {
                            Some(part) => format!("Slot for {}. Holds {part}. Swap from its menu.", state.role.name),
                            None => format!("Slot for {}. {note}", state.role.name),
                        },
                        name: Some(state.role.kind()),
                        note: None,
                    },
                    accent: part_node.map_or(structure, |node| family_colour(node.descriptor.family)),
                    position: place(id),
                    size: sizes.get(&id.0).copied(),
                    unknown: false,
                    inputs: state.role.inputs.iter().map(|port| role_port(port, true)).collect(),
                    outputs: state.role.outputs.iter().map(|port| role_port(port, false)).collect(),
                    panel,
                }
            }
            Tile::Module => continue,
        };
        modules.push(view);
    }

    // The port an end of the level names on its tile: a module's by name,
    // an instance's by its place in the port list, a port tile's one port.
    let port_of = |end: &End, input: bool| -> Option<(u32, usize, SignalType)> {
        let id = hierarchy.id_at(level, end.0)?;
        if let Some(node) = graph.node(id) {
            let (index, signal) = if input {
                let i = node.input_index(&end.1)?;
                (
                    i,
                    node.descriptor
                        .inputs
                        .get(i)
                        .map_or(SignalType::Modulation, |p| p.signal),
                )
            } else {
                let i = node.output_index(&end.1)?;
                (
                    i,
                    node.descriptor
                        .outputs
                        .get(i)
                        .map_or(SignalType::Modulation, |p| p.signal),
                )
            };
            return Some((id.0, index, signal));
        }
        match hidden.tile(graph, id)? {
            Tile::Instance(_) => {
                let (inputs, outputs) = hierarchy.instance_ports(id);
                let list = if input { inputs } else { outputs };
                let index = list.iter().position(|(_, name)| *name == end.1)?;
                let signal = if input { signal_into(end) } else { signal_from(end) };
                Some((id.0, index, signal))
            }
            Tile::Port(direction, _) => {
                let fits = match direction {
                    Direction::Input => !input && end.1 == "out",
                    Direction::Output => input && end.1 == "in",
                };
                fits.then(|| (id.0, 0, if input { signal_from(end) } else { signal_into(end) }))
            }
            Tile::Slot(_) => {
                let state = hierarchy.slot(id)?;
                let list = if input { state.role.inputs } else { state.role.outputs };
                let index = list.iter().position(|port| port.name == end.1)?;
                Some((id.0, index, list[index].signal))
            }
            Tile::Module => None,
        }
    };
    let cables = level_cables
        .into_iter()
        .filter_map(|cable| {
            let (from_id, from_index, from_signal) = port_of(&cable.from, false)?;
            let (to_id, to_index, to_signal) = port_of(&cable.to, true)?;
            // A unit delay is marked where the engine put it: on a module's
            // input, or on the first inner input behind a port.
            let delayed_at = if graph.node(ModuleId(to_id)).is_some() {
                Some(InputRef {
                    module: ModuleId(to_id),
                    port: to_index,
                })
            } else {
                behind.first_input(level, &cable.to).and_then(|(id, name)| {
                    graph
                        .node(id)?
                        .input_index(&name)
                        .map(|port| InputRef { module: id, port })
                })
            };
            Some(CableView {
                from: (from_id, from_index),
                to: (to_id, to_index),
                colour_from: signal_colour(from_signal),
                colour_to: signal_colour(to_signal),
                delayed: delayed_at.is_some_and(|input| delayed.contains(&input)),
            })
        })
        .collect();

    // One line per assignment from a module of the level: to the control
    // bound to its parameter on a module of the level, or to the surface
    // control of the sub-patch tile that exposes an inner parameter
    // (sub-patches.md, Editor model).
    let modulations = graph
        .assignments()
        .filter(|(_, a)| on_level(a.source.module))
        .filter_map(|(id, a)| {
            let d = graph.node(a.target)?.descriptor;
            let name = d.params.get(a.param)?.name;
            let (to, widget) = if on_level(a.target) {
                (
                    a.target.0,
                    d.panel.widgets.iter().position(|w| w.is_control() && w.binds(name)),
                )
            } else {
                let (tile, index) = hierarchy.surface_for(level, &(a.target, name.to_string()))?;
                (tile.0, Some(index))
            };
            Some(ModulationView {
                id: id.0,
                from: (a.source.module.0, a.source.port),
                to,
                widget,
                enabled: a.enabled,
            })
        })
        .collect();

    let non_live: Vec<_> = modules
        .iter()
        .flat_map(|module| {
            (0..module.panel.widgets.len())
                .filter_map(|widget| {
                    let target = crate::surface::control_target(
                        graph,
                        hierarchy,
                        module.id,
                        widget,
                        loom_canvas::ControlPart::Main,
                    )?;
                    target.spec.options.shapes_graph.then_some((module.id, widget))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    Scene::new(modules, cables)
        .with_modulations(modulations)
        .with_non_live_controls(non_live)
}

#[cfg(test)]
mod tests;
