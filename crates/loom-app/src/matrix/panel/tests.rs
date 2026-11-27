//! Tests for the matrix's panel (plan 3.2, row 4e, commit 3): the grid's
//! one stop and its keys, editing each kind of cell, the selects and the
//! Depth field, a press on a card, and what a screen reader meets.

use super::*;
use crate::matrix::{Column, Event, Tab};
use crate::ui_test::Ui;
use iced::keyboard::{self, Key as IcedKey, key::Named};

/// The modulation example with the Matrix open from its tab by the
/// pointer, so the grid has the focus on On in the first row.
pub(crate) fn matrix_open() -> Ui {
    let mut ui = Ui::open("modulation.ron");
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    let _ = ui.redraw();
    ui
}

/// Presses `named` with no modifiers, and draws the frame.
pub(crate) fn key(ui: &mut Ui, named: Named) {
    key_with(ui, named, keyboard::Modifiers::empty());
}

/// Presses `named` with `modifiers`, and draws the frame.
pub(crate) fn key_with(ui: &mut Ui, named: Named, modifiers: keyboard::Modifiers) {
    ui.tap(IcedKey::Named(named), modifiers);
    let _ = ui.redraw();
}

/// The runtime's focus.
pub(crate) fn focused(ui: &Ui) -> Option<UiId> {
    ui.runtime().focused().cloned()
}

/// The UI ID of assignment `id`'s cell in `column`.
fn cell(id: u32, column: Column) -> UiId {
    access::cell(AssignmentId(id), column)
}

/// True while the select `id`'s list is open, as its node says.
fn open(ui: &mut Ui, id: &UiId) -> bool {
    ui.a11y_tree().node_by_id(id).expanded() == Some(true)
}

/// Assignment 1 as it is now.
fn first(ui: &Ui) -> loom_core::Assignment {
    *ui.app().session.controller.graph().assignment(AssignmentId(1)).unwrap()
}

/// Presses the middle of the part `id`.
fn click(ui: &mut Ui, id: &UiId) {
    ui.lay_out_panels();
    let at = ui.runtime().bounds(id).unwrap_or_else(|| panic!("{id} is laid out"));
    let _ = ui.move_to(loom_ui_core::Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0));
    let _ = ui.press();
    let _ = ui.release();
    let _ = ui.redraw();
}

/// The modulation example with 30 more assignments, 32 rows in all.
fn many_rows() -> Ui {
    let mut ui = Ui::open("modulation.ron");
    let a = first(&ui);
    for index in 0..30 {
        ui.app_mut()
            .session
            .controller
            .add_assignment(a.source, a.target, a.param, -0.9 + index as f32 * 0.03)
            .unwrap();
    }
    assert!(ui.app_mut().commit());
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    let _ = ui.redraw();
    ui
}

