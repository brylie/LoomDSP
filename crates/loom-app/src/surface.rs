//! The surface of a sub-patch tile: the parameters its inside exposes,
//! shown as the widgets they have inside, and the one place that says
//! which parameter a control on any tile moves
//! (`docs/design/sub-patches.md`, Exposing; `docs/design/panels.md`,
//! Exposed controls in a sub-patch).
//!
//! Every control on the canvas is named by its tile id and widget index.
//! On a module tile that pair leads to the module's own parameter through
//! its descriptor. On a sub-patch tile it leads through the hierarchy to
//! an inner module's parameter, as [`ControlTarget`] says. Every path that
//! moves, types into, resets, assigns to, or arms a control resolves the
//! pair here, so a surface control and the inner control it stands for
//! are one parameter with two controls (FR-PANEL-03).

use crate::App;
use crate::modulation::Reach;
use loom_canvas::{ControlPart, HoverText, PanelValues, PanelView, WidgetValue, WidgetView, WidgetViewKind};
use loom_core::{Controller, Graph, ModuleId, ModuleNames, ParamRange, ParamSpec, WidgetKind};
use loom_patch::{Exposed, Hierarchy, ModulePath};
use std::collections::BTreeMap;

/// What a control on a tile moves: a flat module's parameter, and the
/// widget kind and spec the control works in.
#[derive(Debug, Clone)]
pub struct ControlTarget {
    /// The flat module whose parameter the control moves.
    pub module: ModuleId,
    /// The parameter's index on that module.
    pub index: usize,
    /// The kind of widget the control is.
    pub kind: WidgetKind,
    /// The parameter as the control maps it: within its selected control
    /// range on a module tile (control-ranges.md, rule 4), and within the
    /// exposed range on a sub-patch surface (sub-patches.md, Exposing).
    pub spec: ParamSpec,
}

/// The parameter the control `widget` of tile `tile` moves, or `None` when
/// the tile or widget is gone, or `part` names no one parameter.
/// [`ControlPart::Second`] is the second parameter of an XY pad or a range,
/// which only a module tile has.
pub fn control_target(
    graph: &Graph,
    hierarchy: &Hierarchy,
    tile: u32,
    widget: usize,
    part: ControlPart,
) -> Option<ControlTarget> {
    if let Some(node) = graph.node(ModuleId(tile)) {
        let d = node.descriptor;
        let w = d.panel.widgets.get(widget)?;
        let index = match part {
            // A button in momentary mode is bound to its hold, and
            // otherwise to its latch (clock.md, Button details).
            ControlPart::Main => d.control_param(w, &node.params)?,
            ControlPart::Second => d.param_index(w.second()?)?,
            ControlPart::Both => return None,
        };
        return Some(ControlTarget {
            module: ModuleId(tile),
            index,
            kind: w.kind,
            spec: node.spec(index)?,
        });
    }
    if part != ControlPart::Main {
        return None;
    }
    let exposed = hierarchy.exposed_target(ModuleId(tile), widget)?;
    let node = graph.node(exposed.module)?;
    let index = node.parameter_index(&exposed.param)?;
    let spec = node.spec(index)?;
    let spec = match exposed.range {
        Some((min, max)) => spec.in_range(ParamRange::new(min, max)),
        None => spec,
    };
    Some(ControlTarget {
        module: exposed.module,
        index,
        kind: surface_kind(graph, hierarchy, ModuleId(tile), widget),
        spec,
    })
}

/// The widget index of the control bound to `name` on tile `tile`: a
/// parameter name on a module tile, an exposed name on a sub-patch tile.
pub fn find_control(graph: &Graph, hierarchy: &Hierarchy, tile: u32, name: &str) -> Option<usize> {
    if let Some(node) = graph.node(ModuleId(tile)) {
        return node
            .descriptor
            .panel
            .widgets
            .iter()
            .position(|w| w.is_control() && w.binds(name));
    }
    hierarchy
        .definition_of(ModuleId(tile))?
        .exposed
        .iter()
        .position(|e| e.name == name)
}

/// True when widget `widget` of tile `tile` is a control, not a light or a
/// display. Every surface widget is a control until displays can be
/// exposed.
pub fn is_control(graph: &Graph, hierarchy: &Hierarchy, tile: u32, widget: usize) -> bool {
    match graph.node(ModuleId(tile)) {
        Some(node) => node
            .descriptor
            .panel
            .widgets
            .get(widget)
            .is_some_and(|w| w.is_control()),
        None => hierarchy.exposed_target(ModuleId(tile), widget).is_some(),
    }
}

