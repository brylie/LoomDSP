//! Builds the canvas scene from the engine graph.

use crate::clip::ClipHolds;
use iced::{Color, Point};
use loom_canvas::{
    CableView, ControlPart, HoverText, ModulationView, ModuleView, PanelValues, PanelView, PartText, PartValue,
    PortShape, PortView, Scene, TransportSignal, WidgetValue, WidgetView, WidgetViewKind,
};
use loom_core::ModuleNames;
use loom_core::{
    Controller, DisplayKind, InputRef, ModuleDescriptor, ModuleId, OutputRef, ParamSpec, PortDefault, PortSpec,
    SignalType, WidgetKind,
};
use loom_patch::{Direction, End, Hierarchy, ModulePath, Tile};
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
        for p in d.params {
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
pub fn update_panel_values(values: &mut PanelValues, controller: &Controller, shown: &ShownValues) {
    update_panel_values_dragging(values, controller, shown, &ClipHolds::default(), None);
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
                values.set(a.target.0, index, next);
            }
        }
    }
}

/// Puts each waiting pickup's ghost mark on its knob or fader. Other controls
/// have no single travel to mark (midi-learn.md, Mapping modes). Call it after
/// the values are updated, which clears the marks.
pub fn set_ghosts(values: &mut PanelValues, controller: &Controller, ghosts: &BTreeMap<(u32, usize), f32>) {
    for (&(module, param), &position) in ghosts {
        let Some(node) = controller.graph().node(ModuleId(module)) else {
            continue;
        };
        let Some(spec) = node.descriptor.params.get(param) else {
            continue;
        };
        for (index, widget) in node.descriptor.panel.widgets.iter().enumerate() {
            if widget.binds(spec.name)
                && matches!(widget.kind, WidgetKind::Knob | WidgetKind::Fader { .. })
                && let Some(value) = values.get(module, index)
            {
                let next = WidgetValue {
                    ghost: Some(position),
                    ..value.clone()
                };
                values.set(module, index, next);
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
                            clip: clips && hold.lit,
                            hold: if clips { hold.text() } else { String::new() },
                            ..WidgetValue::default()
                        }
                    }
                }
            };
            values.set(id.0, index, value);
        }
    }
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

/// The value line of a light's or meter's hover text: the label and the
/// value. A light shows its brightness as a number, or "lit" and "dark" for
/// a trigger. A meter shows its RMS level in dB, and a meter on a display value
/// shows that value. `position` is the value as stored.
fn display_text(d: &ModuleDescriptor, w: &loom_core::Widget, position: f32, right: Option<(f32, f32)>) -> String {
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
        return format!("{}: {position:.1} {}", display.label, display.range.unit)
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
        return match right {
            Some((rms, _)) => format!("{label}: L {}, R {}", level(position), level(rms)),
            None => format!("{label}: {}", level(position)),
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

/// As [`control_param`], for one value of a control. [`ControlPart::Main`] is
/// the control's parameter, and [`ControlPart::Second`] the second parameter
/// of an XY pad or a range. [`ControlPart::Both`] names no one parameter.
pub fn control_part(
    controller: &Controller,
    module: u32,
    widget: usize,
    part: ControlPart,
) -> Option<(usize, WidgetKind, ParamSpec)> {
    match part {
        ControlPart::Main => control_param(controller, module, widget),
        ControlPart::Both => None,
        ControlPart::Second => {
            let node = controller.graph().node(ModuleId(module))?;
            let w = node.descriptor.panel.widgets.get(widget)?;
            let index = node.descriptor.param_index(w.second()?)?;
            Some((index, w.kind, node.spec(index)?))
        }
    }
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
    let delayed: BTreeSet<InputRef> = controller.last_report().inserted_delays().map(|(_, to)| *to).collect();
    let on_level = |id: ModuleId| hierarchy.level_of(id) == *level;

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
                    panel_for(d, &node.params)
                },
            }
        })
        .collect();

    // The sub-patch and port tiles of the level. A port is coloured by the
    // signal behind it: the inner input it feeds, or the output behind it.
    let signal_into = |end: &End| -> SignalType {
        hierarchy
            .inputs_behind(graph, level, end)
            .first()
            .and_then(|(id, name)| {
                let node = graph.node(*id)?;
                node.input_index(name).and_then(|i| node.descriptor.inputs.get(i))
            })
            .map_or(SignalType::Modulation, |spec| spec.signal)
    };
    let signal_from = |end: &End| -> SignalType {
        hierarchy
            .source_behind(graph, level, end)
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
                        .map(|(_, port)| {
                            tile_port(port, signal_from(&(lid, port.clone())), "An output of the sub-patch.")
                        })
                        .collect(),
                    panel: PanelView {
                        width: 10,
                        height: 2,
                        widgets: Vec::new(),
                    },
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
                        vec![tile_port("in", signal_from(&(lid, "in".to_string())), description)],
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
        match hierarchy.tile(graph, id)? {
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
            Tile::Module => None,
        }
    };
    let cables = hierarchy
        .level_cables(graph, level)
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
                hierarchy
                    .inputs_behind(graph, level, &cable.to)
                    .first()
                    .and_then(|(id, name)| {
                        graph
                            .node(*id)?
                            .input_index(name)
                            .map(|port| InputRef { module: *id, port })
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

    // One line per assignment between two modules of the level, to the
    // control bound to its parameter. A line that crosses a boundary
    // waits for the surface panel (sub-patches.md, Editor model).
    let modulations = graph
        .assignments()
        .filter(|(_, a)| on_level(a.source.module) && on_level(a.target))
        .filter_map(|(id, a)| {
            let d = graph.node(a.target)?.descriptor;
            let name = d.params.get(a.param)?.name;
            Some(ModulationView {
                id: id.0,
                from: (a.source.module.0, a.source.port),
                to: a.target.0,
                widget: d.panel.widgets.iter().position(|w| w.is_control() && w.binds(name)),
                enabled: a.enabled,
            })
        })
        .collect();

    Scene::new(modules, cables).with_modulations(modulations)
}

#[cfg(test)]
mod tests;
