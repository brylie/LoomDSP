//! The matrix's parts as a screen reader is told them: [`Item`]s built from
//! the same [`MatrixModel`] the view draws (`matrix.rs`), part for part,
//! with the same UI IDs and the same messages (`docs/design/ui-layer.md`,
//! section 7, rule 25).
//!
//! The assignments are a list of rows, not a table. AccessKit's macOS
//! adapter (0.27.2) reports no columns, no row or column counts, no header
//! cells, and no rows for a table, so a screen reader's table navigation
//! would find nothing in one. A list gives each row its place and the
//! list's size, and the view already draws one card per assignment.
//!
//! Each row is keyed by its `AssignmentId`, as in the inspector, under the
//! panel `matrix`, so the two places never share a node.

use super::{Add, Column, Event, MatrixModel, SortPick, SourcePick, TargetPick, sources, targets};
use crate::a11y::items::{Item, Kind, Number, SetNumber, SetText};
use crate::modulation::{self, PolarityChoice};
use crate::{App, Message};
use loom_core::{AssignmentId, Graph, ModuleId, OutputRef};
use loom_ui_core::UiId;

/// The UI ID of the matrix's content. Every part's ID is inside it.
pub(crate) fn panel() -> UiId {
    UiId::panel("matrix")
}

/// The UI ID of one part of the matrix by a fixed name: `filter`,
/// `selected-only`, `add`, `sort`, `reverse`, `assignments`, `new/source`,
/// and so on.
pub(crate) fn part(name: &'static str) -> UiId {
    crate::inspector::ids::part(&panel(), name)
}

/// The UI ID of an assignment's row, by the assignment's own ID:
/// `matrix/assignment/12`.
pub(crate) fn row(id: AssignmentId) -> UiId {
    panel().with("assignment").with(id.0)
}

/// The UI ID of one cell of an assignment's row: `matrix/assignment/12/depth`.
pub(crate) fn cell(id: AssignmentId, column: Column) -> UiId {
    row(id).with(column.key())
}

/// The UI ID of an assignment's remove button.
pub(crate) fn remove(id: AssignmentId) -> UiId {
    row(id).with("remove")
}

impl Column {
    /// The key that names this column's cell in a row's UI ID.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Source => "source",
            Self::Target => "target",
            Self::Depth => "depth",
            Self::Polarity => "polarity",
        }
    }
}

/// The option after or before `at` in a list of `len`, for Increment and
/// Decrement. From no option, Increment takes the first and Decrement
/// the last. `None` at either end: a step does not wrap, as on a tile.
fn next_at(at: Option<usize>, len: usize, forward: bool) -> Option<usize> {
    match (at, forward) {
        (None, true) => (len > 0).then_some(0),
        (None, false) => len.checked_sub(1),
        (Some(at), true) => (at + 1 < len).then_some(at + 1),
        (Some(at), false) => at.checked_sub(1),
    }
}

/// The matrix's sources and targets, made once for one build of its items.
struct Picks {
    /// Every source, in the order the pick list shows them.
    sources: Vec<SourcePick>,
    /// Every target, in the order the pick list shows them.
    targets: Vec<TargetPick>,
}

/// True when another assignment than `id` already joins this pair, which
/// a retarget refuses.
fn taken(graph: &Graph, id: AssignmentId, source: OutputRef, target: ModuleId, param: usize) -> bool {
    graph
        .assignments()
        .any(|(other, a)| other != id && a.source == source && a.target == target && a.param == param)
}

impl App {
    /// Every part the matrix shows, as one named group. Built from the
    /// model the view draws.
    pub(crate) fn matrix_items(&self) -> Vec<Item> {
        let graph = self.session.controller.graph();
        let state = &self.session.matrix;
        let model = state.model(graph, &self.session.selection, self.right_tab);
        // Each pick list once per build, not once per row and direction:
        // each pick carries a name made for it.
        let picks = Picks {
            sources: sources(graph),
            targets: targets(graph),
        };
        let mut parts = vec![
            Item {
                label: Some("Filter modules or parameters".to_string()),
                value: Some(model.filter.clone()),
                set_text: Some(SetText::MatrixFilter),
                focus: Some(crate::places::field_widget_id(&super::filter_part())),
                ..Item::new(part("filter"), Kind::Field)
            },
            Item {
                label: Some("Selected module only".to_string()),
                on: Some(model.selected_only),
                click: Some(Message::Matrix(Event::SelectedOnly(!model.selected_only))),
                ..Item::new(part("selected-only"), Kind::Check)
            },
            Item::button(part("add"), "Add modulation", Some(Message::Matrix(Event::AddOpen))),
            sort(&model),
        ];
        if let Some((_, reverse)) = model.sort {
            parts.push(Item {
                description: Some(if reverse { "Now descending" } else { "Now ascending" }.to_string()),
                ..Item::button(
                    part("reverse"),
                    "Reverse the order",
                    Some(Message::Matrix(Event::ReverseSort)),
                )
            });
        }
        if let Some(add) = &state.add {
            parts.push(new_assignment(&picks, add));
        }
        if model.rows.is_empty() {
            parts.push(Item::text(part("empty"), "No modulation assignments"));
        }
        let rows = model.rows.iter().map(|r| self.matrix_row(graph, &picks, r)).collect();
        parts.push(Item {
            label: Some("Modulation assignments".to_string()),
            children: rows,
            ..Item::new(part("assignments"), Kind::List)
        });
        vec![Item::group(panel(), "Modulation matrix", parts)]
    }

