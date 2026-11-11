//! The inspector: what it shows for the selection, as plain data, and the
//! widgets drawn from that data.
//!
//! The rows are built first as a small model, then turned into widgets. The
//! window and [`crate::App::ui_snapshot`] both read the same model, so a
//! test of the snapshot checks what the user sees.

use crate::modulation::{self, PolarityChoice};
use crate::style::{self, button};
use crate::view::remove_button;
use crate::{App, DIM, Message as AppMessage, param};
use iced::widget::{
    checkbox, column, container, mouse_area, pick_list, radio, row, scrollable, slider, space, stack, text, text_input,
    tooltip,
};
use iced::{Element, Length, Task, Theme};
use loom_core::{AssignmentId, MidiEncoding, MidiMappingOptions, MidiMode, ModuleId, OutputRef, ParamSpec, WidgetKind};
use loom_ui_core::UiId;
use serde::Serialize;

pub(crate) mod ids;

/// What the inspector's controls send. The control channel sends the two that
/// set a parameter too.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A slider moved to a position from 0 to 1.
    Param { module: u32, index: usize, position: f32 },
    /// An exact parameter value, in the parameter's units, sent as a knob
    /// turn is. The control channel's `set` with a `value` uses it, so the
    /// stored number is the one sent and not a round trip through a position.
    ParamValue { module: u32, index: usize, value: f32 },
    /// Text typed in a value field (panel-widgets.md, Typing a value).
    ValueText { module: u32, index: usize, text: String },
    /// Enter in a value field.
    ValueSubmit { module: u32, index: usize },
    /// Escape with a draft in a value field: it is dropped.
    ValueDraftClear,
    /// A choice name was picked: value `min + choice` of the parameter, as one
    /// undo step (panels.md, Choice names).
    Choice { module: u32, index: usize, choice: usize },
    /// A range was picked in a parameter's **Range** selector: index `range`
    /// of the ranges it offers (`docs/design/control-ranges.md`).
    Range { module: u32, index: usize, range: usize },
    /// A line under **Drives** selects its target module.
    SelectModule(u32),
    /// Edit a placeholder label without changing the patch yet.
    UnknownLabelDraft { module: u32, value: String },
    /// Apply the placeholder label as one undo step.
    UnknownLabelApply { module: u32 },
    /// The eligible parameter currently under an output drag.
    DropRow(Option<(u32, usize)>),
    /// Puts a parameter on the enclosing sub-patch's surface: parameter
    /// `index` of module `module`, or the `index`th surface item of the
    /// sub-patch tile `module`, exposed one level further out
    /// (sub-patches.md, Exposing).
    Expose { module: u32, index: usize },
    /// Takes the `index`th surface item off the sub-patch tile `module`,
    /// and off every surface above that re-exposed it.
    Unexpose { module: u32, index: usize },
    /// Edit a sub-patch's name without changing the patch yet.
    SubPatchNameDraft { module: u32, value: String },
    /// Apply the sub-patch's name as one undo step.
    SubPatchNameApply { module: u32 },
}

/// Width of a parameter's value field, in pixels.
const VALUE_FIELD_WIDTH: f32 = 84.0;

/// What the inspector shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Inspector {
    /// Nothing selected: patch info, then help text.
    Patch(crate::patch_info::PatchRows),
    /// Several modules selected: how many.
    Several(usize),
    /// One module selected.
    Module(ModuleRows),
    /// A missing module and its preserved, read-only data.
    Unknown(UnknownRows),
    /// A sub-patch tile: its name and ports (sub-patches.md, Editor model).
    SubPatch(SubPatchRows),
    /// A slot tile: its part, the parts it can swap to, and the part's
    /// parameters, the role's first (sub-patches.md, Slots).
    Slot(SlotRows),
    /// A Patch input or output tile inside a sub-patch.
    Port(PortRows),
    /// The selected module is gone. Nothing is shown.
    Empty,
}

/// The inspector's view of a sub-patch tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SubPatchRows {
    /// The tile's id.
    pub id: u32,
    /// The definition's name, or the name being typed for it.
    pub name: String,
    /// The input ports, in order.
    pub inputs: Vec<String>,
    /// The output ports, in order.
    pub outputs: Vec<String>,
    /// One row per exposed parameter, in exposure order: the inner
    /// parameter's row, named by its surface name (sub-patches.md,
    /// Exposing). A select's Member control comes first.
    pub params: Vec<ExposedRow>,
    /// The mode (sub-patches.md, The editor for modes).
    pub mode: loom_patch::Mode,
    /// Layer and select: the members, in order.
    pub members: Vec<MemberRow>,
    /// Stack: the copy count, 1 to 16.
    pub copies: u32,
    /// Stack: stereo spread on.
    pub stereo_spread: bool,
    /// Stack: one spread row per exposed item.
    pub spread: Vec<SpreadRow>,
}

/// A member of a layer or select, in the sub-patch inspector.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberRow {
    /// The member tile's id.
    pub id: u32,
    /// Its name.
    pub name: String,
}

/// The spread of one exposed item of a stack, 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpreadRow {
    /// The exposed name.
    pub name: String,
    /// The amount, a share of the knob; 0 is not spread.
    pub amount: f32,
}

/// The inspector's view of a slot tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SlotRows {
    /// The tile's id.
    pub id: u32,
    /// The slot's name.
    pub name: String,
    /// The role's display name.
    pub role: String,
    /// The current part's name, or `None` for an empty slot.
    pub part: Option<String>,
    /// The flat id of the part, whose rows the parameters are.
    pub part_id: Option<u32>,
    /// Every part the slot can take, the current one marked: the **Part**
    /// list.
    pub choices: Vec<crate::slots::SwapChoice>,
    /// The part's parameters that are the role's, as `(label, module, row)`:
    /// the module is the part for a module part, and the inner module an
    /// exposed item stands for on a sub-patch part, as the row's messages
    /// name it.
    pub role_params: Vec<(String, u32, ParamRow)>,
    /// The part's other parameters, under a heading with the part's name.
    pub other_params: Vec<(String, u32, ParamRow)>,
}

/// A surface item's row on a sub-patch tile's inspector.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExposedRow {
    /// The inner module whose parameter the row edits.
    pub module: u32,
    /// The surface name, shown in place of the parameter's label.
    pub label: String,
    /// The Unexpose button for this tile's own surface. The row's own
    /// `exposure` is for the enclosing sub-patch, when there is one.
    pub own: ExposureRow,
    /// The parameter's row, with the inner parameter's index.
    pub row: ParamRow,
    /// True for a select's Member control, which cannot be unexposed.
    pub member: bool,
}

/// The Expose or Unexpose button on a parameter's row (sub-patches.md,
/// Exposing). The button names the surface it acts on: "Expose on Voice",
/// or "as Cutoff" and "Unexpose on Voice".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExposureRow {
    /// The tile the message names: for Expose, the row's own tile (a
    /// module, or a sub-patch tile whose surface item is exposed further
    /// out); for Unexpose, the sub-patch tile whose surface holds the item.
    pub module: u32,
    /// The row's index on that tile: the parameter's index on a module, or
    /// the item's index on the sub-patch's surface.
    pub index: usize,
    /// The surface name it is exposed as, or `None` when it is not exposed.
    pub name: Option<String>,
    /// The name of the sub-patch whose surface the button acts on.
    pub surface: String,
}

/// The inspector's view of a port tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortRows {
    /// The tile's id.
    pub id: u32,
    /// "Patch input" or "Patch output".
    pub direction: String,
    /// The port's name, which the sub-patch tile shows outside.
    pub name: String,
}

/// The inspector's view of a placeholder.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnknownRows {
    /// Module ID for removing a MIDI mapping.
    pub id: u32,
    /// Original module kind and version.
    pub kind: String,
    /// The saved label or its unapplied inspector draft.
    pub label: String,
    /// Saved parameter values.
    pub params: Vec<(String, f32)>,
    /// Existing modulation and MIDI links.
    pub links: Vec<UnknownLink>,
}

/// A preserved link and the action that removes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnknownLink {
    /// Modulation assignment touching the placeholder.
    Assignment { id: u32, text: String },
    /// MIDI mapping to a placeholder parameter.
    Midi { index: usize, text: String },
}

/// The inspector for one module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModuleRows {
    pub id: u32,
    pub name: &'static str,
    /// The module's kind and ID, under its name.
    pub kind: String,
    pub description: &'static str,
    /// Input and output descriptions with their nominal ranges.
    pub ports: Vec<PortRow>,
    /// One row per parameter, in descriptor order. Empty shows "No
    /// parameters."
    pub params: Vec<ParamRow>,
    /// **Drives**: one group per output that drives at least one
    /// assignment. Empty shows no **Drives** section.
    pub drives: Vec<DrivesGroup>,
    /// True for a module with a meter on an output, which shows a **Reset
    /// clip** button: a display takes no keyboard focus (panels.md, Meter
    /// widget).
    pub resets_clip: bool,
}

