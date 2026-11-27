//! The modulation matrix as a panel of `loom-ui`'s Panels layer (plan 3.2,
//! row 4e, commit 3), in the right column's content slot (`chrome.rs`).
//!
//! By its keys it is a grid, the WAI-ARIA data grid pattern: the card list's
//! scroll container takes the grid keys first (ui-layer.md, section 6, rule
//! 6, step 2b) and hands them to the app, which moves the focus from cell
//! to cell (`matrix.rs`, `matrix_grid_key`). Tab meets the grid as one
//! stop, the chosen cell (`matrix_next_stop`). Every cell is focusable, as
//! the pattern's other cells have `tabindex="-1"`: a press or a screen
//! reader's Focus can reach any of them, and the focus that lands there
//! chooses that cell (`matrix_focus_moved`). Each cell is the widget it
//! edits: On a check box, Source, Target, and Polarity the runtime's
//! selects in a cell (`Select::in_cell`), which open on Enter and F2, and
//! Depth the runtime's slider, with its typed value in a field beside it.
//!
//! To a screen reader it is a list (Brylie's decision of 2026-11-11, kept
//! on 2026-11-27): the card list is a List and each card a ListItem, since
//! AccessKit's macOS adapter reports no rows for a grid.
//!
//! Each part keeps the UI ID it had (`access.rs`): the scroll container is
//! keyed `assignment`, each card by its assignment, and each cell by its
//! column, so a cell is `matrix/assignment/<id>/<column>`.

use super::{Column, Event, GridKey, SortPick, SourcePick, TargetPick, access, row_sources, sources, targets};
use crate::modulation::{self, PolarityChoice};
use crate::style::{self, Role as ButtonRole};
use crate::{App, Message};
use loom_core::AssignmentId;
use loom_ui::access::{Access, Action, Role, Value};
use loom_ui::{
    Align, Drag, Element, FieldEvent, Layer, Length, Padding, Select, SelectStyle, SlidePhase, Slider, View,
};
use loom_ui_core::{Color, Key, KeyEvent, UiId};

/// The keys the card list takes before the cell inside it: the grid's
/// moves, Enter and F2 to edit a cell, Space, which toggles On and does
/// nothing elsewhere, so it never pages the list, Delete to remove a row,
/// and Escape to let the grid go (the WAI-ARIA data grid pattern).
const GRID_KEYS: [Key; 13] = [
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::Enter,
    Key::F2,
    Key::Space,
    Key::Delete,
    Key::Escape,
];

/// The key each of the matrix's other parts takes first: Escape closes the
/// Add form, or gives the keys back to the canvas.
const ESCAPE: [Key; 1] = [Key::Escape];

/// The width kept for a card's depth field, such as "-100%".
const DEPTH_TEXT: f32 = 52.0;

/// The UI ID of the matrix's panel.
pub(crate) fn matrix_id() -> UiId {
    access::panel()
}

/// The UI ID of the card list, the grid's scroll container.
pub(crate) fn list_id() -> UiId {
    access::part("assignment")
}

/// The UI ID of a row's Depth field, beside its Depth slider.
pub(crate) fn depth_field(id: AssignmentId) -> UiId {
    access::row(id).with("depth_value")
}

/// What the panel is drawn from, so the runtime builds it again only when
/// this changes.
#[derive(Debug, Clone, PartialEq)]
struct Panel {
    /// The filter's text.
    filter: String,
    /// Whether Selected module only is on.
    selected_only: bool,
    /// The sort, and whether it is reversed.
    sort: Option<(Column, bool)>,
    /// The Add form, while it is open.
    add: Option<AddForm>,
    /// One card per row shown, in the order shown.
    cards: Vec<Card>,
    /// Every target's label, for the Target selects and the Add form.
    targets: Vec<String>,
    /// The chosen row.
    selected: Option<u32>,
    /// The Depth field's draft, while it is open: its row and its text.
    edit: Option<(u32, String)>,
    /// The colours and looks it is painted in.
    looks: Looks,
}