/// The grid is one Tab stop (the WAI-ARIA data grid pattern): Tab from the
/// Matrix tab lands on its entry cell, the next Tab leaves the grid, and
/// Shift+Tab from a cell goes to the part before it. Tab back in lands on
/// the cell last chosen.
#[test]
fn the_grid_is_one_stop_that_remembers_its_cell() {
    let mut ui = matrix_open();
    // From the tab itself: Tab goes into the grid.
    let tab = crate::tabs::tab_id(Tab::Matrix);
    ui.focus(&tab);
    assert_eq!(focused(&ui), Some(tab));
    key(&mut ui, Named::Tab);
    assert_eq!(
        focused(&ui),
        Some(cell(1, Column::On)),
        "into the grid, On in the first row"
    );
    key(&mut ui, Named::Tab);
    assert!(
        !focused(&ui).is_some_and(|id| id.is_under(&list_id())),
        "Tab left the grid in one press: {:?}",
        focused(&ui)
    );
    key_with(&mut ui, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(focused(&ui), Some(cell(1, Column::On)), "back into the grid");
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::ArrowRight);
    key_with(&mut ui, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(focused(&ui), Some(access::part("sort")), "the part before the grid");
    key(&mut ui, Named::Tab);
    assert_eq!(focused(&ui), Some(cell(2, Column::Source)), "the cell last chosen");
}

/// Every grid key moves where the data grid pattern says: Up and Down by
/// row in the same column, Left and Right by cell, stopping at the ends,
/// Home and End to the row's ends, Ctrl+Home and Ctrl+End (Cmd on macOS)
/// to the first and last row, Page Up and Page Down by ten rows.
#[test]
fn every_grid_key_moves_the_focus() {
    let mut ui = many_rows();
    let at = |ui: &Ui| {
        let state = &ui.app().session.matrix;
        let rows = ui.app().matrix_model().rows;
        let row = rows
            .iter()
            .position(|row| Some(row.id) == state.selected.map(|id| id.0))
            .unwrap();
        assert_eq!(focused(ui), Some(access::cell(state.selected.unwrap(), state.cell)));
        (row, state.cell)
    };
    assert_eq!(at(&ui), (0, Column::On));
    key(&mut ui, Named::ArrowUp);
    assert_eq!(at(&ui), (0, Column::On), "Up stops at the first row");
    key(&mut ui, Named::ArrowLeft);
    assert_eq!(at(&ui), (0, Column::On), "Left stops at the first cell");
    key(&mut ui, Named::ArrowRight);
    assert_eq!(at(&ui), (0, Column::Source));
    key(&mut ui, Named::ArrowDown);
    assert_eq!(at(&ui), (1, Column::Source), "Down keeps the column");
    key(&mut ui, Named::End);
    assert_eq!(at(&ui), (1, Column::Depth));
    key(&mut ui, Named::ArrowRight);
    assert_eq!(at(&ui), (1, Column::Depth), "Right stops at the last cell");
    key(&mut ui, Named::Home);
    assert_eq!(at(&ui), (1, Column::On));
    key(&mut ui, Named::PageDown);
    assert_eq!(at(&ui), (11, Column::On));
    key(&mut ui, Named::PageDown);
    key(&mut ui, Named::PageDown);
    key(&mut ui, Named::PageDown);
    assert_eq!(at(&ui), (31, Column::On), "Page Down stops at the last row");
    key(&mut ui, Named::PageUp);
    assert_eq!(at(&ui), (21, Column::On));
    key_with(&mut ui, Named::Home, keyboard::Modifiers::COMMAND);
    assert_eq!(at(&ui), (0, Column::On), "Ctrl+Home: the first row");
    key(&mut ui, Named::ArrowRight);
    key_with(&mut ui, Named::End, keyboard::Modifiers::COMMAND);
    assert_eq!(at(&ui), (31, Column::Source), "Ctrl+End: the last row, same column");
    key(&mut ui, Named::ArrowDown);
    assert_eq!(at(&ui), (31, Column::Source), "Down stops at the last row");
    key(&mut ui, Named::PageUp);
    key(&mut ui, Named::PageUp);
    key(&mut ui, Named::PageUp);
    key(&mut ui, Named::PageUp);
    assert_eq!(at(&ui), (0, Column::Source), "Page Up stops at the first row");
}

/// Enter, F2, and Space each toggle On, and the focus stays on it.
#[test]
fn enter_f2_and_space_toggle_on() {
    let mut ui = matrix_open();
    let mut on = first(&ui).enabled;
    for named in [Named::Enter, Named::F2, Named::Space] {
        key(&mut ui, named);
        assert_eq!(first(&ui).enabled, !on, "{named:?} toggles");
        on = !on;
        assert_eq!(focused(&ui), Some(cell(1, Column::On)));
    }
}

/// Enter and F2 open Source, Target, and Polarity's selects, and Escape
/// closes each with no change, the focus still on the cell. Down while
/// closed is the grid's, never the select's.
#[test]
fn enter_and_f2_open_a_select_cell_and_escape_closes_it() {
    for column in [Column::Source, Column::Target, Column::Polarity] {
        for opener in [Named::Enter, Named::F2] {
            let mut ui = matrix_open();
            ui.send(Message::Matrix(Event::Select(AssignmentId(1), column)));
            let _ = ui.redraw();
            let id = cell(1, column);
            assert_eq!(focused(&ui), Some(id.clone()));
            assert!(!open(&mut ui, &id));
            let before = first(&ui);
            key(&mut ui, opener);
            assert!(open(&mut ui, &id), "{column:?}: {opener:?} opens it");
            key(&mut ui, Named::ArrowDown);
            key(&mut ui, Named::Escape);
            assert!(!open(&mut ui, &id), "{column:?}: Escape closes it");
            assert_eq!(focused(&ui), Some(id.clone()), "{column:?}: back on the cell");
            assert_eq!(first(&ui), before, "{column:?}: nothing chosen");
            key(&mut ui, Named::ArrowDown);
            assert_eq!(focused(&ui), Some(cell(2, column)), "{column:?}: Down moves a row");
        }
    }
}

/// A choice in a cell's select applies at once, as one undo step, and the
/// focus stays on the cell.
#[test]
fn a_select_applies_the_choice() {
    let mut ui = matrix_open();
    ui.send(Message::Matrix(Event::Select(AssignmentId(1), Column::Polarity)));
    let _ = ui.redraw();
    let before = first(&ui);
    key(&mut ui, Named::Enter);
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::Enter);
    let _ = ui.redraw();
    assert_ne!(first(&ui).polarity, before.polarity, "the polarity changed");
    assert_eq!(focused(&ui), Some(cell(1, Column::Polarity)));
    ui.app_mut().undo();
    assert_eq!(first(&ui), before);

    // A target choice is a retarget.
    ui.send(Message::Matrix(Event::Select(AssignmentId(1), Column::Target)));
    let _ = ui.redraw();
    key(&mut ui, Named::F2);
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::Enter);
    let _ = ui.redraw();
    let now = ui
        .app()
        .session
        .controller
        .graph()
        .assignments()
        .find(|(_, a)| a.source == before.source && a.depth == before.depth)
        .unwrap()
        .1;
    assert_ne!((now.target, now.param), (before.target, before.param), "retargeted");
}

