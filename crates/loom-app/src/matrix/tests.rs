use super::*;
use crate::ui_test::Ui;
use iced::{Point, keyboard::key::Named};

/// Keyboard navigation keeps every row within the actual table viewport.
#[test]
fn keyboard_reveals_rows_in_an_overflowing_table() {
    use iced::advanced::widget::{Id, Operation, operation::Scrollable};
    let mut ui = Ui::open("modulation.ron");
    let assignment = *ui
        .app
        .controller
        .graph()
        .assignment(AssignmentId(1))
        .expect("assignment");
    for index in 0..30 {
        ui.app
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
            ui.app.matrix.selected.map(|id| id.0)
        );
        translation
    };
    for _ in 0..32 {
        ui.tap(keyboard::Key::Named(Named::ArrowDown), keyboard::Modifiers::default());
        check(&mut ui);
    }
    assert_eq!(ui.app.matrix.selected, Some(AssignmentId(32)));
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
    assert_eq!(ui.app.matrix.selected, Some(AssignmentId(1)));
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
        let selected = ui.app.matrix.selected.expect("visible selection");
        assert!(
            ui.app
                .matrix
                .model(ui.app.controller.graph(), &ui.app.selection)
                .rows
                .iter()
                .any(|row| row.id == selected.0)
        );
        check(&mut ui);
    }
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
    assert!(ui.app.alerts.status().contains("already modulates"));
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
    // Polarity comes right after the target, as on the card.
    key(&mut ui, Named::ArrowRight);
    assert_eq!(ui.app.matrix.cell, Column::Polarity);
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
    // Pressing Add again keeps the picks.
    ui.app.matrix_event(Event::AddOpen);
    let add = ui.app.matrix.add.as_ref().expect("add picks");
    assert!(add.source.is_some() && add.target.is_some(), "Add again keeps the form");
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
    // Depth is the last cell: on, source, target, polarity, depth.
    for _ in 0..4 {
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
    assert!(ui.app.alerts.status().contains("already modulates"));
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

/// Every part of every card fits the right-hand column: a 24-character
/// source shows in full, there are no column titles or row marker
/// text, and the snapshot says which tab is open (#456).
#[test]
fn cards_fit_the_column() {
    let mut ui = Ui::open("modulation.ron");
    let hold = ui.app.controller.add_module("loom.rand.sample-hold").unwrap();
    let svf = ModuleId(4);
    ui.app
        .controller
        .add_assignment(OutputRef { module: hold, port: 0 }, svf, 1, 0.25)
        .unwrap();
    ui.app.matrix_event(Event::Tab(Tab::Matrix));
    ui.app.matrix_event(Event::Select(AssignmentId(1), Column::Polarity));
    assert_eq!(ui.app.ui_snapshot().matrix.tab, Tab::Matrix);

    let source = format!("Sample and Hold #{}: Out", hold.0);
    assert_eq!(source.len(), 24);
    let column = ui.app.matrix.model(ui.app.controller.graph(), &ui.app.selection);
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