/// One port shown in the inspector.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortRow {
    /// Whether the port is an input or an output.
    pub direction: &'static str,
    /// Human-readable port label.
    pub label: &'static str,
    /// Port description and nominal range, shown in hover text.
    pub description: String,
    /// Signal kind and nominal range, such as "audio, -1 to 1", shown on
    /// the port's line.
    pub details: String,
}

/// One parameter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParamRow {
    pub index: usize,
    pub label: &'static str,
    /// Hover text on the label.
    pub description: &'static str,
    /// The name in patch files, under the description, when it differs
    /// from the label.
    pub name: Option<String>,
    /// The value as text, with its unit.
    pub value: String,
    /// The value field beside the slider, for a parameter with no choice
    /// names, outside MIDI learn mode (panel-widgets.md, Typing a value).
    pub field: Option<ValueField>,
    /// The MIDI control mapped to it.
    pub mapping: Option<String>,
    /// The mapping's mode, range, and ghost mark.
    pub mapping_options: Option<MappingRow>,
    /// The **Range** selector, for a parameter that offers ranges.
    pub range: Option<RangeRow>,
    /// "Not used in AD" while the module's mode hides this parameter's
    /// control on the tile (ADR-0030). The row stays, so it can be set.
    pub unused: Option<String>,
    pub mode: ParamMode,
    /// Whether a dragged output can target this row.
    #[serde(skip)]
    pub accepts_drop: bool,
    /// The Expose or Unexpose button, for a parameter inside a sub-patch.
    /// `None` at the top level, which has no surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<ExposureRow>,
    /// The row's UI ID, from what it shows: the tile, the kind of item,
    /// and its name (`ids.rs`). The view names the row's parts by it, and
    /// the accessibility tree gives the same ID to its nodes.
    #[serde(skip)]
    pub ui: UiId,
}

/// A MIDI mapping's options on a parameter's row (midi-learn.md, Mapping
/// modes).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MappingRow {
    /// The mode's name.
    pub mode: &'static str,
    /// The encoding's name, for a Relative mapping.
    pub encoding: Option<&'static str>,
    /// The low field's text.
    pub low: String,
    /// The high field's text.
    pub high: String,
    /// Why the range text is not a range, if it is not.
    pub range_error: Option<String>,
    /// Whether Invert is on.
    pub invert: bool,
    /// Whether the rows are open.
    pub open: bool,
    /// Where a waiting pickup's hardware stands, from 0 to 1, while its
    /// ghost mark shows on this row's slider. `None` on a row with no slider.
    pub ghost: Option<f32>,
    /// A friendly note when a sub-patch surface narrows what MIDI sweeps:
    /// the range and the surface that gives it (sub-patches.md, MIDI follows
    /// the range). `None` for a parameter no surface narrows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_note: Option<String>,
    /// The options as the controller holds them, for the messages that
    /// change one field.
    #[serde(skip)]
    pub options: MidiMappingOptions,
}

/// What a parameter's value field shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValueField {
    /// The draft being typed, or else the value as text.
    pub text: String,
    /// Why the draft is not a value, if it is not.
    pub error: Option<String>,
}

/// The **Range** selector next to a parameter's slider
/// (`docs/design/control-ranges.md`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RangeRow {
    /// Each range's name, such as "-0.1 to 0.1", from the smallest up.
    pub choices: Vec<String>,
    /// Index of the selected range, or `None` for the parameter's full
    /// range when no listed range held a loaded value.
    pub selected: Option<usize>,
}

/// How a parameter row behaves.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamMode {
    /// MIDI learn mode: the row arms its parameter, and shows `detail` in
    /// place of the value (`docs/design/midi-learn.md`).
    Learn { armed: bool, detail: String },
    /// A slider, or the choice names, then the assignments on the
    /// parameter, and **Add modulation**.
    Edit {
        /// Slider position from 0 to 1.
        position: f32,
        /// Slider step, as a position.
        step: f32,
        /// What edits the value: a slider, or its choice names.
        control: ParamControl,
        /// One per assignment, in engine order.
        assignments: Vec<AssignmentRow>,
        /// The **Add modulation** list, while it is open.
        adding: Option<Vec<SourceRow>>,
    },
}

/// What edits a parameter in the inspector (panels.md, Choice names).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamControl {
    /// A slider, for a parameter without choice names.
    Slider,
    /// Radio buttons, one per choice name, for 4 names or fewer.
    Radio {
        choices: Vec<&'static str>,
        /// Index of the current value's name.
        selected: usize,
    },
    /// A pick list, for more than 4 names.
    PickList {
        choices: Vec<&'static str>,
        /// Index of the current value's name.
        selected: usize,
    },
}

/// Most choice names the inspector shows as radio buttons. More get a pick
/// list, so a long list does not push the other rows down.
pub const MAX_RADIO_CHOICES: usize = 4;

impl ParamControl {
    /// The control for `spec`, showing `value`.
    pub fn of(spec: &loom_core::ParamSpec, value: f32) -> Self {
        let choices = spec.options.choices;
        if choices.is_empty() {
            return Self::Slider;
        }
        let selected = (spec.clamp(value) - spec.min).max(0.0) as usize;
        let choices = choices.to_vec();
        if choices.len() <= MAX_RADIO_CHOICES {
            Self::Radio { choices, selected }
        } else {
            Self::PickList { choices, selected }
        }
    }
}

/// One modulation assignment on a parameter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssignmentRow {
    #[serde(skip)]
    pub id: AssignmentId,
    /// The source output's name.
    pub source: String,
    pub depth: f32,
    /// The depth as text.
    pub depth_text: String,
    #[serde(serialize_with = "display")]
    pub polarity: PolarityChoice,
    /// The polarity choices offered.
    #[serde(skip)]
    pub polarities: [PolarityChoice; 3],
    pub enabled: bool,
}

/// One output in the **Add modulation** list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceRow {
    #[serde(skip)]
    pub source: OutputRef,
    pub name: String,
    /// False when it already drives the parameter: greyed out, and it
    /// cannot be picked.
    pub enabled: bool,
}

/// What one output drives.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DrivesGroup {
    /// The output's label.
    pub output: &'static str,
    pub lines: Vec<DrivesLine>,
}

/// One line under **Drives**. It selects its target.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DrivesLine {
    pub target: u32,
    pub text: String,
    /// False for an assignment that is off: its text ends in "(off)" and
    /// is dimmed.
    pub enabled: bool,
}

fn display<S: serde::Serializer>(value: &impl std::fmt::Display, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(value)
}

impl App {
    /// Handles a message from the inspector. Only a submitted value returns a
    /// task, which moves focus.
    pub(crate) fn inspector_message(&mut self, message: Message) -> Task<AppMessage> {
        match message {
            Message::ValueSubmit { module, index } => return self.value_submit(module, index),
            Message::UnknownLabelDraft { module, value } => self.session.gestures.draft_label(module, value),
            Message::UnknownLabelApply { module } => {
                if let Some(value) = self.session.gestures.take_label(module) {
                    let label = (!value.is_empty()).then_some(value);
                    self.edit(|app| {
                        if app.session.controller.set_label(ModuleId(module), label) {
                            app.refresh();
                        }
                    });
                }
            }
            Message::DropRow(row) => {
                let allowed = !self.session.learn.is_on();
                self.session.gestures.hover_row(row, allowed);
            }
            Message::Expose { module, index } => self.expose(module, index),
            Message::Unexpose { module, index } => self.unexpose(module, index),
            Message::SubPatchNameDraft { module, value } => self.session.gestures.draft_label(module, value),
            Message::SubPatchNameApply { module } => {
                if let Some(value) = self.session.gestures.take_label(module) {
                    self.rename_subpatch(module, &value);
                }
            }
            Message::Param {
                module,
                index,
                position,
            } => {
                let value = self
                    .session
                    .controller
                    .graph()
                    .node(ModuleId(module))
                    .and_then(|n| n.spec(index))
                    .map(|spec| spec.denormalize(position));
                if let Some(value) = value {
                    self.turn_param(module, index, value);
                }
            }
            Message::ParamValue { module, index, value } => {
                let value = self
                    .session
                    .controller
                    .graph()
                    .node(ModuleId(module))
                    .and_then(|n| n.spec(index))
                    .map(|spec| spec.clamp(value));
                if let Some(value) = value {
                    self.turn_param(module, index, value);
                }
            }
            Message::Choice { module, index, choice } => {
                let value = self
                    .session
                    .controller
                    .graph()
                    .node(ModuleId(module))
                    .and_then(|n| n.spec(index))
                    .map(|spec| spec.clamp(spec.min + choice as f32));
                if let Some(value) = value {
                    self.turn_param(module, index, value);
                    self.close_slider_gesture();
                }
            }
            Message::ValueText { module, index, text } => self.type_value_draft(module, index, text),
            Message::ValueDraftClear => self.clear_value_draft(),
            Message::Range { module, index, range } => self.set_range(module, index, range),
            Message::SelectModule(id) => {
                if self.session.controller.graph().node(ModuleId(id)).is_some() {
                    self.session.selection = std::collections::BTreeSet::from([id]);
                }
            }
        }
        Task::none()
    }
}