/// The Add form's two picks.
#[derive(Debug, Clone, PartialEq)]
struct AddForm {
    /// Every source Add offers.
    sources: Vec<String>,
    /// The source picked, if any.
    source: Option<usize>,
    /// The target picked, if any.
    target: Option<usize>,
}

/// One assignment's card.
#[derive(Debug, Clone, PartialEq)]
struct Card {
    /// The row as the snapshot reports it.
    row: super::MatrixRow,
    /// The Source select's options ([`row_sources`]).
    sources: Vec<String>,
    /// The row's own source among them.
    source: Option<usize>,
    /// The row's own target among the panel's targets.
    target: Option<usize>,
    /// The Polarity select's options.
    polarities: Vec<String>,
    /// The row's polarity among them.
    polarity: Option<usize>,
}

/// The colours the matrix is painted in, from the style tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Looks {
    text: Color,
    quiet: Color,
    field: style::FieldLook,
    select: SelectStyle,
    cell: SelectStyle,
    card_fill: Color,
    card_border: Color,
    chosen: Color,
}

impl App {
    /// Whether the runtime draws the matrix now: its tab is open and no
    /// jump list covers it.
    pub(crate) fn matrix_ours(&self) -> bool {
        self.right_tab == super::Tab::Matrix && !self.session.outline.has_jump()
    }

    /// Whether the matrix's panel owns the input: it is drawn, and nothing
    /// is open over the canvas.
    pub(crate) fn matrix_owns(&self) -> bool {
        self.matrix_ours() && self.input_owner() == crate::input_owner::InputOwner::Canvas
    }

    /// Adds the matrix's panel to the runtime's view, while it shows.
    pub(crate) fn matrix_panel(&self, view: &mut View) {
        if !self.matrix_ours() {
            return;
        }
        view.panel_with(matrix_id(), Layer::Panels, &self.matrix_panel_model(), build);
    }

    /// What the matrix's panel is built from: its rows, the options of
    /// their selects, and the colours of the style tokens.
    fn matrix_panel_model(&self) -> Panel {
        let mut small = crate::style::select();
        small.text = style::SMALL_TEXT;
        small.padding_y = 1.0;
        small.padding_x = 4.0;
        let mut cell = small;
        cell.background = Color::TRANSPARENT;
        cell.border = Color::TRANSPARENT;
        cell.mark = crate::style::QUIET;
        let looks = Looks {
            text: crate::style::TEXT,
            quiet: crate::style::QUIET,
            field: crate::style::FIELD,
            select: small,
            cell,
            card_fill: crate::style::CARD_FILL,
            card_border: crate::style::CARD_BORDER,
            chosen: crate::style::ACCENT,
        };
        let graph = self.session.controller.graph();
        let model = self.matrix_model();
        let base = sources(graph, &self.session.hierarchy, &self.session.level);
        let all_targets = targets(graph, &self.session.hierarchy, &self.session.level);
        let cards = model
            .rows
            .iter()
            .filter_map(|row| {
                let a = graph.assignment(AssignmentId(row.id))?;
                let picks = row_sources(&base, a.source, &row.source);
                let own = row.source_polarity;
                let polarities = PolarityChoice::options(own);
                let now = PolarityChoice::of(a.polarity, own);
                Some(Card {
                    row: row.clone(),
                    source: picks.iter().position(|pick| pick.output == a.source),
                    sources: picks.into_iter().map(|pick| pick.label).collect(),
                    target: all_targets
                        .iter()
                        .position(|t| (t.module, t.param) == (a.target, a.param)),
                    polarity: polarities.iter().position(|choice| *choice == now),
                    polarities: polarities.iter().map(ToString::to_string).collect(),
                })
            })
            .collect();
        let add = model.add.as_ref().map(|add| AddForm {
            source: add.source.as_ref().and_then(|s| base.iter().position(|o| o == s)),
            target: add
                .target
                .as_ref()
                .and_then(|t| all_targets.iter().position(|o| o == t)),
            sources: base.iter().map(|pick| pick.label.clone()).collect(),
        });
        Panel {
            filter: model.filter.clone(),
            selected_only: model.selected_only,
            sort: model.sort,
            add,
            cards,
            targets: all_targets.into_iter().map(|t| t.label).collect(),
            selected: model.selected,
            edit: model.edit.as_ref().map(|edit| (edit.id.0, edit.depth.clone())),
            looks,
        }
    }

