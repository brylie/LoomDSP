//! The modulation matrix: a patch-level view of assignments and its
//! session-only sorting, filtering, selection, and edit state.
//!
//! It is drawn by `loom-ui` (`panel.rs`, plan 3.2, row 4e, commit 3): a
//! grid by its keys, the WAI-ARIA data grid pattern, and a list to a screen
//! reader. This file keeps the model both read and what its events do.

use crate::modulation::{self, PolarityChoice};
use crate::{App, Message};
use loom_core::{AssignmentId, Graph, ModuleId, ModuleNames, OutputRef};
use serde::Serialize;
use std::fmt;

pub(crate) mod access;
pub(crate) mod panel;

/// The filter's part, `matrix/filter`.
pub(crate) fn filter_part() -> loom_ui_core::UiId {
    access::part("filter")
}

/// The right-hand panel on show.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    #[default]
    Inspector,
    Matrix,
    /// The level in signal-flow order.
    Outline,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourcePick {
    #[serde(skip)]
    pub output: OutputRef,
    pub label: String,
    /// The visible surface output of an assignment's own row, which Add
    /// cannot offer.
    #[serde(skip)]
    edit_only: bool,
}

impl fmt::Display for SourcePick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// A target in the Add and Target pick lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetPick {
    #[serde(skip)]
    pub module: ModuleId,
    #[serde(skip)]
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
    /// Flat parameter behind a visible surface control, for search and context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_detail: Option<String>,
    pub depth: f32,
    /// The span of this assignment by itself, in the control's units.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    pub polarity: String,
    /// The source's natural polarity, used for the Follow choice.
    #[serde(skip)]
    pub(crate) source_polarity: loom_core::Polarity,
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
    /// The text draft of the open Depth field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<Edit>,
    /// The picks made in the new assignment form, when it is open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub add: Option<Add>,
    /// The available sources while the Add form is open.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourcePick>,
    /// The available targets while the Add form is open.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetPick>,
}

/// A typed depth in the Depth field. Only Enter changes the graph. The
/// source, target, and polarity are the runtime's selects, which apply a
/// choice at once, so the Depth field is the only cell edit left.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Edit {
    #[serde(skip)]
    pub id: AssignmentId,
    pub column: Column,
    pub depth: String,
}

/// The two picks for a new assignment.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Add {
    pub source: Option<SourcePick>,
    pub target: Option<TargetPick>,
}

/// Everything the matrix remembers for this app session.
#[derive(Debug, Default)]
pub struct State {
    pub sort: Option<(Column, bool)>,
    pub filter: String,
    pub selected_only: bool,
    pub selected: Option<AssignmentId>,
    pub cell: Column,
    pub edit: Option<Edit>,
    pub add: Option<Add>,
}

