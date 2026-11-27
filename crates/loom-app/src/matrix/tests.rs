//! Tests for `matrix.rs`: the model's rows, sorting, and filters, retargets
//! and their undo, typed depth, Add, and what follows the graph. The
//! panel's keys, selects, and nodes are tested in `panel/tests.rs`.

use super::panel::tests::{focused, key, key_with, matrix_open};
use super::*;
use crate::ui_test::Ui;
use iced::keyboard::{self, key::Named};

/// The rows the matrix shows now, by assignment.
fn shown(ui: &Ui) -> Vec<u32> {
    ui.app().matrix_model().rows.iter().map(|row| row.id).collect()
}

/// Every assignment, as it is now.
fn assignments(ui: &Ui) -> Vec<(AssignmentId, loom_core::Assignment)> {
    ui.app()
        .session
        .controller
        .graph()
        .assignments()
        .map(|(id, a)| (id, *a))
        .collect()
}

/// A collapsed source keeps its visible name and its place in the source
/// order, and the Source select offers it in that place, so a choice of
/// the next one lands on the source after it.
#[test]
fn matrix_keeps_a_modulation_from_a_subpatch_output() {
    let mut ui = Ui::open("modulation.ron");
    let graph = ui.app().session.controller.graph();
    let (lfo, _) = graph
        .nodes()
        .find(|(_, node)| node.descriptor.id == "loom.mod.lfo")
        .unwrap();
    ui.send(Message::MakeSubPatch(lfo.0));
    let tile = ModuleId(*ui.app().session.selection.first().unwrap());
    assert_ne!(tile, lfo);
    let title = ui.app().session.hierarchy.tile_name(tile).unwrap();
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    ui.send(Message::Matrix(Event::SelectedOnly(true)));
    let rows = ui.app().ui_snapshot().matrix.rows;
    assert_eq!(rows.len(), 1);
    let output = &ui.app().session.scene.module(tile.0).unwrap().outputs[0];
    assert_eq!(rows[0].source, format!("{title}: {}", output.label));
    let id = AssignmentId(rows[0].id);
    ui.send(Message::Matrix(Event::Select(id, Column::Source)));
    assert_eq!(ui.app().matrix_line().unwrap().from, (tile.0, 0));
    let original = *ui.app().session.controller.graph().assignment(id).unwrap();

    // The row's own source is in the select, in module and port order,
    // named as the row names it.
    let graph = ui.app().session.controller.graph();
    let base = sources(graph, &ui.app().session.hierarchy, &ui.app().session.level);
    let picks = row_sources(&base, original.source, &rows[0].source);
    let at = picks.iter().position(|pick| pick.output == original.source).unwrap();
    assert_eq!(picks[at].label, rows[0].source);
    assert!(picks.windows(2).all(|pair| pair[0].output < pair[1].output), "in order");
    // Choosing it again changes nothing and adds no undo step.
    let history = ui.app().session.edits.history.len();
    let cell = access::cell(id, Column::Source);
    let same = ui.app().matrix_chose(&cell, at).expect("the source select");
    ui.send(same);
    assert_eq!(ui.app().session.controller.graph().assignment(id), Some(&original));
    assert_eq!(ui.app().session.edits.history.len(), history);

    ui.send(Message::Matrix(Event::AddOpen));
    let model = ui.app().ui_snapshot().matrix;
    let pick = model
        .sources
        .iter()
        .find(|pick| pick.output == original.source)
        .unwrap();
    assert_eq!(pick.label, rows[0].source);
    assert!(!pick.edit_only);
    let target = model
        .targets
        .iter()
        .find(|pick| pick.module == original.target && pick.param != original.param)
        .unwrap();
    ui.send(Message::Matrix(Event::AddSource(pick.clone())));
    ui.send(Message::Matrix(Event::AddTarget(target.clone())));
    ui.send(Message::Matrix(Event::AddApply));
    assert!(
        assignments(&ui)
            .iter()
            .any(|(_, a)| { a.source == pick.output && a.target == target.module && a.param == target.param })
    );
}

/// Add offers an occupied slot's control with the flat parameter identity,
/// and a row's Target select offers the same.
#[test]
fn matrix_offers_an_occupied_slot_control() {
    let mut ui = Ui::open("modulation.ron");
    let graph = ui.app().session.controller.graph();
    let (_, assignment) = graph.assignments().next().unwrap();
    let assignment = *assignment;
    ui.send(Message::MakeSlot {
        module: assignment.target.0,
        role: "loom.role.filter".into(),
    });
    let slot = ui.app().session.hierarchy.slot_of_part(assignment.target).unwrap();
    let name = ui.app().session.hierarchy.tile_name(slot).unwrap();
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    ui.send(Message::Matrix(Event::AddOpen));
    let add = ui.app().ui_snapshot().matrix;
    let pick = add
        .targets
        .iter()
        .find(|pick| (pick.module, pick.param) == (assignment.target, assignment.param))
        .unwrap();
    assert!(pick.label.starts_with(&format!("{name}: ")));
    let all = targets(
        ui.app().session.controller.graph(),
        &ui.app().session.hierarchy,
        &ui.app().session.level,
    );
    assert!(all.contains(pick), "the Target select offers it too");
}

