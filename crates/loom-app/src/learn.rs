//! MIDI learn in the app: learn mode, the armed parameter, the labels on
//! tiles, and a watcher that wakes the UI when MIDI control changes arrive.
//! Spec: `docs/design/midi-learn.md`.

use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_canvas::{ControlPart, LearnView};
use loom_core::{Controller, MidiMapping, MidiReader, MidiWaiting, ModuleId};
use loom_patch::{Hierarchy, ModulePath};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// How often the watcher looks for new control changes. A hardware knob then
/// moves the knob on screen at the first redraw after the next check
/// (midi-learn.md, Acceptance criteria). A faster check would buy nothing a
/// player notices.
pub const INTERVAL: Duration = Duration::from_millis(15);

/// How long a ghost mark stays after its hardware last moved (midi-learn.md,
/// Mapping modes).
pub const GHOST_TIME: Duration = Duration::from_secs(1);

/// How often the app checks whether a ghost mark has run out, only while one
/// shows.
pub const GHOST_CHECK: Duration = Duration::from_millis(100);

/// A parameter: module ID and parameter index.
pub type Param = (u32, usize);

/// The armed parameter, and the serial of the last control change when it
/// was armed. Only a newer control change maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Armed {
    pub param: Param,
    pub serial: Option<u32>,
}

/// One waiting pickup whose hardware has moved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ghost {
    /// The parameter value the hardware stands for.
    value: f32,
    /// Changes when the hardware moves.
    sequence: u32,
    moved: Instant,
}

/// The ghost marks: where the hardware of each waiting pickup stands, for
/// [`GHOST_TIME`] after it last moved. It reads the time only in
/// [`update`](Self::update), so the view reads state and a test needs no real
/// wait.
#[derive(Debug, Default)]
pub struct Ghosts {
    marks: BTreeMap<Param, Ghost>,
    shown: BTreeMap<Param, f32>,
}

impl Ghosts {
    /// Brings the marks in line with the engine at time `now`. Returns true
    /// when what shows changed.
    pub fn update(&mut self, waiting: &[MidiWaiting], now: Instant) -> bool {
        let mut marks = BTreeMap::new();
        for w in waiting {
            let param = (w.target.0, w.param);
            let moved = match self.marks.get(&param) {
                Some(mark) if mark.sequence == w.sequence => mark.moved,
                _ => now,
            };
            marks.insert(
                param,
                Ghost {
                    value: w.value,
                    sequence: w.sequence,
                    moved,
                },
            );
        }
        let shown: BTreeMap<Param, f32> = marks
            .iter()
            .filter(|(_, mark)| now.saturating_duration_since(mark.moved) < GHOST_TIME)
            .map(|(&param, mark)| (param, mark.value))
            .collect();
        let changed = shown != self.shown;
        self.marks = marks;
        self.shown = shown;
        changed
    }

    /// The marks that show, by parameter, as parameter values. Each control
    /// places a mark on its own scale.
    pub fn shown(&self) -> &BTreeMap<Param, f32> {
        &self.shown
    }
}

/// What is typed in a mapping's range fields, until Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeDraft {
    /// The mapped parameter.
    pub param: Param,
    /// The low field's text.
    pub low: String,
    /// The high field's text.
    pub high: String,
    /// Why the text is not a range, once Enter has found out.
    pub error: Option<String>,
}

/// Watches for control changes. One for the whole app.
#[derive(Clone)]
pub struct Watch {
    pub reader: MidiReader,
    /// Counts the engines the app has built (see `lights::Watch`).
    pub generation: u64,
}

impl Hash for Watch {
    // The reader is the same for one engine, so the subscription restarts
    // only with a new engine.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "midi-learn".hash(state);
        self.generation.hash(state);
    }
}

/// Yields the serial of each new control change, checked every
/// [`INTERVAL`]. Several changes between checks give one message. The thread
/// stops when the subscription ends.
pub fn stream(watch: &Watch) -> impl Stream<Item = u32> + use<> {
    let (mut tx, rx) = mpsc::channel(1);
    let reader = watch.reader.clone();
    std::thread::spawn(move || {
        // Start from "nothing seen", so a control change that arrives while
        // the thread starts is still reported. The app ignores any that came
        // before a parameter was armed.
        let mut last = None;
        while !tx.is_closed() {
            // Control changes and note ons share one serial count.
            let note = reader.last_note_on().map(|seen| seen.serial);
            let now = reader.last_control_change().map(|seen| seen.serial).max(note);
            if let Some(serial) = now.filter(|_| now != last) {
                match tx.try_send(serial) {
                    Ok(()) => last = now,
                    // The UI has not taken the last one yet: it reads fresh
                    // values when it does, so try again next time.
                    Err(e) if e.is_full() => {}
                    Err(_) => break,
                }
            }
            std::thread::sleep(INTERVAL);
        }
    });
    rx
}

