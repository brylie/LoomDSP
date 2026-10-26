//! The inspector: what it shows for the selection, as plain data, and the
//! widgets drawn from that data.
//!
//! The rows are built first as a small model, then turned into widgets. The
//! window and [`crate::App::ui_snapshot`] both read the same model, so a
//! test of the snapshot checks what the user sees.

use crate::modulation::{self, PolarityChoice};
use crate::{App, DIM, HELP, Message, param, remove_button};
use iced::widget::{
    button, column, container, pick_list, radio, row, scrollable, slider, space, text, toggler, tooltip,
};
use iced::{Element, Length};
use loom_core::{AssignmentId, ModuleId, OutputRef};
use serde::Serialize;

/// What the inspector shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Inspector {
    /// Nothing selected: a title and help text.
    Help,
    /// Several modules selected: how many.
    Several(usize),
    /// One module selected.
    Module(ModuleRows),
    /// The selected module is gone. Nothing is shown.
    Empty,
}

/// The inspector for one module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModuleRows {
    pub id: u32,
    pub name: &'static str,
    /// The module's kind and ID, under its name.
    pub kind: String,
    pub description: &'static str,
    /// One row per parameter, in descriptor order. Empty shows "No
    /// parameters."
    pub params: Vec<ParamRow>,
    /// **Drives**: one group per output that drives at least one
    /// assignment. Empty shows no **Drives** section.
    pub drives: Vec<DrivesGroup>,
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
    /// The MIDI control mapped to it.
    pub mapping: Option<String>,
    pub mode: ParamMode,
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
        let graph = self.controller.graph();
        let id = match self.selection.len() {
            0 => return Inspector::Help,
            1 => *self.selection.iter().next().expect("one module selected"),
            n => return Inspector::Several(n),
        };
        let Some(node) = graph.node(ModuleId(id)) else {
            return Inspector::Empty;
        };
        let d = node.descriptor;
        let params = d
            .params
            .iter()
            .zip(&node.params)
            .enumerate()
            .map(|(index, (spec, &value))| {
                // Hovering the label shows what the canvas hover text shows
                // (panels.md, Labels and hover text).
                let hover = loom_canvas::HoverText::new(spec.label, spec.description, spec.name);
                let mapping = graph
                    .midi_mapping_of(ModuleId(id), index)
                    .map(|(_, m)| m.control.to_string());
                let mode = if self.learning {
                    // In learn mode a row arms its parameter, and its value
                    // does not change (midi-learn.md).
                    let armed = self.armed.is_some_and(|a| a.param == (id, index));
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
                        adding: (self.adding == Some((id, index))).then(|| self.sources(id, index)),
                    }
                };
                ParamRow {
                    index,
                    label: spec.label,
                    description: spec.description,
                    name: hover.name,
                    value: param::format(spec, value),
                    mapping,
                    mode,
                }
            })
            .collect();
        Inspector::Module(ModuleRows {
            id,
            name: d.name,
            kind: format!("{} #{id}", d.kind()),
            description: d.description,
            params,
            drives: self.drives(id),
        })
    }

    /// The assignments on a parameter, in engine order
    /// (`docs/design/modulation.md`, Seeing assignments).
    fn assignment_rows(&self, module: u32, index: usize) -> Vec<AssignmentRow> {
        let graph = self.controller.graph();
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
        let graph = self.controller.graph();
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
        let graph = self.controller.graph();
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
pub fn view<'a>(model: Inspector) -> Element<'a, Message> {
    let content: Element<'a, Message> = match model {
        Inspector::Help => column![
            text("Inspector").size(17),
            text("Select a module to edit its parameters.").size(12).color(DIM),
            text(HELP).size(11).color(DIM),
        ]
        .spacing(8)
        .into(),
        Inspector::Several(n) => text(format!("{n} modules selected")).size(13).into(),
        Inspector::Empty => text("").into(),
        Inspector::Module(m) => module_view(m),
    };
    container(scrollable(container(content).padding(12)))
        .width(280)
        .height(Length::Fill)
        .into()
}