impl App {
    /// One parameter's row: parameter `index` of module `id`, edited
    /// through `spec` (the module's selected control range, or the exposed
    /// range on a sub-patch surface), with `exposure` as its Expose button
    /// and the Range selector only when `show_range`. `None` when the module
    /// is gone.
    fn param_row(
        &self,
        id: u32,
        index: usize,
        spec: &ParamSpec,
        exposure: Option<ExposureRow>,
        show_range: bool,
    ) -> Option<ParamRow> {
        let graph = self.session.controller.graph();
        let node = graph.node(ModuleId(id))?;
        let d = node.descriptor;
        let value = *node.params.get(index)?;
        let range = spec.options.ranges.first().filter(|_| show_range).map(|_| RangeRow {
            choices: spec.options.ranges.iter().map(ToString::to_string).collect(),
            selected: node
                .range(index)
                .and_then(|r| spec.options.ranges.iter().position(|&o| o == r)),
        });
        // Hovering the label shows what the canvas hover text shows
        // (panels.md, Labels and hover text).
        let hover = loom_canvas::HoverText::new(spec.label, spec.description, spec.name);
        let mapping = graph
            .midi_mapping_of(ModuleId(id), index)
            .map(|(_, m)| m.control.to_string());
        let mapping_options = graph.midi_mapping_of(ModuleId(id), index).map(|(_, m)| {
            let draft = self.session.learn.range_draft((id, index));
            MappingRow {
                mode: mode_name(m.options.mode),
                encoding: encoding_of(m.options.mode).map(encoding_name),
                low: draft.map_or_else(|| m.options.low.to_string(), |d| d.low.clone()),
                high: draft.map_or_else(|| m.options.high.to_string(), |d| d.high.clone()),
                range_error: draft.and_then(|d| d.error.clone()),
                invert: m.options.invert,
                open: self.session.learn.options_open((id, index)),
                // Only a slider has a travel to mark, and a learn
                // mode row has no slider (midi-learn.md, Mapping modes).
                // The mark sits on this row's scale: the knob's, or the
                // exposed range on a sub-patch row.
                ghost: self
                    .session
                    .learn
                    .ghosts()
                    .shown()
                    .get(&(id, index))
                    .map(|&value| spec.normalize(value).clamp(0.0, 1.0))
                    .filter(|_| spec.options.choices.is_empty() && !self.session.learn.is_on()),
                range_note: self.range_note(id, index, m.options.mode),
                options: m.options,
            }
        });
        let mode = if self.session.learn.is_on() {
            // In learn mode a row arms its parameter, and its value
            // does not change (midi-learn.md).
            let armed = self.session.learn.is_armed((id, index));
            let detail = match (armed, &mapping) {
                (true, _) => "Waiting for MIDI...".to_string(),
                (false, Some(control)) => control.clone(),
                (false, None) => param::format(spec, value),
            };
            ParamMode::Learn { armed, detail }
        } else {
            ParamMode::Edit {
                position: spec.normalize(value),
                step: param::slider_step(spec),
                control: ParamControl::of(spec, value),
                assignments: self.assignment_rows(id, index),
                adding: (self.session.gestures.adding() == Some((id, index))).then(|| self.sources(id, index)),
            }
        };
        let unused = (!d.is_shown(index, &node.params)).then(|| {
            let mode = spec.options.shown_when.and_then(|w| d.param_index(w.param));
            match mode.map(|m| (&d.params[m], node.params[m])) {
                Some((mode, value)) => match mode.choice_name(value) {
                    Some(choice) => format!("Not used in {choice}"),
                    // An on and off switch, such as Scope's Auto.
                    None => format!(
                        "Not used while {} is {}",
                        mode.label,
                        if value > 0.5 { "on" } else { "off" }
                    ),
                },
                None => "Not used in this mode".to_string(),
            }
        });
        let shown = param::format(spec, value);
        let field = (matches!(mode, ParamMode::Edit { .. }) && spec.options.choices.is_empty()).then(|| {
            match self
                .session
                .value_draft
                .as_ref()
                .filter(|d| d.module == id && d.index == index)
            {
                Some(value) => ValueField {
                    text: value.draft.text.clone(),
                    error: value.draft.error().map(str::to_string),
                },
                None => ValueField {
                    text: shown.clone(),
                    error: None,
                },
            }
        });
        Some(ParamRow {
            index,
            label: spec.label,
            description: spec.description,
            name: hover.name,
            unused,
            field,
            value: shown,
            mapping,
            mapping_options,
            range,
            mode,
            accepts_drop: {
                let bound = d.panel.widgets.iter().filter(|w| w.binds(spec.name));
                bound.clone().next().is_none() || bound.into_iter().any(|w| w.kind.takes_modulation())
            },
            exposure,
            ui: ids::row(id, ids::Kind::Param, spec.name),
        })
    }

    /// The note on a mapping row when a sub-patch surface narrows what MIDI
    /// sweeps on parameter `index` of `module`: the span the engine sweeps
    /// ([`Node::midi_spec`](loom_core::Node::midi_spec), the function the
    /// compiler uses, so the text cannot drift from what plays), and the
    /// outermost surface with a range of its own, which is the one that sets
    /// the span, as `fold_ranges` composes it. `None` when no surface
    /// narrows it.
    fn range_note(&self, module: u32, index: usize, mode: MidiMode) -> Option<String> {
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        let node = graph.node(ModuleId(module))?;
        node.limit(index)?;
        let spec = node.spec(index)?;
        let swept = node.midi_spec(index)?;
        // Every surface with a range, from the inside out.
        let mut at = (ModuleId(module), node.descriptor.params.get(index)?.name.to_string());
        let mut chain = Vec::new();
        while let Some((instance, name)) = hierarchy.exposure_of(at.0, &at.1) {
            let range = hierarchy
                .definition_of(instance)
                .and_then(|d| d.exposed.iter().find(|e| e.name == name))
                .and_then(|e| e.range);
            if let Some(range) = range {
                chain.push((instance, range));
            }
            at = (instance, name);
        }
        // Folded outer first, as `fold_ranges` does: a range that does not
        // overlap what came before replaces it, so the surface named is the
        // outermost one whose range still takes part in the span.
        let mut folded: Option<((f32, f32), ModuleId)> = None;
        for &(instance, (min, max)) in chain.iter().rev() {
            folded = Some(match folded {
                Some(((outer_min, outer_max), surface)) if outer_min.max(min) < outer_max.min(max) => {
                    ((outer_min.max(min), outer_max.min(max)), surface)
                }
                _ => ((min, max), instance),
            });
        }
        let name = hierarchy.tile_name(folded?.1)?;
        let span = format!(
            "{} to {}",
            crate::param::format(&spec, swept.min),
            crate::param::format(&spec, swept.max)
        );
        Some(match mode {
            MidiMode::Relative(_) => format!("Each step is a share of {span}, the range exposed on {name}."),
            _ => format!("Low and High are a share of {span}, the range exposed on {name}."),
        })
    }

    /// The Expose or Unexpose button of a row inside a sub-patch: for
    /// `param` of the tile `tile` (a module's parameter, or a sub-patch
    /// tile's surface item named `param`), where `index` is what the
    /// button's message names. `None` at the top level, which has no
    /// surface to expose onto.
    fn exposure_row(&self, tile: u32, index: usize, param: &str) -> Option<ExposureRow> {
        let hierarchy = &self.session.hierarchy;
        let level = hierarchy.level_of(ModuleId(tile));
        let surface = hierarchy.definition_at(&level)?.metadata.name.clone();
        Some(match hierarchy.exposure_of(ModuleId(tile), param) {
            // Unexpose names the item on the enclosing tile's surface.
            Some((enclosing, name)) => ExposureRow {
                module: enclosing.0,
                index: hierarchy
                    .definition_of(enclosing)
                    .and_then(|d| d.exposed.iter().position(|e| e.name == name))
                    .unwrap_or(index),
                name: Some(name),
                surface,
            },
            None => ExposureRow {
                module: tile,
                index,
                name: None,
                surface,
            },
        })
    }