/// What the player asks of learn mode and of a parameter's MIDI mapping.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Turn learn mode on or off.
    Toggle,
    /// Arm a parameter, or disarm it if it is armed. Ignored while learn
    /// mode is off.
    Arm(Param),
    /// Remove a parameter's MIDI mapping.
    Unmap { module: u32, index: usize },
    /// Open or close the options rows of a parameter's MIDI mapping.
    MidiOptionsRows { module: u32, index: usize },
    /// Change the options of a parameter's MIDI mapping.
    MidiOptions {
        module: u32,
        index: usize,
        options: loom_core::MidiMappingOptions,
    },
    /// Text typed in a MIDI mapping's range fields.
    MidiRangeText {
        module: u32,
        index: usize,
        low: String,
        high: String,
    },
    /// Enter in a MIDI mapping's range fields.
    MidiRangeSubmit { module: u32, index: usize },
}

/// Learn mode's state: whether it is on, the armed parameter, and what the
/// canvas shows. The app owns one and reaches it through these methods;
/// mapping a control and the status bar stay with the app, because they
/// touch the controller and undo.
#[derive(Default)]
pub struct Learn {
    on: bool,
    armed: Option<Armed>,
    /// Rebuilt by [`refresh`](Self::refresh) when mappings or the armed
    /// parameter change.
    view: LearnView,
    /// The mapping whose options rows are open in the inspector. App state,
    /// not saved in the patch.
    options_open: Option<Param>,
    /// The text in the open rows' range fields.
    range_draft: Option<RangeDraft>,
    /// The ghost marks of waiting pickups.
    ghosts: Ghosts,
}

impl Learn {
    /// True while learn mode is on.
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Turns learn mode on. Nothing is armed yet.
    pub fn start(&mut self) {
        self.on = true;
    }

    /// Turns learn mode off and disarms.
    pub fn stop(&mut self) {
        self.on = false;
        self.armed = None;
    }

    /// The armed parameter, if any.
    pub fn armed(&self) -> Option<Armed> {
        self.armed
    }

    /// True if `param` is armed.
    pub fn is_armed(&self, param: Param) -> bool {
        self.armed.is_some_and(|armed| armed.param == param)
    }

    /// Arms `param`. Only a control change newer than `serial`, the last
    /// one seen now, maps it.
    pub fn arm(&mut self, param: Param, serial: Option<u32>) {
        self.armed = Some(Armed { param, serial });
    }

    /// True if `param`'s mapping options rows are open.
    pub fn options_open(&self, param: Param) -> bool {
        self.options_open == Some(param)
    }

    /// Opens `param`'s options rows, or closes them if open. Only one is open
    /// at a time, and a draft does not outlive its rows.
    pub fn toggle_options(&mut self, param: Param) {
        self.options_open = (self.options_open != Some(param)).then_some(param);
        self.range_draft = None;
    }

    /// The draft in `param`'s range fields, if any.
    pub fn range_draft(&self, param: Param) -> Option<&RangeDraft> {
        self.range_draft.as_ref().filter(|draft| draft.param == param)
    }

    /// Sets or clears the draft in the range fields.
    pub fn set_range_draft(&mut self, draft: Option<RangeDraft>) {
        self.range_draft = draft;
    }

    /// The ghost marks.
    pub fn ghosts(&self) -> &Ghosts {
        &self.ghosts
    }

    /// Brings the ghost marks in line with the engine. Returns true when what
    /// shows changed.
    pub fn update_ghosts(&mut self, waiting: &[MidiWaiting], now: Instant) -> bool {
        self.ghosts.update(waiting, now)
    }

    /// Disarms, keeping learn mode on.
    pub fn disarm(&mut self) {
        self.armed = None;
    }

    /// What the canvas shows while learn mode is on.
    pub fn shown(&self) -> Option<&LearnView> {
        self.on.then_some(&self.view)
    }