    /// The part the matrix's keys act on, while it has them: the chosen
    /// cell of the chosen row, or the chosen pick of the Add form.
    pub(crate) fn matrix_focus(&self) -> Option<UiId> {
        if !self.matrix_has_keys() {
            return None;
        }
        let state = &self.session.matrix;
        if state.add.is_some() {
            return Some(part(if state.cell == Column::Target {
                "new/target"
            } else {
                "new/source"
            }));
        }
        state.selected.map(|id| cell(id, state.cell))
    }

    /// Before a screen reader acts on a cell of a row, chooses that row and
    /// cell, as a click on the cell does. The keys then act where the
    /// screen reader is, and focus follows the row when a retarget gives
    /// it a new ID. Does nothing for a part that is not a cell.
    pub(crate) fn matrix_reader_select(&mut self, part: &UiId) {
        let found = self.session.controller.graph().assignments().find_map(|(id, _)| {
            Column::ALL
                .into_iter()
                .find(|column| *part == cell(id, *column))
                .map(|column| (id, column))
        });
        if let Some((id, column)) = found {
            self.matrix_select(id, column);
        }
    }

    /// One assignment's row: its switch, its source, target, and polarity
    /// as choices, its depth as a slider in percent, and its remove button.
    fn matrix_row(&self, graph: &Graph, picks: &Picks, r: &super::MatrixRow) -> Item {
        use crate::modulation::Message as Modulation;
        let id = AssignmentId(r.id);
        let assignment = graph.assignment(id);
        let depth = |depth: f32| Message::Modulation(Modulation::Depth { id, depth });
        let mut cells = vec![Item {
            label: Some("On".to_string()),
            on: Some(r.on),
            click: Some(Message::Matrix(Event::Enabled(id, !r.on))),
            ..Item::new(cell(id, Column::On), Kind::Check)
        }];
        cells.push(Item {
            label: Some("Source".to_string()),
            value: Some(r.source.clone()),
            step: (
                self.matrix_source_step(id, true, &picks.sources),
                self.matrix_source_step(id, false, &picks.sources),
            ),
            ..Item::new(cell(id, Column::Source), Kind::Choice)
        });
        cells.push(Item {
            label: Some("Target".to_string()),
            value: Some(r.target.clone()),
            step: (
                self.matrix_target_step(id, true, &picks.targets),
                self.matrix_target_step(id, false, &picks.targets),
            ),
            ..Item::new(cell(id, Column::Target), Kind::Choice)
        });
        let polarity = assignment.map(|a| {
            let own = modulation::source_polarity(graph, a.source);
            let options = PolarityChoice::options(own);
            let at = options.iter().position(|o| *o == PolarityChoice::of(a.polarity, own));
            let choose =
                |forward| next_at(at, options.len(), forward).map(|n| Message::Matrix(Event::Polarity(id, options[n])));
            (choose(true), choose(false))
        });
        cells.push(Item {
            label: Some("Polarity".to_string()),
            value: Some(r.polarity.clone()),
            step: polarity.unwrap_or((None, None)),
            ..Item::new(cell(id, Column::Polarity), Kind::Choice)
        });
        // In percent, as the card says it, so the number is what is heard.
        // One step is the card slider's own step of 1%.
        // One step lands on the percent grid, as the inspector's slider
        // lands on its own grid, so from a depth a drag left between two
        // percents the first step is shorter, and steps never drift.
        let percent = (r.depth * 100.0).round();
        let up = ((percent + 1.0) / 100.0).min(1.0);
        let down = ((percent - 1.0) / 100.0).max(-1.0);
        cells.push(Item {
            label: Some("Depth".to_string()),
            number: Some(Number {
                value: f64::from(percent),
                min: -100.0,
                max: 100.0,
                step: Some(1.0),
            }),
            step: (
                (r.depth < 1.0).then(|| depth(up)),
                (r.depth > -1.0).then(|| depth(down)),
            ),
            set_number: Some(SetNumber::Depth(id)),
            groups: true,
            ..Item::new(cell(id, Column::Depth), Kind::Slider)
        });
        cells.push(Item::button(
            remove(id),
            "Remove modulation",
            Some(Message::Matrix(Event::Remove(id))),
        ));
        Item {
            label: Some(format!("Modulation from {} to {}", r.source, r.target)),
            children: cells,
            ..Item::new(row(id), Kind::ListItem)
        }
    }

