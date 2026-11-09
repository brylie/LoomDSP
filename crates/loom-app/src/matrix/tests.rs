use super::*;
use crate::ui_test::Ui;
use iced::keyboard::key::Named;
use loom_ui_core::Point;

/// Keyboard navigation keeps every row within the actual table viewport.
#[test]
fn keyboard_reveals_rows_in_an_overflowing_table() {
    use iced::advanced::widget::{Id, Operation, operation::Scrollable};
    let mut ui = Ui::open("modulation.ron");
    let assignment = *ui
        .app
        .session
        .controller
        .graph()
        .assignment(AssignmentId(1))
        .expect("assignment");
    for index in 0..30 {
        ui.app
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
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    /// Captures the table viewport and the selected row from the view.
    struct Bounds(
        Option<(iced::Rectangle, iced::Vector)>,
        Option<iced::Rectangle>,
        Option<iced::Rectangle>,
    );
    impl Operation for Bounds {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn Operation)) {
            f(self);
        }
        fn container(&mut self, id: Option<&Id>, bounds: iced::Rectangle) {
            if id == Some(&Id::new("matrix-selected-row")) {
                self.1 = Some(bounds);
            } else if id == Some(&Id::new("matrix-selected-cell")) {
                self.2 = Some(bounds);
            }
        }
        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: iced::Rectangle,
            _: iced::Rectangle,
            translation: iced::Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&Id::new("matrix-table")) {
                self.0 = Some((bounds, translation));
            }
        }
    }
    let before: Vec<_> = ui
        .app
        .session
        .controller
        .graph()
        .assignments()
        .map(|(id, a)| (id, *a))
        .collect();
    let check = |ui: &mut Ui| {
        let mut bounds = Bounds(None, None, None);
        ui.with_ui(|view, renderer, _| view.operate(renderer, &mut bounds));
        let (viewport, translation) = bounds.0.expect("viewport");
        let row = bounds.1.expect("selected row");
        assert!(row.y - translation.y >= viewport.y - 1.0);
        assert!(row.y + row.height - translation.y <= viewport.y + viewport.height + 1.0);
        let cell = bounds.2.expect("selected cell");
        assert!(cell.x - translation.x >= viewport.x - 1.0);
        assert!(cell.x + cell.width - translation.x <= viewport.x + viewport.width + 1.0);
        assert_eq!(
            ui.app.ui_snapshot().matrix.selected,
            ui.app.session.matrix.selected.map(|id| id.0)
        );
        translation
    };
    for _ in 0..32 {
        ui.tap(keyboard::Key::Named(Named::ArrowDown), keyboard::Modifiers::default());
        check(&mut ui);
    }
    assert_eq!(ui.app.session.matrix.selected, Some(AssignmentId(32)));
    // Every cell of a card fits the column, so moving between cells
    // never scrolls sideways (#456).
    for _ in 0..4 {
        ui.tap(keyboard::Key::Named(Named::ArrowRight), keyboard::Modifiers::default());
        assert_eq!(check(&mut ui).x, 0.0, "no sideways scroll");
    }
    for _ in 0..31 {
        ui.tap(keyboard::Key::Named(Named::ArrowUp), keyboard::Modifiers::default());
        assert_eq!(check(&mut ui).x, 0.0, "no sideways scroll");
    }
    assert_eq!(ui.app.session.matrix.selected, Some(AssignmentId(1)));
    ui.send(Message::Matrix(Event::SortBy(Some(Column::Depth))));
    check(&mut ui);
    ui.send(Message::Matrix(Event::ReverseSort));
    check(&mut ui);
    ui.send(Message::Matrix(Event::Filter("LFO".into())));
    check(&mut ui);
    ui.send(Message::Matrix(Event::SelectedOnly(true)));
    check(&mut ui);
    for _ in 0..31 {
        ui.tap(keyboard::Key::Named(Named::ArrowDown), keyboard::Modifiers::default());
        let selected = ui.app.session.matrix.selected.expect("visible selection");
        assert!(
            ui.app
                .session
                .matrix
                .model(
                    ui.app.session.controller.graph(),
                    &ui.app.session.selection,
                    ui.app.right_tab
                )
                .rows
                .iter()
                .any(|row| row.id == selected.0)
        );
        check(&mut ui);
    }
    assert_eq!(
        before,
        ui.app
            .session
            .controller
            .graph()
            .assignments()
            .map(|(id, a)| (id, *a))
            .collect::<Vec<_>>()
    );
    assert!(ui.app.session.edits.history.undo_step().is_none());
}