/// The outside Matrix names, filters and selects the same surface the cable reaches.
#[test]
fn matrix_surface_target_and_range_survive_rename_and_reopen() {
    let mut ui = Ui::with_app(crate::tests::app_with_text(
        r#"Patch(
        version: 1,
        subpatches: {"filter": SubPatch(
            metadata: Metadata(name: "Sweep"),
            modules: [PatchModule(id: 1, kind: "loom.filter.svf@1", params: {"cutoff": 1000.0})],
            exposed: [Exposed(name: "Brightness", module: 1, param: "cutoff")],
        ), "filter-copy": SubPatch(
            metadata: Metadata(name: "Sweep"),
            modules: [PatchModule(id: 1, kind: "loom.filter.svf@1", params: {"cutoff": 1000.0})],
            exposed: [Exposed(name: "Brightness", module: 1, param: "cutoff")],
        )},
        modules: [
            PatchModule(id: 1, kind: "loom.mod.lfo@1"),
            PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("filter")),
            PatchModule(id: 3, kind: "loom.struct.subpatch@1", subpatch: Some("filter-copy")),
        ],
        modulation: [
            PatchModulation(source: (1, "out"), target: (2, "Brightness"), depth: 0.25),
            PatchModulation(source: (1, "out"), target: (3, "Brightness"), depth: 1.0),
        ],
    )"#,
    ));
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    let rows = ui.app().ui_snapshot().matrix.rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].target, "Sweep 2: Brightness");
    assert_eq!(rows[1].target, "Sweep 3: Brightness");
    assert_eq!(rows[0].target_detail.as_deref(), Some("SVF: Cutoff"));
    assert_eq!(rows[0].range.as_deref(), Some("177.8 Hz to 5623 Hz"));
    assert_eq!(rows[1].range.as_deref(), Some("20.00 Hz to 20000 Hz"));
    assert_eq!(
        ui.a11y_tree()
            .node_by_id(&access::cell(AssignmentId(rows[0].id), Column::Depth))
            .description(),
        rows[0].range.as_deref()
    );
    ui.send(Message::Matrix(Event::Select(AssignmentId(rows[0].id), Column::Target)));
    assert_eq!(ui.app().session.selection, [2].into());
    assert_eq!(ui.app().matrix_line().unwrap().to, 2);
    ui.send(Message::Matrix(Event::Filter("sweep 2".into())));
    ui.send(Message::Matrix(Event::SelectedOnly(true)));
    assert_eq!(ui.app().ui_snapshot().matrix.rows.len(), 1);
    ui.app_mut().rename_subpatch(2, "Air");
    ui.send(Message::Matrix(Event::Filter("air".into())));
    assert_eq!(ui.app().ui_snapshot().matrix.rows[0].target, "Air: Brightness");

    let dir = crate::recovery::tests::TempDir::new("matrix-surface");
    let path = dir.0.join("surface.ron");
    crate::files::fake::answer_save(Some(path.clone()));
    ui.send(Message::File(crate::FileAction::Save));
    let reopened = crate::tests::app_with_text(&std::fs::read_to_string(path).unwrap());
    assert_eq!(reopened.ui_snapshot().matrix.rows[0].target, "Air: Brightness");
}

/// The snapshot carries the Depth field's draft and the Add form's picks.
#[test]
fn the_snapshot_describes_open_matrix_editors() {
    let mut ui = matrix_open();
    for _ in 0..4 {
        key(&mut ui, Named::ArrowRight);
    }
    key(&mut ui, Named::Enter);
    let model = ui.app().ui_snapshot().matrix;
    let edit = model.edit.expect("the Depth field");
    assert_eq!(edit.column, Column::Depth);
    assert_eq!(edit.depth, crate::modulation::depth_text(0.08));

    key(&mut ui, Named::Escape);
    ui.send(Message::Matrix(Event::AddOpen));
    let model = ui.app().ui_snapshot().matrix;
    assert!(model.add.is_some());
    assert!(!model.sources.is_empty());
    assert!(!model.targets.is_empty());
}

/// The modulation example with 30 more assignments, so the cards run
/// past the view, committed so a later step is the test's own.
fn overflowing() -> Ui {
    let mut ui = Ui::open("modulation.ron");
    let assignment = *ui
        .app()
        .session
        .controller
        .graph()
        .assignment(AssignmentId(1))
        .expect("assignment");
    for index in 0..30 {
        ui.app_mut()
            .session
            .controller
            .add_assignment(
                assignment.source,
                assignment.target,
                assignment.param,
                -0.9 + index as f32 * 0.03,
            )
            .unwrap();
    }
    assert!(ui.app_mut().commit());
    ui.app_mut().session.edits.history.clear();
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    let _ = ui.redraw();
    ui
}

/// Where the part `id` shows in the window, as the runtime last laid it
/// out: its bounds are moved by the card list's scroll offset already.
fn shown_at(ui: &Ui, id: &loom_ui_core::UiId) -> loom_ui_core::Rect {
    ui.runtime().bounds(id).unwrap_or_else(|| panic!("{id} is laid out"))
}

/// The chosen card and cell lie inside the card list's view, as the
/// runtime last laid it out.
fn check_in_view(ui: &mut Ui) {
    ui.lay_out_panels();
    let selected = ui.app().session.matrix.selected.expect("a chosen row");
    let cell = access::cell(selected, ui.app().session.matrix.cell);
    let list = ui.runtime().bounds(&panel::list_id()).expect("the card list");
    let card = shown_at(ui, &access::row(selected));
    let cell = shown_at(ui, &cell);
    for (what, part) in [("card", card), ("cell", cell)] {
        assert!(part.y >= list.y - 1.0, "{what} {part:?} above {list:?}");
        assert!(
            part.y + part.height <= list.y + list.height + 1.0,
            "{what} {part:?} below {list:?}"
        );
        assert!(part.x >= list.x - 1.0 && part.x + part.width <= list.x + list.width + 1.0);
    }
    assert_eq!(focused(ui), Some(access::cell(selected, ui.app().session.matrix.cell)));
}

/// The card list's scroll offset, down.
fn offset(ui: &Ui) -> f32 {
    ui.runtime().scroll_offset(&panel::list_id()).map_or(0.0, |(y, _)| y)
}

/// Every module's place on the canvas.
fn positions(ui: &Ui) -> Vec<(u32, (f32, f32))> {
    ui.app().session.positions.iter().map(|(id, at)| (*id, *at)).collect()
}

