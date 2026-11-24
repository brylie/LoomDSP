//! Modulation assignments in the app: what a control shows of them (the
//! range arc on a knob, or the dot), and the words and defaults the
//! inspector and a drop use. Spec: `docs/design/modulation.md`, Slice 1.

use crate::App;
use loom_canvas::ModulationMark;
use loom_core::ModuleNames;
use loom_core::{Assignment, AssignmentId, Graph, ModuleId, OutputRef, ParamSpec, Polarity, WidgetKind};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;

/// A change to a modulation assignment, from the inspector or the matrix.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Open or close the **Add modulation** list under a parameter.
    AddModulation { module: u32, index: usize },
    /// Assign an output to a parameter, picked from **Add modulation**.
    Assign {
        source: OutputRef,
        module: u32,
        index: usize,
    },
    /// An assignment's depth slider moved.
    Depth { id: AssignmentId, depth: f32 },
    /// An assignment's polarity was picked.
    Polarity { id: AssignmentId, choice: PolarityChoice },
    /// An assignment was turned on or off.
    Enabled { id: AssignmentId, enabled: bool },
    /// The remove button on an assignment row.
    Unassign(AssignmentId),
}

impl App {
    /// Sets depth for a drag from any view and updates the outline's spoken value.
    pub(crate) fn change_assignment_depth(
        &mut self,
        id: AssignmentId,
        depth: f32,
    ) -> Result<f32, loom_core::LoomError> {
        let depth = self.session.controller.set_assignment_depth(id, depth)?;
        self.fan_out_assignment(id, |controller, copy| {
            controller.set_assignment_depth(copy, depth).map(|_| ())
        });
        self.session.patch_map.update_depth(id.0, depth);
        Ok(depth)
    }

    /// Handles a change to a modulation assignment.
    pub(crate) fn modulation_message(&mut self, message: Message) {
        match message {
            Message::AddModulation { module, index } => {
                self.session.gestures.toggle_adding((module, index));
            }
            Message::Assign { source, module, index } => {
                self.session.gestures.close_adding();
                self.close_slider_gesture();
                self.edit(|app| app.assign(source, ModuleId(module), index));
            }
            Message::Depth { id, depth } => {
                // Like a parameter slider: the first change opens the
                // gesture, and letting go closes it, so one drag is one step.
                self.start_slider_edit();
                if let Err(error) = self.change_assignment_depth(id, depth) {
                    self.alerts.warn(error.to_string());
                }
                self.refresh_values();
            }
            Message::Polarity { id, choice } => {
                self.close_slider_gesture();
                self.edit(|app| {
                    if let Err(e) = app.session.controller.set_assignment_polarity(id, choice.polarity()) {
                        app.alerts.warn(e.to_string());
                    }
                    app.fan_out_assignment(id, |c, copy| c.set_assignment_polarity(copy, choice.polarity()));
                    app.refresh_values();
                });
            }
            Message::Enabled { id, enabled } => {
                self.close_slider_gesture();
                self.edit(|app| {
                    if let Err(e) = app.session.controller.set_assignment_enabled(id, enabled) {
                        app.alerts.warn(e.to_string());
                    } else {
                        app.fan_out_assignment(id, |c, copy| c.set_assignment_enabled(copy, enabled));
                        app.session.scene.set_modulation_enabled(id.0, enabled);
                        app.refresh_patch_map();
                    }
                    app.refresh_values();
                });
            }
            Message::Unassign(id) => {
                self.close_slider_gesture();
                self.edit(|app| match app.session.controller.remove_assignment(id) {
                    Ok(removed) => {
                        app.commit();
                        let names = Names::new(app.session.controller.graph(), &app.session.hierarchy);
                        app.alerts.say(format!(
                            "Removed modulation from {} to {}",
                            names.source(removed.source),
                            names.target(removed.target, removed.param)
                        ));
                    }
                    Err(e) => app.alerts.warn(e.to_string()),
                });
            }
        }
    }
}

/// Depth of a new assignment on a continuous parameter: a quarter of the
/// knob. One constant, so it is easy to change after trying it with users.
pub const DEFAULT_DEPTH: f32 = 0.25;

/// Depth of a new assignment on `spec`: [`DEFAULT_DEPTH`], or one step of
/// a whole-number parameter if that is larger, so a source at its peak
/// moves the value one step (spec, Making an assignment).
pub fn default_depth(spec: &ParamSpec) -> f32 {
    let span = spec.max - spec.min;
    if spec.options.integer && span > 0.0 {
        DEFAULT_DEPTH.max(1.0 / span)
    } else {
        DEFAULT_DEPTH
    }
}