    /// The matrix's stops as Tab meets them, while its panel owns the
    /// input: the filter, Selected module only, Add, Sort by, the reverse
    /// button while sorted, the Add form's parts while it is open, then the
    /// grid as one stop, its entry cell ([`Self::matrix_entry`]).
    pub(crate) fn matrix_stops(&self) -> Vec<UiId> {
        if !self.matrix_owns() {
            return Vec::new();
        }
        let state = &self.session.matrix;
        let mut stops = vec![
            access::part("filter"),
            access::part("selected-only"),
            access::part("add"),
            access::part("sort"),
        ];
        if state.sort.is_some() {
            stops.push(access::part("reverse"));
        }
        if state.add.is_some() {
            stops.extend(["new/source", "new/target", "new/create", "new/cancel"].map(access::part));
        }
        stops.extend(self.matrix_entry());
        stops
    }

    /// Every cell and Depth field of the rows shown, while the panel owns
    /// the input: each stands for the grid's one stop, and a screen
    /// reader's Focus can reach any of them.
    pub(crate) fn matrix_cells(&self) -> Vec<UiId> {
        if !self.matrix_owns() {
            return Vec::new();
        }
        self.matrix_model()
            .rows
            .iter()
            .flat_map(|row| {
                let id = AssignmentId(row.id);
                Column::ALL
                    .into_iter()
                    .map(move |column| access::cell(id, column))
                    .chain(std::iter::once(depth_field(id)))
            })
            .collect()
    }

    /// Where Tab goes from `from` in the matrix, or the one before when
    /// `back` (ui-layer.md, section 6, rule 3): the next stop, with the grid
    /// as one stop, so any cell or Depth field stands for it; past the
    /// last, the bar's first stop, or the top run's first; before the
    /// first, the tab list. `None` when `from` is no part of the matrix.
    pub(crate) fn matrix_next_stop(&self, from: &UiId, back: bool) -> Option<loom_ui::TabStep<Message>> {
        if !self.matrix_owns() || !from.is_under(&matrix_id()) {
            return None;
        }
        let stops = self.matrix_stops();
        let at = if from.is_under(&list_id()) {
            stops.len().checked_sub(1)?
        } else {
            stops.iter().position(|stop| stop == from)?
        };
        let to = if back { at.checked_sub(1) } else { Some(at + 1) };
        let past = || {
            let (top, bar) = (self.top_stops(), self.bar_stops());
            if back {
                self.tabs_stops().first().or(top.last()).or(bar.last()).cloned()
            } else {
                bar.first().or(top.first()).cloned()
            }
        };
        let stop = to.and_then(|to| stops.get(to)).cloned().or_else(past);
        Some(stop.map_or(loom_ui::TabStep::Leave, loom_ui::TabStep::To))
    }

    /// The message for a click on one of the matrix's parts, by the
    /// pointer, a key, or a screen reader: a card chooses its row, On
    /// toggles, × removes, and the buttons do what they say.
    pub(crate) fn matrix_click(&self, id: &UiId) -> Option<Message> {
        if !self.matrix_owns() || !id.is_under(&matrix_id()) {
            return None;
        }
        let state = &self.session.matrix;
        let event = if *id == access::part("selected-only") {
            Event::SelectedOnly(!state.selected_only)
        } else if *id == access::part("add") {
            Event::AddOpen
        } else if *id == access::part("reverse") {
            Event::ReverseSort
        } else if *id == access::part("new/create") {
            Event::AddApply
        } else if *id == access::part("new/cancel") {
            Event::Cancel
        } else {
            let graph = self.session.controller.graph();
            let (assignment, a) = graph
                .assignments()
                .find(|(assignment, _)| id.is_under(&access::row(*assignment)) || *id == access::row(*assignment))?;
            if *id == access::cell(assignment, Column::On) {
                Event::Enabled(assignment, !a.enabled)
            } else if *id == access::remove(assignment) {
                Event::Remove(assignment)
            } else if *id == access::row(assignment) {
                Event::Select(assignment, state.cell)
            } else {
                // A cell whose press is a click: a select or a slider that
                // took the press opens or drags instead, and focuses it.
                let column = Column::ALL.into_iter().find(|c| *id == access::cell(assignment, *c))?;
                Event::Select(assignment, column)
            }
        };
        Some(Message::Matrix(event))
    }