/// The grid's keys keep the chosen card and cell in the card list's view:
/// none in view, below, back above, and the last row, and moving between
/// cells never scrolls sideways. Sorting and filtering keep it in view
/// too, and none of it is an undo step.
#[test]
fn keyboard_reveals_rows_in_an_overflowing_table() {
    let mut ui = overflowing();
    let (places, before) = (positions(&ui), assignments(&ui));
    key(&mut ui, Named::ArrowDown);
    check_in_view(&mut ui);
    assert_eq!(offset(&ui), 0.0, "the second row is in view: no scroll");
    for _ in 1..32 {
        key(&mut ui, Named::ArrowDown);
        check_in_view(&mut ui);
    }
    assert_eq!(ui.app().session.matrix.selected, Some(AssignmentId(32)), "the last row");
    assert!(offset(&ui) > 0.0, "below: it scrolled");
    for _ in 0..4 {
        key(&mut ui, Named::ArrowRight);
        check_in_view(&mut ui);
    }
    for _ in 0..31 {
        key(&mut ui, Named::ArrowUp);
        check_in_view(&mut ui);
    }
    assert_eq!(ui.app().session.matrix.selected, Some(AssignmentId(1)));
    ui.send(Message::Matrix(Event::SortBy(Some(Column::Depth))));
    let _ = ui.redraw();
    check_in_view(&mut ui);
    ui.send(Message::Matrix(Event::ReverseSort));
    let _ = ui.redraw();
    check_in_view(&mut ui);
    // Past the arrow group's deadline, the arrows moved no tile, changed no
    // assignment, and left no undo step (#871).
    let _ = ui.wake_after(std::time::Duration::from_millis(600));
    let _ = ui.redraw();
    assert_eq!(positions(&ui), places, "no tile moved");
    assert_eq!(before, assignments(&ui));
    assert!(ui.app().session.edits.history.undo_step().is_none());
}

/// Tab until the runtime's focus is `to`, at most 20 presses, forward or
/// back.
fn tab_to(ui: &mut Ui, to: &loom_ui_core::UiId, back: bool) {
    let modifiers = if back {
        keyboard::Modifiers::SHIFT
    } else {
        keyboard::Modifiers::empty()
    };
    for _ in 0..20 {
        if focused(ui).as_ref() == Some(to) {
            return;
        }
        key_with(ui, Named::Tab, modifiers);
    }
    panic!("Tab never reached {to}, at {:?}", focused(ui));
}

/// #871: a change of filter left the grid's keys to the canvas, which
/// moved the selected tile with the arrows. Through real input: the
/// filter and Selected module only change, Tab comes back into the grid on
/// the chosen cell, and Down then walks the rows that show. Past the
/// arrow group's deadline, no module moved, no assignment changed, and
/// there is no undo step. Up and Down in the filter move no tile either.
#[test]
fn changing_the_filters_keeps_the_keys_off_the_canvas() {
    let mut ui = overflowing();
    let (places, before) = (positions(&ui), assignments(&ui));
    // Down twice, onto a row from the LFO that the filter keeps.
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::ArrowDown);
    let chosen = focused(&ui).expect("a cell");
    tab_to(&mut ui, &filter_part(), true);
    ui.typewrite("LFO");
    assert_eq!(ui.app().session.matrix.filter, "LFO");
    assert_eq!(focused(&ui), Some(filter_part()), "the focus stays in the filter");
    for named in [Named::ArrowUp, Named::ArrowDown] {
        key(&mut ui, named);
        assert_eq!(focused(&ui), Some(filter_part()), "{named:?} stays in the filter");
    }
    tab_to(&mut ui, &access::part("selected-only"), false);
    key(&mut ui, Named::Space);
    assert!(ui.app().session.matrix.selected_only);
    for _ in 0..10 {
        key(&mut ui, Named::Tab);
        if focused(&ui).is_some_and(|id| id.is_under(&panel::list_id())) {
            break;
        }
    }
    assert_eq!(focused(&ui), Some(chosen), "back on the chosen cell");
    for _ in 0..31 {
        key(&mut ui, Named::ArrowDown);
        let selected = ui.app().session.matrix.selected.expect("a chosen row");
        assert!(shown(&ui).contains(&selected.0), "{selected:?} shows");
        assert!(
            focused(&ui).is_some_and(|id| id.is_under(&panel::list_id())),
            "the grid keeps the keys"
        );
    }
    let _ = ui.wake_after(std::time::Duration::from_millis(600));
    let _ = ui.redraw();
    assert_eq!(positions(&ui), places, "no tile moved");
    assert_eq!(assignments(&ui), before, "no assignment changed");
    assert!(ui.app().session.edits.history.undo_step().is_none(), "no undo step");
}

/// Visible rows follow engine order, sorting, text, and module filters
/// without changing the graph or adding an undo step.
#[test]
fn rows_sort_and_filter_without_editing_assignments() {
    let mut ui = Ui::open("modulation.ron");
    let before = assignments(&ui);
    ui.app_mut().matrix_event(Event::Tab(Tab::Matrix));
    assert_eq!(shown(&ui), vec![1, 2]);
    ui.app_mut().matrix_event(Event::SortBy(Some(Column::Depth)));
    assert_eq!(shown(&ui), vec![1, 2]);
    ui.app_mut().matrix_event(Event::ReverseSort);
    assert_eq!(shown(&ui), vec![2, 1]);
    for column in Column::ALL {
        ui.app_mut().matrix_event(Event::SortBy(Some(column)));
        assert_eq!(shown(&ui).len(), 2, "{} ascending", column.label());
        ui.app_mut().matrix_event(Event::ReverseSort);
        assert_eq!(
            ui.app().matrix_model().sort,
            Some((column, true)),
            "{} descending",
            column.label()
        );
    }
    ui.app_mut().matrix_event(Event::Filter("LFO".into()));
    assert_eq!(shown(&ui), vec![1]);
    ui.app_mut().matrix_event(Event::Filter(String::new()));
    ui.app_mut().matrix_event(Event::SelectedOnly(true));
    ui.app_mut()
        .matrix_event(Event::Select(AssignmentId(1), Column::Source));
    assert_eq!(shown(&ui).len(), 2, "both assignments target the selected filter");
    assert_eq!(ui.app().ui_snapshot().matrix.tab, Tab::Matrix);
    assert_eq!(before, assignments(&ui));
    assert!(ui.app().session.edits.history.undo_step().is_none());
}