fn module_view<'a>(m: ModuleRows) -> Element<'a, Message> {
    let id = m.id;
    let mut items = column![
        text(m.name).size(17),
        text(m.kind).size(11).color(DIM),
        text(m.description).size(12),
    ]
    .spacing(6);
    if m.params.is_empty() {
        items = items.push(text("No parameters.").size(12).color(DIM));
    }
    for p in m.params {
        let index = p.index;
        let unmap = Message::Unmap { module: id, index };
        let line: Element<'a, Message> = match p.mode {
            ParamMode::Learn { armed, detail } => {
                let arm = button(row![text(p.label).size(12), space::horizontal(), text(detail).size(12)])
                    .width(Length::Fill)
                    .style(if armed { button::primary } else { button::secondary })
                    .on_press(Message::LearnParam { module: id, index });
                // A mapping can be removed in learn mode too.
                let mut line = row![arm].spacing(4).align_y(iced::Alignment::Center);
                if p.mapping.is_some() {
                    line = line.push(remove_button("Remove MIDI mapping", unmap));
                }
                line.into()
            }
            ParamMode::Edit {
                position,
                step,
                control,
                assignments,
                adding,
            } => {
                let mut tip = column![text(p.description).size(12)].spacing(4);
                if let Some(name) = p.name {
                    tip = tip.push(text(name).size(11).color(DIM));
                }
                let mut rows = column![
                    row![
                        tooltip(
                            text(p.label).size(12),
                            container(tip).padding(6).max_width(260).style(container::rounded_box),
                            tooltip::Position::Top,
                        ),
                        space::horizontal(),
                        text(p.value).size(12).color(DIM),
                    ],
                    value_control(id, index, control, position, step),
                ]
                .spacing(2);
                if let Some(control) = p.mapping {
                    rows = rows.push(
                        row![
                            text(control).size(11).color(DIM),
                            space::horizontal(),
                            remove_button("Remove MIDI mapping", unmap),
                        ]
                        .align_y(iced::Alignment::Center),
                    );
                }
                rows.push(modulation_view(id, index, assignments, adding)).into()
            }
        };
        items = items.push(line);
    }
    if !m.drives.is_empty() {
        items = items.push(drives_view(m.drives));
    }
    items.into()
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
                list = list.push(radio(name, i, Some(selected), pick).size(12).text_size(12));
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
                .text_size(12)
                .width(Length::Fill)
                .into()
        }
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
        let assignment = column![
            row![
                text(a.source).size(11),
                space::horizontal(),
                remove_button("Remove modulation", Message::Unassign(id)),
            ]
            .align_y(center),
            row![
                slider(-1.0..=1.0, a.depth, move |depth| Message::Depth { id, depth })
                    .on_release(Message::SliderReleased)
                    .step(0.01_f32),
                text(a.depth_text).size(11).width(36),
            ]
            .spacing(6)
            .align_y(center),
            row![
                pick_list(a.polarities, Some(a.polarity), move |choice| Message::Polarity {
                    id,
                    choice
                },)
                .text_size(11),
                space::horizontal(),
                toggler(a.enabled)
                    .label(if a.enabled { "On" } else { "Off" })
                    .on_toggle(move |enabled| Message::Enabled { id, enabled })
                    .size(14)
                    .text_size(11),
            ]
            .spacing(6)
            .align_y(center),
        ]
        .spacing(3);
        rows = rows.push(container(assignment).padding([0, 8]));
    }
    rows = rows.push(
        button(text(if adding.is_some() { "Cancel" } else { "Add modulation" }).size(11))
            .padding([2, 6])
            .style(button::text)
            .on_press(Message::AddModulation { module, index }),
    );
    if let Some(sources) = adding {
        let mut list = column![].spacing(1);
        for s in sources {
            let source = s.source;
            list = list.push(
                button(text(s.name).size(11))
                    .width(Length::Fill)
                    .padding([2, 6])
                    .style(button::secondary)
                    .on_press_maybe(s.enabled.then_some(Message::Assign { source, module, index })),
            );
        }
        rows = rows.push(container(list).padding([0, 8]));
    }
    rows.into()
}

/// **Drives**: one group per output. A line selects its target.
fn drives_view<'a>(drives: Vec<DrivesGroup>) -> Element<'a, Message> {
    let mut list = column![text("Drives").size(13)].spacing(4);
    for group in drives {
        list = list.push(text(group.output).size(11).color(DIM));
        for line in group.lines {
            let label = text(line.text).size(11);
            list = list.push(
                button(if line.enabled { label } else { label.color(DIM) })
                    .width(Length::Fill)
                    .padding([2, 8])
                    .style(button::text)
                    .on_press(Message::SelectModule(line.target)),
            );
        }
    }
    list.into()
}
