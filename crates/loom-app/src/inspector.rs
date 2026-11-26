//! The inspector: what it shows for the selection, as plain data. The
//! runtime's panel draws it (`panel.rs`, `panel/rows.rs`; plan 3.2, row
//! 4d), and [`crate::App::ui_snapshot`] reads the same model, so a test of
//! the snapshot checks what the user sees.

use crate::modulation::{self, PolarityChoice};
use crate::{App, param};
use loom_core::{AssignmentId, MidiEncoding, MidiMappingOptions, MidiMode, ModuleId, OutputRef, ParamSpec, WidgetKind};
use loom_ui_core::UiId;
use serde::Serialize;

pub(crate) mod access;
pub(crate) mod ids;
pub(crate) mod panel;

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
    /// The runtime's slider for a parameter moved to `value`, taken from
    /// its position over the range the row shows (an exposed row's
    /// surface range, which can be narrower than the parameter's), and
    /// `end` when its gesture ended, which closes the undo step as a
    /// release does (ui-layer.md, section 5, rule 16).
    Slid {
        module: u32,
        index: usize,
        value: f32,
        end: bool,
    },
    /// The runtime's depth slider on a modulation's card moved, from -1 to
    /// 1, and `end` when its gesture ended.
    DepthSlid { id: AssignmentId, depth: f32, end: bool },
    /// The runtime's slider for a stack's spread moved, from 0 to 1, and
    /// `end` when its gesture ended (sub-patches.md, The editor for modes).
    SpreadSlid {
        tile: u32,
        name: String,
        amount: f32,
        end: bool,
    },
    /// **Full** was picked in a parameter's **Range** select: the
    /// parameter's whole range (control-ranges.md; Brylie, 2026-11-25).
    FullRange { module: u32, index: usize },
}

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
    /// The authors credited on this definition, in order.
    pub authors: Vec<String>,
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
    /// An empty slot's note, such as `No part. Passes its input through.
    /// Swap to choose one.`; `None` while it holds a part.
    pub note: Option<String>,
    /// The flat id of the part, whose rows the parameters are.
    pub part_id: Option<u32>,
    /// Every part the slot can take, the current one marked: the **Part**
    /// list. `No part` comes first, and is current for an empty slot
    /// (sub-patches.md, Empty slots).
    pub choices: Vec<crate::slots::SwapChoice>,
    /// The part's parameters that are the role's, as `(label, module, row)`:
    /// the module is the part for a module part, and the inner module an
    /// exposed item stands for on a sub-patch part, as the row's messages
    /// name it.
    pub role_params: Vec<(String, u32, ParamRow)>,
    /// The part's other parameters, under a heading with the part's name.
    pub other_params: Vec<(String, u32, ParamRow)>,
    /// The UI ID the part's rows are under: the slot's, then the part's
    /// key (sub-patches.md, Slots; ui-layer.md, section 4).
    #[serde(skip)]
    pub rows_id: Option<UiId>,
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

impl ExposureRow {
    /// The action label shared by the inspector and its accessibility tree.
    pub(crate) fn button_label(&self) -> String {
        let verb = if self.name.is_some() { "Unexpose" } else { "Expose" };
        format!("{verb} on {}", self.surface)
    }
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
    pub name: String,
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
    /// The parameter as this row's control maps it: the module's selected
    /// range, or the range an enclosing surface exposes.
    #[serde(skip)]
    pub spec: ParamSpec,
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
    /// The value is governed by the selected external clock.
    MidiClock,
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
    #[serde(serialize_with = "crate::snapshot::display")]
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
    /// The assignment the line shows, which keys its UI ID: a line keeps
    /// its node while the lines before it come and go.
    #[serde(skip)]
    pub id: AssignmentId,
    pub target: u32,
    pub text: String,
    /// False for an assignment that is off: its text ends in "(off)" and
    /// is dimmed.
    pub enabled: bool,
}

impl App {
    /// Whether MIDI clock owns this parameter, leaving no editor on its row.
    fn midi_clock_owns(&self, node: &loom_core::Node, spec: &ParamSpec) -> bool {
        node.descriptor.role.is_transport()
            && self.session.controller.follows_midi_clock()
            && matches!(spec.name, "tempo" | "run" | "reset")
    }

