//! Undo and redo (`docs/design/undo.md`).
//!
//! An edit is recorded by comparing the patch before and after it. The
//! difference is a [`Step`]: only the modules, cables, assignments, and MIDI
//! mappings that changed, each with its old and new value. Undo applies the
//! old side, redo the new side, through the same [`Controller`] methods an
//! edit uses. Nothing here runs on the audio thread.
//!
//! Why compare states: one user action can touch several things (a delete
//! takes cables, assignments, and mappings with it). A difference holds all
//! of them in one step, with no per-edit inverse to keep in step with the
//! controller.

use iced::Point;
use loom_canvas::CanvasEvent;
use loom_core::{
    Assignment, AssignmentId, ButtonMode, Controller, InputRef, LoomError, MidiControl, ModuleDescriptor, ModuleId,
    ModuleRole, OutputRef, ParamRange, WidgetKind,
};
use std::collections::{BTreeMap, BTreeSet};

/// The history keeps this many steps. The oldest is dropped first.
pub const LIMIT: usize = 200;

/// One module as the patch holds it.
#[derive(Clone)]
pub struct ModuleState {
    descriptor: &'static ModuleDescriptor,
    params: Vec<f32>,
    /// The selected control range of each parameter (control-ranges.md).
    ranges: Vec<Option<ParamRange>>,
    position: Option<Point>,
}

impl PartialEq for ModuleState {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.descriptor, other.descriptor)
            && self.params == other.params
            && self.ranges == other.ranges
            && self.position == other.position
    }
}

/// What the patch holds that undo covers.
#[derive(Clone, PartialEq, Default)]
pub struct Snapshot {
    modules: BTreeMap<u32, ModuleState>,
    cables: BTreeMap<InputRef, OutputRef>,
    assignments: BTreeMap<AssignmentId, Assignment>,
    /// The control mapped to each parameter, by module and parameter.
    /// Mapping IDs are left out: no step refers to them.
    midi: BTreeMap<(u32, usize), MidiControl>,
}

impl Snapshot {
    /// Reads the patch.
    pub fn take(controller: &Controller, positions: &BTreeMap<u32, Point>) -> Self {
        let graph = controller.graph();
        Self {
            modules: graph
                .nodes()
                .map(|(id, node)| {
                    (
                        id.0,
                        ModuleState {
                            descriptor: node.descriptor,
                            params: node.params.clone(),
                            ranges: (0..node.params.len()).map(|index| node.range(index)).collect(),
                            position: positions.get(&id.0).copied(),
                        },
                    )
                })
                .collect(),
            cables: graph.cables().map(|(from, to)| (to, from)).collect(),
            assignments: graph.assignments().map(|(id, a)| (id, *a)).collect(),
            midi: graph
                .midi_mappings()
                .map(|(_, m)| ((m.target.0, m.param), m.control))
                .collect(),
        }
    }
}

type Pair<T> = (Option<T>, Option<T>);

/// One undo step: what changed, as (before, after) pairs.
#[derive(Clone)]
pub struct Step {
    name: String,
    modules: BTreeMap<u32, Pair<ModuleState>>,
    cables: BTreeMap<InputRef, Pair<OutputRef>>,
    assignments: BTreeMap<AssignmentId, Pair<Assignment>>,
    midi: BTreeMap<(u32, usize), Pair<MidiControl>>,
}

impl Step {
    /// What the step did, for the toolbar and the status bar.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gives the step the name of the command that made it, such as
    /// "paste Filter", in place of the name read from what changed.
    pub fn rename(&mut self, name: String) {
        self.name = name;
    }

    /// The modules the step touched.
    pub fn touched_modules(&self) -> impl Iterator<Item = u32> + '_ {
        self.modules.keys().copied()
    }
}

/// True if a canvas event is an edit, and so becomes a step. Views,
/// selection, and the transport are not edits. A momentary button hold is
/// a performance action, not an edit (undo.md, What is not undoable), so
/// only a button in toggle mode makes a step.
pub fn is_edit(controller: &Controller, event: &CanvasEvent) -> bool {
    match *event {
        CanvasEvent::Move { .. }
        | CanvasEvent::Connect { .. }
        | CanvasEvent::Disconnect(_)
        | CanvasEvent::Assign { .. }
        | CanvasEvent::DragDepth { .. }
        | CanvasEvent::Delete(_)
        | CanvasEvent::SetControl { .. }
        | CanvasEvent::ResetControl { .. } => true,
        CanvasEvent::ButtonPressed { module, widget }
        | CanvasEvent::ButtonReleased { module, widget }
        | CanvasEvent::ButtonArrow { module, widget, .. } => !is_momentary_button(controller, module, widget),
        _ => false,
    }
}