/// True if `source` already drives this parameter. A second one would only
/// add its depth to the first, so a drop or pick makes none.
pub fn exists(graph: &Graph, source: OutputRef, target: ModuleId, param: usize) -> bool {
    graph
        .assignments()
        .any(|(_, a)| a.source == source && a.target == target && a.param == param)
}

/// The first instance and count for each displayed instance name in a level.
type InstanceNames = BTreeMap<String, (ModuleId, usize)>;

/// Duplicate counts shared by the scene and every name in a panel model.
pub(crate) struct Names<'a> {
    graph: &'a Graph,
    hierarchy: &'a loom_patch::Hierarchy,
    hidden: loom_patch::hierarchy::Hidden<'a>,
    counts: BTreeMap<(loom_patch::ModulePath, &'a str), usize>,
    instance_names: RefCell<BTreeMap<loom_patch::ModulePath, InstanceNames>>,
}

impl<'a> Names<'a> {
    /// Counts visible modules of each kind at each level.
    pub(crate) fn new(graph: &'a Graph, hierarchy: &'a loom_patch::Hierarchy) -> Self {
        let hidden = hierarchy.hidden();
        let mut counts = BTreeMap::new();
        for (id, node) in graph.nodes().filter(|(id, _)| !hidden.contains(*id)) {
            *counts.entry((hierarchy.level_of(id), module_kind(node))).or_default() += 1;
        }
        Self {
            graph,
            hierarchy,
            hidden,
            counts,
            instance_names: RefCell::default(),
        }
    }

    /// A module's visible name, including its local ID when disambiguation is needed.
    pub(crate) fn module(&self, module: ModuleId) -> String {
        let Some(node) = self.graph.node(module) else {
            return module.0.to_string();
        };
        let name = node.unknown.as_ref().map_or(node.descriptor.name, |unknown| {
            node.label.as_deref().unwrap_or(&unknown.kind)
        });
        let key = (self.hierarchy.level_of(module), module_kind(node));
        let visible = self.counts.get(&key).copied().unwrap_or_default();
        if visible > usize::from(!self.hidden.contains(module)) {
            format!("{name} {}", self.hierarchy.path_of(module).last())
        } else {
            name.to_string()
        }
    }

    /// Names an output on a module.
    pub(crate) fn source(&self, source: OutputRef) -> String {
        let label = self
            .graph
            .node(source.module)
            .and_then(|n| n.output_label(source.port))
            .unwrap_or("?");
        format!("{}: {label}", self.module(source.module))
    }

    /// Names a parameter on a module.
    pub(crate) fn target(&self, target: ModuleId, param: usize) -> String {
        let label = self
            .graph
            .node(target)
            .and_then(|n| n.parameter_label(param))
            .unwrap_or("?");
        format!("{}: {label}", self.module(target))
    }

    /// True for a module shown directly at this level.
    pub(crate) fn on_level(&self, module: ModuleId, level: &loom_patch::ModulePath) -> bool {
        self.hierarchy.level_of(module) == *level && !self.hidden.contains(module)
    }

    /// Names the visible slot control or exposed instance parameter for a flat target.
    pub(crate) fn structural_target(
        &self,
        level: &loom_patch::ModulePath,
        target: ModuleId,
        param: usize,
    ) -> Option<String> {
        let spec = self.graph.node(target)?.descriptor.params.get(param)?;
        if self.hierarchy.level_of(target) == *level {
            let tile = self.hierarchy.slot_of_part(target)?;
            let slot = self.hierarchy.slot(tile)?;
            return self.structural_endpoint(level, tile, slot.role_param_of(spec.name).unwrap_or(spec.name));
        }
        let (part, index) = self.hierarchy.surface_for(level, &(target, spec.name.to_string()))?;
        let label = &self.hierarchy.definition_of(part)?.exposed.get(index)?.name;
        let tile = self.hierarchy.slot_of_part(part).unwrap_or(part);
        self.structural_endpoint(level, tile, label)
    }

