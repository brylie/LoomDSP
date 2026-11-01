//! The modulation matrix: a patch-level view of assignments and its
//! session-only sorting, filtering, selection, and edit state.

use crate::modulation::{self, PolarityChoice};
use crate::style::{self, button};
use crate::view::remove_button;
use crate::{App, DIM, Message};
use iced::widget::{
    checkbox, column, container, mouse_area, pick_list, row, scrollable, slider, space, text, text_input,
};
use iced::{Element, Length, keyboard};
use loom_core::{AssignmentId, Graph, ModuleId, OutputRef, WidgetKind};
use serde::Serialize;
use std::fmt;

/// Finds the selected cell and reveals it with the smallest scroll.
struct RevealRow {
    viewport: Option<(iced::Rectangle, iced::Vector)>,
    row: Option<iced::Rectangle>,
    cell: Option<iced::Rectangle>,
}

impl<T: 'static> iced::advanced::widget::Operation<T> for RevealRow {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<T>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        bounds: iced::Rectangle,
        _content_bounds: iced::Rectangle,
        translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&iced::advanced::widget::Id::new("matrix-table")) {
            self.viewport = Some((bounds, translation));
        }
    }

    fn container(&mut self, id: Option<&iced::advanced::widget::Id>, bounds: iced::Rectangle) {
        if id == Some(&iced::advanced::widget::Id::new("matrix-selected-row")) {
            self.row = Some(bounds);
        } else if id == Some(&iced::advanced::widget::Id::new("matrix-selected-cell")) {
            self.cell = Some(bounds);
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<T> {
        use iced::advanced::widget::operation::{Outcome, scrollable};
        let Some((viewport, translation)) = self.viewport else {
            return Outcome::None;
        };
        let y = self.row.map(|row| {
            let top = row.y - translation.y;
            let bottom = top + row.height;
            if top < viewport.y {
                (translation.y + top - viewport.y).max(0.0)
            } else if bottom > viewport.y + viewport.height {
                translation.y + bottom - (viewport.y + viewport.height)
            } else {
                translation.y
            }
        });
        let x = self.cell.map(|cell| {
            let left = cell.x - translation.x;
            let right = left + cell.width;
            if left < viewport.x {
                (translation.x + left - viewport.x).max(0.0)
            } else if right > viewport.x + viewport.width {
                translation.x + right - (viewport.x + viewport.width)
            } else {
                translation.x
            }
        });
        Outcome::Chain(Box::new(scrollable::scroll_to(
            iced::advanced::widget::Id::new("matrix-table"),
            scrollable::AbsoluteOffset { x, y },
        )))
    }
}

/// Reveals the selected matrix row and column after the view is laid out.
pub(crate) fn reveal_selected() -> iced::Task<Message> {
    iced::advanced::widget::operate(RevealRow {
        viewport: None,
        row: None,
        cell: None,
    })
}

/// Marks the active cell so its actual bounds can guide scrolling.
fn matrix_cell<'a>(content: impl Into<Element<'a, Message>>, selected: bool) -> Element<'a, Message> {
    let cell = container(content);
    if selected {
        cell.id("matrix-selected-cell")
    } else {
        cell
    }
    .into()
}

/// The right-hand panel visible this session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    #[default]
    Inspector,
    Matrix,
}

/// One sortable, editable matrix column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    #[default]
    On,
    Source,
    Target,
    Depth,
    Polarity,
}

impl Column {
    /// Every column in keyboard order, which is reading order on a card:
    /// on and source, then target and polarity, then depth.
    pub const ALL: [Self; 5] = [Self::On, Self::Source, Self::Target, Self::Polarity, Self::Depth];

    /// The heading shown above this column.
    pub const fn label(self) -> &'static str {
        match self {
            Self::On => "On",
            Self::Source => "Source",
            Self::Target => "Target",
            Self::Depth => "Depth",
            Self::Polarity => "Polarity",
        }
    }
}

/// A source in the Add and Source pick lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePick {
    pub output: OutputRef,
    pub label: String,
}