/// Retargeting keeps the same id and fields, refuses duplicates, and
/// undo restores the old endpoint in engine order.
#[test]
fn retarget_keeps_id_order_and_one_undo_step() {
    let mut ui = Ui::open("modulation.ron");
    ui.app_mut().matrix_event(Event::Tab(Tab::Matrix));
    let id = AssignmentId(1);
    let original = *ui.app().session.controller.graph().assignment(id).unwrap();
    let all = |ui: &Ui| {
        targets(
            ui.app().session.controller.graph(),
            &ui.app().session.hierarchy,
            &ui.app().session.level,
        )
    };
    let target = all(&ui)
        .into_iter()
        .find(|t| t.module == ModuleId(4) && t.param != original.param)
        .expect("another filter parameter");
    ui.app_mut().matrix_event(Event::Target(id, target));
    let changed = *ui.app().session.controller.graph().assignment(id).unwrap();
    assert_ne!(changed.param, original.param);
    assert_eq!(changed.depth, original.depth);
    assert_eq!(changed.polarity, original.polarity);
    assert_eq!(changed.enabled, original.enabled);
    assert_eq!(
        assignments(&ui).iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        vec![1, 2]
    );
    ui.app_mut().undo();
    assert_eq!(ui.app().session.controller.graph().assignment(id), Some(&original));
    ui.app_mut().redo();
    assert_eq!(ui.app().session.controller.graph().assignment(id), Some(&changed));
    let duplicate = sources(
        ui.app().session.controller.graph(),
        &ui.app().session.hierarchy,
        &ui.app().session.level,
    )
    .into_iter()
    .find(|s| s.output.module == ModuleId(6))
    .expect("envelope output");
    let cutoff = all(&ui)
        .into_iter()
        .find(|t| t.module == ModuleId(4) && t.param == original.param)
        .expect("cutoff");
    ui.app_mut().matrix_event(Event::Target(id, cutoff));
    ui.app_mut().matrix_event(Event::Source(id, duplicate));
    assert!(ui.app().alerts.status().contains("already modulates"));
    assert_eq!(
        ui.app().session.controller.graph().assignment(id).unwrap().source,
        original.source
    );
}

/// The depth of assignment 1.
fn depth(ui: &Ui) -> f32 {
    ui.app()
        .session
        .controller
        .graph()
        .assignment(AssignmentId(1))
        .unwrap()
        .depth
}

/// Typed depth validation leaves a bad edit open and commits an in-range
/// number as one step; Escape discards an unfinished edit.
#[test]
fn typed_depth_and_escape() {
    let mut ui = matrix_open();
    let id = AssignmentId(1);
    ui.app_mut().matrix_event(Event::DepthText(id, "101".into()));
    ui.app_mut().matrix_event(Event::Apply);
    assert!(ui.app().session.matrix.edit.is_some());
    assert_eq!(depth(&ui), 0.08);
    ui.app_mut().matrix_event(Event::DepthText(id, "-35%".into()));
    ui.app_mut().matrix_event(Event::Apply);
    assert_eq!(depth(&ui), -0.35);
    ui.app_mut().undo();
    assert_eq!(depth(&ui), 0.08);
    ui.app_mut().matrix_event(Event::DepthText(id, "90".into()));
    ui.app_mut().matrix_event(Event::Cancel);
    assert_eq!(depth(&ui), 0.08);
    assert!(ui.app().session.matrix.edit.is_none());
}

/// Delete removes the chosen row and the focus goes on to the row that
/// takes its place, so a second Delete removes that one and never a
/// module; with none left the focus goes to Add. Undo brings each back.
#[test]
fn delete_twice_removes_two_rows_and_no_module() {
    let mut ui = matrix_open();
    let modules = ui.app().session.controller.graph().nodes().count();
    key(&mut ui, Named::Delete);
    assert!(
        ui.app()
            .session
            .controller
            .graph()
            .assignment(AssignmentId(1))
            .is_none()
    );
    assert_eq!(focused(&ui), Some(access::cell(AssignmentId(2), Column::On)));
    key(&mut ui, Named::Delete);
    assert_eq!(ui.app().session.controller.graph().assignments().count(), 0);
    assert_eq!(focused(&ui), Some(access::part("add")), "no row left: Add");
    assert_eq!(
        ui.app().session.controller.graph().nodes().count(),
        modules,
        "no module went"
    );
    ui.app_mut().undo();
    ui.app_mut().undo();
    assert_eq!(ui.app().session.controller.graph().assignments().count(), 2);
}

/// Command shortcuts still undo and redo while a cell has the focus.
#[test]
fn command_shortcuts_undo_and_redo_while_the_grid_has_the_focus() {
    let mut ui = matrix_open();
    let id = AssignmentId(1);
    let on = |ui: &Ui| ui.app().session.controller.graph().assignment(id).unwrap().enabled;
    let original = on(&ui);
    key(&mut ui, Named::Enter);
    assert_eq!(on(&ui), !original);
    ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);
    assert_eq!(on(&ui), original);
    ui.tap(
        keyboard::Key::Character("z".into()),
        keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
    );
    assert_eq!(on(&ui), !original);
}