    /// The inspector's rows for the slot tile `id`: its part, the Part
    /// list, and the part's parameter rows, the role's first (sub-patches.md,
    /// Slots). A sub-patch part shows its surface items as the role's rows.
    fn slot_rows(&self, id: u32) -> Inspector {
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        let Some(state) = hierarchy.slot(ModuleId(id)) else {
            return Inspector::Empty;
        };
        let part = hierarchy.part_of_slot(ModuleId(id));
        // A slot's rows are its part's, keyed under the part as its tile
        // controls are, so a swap makes new elements (ui-layer.md,
        // section 4). A part with no key keeps the rows' own IDs.
        let under = part.and_then(|part| {
            let slot = ids::panel(id);
            match graph.node(part) {
                Some(node) => Some(slot.with(node.descriptor.id)),
                None => hierarchy
                    .key_at(&hierarchy.path_of(part))
                    .map(|key| slot.with("subpatch").with(key)),
            }
        });
        let keyed = |mut row: ParamRow, kind: ids::Kind, name: &str| {
            if let Some(under) = &under {
                row.ui = under.clone().with(kind.key()).with(name);
            }
            row
        };
        let mut role_params = Vec::new();
        let mut other_params = Vec::new();
        let mut part_name = None;
        if let Some(part) = part {
            if let Some(node) = graph.node(part) {
                part_name = Some(node.descriptor.name.to_string());
                for (index, spec) in node.descriptor.params.iter().enumerate() {
                    if spec.is_transient() {
                        continue;
                    }
                    let Some(spec) = node.spec(index) else { continue };
                    let exposure = self.exposure_row(part.0, index, spec.name);
                    let exposed = exposure.as_ref().is_some_and(|e| e.name.is_some());
                    let Some(row) = self.param_row(part.0, index, &spec, exposure, !exposed) else {
                        continue;
                    };
                    let row = keyed(row, ids::Kind::Param, spec.name);
                    if state.role_param_of(spec.name).is_some() {
                        role_params.push((spec.label.to_string(), part.0, row));
                    } else {
                        other_params.push((spec.label.to_string(), part.0, row));
                    }
                }
            } else if let Some(definition) = hierarchy.definition_of(part) {
                part_name = Some(definition.metadata.name.clone());
                for (index, exposed) in definition.exposed.iter().enumerate() {
                    let Some(target) =
                        crate::surface::control_target(graph, hierarchy, part.0, index, loom_canvas::ControlPart::Main)
                    else {
                        continue;
                    };
                    let exposure = self.exposure_row(part.0, index, &exposed.name);
                    let Some(row) = self.param_row(target.module.0, target.index, &target.spec, exposure, false) else {
                        continue;
                    };
                    let row = keyed(row, ids::Kind::Exposed, &exposed.name);
                    if state.role.param(&exposed.name).is_some() {
                        role_params.push((exposed.name.clone(), target.module.0, row));
                    } else {
                        other_params.push((exposed.name.clone(), target.module.0, row));
                    }
                }
            }
        }
        Inspector::Slot(SlotRows {
            id,
            name: state.name(),
            role: state.role.name.to_string(),
            part: part_name,
            part_id: part.map(|p| p.0),
            choices: self.swap_choices(id),
            role_params,
            other_params,
        })
    }

    /// The inspector's content for the current selection.
    pub(crate) fn inspector_model(&self) -> Inspector {
        let graph = self.session.controller.graph();
        let mut selected = self.session.selection.iter();
        let id = match (selected.next(), selected.next()) {
            (None, _) => return Inspector::Patch(self.patch_rows()),
            (Some(&id), None) => id,
            (Some(_), Some(_)) => return Inspector::Several(self.session.selection.len()),
        };
        let hierarchy = &self.session.hierarchy;
        match hierarchy.tile(graph, ModuleId(id)) {
            Some(loom_patch::Tile::Instance(_)) => {
                let (inputs, outputs) = hierarchy.instance_ports(ModuleId(id));
                // Each surface item is the inner parameter's row, keyed by
                // the inner module and parameter, so every row message
                // works on it unchanged; the Expose button names the tile
                // and the item, for exposing one level further out.
                let params = hierarchy
                    .definition_of(ModuleId(id))
                    .map_or_else(Vec::new, |definition| {
                        definition
                            .exposed
                            .iter()
                            .enumerate()
                            .filter_map(|(index, exposed)| {
                                let target = crate::surface::control_target(
                                    graph,
                                    hierarchy,
                                    id,
                                    index,
                                    loom_canvas::ControlPart::Main,
                                )?;
                                let exposure = self.exposure_row(id, index, &exposed.name);
                                let mut row =
                                    self.param_row(target.module.0, target.index, &target.spec, exposure, false)?;
                                // On this tile the row is the exposed item,
                                // not the inner module's parameter.
                                row.ui = ids::row(id, ids::Kind::Exposed, &exposed.name);
                                let member = loom_patch::Hierarchy::is_member_entry(exposed);
                                let label = if member {
                                    member_label(hierarchy, ModuleId(id), row_value(graph, target.module, target.index))
                                } else {
                                    exposed.name.clone()
                                };
                                Some(ExposedRow {
                                    member,
                                    module: target.module.0,
                                    label,
                                    own: ExposureRow {
                                        module: id,
                                        index,
                                        name: Some(exposed.name.clone()),
                                        surface: hierarchy.tile_name(ModuleId(id)).unwrap_or_default(),
                                    },
                                    row,
                                })
                            })
                            .collect()
                    });
                return Inspector::SubPatch(SubPatchRows {
                    id,
                    name: self
                        .session
                        .gestures
                        .label_draft(id)
                        .map_or_else(|| hierarchy.tile_name(ModuleId(id)).unwrap_or_default(), str::to_string),
                    inputs: inputs.into_iter().map(|(_, name)| name).collect(),
                    outputs: outputs.into_iter().map(|(_, name)| name).collect(),
                    params,
                    mode: hierarchy.modes_of(ModuleId(id)).map(|m| m.mode).unwrap_or_default(),
                    members: hierarchy
                        .members_of(ModuleId(id))
                        .into_iter()
                        .map(|member| MemberRow {
                            id: member.0,
                            name: hierarchy.tile_name(member).unwrap_or_default(),
                        })
                        .collect(),
                    copies: hierarchy.modes_of(ModuleId(id)).map_or(1, |m| m.copy_count()),
                    stereo_spread: hierarchy.modes_of(ModuleId(id)).is_some_and(|m| m.stereo_spread),
                    spread: spread_rows(hierarchy, ModuleId(id)),
                });
            }
            Some(loom_patch::Tile::Slot(_)) => {
                return self.slot_rows(id);
            }
            Some(loom_patch::Tile::Port(direction, name)) => {
                return Inspector::Port(PortRows {
                    id,
                    direction: match direction {
                        loom_patch::Direction::Input => "Patch input",
                        loom_patch::Direction::Output => "Patch output",
                    }
                    .to_string(),
                    name,
                });
            }
            _ => {}
        }
        let Some(node) = graph.node(ModuleId(id)) else {
            return Inspector::Empty;
        };
        if let Some(unknown) = &node.unknown {
            let mut links = Vec::new();
            for (assignment_id, assignment) in graph.assignments() {
                if assignment.source.module.0 == id || assignment.target.0 == id {
                    links.push(UnknownLink::Assignment {
                        id: assignment_id.0,
                        text: format!(
                            "{} → {}",
                            modulation::source_name(graph, assignment.source),
                            modulation::target_name(graph, assignment.target, assignment.param)
                        ),
                    });
                }
            }
            for (_, mapping) in graph.midi_mappings() {
                if mapping.target.0 == id {
                    links.push(UnknownLink::Midi {
                        index: mapping.param,
                        text: format!(
                            "{} → {}",
                            mapping.control,
                            modulation::target_name(graph, mapping.target, mapping.param)
                        ),
                    });
                }
            }
            return Inspector::Unknown(UnknownRows {
                id,
                kind: unknown.kind.clone(),
                label: self
                    .session
                    .gestures
                    .label_draft(id)
                    .map_or_else(|| node.label.clone().unwrap_or_default(), str::to_string),
                params: unknown
                    .params
                    .iter()
                    .map(|(name, value)| (name.clone(), *value))
                    .collect(),
                links,
            });
        }
        let d = node.descriptor;
        let params = d
            .params
            .iter()
            .enumerate()
            // A transient parameter has no row: it is played on its panel
            // widget only (transient-params.md).
            .filter(|(_, spec)| !spec.is_transient())
            .filter_map(|(index, _)| {
                // The control spans the selected range (control-ranges.md).
                let spec = node.spec(index)?;
                // Inside a sub-patch the row can expose its parameter. An
                // exposed parameter's control range is the surface's, so
                // its Range selector hides (flattening rule 12).
                let exposure = self.exposure_row(id, index, spec.name);
                let exposed = exposure.as_ref().is_some_and(|e| e.name.is_some());
                self.param_row(id, index, &spec, exposure, !exposed)
            })
            .collect();
        Inspector::Module(ModuleRows {
            id,
            name: d.name,
            kind: format!("{} #{id}", d.kind()),
            description: d.description,
            resets_clip: d
                .panel
                .widgets
                .iter()
                .any(|w| w.kind == WidgetKind::Meter && d.meter_display(w).is_none()),
            ports: d
                .inputs
                .iter()
                .map(|p| ("Input", p))
                .chain(d.outputs.iter().map(|p| ("Output", p)))
                .map(|(direction, p)| {
                    let (min, max) = p.nominal_range();
                    PortRow {
                        direction,
                        label: p.label,
                        description: format!("{} Usually {min} to {max}.", p.description),
                        details: format!("{}, {min} to {max}", p.signal.name()),
                    }
                })
                .collect(),
            params,
            drives: self.drives(id),
        })
    }