/// Enter or F2 on Depth puts the focus in its field with the depth's text
/// all selected, so a typed number replaces it; Enter applies and goes
/// back to the cell, and Escape cancels and goes back to it.
#[test]
fn the_depth_field_applies_and_cancels() {
    for opener in [Named::Enter, Named::F2] {
        let mut ui = matrix_open();
        key(&mut ui, Named::End);
        assert_eq!(focused(&ui), Some(cell(1, Column::Depth)));
        key(&mut ui, opener);
        let field = depth_field(AssignmentId(1));
        assert_eq!(focused(&ui), Some(field.clone()), "{opener:?} focuses the field");
        ui.typewrite("42");
        assert_eq!(
            ui.app().session.matrix.edit.as_ref().map(|edit| edit.depth.as_str()),
            Some("42"),
            "the typed number replaced the selected text"
        );
        // The field's keys are its own: Delete and the arrows edit text.
        let count = ui.app().session.controller.graph().assignments().count();
        key(&mut ui, Named::ArrowLeft);
        key(&mut ui, Named::Delete);
        assert_eq!(ui.app().session.controller.graph().assignments().count(), count);
        assert_eq!(focused(&ui), Some(field.clone()));
        ui.typewrite("2");
        key(&mut ui, Named::Enter);
        assert_eq!(first(&ui).depth, 0.42);
        assert!(ui.app().session.matrix.edit.is_none());
        assert_eq!(
            focused(&ui),
            Some(cell(1, Column::Depth)),
            "Enter goes back to the cell"
        );

        key(&mut ui, opener);
        ui.typewrite("9");
        key(&mut ui, Named::Escape);
        assert_eq!(first(&ui).depth, 0.42, "Escape changes nothing");
        assert!(ui.app().session.matrix.edit.is_none());
        assert_eq!(
            focused(&ui),
            Some(cell(1, Column::Depth)),
            "Escape goes back to the cell"
        );
    }
}