/// True if the button's stored mode is momentary.
fn is_momentary_button(controller: &Controller, module: u32, widget: usize) -> bool {
    controller.graph().node(ModuleId(module)).is_some_and(|node| {
        let Some(panel_widget) = node.descriptor.panel.widgets.get(widget) else {
            return false;
        };
        let WidgetKind::Button { mode } = panel_widget.kind else {
            return false;
        };
        node.descriptor
            .param_index(mode)
            .is_some_and(|i| ButtonMode::from_value(node.params[i]) == ButtonMode::Momentary)
    })
}

/// Compares the patch before and after an edit. `touched` lists the
/// parameters the app set itself. A parameter with a MIDI mapping may also
/// move by hardware, and that is not an edit (undo.md, Mapped parameters), so
/// a change on a mapped parameter counts only if it is in `touched`.
/// Returns `None` when the edit changed nothing.
pub fn diff(before: &Snapshot, after: &Snapshot, touched: &BTreeSet<(u32, usize)>) -> Option<Step> {
    let mapped = |module: u32, index: usize| {
        before.midi.contains_key(&(module, index)) || after.midi.contains_key(&(module, index))
    };
    let mut step = Step {
        name: String::new(),
        modules: BTreeMap::new(),
        cables: BTreeMap::new(),
        assignments: BTreeMap::new(),
        midi: BTreeMap::new(),
    };
    for id in before
        .modules
        .keys()
        .chain(after.modules.keys())
        .collect::<BTreeSet<_>>()
    {
        let old = before.modules.get(id);
        let mut new = after.modules.get(id).cloned();
        if let (Some(old), Some(new)) = (old, new.as_mut()) {
            for index in 0..new.params.len().min(old.params.len()) {
                if mapped(*id, index) && !touched.contains(&(*id, index)) {
                    new.params[index] = old.params[index];
                }
            }
        }
        if old != new.as_ref() {
            step.modules.insert(*id, (old.cloned(), new));
        }
    }
    for input in before.cables.keys().chain(after.cables.keys()).collect::<BTreeSet<_>>() {
        let (old, new) = (before.cables.get(input), after.cables.get(input));
        if old != new {
            step.cables.insert(*input, (old.copied(), new.copied()));
        }
    }
    for id in before
        .assignments
        .keys()
        .chain(after.assignments.keys())
        .collect::<BTreeSet<_>>()
    {
        let (old, new) = (before.assignments.get(id), after.assignments.get(id));
        if old != new {
            step.assignments.insert(*id, (old.copied(), new.copied()));
        }
    }
    for key in before.midi.keys().chain(after.midi.keys()).collect::<BTreeSet<_>>() {
        let (old, new) = (before.midi.get(key), after.midi.get(key));
        if old != new {
            step.midi.insert(*key, (old.copied(), new.copied()));
        }
    }
    if step.modules.is_empty() && step.cables.is_empty() && step.assignments.is_empty() && step.midi.is_empty() {
        return None;
    }
    step.name = name(&step);
    Some(step)
}