/// Visible rows follow engine order, sorting, text, and module filters
/// without changing the graph or adding an undo step.
#[test]
fn rows_sort_and_filter_without_editing_assignments() {
    let mut ui = Ui::open("modulation.ron");
    let before: Vec<_> = ui
        .app
        .session
        .controller
        .graph()
        .assignments()
        .map(|(id, a)| (id, *a))
        .collect();
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    let model = |ui: &Ui| {
        ui.app.session.matrix.model(
            ui.app.session.controller.graph(),
            &ui.app.session.selection,
            ui.app.right_tab,
        )
    };
    assert_eq!(model(&ui).rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1, 2]);
    ui.app.matrix_event(Event::SortBy(Some(Column::Depth)));
    assert_eq!(model(&ui).rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1, 2]);
    ui.app.matrix_event(Event::ReverseSort);
    assert_eq!(model(&ui).rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![2, 1]);
    for column in Column::ALL {
        ui.app.matrix_event(Event::SortBy(Some(column)));
        assert_eq!(model(&ui).rows.len(), 2, "{} ascending", column.label());
        ui.app.matrix_event(Event::ReverseSort);
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
            .session
            .controller
            .graph()
            .assignments()
            .map(|(id, a)| (id, *a))
            .collect::<Vec<_>>()
    );
    assert!(ui.app.session.edits.history.undo_step().is_none());
}

/// Retargeting keeps the same id and fields, refuses duplicates, and
/// undo restores the old endpoint in engine order.
#[test]
fn retarget_keeps_id_order_and_one_undo_step() {
    let mut ui = Ui::open("modulation.ron");
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    let id = AssignmentId(1);
    let original = *ui
        .app
        .session
        .controller
        .graph()
        .assignment(id)
        .expect("first assignment");
    let target = targets(ui.app.session.controller.graph())
        .into_iter()
        .find(|t| t.module == ModuleId(4) && t.param != original.param)
        .expect("another filter parameter");
    ui.app.matrix_event(Event::Target(id, target));
    let changed = *ui
        .app
        .session
        .controller
        .graph()
        .assignment(id)
        .expect("retargeted assignment");
    assert_ne!(changed.param, original.param);
    assert_eq!(changed.depth, original.depth);
    assert_eq!(changed.polarity, original.polarity);
    assert_eq!(changed.enabled, original.enabled);
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignments()
            .map(|(id, _)| id.0)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    ui.app.undo();
    assert_eq!(ui.app.session.controller.graph().assignment(id), Some(&original));
    ui.app.redo();
    assert_eq!(ui.app.session.controller.graph().assignment(id), Some(&changed));
    let duplicate = sources(ui.app.session.controller.graph())
        .into_iter()
        .find(|s| s.output.module == ModuleId(6))
        .expect("envelope output");
    ui.app.matrix_event(Event::Target(
        id,
        targets(ui.app.session.controller.graph())
            .into_iter()
            .find(|t| t.module == ModuleId(4) && t.param == original.param)
            .expect("cutoff"),
    ));
    ui.app.matrix_event(Event::Source(id, duplicate));
    assert!(ui.app.alerts.status().contains("already modulates"));
    assert_eq!(
        ui.app
            .session
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
    assert!(ui.app.session.matrix.edit.is_some());
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .depth,
        0.08
    );
    ui.app.matrix_event(Event::DepthText("-35%".into()));
    ui.app.matrix_event(Event::Apply);
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .depth,
        -0.35
    );
    ui.app.undo();
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .depth,
        0.08
    );
    ui.app.matrix_event(Event::Edit(id, Column::Depth));
    ui.app.matrix_event(Event::DepthText("90".into()));
    ui.app.matrix_event(Event::Cancel);
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .depth,
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
    assert_eq!(ui.app.right_tab, Tab::Matrix);
    let key = |ui: &mut Ui, named| {
        ui.tap(keyboard::Key::Named(named), keyboard::Modifiers::default());
    };
    key(&mut ui, Named::ArrowDown);
    assert_eq!(ui.app.session.matrix.selected, Some(AssignmentId(1)));
    key(&mut ui, Named::ArrowRight);
    key(&mut ui, Named::ArrowRight);
    assert_eq!(ui.app.session.matrix.cell, Column::Target);
    key(&mut ui, Named::Enter);
    assert_eq!(
        ui.app.session.matrix.edit.as_ref().map(|e| e.column),
        Some(Column::Target)
    );
    key(&mut ui, Named::Escape);
    assert!(ui.app.session.matrix.edit.is_none());
    key(&mut ui, Named::Delete);
    assert!(ui.app.session.controller.graph().assignment(AssignmentId(1)).is_none());
    ui.app.undo();
    assert!(ui.app.session.controller.graph().assignment(AssignmentId(1)).is_some());
    key(&mut ui, Named::Tab);
    assert!(!ui.app.session.matrix.focused);
}

