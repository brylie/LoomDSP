//! The modulation matrix: a patch-level view of assignments and its
//! session-only sorting, filtering, selection, and edit state.

use crate::modulation::{self, PolarityChoice};
use crate::{App, DIM, Message, remove_button};
use iced::widget::{button, checkbox, column, container, pick_list, row, scrollable, slider, space, text, text_input};
use iced::{Element, Length, keyboard};
use loom_core::{AssignmentId, Graph, ModuleId, OutputRef, WidgetKind};
use serde::Serialize;
use std::fmt;

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
    /// Every column in left-to-right keyboard order.
    pub const ALL: [Self; 5] = [Self::On, Self::Source, Self::Target, Self::Depth, Self::Polarity];

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

    /// Selects a column heading, reversing a repeated sort.
    pub fn sort_by(&mut self, column: Column) {
        self.sort = Some((
            column,
            self.sort.is_some_and(|(old, reverse)| old == column && !reverse),
        ));
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
                                && matches!(w.kind, WidgetKind::Knob | WidgetKind::Toggle | WidgetKind::Radio)
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
    Sort(Column),
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

/// Draws the tab's matrix from the same model used by the snapshot.
pub fn view<'a>(model: MatrixModel, state: &State, graph: &Graph) -> Element<'a, Message> {
    let mut content = column![].spacing(7).width(Length::Fill);
    let sort = model.sort;
    let mut headings = row![space().width(20)].spacing(3);
    for column in Column::ALL {
        let arrow = match sort {
            Some((c, true)) if c == column => " ↓",
            Some((c, false)) if c == column => " ↑",
            _ => "",
        };
        let width = match column {
            Column::On => 40,
            Column::Source | Column::Target => 125,
            Column::Depth => 95,
            Column::Polarity => 140,
        };
        headings = headings.push(
            button(text(format!("{}{arrow}", column.label())).size(11))
                .on_press(Message::Matrix(Event::Sort(column)))
                .padding([2, 3])
                .width(width),
        );
    }
    let mut table = column![headings].spacing(3).width(Length::Fixed(600.0));
    content = content.push(
        text_input("Filter modules or parameters", &model.filter)
            .on_input(|s| Message::Matrix(Event::Filter(s)))
            .size(12),
    );
    content = content.push(
        checkbox(model.selected_only)
            .label("Selected module only")
            .on_toggle(|v| Message::Matrix(Event::SelectedOnly(v)))
            .size(13)
            .text_size(12),
    );
    if let Some(id) = model.selected {
        content = content.push(
            text(format!(
                "Row #{id}: {} cell{}",
                model.cell.label(),
                if model.editing { " (editing)" } else { "" }
            ))
            .size(11),
        );
    }
    if let Some(add) = &state.add {
        let sources = sources(graph);
        let targets = targets(graph);
        content = content.push(
            column![
                pick_list(sources, add.source.clone(), |s| Message::Matrix(Event::AddSource(s)))
                    .placeholder("Source output")
                    .text_size(11),
                pick_list(targets, add.target.clone(), |t| Message::Matrix(Event::AddTarget(t)))
                    .placeholder("Target parameter")
                    .text_size(11),
                row![
                    button("Create").on_press(Message::Matrix(Event::AddApply)),
                    button("Cancel").on_press(Message::Matrix(Event::Cancel)),
                ]
                .spacing(5),
            ]
            .spacing(5),
        );
    } else {
        content = content.push(button("Add").on_press(Message::Matrix(Event::AddOpen)));
    }
    if model.rows.is_empty() {
        table = table.push(text("No modulation assignments").size(12).color(DIM));
    }
    for a in model.rows {
        let id = AssignmentId(a.id);
        let selected = state.selected == Some(id);
        let editing = state.edit.as_ref().filter(|e| e.id == id);
        let on = checkbox(a.on)
            .on_toggle(move |v| Message::Matrix(Event::Enabled(id, v)))
            .size(13);
        let source: Element<'a, Message> = if editing.is_some_and(|e| e.column == Column::Source) {
            let options = sources(graph);
            let current = editing.and_then(|e| options.get(e.choice)).cloned();
            pick_list(options, current, move |s| Message::Matrix(Event::Source(id, s)))
                .text_size(11)
                .into()
        } else {
            button(text(a.source).size(11))
                .on_press(Message::Matrix(Event::Edit(id, Column::Source)))
                .style(if selected && state.cell == Column::Source {
                    button::primary
                } else {
                    button::secondary
                })
                .into()
        };
        let target: Element<'a, Message> = if editing.is_some_and(|e| e.column == Column::Target) {
            let options = targets(graph);
            let current = editing.and_then(|e| options.get(e.choice)).cloned();
            pick_list(options, current, move |t| Message::Matrix(Event::Target(id, t)))
                .text_size(11)
                .into()
        } else {
            button(text(a.target).size(11))
                .on_press(Message::Matrix(Event::Edit(id, Column::Target)))
                .style(if selected && state.cell == Column::Target {
                    button::primary
                } else {
                    button::secondary
                })
                .into()
        };
        let depth: Element<'a, Message> = if let Some(e) = editing.filter(|e| e.column == Column::Depth) {
            text_input(&modulation::depth_text(a.depth), &e.depth)
                .id("matrix-depth")
                .on_input(|s| Message::Matrix(Event::DepthText(s)))
                .on_submit(Message::Matrix(Event::Apply))
                .size(11)
                .width(55)
                .into()
        } else {
            row![
                button(text(modulation::depth_text(a.depth)).size(11))
                    .on_press(Message::Matrix(Event::Edit(id, Column::Depth)))
                    .style(if selected && state.cell == Column::Depth {
                        button::primary
                    } else {
                        button::secondary
                    }),
                slider(-1.0..=1.0, a.depth, move |v| Message::Matrix(Event::DepthDrag(id, v)))
                    .on_release(Message::Matrix(Event::DepthReleased))
                    .step(0.01_f32)
                    .width(55),
            ]
            .spacing(2)
            .into()
        };
        let polarity: Element<'a, Message> = if editing.is_some_and(|e| e.column == Column::Polarity) {
            let assignment = graph.assignment(id).expect("visible assignment");
            let own = modulation::source_polarity(graph, assignment.source);
            let options = PolarityChoice::options(own);
            let current = options[editing.expect("editing polarity").choice];
            pick_list(options, Some(current), move |p| Message::Matrix(Event::Polarity(id, p)))
                .text_size(11)
                .into()
        } else {
            button(text(a.polarity).size(11))
                .on_press(Message::Matrix(Event::Edit(id, Column::Polarity)))
                .style(if selected && state.cell == Column::Polarity {
                    button::primary
                } else {
                    button::secondary
                })
                .into()
        };
        let line = row![
            button(
                text(if selected {
                    if state.cell == Column::On { "▶" } else { "●" }
                } else {
                    "○"
                })
                .size(11)
            )
            .on_press(Message::Matrix(Event::Select(id, state.cell)))
            .width(20),
            container(on).width(40),
            container(source).width(125),
            container(target).width(125),
            container(depth).width(95),
            container(polarity).width(140),
            remove_button("Remove modulation", Message::Matrix(Event::Remove(id))),
        ]
        .spacing(3)
        .align_y(iced::Alignment::Center);
        table = table.push(container(line).padding(3).style(if selected {
            container::rounded_box
        } else {
            container::transparent
        }));
    }
    content = content.push(
        scrollable(table)
            .id("matrix-table")
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
    );
    container(content.padding(8)).width(280).height(Length::Fill).into()
}

