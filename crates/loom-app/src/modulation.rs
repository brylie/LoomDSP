//! Modulation assignments in the app: what a control shows of them (the
//! range arc on a knob, or the dot), and the words and defaults the
//! inspector and a drop use. Spec: `docs/design/modulation.md`, Slice 1.

use loom_canvas::ModulationMark;
use loom_core::{Assignment, AssignmentId, Graph, ModuleId, OutputRef, ParamSpec, Polarity, WidgetKind};
use std::collections::BTreeMap;
use std::fmt;

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

/// "module name #id", as the inspector names a module.
pub fn module_name(graph: &Graph, module: ModuleId) -> String {
    match graph.node(module) {
        Some(node) => format!(
            "{} #{}",
            node.unknown.as_ref().map_or(node.descriptor.name, |unknown| unknown
                .label
                .as_deref()
                .unwrap_or(&unknown.kind)),
            module.0
        ),
        None => format!("#{}", module.0),
    }
}

/// "module name #id: output label".
pub fn source_name(graph: &Graph, source: OutputRef) -> String {
    let label = graph
        .node(source.module)
        .and_then(|n| {
            n.unknown.as_ref().map_or_else(
                || n.descriptor.outputs.get(source.port).map(|p| p.label),
                |unknown| unknown.outputs.get(source.port).map(String::as_str),
            )
        })
        .unwrap_or("?");
    format!("{}: {label}", module_name(graph, source.module))
}

/// "module name #id: parameter label".
pub fn target_name(graph: &Graph, target: ModuleId, param: usize) -> String {
    let label = graph
        .node(target)
        .and_then(|n| {
            n.unknown.as_ref().map_or_else(
                || n.descriptor.params.get(param).map(|p| p.label),
                |unknown| unknown.parameters.get(param).map(String::as_str),
            )
        })
        .unwrap_or("?");
    format!("{}: {label}", module_name(graph, target))
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
    if kind != WidgetKind::Knob {
        return ModulationMark::Dot;
    }
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
    let (low, high) = (end(low), end(high));
    if high > low {
        ModulationMark::Range { low, high }
    } else {
        ModulationMark::Dot
    }
}

#[cfg(test)]
mod tests;