/// Matrix focus leaves command shortcuts available after an edit.
#[test]
fn command_shortcuts_undo_and_redo_while_matrix_is_focused() {
    let mut ui = Ui::open("modulation.ron");
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    let id = AssignmentId(1);
    let original = ui
        .app
        .session
        .controller
        .graph()
        .assignment(id)
        .expect("assignment")
        .enabled;
    ui.app.matrix_event(Event::Enabled(id, !original));
    assert!(ui.app.session.matrix.focused);

    ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .enabled,
        original
    );

    ui.tap(
        keyboard::Key::Character("z".into()),
        keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
    );
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .enabled,
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

/// #597: the same for a depth field opened with the pointer. The click goes
/// through the depth cell, so the field still takes focus by a task.
#[test]
fn cmd_v_in_a_depth_field_opened_by_pointer_stays_out_of_the_canvas() {
    let mut ui = Ui::open("modulation.ron");
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    ui.redraw();
    let id = ui
        .app
        .session
        .controller
        .graph()
        .assignments()
        .next()
        .expect("an assignment")
        .0;
    ui.app.matrix_event(Event::Select(id, Column::Depth));
    ui.redraw();
    let depth = {
        let assignment = ui.app.session.controller.graph().assignment(id).unwrap();
        crate::modulation::depth_text(assignment.depth)
    };
    let cell = ui.text(&depth).expect("the depth cell shows its value");
    ui.move_to(cell.center());
    ui.press();
    ui.release();
    assert_eq!(
        ui.app.session.matrix.edit.as_ref().map(|e| e.column),
        Some(Column::Depth)
    );

    let messages = ui.tap(keyboard::Key::Character("v".into()), keyboard::Modifiers::COMMAND);
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Canvas(loom_canvas::CanvasEvent::Paste { .. }))),
        "the canvas took the paste: {messages:?}"
    );
}