/// True when widget `widget` of tile `tile` is bound to `name`: a parameter
/// or display value on a module tile, an exposed name on a sub-patch tile.
pub fn widget_binds(graph: &Graph, hierarchy: &Hierarchy, tile: u32, widget: usize, name: &str) -> bool {
    match graph.node(ModuleId(tile)) {
        Some(node) => node.descriptor.panel.widgets.get(widget).is_some_and(|w| w.binds(name)),
        None => hierarchy
            .definition_of(ModuleId(tile))
            .and_then(|d| d.exposed.get(widget))
            .is_some_and(|e| e.name == name),
    }
}

/// The name that finds widget `widget` of tile `tile` again: the bound
/// parameter or display value on a module tile, the exposed name on a
/// sub-patch tile.
pub fn widget_name(graph: &Graph, hierarchy: &Hierarchy, tile: u32, widget: usize) -> Option<String> {
    match graph.node(ModuleId(tile)) {
        Some(node) => Some(node.descriptor.panel.widgets.get(widget)?.bind().to_string()),
        None => Some(
            hierarchy
                .definition_of(ModuleId(tile))?
                .exposed
                .get(widget)?
                .name
                .clone(),
        ),
    }
}

/// The widget a surface control takes: the one the inner tile shows for
/// that parameter, when a control there binds it alone (panels.md, The
/// same widget on both sides). A parameter with no such control, because it
/// is shown only in the inspector or bound through a widget with two
/// values, takes the widget its kind of value gets (panels.md, Default
/// panels, Which widget). An exposure of a nested sub-patch's surface item
/// takes that surface's widget. A member that cannot be found gets a knob;
/// its control then moves nothing.
pub fn surface_kind(graph: &Graph, hierarchy: &Hierarchy, instance: ModuleId, index: usize) -> WidgetKind {
    let Some(definition) = hierarchy.definition_of(instance) else {
        return WidgetKind::Knob;
    };
    let Some(exposed) = definition.exposed.get(index) else {
        return WidgetKind::Knob;
    };
    let member = hierarchy.path_of(instance).child(exposed.module);
    let Some(member_id) = hierarchy.ids().get(&member) else {
        return WidgetKind::Knob;
    };
    if let Some(inner) = hierarchy.definition_of(member_id) {
        return match inner.exposed.iter().position(|e| e.name == exposed.param) {
            Some(inner_index) => surface_kind(graph, hierarchy, member_id, inner_index),
            None => WidgetKind::Knob,
        };
    }
    let Some(node) = graph.node(member_id) else {
        return WidgetKind::Knob;
    };
    let d = node.descriptor;
    if let Some(w) = d
        .panel
        .widgets
        .iter()
        .find(|w| w.is_control() && w.binds(&exposed.param) && w.params().count() == 1)
    {
        return w.kind;
    }
    node.parameter_index(&exposed.param)
        .and_then(|index| d.params.get(index))
        .map_or(WidgetKind::Knob, derived_kind)
}

/// The widget a kind of value gets when nothing else says (panels.md,
/// Default panels, Which widget): a toggle for a whole number from 0 to 1
/// with no choice names, a segmented control for 2 to 4 names, radio
/// buttons for 5, a select list for more, and a knob for anything else.
fn derived_kind(spec: &ParamSpec) -> WidgetKind {
    if !spec.options.integer {
        return WidgetKind::Knob;
    }
    let choices = spec.options.choices.len();
    match choices {
        0 if spec.min == 0.0 && spec.max == 1.0 => WidgetKind::Toggle,
        0 => WidgetKind::Knob,
        2..=4 => WidgetKind::Segmented {
            width: (choices * 3) as u8,
        },
        5 => WidgetKind::Radio,
        _ => WidgetKind::Select,
    }
}

impl App {
    /// [`control_target`] for the open patch.
    pub(crate) fn control_target(&self, tile: u32, widget: usize, part: ControlPart) -> Option<ControlTarget> {
        control_target(
            self.session.controller.graph(),
            &self.session.hierarchy,
            tile,
            widget,
            part,
        )
    }
}