    /// The name of a visible endpoint on a sub-patch or slot tile.
    pub(crate) fn structural_endpoint(
        &self,
        level: &loom_patch::ModulePath,
        tile: ModuleId,
        label: &str,
    ) -> Option<String> {
        let name = self.hierarchy.tile_name(tile)?;
        let mut levels = self.instance_names.borrow_mut();
        let names = levels.entry(level.clone()).or_insert_with(|| {
            let mut names = InstanceNames::new();
            for (id, kind) in self.hierarchy.structural_tiles(level) {
                if matches!(kind, loom_patch::Tile::Instance(_) | loom_patch::Tile::Slot(_))
                    && self.hierarchy.slot_of_part(id).is_none()
                    && let Some(name) = self.hierarchy.tile_name(id)
                {
                    names.entry(name).or_insert((id, 0)).1 += 1;
                }
            }
            names
        });
        let duplicates = names.get(&name).is_some_and(|&(id, count)| count > 1 || id != tile);
        let name = if duplicates {
            format!("{name} {}", self.hierarchy.path_of(tile).last())
        } else {
            name
        };
        Some(format!("{name}: {label}"))
    }
}

/// The type key used to distinguish duplicates, including unknown modules.
fn module_kind(node: &loom_core::Node) -> &str {
    node.unknown
        .as_ref()
        .map_or(node.descriptor.id, |unknown| unknown.kind.as_str())
}

/// A depth as a signed percentage: "+25%", "-10%", "0%".
pub fn depth_text(depth: f32) -> String {
    let percent = (depth * 100.0).round() as i32;
    if percent > 0 {
        format!("+{percent}%")
    } else {
        format!("{percent}%")
    }
}

/// Every output at the current patch level, in module id order, then port
/// order: the list **Add modulation** picks from.
pub fn sources(graph: &Graph) -> Vec<OutputRef> {
    graph
        .nodes()
        .flat_map(|(module, node)| (0..node.descriptor.outputs.len()).map(move |port| OutputRef { module, port }))
        .collect()
}

/// The assignments on one parameter, in the order the engine adds them.
pub fn on_param(graph: &Graph, target: ModuleId, param: usize) -> Vec<(AssignmentId, Assignment)> {
    graph
        .assignments()
        .filter(|(_, a)| a.target == target && a.param == param)
        .map(|(id, a)| (id, *a))
        .collect()
}

/// What each output of `module` drives, by output index, in the order the
/// assignments were added. Outputs that drive nothing are left out.
pub fn drives(graph: &Graph, module: ModuleId) -> BTreeMap<usize, Vec<(AssignmentId, Assignment)>> {
    let mut out: BTreeMap<usize, Vec<(AssignmentId, Assignment)>> = BTreeMap::new();
    for (id, a) in graph.assignments().filter(|(_, a)| a.source.module == module) {
        out.entry(a.source.port).or_default().push((id, *a));
    }
    out
}

/// The polarity choice in an inspector row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolarityChoice {
    /// No override: the source's own polarity, shown in brackets.
    Follow(Polarity),
    /// Forced unipolar.
    Unipolar,
    /// Forced bipolar.
    Bipolar,
}

impl PolarityChoice {
    /// The three options for a source with polarity `own`.
    pub fn options(own: Polarity) -> [Self; 3] {
        [Self::Follow(own), Self::Unipolar, Self::Bipolar]
    }

    /// The choice an assignment has now.
    pub fn of(polarity: Option<Polarity>, own: Polarity) -> Self {
        match polarity {
            None => Self::Follow(own),
            Some(Polarity::Unipolar) => Self::Unipolar,
            Some(Polarity::Bipolar) => Self::Bipolar,
        }
    }

    /// The override to store: `None` follows the source.
    pub fn polarity(self) -> Option<Polarity> {
        match self {
            Self::Follow(_) => None,
            Self::Unipolar => Some(Polarity::Unipolar),
            Self::Bipolar => Some(Polarity::Bipolar),
        }
    }
}

impl fmt::Display for PolarityChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = |p: Polarity| match p {
            Polarity::Unipolar => "unipolar",
            Polarity::Bipolar => "bipolar",
        };
        match self {
            Self::Follow(own) => write!(f, "Follow source ({})", name(*own)),
            Self::Unipolar => f.write_str("Unipolar"),
            Self::Bipolar => f.write_str("Bipolar"),
        }
    }
}

/// The polarity an output declares, or bipolar if it is gone.
pub fn source_polarity(graph: &Graph, source: OutputRef) -> Polarity {
    graph
        .node(source.module)
        .and_then(|n| n.descriptor.outputs.get(source.port))
        .map_or(Polarity::Bipolar, |p| p.polarity())
}

/// One assignment as the range arc sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    /// Share of the control's range, from -1 to 1.
    pub depth: f32,
    /// The polarity the engine uses: the override, or the source's own.
    pub polarity: Polarity,
    /// False when the assignment is off.
    pub enabled: bool,
}