/// A canvas click returns keyboard control to the canvas before Delete.
#[test]
fn canvas_click_then_delete_removes_the_module_not_a_matrix_row() {
    let mut ui = Ui::open("modulation.ron");
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    ui.app.matrix_event(Event::Select(AssignmentId(1), Column::On));
    let tile = ui.app.session.scene.module(3).expect("saw tile").bounds();
    let at = ui
        .canvas()
        .to_window(loom_ui_core::Point::new(tile.center().x, tile.y + 10.0));
    ui.move_to(at);
    ui.press();
    ui.release();
    assert!(!ui.app.session.matrix.focused);
    assert!(ui.app.session.selection.contains(&3));

    ui.tap(keyboard::Key::Named(Named::Delete), keyboard::Modifiers::default());
    assert!(ui.app.session.controller.graph().node(ModuleId(3)).is_none());
    assert_eq!(ui.app.session.controller.graph().assignments().count(), 2);
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
    assert!(
        !ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .enabled
    );
    ui.app.undo();
    assert!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .enabled
    );
    key(&mut ui, Named::ArrowRight);
    key(&mut ui, Named::Enter);
    let before = ui
        .app
        .session
        .controller
        .graph()
        .assignment(id)
        .expect("assignment")
        .source;
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::Enter);
    assert_ne!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .source,
        before
    );
    key(&mut ui, Named::ArrowRight);
    key(&mut ui, Named::Enter);
    let before_target = ui
        .app
        .session
        .controller
        .graph()
        .assignment(id)
        .expect("assignment")
        .param;
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::Enter);
    assert_ne!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .param,
        before_target
    );
    // Polarity comes right after the target, as on the card.
    key(&mut ui, Named::ArrowRight);
    assert_eq!(ui.app.session.matrix.cell, Column::Polarity);
    key(&mut ui, Named::Enter);
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::Enter);
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(id)
            .expect("assignment")
            .polarity,
        Some(loom_core::Polarity::Unipolar)
    );

    ui.app.matrix_event(Event::AddOpen);
    key(&mut ui, Named::ArrowDown);
    key(&mut ui, Named::ArrowRight);
    key(&mut ui, Named::ArrowDown);
    let add = ui.app.session.matrix.add.as_ref().expect("add picks");
    assert!(add.source.is_some() && add.target.is_some());
    // Pressing Add again keeps the picks.
    ui.app.matrix_event(Event::AddOpen);
    let add = ui.app.session.matrix.add.as_ref().expect("add picks");
    assert!(add.source.is_some() && add.target.is_some(), "Add again keeps the form");
    let before_count = ui.app.session.controller.graph().assignments().count();
    key(&mut ui, Named::Enter);
    assert_eq!(
        ui.app.session.controller.graph().assignments().count(),
        before_count + 1
    );
    let added = ui
        .app
        .session
        .controller
        .graph()
        .assignments()
        .last()
        .expect("new assignment")
        .1;
    let spec = ui
        .app
        .session
        .controller
        .graph()
        .node(added.target)
        .expect("target")
        .spec(added.param)
        .expect("parameter");
    assert_eq!(added.depth, modulation::default_depth(&spec));
    ui.app.undo();
    assert_eq!(ui.app.session.controller.graph().assignments().count(), before_count);
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
    // Depth is the last cell: on, source, target, polarity, depth.
    for _ in 0..4 {
        key(&mut ui, Named::ArrowRight);
    }
    assert_eq!(ui.app.session.matrix.cell, Column::Depth);
    key(&mut ui, Named::Enter);
    assert_eq!(
        ui.app.session.matrix.edit.as_ref().map(|e| e.column),
        Some(Column::Depth)
    );
    ui.typewrite("42");
    assert_eq!(ui.app.session.matrix.edit.as_ref().expect("depth editor").depth, "42");
    key(&mut ui, Named::Enter);
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(AssignmentId(1))
            .expect("assignment")
            .depth,
        0.42
    );
    assert!(ui.app.session.matrix.edit.is_none(), "Enter leaves the depth editor");
    ui.app.undo();
    assert_eq!(
        ui.app
            .session
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
        .session
        .controller
        .graph()
        .assignment(AssignmentId(1))
        .expect("assignment");
    let source = sources(ui.app.session.controller.graph())
        .into_iter()
        .find(|s| s.output == existing.source)
        .expect("source");
    let target = targets(ui.app.session.controller.graph())
        .into_iter()
        .find(|t| (t.module, t.param) == (existing.target, existing.param))
        .expect("target");
    ui.app.matrix_event(Event::AddOpen);
    ui.app.matrix_event(Event::AddSource(source));
    ui.app.matrix_event(Event::AddTarget(target));
    ui.app.matrix_event(Event::AddApply);
    assert_eq!(ui.app.session.controller.graph().assignments().count(), 2);
    assert!(ui.app.alerts.status().contains("already modulates"));
    ui.app.matrix_event(Event::DepthDrag(AssignmentId(1), 0.4));
    ui.app.matrix_event(Event::DepthDrag(AssignmentId(1), 0.5));
    ui.app.matrix_event(Event::DepthReleased);
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .assignment(AssignmentId(1))
            .expect("assignment")
            .depth,
        0.5
    );
    ui.app.undo();
    assert_eq!(
        ui.app.session.controller.graph().assignment(AssignmentId(1)),
        Some(&existing)
    );
}