    /// The assignments on a parameter, in engine order
    /// (`docs/design/modulation.md`, Seeing assignments).
    fn assignment_rows(&self, module: u32, index: usize) -> Vec<AssignmentRow> {
        let graph = self.session.controller.graph();
        modulation::on_param(graph, ModuleId(module), index)
            .into_iter()
            .map(|(id, a)| {
                let own = modulation::source_polarity(graph, a.source);
                AssignmentRow {
                    id,
                    source: modulation::source_name(graph, a.source),
                    depth: a.depth,
                    depth_text: modulation::depth_text(a.depth),
                    polarity: PolarityChoice::of(a.polarity, own),
                    polarities: PolarityChoice::options(own),
                    enabled: a.enabled,
                }
            })
            .collect()
    }

    /// Every output at this patch level, for **Add modulation**. One that
    /// already drives the parameter cannot be picked.
    fn sources(&self, module: u32, index: usize) -> Vec<SourceRow> {
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        // Only this level's modules: nothing crosses a level (rule 4).
        let level = hierarchy.level_of(ModuleId(module));
        modulation::sources(graph)
            .into_iter()
            .filter(|source| hierarchy.level_of(source.module) == level)
            .map(|source| SourceRow {
                source,
                name: modulation::source_name(graph, source),
                enabled: !modulation::exists(graph, source, ModuleId(module), index),
            })
            .collect()
    }

    /// **Drives**: what each output of `module` drives. Lines for
    /// assignments that are off say "off", so the state does not rest on
    /// colour alone.
    fn drives(&self, module: u32) -> Vec<DrivesGroup> {
        let graph = self.session.controller.graph();
        let Some(node) = graph.node(ModuleId(module)) else {
            return Vec::new();
        };
        modulation::drives(graph, ModuleId(module))
            .into_iter()
            .map(|(port, assignments)| DrivesGroup {
                output: node.descriptor.outputs.get(port).map_or("?", |p| p.label),
                lines: assignments
                    .into_iter()
                    .map(|(_, a)| {
                        let mut line = format!(
                            "{}, {}",
                            modulation::target_name(graph, a.target, a.param),
                            modulation::depth_text(a.depth)
                        );
                        if !a.enabled {
                            line.push_str(" (off)");
                        }
                        DrivesLine {
                            target: a.target.0,
                            text: line,
                            enabled: a.enabled,
                        }
                    })
                    .collect(),
            })
            .collect()
    }
}

/// The inspector's widgets.
pub fn view<'a>(
    model: Inspector,
    authors: &'a iced::widget::text_editor::Content,
    drop_row: Option<(u32, usize)>,
    dragging: bool,
) -> Element<'a, AppMessage> {
    let content: Element<'a, AppMessage> = match model {
        // Short, with the full list one click away (visual-system.md).
        Inspector::Patch(rows) => column![
            crate::patch_info::view(rows, authors),
            iced::widget::rule::horizontal(1),
            text("Select a module to edit it.").size(style::BODY).color(DIM),
            text("Press A over the canvas to add one.").size(style::BODY).color(DIM),
            button(text("Keyboard shortcuts").size(style::CONTROL)).on_press(AppMessage::ShowShortcuts),
        ]
        .spacing(8)
        .into(),
        Inspector::Several(n) => text(format!("{n} modules selected")).size(style::CONTROL).into(),
        Inspector::Empty => text("").into(),
        Inspector::Unknown(m) => {
            let id = m.id;
            let mut rows = column![
                text(m.kind).size(style::HEADING),
                text("This version of LoomDSP does not have this module. It makes no sound. Its settings are kept and saved.").size(style::BODY),
                text("Label").size(style::BODY),
                row![
                    text_input("Label", &m.label)
                        .on_input(move |value| AppMessage::Inspector(Message::UnknownLabelDraft { module: id, value }))
                        .on_submit(AppMessage::Inspector(Message::UnknownLabelApply { module: id })),
                    button("Apply").on_press(AppMessage::Inspector(Message::UnknownLabelApply { module: id })),
                ].spacing(6),
            ].spacing(6);
            for (name, value) in m.params {
                rows = rows.push(text(format!("{name}: {value}")).size(style::BODY));
            }
            for link in m.links {
                let (label, message) = match link {
                    UnknownLink::Assignment { id, text } => (
                        text,
                        AppMessage::Modulation(crate::modulation::Message::Unassign(AssignmentId(id))),
                    ),
                    UnknownLink::Midi { index, text } => (
                        text,
                        AppMessage::Learn(crate::learn::Message::Unmap { module: m.id, index }),
                    ),
                };
                rows = rows.push(
                    row![
                        text(label).size(style::SMALL),
                        space::horizontal(),
                        remove_button("Remove link", message)
                    ]
                    .align_y(iced::Alignment::Center),
                );
            }
            rows.into()
        }
        Inspector::Module(m) => module_view(m, drop_row, dragging),
        Inspector::SubPatch(s) => {
            let id = s.id;
            // The name is edited in place, as a placeholder's label is, and
            // applied with Enter (sub-patches.md, Editor model).
            let mut rows = column![
                text_input("Sub-patch name", &s.name)
                    .on_input(move |value| AppMessage::Inspector(Message::SubPatchNameDraft { module: id, value }))
                    .on_submit(AppMessage::Inspector(Message::SubPatchNameApply { module: id })),
                text("Sub-patch").size(style::BODY).color(DIM),
            ]
            .spacing(6);
            for (label, ports) in [("Inputs", &s.inputs), ("Outputs", &s.outputs)] {
                if !ports.is_empty() {
                    rows = rows.push(text(format!("{label}: {}", ports.join(", "))).size(style::BODY));
                }
            }
            rows = rows.push(button(text("Open").size(style::CONTROL)).on_press(AppMessage::OpenTile(id)));
            rows = mode_rows(rows, &s);
            if s.params.is_empty() {
                rows = rows.push(
                    text("Nothing exposed. Open it and press Expose on a parameter.")
                        .size(style::BODY)
                        .color(DIM),
                );
            }
            for exposed in s.params {
                // The Member control is the select's own: no Unexpose.
                let own = (!exposed.member).then_some(exposed.own);
                rows = rows.push(param_row_view(
                    exposed.module,
                    exposed.label,
                    exposed.row,
                    own,
                    drop_row,
                    dragging,
                ));
            }
            rows.into()
        }
        Inspector::Slot(s) => {
            let id = s.id;
            let mut rows = column![
                text(s.name.clone()).size(style::HEADING),
                text(format!("Slot for {}", s.role)).size(style::BODY).color(DIM),
            ]
            .spacing(6);
            let current = s.choices.iter().find(|c| c.current).cloned();
            let choices: Vec<crate::slots::SwapChoice> = s.choices.clone();
            rows = rows.push(
                row![
                    text("Part").size(style::BODY),
                    pick_list(choices, current, move |choice: crate::slots::SwapChoice| {
                        AppMessage::SwapSlot {
                            slot: id,
                            to: choice.to,
                        }
                    })
                    .placeholder(crate::slots::NO_PART)
                    .text_size(style::CONTROL),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
            for (label, module, p) in s.role_params {
                rows = rows.push(param_row_view(module, label, p, None, drop_row, dragging));
            }
            if !s.other_params.is_empty() {
                rows = rows.push(
                    text(format!("{} only", s.part.clone().unwrap_or_default()))
                        .size(style::BODY)
                        .color(DIM),
                );
                for (label, module, p) in s.other_params {
                    rows = rows.push(param_row_view(module, label, p, None, drop_row, dragging));
                }
            }
            rows = rows.push(button(text("Dissolve slot").size(style::CONTROL)).on_press(AppMessage::DissolveSlot(id)));
            rows.into()
        }
        Inspector::Port(p) => column![
            text(p.name).size(style::HEADING),
            text(p.direction).size(style::BODY).color(DIM),
            text("A port of this sub-patch. Its name is the port's label on the tile outside.").size(style::BODY),
        ]
        .spacing(6)
        .into(),
    };
    // The scrollbar takes its own space, so it never covers a value.
    container(scrollable(container(content).padding(12)).spacing(4))
        .width(280)
        .height(Length::Fill)
        .into()
}

fn module_view<'a>(m: ModuleRows, drop_row: Option<(u32, usize)>, dragging: bool) -> Element<'a, AppMessage> {
    let id = m.id;
    let panel = ids::panel(id);
    let mut items = column![
        row![
            text(m.name).size(style::HEADING),
            space::horizontal(),
            ids::named(
                &ids::part(&panel, "help"),
                button(text("Help").size(style::CONTROL)).on_press(AppMessage::ShowModuleHelp(id))
            )
        ]
        .align_y(iced::Alignment::Center),
        text(m.description).size(style::BODY),
    ]
    .spacing(6);
    if m.resets_clip {
        items = items.push(ids::named(
            &ids::part(&panel, "reset_clip"),
            button(text("Reset clip").size(style::CONTROL)).on_press(AppMessage::ResetClip(id)),
        ));
    }
    if m.params.is_empty() {
        items = items.push(text("No parameters.").size(style::BODY).color(DIM));
    }
    for p in m.params {
        let label = p.label.to_string();
        items = items.push(param_row_view(id, label, p, None, drop_row, dragging));
    }
    if !m.drives.is_empty() {
        items = items.push(drives_view(m.drives));
    }
    // One line per port, its description in hover text.
    let mut direction = "";
    for port in m.ports {
        if port.direction != direction {
            direction = port.direction;
            let heading = if direction == "Input" { "Inputs" } else { "Outputs" };
            items = items.push(text(heading).size(style::SMALL).color(DIM));
        }
        let line = row![
            text(port.label).size(style::SMALL),
            space::horizontal(),
            text(port.details).size(style::SMALL).color(DIM),
        ];
        items = items.push(tooltip(
            line,
            container(text(port.description).size(style::BODY))
                .padding(6)
                .max_width(260)
                .style(container::rounded_box),
            tooltip::Position::Top,
        ));
    }
    items.into()
}

