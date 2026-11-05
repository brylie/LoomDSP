//! The inspector: what it shows for the selection, as plain data, and the
//! widgets drawn from that data.
//!
//! The rows are built first as a small model, then turned into widgets. The
//! window and [`crate::App::ui_snapshot`] both read the same model, so a
//! test of the snapshot checks what the user sees.

use crate::modulation::{self, PolarityChoice};
use crate::style::{self, button};
use crate::view::remove_button;
use crate::{App, DIM, Message, param};
use iced::widget::{
    checkbox, column, container, mouse_area, pick_list, radio, row, scrollable, slider, space, stack, text, text_input,
    tooltip,
};
use iced::{Element, Length, Theme};
use loom_core::{AssignmentId, MidiEncoding, MidiMappingOptions, MidiMode, ModuleId, OutputRef, WidgetKind};
use serde::Serialize;

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
    /// The selected module is gone. Nothing is shown.
    Empty,
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
    /// The inspector's content for the current selection.
    pub(crate) fn inspector_model(&self) -> Inspector {
        let graph = self.session.controller.graph();
        let mut selected = self.session.selection.iter();
        let id = match (selected.next(), selected.next()) {
            (None, _) => return Inspector::Patch(self.patch_rows()),
            (Some(&id), None) => id,
            (Some(_), Some(_)) => return Inspector::Several(self.session.selection.len()),
        };
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
                    .map_or_else(|| unknown.label.clone().unwrap_or_default(), str::to_string),
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
            .zip(&node.params)
            .enumerate()
            // A transient parameter has no row: it is played on its panel
            // widget only (transient-params.md).
            .filter(|(_, (spec, _))| !spec.is_transient())
            .map(|(index, (_, &value))| {
                // The control spans the selected range (control-ranges.md).
                let spec = &node
                    .spec(index)
                    .expect("node.spec covers every parameter index of the node");
                let range = spec.options.ranges.first().map(|_| RangeRow {
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
                        ghost: self
                            .session
                            .learn
                            .ghosts()
                            .shown()
                            .get(&(id, index))
                            .copied()
                            .filter(|_| spec.options.choices.is_empty() && !self.session.learn.is_on()),
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
                        Some(draft) => ValueField {
                            text: draft.text.clone(),
                            error: draft.error.clone(),
                        },
                        None => ValueField {
                            text: shown.clone(),
                            error: None,
                        },
                    }
                });
                ParamRow {
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
                }
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
        modulation::sources(graph)
            .into_iter()
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
) -> Element<'a, Message> {
    let content: Element<'a, Message> = match model {
        // Short, with the full list one click away (visual-system.md).
        Inspector::Patch(rows) => column![
            crate::patch_info::view(rows, authors),
            iced::widget::rule::horizontal(1),
            text("Select a module to edit it.").size(style::BODY).color(DIM),
            text("Press A over the canvas to add one.").size(style::BODY).color(DIM),
            button(text("Keyboard shortcuts").size(style::CONTROL)).on_press(Message::ShowShortcuts),
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
                        .on_input(move |value| Message::UnknownLabelDraft { module: id, value })
                        .on_submit(Message::UnknownLabelApply { module: id }),
                    button("Apply").on_press(Message::UnknownLabelApply { module: id }),
                ].spacing(6),
            ].spacing(6);
            for (name, value) in m.params {
                rows = rows.push(text(format!("{name}: {value}")).size(style::BODY));
            }
            for link in m.links {
                let (label, message) = match link {
                    UnknownLink::Assignment { id, text } => (text, Message::Unassign(AssignmentId(id))),
                    UnknownLink::Midi { index, text } => (text, Message::Unmap { module: m.id, index }),
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
    };
    // The scrollbar takes its own space, so it never covers a value.
    container(scrollable(container(content).padding(12)).spacing(4))
        .width(280)
        .height(Length::Fill)
        .into()
}