/// #597: Cmd+V in the Depth field pastes into the field only, opened by a
/// key or by a press on it.
#[test]
fn cmd_v_in_a_depth_field_stays_out_of_the_canvas() {
    for by_key in [true, false] {
        let mut ui = matrix_open();
        if by_key {
            key(&mut ui, Named::End);
            key(&mut ui, Named::Enter);
        } else {
            ui.lay_out_panels();
            let at = ui
                .runtime()
                .bounds(&panel::depth_field(AssignmentId(1)))
                .expect("the Depth field");
            let _ = ui.move_to(loom_ui_core::Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0));
            let _ = ui.press();
            let _ = ui.release();
            let _ = ui.redraw();
        }
        assert_eq!(
            focused(&ui),
            Some(panel::depth_field(AssignmentId(1))),
            "by key: {by_key}"
        );
        let messages = ui.tap(keyboard::Key::Character("v".into()), keyboard::Modifiers::COMMAND);
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, Message::Canvas(loom_canvas::CanvasEvent::Paste { .. }))),
            "the canvas took the paste: {messages:?}"
        );
    }
}

/// A canvas click returns the keys to the canvas before Delete.
#[test]
fn canvas_click_then_delete_removes_the_module_not_a_matrix_row() {
    let mut ui = matrix_open();
    assert!(ui.app().focus_in_matrix());
    let tile = ui.app().session.scene.module(3).expect("saw tile").bounds();
    let at = ui
        .canvas()
        .to_window(loom_ui_core::Point::new(tile.center().x, tile.y + 10.0));
    let mut heard = ui.move_to(at);
    heard.extend(ui.press());
    heard.extend(ui.release());
    heard.extend(ui.redraw());
    assert!(!ui.app().focus_in_matrix(), "{heard:?} {:?}", ui.runtime().focused());
    assert!(ui.app().session.selection.contains(&3));
    ui.tap(keyboard::Key::Named(Named::Delete), keyboard::Modifiers::default());
    assert!(ui.app().session.controller.graph().node(ModuleId(3)).is_none());
    assert_eq!(ui.app().session.controller.graph().assignments().count(), 2);
}

/// Add uses the normal default depth and duplicate status, a new pair
/// is one step, and a depth slider drag records one undo step.
#[test]
fn add_duplicate_and_drag_follow_inspector_edits() {
    let mut ui = Ui::open("modulation.ron");
    ui.app_mut().matrix_event(Event::Tab(Tab::Matrix));
    let existing = *ui.app().session.controller.graph().assignment(AssignmentId(1)).unwrap();
    let graph = ui.app().session.controller.graph();
    let (hierarchy, level) = (&ui.app().session.hierarchy, &ui.app().session.level);
    let all_sources = sources(graph, hierarchy, level);
    let all_targets = targets(graph, hierarchy, level);
    let source = all_sources
        .iter()
        .find(|s| s.output == existing.source)
        .unwrap()
        .clone();
    let target = all_targets
        .iter()
        .find(|t| (t.module, t.param) == (existing.target, existing.param))
        .unwrap()
        .clone();
    ui.app_mut().matrix_event(Event::AddOpen);
    ui.app_mut().matrix_event(Event::AddSource(source.clone()));
    ui.app_mut().matrix_event(Event::AddTarget(target));
    ui.app_mut().matrix_event(Event::AddApply);
    assert_eq!(ui.app().session.controller.graph().assignments().count(), 2);
    assert!(ui.app().alerts.status().contains("already modulates"));
    // Pressing Add again keeps the open form's picks.
    ui.app_mut().matrix_event(Event::AddOpen);
    ui.app_mut().matrix_event(Event::AddSource(source.clone()));
    ui.app_mut().matrix_event(Event::AddOpen);
    assert!(
        ui.app().session.matrix.add.as_ref().unwrap().source.is_some(),
        "Add again keeps the form"
    );
    let fresh = all_targets
        .iter()
        .find(|t| !modulation::exists(ui.app().session.controller.graph(), source.output, t.module, t.param))
        .unwrap()
        .clone();
    ui.app_mut().matrix_event(Event::AddTarget(fresh.clone()));
    ui.app_mut().matrix_event(Event::AddApply);
    assert_eq!(ui.app().session.controller.graph().assignments().count(), 3);
    let added = assignments(&ui).last().unwrap().1;
    let spec = ui
        .app()
        .session
        .controller
        .graph()
        .node(added.target)
        .unwrap()
        .spec(added.param)
        .unwrap();
    assert_eq!(added.depth, modulation::default_depth(&spec));
    ui.app_mut().undo();
    assert_eq!(ui.app().session.controller.graph().assignments().count(), 2);

    // A drag on the Depth slider, as the panel's slider sends it.
    let slider = access::cell(AssignmentId(1), Column::Depth);
    for position in [0.7, 0.75] {
        let moved = ui
            .app()
            .matrix_slid(&slider, position, loom_ui::SlidePhase::Move)
            .unwrap();
        ui.send(moved);
    }
    ui.app_mut().matrix_event(Event::DepthReleased);
    assert_eq!(depth(&ui), 0.5);
    ui.app_mut().undo();
    assert_eq!(
        ui.app().session.controller.graph().assignment(AssignmentId(1)),
        Some(&existing)
    );
}

/// A new patch keeps the session tab but drops patch-specific rows and
/// selection; controller endpoint validation leaves invalid edits alone.
#[test]
fn tab_survives_patch_change_and_bad_endpoint_changes_nothing() {
    let mut ui = Ui::open("modulation.ron");
    ui.app_mut().matrix_event(Event::Tab(Tab::Matrix));
    let id = AssignmentId(1);
    let old = *ui.app().session.controller.graph().assignment(id).unwrap();
    let nowhere = OutputRef {
        module: ModuleId(9999),
        port: 0,
    };
    assert!(
        ui.app_mut()
            .session
            .controller
            .retarget_assignment(id, nowhere, old.target, old.param)
            .is_err()
    );
    assert_eq!(ui.app().session.controller.graph().assignment(id), Some(&old));
    ui.app_mut().matrix_event(Event::Select(id, Column::Source));
    assert_eq!(
        ui.app().matrix_line().expect("selected line").from,
        (old.source.module.0, old.source.port)
    );
    assert!(ui.app_mut().replace(crate::Source::Empty));
    assert_eq!(ui.app().right_tab, Tab::Matrix);
    assert!(ui.app().matrix_model().rows.is_empty());
    assert!(ui.app().session.matrix.selected.is_none());
}