/// One parameter's row, as the module inspector and a sub-patch tile's
/// inspector draw it: its label line (with the Expose button inside a
/// sub-patch), its value control, mapping lines, and assignments. `label`
/// is the parameter's label, or the surface name on a sub-patch tile.
fn param_row_view<'a>(
    id: u32,
    label: String,
    p: ParamRow,
    own: Option<ExposureRow>,
    drop_row: Option<(u32, usize)>,
    dragging: bool,
) -> Element<'a, AppMessage> {
    let index = p.index;
    let ui = p.ui.clone();
    let accepts_drop = p.accepts_drop;
    let unmap = AppMessage::Learn(crate::learn::Message::Unmap { module: id, index });
    let ghost = p.mapping_options.as_ref().and_then(|row| row.ghost);
    let line: Element<'a, AppMessage> = match p.mode {
        ParamMode::Learn { armed, detail } => {
            let arm = button(row![
                text(label.clone()).size(style::BODY),
                space::horizontal(),
                text(detail).size(style::BODY)
            ])
            .width(Length::Fill)
            .style(if armed { style::primary } else { style::secondary })
            .on_press(AppMessage::Learn(crate::learn::Message::Arm((id, index))));
            // A mapping can be removed or changed in learn mode too.
            let mut line = row![ids::named(&ids::part(&ui, "learn"), arm)]
                .spacing(4)
                .align_y(iced::Alignment::Center);
            if p.mapping.is_some() {
                line = line.push(options_button(id, index, &ui));
                line = line.push(ids::named(
                    &ids::part(&ui, "midi/remove"),
                    remove_button("Remove MIDI mapping", unmap),
                ));
            }
            let line: Element<'a, AppMessage> = match p.mapping_options.filter(|row| row.open) {
                Some(mapping) => column![line, mapping_rows(id, index, mapping, &ui)].spacing(2).into(),
                None => line.into(),
            };
            ids::named(&ui, line)
        }
        ParamMode::Edit {
            position,
            step,
            control,
            assignments,
            adding,
        } => {
            let mut tip = column![text(p.description).size(style::BODY)].spacing(4);
            if let Some(name) = p.name {
                tip = tip.push(text(name).size(style::SMALL).color(DIM));
            }
            // The label, then the Expose button inside a sub-patch, then
            // the value field at the right.
            let mut head = row![tooltip(
                text(label.clone()).size(style::BODY),
                container(tip).padding(6).max_width(260).style(container::rounded_box),
                tooltip::Position::Top,
            )]
            .spacing(6)
            .align_y(iced::Alignment::Center);
            for exposure in own.into_iter().chain(p.exposure) {
                head = head.push(expose_button(exposure, &ui));
            }
            head = head
                .push(space::horizontal())
                .push(text(p.unused.unwrap_or_default()).size(style::SMALL).color(DIM))
                .push(value_field(id, index, p.field, p.value, &ui));
            let mut rows = column![
                head,
                match p.range {
                    Some(range) => row![
                        with_ghost(value_control(id, index, control, position, step, &ui), ghost),
                        range_view(id, index, range, &ui),
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
                    .into(),
                    None => with_ghost(value_control(id, index, control, position, step, &ui), ghost),
                },
            ]
            .spacing(2);
            if let Some(control) = p.mapping {
                rows = rows.push(
                    row![
                        text(control).size(style::SMALL).color(DIM),
                        space::horizontal(),
                        options_button(id, index, &ui),
                        ids::named(
                            &ids::part(&ui, "midi/remove"),
                            remove_button("Remove MIDI mapping", unmap)
                        ),
                    ]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
                );
            }
            if let Some(mapping) = p.mapping_options.filter(|row| row.open) {
                rows = rows.push(mapping_rows(id, index, mapping, &ui));
            }
            // The row's own container is the one the window names.
            let editor: Element<'a, AppMessage> = container(rows)
                .id(crate::places::widget_id(&ui))
                .width(Length::Fill)
                .style(move |theme: &Theme| {
                    let mut style = container::Style::default();
                    if dragging && drop_row == Some((id, index)) {
                        style.background = Some(theme.extended_palette().primary.weak.color.into());
                        style.border.radius = 4.0.into();
                    }
                    style
                })
                .into();
            let editor: Element<'a, AppMessage> = if accepts_drop {
                mouse_area(editor)
                    .on_enter(AppMessage::Inspector(Message::DropRow(Some((id, index)))))
                    .on_exit(AppMessage::Inspector(Message::DropRow(None)))
                    .into()
            } else {
                editor
            };
            column![editor, modulation_view(id, index, assignments, adding, &ui)]
                .spacing(2)
                .into()
        }
    };
    line
}

/// The button on a parameter's row that puts it on the enclosing sub-patch's
/// surface, or takes it off again, saying what it is exposed as
/// (sub-patches.md, Exposing).
fn expose_button<'a>(exposure: ExposureRow, row: &UiId) -> Element<'a, AppMessage> {
    let ExposureRow {
        module,
        index,
        name,
        surface,
    } = exposure;
    // A row can have two: one for its own tile's surface and one for the
    // enclosing sub-patch. The surface's name tells them apart.
    let ui = ids::part(row, "expose").with(surface.as_str());
    let shown: Element<'a, AppMessage> = match name {
        Some(name) => row![
            text(format!("as {name}")).size(style::SMALL).color(DIM),
            button(text(format!("Unexpose on {surface}")).size(style::SMALL))
                .style(style::plain)
                .padding(1)
                .on_press(AppMessage::Inspector(Message::Unexpose { module, index })),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center)
        .into(),
        None => button(text(format!("Expose on {surface}")).size(style::SMALL))
            .style(style::plain)
            .padding(1)
            .on_press(AppMessage::Inspector(Message::Expose { module, index }))
            .into(),
    };
    ids::named(&ui, shown)
}

/// The small button beside a mapping's ×, which opens its options rows.
fn options_button<'a>(module: u32, index: usize, row: &UiId) -> Element<'a, AppMessage> {
    ids::named(
        &ids::part(row, "midi/options"),
        button(text("Options").size(style::SMALL))
            .style(style::plain)
            .padding(1)
            .on_press(AppMessage::Learn(crate::learn::Message::MidiOptionsRows {
                module,
                index,
            })),
    )
}

