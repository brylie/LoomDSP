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
use loom_canvas::ControlPart;
use loom_core::{Graph, ModuleId, ModuleNames, ParamRange, ParamSpec, WidgetKind};
use loom_patch::Hierarchy;

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

#[cfg(test)]
mod tests;