    /// The message for one of the keys the matrix takes first: the grid's
    /// keys on a cell, Escape on its other parts. In the Depth field, only
    /// Escape, which closes it: every other key is the field's, so Delete
    /// and the arrows edit the text and never remove or leave the row.
    pub(crate) fn matrix_panel_key(&self, id: &UiId, key: &KeyEvent) -> Option<Message> {
        if !self.matrix_owns() || !id.is_under(&matrix_id()) {
            return None;
        }
        let KeyEvent::Pressed { key, modifiers, .. } = key else {
            return None;
        };
        let in_field = self.focus.as_ref().is_some_and(|focus| {
            self.session
                .controller
                .graph()
                .assignments()
                .any(|(assignment, _)| *focus == depth_field(assignment))
        });
        let plain = *modifiers == loom_ui_core::Modifiers::NONE;
        if *key == Key::Escape {
            if !plain {
                return None;
            }
            let open = in_field || self.session.matrix.edit.is_some() || self.session.matrix.add.is_some();
            return Some(Message::Matrix(if open { Event::Cancel } else { Event::Leave }));
        }
        if in_field || *id != list_id() {
            return None;
        }
        // Ctrl+Home and Ctrl+End, Cmd on macOS: the platform's shortcut.
        let shortcut = modifiers.shortcut() && !modifiers.shift && !modifiers.alt;
        let grid = match key {
            Key::Home if shortcut => GridKey::FirstRow,
            Key::End if shortcut => GridKey::LastRow,
            _ if !plain => return None,
            Key::Up => GridKey::Up,
            Key::Down => GridKey::Down,
            Key::Left => GridKey::Left,
            Key::Right => GridKey::Right,
            Key::Home => GridKey::Home,
            Key::End => GridKey::End,
            Key::PageUp => GridKey::PageUp,
            Key::PageDown => GridKey::PageDown,
            Key::Enter | Key::F2 => GridKey::Edit,
            Key::Space => GridKey::Space,
            Key::Delete => GridKey::Delete,
            _ => return None,
        };
        Some(Message::Matrix(Event::Grid(grid)))
    }

    /// The message for option `index` chosen in one of the matrix's
    /// selects: Sort by, a row's source, target, or polarity, or the Add
    /// form's picks. Each index means the option the select was built
    /// with, from the same lists.
    pub(crate) fn matrix_chose(&self, id: &UiId, index: usize) -> Option<Message> {
        if !self.matrix_owns() || !id.is_under(&matrix_id()) {
            return None;
        }
        let graph = self.session.controller.graph();
        let (hierarchy, level) = (&self.session.hierarchy, &self.session.level);
        let event = if *id == access::part("sort") {
            Event::SortBy(SortPick::all().get(index)?.0)
        } else if *id == access::part("new/source") {
            Event::AddSource(sources(graph, hierarchy, level).get(index)?.clone())
        } else if *id == access::part("new/target") {
            Event::AddTarget(targets(graph, hierarchy, level).get(index)?.clone())
        } else {
            let (assignment, a) = graph
                .assignments()
                .find(|(assignment, _)| id.is_under(&access::row(*assignment)))?;
            if *id == access::cell(assignment, Column::Source) {
                let model = self.matrix_model();
                let row = model.rows.iter().find(|row| row.id == assignment.0)?;
                let picks: Vec<SourcePick> = row_sources(&sources(graph, hierarchy, level), a.source, &row.source);
                Event::Source(assignment, picks.get(index)?.clone())
            } else if *id == access::cell(assignment, Column::Target) {
                let picks: Vec<TargetPick> = targets(graph, hierarchy, level);
                Event::Target(assignment, picks.get(index)?.clone())
            } else if *id == access::cell(assignment, Column::Polarity) {
                let own = modulation::source_polarity(graph, a.source);
                Event::Polarity(assignment, *PolarityChoice::options(own).get(index)?)
            } else {
                return None;
            }
        };
        Some(Message::Matrix(event))
    }