fn module_view<'a>(m: ModuleRows, drop_row: Option<(u32, usize)>, dragging: bool) -> Element<'a, Message> {
    let id = m.id;
    let mut items = column![
        row![
            text(m.name).size(style::HEADING),
            space::horizontal(),
            button(text("Help").size(style::CONTROL)).on_press(Message::ShowModuleHelp(id))
        ]
        .align_y(iced::Alignment::Center),
        text(m.description).size(style::BODY),
    ]
    .spacing(6);
    if m.resets_clip {
        items = items.push(button(text("Reset clip").size(style::CONTROL)).on_press(Message::ResetClip(id)));
    }
    if m.params.is_empty() {
        items = items.push(text("No parameters.").size(style::BODY).color(DIM));
    }
    for p in m.params {
        let index = p.index;
        let accepts_drop = p.accepts_drop;
        let unmap = Message::Unmap { module: id, index };
        let ghost = p.mapping_options.as_ref().and_then(|row| row.ghost);
        let line: Element<'a, Message> = match p.mode {
            ParamMode::Learn { armed, detail } => {
                let arm = button(row![
                    text(p.label).size(style::BODY),
                    space::horizontal(),
                    text(detail).size(style::BODY)
                ])
                .width(Length::Fill)
                .style(if armed { style::primary } else { style::secondary })
                .on_press(Message::Learn(crate::learn::Message::Arm((id, index))));
                // A mapping can be removed or changed in learn mode too.
                let mut line = row![arm].spacing(4).align_y(iced::Alignment::Center);
                if p.mapping.is_some() {
                    line = line.push(options_button(id, index));
                    line = line.push(remove_button("Remove MIDI mapping", unmap));
                }
                match p.mapping_options.filter(|row| row.open) {
                    Some(mapping) => column![line, mapping_rows(id, index, mapping)].spacing(2).into(),
                    None => line.into(),
                }
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
                let mut rows = column![
                    row![
                        tooltip(
                            text(p.label).size(style::BODY),
                            container(tip).padding(6).max_width(260).style(container::rounded_box),
                            tooltip::Position::Top,
                        ),
                        space::horizontal(),
                        text(p.unused.unwrap_or_default()).size(style::SMALL).color(DIM),
                        value_field(id, index, p.field, p.value),
                    ]
                    .spacing(6),
                    match p.range {
                        Some(range) => row![
                            with_ghost(value_control(id, index, control, position, step), ghost),
                            range_view(id, index, range),
                        ]
                        .spacing(6)
                        .align_y(iced::Alignment::Center)
                        .into(),
                        None => with_ghost(value_control(id, index, control, position, step), ghost),
                    },
                ]
                .spacing(2);
                if let Some(control) = p.mapping {
                    rows = rows.push(
                        row![
                            text(control).size(style::SMALL).color(DIM),
                            space::horizontal(),
                            options_button(id, index),
                            remove_button("Remove MIDI mapping", unmap),
                        ]
                        .spacing(4)
                        .align_y(iced::Alignment::Center),
                    );
                }
                if let Some(mapping) = p.mapping_options.filter(|row| row.open) {
                    rows = rows.push(mapping_rows(id, index, mapping));
                }
                let editor: Element<'a, Message> = container(rows)
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
                let editor: Element<'a, Message> = if accepts_drop {
                    mouse_area(editor)
                        .on_enter(Message::DropRow(Some((id, index))))
                        .on_exit(Message::DropRow(None))
                        .into()
                } else {
                    editor
                };
                column![editor, modulation_view(id, index, assignments, adding)]
                    .spacing(2)
                    .into()
            }
        };
        items = items.push(line);
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

/// The small button beside a mapping's ×, which opens its options rows.
fn options_button<'a>(module: u32, index: usize) -> Element<'a, Message> {
    button(text("Options").size(style::SMALL))
        .style(style::plain)
        .padding(1)
        .on_press(Message::MidiOptionsRows { module, index })
        .into()
}

/// A mapping's options, under its line: mode, then range and invert, or the
/// encoding for Relative (midi-learn.md, Mapping modes).
fn mapping_rows<'a>(module: u32, index: usize, row_data: MappingRow) -> Element<'a, Message> {
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
            pick_list(modes, current, move |choice: Labelled<MidiMode>| Message::MidiOptions {
                module,
                index,
                options: options.with_mode(choice.value),
            })
            .text_size(style::SMALL)
            .width(120),
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
                pick_list(encodings, current, move |choice: Labelled<MidiEncoding>| {
                    Message::MidiOptions {
                        module,
                        index,
                        options: options.with_mode(MidiMode::Relative(choice.value)),
                    }
                })
                .text_size(style::SMALL)
                .width(120),
            ]
            .align_y(iced::Alignment::Center),
        );
    } else {
        let (low, high) = (row_data.low, row_data.high);
        let bad = row_data.range_error.is_some();
        let field = |shown: &str| {
            text_input("", shown)
                .on_submit(Message::MidiRangeSubmit { module, index })
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
        let low_field = field(&low).on_input(move |text| Message::MidiRangeText {
            module,
            index,
            low: text,
            high: high_text.clone(),
        });
        let low_text = low.clone();
        let high_field = field(&high).on_input(move |text| Message::MidiRangeText {
            module,
            index,
            low: low_text.clone(),
            high: text,
        });
        rows = rows.push(
            row![label("Range"), space::horizontal(), low_field, label("to"), high_field]
                .spacing(4)
                .align_y(iced::Alignment::Center),
        );
        if let Some(reason) = row_data.range_error {
            rows = rows.push(label_owned(reason));
        }
        rows = rows.push(
            checkbox(options.invert)
                .label("Invert")
                .size(style::SWITCH)
                .text_size(style::SMALL)
                .on_toggle(move |invert| Message::MidiOptions {
                    module,
                    index,
                    options: MidiMappingOptions { invert, ..options },
                }),
        );
    }
    rows.into()
}