/// Escape on a cell with nothing open lets the grid go: the focus leaves
/// the matrix, and the keys are the canvas's again.
#[test]
fn escape_lets_the_grid_go() {
    let mut ui = matrix_open();
    // Entering the grid selects nothing, so select a tile: the canvas
    // takes keys while one is selected.
    let tile = target_tile(&ui, 1);
    ui.app_mut().session.selection = std::collections::BTreeSet::from([tile]);
    key(&mut ui, Named::Escape);
    assert!(!ui.app().focus_in_matrix());
    assert!(ui.app().canvas_has_keys(), "the canvas has the keys");
}

/// A press on a card chooses its row, and a press on a cell chooses and
/// focuses that cell.
#[test]
fn a_press_chooses_a_row_and_a_cell() {
    let mut ui = matrix_open();
    click(&mut ui, &cell(2, Column::Depth));
    assert_eq!(focused(&ui), Some(cell(2, Column::Depth)));
    assert_eq!(ui.app().session.matrix.selected, Some(AssignmentId(2)));
    assert_eq!(ui.app().session.matrix.cell, Column::Depth);
    // A press on the card around the cells: its arrow line's room.
    ui.lay_out_panels();
    let card = ui.runtime().bounds(&access::row(AssignmentId(1))).unwrap();
    let _ = ui.move_to(loom_ui_core::Point::new(card.x + 3.0, card.y + 3.0));
    let _ = ui.press();
    let _ = ui.release();
    let _ = ui.redraw();
    assert_eq!(ui.app().session.matrix.selected, Some(AssignmentId(1)));
    assert_eq!(focused(&ui), Some(cell(1, Column::Depth)), "the same column");
}

/// To a screen reader the matrix is a group named "Modulation matrix"
/// holding a List named "Modulation assignments" with its number of rows,
/// each a ListItem named by its source and target, holding On as a check
/// box, Source, Target, and Polarity as combo boxes, Depth as a slider,
/// and × as a button.
#[test]
fn a_screen_reader_meets_a_list_of_cards() {
    let mut ui = matrix_open();
    let rows = ui.app().matrix_model().rows;
    let tree = ui.a11y_tree();
    let group = tree.node_by_id(&matrix_id());
    assert_eq!(group.role(), accesskit::Role::Group);
    assert_eq!(group.name(), Some("Modulation matrix"));
    let list = tree.node_by_id(&list_id());
    assert_eq!(list.role(), accesskit::Role::List);
    assert_eq!(list.name(), Some("Modulation assignments"));
    assert_eq!(list.size_of_set(), Some(rows.len()));
    for (at, row) in rows.iter().enumerate() {
        let id = AssignmentId(row.id);
        let item = tree.node_by_id(&access::row(id));
        assert_eq!(item.role(), accesskit::Role::ListItem);
        let name = format!("Modulation from {} to {}", row.source, row.target);
        assert_eq!(item.name(), Some(name.as_str()));
        assert_eq!(item.position_in_set(), Some(at));
        for (column, role) in [
            (Column::On, accesskit::Role::CheckBox),
            (Column::Source, accesskit::Role::ComboBox),
            (Column::Target, accesskit::Role::ComboBox),
            (Column::Polarity, accesskit::Role::ComboBox),
            (Column::Depth, accesskit::Role::Slider),
        ] {
            assert_eq!(tree.node_by_id(&access::cell(id, column)).role(), role, "{column:?}");
        }
        assert_eq!(tree.node_by_id(&access::remove(id)).role(), accesskit::Role::Button);
    }
    assert!(tree.node_by_id(&cell(1, Column::On)).is_focused());
}