    /// The message for a row's Depth slider in `phase` of its gesture: a
    /// move sets the depth on the percent grid, and the end closes the undo
    /// step, as the inspector's card does.
    pub(crate) fn matrix_slid(&self, id: &UiId, position: f32, phase: SlidePhase) -> Option<Message> {
        if !self.matrix_owns() || !id.is_under(&list_id()) {
            return None;
        }
        let (assignment, _) = self
            .session
            .controller
            .graph()
            .assignments()
            .find(|(assignment, _)| *id == access::cell(*assignment, Column::Depth))?;
        match phase {
            SlidePhase::Start => None,
            // On the percent grid, as the inspector's card steps it.
            SlidePhase::Move => Some(Message::Inspector(crate::inspector::Message::DepthSlid {
                id: assignment,
                depth: position.mul_add(2.0, -1.0).clamp(-1.0, 1.0),
            })),
            SlidePhase::End => Some(Message::Matrix(Event::DepthReleased)),
        }
    }

    /// What a screen reader's SetValue on a row's Depth slider sets: the
    /// depth in percent, joining the screen reader's undo group, as a step
    /// does. `None` for any other part.
    pub(crate) fn matrix_depth_item(&self, id: &UiId) -> Option<crate::a11y::items::Item> {
        use crate::a11y::items::{Item, Kind, SetNumber};
        let (assignment, _) = self
            .session
            .controller
            .graph()
            .assignments()
            .find(|(assignment, _)| *id == access::cell(*assignment, Column::Depth))?;
        Some(Item {
            set_number: Some(SetNumber::Depth(assignment)),
            groups: true,
            ..Item::new(id.clone(), Kind::Slider)
        })
    }

    /// The message for an event on one of the matrix's fields: the filter,
    /// which filters as it is typed in, and a row's Depth field, whose
    /// Enter applies what is typed.
    pub(crate) fn matrix_field(&self, id: &UiId, event: &FieldEvent) -> Option<Message> {
        if *id == access::part("filter") {
            return match event {
                FieldEvent::Edited(text) => Some(Message::Matrix(Event::Filter(text.clone()))),
                _ => None,
            };
        }
        if !id.is_under(&list_id()) {
            return None;
        }
        let (assignment, _) = self
            .session
            .controller
            .graph()
            .assignments()
            .find(|(assignment, _)| *id == depth_field(*assignment))?;
        match event {
            FieldEvent::Edited(text) => Some(Message::Matrix(Event::DepthText(assignment, text.clone()))),
            FieldEvent::Committed(_) => Some(Message::Matrix(Event::Apply)),
            _ => None,
        }
    }
}