/// A mapping's options, under its line: mode, then range and invert, or the
/// encoding for Relative (midi-learn.md, Mapping modes).
fn mapping_rows<'a>(module: u32, index: usize, row_data: MappingRow, ui: &UiId) -> Element<'a, AppMessage> {
    let options = row_data.options;
    let modes: Vec<Labelled<MidiMode>> = [
        MidiMode::Absolute,
        MidiMode::Pickup,
        MidiMode::Relative(encoding_of(options.mode).unwrap_or_default()),
    ]
    .into_iter()
    .map(|mode| Labelled {
        value: mode,
        label: mode_name(mode),
    })
    .collect();
    let current = modes.iter().copied().find(|choice| choice.value == options.mode);
    let label = |name| text(name).size(style::SMALL).color(DIM);
    let mut rows = column![
        row![
            label("Mode"),
            space::horizontal(),
            ids::named(
                &ids::part(ui, "midi/mode"),
                pick_list(modes, current, move |choice: Labelled<MidiMode>| AppMessage::Learn(
                    crate::learn::Message::MidiOptions {
                        module,
                        index,
                        options: options.with_mode(choice.value),
                    }
                ))
                .text_size(style::SMALL)
                .width(120)
            ),
        ]
        .align_y(iced::Alignment::Center)
    ]
    .spacing(2);
    if let Some(encoding) = encoding_of(options.mode) {
        let encodings: Vec<Labelled<MidiEncoding>> = [
            MidiEncoding::Offset64,
            MidiEncoding::TwosComplement,
            MidiEncoding::SignBit,
        ]
        .into_iter()
        .map(|value| Labelled {
            value,
            label: encoding_name(value),
        })
        .collect();
        let current = encodings.iter().copied().find(|choice| choice.value == encoding);
        rows = rows.push(
            row![
                label("Encoding"),
                space::horizontal(),
                ids::named(
                    &ids::part(ui, "midi/encoding"),
                    pick_list(encodings, current, move |choice: Labelled<MidiEncoding>| {
                        AppMessage::Learn(crate::learn::Message::MidiOptions {
                            module,
                            index,
                            options: options.with_mode(MidiMode::Relative(choice.value)),
                        })
                    })
                    .text_size(style::SMALL)
                    .width(120)
                ),
            ]
            .align_y(iced::Alignment::Center),
        );
    } else {
        let (low, high) = (row_data.low, row_data.high);
        let bad = row_data.range_error.is_some();
        let field = |shown: &str| {
            text_input("", shown)
                .on_submit(AppMessage::Learn(crate::learn::Message::MidiRangeSubmit {
                    module,
                    index,
                }))
                .size(style::SMALL)
                .width(72)
                .padding(1)
                .style(move |theme: &Theme, status| {
                    let mut style = text_input::default(theme, status);
                    if bad {
                        style.border.color = theme.extended_palette().danger.strong.color;
                        style.border.width = 2.0;
                    }
                    style
                })
        };
        let high_text = high.clone();
        let low_field = field(&low).on_input(move |text| {
            AppMessage::Learn(crate::learn::Message::MidiRangeText {
                module,
                index,
                low: text,
                high: high_text.clone(),
            })
        });
        let low_text = low.clone();
        let high_field = field(&high).on_input(move |text| {
            AppMessage::Learn(crate::learn::Message::MidiRangeText {
                module,
                index,
                low: low_text.clone(),
                high: text,
            })
        });
        rows = rows.push(
            row![
                label("Range"),
                space::horizontal(),
                ids::named(&ids::part(ui, "midi/low"), low_field),
                label("to"),
                ids::named(&ids::part(ui, "midi/high"), high_field)
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        );
        if let Some(reason) = row_data.range_error {
            rows = rows.push(label_owned(reason));
        }
        rows = rows.push(ids::named(
            &ids::part(ui, "midi/invert"),
            checkbox(options.invert)
                .label("Invert")
                .size(style::SWITCH)
                .text_size(style::SMALL)
                .on_toggle(move |invert| {
                    AppMessage::Learn(crate::learn::Message::MidiOptions {
                        module,
                        index,
                        options: MidiMappingOptions { invert, ..options },
                    })
                }),
        ));
    }
    if let Some(note) = row_data.range_note {
        rows = rows.push(label_owned(note));
    }
    rows.into()
}

/// Dim small text that the view owns.
fn label_owned<'a>(words: String) -> Element<'a, AppMessage> {
    text(words).size(style::SMALL).color(DIM).into()
}

/// A value and the name a pick list shows for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Labelled<T> {
    value: T,
    label: &'static str,
}

impl<T> std::fmt::Display for Labelled<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

/// A mode's name in the inspector and the snapshot.
fn mode_name(mode: MidiMode) -> &'static str {
    match mode {
        MidiMode::Absolute => "Absolute",
        MidiMode::Pickup => "Pickup",
        MidiMode::Relative(_) => "Relative",
    }
}

/// The encoding of a Relative mode.
fn encoding_of(mode: MidiMode) -> Option<MidiEncoding> {
    match mode {
        MidiMode::Relative(encoding) => Some(encoding),
        _ => None,
    }
}

/// An encoding's name in the inspector and the snapshot.
fn encoding_name(encoding: MidiEncoding) -> &'static str {
    match encoding {
        MidiEncoding::Offset64 => "Offset 64",
        MidiEncoding::TwosComplement => "Two's complement",
        MidiEncoding::SignBit => "Sign bit",
    }
}

/// A slider with a waiting pickup's hollow ghost mark over its track, in
/// proportion across it. It takes no presses, so the slider still gets them.
fn with_ghost<'a>(control: Element<'a, AppMessage>, ghost: Option<f32>) -> Element<'a, AppMessage> {
    let Some(ghost) = ghost else {
        return control;
    };
    let before = (ghost.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let mark = container(space())
        .width(10)
        .height(10)
        .style(|theme: &Theme| container::Style {
            border: iced::Border {
                color: theme.palette().text,
                width: 1.5,
                radius: 5.0.into(),
            },
            ..container::Style::default()
        });
    let track = row![
        space().width(Length::FillPortion(before.max(1))),
        mark,
        space().width(Length::FillPortion((1000 - before).max(1))),
    ]
    .align_y(iced::Alignment::Center);
    stack![control, container(track).center_y(Length::Fill)].into()
}

/// A parameter's value: a field to type in, or plain text for a parameter
/// that names its values. Text that is not a value gets a warning outline and
/// the reason in hover text.
fn value_field<'a>(
    module: u32,
    index: usize,
    field: Option<ValueField>,
    value: String,
    row: &UiId,
) -> Element<'a, AppMessage> {
    let Some(field) = field else {
        return text(value).size(style::BODY).color(DIM).into();
    };
    let bad = field.error.is_some();
    let input = text_input(&value, &field.text)
        .id(crate::controls::value_field_id(module, index))
        .on_input(move |text| AppMessage::Inspector(Message::ValueText { module, index, text }))
        .on_submit(AppMessage::Inspector(Message::ValueSubmit { module, index }))
        .size(style::BODY)
        .width(VALUE_FIELD_WIDTH)
        .padding(1)
        .style(move |theme: &Theme, status| {
            let mut style = text_input::default(theme, status);
            if bad {
                style.border.color = theme.extended_palette().danger.strong.color;
                style.border.width = 2.0;
            }
            style
        });
    // The tooltip stays when the text is good, with nothing in it. Without it
    // the field would sit at another place in the widget tree once the text
    // turned bad, and lose its focus and caret on that character.
    let reason: Element<'a, AppMessage> = match field.error {
        Some(reason) => container(text(reason).size(style::BODY))
            .padding(6)
            .style(container::rounded_box)
            .into(),
        None => iced::widget::space().into(),
    };
    ids::named(&ids::part(row, "field"), tooltip(input, reason, tooltip::Position::Top))
}

/// The slider of a parameter, or its choice names as radio buttons or a
/// pick list. A pick is one undo step, as a click on a radio tile is.
fn value_control<'a>(
    module: u32,
    index: usize,
    control: ParamControl,
    position: f32,
    step: f32,
    row: &UiId,
) -> Element<'a, AppMessage> {
    let ui = ids::part(row, "value");
    // A choice is the parameter's minimum plus its index. The model gives
    // the index; the message turns it into a value.
    let pick = move |choice: usize| AppMessage::Inspector(Message::Choice { module, index, choice });
    let shown: Element<'a, AppMessage> = match control {
        ParamControl::Slider => slider(0.0..=1.0, position, move |position| {
            AppMessage::Inspector(Message::Param {
                module,
                index,
                position,
            })
        })
        .on_release(AppMessage::SliderReleased)
        .step(step)
        .into(),
        ParamControl::Radio { choices, selected } => {
            let mut list = column![].spacing(3);
            for (i, name) in choices.into_iter().enumerate() {
                // Each choice by its index, which is the parameter's value.
                list = list.push(ids::named(
                    &ui.clone().with("option").with(i as u32),
                    radio(name, i, Some(selected), pick)
                        .size(style::SWITCH)
                        .text_size(style::BODY),
                ));
            }
            list.into()
        }
        ParamControl::PickList { choices, selected } => {
            let options: Vec<Choice> = choices
                .iter()
                .enumerate()
                .map(|(i, &name)| Choice { index: i, name })
                .collect();
            let current = options.get(selected).copied();
            pick_list(options, current, move |c: Choice| pick(c.index))
                .text_size(style::BODY)
                .width(Length::Fill)
                .into()
        }
    };
    // The slider and the pick list fill the row; the container must too.
    container(shown)
        .id(crate::places::widget_id(&ui))
        .width(Length::Fill)
        .into()
}

/// The **Range** selector: a pick list of the range names. A pick is one
/// undo step.
fn range_view<'a>(module: u32, index: usize, range: RangeRow, row: &UiId) -> Element<'a, AppMessage> {
    let options: Vec<Named> = range
        .choices
        .into_iter()
        .enumerate()
        .map(|(i, name)| Named { index: i, name })
        .collect();
    let current = range.selected.and_then(|i| options.get(i).cloned());
    ids::named(
        &ids::part(row, "range"),
        tooltip(
            pick_list(options, current, move |n: Named| {
                AppMessage::Inspector(Message::Range {
                    module,
                    index,
                    range: n.index,
                })
            })
            .placeholder("Full")
            .text_size(style::SMALL)
            .width(96),
            container(text("Range: what a full turn of the control covers.").size(style::BODY))
                .padding(6)
                .style(container::rounded_box),
            tooltip::Position::Top,
        ),
    )
}

/// One entry of the **Range** list: its index and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Named {
    index: usize,
    name: String,
}