/// Names a step in the words the user sees on screen.
fn name(step: &Step) -> String {
    let count = |n: usize, one: String, noun: &str| if n == 1 { one } else { format!("{n} {noun}") };
    let added: Vec<&ModuleState> = step
        .modules
        .values()
        .filter_map(|(o, n)| o.is_none().then_some(n.as_ref()).flatten())
        .collect();
    let removed: Vec<&ModuleState> = step
        .modules
        .values()
        .filter_map(|(o, n)| n.is_none().then_some(o.as_ref()).flatten())
        .collect();
    if !removed.is_empty() {
        return format!(
            "delete {}",
            count(removed.len(), removed[0].descriptor.name.to_string(), "modules")
        );
    }
    if !added.is_empty() {
        return format!(
            "add {}",
            count(added.len(), added[0].descriptor.name.to_string(), "modules")
        );
    }
    if let Some(control) = step.midi.values().find_map(|(_, new)| *new) {
        // The control left another parameter: the mapping moved.
        if step
            .midi
            .values()
            .any(|(old, new)| *old == Some(control) && new.is_none())
        {
            return "move MIDI mapping".to_string();
        }
        return format!("learn {control}");
    }
    if !step.midi.is_empty() {
        return "remove MIDI mapping".to_string();
    }
    if !step.cables.is_empty() {
        let added = step.cables.values().filter(|(_, n)| n.is_some()).count();
        let removed = step.cables.values().filter(|(o, _)| o.is_some()).count();
        return match (added, removed) {
            (0, _) => "remove cable".to_string(),
            (_, 0) => "connect cable".to_string(),
            (1, 1) if step.cables.len() == 2 => "move cable".to_string(),
            _ => "connect cable".to_string(),
        };
    }
    if !step.assignments.is_empty() {
        return match step.assignments.values().next() {
            Some((None, _)) => "add modulation",
            Some((_, None)) => "remove modulation",
            _ => "change modulation",
        }
        .to_string();
    }
    // A new range, with or without the value it clamped.
    for (old, new) in step.modules.values() {
        if let (Some(old), Some(new)) = (old, new)
            && let Some(index) = (0..new.ranges.len()).find(|&i| old.ranges.get(i) != new.ranges.get(i))
        {
            return format!(
                "change {} {} range",
                new.descriptor.name, new.descriptor.params[index].label
            );
        }
    }
    let moved: Vec<&ModuleState> = step
        .modules
        .values()
        .filter(|(o, n)| o.as_ref().zip(n.as_ref()).is_some_and(|(o, n)| o.params == n.params))
        .filter_map(|(o, _)| o.as_ref())
        .collect();
    if moved.len() == step.modules.len() {
        return format!(
            "move {}",
            count(moved.len(), moved[0].descriptor.name.to_string(), "modules")
        );
    }
    let mut changes = Vec::new();
    for (old, new) in step.modules.values() {
        if let (Some(old), Some(new)) = (old, new) {
            for (index, (a, b)) in old.params.iter().zip(&new.params).enumerate() {
                if a != b {
                    changes.push((new.descriptor, index));
                }
            }
        }
    }
    match changes.as_slice() {
        [(descriptor, index)] => format!("change {} {}", descriptor.name, descriptor.params[*index].label),
        [(descriptor, _), rest @ ..] if rest.iter().all(|(d, _)| std::ptr::eq(*d, *descriptor)) => {
            format!("change {} settings", descriptor.name)
        }
        _ => format!("change {} parameters", changes.len()),
    }
}

/// Checks that a step can be applied, before anything changes (undo.md,
/// rule 10). A refusal leaves the patch and the history as they were.
pub fn check(step: &Step, forward: bool, controller: &Controller) -> Result<(), String> {
    let mut needed = 0;
    for (old, new) in step.modules.values() {
        let (have, want) = if forward { (old, new) } else { (new, old) };
        if let (None, Some(state)) = (have, want) {
            if controller.registry().resolve(&state.descriptor.kind()).is_err() {
                return Err(format!("{} is no longer available", state.descriptor.name));
            }
            if matches!(state.descriptor.role, ModuleRole::Dsp(_)) {
                needed += 1;
            }
        }
    }
    if needed > controller.free_module_slots() {
        return Err("the patch has no room for these modules".to_string());
    }
    Ok(())
}