/// Every card fits the right-hand column: a long source shows in full,
/// and no column titles show (#456).
#[test]
fn cards_fit_the_column() {
    let mut ui = Ui::open("modulation.ron");
    let hold = ui
        .app_mut()
        .session
        .controller
        .add_module("loom.rand.sample-hold")
        .unwrap();
    ui.app_mut()
        .session
        .controller
        .add_module("loom.rand.sample-hold")
        .unwrap();
    ui.app_mut()
        .session
        .controller
        .add_assignment(OutputRef { module: hold, port: 0 }, ModuleId(4), 1, 0.25)
        .unwrap();
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    let _ = ui.redraw();
    let source = modulation::Names::new(ui.app().session.controller.graph(), &ui.app().session.hierarchy)
        .source(OutputRef { module: hold, port: 0 });
    assert!(source.len() >= 23, "{source}");
    let right = crate::WINDOW.width;
    let left = right - style_width();
    let mut labels = vec![source];
    for row in &ui.app().matrix_model().rows {
        labels.extend([row.source.clone(), row.target.clone(), row.polarity.clone()]);
        labels.push(modulation::depth_text(row.depth));
    }
    for label in labels {
        let bounds = ui.text(&label).unwrap_or_else(|| panic!("`{label}` is not shown"));
        assert!(
            bounds.x >= left && bounds.x + bounds.width <= right + 1.0,
            "`{label}` at {bounds:?} is outside the column"
        );
    }
    for gone in ["Source", "Target", "Depth", "Polarity"] {
        assert!(ui.text(gone).is_none(), "`{gone}` is shown as a title");
    }
}

/// The right column's width.
fn style_width() -> f32 {
    crate::style::RIGHT_COLUMN_WIDTH
}

/// #435: arrows move no row behind an overlay, and none of it is an undo
/// step.
#[test]
fn the_matrix_has_no_keys_behind_an_overlay() {
    use crate::input_owner::{InputOwner, tests::open_overlay};
    for owner in [
        InputOwner::Help,
        InputOwner::Chooser,
        InputOwner::Patches,
        InputOwner::Preferences,
        InputOwner::Browser,
        InputOwner::RecentList,
    ] {
        let mut ui = matrix_open();
        let undo = ui.app().session.edits.history.undo_step().map(|s| s.name().to_string());
        open_overlay(&mut ui, owner);
        let before = ui.app().session.matrix.selected;
        key(&mut ui, Named::ArrowDown);
        assert_eq!(ui.app().session.matrix.selected, before, "{owner:?}");
        assert_eq!(
            ui.app().session.edits.history.undo_step().map(|s| s.name().to_string()),
            undo,
            "{owner:?}"
        );
    }
}

/// A card that a sort moves wholly below the view comes in with its
/// bottom at the view's bottom, and one moved above with its top at the
/// view's top: the card's scroll and then its cell's do not add up.
#[test]
fn a_card_moved_out_of_view_comes_back_just_inside() {
    let mut ui = overflowing();
    ui.send(Message::Matrix(Event::Select(AssignmentId(1), Column::Depth)));
    let _ = ui.redraw();
    let card = |ui: &mut Ui| {
        ui.lay_out_panels();
        let card = shown_at(ui, &access::row(AssignmentId(1)));
        let view = ui.runtime().bounds(&panel::list_id()).expect("the card list");
        (card, view)
    };
    let near = |a: f32, b: f32| (a - b).abs() <= 8.0;
    let (start, view) = card(&mut ui);
    assert!(
        start.y >= view.y && start.y + start.height <= view.y + view.height,
        "it starts in view"
    );
    ui.send(Message::Matrix(Event::SortBy(Some(Column::Depth))));
    let _ = ui.redraw();
    let order = |ui: &Ui| {
        let rows = shown(ui);
        (rows.iter().position(|id| *id == 1).unwrap(), rows.len())
    };
    if order(&ui).0 < 2 {
        ui.send(Message::Matrix(Event::ReverseSort));
        let _ = ui.redraw();
    }
    let (at, rows) = order(&ui);
    assert!(at + 2 >= rows, "it is at the end of the table now: {at} of {rows}");
    let (below, view) = card(&mut ui);
    assert!(
        near(below.y + below.height, view.y + view.height),
        "below: its bottom at the view's bottom, {below:?} in {view:?}"
    );
    ui.send(Message::Matrix(Event::ReverseSort));
    let _ = ui.redraw();
    assert!(order(&ui).0 < 2, "and at its start now");
    let (above, view) = card(&mut ui);
    assert!(
        near(above.y, view.y),
        "above: its top at the view's top, {above:?} in {view:?}"
    );
}