/// The surface of the sub-patch tile `instance`: one widget per exposed
/// item, in the order the items were exposed, each the widget the item has
/// inside ([`surface_kind`]), labelled with its surface name, and with
/// hover text that names the inner module and parameter it controls
/// (panels.md, Exposed controls in a sub-patch). Widgets flow left to
/// right on the panel grid and wrap at 12 units; each row is as tall as
/// its tallest widget (panels.md, Default panels, Layout). With nothing
/// exposed the panel is empty.
pub fn surface_panel(graph: &Graph, hierarchy: &Hierarchy, instance: ModuleId) -> PanelView {
    let Some(definition) = hierarchy.definition_of(instance) else {
        return empty_surface();
    };
    let path = hierarchy.path_of(instance);
    let widgets: Vec<WidgetView> = definition
        .exposed
        .iter()
        .enumerate()
        .map(|(index, exposed)| {
            let target = control_target(graph, hierarchy, instance.0, index, ControlPart::Main);
            let kind = surface_kind(graph, hierarchy, instance, index);
            let (description, controls) = inner_text(graph, hierarchy, &path, exposed);
            // A surface item the enclosing sub-patch exposes again is an
            // exposed control too, with the mark and the note.
            let further = hierarchy.exposure_of(instance, &exposed.name).map(|(_, name)| name);
            let note = match &further {
                Some(name) => format!("Controls {controls}. Exposed as {name}"),
                None => format!("Controls {controls}"),
            };
            WidgetView {
                kind: view_kind(kind, target.as_ref().map(|t| &t.spec)),
                x: 0,
                y: 0,
                label: exposed.name.clone(),
                hover: HoverText {
                    description,
                    name: None,
                    note: Some(note),
                },
                hidden: false,
                exposed: further.is_some(),
            }
        })
        .collect();
    flow_layout(widgets)
}

/// A surface with nothing on it: the tile shows its name and ports.
fn empty_surface() -> PanelView {
    PanelView {
        width: 10,
        height: 2,
        widgets: Vec::new(),
    }
}

/// Places `widgets` on the panel grid in order, left to right, wrapping
/// at [`SURFACE_WIDTH`] units; each row is as tall as its tallest widget,
/// and the panel is as wide as its widest row, at least as wide as an
/// empty surface (panels.md, Default panels, Layout).
fn flow_layout(mut widgets: Vec<WidgetView>) -> PanelView {
    let (mut x, mut y, mut row_height, mut width) = (0u8, 0u8, 0u8, 0u8);
    for widget in &mut widgets {
        let (w, h) = widget.size();
        if x > 0 && x + w > SURFACE_WIDTH {
            x = 0;
            y += row_height;
            row_height = 0;
        }
        widget.x = x;
        widget.y = y;
        x += w;
        row_height = row_height.max(h);
        width = width.max(x);
    }
    let empty = empty_surface();
    PanelView {
        width: width.max(empty.width),
        height: if widgets.is_empty() {
            empty.height
        } else {
            y + row_height
        },
        widgets,
    }
}

/// Where a surface row wraps, in grid units: four knobs wide (panels.md,
/// Default panels, Layout).
const SURFACE_WIDTH: u8 = 12;

/// The canvas widget for a surface control of kind `kind` whose parameter
/// is `spec`: a knob, fader, or number steps through a whole number's
/// values, and a choice widget shows the parameter's choice names
/// (panels.md, Choice names). A kind the surface cannot show yet gets a
/// knob.
fn view_kind(kind: WidgetKind, spec: Option<&ParamSpec>) -> WidgetViewKind {
    let steps = spec
        .filter(|spec| spec.options.integer)
        .map_or(0, |spec| (spec.max - spec.min) as u32);
    let choices = || {
        spec.map_or_else(Vec::new, |spec| {
            spec.options.choices.iter().map(ToString::to_string).collect()
        })
    };
    match kind {
        WidgetKind::Knob => WidgetViewKind::Knob { steps },
        WidgetKind::Fader { vertical } => WidgetViewKind::Fader { vertical, steps },
        WidgetKind::Number => WidgetViewKind::Number {
            steps,
            key_step: spec
                .and_then(|spec| Some(spec.options.key_step? / (spec.max - spec.min)))
                .unwrap_or(0.0),
        },
        WidgetKind::Toggle => WidgetViewKind::Toggle,
        WidgetKind::Radio => WidgetViewKind::Radio { options: choices() },
        WidgetKind::Select => WidgetViewKind::Select {
            options: spec.map_or_else(Vec::new, ParamSpec::value_names),
        },
        WidgetKind::Segmented { width } => WidgetViewKind::Segmented {
            options: choices(),
            width,
        },
        _ => WidgetViewKind::Knob { steps },
    }
}