/// Applies one side of a step: the new values going forward (redo), the old
/// ones going back (undo). The caller commits afterwards.
pub fn apply(
    step: &Step,
    forward: bool,
    controller: &mut Controller,
    positions: &mut BTreeMap<u32, Point>,
) -> Result<(), LoomError> {
    fn sides<T>(pair: &Pair<T>, forward: bool) -> (&Option<T>, &Option<T>) {
        if forward {
            (&pair.0, &pair.1)
        } else {
            (&pair.1, &pair.0)
        }
    }

    // Take away what must not exist afterwards, then add what must.
    for (&(module, param), pair) in &step.midi {
        let (have, want) = sides(pair, forward);
        if have.is_some() && have != want {
            controller.unmap_midi(ModuleId(module), param)?;
        }
    }
    for (&id, pair) in &step.assignments {
        let (have, want) = sides(pair, forward);
        if have.is_some() && have != want {
            controller.remove_assignment(id)?;
        }
    }
    for (&input, pair) in &step.cables {
        let (have, want) = sides(pair, forward);
        if have.is_some() && have != want {
            controller.disconnect(input)?;
        }
    }
    for (&id, pair) in &step.modules {
        if let (Some(_), None) = sides(pair, forward) {
            controller.remove_module(ModuleId(id))?;
            positions.remove(&id);
        }
    }
    for (&id, pair) in &step.modules {
        let (have, want) = sides(pair, forward);
        let Some(state) = want else { continue };
        if have.is_none() {
            controller.add_module_with_id(ModuleId(id), &state.descriptor.kind())?;
        }
        // Ranges before values, so a range never clamps a value the step
        // sets.
        for (index, range) in state.ranges.iter().enumerate() {
            let current = controller.graph().node(ModuleId(id)).and_then(|n| n.range(index));
            if let Some(range) = *range
                && current != Some(range)
            {
                controller.set_range(ModuleId(id), index, range)?;
            }
        }
        let current = controller.graph().node(ModuleId(id)).map(|node| node.params.clone());
        // For a module that is already there, replay only what the step
        // changed. A mapped parameter the hardware moved since is not the
        // step's to set (undo.md, Mapped parameters). A module that comes
        // back gets every value.
        let changes: Vec<(usize, f32)> = state
            .params
            .iter()
            .enumerate()
            .filter(|&(index, &value)| {
                have.as_ref()
                    .is_none_or(|other| other.params.get(index) != Some(&value))
                    && current.as_ref().is_none_or(|c| c.get(index) != Some(&value))
            })
            .map(|(index, &value)| (index, value))
            .collect();
        if !changes.is_empty() {
            controller.set_params(ModuleId(id), &changes)?;
        }
        match state.position {
            Some(position) => positions.insert(id, position),
            None => positions.remove(&id),
        };
    }
    for (&input, pair) in &step.cables {
        let (have, want) = sides(pair, forward);
        if let Some(output) = want
            && have != want
        {
            controller.connect(*output, input)?;
        }
    }
    for (&id, pair) in &step.assignments {
        let (have, want) = sides(pair, forward);
        if let Some(assignment) = want
            && have != want
        {
            controller.restore_assignment(id, *assignment)?;
        }
    }
    for (&(module, param), pair) in &step.midi {
        let (have, want) = sides(pair, forward);
        if let Some(control) = want
            && have != want
        {
            controller.map_midi(*control, ModuleId(module), param)?;
        }
    }
    Ok(())
}

/// The steps, and where the user is in them.
#[derive(Default)]
pub struct History {
    steps: Vec<Step>,
    /// Steps before this can be undone; steps from here can be redone.
    position: usize,
    /// The position at the last save, if the history can still reach it.
    saved: Option<usize>,
}

impl History {
    /// Adds a step after the current one. Steps that could be redone are
    /// dropped (rule 9), and the oldest goes when the history is full.
    pub fn push(&mut self, step: Step) {
        // The saved state is lost if it was among the steps that could be
        // redone: they are dropped now.
        if self.saved.is_some_and(|saved| saved > self.position) {
            self.saved = None;
        }
        self.steps.truncate(self.position);
        self.steps.push(step);
        if self.steps.len() > LIMIT {
            self.steps.remove(0);
            // Everything moved down one place. A saved state before the first
            // kept step cannot be reached again.
            self.saved = self.saved.and_then(|saved| saved.checked_sub(1));
        }
        self.position = self.steps.len();
    }

    /// The step undo would take back.
    pub fn undo_step(&self) -> Option<&Step> {
        self.position.checked_sub(1).and_then(|i| self.steps.get(i))
    }

    /// The step redo would replay.
    pub fn redo_step(&self) -> Option<&Step> {
        self.steps.get(self.position)
    }

    /// Moves back over the step undo took back.
    pub fn undone(&mut self) {
        self.position = self.position.saturating_sub(1);
    }

    /// Moves forward over the step redo replayed.
    pub fn redone(&mut self) {
        self.position = (self.position + 1).min(self.steps.len());
    }

    /// Forgets everything. The saved state is forgotten too: with no steps
    /// to compare, the patch counts as changed (`docs/design/files.md`).
    pub fn clear(&mut self) {
        self.steps.clear();
        self.position = 0;
        self.saved = None;
    }

    /// Notes that the patch as it is now is on disk.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.position);
    }

    /// True when the patch is the one last saved: nothing was done since, or
    /// everything done since was undone.
    pub fn is_saved(&self) -> bool {
        self.saved == Some(self.position)
    }

    /// How many steps are kept.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.steps.len()
    }
}

#[cfg(test)]
mod tests;