/// The panel: the filter, Selected module only and Add, Sort by, the Add
/// form while it is open, then the cards in their scroll container.
fn build(panel: &Panel) -> Element {
    let looks = panel.looks;
    let mut parts = vec![
        Element::field(panel.filter.clone(), style::CONTROL_TEXT)
            .placeholder("Filter modules or parameters", looks.quiet)
            .padding(style::FIELD_PADDING)
            .background(looks.field.fill)
            .border(style::RULE, looks.field.border)
            .radius(looks.field.radius)
            .width(Length::FILL)
            .key("filter")
            .takes_first(ESCAPE)
            .access(Access::new(Role::TextInput, "Filter modules or parameters")),
        Element::row([
            Element::checkbox("Selected module only", style::CONTROL_TEXT, panel.selected_only)
                .key("selected-only")
                .takes_first(ESCAPE),
            Element::space(Length::FILL, Length::Shrink),
            button("add", "Add", "Add modulation", ButtonRole::Secondary),
        ])
        .align_y(Align::Center)
        .width(Length::FILL),
        sort_row(panel),
    ];
    if let Some(add) = &panel.add {
        parts.push(add_form(add, &panel.targets, looks));
    }
    if panel.cards.is_empty() {
        let words = "No modulation assignments";
        parts.push(
            Element::text(words, style::BODY_TEXT)
                .colour(looks.quiet)
                .key("empty")
                .access(Access::label(words)),
        );
        parts.push(Element::space(Length::FILL, Length::FILL));
    } else {
        let count = panel.cards.len();
        let cards = panel
            .cards
            .iter()
            .enumerate()
            .map(|(at, card)| one_card(panel, card, at, count));
        parts.push(
            Element::scroll(Element::column(cards).spacing(6.0).width(Length::FILL), style::scroll())
                .key("assignment")
                .takes_first(GRID_KEYS)
                .access(Access::new(Role::List, "Modulation assignments").size_of_set(count))
                .width(Length::FILL)
                .height(Length::FILL)
                .min_height(0.0),
        );
    }
    Element::column(parts)
        .spacing(7.0)
        .padding(8.0)
        .width(Length::FILL)
        .height(Length::FILL)
}

/// Sort by and its select, and the reverse button while sorted.
fn sort_row(panel: &Panel) -> Element {
    let looks = panel.looks;
    let options = SortPick::all();
    let now = SortPick(panel.sort.map(|(column, _)| column));
    let chosen = options.iter().position(|o| *o == now);
    let names = options.iter().map(ToString::to_string).collect();
    let mut line = vec![
        Element::text("Sort by", style::SMALL_TEXT).colour(looks.quiet),
        Element::select(Select::new("Sort by", names, chosen, looks.select))
            .width(Length::Shrink)
            .key("sort")
            .takes_first(ESCAPE),
    ];
    if let Some((_, reverse)) = panel.sort {
        let now = if reverse { "Now descending" } else { "Now ascending" };
        line.push(
            Element::text(if reverse { "↓" } else { "↑" }, style::CONTROL_TEXT)
                .padding(Padding::symmetric(0.0, 6.0))
                .looks(ButtonRole::Plain.looks())
                .hover_text("Reverse the order")
                .key("reverse")
                .focusable()
                .takes_first(ESCAPE)
                .access(
                    Access::new(Role::Button, "Reverse the order")
                        .description(now)
                        .action(Action::Click),
                ),
        );
    }
    Element::row(line).spacing(6.0).align_y(Align::Center)
}

/// The Add form: its source and target picks, Create, and Cancel
/// (`matrix/new/…`).
fn add_form(add: &AddForm, targets: &[String], looks: Looks) -> Element {
    let pick = |label: &'static str, name: &'static str, key: &'static str, options: Vec<String>, at| {
        Element::row([
            Element::text(label, style::SMALL_TEXT).colour(looks.quiet),
            Element::select(Select::new(name, options, at, looks.select))
                .width(Length::FILL)
                .key(key)
                .takes_first(ESCAPE),
        ])
        .spacing(6.0)
        .align_y(Align::Center)
        .width(Length::FILL)
    };
    Element::column([
        pick("Source", "Source output", "source", add.sources.clone(), add.source),
        pick("Target", "Target parameter", "target", targets.to_vec(), add.target),
        Element::row([
            button("create", "Create", "Create", ButtonRole::Primary),
            button("cancel", "Cancel", "Cancel", ButtonRole::Secondary),
        ])
        .spacing(5.0),
    ])
    .spacing(5.0)
    .width(Length::FILL)
    .key("new")
    .access(Access::new(Role::Group, "New modulation"))
}