impl fmt::Display for SourcePick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// A target in the Add and Target pick lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPick {
    pub module: ModuleId,
    pub param: usize,
    pub label: String,
}

impl fmt::Display for TargetPick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// One visible assignment row, built from the current graph.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatrixRow {
    pub id: u32,
    pub on: bool,
    pub source: String,
    pub target: String,
    pub depth: f32,
    pub polarity: String,
}

/// The same plain data used by the window and UI snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatrixModel {
    pub tab: Tab,
    pub sort: Option<(Column, bool)>,
    pub filter: String,
    pub selected_only: bool,
    pub selected: Option<u32>,
    pub cell: Column,
    pub editing: bool,
    pub rows: Vec<MatrixRow>,
}

/// A pending cell edit. Only Apply changes the graph.
#[derive(Debug, Clone)]
pub struct Edit {
    pub id: AssignmentId,
    pub column: Column,
    pub depth: String,
    pub choice: usize,
}

/// The two picks for a new assignment.
#[derive(Debug, Clone, Default)]
pub struct Add {
    pub source: Option<SourcePick>,
    pub target: Option<TargetPick>,
}

/// Everything the matrix remembers for this app session.
#[derive(Debug, Default)]
pub struct State {
    pub tab: Tab,
    pub sort: Option<(Column, bool)>,
    pub filter: String,
    pub selected_only: bool,
    pub selected: Option<AssignmentId>,
    pub cell: Column,
    pub edit: Option<Edit>,
    pub add: Option<Add>,
    pub focused: bool,
}

impl State {
    /// Rows in the current graph level, with filters and sorting applied.
    pub fn model(&self, graph: &Graph, selected_modules: &std::collections::BTreeSet<u32>) -> MatrixModel {
        let filter = self.filter.to_lowercase();
        let mut rows: Vec<_> = graph
            .assignments()
            .filter(|(_, a)| {
                (!self.selected_only
                    || selected_modules.contains(&a.source.module.0)
                    || selected_modules.contains(&a.target.0))
                    && (filter.is_empty()
                        || modulation::source_name(graph, a.source)
                            .to_lowercase()
                            .contains(&filter)
                        || modulation::target_name(graph, a.target, a.param)
                            .to_lowercase()
                            .contains(&filter))
            })
            .map(|(id, a)| MatrixRow {
                id: id.0,
                on: a.enabled,
                source: modulation::source_name(graph, a.source),
                target: modulation::target_name(graph, a.target, a.param),
                depth: a.depth,
                polarity: PolarityChoice::of(a.polarity, modulation::source_polarity(graph, a.source)).to_string(),
            })
            .collect();
        if let Some((column, reverse)) = self.sort {
            rows.sort_by(|a, b| {
                let cmp = match column {
                    Column::On => a.on.cmp(&b.on),
                    Column::Source => a.source.cmp(&b.source),
                    Column::Target => a.target.cmp(&b.target),
                    Column::Depth => a.depth.total_cmp(&b.depth),
                    Column::Polarity => a.polarity.cmp(&b.polarity),
                };
                let cmp = if reverse { cmp.reverse() } else { cmp };
                cmp.then_with(|| a.id.cmp(&b.id))
            });
        }
        MatrixModel {
            tab: self.tab,
            sort: self.sort,
            filter: self.filter.clone(),
            selected_only: self.selected_only,
            selected: self.selected.map(|id| id.0),
            cell: self.cell,
            editing: self.edit.is_some(),
            rows,
        }
    }

    /// Drops a selected row or edit if its assignment was deleted.
    pub fn reconcile(&mut self, graph: &Graph) {
        if self.selected.is_some_and(|id| graph.assignment(id).is_none()) {
            self.selected = None;
            self.edit = None;
        }
    }
}

/// Sources offered by the matrix, in module and port order.
pub fn sources(graph: &Graph) -> Vec<SourcePick> {
    modulation::sources(graph)
        .into_iter()
        .map(|output| SourcePick {
            output,
            label: modulation::source_name(graph, output),
        })
        .collect()
}