/// The description a surface control shows, and what it controls: the
/// member's name and the parameter's label, such as "Filter › Cutoff",
/// naming the nested sub-patch and its surface item when the member is
/// one.
fn inner_text(graph: &Graph, hierarchy: &Hierarchy, instance: &ModulePath, exposed: &Exposed) -> (String, String) {
    let member = instance.child(exposed.module);
    let member_id = hierarchy.ids().get(&member);
    let description = hierarchy
        .resolve_exposed(instance, exposed)
        .and_then(|(id, param)| {
            let node = graph.node(id)?;
            let index = node.parameter_index(&param)?;
            Some(node.descriptor.params[index].description.to_string())
        })
        .unwrap_or_default();
    let (member_name, param_label) = match member_id {
        Some(id) if hierarchy.definition_of(id).is_some() => {
            (hierarchy.tile_name(id).unwrap_or_default(), exposed.param.clone())
        }
        Some(id) => match graph.node(id) {
            Some(node) => (
                node.descriptor.name.to_string(),
                node.parameter_index(&exposed.param)
                    .map_or(exposed.param.clone(), |i| node.descriptor.params[i].label.to_string()),
            ),
            None => (format!("module {}", exposed.module), exposed.param.clone()),
        },
        None => (format!("module {}", exposed.module), exposed.param.clone()),
    };
    (description, format!("{member_name} › {param_label}"))
}

/// Fills the values of every surface control from the inner parameter it
/// moves: position and text in the exposed range, the modulation mark of
/// the inner parameter's assignments (only `dragged` while its arc is
/// dragged), and the live value while an assignment is on, as
/// `scene::update_panel_values` does for module controls.
pub fn surface_values(
    values: &mut PanelValues,
    controller: &Controller,
    hierarchy: &Hierarchy,
    reaches: &BTreeMap<(ModuleId, usize), Vec<Reach>>,
    dragged: Option<&((ModuleId, usize), Reach)>,
) {
    let graph = controller.graph();
    for (instance, count) in instances(hierarchy) {
        for index in 0..count {
            let Some(target) = control_target(graph, hierarchy, instance.0, index, ControlPart::Main) else {
                values.set(instance.0, index, WidgetValue::default());
                continue;
            };
            let Some(node) = graph.node(target.module) else {
                continue;
            };
            let value = node.params[target.index];
            let key = (target.module, target.index);
            let all = reaches.get(&key).map_or(&[][..], Vec::as_slice);
            let assigned = match dragged {
                Some((dragged_key, reach)) if *dragged_key == key => std::slice::from_ref(reach),
                _ => all,
            };
            let live = (target.kind.has_range() && all.iter().any(|r| r.enabled))
                .then(|| controller.modulated(target.module, target.index))
                .flatten();
            values.set(
                instance.0,
                index,
                WidgetValue {
                    position: crate::scene::control_position(target.kind, &target.spec, value),
                    text: crate::param::format(&target.spec, value),
                    modulation: crate::modulation::mark(target.kind, &target.spec, value, assigned),
                    live,
                    ..WidgetValue::default()
                },
            );
        }
    }
}

/// Updates the live dots of the surface controls that stand for the
/// parameter `target`, at every level above it, as `scene::update_live_values`
/// does for module controls.
pub fn surface_live_values(
    values: &mut PanelValues,
    hierarchy: &Hierarchy,
    target: (ModuleId, &str),
    live: Option<f32>,
) {
    let mut at = (target.0, target.1.to_string());
    while let Some((instance, name)) = hierarchy.exposure_of(at.0, &at.1) {
        let index = hierarchy
            .definition_of(instance)
            .and_then(|d| d.exposed.iter().position(|e| e.name == name));
        if let Some(index) = index
            && let Some(value) = values.get(instance.0, index)
            && value.live != live
        {
            let next = WidgetValue { live, ..value.clone() };
            values.set(instance.0, index, next);
        }
        at = (instance, name);
    }
}

/// Every sub-patch tile in the patch and how many items its surface
/// shows, in path order.
fn instances(hierarchy: &Hierarchy) -> Vec<(ModuleId, usize)> {
    hierarchy
        .ids()
        .paths()
        .filter_map(|(path, id)| Some((id, hierarchy.definition_at(path)?.exposed.len())))
        .collect()
}

#[cfg(test)]
mod tests;