    /// The message for one step of assignment `id`'s source, forward or
    /// back through the matrix's sources: the next one the assignment may
    /// take. A source that would be refused (another level, with no
    /// surface between) or that would repeat another assignment's pair is
    /// skipped, so a step never edits nothing and never warns. `None` when
    /// no such source is left that way.
    /// `options` is [`sources`] for the graph now.
    pub(crate) fn matrix_source_step(
        &self,
        id: AssignmentId,
        forward: bool,
        options: &[SourcePick],
    ) -> Option<Message> {
        let graph = self.session.controller.graph();
        let a = graph.assignment(id)?;
        let mut at = options.iter().position(|s| s.output == a.source);
        while let Some(next) = next_at(at, options.len(), forward) {
            let pick = &options[next];
            if self.allows_assignment(pick.output, a.target, a.param)
                && !taken(graph, id, pick.output, a.target, a.param)
            {
                return Some(Message::Matrix(Event::Source(id, pick.clone())));
            }
            at = Some(next);
        }
        None
    }

    /// The message for one step of assignment `id`'s target, forward or
    /// back through the matrix's targets, skipping those it may not take,
    /// as [`matrix_source_step`](Self::matrix_source_step) does.
    /// `options` is [`targets`] for the graph now.
    pub(crate) fn matrix_target_step(
        &self,
        id: AssignmentId,
        forward: bool,
        options: &[TargetPick],
    ) -> Option<Message> {
        let graph = self.session.controller.graph();
        let a = graph.assignment(id)?;
        let mut at = options.iter().position(|t| (t.module, t.param) == (a.target, a.param));
        while let Some(next) = next_at(at, options.len(), forward) {
            let pick = &options[next];
            if self.allows_assignment(a.source, pick.module, pick.param)
                && !taken(graph, id, a.source, pick.module, pick.param)
            {
                return Some(Message::Matrix(Event::Target(id, pick.clone())));
            }
            at = Some(next);
        }
        None
    }
}

/// The Sort by choice: engine order, then each column.
fn sort(model: &MatrixModel) -> Item {
    let options = SortPick::all();
    let now = SortPick(model.sort.map(|(column, _)| column));
    let at = options.iter().position(|o| *o == now);
    let choose = |forward| next_at(at, options.len(), forward).map(|n| Message::Matrix(Event::SortBy(options[n].0)));
    Item {
        label: Some("Sort by".to_string()),
        value: Some(now.to_string()),
        step: (choose(true), choose(false)),
        ..Item::new(part("sort"), Kind::Choice)
    }
}

/// The Add form, while it is open: its two picks and its two buttons.
fn new_assignment(picks: &Picks, add: &Add) -> Item {
    let (sources, targets) = (&picks.sources, &picks.targets);
    let source_at = add.source.as_ref().and_then(|s| sources.iter().position(|o| o == s));
    let target_at = add.target.as_ref().and_then(|t| targets.iter().position(|o| o == t));
    let source = |forward| {
        next_at(source_at, sources.len(), forward).map(|n| Message::Matrix(Event::AddSource(sources[n].clone())))
    };
    let target = |forward| {
        next_at(target_at, targets.len(), forward).map(|n| Message::Matrix(Event::AddTarget(targets[n].clone())))
    };
    Item::group(
        part("new"),
        "New modulation",
        vec![
            Item {
                label: Some("Source output".to_string()),
                value: add.source.as_ref().map(|s| s.label.clone()),
                step: (source(true), source(false)),
                ..Item::new(part("new/source"), Kind::Choice)
            },
            Item {
                label: Some("Target parameter".to_string()),
                value: add.target.as_ref().map(|t| t.label.clone()),
                step: (target(true), target(false)),
                ..Item::new(part("new/target"), Kind::Choice)
            },
            Item::button(part("new/create"), "Create", Some(Message::Matrix(Event::AddApply))),
            Item::button(part("new/cancel"), "Cancel", Some(Message::Matrix(Event::Cancel))),
        ],
    )
}