/// A screen reader opens a row's Source select and chooses another source
/// through its actions alone, and the new source reaches the patch.
#[test]
fn a_screen_reader_chooses_a_source_through_the_select() {
    let mut ui = matrix_open();
    let id = AssignmentId(2);
    let before = *ui.app().session.controller.graph().assignment(id).unwrap();
    let source = cell(2, Column::Source);
    ui.a11y_action(&source, accesskit::Action::Expand);
    assert!(open(&mut ui, &source), "Expand opened it");
    assert_eq!(focused(&ui), Some(source.clone()), "and focused it");
    // An option that is another source this row may take.
    let graph = ui.app().session.controller.graph();
    let row = ui
        .app()
        .matrix_model()
        .rows
        .into_iter()
        .find(|row| row.id == 2)
        .unwrap();
    let picks = crate::matrix::row_sources(
        &sources(graph, &ui.app().session.hierarchy, &ui.app().session.level),
        before.source,
        &row.source,
    );
    let at = picks
        .iter()
        .position(|pick| {
            pick.output != before.source
                && ui.app().allows_assignment(pick.output, before.target, before.param)
                && !crate::modulation::exists(graph, pick.output, before.target, before.param)
        })
        .expect("another source");
    let option = UiId::panel("loom-ui.select-list")
        .with("list")
        .with("options")
        .with(format!("o{at}"));
    ui.a11y_action(&option, accesskit::Action::Click);
    let _ = ui.redraw();
    let changed = ui
        .app()
        .session
        .controller
        .graph()
        .assignments()
        .find(|(_, a)| a.target == before.target && a.param == before.param && a.depth == before.depth)
        .unwrap()
        .1;
    assert_eq!(changed.source, picks[at].output, "the patch has the new source");
}

/// Opens the first row's Depth field by keys and types `text` over its
/// selected text.
fn type_a_depth(ui: &mut Ui, text: &str) {
    key(ui, Named::End);
    key(ui, Named::Enter);
    assert_eq!(focused(ui), Some(depth_field(AssignmentId(1))));
    ui.typewrite(text);
    assert_eq!(
        ui.app().session.matrix.edit.as_ref().map(|edit| edit.depth.as_str()),
        Some(text)
    );
}

/// Tab out of the Depth field commits what was typed, as Enter does
/// (Brylie's decision, 2026-11-25: next stop, commits), as one undo step.
#[test]
fn tab_out_of_the_depth_field_commits() {
    let mut ui = matrix_open();
    let steps = ui.app().session.edits.history.len();
    type_a_depth(&mut ui, "42");
    key(&mut ui, Named::Tab);
    assert_ne!(focused(&ui), Some(depth_field(AssignmentId(1))), "Tab left the field");
    assert_eq!(first(&ui).depth, 0.42);
    assert!(ui.app().session.matrix.edit.is_none());
    assert_eq!(ui.app().session.edits.history.len(), steps + 1, "one undo step");
}

/// A press elsewhere commits what was typed too.
#[test]
fn a_press_elsewhere_commits_the_depth_field() {
    let mut ui = matrix_open();
    type_a_depth(&mut ui, "33");
    click(&mut ui, &cell(2, Column::Depth));
    assert_eq!(focused(&ui), Some(cell(2, Column::Depth)));
    assert_eq!(first(&ui).depth, 0.33);
    assert!(ui.app().session.matrix.edit.is_none());
}

/// An overlay taking the focus is not leaving: the draft waits,
/// uncommitted, and is still there when the overlay closes and the field
/// has the focus again.
#[test]
fn an_overlay_keeps_the_depth_fields_draft() {
    use crate::input_owner::{InputOwner, tests::open_overlay};
    let mut ui = matrix_open();
    type_a_depth(&mut ui, "55");
    open_overlay(&mut ui, InputOwner::Browser);
    let _ = ui.redraw();
    assert_eq!(ui.app().input_owner(), InputOwner::Browser);
    assert_eq!(first(&ui).depth, 0.08, "nothing committed");
    assert_eq!(
        ui.app().session.matrix.edit.as_ref().map(|edit| edit.depth.as_str()),
        Some("55"),
        "the draft waits"
    );
    key(&mut ui, Named::Escape);
    let _ = ui.redraw();
    assert_eq!(focused(&ui), Some(depth_field(AssignmentId(1))), "back in the field");
    assert_eq!(
        ui.app().session.matrix.edit.as_ref().map(|edit| edit.depth.as_str()),
        Some("55")
    );
    key(&mut ui, Named::Enter);
    assert_eq!(first(&ui).depth, 0.55);
}