/// #435, ui-layer.md section 6, rule 4: closing the module browser,
/// Preferences, or a question gives the focus back to the matrix cell that
/// had it, and Down then moves the row again.
#[test]
fn the_grid_has_the_keys_again_after_an_overlay_closes() {
    use crate::input_owner::{InputOwner, tests::open_overlay};
    let owners = [InputOwner::Browser, InputOwner::Preferences, InputOwner::Question];
    for (owner, column) in owners.into_iter().flat_map(|o| [(o, Column::On), (o, Column::Source)]) {
        let mut ui = matrix_open();
        if column == Column::Source {
            key(&mut ui, Named::ArrowRight);
        }
        let cell = access::cell(AssignmentId(1), column);
        assert_eq!(focused(&ui), Some(cell.clone()));
        open_overlay(&mut ui, owner);
        let _ = ui.redraw();
        assert_eq!(ui.app().input_owner(), owner, "{owner:?} open");
        key(&mut ui, Named::Escape);
        let _ = ui.redraw();
        assert_eq!(ui.app().input_owner(), InputOwner::Canvas, "{owner:?} closed");
        assert_eq!(focused(&ui), Some(cell), "{owner:?}: the cell has the focus again");
        key(&mut ui, Named::ArrowDown);
        assert_eq!(
            ui.app().session.matrix.selected,
            Some(AssignmentId(2)),
            "{owner:?}: Down moved the row"
        );
    }
}

/// Presses the filter field, as a person would, so it has the focus.
fn click_filter(ui: &mut Ui) {
    ui.lay_out_panels();
    let at = ui.runtime().bounds(&filter_part()).expect("the filter");
    let _ = ui.move_to(loom_ui_core::Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0));
    let _ = ui.press();
    let _ = ui.release();
    let _ = ui.redraw();
    assert_eq!(focused(ui), Some(filter_part()), "the press focused the filter");
}

/// What a key must not change: the row, the cell, any edit, and the patch.
fn rows_and_cells(ui: &Ui) -> (Option<AssignmentId>, Column, bool, usize) {
    (
        ui.app().session.matrix.selected,
        ui.app().session.matrix.cell,
        ui.app().session.matrix.edit.is_some(),
        ui.app().session.controller.graph().assignments().count(),
    )
}

/// #435, modulation.md, Filter focus: keys in the filter leave the rows,
/// the cells, the assignments, and the tiles alone, and the field keeps
/// the focus.
#[test]
fn keys_in_the_filter_leave_rows_alone() {
    for named in [
        Named::Delete,
        Named::Backspace,
        Named::ArrowUp,
        Named::ArrowDown,
        Named::ArrowRight,
        Named::Enter,
    ] {
        let mut ui = matrix_open();
        click_filter(&mut ui);
        let (before, places) = (rows_and_cells(&ui), positions(&ui));
        key(&mut ui, named);
        assert_eq!(rows_and_cells(&ui), before, "{named:?}");
        assert_eq!(positions(&ui), places, "{named:?} moved no tile");
        assert_eq!(
            focused(&ui),
            Some(filter_part()),
            "{named:?}: the field keeps the focus"
        );
    }
}

/// #435: typing and clearing the filter changes only the filter, and
/// Escape from it gives the keys back to the canvas.
#[test]
fn typing_clearing_and_leaving_the_filter() {
    let mut ui = matrix_open();
    // Entering the grid selects nothing, so select a tile: the canvas
    // takes keys while one is selected.
    let tile = *ui
        .app()
        .session
        .patch_map
        .structure
        .tiles
        .keys()
        .next()
        .expect("a tile");
    ui.app_mut().session.selection = std::collections::BTreeSet::from([tile]);
    let count = ui.app().session.controller.graph().assignments().count();
    click_filter(&mut ui);
    ui.typewrite("zz");
    assert_eq!(ui.app().session.matrix.filter, "zz");
    key(&mut ui, Named::Backspace);
    key(&mut ui, Named::Backspace);
    assert_eq!(ui.app().session.matrix.filter, "");
    assert_eq!(ui.app().session.controller.graph().assignments().count(), count);
    assert!(ui.app().session.edits.history.undo_step().is_none());
    key(&mut ui, Named::Escape);
    assert!(!ui.app().focus_in_matrix(), "Escape left the matrix");
    assert!(ui.app().canvas_has_keys());
}

/// #435: Cmd+Z in the filter is the field's own undo, so it undoes nothing
/// in the patch behind it.
#[test]
fn cmd_z_in_the_filter_undoes_nothing_behind_it() {
    let mut ui = matrix_open();
    let on = |ui: &Ui| {
        ui.app()
            .session
            .controller
            .graph()
            .assignment(AssignmentId(1))
            .unwrap()
            .enabled
    };
    key(&mut ui, Named::Enter);
    let toggled = on(&ui);
    click_filter(&mut ui);
    ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);
    let _ = ui.redraw();
    assert_eq!(on(&ui), toggled, "the toggle stays");
}

/// Add by keyboard alone: Add opens the form on its source select, each
/// select opens on Enter and chooses with the arrows and Enter, Create
/// adds the pair and puts the focus back on Add, and Cancel and Escape
/// close the form there too.
#[test]
fn the_add_form_by_keyboard() {
    let mut ui = matrix_open();
    let count = ui.app().session.controller.graph().assignments().count();
    tab_to(&mut ui, &access::part("add"), true);
    key(&mut ui, Named::Enter);
    let _ = ui.redraw();
    assert_eq!(focused(&ui), Some(access::part("new/source")), "the form's first pick");
    // The last source and the last target: a pair no row has.
    for pick in ["new/source", "new/target"] {
        tab_to(&mut ui, &access::part(pick), false);
        key(&mut ui, Named::Enter);
        key(&mut ui, Named::End);
        key(&mut ui, Named::Enter);
        let _ = ui.redraw();
    }
    let add = ui.app().session.matrix.add.clone().expect("the form");
    assert!(add.source.is_some() && add.target.is_some(), "both picked");
    tab_to(&mut ui, &access::part("new/create"), false);
    key(&mut ui, Named::Enter);
    let _ = ui.redraw();
    assert_eq!(ui.app().session.controller.graph().assignments().count(), count + 1);
    assert_eq!(focused(&ui), Some(access::part("add")), "back on Add");
    for closer in ["cancel", "escape"] {
        key(&mut ui, Named::Enter);
        let _ = ui.redraw();
        if closer == "cancel" {
            tab_to(&mut ui, &access::part("new/cancel"), false);
            key(&mut ui, Named::Enter);
        } else {
            key(&mut ui, Named::Escape);
        }
        let _ = ui.redraw();
        assert!(ui.app().session.matrix.add.is_none(), "{closer} closed the form");
        assert_eq!(focused(&ui), Some(access::part("add")), "{closer}: back on Add");
    }
}