/// Dim small text that the view owns.
fn label_owned<'a>(words: String) -> Element<'a, Message> {
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
fn with_ghost<'a>(control: Element<'a, Message>, ghost: Option<f32>) -> Element<'a, Message> {
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
fn value_field<'a>(module: u32, index: usize, field: Option<ValueField>, value: String) -> Element<'a, Message> {
    let Some(field) = field else {
        return text(value).size(style::BODY).color(DIM).into();
    };
    let bad = field.error.is_some();
    let input = text_input(&value, &field.text)
        .id(crate::controls::value_field_id(module, index))
        .on_input(move |text| Message::ValueText { module, index, text })
        .on_submit(Message::ValueSubmit { module, index })
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
    let reason: Element<'a, Message> = match field.error {
        Some(reason) => container(text(reason).size(style::BODY))
            .padding(6)
            .style(container::rounded_box)
            .into(),
        None => iced::widget::space().into(),
    };
    tooltip(input, reason, tooltip::Position::Top).into()
}

/// The slider of a parameter, or its choice names as radio buttons or a
/// pick list. A pick is one undo step, as a click on a radio tile is.
fn value_control<'a>(
    module: u32,
    index: usize,
    control: ParamControl,
    position: f32,
    step: f32,
) -> Element<'a, Message> {
    // A choice is the parameter's minimum plus its index. The model gives
    // the index; the message turns it into a value.
    let pick = move |choice: usize| Message::Choice { module, index, choice };
    match control {
        ParamControl::Slider => slider(0.0..=1.0, position, move |position| Message::Param {
            module,
            index,
            position,
        })
        .on_release(Message::SliderReleased)
        .step(step)
        .into(),
        ParamControl::Radio { choices, selected } => {
            let mut list = column![].spacing(3);
            for (i, name) in choices.into_iter().enumerate() {
                list = list.push(
                    radio(name, i, Some(selected), pick)
                        .size(style::SWITCH)
                        .text_size(style::BODY),
                );
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
    }
}

/// The **Range** selector: a pick list of the range names. A pick is one
/// undo step.
fn range_view<'a>(module: u32, index: usize, range: RangeRow) -> Element<'a, Message> {
    let options: Vec<Named> = range
        .choices
        .into_iter()
        .enumerate()
        .map(|(i, name)| Named { index: i, name })
        .collect();
    let current = range.selected.and_then(|i| options.get(i).cloned());
    tooltip(
        pick_list(options, current, move |n: Named| Message::Range {
            module,
            index,
            range: n.index,
        })
        .placeholder("Full")
        .text_size(style::SMALL)
        .width(96),
        container(text("Range: what a full turn of the control covers.").size(style::BODY))
            .padding(6)
            .style(container::rounded_box),
        tooltip::Position::Top,
    )
    .into()
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
) -> Element<'a, Message> {
    let center = iced::Alignment::Center;
    let mut rows = column![].spacing(6);
    for a in assignments {
        let id = a.id;
        // The same card as the modulation matrix, without the target line:
        // the target is this parameter (modulation.md, slice 3).
        let card = column![
            row![
                checkbox(a.enabled)
                    .on_toggle(move |enabled| Message::Enabled { id, enabled })
                    .size(style::SWITCH),
                text(a.source).size(style::SMALL),
                space::horizontal(),
                remove_button("Remove modulation", Message::Unassign(id)),
            ]
            .spacing(4)
            .align_y(center),
            pick_list(a.polarities, Some(a.polarity), move |choice| Message::Polarity {
                id,
                choice
            })
            .text_size(style::SMALL)
            .padding([1, 4])
            .style(style::quiet_pick_list),
            row![
                slider(-1.0..=1.0, a.depth, move |depth| Message::Depth { id, depth })
                    .on_release(Message::SliderReleased)
                    .step(0.01_f32),
                text(a.depth_text).size(style::SMALL).width(40),
            ]
            .spacing(6)
            .align_y(center),
        ]
        .spacing(2);
        rows = rows.push(container(card).padding(6).width(Length::Fill).style(style::card(false)));
    }
    rows = rows.push(
        button(text(if adding.is_some() { "Cancel" } else { "+ Add modulation" }).size(style::SMALL))
            .padding([2, 8])
            .on_press(Message::AddModulation { module, index }),
    );
    if let Some(sources) = adding {
        let mut list = column![].spacing(1);
        for s in sources {
            let source = s.source;
            list = list.push(
                button(text(s.name).size(style::SMALL))
                    .width(Length::Fill)
                    .padding([2, 6])
                    .style(style::secondary)
                    .on_press_maybe(s.enabled.then_some(Message::Assign { source, module, index })),
            );
        }
        rows = rows.push(container(list).padding([0, 8]));
    }
    rows.into()
}

/// **Drives**: one group per output. A line selects its target.
fn drives_view<'a>(drives: Vec<DrivesGroup>) -> Element<'a, Message> {
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
                    .on_press(Message::SelectModule(line.target)),
            );
        }
    }
    list.into()
}