/// Parameters with a knob, toggle, or radio control, in module order.
pub fn targets(graph: &Graph) -> Vec<TargetPick> {
    graph
        .nodes()
        .flat_map(|(module, node)| {
            node.descriptor
                .params
                .iter()
                .enumerate()
                .filter_map(move |(param, spec)| {
                    let eligible = !spec.options.transient
                        && node.descriptor.panel.widgets.iter().any(|w| {
                            w.bind == spec.name
                                && matches!(
                                    w.kind,
                                    WidgetKind::Knob
                                        | WidgetKind::Toggle
                                        | WidgetKind::Radio
                                        | WidgetKind::Segmented { .. }
                                )
                        });
                    eligible.then(|| TargetPick {
                        module,
                        param,
                        label: modulation::target_name(graph, module, param),
                    })
                })
        })
        .collect()
}

/// Messages produced by the matrix widgets.
#[derive(Debug, Clone)]
pub enum Event {
    Tab(Tab),
    /// Sort by a column, or `None` for engine order, keeping the direction.
    SortBy(Option<Column>),
    /// Reverse the current sort.
    ReverseSort,
    Filter(String),
    SelectedOnly(bool),
    Select(AssignmentId, Column),
    Edit(AssignmentId, Column),
    DepthText(String),
    DepthDrag(AssignmentId, f32),
    DepthReleased,
    Source(AssignmentId, SourcePick),
    Target(AssignmentId, TargetPick),
    Polarity(AssignmentId, PolarityChoice),
    Enabled(AssignmentId, bool),
    Remove(AssignmentId),
    AddOpen,
    AddSource(SourcePick),
    AddTarget(TargetPick),
    AddApply,
    Apply,
    Cancel,
}

/// Width kept for a card's depth value, such as "-100%".
const DEPTH_TEXT: f32 = 52.0;

/// How the Sort by pick list names a sort: engine order or a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortPick(pub Option<Column>);

impl SortPick {
    /// Engine order, then every column in keyboard order.
    fn all() -> Vec<Self> {
        std::iter::once(Self(None))
            .chain(Column::ALL.into_iter().map(|c| Self(Some(c))))
            .collect()
    }
}

impl fmt::Display for SortPick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.map_or("Engine order", Column::label))
    }
}