/// Sort by by keyboard: its select opens on Enter and a choice sorts.
#[test]
fn sort_by_by_keyboard() {
    let mut ui = matrix_open();
    tab_to(&mut ui, &access::part("sort"), true);
    key(&mut ui, Named::Enter);
    key(&mut ui, Named::End);
    key(&mut ui, Named::Enter);
    let _ = ui.redraw();
    assert_eq!(
        ui.app().session.matrix.sort,
        Some((Column::Depth, false)),
        "the last option"
    );
    tab_to(&mut ui, &access::part("reverse"), false);
    key(&mut ui, Named::Enter);
    let _ = ui.redraw();
    assert_eq!(ui.app().session.matrix.sort, Some((Column::Depth, true)));
}

/// Ctrl+Tab into the right column with the Matrix open lands on the
/// grid's chosen cell.
#[test]
fn ctrl_tab_enters_the_grid_on_its_chosen_cell() {
    let mut ui = matrix_open();
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::ArrowRight);
    key(&mut ui, Named::Escape);
    assert!(!ui.app().focus_in_matrix());
    ui.app_mut().session.selection.clear();
    for _ in 0..3 {
        if ui.app().focus_in_matrix() {
            break;
        }
        key_with(&mut ui, Named::Tab, keyboard::Modifiers::CTRL);
    }
    assert_eq!(focused(&ui), Some(access::cell(AssignmentId(2), Column::Source)));
}

/// Tab into the grid when the chosen row is filtered out lands on On in
/// the first row that shows.
#[test]
fn tab_into_the_grid_with_its_row_filtered_out() {
    let mut ui = matrix_open();
    key(&mut ui, Named::ArrowDown);
    assert_eq!(ui.app().session.matrix.selected, Some(AssignmentId(2)));
    tab_to(&mut ui, &filter_part(), true);
    ui.typewrite("LFO");
    assert!(!shown(&ui).contains(&2), "the envelope's row is filtered out");
    for _ in 0..10 {
        key(&mut ui, Named::Tab);
        if focused(&ui).is_some_and(|id| id.is_under(&panel::list_id())) {
            break;
        }
    }
    assert_eq!(focused(&ui), Some(access::cell(AssignmentId(1), Column::On)));
}

/// Keys the grid does not take do nothing behind it while a cell has the
/// focus: Backspace removes no module, a letter opens nothing and changes
/// no tab, and Space on a select or the Depth slider scrolls no card out
/// of view.
#[test]
fn keys_the_grid_does_not_take_reach_nothing_behind_it() {
    let mut ui = overflowing();
    let modules = ui.app().session.controller.graph().nodes().count();
    for column in [Column::On, Column::Source, Column::Depth] {
        ui.send(Message::Matrix(Event::Select(AssignmentId(1), column)));
        let _ = ui.redraw();
        let cell = access::cell(AssignmentId(1), column);
        assert_eq!(focused(&ui), Some(cell.clone()));
        key(&mut ui, Named::Backspace);
        for letter in ["a", "o", "c"] {
            ui.tap(keyboard::Key::Character(letter.into()), keyboard::Modifiers::empty());
            let _ = ui.redraw();
        }
        if column != Column::On {
            key(&mut ui, Named::Space);
            assert_eq!(offset(&ui), 0.0, "{column:?}: Space scrolled nothing");
        }
        assert_eq!(
            ui.app().session.controller.graph().nodes().count(),
            modules,
            "{column:?}"
        );
        assert_eq!(ui.app().right_tab, Tab::Matrix, "{column:?}");
        assert_eq!(
            ui.app().input_owner(),
            crate::input_owner::InputOwner::Canvas,
            "{column:?}"
        );
        assert_eq!(focused(&ui), Some(cell), "{column:?}");
    }
}

/// A row that goes while it has the focus, by an undo of its Add, leaves
/// the focus on a live cell, not in the filter, so the next undo still
/// works.
#[test]
fn undoing_an_add_keeps_the_focus_in_the_grid() {
    let mut ui = matrix_open();
    let on = |ui: &Ui| {
        ui.app()
            .session
            .controller
            .graph()
            .assignment(AssignmentId(1))
            .unwrap()
            .enabled
    };
    let before = on(&ui);
    key(&mut ui, Named::Enter);
    let graph = ui.app().session.controller.graph();
    let (hierarchy, level) = (&ui.app().session.hierarchy, &ui.app().session.level);
    let source = sources(graph, hierarchy, level).last().unwrap().clone();
    let target = targets(graph, hierarchy, level).last().unwrap().clone();
    ui.send(Message::Matrix(Event::AddOpen));
    ui.send(Message::Matrix(Event::AddSource(source)));
    ui.send(Message::Matrix(Event::AddTarget(target)));
    ui.send(Message::Matrix(Event::AddApply));
    let added = assignments(&ui).last().unwrap().0;
    ui.send(Message::Matrix(Event::Select(added, Column::Depth)));
    let _ = ui.redraw();
    assert_eq!(focused(&ui), Some(access::cell(added, Column::Depth)));
    ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);
    let _ = ui.redraw();
    let _ = ui.redraw();
    assert!(
        ui.app().session.controller.graph().assignment(added).is_none(),
        "the Add is undone"
    );
    assert!(
        focused(&ui).is_some_and(|id| id.is_under(&panel::list_id())),
        "the focus is on a live cell: {:?}",
        focused(&ui)
    );
    ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);
    let _ = ui.redraw();
    assert_eq!(on(&ui), before, "the next undo works");
}