/// A draft that is not a depth commits nothing on leaving: the depth stays
/// as it was, the draft goes, and the status line says why, as the
/// inspector's value field does.
#[test]
fn an_invalid_depth_draft_commits_nothing_on_leaving() {
    let mut ui = matrix_open();
    let steps = ui.app().session.edits.history.len();
    type_a_depth(&mut ui, "500");
    key(&mut ui, Named::Tab);
    assert_eq!(first(&ui).depth, 0.08, "the depth stays");
    assert!(ui.app().session.matrix.edit.is_none(), "the draft goes");
    assert!(
        ui.app().alerts.status().contains("not changed"),
        "{}",
        ui.app().alerts.status()
    );
    assert_eq!(ui.app().session.edits.history.len(), steps, "no undo step");
}

/// The tile a row's target shows on: the tile its modulation line ends
/// on, as `matrix_select` picks it.
fn target_tile(ui: &Ui, id: u32) -> u32 {
    let a = *ui
        .app()
        .session
        .controller
        .graph()
        .assignment(AssignmentId(id))
        .unwrap();
    ui.app()
        .session
        .scene
        .modulations()
        .iter()
        .find(|line| line.id == id)
        .map_or(a.target.0, |line| line.to)
}

/// Entering the grid only chooses its cell and never changes the canvas
/// selection, by opening the Matrix tab, by Tab, or by Shift+Tab back in;
/// a move to another row then selects that row's target (Brylie's
/// decision, 2026-11-27; WCAG 2.2, 3.2.1 On Focus).
#[test]
fn entering_the_grid_keeps_the_canvas_selection() {
    let mut ui = Ui::open("modulation.ron");
    let targets: Vec<u32> = [1, 2].iter().map(|id| target_tile(&ui, *id)).collect();
    let other = *ui
        .app()
        .session
        .patch_map
        .structure
        .tiles
        .keys()
        .find(|tile| !targets.contains(tile))
        .expect("a tile no row targets");
    let selection = std::collections::BTreeSet::from([other]);
    ui.app_mut().session.selection = selection.clone();
    // Opening the Matrix tab focuses the grid's entry cell.
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    let _ = ui.redraw();
    assert_eq!(focused(&ui), Some(cell(1, Column::On)));
    assert_eq!(ui.app().session.selection, selection, "opening the tab");
    // Tab out of the grid and Shift+Tab back in.
    key(&mut ui, Named::Tab);
    assert!(!focused(&ui).is_some_and(|id| id.is_under(&list_id())));
    key_with(&mut ui, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(focused(&ui), Some(cell(1, Column::On)));
    assert_eq!(ui.app().session.selection, selection, "Tab out and back in");
    // From the tab itself, Tab in.
    let tab = crate::tabs::tab_id(Tab::Matrix);
    ui.focus(&tab);
    key(&mut ui, Named::Tab);
    assert_eq!(focused(&ui), Some(cell(1, Column::On)));
    assert_eq!(ui.app().session.selection, selection, "Tab from the tab");
    // A move within the row keeps it too.
    key(&mut ui, Named::ArrowRight);
    assert_eq!(ui.app().session.selection, selection, "a move within the row");
    // A move to another row selects that row's target.
    key(&mut ui, Named::ArrowDown);
    assert_eq!(focused(&ui), Some(cell(2, Column::Source)));
    assert_eq!(
        ui.app().session.selection,
        std::collections::BTreeSet::from([targets[1]]),
        "Down selects the new row's target"
    );
}