/// A new patch keeps the session tab but drops patch-specific rows and
/// selection; controller endpoint validation leaves invalid edits alone.
#[test]
fn tab_survives_patch_change_and_bad_endpoint_changes_nothing() {
    let mut ui = Ui::open("modulation.ron");
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    let id = AssignmentId(1);
    let old = *ui.app.session.controller.graph().assignment(id).expect("assignment");
    assert!(
        ui.app
            .session
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
    assert_eq!(ui.app.session.controller.graph().assignment(id), Some(&old));
    ui.app.matrix_event(Event::Select(id, Column::Source));
    assert_eq!(
        ui.app.matrix_line().expect("selected line").from,
        (old.source.module.0, old.source.port)
    );
    assert!(ui.app.replace(crate::Source::Empty));
    assert_eq!(ui.app.right_tab, Tab::Matrix);
    assert!(
        ui.app
            .session
            .matrix
            .model(
                ui.app.session.controller.graph(),
                &ui.app.session.selection,
                ui.app.right_tab
            )
            .rows
            .is_empty()
    );
    assert!(ui.app.session.matrix.selected.is_none());
}

/// Every part of every card fits the right-hand column: a 24-character
/// source shows in full, there are no column titles or row marker
/// text, and the snapshot says which tab is open (#456).
#[test]
fn cards_fit_the_column() {
    let mut ui = Ui::open("modulation.ron");
    let hold = ui.app.session.controller.add_module("loom.rand.sample-hold").unwrap();
    let svf = ModuleId(4);
    ui.app
        .session
        .controller
        .add_assignment(OutputRef { module: hold, port: 0 }, svf, 1, 0.25)
        .unwrap();
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    ui.app.matrix_event(Event::Select(AssignmentId(1), Column::Polarity));
    assert_eq!(ui.app.ui_snapshot().matrix.tab, Tab::Matrix);

    let source = format!("Sample and Hold #{}: Out", hold.0);
    assert_eq!(source.len(), 24);
    let column = ui.app.session.matrix.model(
        ui.app.session.controller.graph(),
        &ui.app.session.selection,
        ui.app.right_tab,
    );
    let right = crate::WINDOW.width;
    let left = right - 280.0;
    let mut labels = vec![source.clone()];
    for row in &column.rows {
        labels.extend([row.source.clone(), row.target.clone(), row.polarity.clone()]);
        labels.push(modulation::depth_text(row.depth));
    }
    for label in labels {
        let bounds = ui.text(&label).unwrap_or_else(|| panic!("`{label}` is not shown"));
        assert!(
            bounds.x >= left && bounds.x + bounds.width <= right,
            "`{label}` at {bounds:?} is outside the column"
        );
    }
    for gone in ["On", "Source", "Target", "Depth", "Polarity", "Row #1: Polarity cell"] {
        assert!(ui.text(gone).is_none(), "`{gone}` is still shown");
    }
}

/// Cmd+V in a depth field opened by keyboard pastes into the field only. The
/// field takes focus by a task, which sends no message, so the app must ask
/// about focus afterwards, or the canvas also pastes (clipboard.md, rule 7).
#[test]
fn cmd_v_in_a_depth_field_stays_out_of_the_canvas() {
    let mut ui = Ui::open("modulation.ron");
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    let key = |ui: &mut Ui, named| {
        ui.tap(keyboard::Key::Named(named), keyboard::Modifiers::default());
    };
    key(&mut ui, Named::ArrowDown);
    for _ in 0..4 {
        key(&mut ui, Named::ArrowRight);
    }
    key(&mut ui, Named::Enter);
    assert_eq!(
        ui.app.session.matrix.edit.as_ref().map(|e| e.column),
        Some(Column::Depth)
    );

    let messages = ui.tap(keyboard::Key::Character("v".into()), keyboard::Modifiers::COMMAND);
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Canvas(loom_canvas::CanvasEvent::Paste { .. }))),
        "the canvas took the paste: {messages:?}"
    );
}