/// One assignment's card, keyed by its assignment, at place `at` of
/// `count`: On, the source, and ×; an arrow, the target, and the polarity;
/// the depth slider and its field; and the span it adds.
fn one_card(panel: &Panel, card: &Card, at: usize, count: usize) -> Element {
    let looks = panel.looks;
    let row = &card.row;
    let chosen = panel.selected == Some(row.id);
    let cell = |name: &'static str, options: Vec<String>, at: Option<usize>| {
        Element::select(Select::new(name, options, at, looks.cell).in_cell())
    };
    // Source and Target take the room left and may narrow below their
    // label, which they cut, as Iced's pick lists did: a long name, or a
    // wider system font, never pushes Polarity or × past the column.
    let narrowing = |element: Element| element.width(Length::FILL).min_width(0.0).clip();
    let mut target = narrowing(cell("Target", panel.targets.clone(), card.target)).key("target");
    if let Some(detail) = &row.target_detail {
        target = target.hover_text(detail.clone());
    }
    let depth_text = match &panel.edit {
        Some((id, draft)) if *id == row.id => draft.clone(),
        _ => modulation::depth_text(row.depth),
    };
    let mut depth = Access::new(Role::Slider, "Depth")
        .value(Value::Number {
            value: f64::from((row.depth * 100.0).round()),
            min: -100.0,
            max: 100.0,
            step: Some(1.0),
        })
        .action(Action::Increment)
        .action(Action::Decrement)
        .action(Action::SetValue);
    if let Some(range) = &row.range {
        depth = depth.description(range.clone());
    }
    let mut lines = vec![
        Element::row([
            Element::checkbox("On", style::SMALL_TEXT, row.on).key("on"),
            narrowing(cell("Source", card.sources.clone(), card.source)).key("source"),
            Element::text("×", style::CONTROL_TEXT)
                .padding(Padding::symmetric(0.0, 6.0))
                .looks(ButtonRole::Plain.looks())
                .hover_text("Remove modulation")
                .key("remove")
                .access(Access::new(Role::Button, "Remove modulation").action(Action::Click)),
        ])
        .spacing(4.0)
        .align_y(Align::Center)
        .width(Length::FILL),
        Element::row([
            Element::text("→", style::SMALL_TEXT).colour(looks.quiet),
            target,
            cell("Polarity", card.polarities.clone(), card.polarity)
                .width(Length::Shrink)
                .key("polarity"),
        ])
        .spacing(4.0)
        .padding(Padding::symmetric(0.0, 4.0))
        .align_y(Align::Center)
        .width(Length::FILL),
        Element::row([
            // One percent a drag step, as Iced's slider stepped.
            Element::slider(
                Slider::new((row.depth + 1.0) / 2.0)
                    .centred()
                    .key_step(0.005)
                    .fine_key_step(0.0005)
                    .drag(Drag::Step(0.005)),
            )
            .key("depth")
            .access(depth),
            Element::field(depth_text, style::SMALL_TEXT)
                .padding(Padding::all(1.0))
                .background(looks.field.fill)
                .border(style::RULE, looks.field.border)
                .radius(looks.field.radius)
                .width(Length::Fixed(DEPTH_TEXT))
                .key("depth_value")
                .access(Access::new(Role::TextInput, "Depth in percent")),
        ])
        .spacing(6.0)
        .padding(Padding::symmetric(0.0, 4.0))
        .align_y(Align::Center)
        .width(Length::FILL),
    ];
    if let Some(range) = &row.range {
        lines.push(
            Element::text(range.clone(), style::SMALL_TEXT)
                .colour(looks.quiet)
                .padding(Padding::symmetric(0.0, 4.0)),
        );
    }
    let name = format!("Modulation from {} to {}", row.source, row.target);
    Element::column(lines)
        .spacing(2.0)
        .padding(6.0)
        .width(Length::FILL)
        .background(looks.card_fill)
        .border(style::RULE, if chosen { looks.chosen } else { looks.card_border })
        .radius(style::CARD_RADIUS)
        .key(row.id)
        .access(Access::new(Role::ListItem, name).position_in_set(at).size_of_set(count))
}

/// A button in `role`, keyed `key`, showing `label`, named `name`.
fn button(key: &'static str, label: &'static str, name: &'static str, role: ButtonRole) -> Element {
    Element::text(label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(key)
        .focusable()
        .takes_first(ESCAPE)
        .access(Access::new(Role::Button, name).action(Action::Click))
}

#[cfg(test)]
pub(crate) mod tests;