/// Draws the tab's matrix from the same model used by the snapshot: one
/// card per assignment, so every part fits the right-hand column without
/// scrolling sideways (modulation.md, slice 3).
pub fn view<'a>(model: MatrixModel, state: &State, graph: &Graph) -> Element<'a, Message> {
    let mut content = column![].spacing(7).width(Length::Fill);
    content = content.push(
        text_input("Filter modules or parameters", &model.filter)
            .on_input(|s| Message::Matrix(Event::Filter(s)))
            .size(style::CONTROL),
    );
    content = content.push(
        row![
            checkbox(model.selected_only)
                .label("Selected module only")
                .on_toggle(|v| Message::Matrix(Event::SelectedOnly(v)))
                .size(style::SWITCH)
                .text_size(style::CONTROL),
            space::horizontal(),
            button(text("Add").size(style::CONTROL)).on_press(Message::Matrix(Event::AddOpen)),
        ]
        .align_y(iced::Alignment::Center),
    );
    let mut sort = row![
        text("Sort by").size(style::SMALL).color(DIM),
        pick_list(SortPick::all(), Some(SortPick(model.sort.map(|(c, _)| c))), |p| {
            Message::Matrix(Event::SortBy(p.0))
        })
        .text_size(style::SMALL),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    if let Some((_, reverse)) = model.sort {
        sort = sort.push(iced::widget::tooltip(
            button(text(if reverse { "↓" } else { "↑" }).size(style::CONTROL))
                .style(style::plain)
                .on_press(Message::Matrix(Event::ReverseSort)),
            container(text("Reverse the order").size(style::BODY))
                .padding(6)
                .style(container::rounded_box),
            iced::widget::tooltip::Position::Top,
        ));
    }
    content = content.push(sort);
    if let Some(add) = &state.add {
        let sources = sources(graph);
        let targets = targets(graph);
        content = content.push(
            column![
                pick_list(sources, add.source.clone(), |s| Message::Matrix(Event::AddSource(s)))
                    .placeholder("Source output")
                    .text_size(style::SMALL),
                pick_list(targets, add.target.clone(), |t| Message::Matrix(Event::AddTarget(t)))
                    .placeholder("Target parameter")
                    .text_size(style::SMALL),
                row![
                    button("Create")
                        .on_press(Message::Matrix(Event::AddApply))
                        .style(style::primary),
                    button("Cancel").on_press(Message::Matrix(Event::Cancel)),
                ]
                .spacing(5),
            ]
            .spacing(5),
        );
    }
    let mut table = column![].spacing(6).width(Length::Fill);
    if model.rows.is_empty() {
        table = table.push(text("No modulation assignments").size(style::BODY).color(DIM));
    }
    for a in model.rows {
        let id = AssignmentId(a.id);
        let selected = state.selected == Some(id);
        let editing = state.edit.as_ref().filter(|e| e.id == id);
        let focused = |column: Column| selected && state.cell == column;
        // A cell is plain text until edited; a click selects it and edits.
        let cell = |label: String, column: Column| -> Element<'a, Message> {
            button(text(label).size(style::SMALL))
                .padding([1, 4])
                .style(style::cell(focused(column)))
                .on_press(Message::Matrix(Event::Edit(id, column)))
                .into()
        };
        let on = checkbox(a.on)
            .on_toggle(move |v| Message::Matrix(Event::Enabled(id, v)))
            .size(style::SWITCH);
        let source: Element<'a, Message> = if editing.is_some_and(|e| e.column == Column::Source) {
            let options = sources(graph);
            let current = editing.and_then(|e| options.get(e.choice)).cloned();
            pick_list(options, current, move |s| Message::Matrix(Event::Source(id, s)))
                .text_size(style::SMALL)
                .into()
        } else {
            cell(a.source, Column::Source)
        };
        let target: Element<'a, Message> = if editing.is_some_and(|e| e.column == Column::Target) {
            let options = targets(graph);
            let current = editing.and_then(|e| options.get(e.choice)).cloned();
            pick_list(options, current, move |t| Message::Matrix(Event::Target(id, t)))
                .text_size(style::SMALL)
                .into()
        } else {
            cell(a.target, Column::Target)
        };
        let depth_value: Element<'a, Message> = if let Some(e) = editing.filter(|e| e.column == Column::Depth) {
            text_input(&modulation::depth_text(a.depth), &e.depth)
                .id("matrix-depth")
                .on_input(|s| Message::Matrix(Event::DepthText(s)))
                .on_submit(Message::Matrix(Event::Apply))
                .size(style::SMALL)
                .width(52)
                .into()
        } else {
            cell(modulation::depth_text(a.depth), Column::Depth)
        };
        let depth_slider = slider(-1.0..=1.0, a.depth, move |v| Message::Matrix(Event::DepthDrag(id, v)))
            .on_release(Message::Matrix(Event::DepthReleased))
            .step(0.01_f32)
            // Cards share one width, so filling it lines the sliders up.
            .width(Length::Fill);
        let polarity: Element<'a, Message> = if let Some(edit) = editing.filter(|e| e.column == Column::Polarity)
            && let Some(assignment) = graph.assignment(id)
        {
            let own = modulation::source_polarity(graph, assignment.source);
            let options = PolarityChoice::options(own);
            let current = options[edit.choice];
            pick_list(options, Some(current), move |p| Message::Matrix(Event::Polarity(id, p)))
                .text_size(style::SMALL)
                .into()
        } else {
            cell(a.polarity, Column::Polarity)
        };
        let card = column![
            row![
                matrix_cell(on, focused(Column::On)),
                matrix_cell(source, focused(Column::Source)),
                space::horizontal(),
                remove_button("Remove modulation", Message::Matrix(Event::Remove(id))),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
            row![
                text("→").size(style::SMALL).color(DIM),
                matrix_cell(target, focused(Column::Target)),
                space::horizontal(),
                matrix_cell(polarity, focused(Column::Polarity)),
            ]
            .spacing(4)
            .padding([0, 4])
            .align_y(iced::Alignment::Center),
            row![
                depth_slider,
                container(matrix_cell(depth_value, focused(Column::Depth))).width(DEPTH_TEXT),
            ]
            .spacing(6)
            .padding([0, 4])
            .align_y(iced::Alignment::Center),
        ]
        .spacing(2);
        let card = mouse_area(
            container(card)
                .padding(6)
                .width(Length::Fill)
                .style(style::card(selected)),
        )
        .on_press(Message::Matrix(Event::Select(id, state.cell)));
        let card = container(card);
        table = table.push(if selected { card.id("matrix-selected-row") } else { card });
    }
    content = content.push(
        scrollable(table)
            .id("matrix-table")
            .spacing(4)
            .width(Length::Fill)
            .height(Length::Fill),
    );
    container(content.padding(8)).width(280).height(Length::Fill).into()
}