/// An app with the matrix open and a row selected, ready for keys.
fn matrix_with_a_row() -> Ui {
    let mut ui = Ui::open("modulation.ron");
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    ui.app.matrix_event(Event::Select(AssignmentId(1), Column::On));
    ui.redraw();
    assert!(ui.app.session.matrix.focused);
    ui
}

/// Clicks the filter, as a person would.
fn click_filter(ui: &mut Ui) {
    let at = ui.field(FILTER);
    ui.move_to(at);
    ui.press();
    ui.release();
    assert_eq!(
        ui.focused_field(),
        Some(iced::advanced::widget::Id::new(FILTER)),
        "the click focused the filter"
    );
}

/// What a key must not change: the row, the cell, any edit, and the patch.
fn rows_and_cells(ui: &Ui) -> (Option<AssignmentId>, Column, bool, usize) {
    (
        ui.app.session.matrix.selected,
        ui.app.session.matrix.cell,
        ui.app.session.matrix.edit.is_some(),
        ui.app.session.controller.graph().assignments().count(),
    )
}

/// #435, modulation.md, Filter focus: keys in an empty filter leave the rows,
/// the cells, and the assignments alone, whether or not the field used them.
#[test]
fn keys_in_an_empty_filter_leave_rows_alone() {
    // A fresh app per key: Delete and Backspace send an empty filter, which
    // takes the table's focus, and the next key would then prove nothing.
    for named in [
        Named::Delete,
        Named::Backspace,
        Named::ArrowUp,
        Named::ArrowDown,
        Named::ArrowRight,
        Named::Enter,
    ] {
        let mut ui = matrix_with_a_row();
        click_filter(&mut ui);
        assert!(ui.app.session.matrix.focused, "the click left the table focused");
        let before = rows_and_cells(&ui);
        ui.tap(keyboard::Key::Named(named), keyboard::Modifiers::default());
        assert_eq!(rows_and_cells(&ui), before, "{named:?}");
        assert!(ui.text_field_focused(), "{named:?}: the field keeps focus");
    }
}

/// #435: typing and clearing changes the filter only.
#[test]
fn typing_and_clearing_a_filter_changes_only_the_filter() {
    let mut ui = matrix_with_a_row();
    let count = ui.app.session.controller.graph().assignments().count();
    let undo = ui.app.session.edits.history.undo_step().map(|s| s.name().to_string());
    click_filter(&mut ui);
    ui.typewrite("zz");
    assert_eq!(ui.app.session.matrix.filter, "zz");
    ui.tap(keyboard::Key::Named(Named::Backspace), keyboard::Modifiers::default());
    ui.tap(keyboard::Key::Named(Named::Backspace), keyboard::Modifiers::default());
    assert_eq!(ui.app.session.matrix.filter, "");
    assert_eq!(ui.app.session.controller.graph().assignments().count(), count);
    assert_eq!(
        ui.app.session.edits.history.undo_step().map(|s| s.name().to_string()),
        undo
    );
}

/// #435, modulation.md, Filter focus: Escape and Tab each end the field's focus
/// and leave the table, in one press. Tab depends on the order of the route:
/// the matrix lets go before the text field rule looks.
#[test]
fn escape_and_tab_leave_the_filter_and_the_table_in_one_press() {
    for named in [Named::Escape, Named::Tab] {
        let mut ui = matrix_with_a_row();
        click_filter(&mut ui);
        ui.tap(keyboard::Key::Named(named), keyboard::Modifiers::default());
        ui.redraw();
        assert!(!ui.app.session.matrix.focused, "{named:?} left the table");
        assert!(!ui.text_field_focused(), "{named:?} ended the field's focus");
    }
}