    /// Rebuilds what the canvas shows, after mappings, the armed parameter,
    /// or the shown level changed. A removed module disarms its parameter.
    pub fn refresh(&mut self, controller: &Controller, hierarchy: &Hierarchy, level: &ModulePath) {
        if self
            .armed
            .is_some_and(|armed| controller.graph().node(ModuleId(armed.param.0)).is_none())
        {
            self.armed = None;
        }
        self.view = view(controller, hierarchy, level, self.armed);
    }
}

/// What learn mode shows on the canvas: the armed control and a label on
/// every mapped control, on module tiles and on the surface controls of
/// the sub-patch tiles of `level`, which stand for inner parameters
/// (sub-patches.md, Exposing).
pub fn view(controller: &Controller, hierarchy: &Hierarchy, level: &ModulePath, armed: Option<Armed>) -> LearnView {
    let graph = controller.graph();
    let mut view = LearnView::default();
    for (id, node) in graph.nodes() {
        let d = node.descriptor;
        for (widget, w) in d.panel.widgets.iter().enumerate() {
            if !w.is_control() {
                continue;
            }
            // A widget with two values shows no armed state or label on the
            // tile. Its hover text names each mapping (midi-learn.md).
            if w.kind.is_two_numbers() {
                for (part, name) in [ControlPart::Main, ControlPart::Second].into_iter().zip(w.params()) {
                    let mapping = d.param_index(name).and_then(|param| graph.midi_mapping_of(id, param));
                    if let Some((_, mapping)) = mapping {
                        view.part_labels
                            .insert((id.0, widget, part), format!("CC {}", mapping.control.controller));
                    }
                }
                continue;
            }
            let Some(param) = d.param_index(w.bind()) else {
                continue;
            };
            if armed.is_some_and(|a| a.param == (id.0, param)) {
                view.armed = Some((id.0, widget));
            }
            if let Some((_, mapping)) = graph.midi_mapping_of(id, param) {
                view.labels
                    .insert((id.0, widget), format!("CC {}", mapping.control.controller));
            }
        }
    }
    // The surface controls of the level's sub-patch tiles stand for inner
    // parameters, which are not on this level, so they take the armed mark
    // and the labels here.
    for (id, tile) in hierarchy.structural_tiles(level) {
        if !matches!(tile, loom_patch::Tile::Instance(_)) {
            continue;
        }
        let count = hierarchy.definition_of(id).map_or(0, |d| d.exposed.len());
        for widget in 0..count {
            let Some(target) = crate::surface::control_target(graph, hierarchy, id.0, widget, ControlPart::Main) else {
                continue;
            };
            if armed.is_some_and(|a| a.param == (target.module.0, target.index)) {
                view.armed = Some((id.0, widget));
            }
            if let Some((_, mapping)) = graph.midi_mapping_of(target.module, target.index) {
                view.labels
                    .insert((id.0, widget), format!("CC {}", mapping.control.controller));
            }
        }
    }
    view
}

/// A parameter as the status bar names it: "Filter #3 Cutoff".
pub fn param_name(controller: &Controller, (module, param): Param) -> String {
    controller
        .graph()
        .node(ModuleId(module))
        .and_then(|node| {
            if let Some(unknown) = &node.unknown {
                return Some(format!(
                    "{} #{module} {}",
                    node.label.as_deref().unwrap_or(&unknown.kind),
                    unknown.parameters.get(param)?
                ));
            }
            let spec = node.descriptor.params.get(param)?;
            Some(format!("{} #{module} {}", node.descriptor.name, spec.label))
        })
        .unwrap_or_else(|| format!("#{module}"))
}

/// The status after a mapping is made. Names the parameter a control left,
/// if it was mapped elsewhere before (spec, Making a mapping).
pub fn mapped_status(controller: &Controller, mapping: MidiMapping, removed: &[MidiMapping]) -> String {
    let target = (mapping.target.0, mapping.param);
    let mut status = format!("Mapped {} to {}", mapping.control, param_name(controller, target));
    if let Some(left) = removed
        .iter()
        .find(|old| old.control == mapping.control && (old.target.0, old.param) != target)
    {
        status.push_str(&format!(
            ". It no longer moves {}",
            param_name(controller, (left.target.0, left.param))
        ));
    }
    status
}

#[cfg(test)]
mod tests;