/// Parses a signed percentage and rejects anything outside the edit range.
pub fn parse_depth(input: &str) -> Option<f32> {
    let percent: f32 = input.trim().trim_end_matches('%').trim().parse().ok()?;
    (percent.is_finite() && (-100.0..=100.0).contains(&percent)).then_some(percent / 100.0)
}

impl App {
    /// Handles a matrix event, and returns what the view must do after it:
    /// focus the depth field when an edit opens it, or scroll the selected
    /// card into view when the order or filter changes.
    pub(crate) fn matrix_message(&mut self, event: Event) -> iced::Task<Message> {
        let task = match event {
            Event::Edit(_, Column::Depth) => iced::widget::operation::focus("matrix-depth"),
            Event::SortBy(_) | Event::ReverseSort | Event::Filter(_) | Event::SelectedOnly(_) => reveal_selected(),
            _ => iced::Task::none(),
        };
        self.matrix_event(event);
        task
    }

    /// Handles a key while the matrix has focus. Keeps the selected card in
    /// view, and focuses the depth field when Enter opens it.
    pub(crate) fn matrix_key_message(&mut self, key: keyboard::Event) -> iced::Task<Message> {
        let open_depth = matches!(
            key,
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Enter),
                ..
            }
        ) && self.matrix.edit.is_none()
            && self.matrix.cell == Column::Depth;
        self.matrix_key(key);
        let scroll = if self.matrix.tab == Tab::Matrix && self.matrix.focused {
            reveal_selected()
        } else {
            iced::Task::none()
        };
        if open_depth && self.matrix.edit.as_ref().is_some_and(|e| e.column == Column::Depth) {
            iced::Task::batch([scroll, iced::widget::operation::focus("matrix-depth")])
        } else {
            scroll
        }
    }

    /// Which one matrix assignment the canvas should show without pinning.
    pub(crate) fn matrix_line(&self) -> Option<loom_canvas::ModulationView> {
        if self.matrix.tab != Tab::Matrix {
            return None;
        }
        let id = self.matrix.selected?;
        let a = self.controller.graph().assignment(id)?;
        let descriptor = self.controller.graph().node(a.target)?.descriptor;
        let name = descriptor.params.get(a.param)?.name;
        Some(loom_canvas::ModulationView {
            id: id.0,
            from: (a.source.module.0, a.source.port),
            to: a.target.0,
            widget: descriptor
                .panel
                .widgets
                .iter()
                .position(|w| w.is_control() && w.bind == name),
            enabled: a.enabled,
        })
    }

    /// Handles a matrix widget without adding view state to undo history.
    pub(crate) fn matrix_event(&mut self, event: Event) {
        match event {
            Event::Tab(tab) => {
                self.matrix.tab = tab;
                self.matrix.focused = tab == Tab::Matrix;
                self.matrix.edit = None;
                self.matrix.add = None;
            }
            Event::SortBy(column) => {
                // A new column starts ascending; the same one keeps its
                // direction.
                let reverse = self.matrix.sort.is_some_and(|(c, r)| Some(c) == column && r);
                self.matrix.sort = column.map(|c| (c, reverse));
            }
            Event::ReverseSort => {
                if let Some((_, reverse)) = &mut self.matrix.sort {
                    *reverse = !*reverse;
                }
            }
            Event::Filter(filter) => {
                self.matrix.filter = filter;
                self.matrix.focused = false;
            }
            Event::SelectedOnly(value) => self.matrix.selected_only = value,
            Event::Select(id, column) => self.matrix_select(id, column),
            Event::Edit(id, column) => self.matrix_begin_edit(id, column),
            Event::DepthText(s) => {
                if let Some(e) = &mut self.matrix.edit {
                    e.depth = s;
                }
            }
            Event::DepthDrag(id, depth) => self.handle(Message::Depth { id, depth }),
            Event::DepthReleased => self.handle(Message::SliderReleased),
            Event::Source(id, source) => self.matrix_retarget(id, Some(source.output), None),
            Event::Target(id, target) => self.matrix_retarget(id, None, Some((target.module, target.param))),
            Event::Polarity(id, choice) => {
                self.handle(Message::Polarity { id, choice });
                self.matrix.edit = None;
            }
            Event::Enabled(id, enabled) => {
                self.matrix_select(id, Column::On);
                self.handle(Message::Enabled { id, enabled });
            }
            Event::Remove(id) => {
                self.handle(Message::Unassign(id));
                self.matrix.reconcile(self.controller.graph());
            }
            Event::AddOpen => {
                // Pressing Add again keeps what the open form has picked.
                self.matrix.add.get_or_insert_with(Add::default);
                self.matrix.focused = true;
                self.matrix.cell = Column::Source;
            }
            Event::AddSource(source) => {
                if let Some(add) = &mut self.matrix.add {
                    add.source = Some(source);
                }
            }
            Event::AddTarget(target) => {
                if let Some(add) = &mut self.matrix.add {
                    add.target = Some(target);
                }
            }
            Event::AddApply => self.matrix_add(),
            Event::Apply => self.matrix_apply(),
            Event::Cancel => {
                self.matrix.edit = None;
                self.matrix.add = None;
            }
        }
    }

    /// Selects a row and its target module, leaving the badge pin alone.
    fn matrix_select(&mut self, id: AssignmentId, column: Column) {
        if let Some(a) = self.controller.graph().assignment(id) {
            self.matrix.selected = Some(id);
            self.matrix.cell = column;
            self.matrix.focused = true;
            self.selection = std::collections::BTreeSet::from([a.target.0]);
        }
    }

    /// Opens an editor with the current value as its draft.
    fn matrix_begin_edit(&mut self, id: AssignmentId, column: Column) {
        self.matrix_select(id, column);
        let Some(a) = self.controller.graph().assignment(id).copied() else {
            return;
        };
        if column == Column::On {
            self.matrix_event(Event::Enabled(id, !a.enabled));
            return;
        }
        let choice = match column {
            Column::Source => sources(self.controller.graph())
                .iter()
                .position(|s| s.output == a.source)
                .unwrap_or(0),
            Column::Target => targets(self.controller.graph())
                .iter()
                .position(|t| (t.module, t.param) == (a.target, a.param))
                .unwrap_or(0),
            Column::Polarity => match a.polarity {
                None => 0,
                Some(loom_core::Polarity::Unipolar) => 1,
                Some(loom_core::Polarity::Bipolar) => 2,
            },
            _ => 0,
        };
        self.matrix.edit = Some(Edit {
            id,
            column,
            depth: String::new(),
            choice,
        });
    }

    /// Applies a source or target change as a single undoable graph edit.
    fn matrix_retarget(&mut self, id: AssignmentId, source: Option<OutputRef>, target: Option<(ModuleId, usize)>) {
        let Some(old) = self.controller.graph().assignment(id).copied() else {
            return;
        };
        let source = source.unwrap_or(old.source);
        let (module, param) = target.unwrap_or((old.target, old.param));
        if self
            .controller
            .graph()
            .assignments()
            .any(|(other, a)| other != id && a.source == source && a.target == module && a.param == param)
        {
            self.alerts.say(format!(
                "{} already modulates {}",
                modulation::source_name(self.controller.graph(), source),
                modulation::target_name(self.controller.graph(), module, param)
            ));
            return;
        }
        self.close_slider_gesture();
        self.edit(
            |app| match app.controller.retarget_assignment(id, source, module, param) {
                Ok(()) => {
                    app.commit();
                }
                Err(e) => app.alerts.say(format!("error: {e}")),
            },
        );
        self.matrix.edit = None;
        self.matrix_select(id, self.matrix.cell);
    }

    /// Creates the two picked endpoints with the normal app assignment path.
    fn matrix_add(&mut self) {
        let Some(add) = self.matrix.add.as_ref() else {
            return;
        };
        let (Some(source), Some(target)) = (&add.source, &add.target) else {
            self.alerts.say("Choose a source and a target".to_string());
            return;
        };
        let (source, module, param) = (source.output, target.module, target.param);
        self.close_slider_gesture();
        self.edit(|app| app.assign(source, module, param));
        self.matrix.add = None;
    }

    /// Commits a typed depth or a keyboard-selected source, target, or polarity.
    fn matrix_apply(&mut self) {
        let Some(edit) = self.matrix.edit.clone() else {
            return;
        };
        match edit.column {
            Column::On => {}
            Column::Depth => match parse_depth(&edit.depth) {
                Some(depth) => {
                    self.handle(Message::Depth { id: edit.id, depth });
                    self.handle(Message::SliderReleased);
                    self.matrix.edit = None;
                }
                None => self.alerts.say("Depth must be between -100% and 100%".to_string()),
            },
            Column::Source => {
                if let Some(source) = sources(self.controller.graph()).get(edit.choice) {
                    self.matrix_retarget(edit.id, Some(source.output), None);
                }
            }
            Column::Target => {
                if let Some(target) = targets(self.controller.graph()).get(edit.choice) {
                    self.matrix_retarget(edit.id, None, Some((target.module, target.param)));
                }
            }
            Column::Polarity => {
                if let Some(a) = self.controller.graph().assignment(edit.id) {
                    let own = modulation::source_polarity(self.controller.graph(), a.source);
                    self.matrix_event(Event::Polarity(edit.id, PolarityChoice::options(own)[edit.choice]));
                }
            }
        }
    }

    /// Keyboard path through rows, cells, Add, and cell editors.
    pub(crate) fn matrix_key(&mut self, event: keyboard::Event) {
        use keyboard::key::Named;
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return;
        };
        if self.matrix.tab != Tab::Matrix || !self.matrix.focused || self.browser.is_some() || self.rig.window.is_some()
        {
            return;
        }
        if modifiers.command() {
            return;
        }
        if key == keyboard::Key::Named(Named::Escape) {
            if self.matrix.edit.is_some() || self.matrix.add.is_some() {
                self.matrix_event(Event::Cancel);
            } else {
                self.matrix.focused = false;
            }
            return;
        }
        if key == keyboard::Key::Named(Named::Tab) {
            self.matrix.focused = false;
            self.matrix.edit = None;
            return;
        }
        if let Some(add) = self.matrix.add.as_mut() {
            let options_sources = sources(self.controller.graph());
            let options_targets = targets(self.controller.graph());
            match key {
                keyboard::Key::Named(Named::ArrowLeft) => self.matrix.cell = Column::Source,
                keyboard::Key::Named(Named::ArrowRight) => self.matrix.cell = Column::Target,
                keyboard::Key::Named(Named::ArrowDown | Named::ArrowUp) => {
                    let down = key == keyboard::Key::Named(Named::ArrowDown);
                    if self.matrix.cell == Column::Source && !options_sources.is_empty() {
                        let next = add
                            .source
                            .as_ref()
                            .and_then(|s| options_sources.iter().position(|x| x == s))
                            .map_or(0, |i| {
                                if down {
                                    (i + 1) % options_sources.len()
                                } else {
                                    (i + options_sources.len() - 1) % options_sources.len()
                                }
                            });
                        add.source = Some(options_sources[next].clone());
                    } else if !options_targets.is_empty() {
                        let next = add
                            .target
                            .as_ref()
                            .and_then(|t| options_targets.iter().position(|x| x == t))
                            .map_or(0, |i| {
                                if down {
                                    (i + 1) % options_targets.len()
                                } else {
                                    (i + options_targets.len() - 1) % options_targets.len()
                                }
                            });
                        add.target = Some(options_targets[next].clone());
                    }
                }
                keyboard::Key::Named(Named::Enter) => self.matrix_add(),
                _ => {}
            }
            return;
        }
        if let Some(edit) = &mut self.matrix.edit {
            match key {
                keyboard::Key::Named(Named::ArrowUp | Named::ArrowDown) if edit.column != Column::Depth => {
                    let len = match edit.column {
                        Column::Source => sources(self.controller.graph()).len(),
                        Column::Target => targets(self.controller.graph()).len(),
                        Column::Polarity => 3,
                        _ => 0,
                    };
                    if len > 0 {
                        edit.choice = if key == keyboard::Key::Named(Named::ArrowDown) {
                            (edit.choice + 1) % len
                        } else {
                            (edit.choice + len - 1) % len
                        };
                    }
                }
                keyboard::Key::Named(Named::Enter) if edit.column != Column::Depth => self.matrix_apply(),
                _ => {}
            }
            return;
        }
        let model = self.matrix.model(self.controller.graph(), &self.selection);
        let Some(i) = self
            .matrix
            .selected
            .and_then(|id| model.rows.iter().position(|r| r.id == id.0))
        else {
            if let Some(first) = model.rows.first() {
                self.matrix_select(AssignmentId(first.id), Column::On);
            }
            return;
        };
        match key {
            keyboard::Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                let next = if key == keyboard::Key::Named(Named::ArrowDown) {
                    (i + 1).min(model.rows.len() - 1)
                } else {
                    i.saturating_sub(1)
                };
                self.matrix_select(AssignmentId(model.rows[next].id), self.matrix.cell);
            }
            keyboard::Key::Named(Named::ArrowLeft | Named::ArrowRight) => {
                let i = Column::ALL.iter().position(|&c| c == self.matrix.cell).unwrap_or(0);
                self.matrix.cell = Column::ALL[if key == keyboard::Key::Named(Named::ArrowRight) {
                    (i + 1).min(4)
                } else {
                    i.saturating_sub(1)
                }];
            }
            keyboard::Key::Named(Named::Enter) => {
                self.matrix_begin_edit(AssignmentId(model.rows[i].id), self.matrix.cell)
            }
            keyboard::Key::Named(Named::Delete) => self.matrix_event(Event::Remove(AssignmentId(model.rows[i].id))),
            _ => {}
        }
    }
}

/// Lets a focused editor keep captured keys except Escape and Tab.
pub(crate) fn key_message(key: keyboard::Event, status: iced::event::Status) -> Option<Message> {
    use keyboard::key::Named;
    let leaves_focus = matches!(
        &key,
        keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape | Named::Tab),
            ..
        }
    );
    if status == iced::event::Status::Captured && !leaves_focus {
        None
    } else {
        Some(Message::MatrixKey(key))
    }
}

/// Delivers matrix navigation keys from the Iced event stream.
pub(crate) fn key_listener(
    event: iced::Event,
    status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(key) => key_message(key, status),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