/// #435: the app's shortcuts still work while the filter has focus.
#[test]
fn command_shortcuts_work_from_the_filter() {
    let mut ui = matrix_with_a_row();
    let id = AssignmentId(1);
    let original = ui
        .app
        .session
        .controller
        .graph()
        .assignment(id)
        .expect("assignment")
        .enabled;
    ui.app.matrix_event(Event::Enabled(id, !original));
    ui.app.commit();
    ui.redraw();
    click_filter(&mut ui);
    let messages = ui.tap(keyboard::Key::Character("z".into()), keyboard::Modifiers::COMMAND);
    assert!(
        messages.iter().any(|m| matches!(m, Message::Key(_))),
        "the shortcut reached the app: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::HistoryKey { typing: true, .. })),
        "it asked about typing, and undoes nothing behind the field: {messages:?}"
    );
}

/// #435: a cell editor keeps the keys it uses, and they do not reach the rows.
#[test]
fn a_cell_editor_keeps_its_captured_keys() {
    let mut ui = matrix_with_a_row();
    ui.app.matrix_event(Event::Select(AssignmentId(1), Column::Depth));
    ui.tap(keyboard::Key::Named(Named::Enter), keyboard::Modifiers::default());
    assert_eq!(
        ui.app.session.matrix.edit.as_ref().map(|e| e.column),
        Some(Column::Depth)
    );
    let before = ui.app.session.matrix.selected;
    let count = ui.app.session.controller.graph().assignments().count();
    for named in [Named::Delete, Named::Backspace, Named::ArrowLeft] {
        ui.tap(keyboard::Key::Named(named), keyboard::Modifiers::default());
        assert!(ui.app.session.matrix.edit.is_some(), "{named:?} stayed in the edit");
        assert_eq!(ui.app.session.matrix.selected, before, "{named:?}");
        assert_eq!(
            ui.app.session.controller.graph().assignments().count(),
            count,
            "{named:?}"
        );
    }
    ui.tap(keyboard::Key::Named(Named::Escape), keyboard::Modifiers::default());
    assert!(ui.app.session.matrix.edit.is_none(), "Escape left the edit");
    assert!(ui.app.session.matrix.focused, "and stayed in the table");
}

/// #435: arrows no longer move rows behind an overlay. They used to, behind
/// help and every overlay but the browser and Preferences. The row comes back
/// to the keyboard when the overlay closes, and none of it is an undo step.
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
        let mut ui = matrix_with_a_row();
        let undo = ui.app.session.edits.history.undo_step().map(|s| s.name().to_string());
        open_overlay(&mut ui, owner);
        let before = ui.app.session.matrix.selected;
        ui.tap(keyboard::Key::Named(Named::ArrowDown), keyboard::Modifiers::default());
        assert_eq!(ui.app.session.matrix.selected, before, "{owner:?}");
        assert_eq!(
            ui.app.session.edits.history.undo_step().map(|s| s.name().to_string()),
            undo,
            "{owner:?}"
        );
    }
}

/// #435: closing the browser or Preferences gives the keys back to the table.
#[test]
fn the_table_has_the_keys_again_after_an_overlay_closes() {
    use crate::input_owner::{InputOwner, tests::open_overlay};
    for owner in [InputOwner::Browser, InputOwner::Preferences] {
        let mut ui = matrix_with_a_row();
        let before = ui.app.session.matrix.selected;
        open_overlay(&mut ui, owner);
        ui.tap(keyboard::Key::Named(Named::Escape), keyboard::Modifiers::default());
        ui.redraw();
        assert_eq!(ui.app.input_owner(), InputOwner::Canvas, "{owner:?} closed");
        ui.tap(keyboard::Key::Named(Named::ArrowDown), keyboard::Modifiers::default());
        assert_ne!(ui.app.session.matrix.selected, before, "{owner:?}: Down moved the row");
    }
}