    /// The parent of a slot's parameter rows, keyed by its current part so
    /// a swap makes new elements (ui-layer.md, section 4).
    fn slot_param_parent(&self, slot: u32, part: ModuleId) -> Option<UiId> {
        let panel = ids::panel(slot);
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        match graph.node(part) {
            Some(node) => Some(panel.with(node.descriptor.id)),
            None => hierarchy
                .key_at(&hierarchy.path_of(part))
                .map(|key| panel.with("subpatch").with(key)),
        }
    }

    /// The selected inspector row that edits this parameter, without building
    /// the inspector model. On a slot, role rows come before other rows.
    pub(crate) fn inspected_param_id(&self, module: ModuleId, index: usize) -> Option<UiId> {
        let mut selected = self.session.selection.iter();
        let (Some(&tile), None) = (selected.next(), selected.next()) else {
            return None;
        };
        if self.right_tab != crate::RightTab::Inspector {
            return None;
        }
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        let node = graph.node(module)?;
        let spec = node.control_spec(index)?;
        if self.midi_clock_owns(node, &spec) {
            return None;
        }
        let own = || ids::row(module.0, ids::Kind::Param, spec.name);
        let slot = hierarchy.slot(ModuleId(tile));
        let part = if slot.is_some() {
            hierarchy.part_of_slot(ModuleId(tile))?
        } else {
            ModuleId(tile)
        };
        if part == module {
            if spec.is_transient() || (slot.is_none() && node.unknown.is_some()) {
                return None;
            }
            // A part's other rows are in its `others` group (the slot's
            // model, `slot_rows`).
            let other = slot.is_some_and(|slot| slot.role_param_of(spec.name).is_none());
            return Some(
                slot.and_then(|_| self.slot_param_parent(tile, part))
                    .map(|parent| if other { parent.with("others") } else { parent })
                    .map_or_else(own, |parent| parent.with(ids::Kind::Param.key()).with(spec.name)),
            );
        }
        let definition = hierarchy.definition_of(part)?;
        let (_, exposed) = definition
            .exposed
            .iter()
            .enumerate()
            .filter(|(surface_index, _)| {
                self.control_target(part.0, *surface_index, loom_canvas::ControlPart::Main)
                    .is_some_and(|target| target.module == module && target.index == index)
            })
            .min_by_key(|(_, exposed)| slot.is_some_and(|slot| slot.role.param(&exposed.name).is_none()))?;
        let kind = ids::Kind::Exposed;
        Some(match slot {
            Some(slot) => self
                .slot_param_parent(tile, part)
                .map(|parent| {
                    if slot.role.param(&exposed.name).is_none() {
                        parent.with("others")
                    } else {
                        parent
                    }
                })
                .map_or_else(own, |parent| parent.with(kind.key()).with(exposed.name.as_str())),
            None => ids::row(tile, kind, &exposed.name),
        })
    }