/// The assignments on every modulated parameter, by module and parameter
/// index, in engine order.
pub fn reaches(graph: &Graph) -> BTreeMap<(ModuleId, usize), Vec<Reach>> {
    let mut out: BTreeMap<(ModuleId, usize), Vec<Reach>> = BTreeMap::new();
    for (_, a) in graph.assignments() {
        if let Some(reach) = reach(graph, a) {
            out.entry((a.target, a.param)).or_default().push(reach);
        }
    }
    out
}

/// One assignment as the range arc sees it, if its source exists.
pub fn reach(graph: &Graph, a: &Assignment) -> Option<Reach> {
    let source = graph.node(a.source.module)?;
    if a.source.port >= source.output_count() {
        return None;
    }
    Some(Reach {
        depth: a.depth,
        polarity: a.polarity.unwrap_or_else(|| source_polarity(graph, a.source)),
        enabled: a.enabled,
    })
}

/// Which assignment an arc drag on parameter `param` of `target` changes
/// (spec, Arc drag), in this order: the pinned output's, then the one from
/// the only output of the selected module that drives it, then the only
/// enabled one. `Err` holds what the status bar says when none fits. A
/// source with more than one assignment on the parameter (only in patch
/// files written by hand) is not a choice either.
pub fn drag_target(
    graph: &Graph,
    target: ModuleId,
    param: usize,
    pinned: Option<OutputRef>,
    selected: Option<ModuleId>,
) -> Result<AssignmentId, &'static str> {
    const DUPLICATES: &str = "This source drives the knob more than once: use the inspector";
    let on = on_param(graph, target, param);
    let from = |source: OutputRef| -> Vec<AssignmentId> {
        on.iter()
            .filter(|(_, a)| a.source == source)
            .map(|(id, _)| *id)
            .collect()
    };
    if let Some(source) = pinned {
        match from(source).as_slice() {
            [id] => return Ok(*id),
            [] => {}
            _ => return Err(DUPLICATES),
        }
    }
    if let Some(module) = selected {
        let mut outputs: Vec<usize> = on
            .iter()
            .filter(|(_, a)| a.source.module == module)
            .map(|(_, a)| a.source.port)
            .collect();
        outputs.sort_unstable();
        outputs.dedup();
        if let [port] = outputs.as_slice() {
            return match from(OutputRef { module, port: *port }).as_slice() {
                [id] => Ok(*id),
                _ => Err(DUPLICATES),
            };
        }
    }
    match on.iter().filter(|(_, a)| a.enabled).collect::<Vec<_>>().as_slice() {
        [(id, _)] => Ok(*id),
        _ => Err("Several sources: pin one with its badge, or use the inspector"),
    }
}

/// The mark for a control bound to `spec`, at value `base`, with these
/// assignments.
///
/// A knob gets the range from `base + low` to `base + high` in its 0 to 1
/// position, clamped, where each enabled assignment adds to `low` and
/// `high` by the spec's table. A whole-number parameter rounds each end the
/// way the engine rounds the value. A range of zero length gives the dot.
pub fn mark(kind: WidgetKind, spec: &ParamSpec, base: f32, reaches: &[Reach]) -> ModulationMark {
    if reaches.is_empty() {
        return ModulationMark::None;
    }
    if reaches.iter().all(|r| !r.enabled) {
        return ModulationMark::Ring;
    }
    if !kind.has_range() {
        return ModulationMark::Dot;
    }
    let (low, high) = range_positions(spec, base, reaches);
    if high > low {
        ModulationMark::Range { low, high }
    } else {
        ModulationMark::Dot
    }
}

/// Clamped endpoints in the control's position space, shared by the canvas and Matrix.
pub(crate) fn range_positions(spec: &ParamSpec, base: f32, reaches: &[Reach]) -> (f32, f32) {
    let (mut low, mut high) = (0.0_f32, 0.0_f32);
    for r in reaches.iter().filter(|r| r.enabled) {
        match r.polarity {
            Polarity::Unipolar => {
                low += r.depth.min(0.0);
                high += r.depth.max(0.0);
            }
            Polarity::Bipolar => {
                low -= r.depth.abs();
                high += r.depth.abs();
            }
        }
    }
    let position = spec.normalize(base);
    let end = |offset: f32| {
        let t = (position + offset).clamp(0.0, 1.0);
        if spec.options.integer {
            spec.normalize(spec.denormalize(t))
        } else {
            t
        }
    };
    (end(low), end(high))
}

#[cfg(test)]
mod tests;