/// Horizontal offset that keeps the keyboard-selected column in view.
pub(crate) const fn column_offset(column: Column) -> f32 {
    match column {
        Column::On | Column::Source => 0.0,
        Column::Target => 150.0,
        Column::Depth => 290.0,
        Column::Polarity => 420.0,
    }
}

/// Parses a signed percentage and rejects anything outside the edit range.
pub fn parse_depth(input: &str) -> Option<f32> {
    let percent: f32 = input.trim().trim_end_matches('%').trim().parse().ok()?;
    (percent.is_finite() && (-100.0..=100.0).contains(&percent)).then_some(percent / 100.0)
}

impl App {
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
            Event::Sort(column) => self.matrix.sort_by(column),
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
                self.matrix.add = Some(Add::default());
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
            self.status = format!(
                "{} already modulates {}",
                modulation::source_name(self.controller.graph(), source),
                modulation::target_name(self.controller.graph(), module, param)
            );
            return;
        }
        self.close_slider_gesture();
        self.edit(
            |app| match app.controller.retarget_assignment(id, source, module, param) {
                Ok(()) => {
                    app.commit();
                }
                Err(e) => app.status = format!("error: {e}"),
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
            self.status = "Choose a source and a target".to_string();
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
                None => self.status = "Depth must be between -100% and 100%".to_string(),
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
        if self.matrix.add.is_some() {
            let options_sources = sources(self.controller.graph());
            let options_targets = targets(self.controller.graph());
            let add = self.matrix.add.as_mut().expect("open add");
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
mod tests {
    use super::*;
    use crate::ui_test::Ui;
    use iced::{Point, keyboard::key::Named};

    /// Visible rows follow engine order, sorting, text, and module filters
    /// without changing the graph or adding an undo step.
    #[test]
    fn rows_sort_and_filter_without_editing_assignments() {
        let mut ui = Ui::open("modulation.ron");
        let before: Vec<_> = ui
            .app
            .controller
            .graph()
            .assignments()
            .map(|(id, a)| (id, *a))
            .collect();
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        let model = |ui: &Ui| ui.app.matrix.model(ui.app.controller.graph(), &ui.app.selection);
        assert_eq!(model(&ui).rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1, 2]);
        ui.app.matrix_event(Event::Sort(Column::Depth));
        assert_eq!(model(&ui).rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1, 2]);
        ui.app.matrix_event(Event::Sort(Column::Depth));
        assert_eq!(model(&ui).rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![2, 1]);
        for column in Column::ALL {
            ui.app.matrix_event(Event::Sort(column));
            assert_eq!(model(&ui).rows.len(), 2, "{} ascending", column.label());
            ui.app.matrix_event(Event::Sort(column));
            assert_eq!(model(&ui).sort, Some((column, true)), "{} descending", column.label());
        }
        ui.app.matrix_event(Event::Filter("LFO".into()));
        assert_eq!(model(&ui).rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1]);
        ui.app.matrix_event(Event::Filter(String::new()));
        ui.app.matrix_event(Event::SelectedOnly(true));
        ui.app.matrix_event(Event::Select(AssignmentId(1), Column::Source));
        assert_eq!(model(&ui).rows.len(), 2, "both assignments target the selected filter");
        assert_eq!(ui.app.ui_snapshot().matrix.tab, Tab::Matrix);
        assert_eq!(
            before,
            ui.app
                .controller
                .graph()
                .assignments()
                .map(|(id, a)| (id, *a))
                .collect::<Vec<_>>()
        );
        assert!(ui.app.edits.history.undo_step().is_none());
    }

    /// Retargeting keeps the same id and fields, refuses duplicates, and
    /// undo restores the old endpoint in engine order.
    #[test]
    fn retarget_keeps_id_order_and_one_undo_step() {
        let mut ui = Ui::open("modulation.ron");
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        let id = AssignmentId(1);
        let original = *ui.app.controller.graph().assignment(id).expect("first assignment");
        let target = targets(ui.app.controller.graph())
            .into_iter()
            .find(|t| t.module == ModuleId(4) && t.param != original.param)
            .expect("another filter parameter");
        ui.app.matrix_event(Event::Target(id, target));
        let changed = *ui.app.controller.graph().assignment(id).expect("retargeted assignment");
        assert_ne!(changed.param, original.param);
        assert_eq!(changed.depth, original.depth);
        assert_eq!(changed.polarity, original.polarity);
        assert_eq!(changed.enabled, original.enabled);
        assert_eq!(
            ui.app
                .controller
                .graph()
                .assignments()
                .map(|(id, _)| id.0)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        ui.app.undo();
        assert_eq!(ui.app.controller.graph().assignment(id), Some(&original));
        ui.app.redo();
        assert_eq!(ui.app.controller.graph().assignment(id), Some(&changed));
        let duplicate = sources(ui.app.controller.graph())
            .into_iter()
            .find(|s| s.output.module == ModuleId(6))
            .expect("envelope output");
        ui.app.matrix_event(Event::Target(
            id,
            targets(ui.app.controller.graph())
                .into_iter()
                .find(|t| t.module == ModuleId(4) && t.param == original.param)
                .expect("cutoff"),
        ));
        ui.app.matrix_event(Event::Source(id, duplicate));
        assert!(ui.app.status.contains("already modulates"));
        assert_eq!(
            ui.app
                .controller
                .graph()
                .assignment(id)
                .expect("same assignment")
                .source,
            original.source
        );
    }

    /// Typed depth validation leaves a bad edit open and commits an in-range
    /// number as one step; Escape discards an unfinished edit.
    #[test]
    fn typed_depth_and_escape() {
        let mut ui = Ui::open("modulation.ron");
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        let id = AssignmentId(1);
        ui.app.matrix_event(Event::Edit(id, Column::Depth));
        ui.app.matrix_event(Event::DepthText("101".into()));
        ui.app.matrix_event(Event::Apply);
        assert!(ui.app.matrix.edit.is_some());
        assert_eq!(
            ui.app.controller.graph().assignment(id).expect("assignment").depth,
            0.08
        );
        ui.app.matrix_event(Event::DepthText("-35%".into()));
        ui.app.matrix_event(Event::Apply);
        assert_eq!(
            ui.app.controller.graph().assignment(id).expect("assignment").depth,
            -0.35
        );
        ui.app.undo();
        assert_eq!(
            ui.app.controller.graph().assignment(id).expect("assignment").depth,
            0.08
        );
        ui.app.matrix_event(Event::Edit(id, Column::Depth));
        ui.app.matrix_event(Event::DepthText("90".into()));
        ui.app.matrix_event(Event::Cancel);
        assert_eq!(
            ui.app.controller.graph().assignment(id).expect("assignment").depth,
            0.08
        );
    }

    /// Real key events select rows and cells, enter and leave an editor,
    /// delete the row, and let Undo restore it at the same id.
    #[test]
    fn keyboard_path_selects_edits_and_deletes() {
        let mut ui = Ui::open("modulation.ron");
        let tab = ui.text("Matrix").expect("matrix tab").center();
        ui.move_to(Point::new(tab.x, tab.y));
        ui.press();
        ui.release();
        assert_eq!(ui.app.matrix.tab, Tab::Matrix);
        let key = |ui: &mut Ui, named| {
            ui.tap(keyboard::Key::Named(named), keyboard::Modifiers::default());
        };
        key(&mut ui, Named::ArrowDown);
        assert_eq!(ui.app.matrix.selected, Some(AssignmentId(1)));
        key(&mut ui, Named::ArrowRight);
        key(&mut ui, Named::ArrowRight);
        assert_eq!(ui.app.matrix.cell, Column::Target);
        key(&mut ui, Named::Enter);
        assert_eq!(ui.app.matrix.edit.as_ref().map(|e| e.column), Some(Column::Target));
        key(&mut ui, Named::Escape);
        assert!(ui.app.matrix.edit.is_none());
        key(&mut ui, Named::Delete);
        assert!(ui.app.controller.graph().assignment(AssignmentId(1)).is_none());
        ui.app.undo();
        assert!(ui.app.controller.graph().assignment(AssignmentId(1)).is_some());
        key(&mut ui, Named::Tab);
        assert!(!ui.app.matrix.focused);
    }

    /// Matrix focus leaves command shortcuts available after an edit.
    #[test]
    fn command_shortcuts_undo_and_redo_while_matrix_is_focused() {
        let mut ui = Ui::open("modulation.ron");
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        let id = AssignmentId(1);
        let original = ui.app.controller.graph().assignment(id).expect("assignment").enabled;
        ui.app.matrix_event(Event::Enabled(id, !original));
        assert!(ui.app.matrix.focused);

        ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);
        assert_eq!(
            ui.app.controller.graph().assignment(id).expect("assignment").enabled,
            original
        );

        ui.tap(
            keyboard::Key::Character("z".into()),
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
        );
        assert_eq!(
            ui.app.controller.graph().assignment(id).expect("assignment").enabled,
            !original
        );
    }

    /// Captured editor keys stay with the widget except Escape and Tab.
    #[test]
    fn captured_filter_keys_do_not_reach_matrix() {
        let pressed = |named| {
            let key = keyboard::Key::Named(named);
            keyboard::Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: None,
                repeat: false,
            }
        };
        for named in [Named::Delete, Named::ArrowUp, Named::ArrowDown, Named::Enter] {
            assert!(key_message(pressed(named), iced::event::Status::Captured).is_none());
        }
        for named in [Named::Escape, Named::Tab] {
            assert!(key_message(pressed(named), iced::event::Status::Captured).is_some());
        }
        assert!(key_message(pressed(Named::Delete), iced::event::Status::Ignored).is_some());
    }

    /// A canvas click returns keyboard control to the canvas before Delete.
    #[test]
    fn canvas_click_then_delete_removes_the_module_not_a_matrix_row() {
        let mut ui = Ui::open("modulation.ron");
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        ui.app.matrix_event(Event::Select(AssignmentId(1), Column::On));
        let tile = ui.app.scene.module(2).expect("phase tile").bounds();
        let at = ui.canvas().to_window(iced::Point::new(tile.center().x, tile.y + 10.0));
        ui.move_to(at);
        ui.press();
        ui.release();
        assert!(!ui.app.matrix.focused);
        assert!(ui.app.selection.contains(&2));

        ui.tap(keyboard::Key::Named(Named::Delete), keyboard::Modifiers::default());
        assert!(ui.app.controller.graph().node(ModuleId(2)).is_none());
        assert_eq!(ui.app.controller.graph().assignments().count(), 2);
    }

    /// The keyboard can toggle On, choose a new source and polarity, and
    /// create a fresh row through the two Add choices.
    #[test]
    fn keyboard_changes_cells_and_adds_an_assignment() {
        let mut ui = Ui::open("modulation.ron");
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        let key = |ui: &mut Ui, named| {
            ui.tap(keyboard::Key::Named(named), keyboard::Modifiers::default());
        };
        key(&mut ui, Named::ArrowDown);
        let id = AssignmentId(1);
        key(&mut ui, Named::Enter);
        assert!(!ui.app.controller.graph().assignment(id).expect("assignment").enabled);
        ui.app.undo();
        assert!(ui.app.controller.graph().assignment(id).expect("assignment").enabled);
        key(&mut ui, Named::ArrowRight);
        key(&mut ui, Named::Enter);
        let before = ui.app.controller.graph().assignment(id).expect("assignment").source;
        key(&mut ui, Named::ArrowDown);
        key(&mut ui, Named::Enter);
        assert_ne!(
            ui.app.controller.graph().assignment(id).expect("assignment").source,
            before
        );
        key(&mut ui, Named::ArrowRight);
        key(&mut ui, Named::Enter);
        let before_target = ui.app.controller.graph().assignment(id).expect("assignment").param;
        key(&mut ui, Named::ArrowDown);
        key(&mut ui, Named::Enter);
        assert_ne!(
            ui.app.controller.graph().assignment(id).expect("assignment").param,
            before_target
        );
        key(&mut ui, Named::ArrowRight);
        key(&mut ui, Named::ArrowRight);
        key(&mut ui, Named::Enter);
        key(&mut ui, Named::ArrowDown);
        key(&mut ui, Named::Enter);
        assert_eq!(
            ui.app.controller.graph().assignment(id).expect("assignment").polarity,
            Some(loom_core::Polarity::Unipolar)
        );

        ui.app.matrix_event(Event::AddOpen);
        key(&mut ui, Named::ArrowDown);
        key(&mut ui, Named::ArrowRight);
        key(&mut ui, Named::ArrowDown);
        let add = ui.app.matrix.add.as_ref().expect("add picks");
        assert!(add.source.is_some() && add.target.is_some());
        let before_count = ui.app.controller.graph().assignments().count();
        key(&mut ui, Named::Enter);
        assert_eq!(ui.app.controller.graph().assignments().count(), before_count + 1);
        let added = ui
            .app
            .controller
            .graph()
            .assignments()
            .last()
            .expect("new assignment")
            .1;
        let spec = ui
            .app
            .controller
            .graph()
            .node(added.target)
            .expect("target")
            .spec(added.param)
            .expect("parameter");
        assert_eq!(added.depth, modulation::default_depth(&spec));
        ui.app.undo();
        assert_eq!(ui.app.controller.graph().assignments().count(), before_count);
    }

    /// Enter on the Depth cell focuses its text field, so typing a percent
    /// with no pointer commits the value through the widget.
    #[test]
    fn keyboard_types_a_depth_percentage() {
        let mut ui = Ui::open("modulation.ron");
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        let key = |ui: &mut Ui, named| {
            ui.tap(keyboard::Key::Named(named), keyboard::Modifiers::default());
        };
        key(&mut ui, Named::ArrowDown);
        for _ in 0..3 {
            key(&mut ui, Named::ArrowRight);
        }
        assert_eq!(ui.app.matrix.cell, Column::Depth);
        key(&mut ui, Named::Enter);
        assert_eq!(ui.app.matrix.edit.as_ref().map(|e| e.column), Some(Column::Depth));
        ui.typewrite("42");
        assert_eq!(ui.app.matrix.edit.as_ref().expect("depth editor").depth, "42");
        key(&mut ui, Named::Enter);
        assert_eq!(
            ui.app
                .controller
                .graph()
                .assignment(AssignmentId(1))
                .expect("assignment")
                .depth,
            0.42
        );
        assert!(ui.app.matrix.edit.is_none(), "Enter leaves the depth editor");
        ui.app.undo();
        assert_eq!(
            ui.app
                .controller
                .graph()
                .assignment(AssignmentId(1))
                .expect("assignment")
                .depth,
            0.08
        );
    }

    /// Add uses the normal default depth and duplicate status, and a depth
    /// slider drag records one undo step.
    #[test]
    fn add_duplicate_and_drag_follow_inspector_edits() {
        let mut ui = Ui::open("modulation.ron");
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        let existing = *ui
            .app
            .controller
            .graph()
            .assignment(AssignmentId(1))
            .expect("assignment");
        let source = sources(ui.app.controller.graph())
            .into_iter()
            .find(|s| s.output == existing.source)
            .expect("source");
        let target = targets(ui.app.controller.graph())
            .into_iter()
            .find(|t| (t.module, t.param) == (existing.target, existing.param))
            .expect("target");
        ui.app.matrix_event(Event::AddOpen);
        ui.app.matrix_event(Event::AddSource(source));
        ui.app.matrix_event(Event::AddTarget(target));
        ui.app.matrix_event(Event::AddApply);
        assert_eq!(ui.app.controller.graph().assignments().count(), 2);
        assert!(ui.app.status.contains("already modulates"));
        ui.app.matrix_event(Event::DepthDrag(AssignmentId(1), 0.4));
        ui.app.matrix_event(Event::DepthDrag(AssignmentId(1), 0.5));
        ui.app.matrix_event(Event::DepthReleased);
        assert_eq!(
            ui.app
                .controller
                .graph()
                .assignment(AssignmentId(1))
                .expect("assignment")
                .depth,
            0.5
        );
        ui.app.undo();
        assert_eq!(ui.app.controller.graph().assignment(AssignmentId(1)), Some(&existing));
    }

    /// A new patch keeps the session tab but drops patch-specific rows and
    /// selection; controller endpoint validation leaves invalid edits alone.
    #[test]
    fn tab_survives_patch_change_and_bad_endpoint_changes_nothing() {
        let mut ui = Ui::open("modulation.ron");
        ui.app.matrix_event(Event::Tab(Tab::Matrix));
        let id = AssignmentId(1);
        let old = *ui.app.controller.graph().assignment(id).expect("assignment");
        assert!(
            ui.app
                .controller
                .retarget_assignment(
                    id,
                    OutputRef {
                        module: ModuleId(9999),
                        port: 0
                    },
                    old.target,
                    old.param
                )
                .is_err()
        );
        assert_eq!(ui.app.controller.graph().assignment(id), Some(&old));
        ui.app.matrix_event(Event::Select(id, Column::Source));
        assert_eq!(
            ui.app.matrix_line().expect("selected line").from,
            (old.source.module.0, old.source.port)
        );
        assert!(ui.app.replace(crate::Source::Empty));
        assert_eq!(ui.app.matrix.tab, Tab::Matrix);
        assert!(
            ui.app
                .matrix
                .model(ui.app.controller.graph(), &ui.app.selection)
                .rows
                .is_empty()
        );
        assert!(ui.app.matrix.selected.is_none());
    }
}