    /// Handles a message from the inspector. A submitted value also moves
    /// focus, as an effect.
    pub(crate) fn inspector_message(&mut self, message: Message) {
        match message {
            Message::ValueSubmit { module, index } => self.value_submit(module, index),
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
                    .and_then(|n| n.control_spec(index))
                    .map(|spec| spec.clamp(spec.min + choice as f32));
                if let Some(value) = value {
                    self.turn_param(module, index, value);
                    self.close_slider_gesture();
                }
            }
            Message::ValueText { module, index, text } => self.type_value_draft(module, index, text),
            Message::ValueDraftClear => self.clear_value_draft(),
            Message::Range { module, index, range } => self.set_range(module, index, range),
            Message::FullRange { module, index } => self.set_full_range(module, index),
            Message::Slid {
                module,
                index,
                value,
                end,
            } => {
                self.inspector_message(Message::ParamValue { module, index, value });
                if end {
                    self.close_slider_gesture();
                }
            }
            Message::SpreadSlid {
                tile,
                name,
                amount,
                end,
            } => {
                self.mode_message(crate::modes::Message::SetSpread { tile, name, amount });
                if end {
                    self.close_slider_gesture();
                }
            }
            Message::DepthSlid { id, depth, end } => {
                // On the percent grid, as the card says the depth and as
                // Iced's slider stepped it, by drag and by key alike.
                let depth = (depth * 100.0).round() / 100.0;
                self.modulation_message(crate::modulation::Message::Depth { id, depth });
                if end {
                    self.close_slider_gesture();
                }
            }
            Message::SelectModule(id) => {
                if self.session.controller.graph().node(ModuleId(id)).is_some() {
                    self.session.selection = std::collections::BTreeSet::from([id]);
                }
            }
        }
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
        names: &modulation::Names<'_>,
        id: u32,
        index: usize,
        spec: &ParamSpec,
        exposure: Option<ExposureRow>,
        show_range: bool,
    ) -> Option<ParamRow> {
        let graph = self.session.controller.graph();
        let node = graph.node(ModuleId(id))?;
        let d = node.descriptor;
        let filtered = if spec.options.count_choices.is_some() {
            node.control_spec(index)?
        } else {
            *spec
        };
        let spec = &filtered;
        let stored = *node.params.get(index)?;
        let clock = self.midi_clock_owns(node, spec);
        let value = match spec.name {
            "tempo" if clock => self
                .session
                .controller
                .rendered_tempo()
                .map_or(stored, |tempo| (tempo * 10.0).round() as f32 / 10.0),
            "run" if clock => f32::from(crate::transport_running(&self.session.controller).unwrap_or(false)),
            _ => spec.effective_choice(stored),
        };
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
        let mode = if clock {
            ParamMode::MidiClock
        } else if self.session.learn.is_on() && !spec.options.shapes_graph {
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
                assignments: self.assignment_rows(names, id, index),
                adding: (!spec.options.shapes_graph && self.session.gestures.adding() == Some((id, index)))
                    .then(|| self.sources(names, id, index)),
            }
        };
        let unused = (!d.is_shown(index, &node.params)).then(|| {
            let mode = spec.options.shown_when.and_then(|w| d.param_index(w.param));
            match mode.map(|m| (&d.params[m], d.effective_value(m, &node.params))) {
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
                !spec.options.shapes_graph
                    && (bound.clone().next().is_none() || bound.into_iter().any(|w| w.kind.takes_modulation()))
            },
            exposure,
            spec: *spec,
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
        let names = modulation::Names::new(graph, hierarchy);
        let part = hierarchy.part_of_slot(ModuleId(id));
        // Every row is under the slot's part, by its kind and name; the
        // part's other rows are in its `others` group, under their
        // heading, as the panel nests them (ui-layer.md, section 4).
        let under = part.map(|part| {
            self.slot_param_parent(id, part)
                .unwrap_or_else(|| ids::panel(id).with(part.0))
        });
        let keyed = |mut row: ParamRow, kind: ids::Kind, name: &str, other: bool| {
            if let Some(under) = &under {
                let group = if other {
                    under.clone().with("others")
                } else {
                    under.clone()
                };
                row.ui = group.with(kind.key()).with(name);
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
                    let Some(row) = self.param_row(&names, part.0, index, &spec, exposure, !exposed) else {
                        continue;
                    };
                    let role = state.role_param_of(spec.name).is_some();
                    let row = keyed(row, ids::Kind::Param, spec.name, !role);
                    if role {
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
                    let Some(row) =
                        self.param_row(&names, target.module.0, target.index, &target.spec, exposure, false)
                    else {
                        continue;
                    };
                    let role = state.role.param(&exposed.name).is_some();
                    let row = keyed(row, ids::Kind::Exposed, &exposed.name, !role);
                    if role {
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
            note: part
                .is_none()
                .then(|| loom_patch::hierarchy::no_part_text(state.role).to_string()),
            part: part_name,
            part_id: part.map(|p| p.0),
            rows_id: under,
            choices: std::iter::once(crate::slots::SwapChoice {
                label: crate::slots::NO_PART_CHOICE.to_string(),
                to: crate::slots::SwapTarget::Empty,
                current: part.is_none(),
            })
            .chain(self.swap_choices(id))
            .collect(),
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
        let names = modulation::Names::new(graph, hierarchy);
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
                                let member = loom_patch::Hierarchy::is_member_entry(exposed);
                                let mut surface = hierarchy.tile_name(ModuleId(id)).unwrap_or_default();
                                let mut exposure = self.exposure_row(id, index, &exposed.name);
                                if !member
                                    && let Some(outer) = &mut exposure
                                    && outer.surface == surface
                                {
                                    surface.push_str(" (this tile)");
                                    outer.surface.push_str(" (outer)");
                                }
                                let mut row = self.param_row(
                                    &names,
                                    target.module.0,
                                    target.index,
                                    &target.spec,
                                    exposure,
                                    false,
                                )?;
                                // On this tile the row is the exposed item,
                                // not the inner module's parameter.
                                row.ui = ids::row(id, ids::Kind::Exposed, &exposed.name);
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
                                        surface,
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
                    authors: hierarchy
                        .definition_of(ModuleId(id))
                        .map_or_else(Vec::new, |d| d.metadata.authors.clone()),
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
                            names.source(assignment.source),
                            names.target(assignment.target, assignment.param)
                        ),
                    });
                }
            }
            for (_, mapping) in graph.midi_mappings() {
                if mapping.target.0 == id {
                    links.push(UnknownLink::Midi {
                        index: mapping.param,
                        text: format!("{} → {}", mapping.control, names.target(mapping.target, mapping.param)),
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
                self.param_row(&names, id, index, &spec, exposure, !exposed)
            })
            .collect();
        Inspector::Module(ModuleRows {
            id,
            name: names.module(ModuleId(id)),
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
                .enumerate()
                .map(|(port, p)| (false, port, p))
                .chain(d.outputs.iter().enumerate().map(|(port, p)| (true, port, p)))
                .map(|(output, port, p)| {
                    let (min, max) = p.nominal_range();
                    let module = ModuleId(id);
                    let source = if output {
                        Some(loom_core::OutputRef { module, port })
                    } else {
                        graph.source_of(loom_core::InputRef { module, port })
                    };
                    let width = source.and_then(|source| {
                        crate::scene::width_label(self.session.controller.last_report().width(source))
                    });
                    let mut details = format!("{}, {min} to {max}", p.signal.name());
                    if let Some(width) = width {
                        details.push_str(" · ");
                        details.push_str(&width);
                    }
                    PortRow {
                        direction: if output { "Output" } else { "Input" },
                        label: p.label,
                        description: format!("{} Usually {min} to {max}.", p.description),
                        details,
                    }
                })
                .collect(),
            params,
            drives: self.drives(&names, id),
        })
    }

    /// The assignments on a parameter, in engine order
    /// (`docs/design/modulation.md`, Seeing assignments).
    fn assignment_rows(&self, names: &modulation::Names<'_>, module: u32, index: usize) -> Vec<AssignmentRow> {
        let graph = self.session.controller.graph();
        modulation::on_param(graph, ModuleId(module), index)
            .into_iter()
            .map(|(id, a)| {
                let own = modulation::source_polarity(graph, a.source);
                AssignmentRow {
                    id,
                    source: names.source(a.source),
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
    fn sources(&self, names: &modulation::Names<'_>, module: u32, index: usize) -> Vec<SourceRow> {
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        // Only this level's modules: nothing crosses a level (rule 4).
        let level = hierarchy.level_of(ModuleId(module));
        modulation::sources(graph)
            .into_iter()
            .filter(|source| hierarchy.level_of(source.module) == level)
            .map(|source| SourceRow {
                source,
                name: names.source(source),
                enabled: !modulation::exists(graph, source, ModuleId(module), index),
            })
            .collect()
    }

    /// **Drives**: what each output of `module` drives. Lines for
    /// assignments that are off say "off", so the state does not rest on
    /// colour alone.
    fn drives(&self, names: &modulation::Names<'_>, module: u32) -> Vec<DrivesGroup> {
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
                    .map(|(id, a)| {
                        let mut line = format!(
                            "{}, {}",
                            names.target(a.target, a.param),
                            modulation::depth_text(a.depth)
                        );
                        if !a.enabled {
                            line.push_str(" (off)");
                        }
                        DrivesLine {
                            id,
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

/// Filtered choice names keep their UI identity when the count switches lists.
fn choice_ui(root: &UiId, spec: &ParamSpec, index: usize, name: &str) -> UiId {
    let root = root.clone();
    // A radio group's radios are its direct children (the 4d.2 renames).
    if spec.options.count_choices.is_some() {
        root.with(name)
    } else {
        root.with(index as u32)
    }
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