impl State {
    /// Rows in the current graph level, with filters and sorting applied.
    pub fn model(
        &self,
        graph: &Graph,
        hierarchy: &loom_patch::Hierarchy,
        level: &loom_patch::ModulePath,
        selected_modules: &std::collections::BTreeSet<u32>,
        tab: Tab,
    ) -> MatrixModel {
        let names = modulation::Names::new(graph, hierarchy);
        let filter = self.filter.to_lowercase();
        let mut rows: Vec<_> = hierarchy
            .level_assignments(graph, level)
            .filter_map(|(id, a, shown)| {
                let source_id = hierarchy.id_at(level, shown.source.0)?;
                let target_id = hierarchy.id_at(level, shown.target.0)?;
                if self.selected_only
                    && !selected_modules.contains(&source_id.0)
                    && !selected_modules.contains(&target_id.0)
                {
                    return None;
                }
                let source = if source_id == a.source.module {
                    names.source(a.source)
                } else {
                    names.structural_endpoint(level, source_id, &shown.source.1)?
                };
                let inner = names.target(a.target, a.param);
                let surface_control = hierarchy.definition_of(target_id).and_then(|definition| {
                    definition
                        .exposed
                        .iter()
                        .position(|exposed| exposed.name == shown.target.1)
                        .map(|index| (target_id, index))
                });
                let (target, target_detail) = if target_id == a.target {
                    (inner, None)
                } else {
                    (
                        names.structural_endpoint(level, target_id, &shown.target.1)?,
                        Some(inner),
                    )
                };
                if !filter.is_empty()
                    && ![Some(source.as_str()), Some(target.as_str()), target_detail.as_deref()]
                        .into_iter()
                        .flatten()
                        .any(|label| label.to_lowercase().contains(&filter))
                {
                    return None;
                }
                let source_polarity = modulation::source_polarity(graph, a.source);
                Some(MatrixRow {
                    id: id.0,
                    on: a.enabled,
                    source,
                    target,
                    target_detail,
                    depth: a.depth,
                    range: assignment_range(graph, hierarchy, a, surface_control),
                    polarity: PolarityChoice::of(a.polarity, source_polarity).to_string(),
                    source_polarity,
                })
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
        let adding = self.add.is_some();
        MatrixModel {
            tab,
            sort: self.sort,
            filter: self.filter.clone(),
            selected_only: self.selected_only,
            selected: self.selected.map(|id| id.0),
            cell: self.cell,
            editing: self.edit.is_some(),
            rows,
            edit: self.edit.clone(),
            add: self.add.clone(),
            sources: if adding {
                source_picks(graph, hierarchy, &names, level)
            } else {
                Vec::new()
            },
            targets: if adding {
                target_picks(graph, &names, level)
            } else {
                Vec::new()
            },
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

/// The contribution from a single assignment at the current base value.
fn assignment_range(
    graph: &Graph,
    hierarchy: &loom_patch::Hierarchy,
    assign: &loom_core::Assignment,
    surface: Option<(ModuleId, usize)>,
) -> Option<String> {
    let node = graph.node(assign.target)?;
    let mut spec = node.control_spec(assign.param)?;
    let mut base = *node.params.get(assign.param)?;
    if let Some((tile, index)) = surface
        && let Some((min, max)) = hierarchy.exposed_target(tile, index)?.range
    {
        spec = spec.in_range(loom_core::ParamRange::new(min, max));
        base = spec.clamp(base);
    }
    let reach = modulation::reach(graph, assign)?;
    let (low, high) = modulation::range_positions(&spec, base, &[reach]);
    let text = |position| crate::param::format(&spec, spec.denormalize(position));
    Some(if low == high {
        text(low)
    } else {
        format!("{} to {}", text(low), text(high))
    })
}

/// Sources offered by the matrix at this patch level.
pub(crate) fn sources(
    graph: &Graph,
    hierarchy: &loom_patch::Hierarchy,
    level: &loom_patch::ModulePath,
) -> Vec<SourcePick> {
    source_picks(graph, hierarchy, &modulation::Names::new(graph, hierarchy), level)
}

/// Targets offered by the matrix at this patch level.
pub(crate) fn targets(
    graph: &Graph,
    hierarchy: &loom_patch::Hierarchy,
    level: &loom_patch::ModulePath,
) -> Vec<TargetPick> {
    target_picks(graph, &modulation::Names::new(graph, hierarchy), level)
}

/// Visible sources, ordered by the flat output the assignment stores.
fn source_picks(
    graph: &Graph,
    hierarchy: &loom_patch::Hierarchy,
    names: &modulation::Names<'_>,
    level: &loom_patch::ModulePath,
) -> Vec<SourcePick> {
    let mut picks: Vec<_> = modulation::sources(graph)
        .into_iter()
        .filter(|output| names.on_level(output.module, level))
        .map(|output| SourcePick {
            output,
            label: names.source(output),
            edit_only: false,
        })
        .collect();
    for (tile, kind) in hierarchy.structural_tiles(level) {
        if !matches!(kind, loom_patch::Tile::Instance(_)) || hierarchy.slot_of_part(tile).is_some() {
            continue;
        }
        let local = hierarchy.path_of(tile).last();
        for (_, port) in hierarchy.instance_ports(tile).1 {
            let label = names.structural_endpoint(level, tile, &port);
            let Some((module, port)) = hierarchy.source_behind(graph, level, &(local, port)) else {
                continue;
            };
            if let (Some(port), Some(label)) = (graph.node(module).and_then(|node| node.output_index(&port)), label) {
                picks.push(SourcePick {
                    output: OutputRef { module, port },
                    label,
                    edit_only: false,
                });
            }
        }
    }
    picks.sort_by_key(|pick| pick.output);
    // Two surface ports can expose one output. Keep one pick per assignment identity.
    picks.dedup_by_key(|pick| pick.output);
    picks
}

/// Parameters with a knob, toggle, or radio control, in module order.
fn target_picks(graph: &Graph, names: &modulation::Names<'_>, level: &loom_patch::ModulePath) -> Vec<TargetPick> {
    graph
        .nodes()
        .flat_map(|(module, node)| {
            node.descriptor
                .params
                .iter()
                .enumerate()
                .filter_map(move |(param, spec)| {
                    let eligible = !spec.options.transient
                        && !spec.options.shapes_graph
                        && node
                            .descriptor
                            .panel
                            .widgets
                            .iter()
                            .any(|w| w.binds(spec.name) && w.kind.takes_modulation());
                    if !eligible {
                        return None;
                    }
                    let label = if names.on_level(module, level) {
                        names.target(module, param)
                    } else {
                        names.structural_target(level, module, param)?
                    };
                    Some(TargetPick { module, param, label })
                })
        })
        .collect()
}

/// The sources a row's Source select offers, in module and port order:
/// every source of `base`, with the row's own source in its place when Add
/// could not offer it (a sub-patch or slot output), named as the row names
/// it (modulation.md, Editing a source). The select's options and its
/// choices both come from here, so an index always means the same source.
pub(crate) fn row_sources(base: &[SourcePick], own: OutputRef, label: &str) -> Vec<SourcePick> {
    let mut picks = base.to_vec();
    match picks.binary_search_by_key(&own, |pick| pick.output) {
        Ok(at) => label.clone_into(&mut picks[at].label),
        Err(at) => picks.insert(
            at,
            SourcePick {
                output: own,
                label: label.to_string(),
                edit_only: true,
            },
        ),
    }
    picks
}

/// Messages produced by the matrix's parts.
#[derive(Debug, Clone)]
pub enum Event {
    Tab(Tab),
    /// Sort by a column, or `None` for engine order, keeping the direction.
    SortBy(Option<Column>),
    /// Reverse the current sort.
    ReverseSort,
    Filter(String),
    SelectedOnly(bool),
    /// Choose a row and its cell, and move the focus there.
    Select(AssignmentId, Column),
    /// An edit of a row's Depth field: its text as typed.
    DepthText(AssignmentId, String),
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
    /// Enter in the Depth field: apply the typed depth.
    Apply,
    /// Escape in the Depth field, or Cancel: close the field or the Add
    /// form with no change.
    Cancel,
    /// One of the grid's keys on the chosen cell (the WAI-ARIA data grid
    /// pattern).
    Grid(GridKey),
    /// Escape with nothing open: the matrix lets the keys go, back to the
    /// canvas.
    Leave,
    /// After a frame drawn with a new order or filter: scroll the chosen
    /// card, then its cell, into view, now that the layout has them where
    /// they are.
    Reveal,
}

/// What one of the grid's keys does on the chosen cell (modulation.md,
/// Keyboard; the WAI-ARIA data grid pattern).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridKey {
    /// Up: the row above, in the same column.
    Up,
    /// Down: the row below, in the same column.
    Down,
    /// Page Up: ten rows up, stopping at the first.
    PageUp,
    /// Page Down: ten rows down, stopping at the last.
    PageDown,
    /// Ctrl+Home: the first row, in the same column.
    FirstRow,
    /// Ctrl+End: the last row, in the same column.
    LastRow,
    /// Left: the cell before, stopping at the row's first.
    Left,
    /// Right: the cell after, stopping at the row's last.
    Right,
    /// Home: the row's first cell.
    Home,
    /// End: the row's last cell.
    End,
    /// Enter or F2: edit the cell.
    Edit,
    /// Space: toggle On, and nothing on any other cell.
    Space,
    /// Delete: remove the row.
    Delete,
}

/// How far Page Up and Page Down move, in rows.
const PAGE_ROWS: usize = 10;

/// How the Sort by select names a sort: engine order or a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortPick(pub Option<Column>);

impl SortPick {
    /// Engine order, then every column in keyboard order.
    pub(crate) fn all() -> Vec<Self> {
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

/// Parses a signed percentage and rejects anything outside the edit range.
pub fn parse_depth(input: &str) -> Option<f32> {
    let percent: f32 = input.trim().trim_end_matches('%').trim().parse().ok()?;
    (percent.is_finite() && (-100.0..=100.0).contains(&percent)).then_some(percent / 100.0)
}

impl App {
    /// Handles a matrix event, then scrolls the chosen card into view when
    /// the order or the filter changed. The scroll waits for the frame
    /// drawn with the new order: a scroll in this turn would find the card
    /// where the old order had it.
    pub(crate) fn matrix_message(&mut self, event: Event) {
        let reveal = matches!(
            event,
            Event::SortBy(_) | Event::ReverseSort | Event::Filter(_) | Event::SelectedOnly(_)
        );
        self.matrix_event(event);
        if reveal && self.session.matrix.selected.is_some() {
            self.effect(loom_ui::Effects::after_draw(Message::Matrix(Event::Reveal)));
        }
    }

    /// Scrolls the selected assignment's card into view, then its chosen
    /// cell: the card first, so all of it shows where it fits. Nothing
    /// while no card is selected, or while it is filtered out of the view.
    fn reveal_selected(&self) -> loom_ui::Effects<Message> {
        let matrix = &self.session.matrix;
        match matrix.selected {
            Some(id) => loom_ui::Effects::scroll_into_view(access::row(id))
                .and(loom_ui::Effects::scroll_into_view(access::cell(id, matrix.cell))),
            None => loom_ui::Effects::none(),
        }
    }

    /// The rows the matrix shows now, in the order shown.
    pub(crate) fn matrix_model(&self) -> MatrixModel {
        self.session.matrix.model(
            self.session.controller.graph(),
            &self.session.hierarchy,
            &self.session.level,
            &self.session.selection,
            self.right_tab,
        )
    }

    /// Which one matrix assignment the canvas should show without pinning.
    pub(crate) fn matrix_line(&self) -> Option<loom_canvas::ModulationView> {
        if self.right_tab != Tab::Matrix {
            return None;
        }
        let id = self.session.matrix.selected?.0;
        self.session
            .scene
            .modulations()
            .iter()
            .find(|line| line.id == id)
            .cloned()
    }

    /// Handles a matrix part without adding view state to undo history.
    pub(crate) fn matrix_event(&mut self, event: Event) {
        match event {
            Event::Tab(tab) => {
                self.session.outline.close_jump();
                self.session.outline.inspector_action = None;
                self.right_tab = tab;
                // From the tabs' own stop (a key, Enter, or a screen reader
                // on a tab), the focus moves to the tab now open and the
                // content keeps no keys until Tab enters it (the WAI-ARIA
                // Tabs pattern). From the pointer, which focuses no tab,
                // the open content takes the keys, as before.
                let on_tabs = self.focus_on_tabs();
                if on_tabs {
                    self.effect(loom_ui::Effects::focus(crate::tabs::tab_id(tab)));
                } else if self.toolbar_has_focus() {
                    self.effect(loom_ui::Effects::unfocus());
                }
                self.session.outline.active = false;
                self.session.matrix.edit = None;
                self.session.matrix.add = None;
                if tab == Tab::Outline && !on_tabs {
                    self.focus_outline();
                }
                if tab == Tab::Matrix && !on_tabs {
                    self.focus_matrix();
                }
            }
            Event::SortBy(column) => {
                // A new column starts ascending; the same one keeps its
                // direction.
                let reverse = self.session.matrix.sort.is_some_and(|(c, r)| Some(c) == column && r);
                self.session.matrix.sort = column.map(|c| (c, reverse));
            }
            Event::ReverseSort => {
                if let Some((_, reverse)) = &mut self.session.matrix.sort {
                    reverse.toggle();
                }
            }
            // Only the filter changes: the focus stays in the field, and no
            // flag of the grid's is touched (#871).
            Event::Filter(filter) => self.session.matrix.filter = filter,
            Event::SelectedOnly(value) => self.session.matrix.selected_only = value,
            Event::Select(id, column) => self.matrix_choose(id, column),
            // Typing in the field with no edit open, after a click on it,
            // opens one.
            Event::DepthText(id, text) => match &mut self.session.matrix.edit {
                Some(edit) if edit.id == id => edit.depth = text,
                _ => {
                    self.session.matrix.edit = Some(Edit {
                        id,
                        column: Column::Depth,
                        depth: text,
                    })
                }
            },
            Event::DepthReleased => self.handle(Message::SliderReleased),
            Event::Source(id, source) => self.matrix_retarget(id, Some(source.output), None),
            Event::Target(id, target) => self.matrix_retarget(id, None, Some((target.module, target.param))),
            Event::Polarity(id, choice) => {
                self.modulation_message(crate::modulation::Message::Polarity { id, choice });
                self.matrix_choose(id, Column::Polarity);
            }
            Event::Enabled(id, enabled) => {
                self.matrix_choose(id, Column::On);
                self.modulation_message(crate::modulation::Message::Enabled { id, enabled });
            }
            Event::Remove(id) => self.matrix_remove(id),
            Event::AddOpen => {
                // Pressing Add again keeps what the open form has picked.
                self.session.matrix.add.get_or_insert_with(Add::default);
                self.effect(loom_ui::Effects::focus(access::part("new/source")));
            }
            Event::AddSource(source) => {
                if let Some(add) = &mut self.session.matrix.add {
                    add.source = Some(source);
                }
            }
            Event::AddTarget(target) => {
                if let Some(add) = &mut self.session.matrix.add {
                    add.target = Some(target);
                }
            }
            Event::AddApply => self.matrix_add(),
            Event::Apply => self.matrix_apply(),
            Event::Cancel => self.matrix_cancel(),
            Event::Grid(key) => self.matrix_grid_key(key),
            Event::Leave => {
                if self.focus_in_matrix() {
                    self.effect(loom_ui::Effects::unfocus());
                }
            }
            Event::Reveal => {
                let effects = self.reveal_selected();
                self.effect(effects);
            }
        }
    }

    /// Closes the Depth field or the Add form with no change, and puts the
    /// focus back where it came from: the Depth cell, or the Add button.
    fn matrix_cancel(&mut self) {
        // The Depth field's row: the edit's, or the field's that has the
        // focus with no edit open yet, after a press on it.
        let field = self
            .session
            .controller
            .graph()
            .assignments()
            .find_map(|(id, _)| (self.focus.as_ref() == Some(&panel::depth_field(id))).then_some(id));
        if let Some(id) = self.session.matrix.edit.take().map(|edit| edit.id).or(field) {
            self.effect(loom_ui::Effects::focus(access::cell(id, Column::Depth)));
        }
        if self.session.matrix.add.take().is_some() {
            self.effect(loom_ui::Effects::focus(access::part("add")));
        }
    }

    /// Chooses a row's cell, the grid's one stop, and leaves the canvas
    /// selection alone: what entering the grid and a move within a row do.
    fn matrix_choose_cell(&mut self, id: AssignmentId, column: Column) {
        if self.session.controller.graph().assignment(id).is_some() {
            self.session.matrix.selected = Some(id);
            self.session.matrix.cell = column;
        }
    }

    /// Selects a row and its visible target tile, leaving the badge pin
    /// alone. The focus is not moved: see [`Self::matrix_choose`].
    pub(crate) fn matrix_select(&mut self, id: AssignmentId, column: Column) {
        let graph = self.session.controller.graph();
        if let Some(a) = graph.assignment(id) {
            self.session.matrix.selected = Some(id);
            self.session.matrix.cell = column;
            let target = self
                .session
                .scene
                .modulations()
                .iter()
                .find(|line| line.id == id.0)
                .map_or(a.target.0, |line| line.to);
            self.session.selection = std::collections::BTreeSet::from([target]);
        }
    }

    /// Chooses a row's cell and moves the runtime's focus to it, which also
    /// scrolls it into view (the grid's roving focus). The card is scrolled
    /// into view first, so all of it shows where it fits.
    pub(crate) fn matrix_choose(&mut self, id: AssignmentId, column: Column) {
        self.matrix_select(id, column);
        self.matrix_focus_cell(id, column);
    }

    /// Moves the runtime's focus to a row's cell, the card first into view,
    /// unless it is there already.
    fn matrix_focus_cell(&mut self, id: AssignmentId, column: Column) {
        let cell = access::cell(id, column);
        if self.focus.as_ref() != Some(&cell) {
            self.effect(loom_ui::Effects::scroll_into_view(access::row(id)));
            self.effect(loom_ui::Effects::focus(cell));
        }
    }

    /// The cell Tab and Ctrl+Tab enter the grid on: the chosen cell while
    /// its row shows, otherwise On in the first row. `None` with no rows.
    pub(crate) fn matrix_entry(&self) -> Option<loom_ui_core::UiId> {
        let model = self.matrix_model();
        let state = &self.session.matrix;
        let shown = |id: AssignmentId| model.rows.iter().any(|row| row.id == id.0);
        match state.selected.filter(|id| shown(*id)) {
            Some(id) => Some(access::cell(id, state.cell)),
            None => model
                .rows
                .first()
                .map(|row| access::cell(AssignmentId(row.id), Column::On)),
        }
    }

    /// Moves the focus into the matrix, as Ctrl+Tab and a press on its tab
    /// do: its entry cell, or the filter when it has no rows.
    pub(crate) fn focus_matrix(&mut self) {
        let to = self.matrix_entry().unwrap_or_else(filter_part);
        self.effect(loom_ui::Effects::focus(to));
    }

    /// True while the runtime's focus is on one of the matrix's parts.
    pub(crate) fn focus_in_matrix(&self) -> bool {
        self.focus.as_ref().is_some_and(|id| id.is_under(&access::panel()))
    }

    /// Opens a cell's editor: On toggles, and Depth opens its field with
    /// the depth as typed text, all of it selected (the APG's widget in a
    /// cell). The selects in Source, Target, and Polarity open themselves on
    /// Enter and F2, so they never come here.
    fn matrix_begin_edit(&mut self, id: AssignmentId, column: Column) {
        let Some(a) = self.session.controller.graph().assignment(id).copied() else {
            return;
        };
        self.matrix_select(id, column);
        match column {
            Column::On => self.matrix_event(Event::Enabled(id, !a.enabled)),
            Column::Depth => {
                self.session.matrix.edit = Some(Edit {
                    id,
                    column,
                    depth: modulation::depth_text(a.depth),
                });
                self.effect(loom_ui::Effects::focus_and_select_all(panel::depth_field(id)));
            }
            Column::Source | Column::Target | Column::Polarity => {}
        }
    }

    /// One of the grid's keys on the chosen cell. With no row chosen, or
    /// one filtered out, any move chooses On in the first row.
    fn matrix_grid_key(&mut self, key: GridKey) {
        let model = self.matrix_model();
        let rows: Vec<AssignmentId> = model.rows.iter().map(|row| AssignmentId(row.id)).collect();
        let Some(last) = rows.len().checked_sub(1) else {
            return;
        };
        let state = &self.session.matrix;
        let Some(at) = state.selected.and_then(|id| rows.iter().position(|row| *row == id)) else {
            self.matrix_choose(rows[0], Column::On);
            return;
        };
        let column = state.cell;
        let place = Column::ALL.iter().position(|c| *c == column).unwrap_or(0);
        let (row, column) = match key {
            GridKey::Up => (at.saturating_sub(1), column),
            GridKey::Down => ((at + 1).min(last), column),
            GridKey::PageUp => (at.saturating_sub(PAGE_ROWS), column),
            GridKey::PageDown => ((at + PAGE_ROWS).min(last), column),
            GridKey::FirstRow => (0, column),
            GridKey::LastRow => (last, column),
            GridKey::Left => (at, Column::ALL[place.saturating_sub(1)]),
            GridKey::Right => (at, Column::ALL[(place + 1).min(Column::ALL.len() - 1)]),
            GridKey::Home => (at, Column::ALL[0]),
            GridKey::End => (at, Column::ALL[Column::ALL.len() - 1]),
            GridKey::Edit => return self.matrix_begin_edit(rows[at], column),
            GridKey::Space if column == Column::On => return self.matrix_begin_edit(rows[at], column),
            GridKey::Space => return,
            GridKey::Delete => return self.matrix_remove(rows[at]),
        };
        // A move within the row keeps the canvas selection, as entering
        // the grid does: only a move to another row selects its target.
        if row == at {
            self.matrix_choose_cell(rows[row], column);
            self.matrix_focus_cell(rows[row], column);
        } else {
            self.matrix_choose(rows[row], column);
        }
    }

    /// Removes an assignment. While the focus is in the matrix it goes on
    /// to the same cell of the row that takes the removed one's place, or
    /// of the row above at the end, or to Add when no row is left: never
    /// to nothing, which would give the arrows to the canvas (#871).
    fn matrix_remove(&mut self, id: AssignmentId) {
        let before = self.matrix_model();
        let at = before.rows.iter().position(|row| row.id == id.0);
        let focused = self.focus_in_matrix();
        self.modulation_message(crate::modulation::Message::Unassign(id));
        self.session.matrix.reconcile(self.session.controller.graph());
        if !focused {
            return;
        }
        let after = self.matrix_model();
        let next = at.and_then(|at| after.rows.get(at).or_else(|| after.rows.last()));
        match next {
            Some(row) => {
                let column = self.session.matrix.cell;
                self.matrix_choose(AssignmentId(row.id), column);
            }
            None => self.effect(loom_ui::Effects::focus(access::part("add"))),
        }
    }

    /// Applies a source or target change as a single undoable graph edit.
    /// The new pair must be one a drop could make: nothing crosses a
    /// sub-patch boundary except through an exposed parameter (rule 4).
    pub(crate) fn matrix_retarget(
        &mut self,
        id: AssignmentId,
        source: Option<OutputRef>,
        target: Option<(ModuleId, usize)>,
    ) {
        let Some(old) = self.session.controller.graph().assignment(id).copied() else {
            return;
        };
        let source = source.unwrap_or(old.source);
        let (module, param) = target.unwrap_or((old.target, old.param));
        if (source, module, param) == (old.source, old.target, old.param) {
            return;
        }
        if self.refuses_assignment(source, module, param) {
            return;
        }
        if self
            .session
            .controller
            .graph()
            .assignments()
            .any(|(other, a)| other != id && a.source == source && a.target == module && a.param == param)
        {
            let names = modulation::Names::new(self.session.controller.graph(), &self.session.hierarchy);
            self.alerts.warn(format!(
                "{} already modulates {}",
                names.source(source),
                names.target(module, param)
            ));
            return;
        }
        self.close_slider_gesture();
        self.edit(
            |app| match app.session.controller.retarget_assignment(id, source, module, param) {
                Ok(()) => {
                    // The moved assignment takes its place in the file's
                    // order (sub-patches.md, Editor model), which can give
                    // it a new id. A hierarchy that cannot be written puts
                    // the assignment back.
                    if app.sync_hierarchy("retarget") {
                        app.commit();
                    } else {
                        // The sync may have renamed the assignment before it
                        // failed, so the moved one is found by its pair,
                        // which is unique (checked above).
                        let moved = app
                            .session
                            .controller
                            .graph()
                            .assignments()
                            .find(|(_, a)| a.source == source && a.target == module && a.param == param)
                            .map_or(id, |(id, _)| id);
                        let _ = app
                            .session
                            .controller
                            .retarget_assignment(moved, old.source, old.target, old.param);
                    }
                }
                Err(e) => app.alerts.warn(e.to_string()),
            },
        );
        // The moved assignment, under whatever id it has now: the pair is
        // unique, as checked above. The focus follows it, since a new id
        // is a new row.
        let moved = self
            .session
            .controller
            .graph()
            .assignments()
            .find(|(_, a)| a.source == source && a.target == module && a.param == param)
            .map_or(id, |(id, _)| id);
        let column = if source == old.source {
            Column::Target
        } else {
            Column::Source
        };
        self.matrix_choose(moved, column);
    }

    /// Creates the two picked endpoints with the normal app assignment path.
    fn matrix_add(&mut self) {
        let Some(add) = self.session.matrix.add.as_ref() else {
            return;
        };
        let (Some(source), Some(target)) = (&add.source, &add.target) else {
            self.alerts.warn("Choose a source and a target".to_string());
            return;
        };
        let (source, module, param) = (source.output, target.module, target.param);
        self.close_slider_gesture();
        self.edit(|app| app.assign(source, module, param));
        self.session.matrix.add = None;
        self.effect(loom_ui::Effects::focus(access::part("add")));
    }

    /// Commits a typed depth, and puts the focus back on its cell. A depth
    /// out of range stays in the field, with the reason in the status line.
    fn matrix_apply(&mut self) {
        let Some(edit) = self.session.matrix.edit.clone() else {
            return;
        };
        match self.matrix_commit_depth(&edit) {
            Some(()) => {
                self.session.matrix.edit = None;
                self.effect(loom_ui::Effects::focus(access::cell(edit.id, Column::Depth)));
            }
            None => self.alerts.warn("Depth must be between -100% and 100%".to_string()),
        }
    }

    /// Sets the depth `edit` typed, as one undo step, unless the text is
    /// what the field already showed, which is rounded, so nothing changes,
    /// as in the inspector's value field. `None` when the text is not a
    /// depth: nothing changes then either.
    fn matrix_commit_depth(&mut self, edit: &Edit) -> Option<()> {
        let depth = parse_depth(&edit.depth)?;
        let shown = self
            .session
            .controller
            .graph()
            .assignment(edit.id)
            .map(|a| modulation::depth_text(a.depth));
        if shown.as_deref() != Some(edit.depth.trim()) {
            self.modulation_message(crate::modulation::Message::Depth { id: edit.id, depth });
            self.handle(Message::SliderReleased);
        }
        Some(())
    }

    /// Commits the Depth field's draft as the focus leaves it, by Tab, a
    /// press elsewhere, or any other move (Brylie's decision, 2026-11-25,
    /// as the inspector's value field does): as Enter does, when the text
    /// is a depth; otherwise the depth stays as it was, the draft goes, and
    /// the status line says why. It leaves the focus where it went.
    fn matrix_leave_depth_field(&mut self) {
        let Some(edit) = self.session.matrix.edit.take() else {
            return;
        };
        if self.matrix_commit_depth(&edit).is_none() {
            self.alerts
                .warn("The depth was not changed: it must be between -100% and 100%".to_string());
        }
    }

    /// Follows the runtime's focus onto one of the matrix's cells, or a
    /// row's Depth field: that row and cell are chosen, by a press, a
    /// screen reader's Focus, or a key. An open Depth field commits its
    /// draft once the focus leaves it, unless an overlay took the focus,
    /// which is not leaving: the draft waits for the focus to come back.
    pub(crate) fn matrix_focus_moved(&mut self, focus: Option<&loom_ui_core::UiId>) {
        // A row that went while it had the focus (an undo of Add, a level
        // change) leaves the runtime's focus on the panel's first stop, the
        // filter, where it would keep undo and redo from working: it goes
        // to the grid, or to Add, as after a Delete.
        let gone = self.focus.as_ref().is_some_and(|before| {
            before.is_under(&panel::list_id())
                && !self
                    .session
                    .controller
                    .graph()
                    .assignments()
                    .any(|(id, _)| before.is_under(&access::row(id)))
        });
        if gone && focus == Some(&filter_part()) {
            let to = self.matrix_entry().unwrap_or_else(|| access::part("add"));
            self.effect(loom_ui::Effects::focus(to));
        }
        if let Some(edit) = &self.session.matrix.edit
            && focus != Some(&panel::depth_field(edit.id))
            && !self.focus_went_into_an_overlay()
        {
            self.matrix_leave_depth_field();
        }
        let Some(focus) = focus else {
            return;
        };
        let found = self.session.controller.graph().assignments().find_map(|(id, _)| {
            if *focus == panel::depth_field(id) {
                return Some((id, Column::Depth));
            }
            Column::ALL
                .into_iter()
                .find(|column| *focus == access::cell(id, *column))
                .map(|column| (id, column))
        });
        let Some((id, column)) = found else {
            return;
        };
        // Only a move from another row of the grid selects the new row's
        // target on the canvas. Entering the grid by Tab, Ctrl+Tab, or its
        // tab only chooses the cell, so moving the focus past the matrix
        // never changes what is selected (Brylie's decision, 2026-11-27;
        // WCAG 2.2, 3.2.1 On Focus). A move within the row keeps its
        // target.
        let from_row = self.focus.as_ref().and_then(|before| {
            self.session
                .controller
                .graph()
                .assignments()
                .find_map(|(other, _)| before.is_under(&access::row(other)).then_some(other))
        });
        match from_row {
            Some(other) if other != id => self.matrix_select(id, column),
            _ => self.matrix_choose_cell(id, column),
        }
    }
}

#[cfg(test)]
mod tests;