impl std::fmt::Display for Named {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// One entry of a parameter's pick list: its index and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Choice {
    index: usize,
    name: &'static str,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

/// Under a parameter's slider: one row per assignment, then **Add
/// modulation** and, while it is open, its list.
fn modulation_view<'a>(
    module: u32,
    index: usize,
    assignments: Vec<AssignmentRow>,
    adding: Option<Vec<SourceRow>>,
    row: &UiId,
) -> Element<'a, AppMessage> {
    let center = iced::Alignment::Center;
    let mut rows = column![].spacing(6);
    for a in assignments {
        let id = a.id;
        // Keyed by the assignment, never by its place in the list.
        let ui = ids::assignment(row, id.0);
        // The same card as the modulation matrix, without the target line:
        // the target is this parameter (modulation.md, slice 3).
        let card = column![
            row![
                ids::named(
                    &ids::part(&ui, "on"),
                    checkbox(a.enabled)
                        .on_toggle(
                            move |enabled| AppMessage::Modulation(crate::modulation::Message::Enabled { id, enabled })
                        )
                        .size(style::SWITCH)
                ),
                text(a.source).size(style::SMALL),
                space::horizontal(),
                ids::named(
                    &ids::part(&ui, "remove"),
                    remove_button(
                        "Remove modulation",
                        AppMessage::Modulation(crate::modulation::Message::Unassign(id))
                    )
                ),
            ]
            .spacing(4)
            .align_y(center),
            ids::named(
                &ids::part(&ui, "polarity"),
                pick_list(a.polarities, Some(a.polarity), move |choice| AppMessage::Modulation(
                    crate::modulation::Message::Polarity { id, choice }
                ))
                .text_size(style::SMALL)
                .padding([1, 4])
                .style(style::quiet_pick_list)
            ),
            row![
                container(
                    slider(-1.0..=1.0, a.depth, move |depth| AppMessage::Modulation(
                        crate::modulation::Message::Depth { id, depth }
                    ))
                    .on_release(AppMessage::SliderReleased)
                    .step(0.01_f32)
                )
                .id(crate::places::widget_id(&ids::part(&ui, "depth")))
                .width(Length::Fill),
                text(a.depth_text).size(style::SMALL).width(40),
            ]
            .spacing(6)
            .align_y(center),
        ]
        .spacing(2);
        // The card's own container is the one the window names.
        rows = rows.push(
            container(card)
                .id(crate::places::widget_id(&ui))
                .padding(6)
                .width(Length::Fill)
                .style(style::card(false)),
        );
    }
    rows = rows.push(ids::named(
        &ids::part(row, "add"),
        button(text(if adding.is_some() { "Cancel" } else { "+ Add modulation" }).size(style::SMALL))
            .padding([2, 8])
            .on_press(AppMessage::Modulation(crate::modulation::Message::AddModulation {
                module,
                index,
            })),
    ));
    if let Some(sources) = adding {
        let mut list = column![].spacing(1);
        for s in sources {
            let source = s.source;
            // An output is its module and its port.
            let ui = ids::part(row, "add/source")
                .with(source.module.0)
                .with(source.port as u32);
            list = list.push(
                container(
                    button(text(s.name).size(style::SMALL))
                        .width(Length::Fill)
                        .padding([2, 6])
                        .style(style::secondary)
                        .on_press_maybe(s.enabled.then_some(AppMessage::Modulation(
                            crate::modulation::Message::Assign { source, module, index },
                        ))),
                )
                .id(crate::places::widget_id(&ui))
                .width(Length::Fill),
            );
        }
        rows = rows.push(container(list).padding([0, 8]));
    }
    rows.into()
}

/// **Drives**: one group per output. A line selects its target.
fn drives_view<'a>(drives: Vec<DrivesGroup>) -> Element<'a, AppMessage> {
    let mut list = column![text("Drives").size(style::CONTROL)].spacing(4);
    for group in drives {
        list = list.push(text(group.output).size(style::SMALL).color(DIM));
        for line in group.lines {
            let label = text(line.text).size(style::SMALL);
            list = list.push(
                button(if line.enabled { label } else { label.color(DIM) })
                    .width(Length::Fill)
                    .padding([2, 8])
                    .style(style::plain)
                    .on_press(AppMessage::Inspector(Message::SelectModule(line.target))),
            );
        }
    }
    list.into()
}

/// The value of parameter `index` of `module`, or 0 when it is gone.
fn row_value(graph: &loom_core::Graph, module: ModuleId, index: usize) -> f32 {
    graph
        .node(module)
        .and_then(|n| n.params.get(index).copied())
        .unwrap_or(0.0)
}

/// The Member row's label: `Member: 2 (Square)`, naming the chosen member
/// (sub-patches.md, The editor for modes).
fn member_label(hierarchy: &loom_patch::Hierarchy, instance: ModuleId, value: f32) -> String {
    let members = hierarchy.members_of(instance);
    let n = (value.round().max(1.0) as usize).min(members.len().max(1));
    match members.get(n - 1).and_then(|m| hierarchy.tile_name(*m)) {
        Some(name) => format!("Member: {n} ({name})"),
        None => format!("Member: {n}"),
    }
}

/// A stack's spread rows: one per exposed item, with its amount or 0.
fn spread_rows(hierarchy: &loom_patch::Hierarchy, instance: ModuleId) -> Vec<SpreadRow> {
    let (Some(definition), Some(modes)) = (hierarchy.definition_of(instance), hierarchy.modes_of(instance)) else {
        return Vec::new();
    };
    if modes.mode != loom_patch::Mode::Stack {
        return Vec::new();
    }
    definition
        .exposed
        .iter()
        .filter(|e| !loom_patch::Hierarchy::is_member_entry(e))
        .map(|e| SpreadRow {
            name: e.name.clone(),
            amount: modes.spread.get(&e.name).copied().unwrap_or(0.0),
        })
        .collect()
}

/// A mode as the Mode list shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModeChoice(loom_patch::Mode);

impl std::fmt::Display for ModeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.label())
    }
}

/// The mode rows of a sub-patch tile's inspector (sub-patches.md, The
/// editor for modes): the Mode list; for a layer or select, its members,
/// each with Open, Up, Down, and Remove, and Add member; for a stack,
/// Copies, Stereo spread, and a spread slider per exposed item.
fn mode_rows<'a>(
    mut rows: iced::widget::Column<'a, AppMessage>,
    s: &SubPatchRows,
) -> iced::widget::Column<'a, AppMessage> {
    use crate::modes::Message as ModeMessage;
    let id = s.id;
    let choices: Vec<ModeChoice> = loom_patch::Mode::ALL.into_iter().map(ModeChoice).collect();
    rows = rows.push(
        row![
            text("Mode").size(style::BODY),
            pick_list(choices, Some(ModeChoice(s.mode)), move |choice: ModeChoice| {
                AppMessage::Mode(ModeMessage::SetMode {
                    tile: id,
                    mode: choice.0,
                })
            })
            .text_size(style::CONTROL),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    );
    if s.mode.has_members() {
        rows = rows.push(text("Members").size(style::BODY).color(DIM));
        let last = s.members.len().saturating_sub(1);
        for (n, member) in s.members.iter().enumerate() {
            let m = member.id;
            let mut line = row![text(format!("{}. {}", n + 1, member.name)).size(style::BODY)]
                .spacing(6)
                .align_y(iced::Alignment::Center);
            line = line.push(button(text("Open").size(style::CONTROL)).on_press(AppMessage::OpenTile(m)));
            let up = button(text("Up").size(style::CONTROL));
            line = line.push(if n > 0 {
                up.on_press(AppMessage::Mode(ModeMessage::MoveMember { member: m, up: true }))
            } else {
                up
            });
            let down = button(text("Down").size(style::CONTROL));
            line = line.push(if n < last {
                down.on_press(AppMessage::Mode(ModeMessage::MoveMember { member: m, up: false }))
            } else {
                down
            });
            line = line.push(
                button(text("Remove").size(style::CONTROL)).on_press(AppMessage::Mode(ModeMessage::RemoveMember(m))),
            );
            rows = rows.push(line);
        }
        rows = rows.push(
            button(text("Add member").size(style::CONTROL)).on_press(AppMessage::Mode(ModeMessage::AddMember(id))),
        );
    }
    if s.mode == loom_patch::Mode::Stack {
        let counts: Vec<u32> = (1..=loom_patch::MOST_MEMBERS as u32).collect();
        rows = rows.push(
            row![
                text("Copies").size(style::BODY),
                pick_list(counts, Some(s.copies), move |copies| {
                    AppMessage::Mode(ModeMessage::SetCopies { tile: id, copies })
                })
                .text_size(style::CONTROL),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
        rows = rows.push(
            checkbox(s.stereo_spread)
                .label("Stereo spread")
                .on_toggle(move |on| AppMessage::Mode(ModeMessage::SetStereoSpread { tile: id, on }))
                .text_size(style::BODY),
        );
        if !s.spread.is_empty() {
            rows = rows.push(text("Spread").size(style::BODY).color(DIM));
        }
        for spread in &s.spread {
            let name = spread.name.clone();
            rows = rows.push(
                row![
                    text(spread.name.clone()).size(style::BODY),
                    slider(0.0..=1.0, spread.amount, move |amount| {
                        AppMessage::Mode(ModeMessage::SetSpread {
                            tile: id,
                            name: name.clone(),
                            amount,
                        })
                    })
                    .step(0.01_f32)
                    .on_release(AppMessage::SliderReleased),
                    text(format!("{:.0}%", spread.amount * 100.0)).size(style::BODY),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
        }
    }
    rows
}
