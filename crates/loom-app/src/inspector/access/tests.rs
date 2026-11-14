use super::*;

/// What shows when no one module is selected: the number selected, or
/// nothing at all.
#[test]
fn several_and_none() {
    let several = items(&Inspector::Several(3));
    assert_eq!(several.len(), 1);
    assert_eq!(several[0].kind, Kind::Text);
    assert_eq!(several[0].value.as_deref(), Some("3 modules selected"));
    assert!(items(&Inspector::Empty).is_empty());
}

/// With nothing selected the panel is the patch's own: its name, the
/// authors field (read only), the tag field, the licence, and the help.
#[test]
fn the_patch_inspector() {
    let app = crate::tests::app();
    let model = app.inspector_model();
    let panel = items(&model);
    assert_eq!(panel.len(), 1);
    let all = panel[0].walk();
    let authors = all
        .iter()
        .find(|item| item.id == ids::part(&ids::patch(), "authors"))
        .unwrap();
    assert_eq!(authors.kind, Kind::Field);
    assert!(authors.set_text.is_none(), "read only until phase 3");
    assert!(authors.focus.is_some());
    let tags = all
        .iter()
        .find(|item| item.id == ids::part(&ids::patch(), "tags"))
        .unwrap();
    assert!(matches!(tags.set_text, Some(SetText::Tag)));
    for name in ["choose_licence", "shortcuts"] {
        let button = all
            .iter()
            .find(|item| item.id == ids::part(&ids::patch(), name))
            .unwrap();
        assert_eq!(button.kind, Kind::Button);
        assert!(button.click.is_some());
    }
}

/// A sub-patch tile's port tile shows its name, direction, and what it is.
#[test]
fn a_port_tile() {
    let rows = super::super::PortRows {
        id: 12,
        direction: "Patch input".into(),
        name: "pitch".into(),
    };
    let panel = items(&Inspector::Port(rows));
    assert_eq!(panel[0].label.as_deref(), Some("pitch"));
    assert_eq!(panel[0].children.len(), 2);
    assert!(panel[0].children.iter().all(|item| item.kind == Kind::Text));
}

/// A filter's inspector, selected in a fresh app: the harness and the
/// model the view draws.
fn filter_inspector() -> (crate::ui_test::Ui, super::super::ModuleRows) {
    let mut app = crate::tests::app();
    let id = app.session.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Select(vec![id.0])));
    let Inspector::Module(rows) = ui.app().inspector_model() else {
        panic!("a module's inspector");
    };
    (ui, rows)
}

/// The item of the row at `at` in `rows`, once `change` has been made to
/// its model.
fn row_item(mut rows: super::super::ModuleRows, at: usize, change: impl FnOnce(&mut ParamRow)) -> Item {
    change(&mut rows.params[at]);
    let ui = rows.params[at].ui.clone();
    let panel = items(&Inspector::Module(rows));
    crate::a11y::items::find(&panel, &ui).expect("the row").clone()
}

/// What must be heard goes in a name or a value, not a description
/// (ui-layer.md, section 7, rule 26): a parameter not used in this mode
/// says so in its row's name, and a typed value that is not one is a text
/// part of its own after the field.
#[test]
fn a_rows_note_and_a_fields_error_are_heard() {
    let (_ui, rows) = filter_inspector();
    let row = row_item(rows.clone(), 0, |p| p.unused = Some("Not used in AD".into()));
    let name = row.label.as_deref().unwrap();
    assert!(name.ends_with(", Not used in AD"), "{name}");
    assert!(!row.description.as_deref().unwrap_or("").contains("Not used"));

    let row = row_item(rows, 0, |p| {
        p.field = Some(super::super::ValueField {
            text: "abc".into(),
            error: Some("Not a number".into()),
        });
    });
    let at = row
        .children
        .iter()
        .position(|item| item.kind == Kind::Field)
        .expect("the field");
    let error = &row.children[at + 1];
    assert_eq!(error.kind, Kind::Text);
    assert_eq!(error.value.as_deref(), Some("Not a number"));
}

/// The Range choice shows "Full" while no listed range holds the value,
/// and a step up from there picks the first range, as a pointer can.
#[test]
fn a_range_steps_up_out_of_full() {
    let (_ui, rows) = filter_inspector();
    let index = rows.params[0].index;
    let row = row_item(rows, 0, |p| {
        p.range = Some(super::super::RangeRow {
            choices: vec!["-0.1 to 0.1".into(), "-1 to 1".into()],
            selected: None,
        });
    });
    let range = row
        .children
        .iter()
        .find(|item| item.label.as_deref() == Some("Range"))
        .expect("the Range choice");
    assert_eq!(range.value.as_deref(), Some("Full"));
    assert!(matches!(
        range.step.0,
        Some(crate::Message::Inspector(Message::Range { index: i, range: 0, .. })) if i == index
    ));
    assert!(range.step.1.is_none(), "nothing below Full");
}

/// The Part of a slot, the one picked from `item`'s step, as the arrow
/// keys pick it: up when `up`, down otherwise.
fn part_step(item: &Item, up: bool) -> Option<crate::slots::SwapTarget> {
    let step = if up { &item.step.0 } else { &item.step.1 };
    match step {
        Some(AppMessage::SwapSlot { to, .. }) => Some(to.clone()),
        _ => None,
    }
}

/// The inspector's Part item for the selected slot `slot`.
fn part_item(ui: &crate::ui_test::Ui, slot: u32) -> Item {
    let panel = items(&ui.app().inspector_model());
    crate::a11y::items::find(&panel, &ids::part(&ids::panel(slot), "part"))
        .expect("the Part item")
        .clone()
}

/// The Part list steps with the arrow keys down to `No part` and back up:
/// a step down from the first part removes it as one undo step, the item
/// keeps its UI ID and says `No part`, and a step up from there swaps the
/// first part in (sub-patches.md, Empty slots).
#[test]
fn the_part_list_steps_down_to_no_part_and_back() {
    let app = crate::tests::app_with_text(
        r#"Patch(
    version: 1,
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 2, kind: "loom.filter.ladder@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(2)))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (3, "in")),
    ],
)"#,
    );
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Select(vec![9])));
    let filled = part_item(&ui, 9);
    assert_eq!(filled.value.as_deref(), Some("Ladder"));
    let Inspector::Slot(rows) = ui.app().inspector_model() else {
        panic!("a slot's inspector");
    };
    assert_eq!(
        rows.choices[0].to,
        crate::slots::SwapTarget::Empty,
        "the list starts with No part"
    );
    let first_part = rows.choices[1].to.clone();
    assert_eq!(
        first_part,
        crate::slots::SwapTarget::Module("loom.filter.ladder@1".into()),
        "the Ladder is the first part"
    );
    assert_eq!(
        part_step(&filled, false),
        Some(crate::slots::SwapTarget::Empty),
        "down from the first part is No part"
    );

    ui.send(crate::Message::SwapSlot {
        slot: 9,
        to: crate::slots::SwapTarget::Empty,
    });
    ui.app_mut().process(600);
    assert!(
        ui.app()
            .session
            .hierarchy
            .part_of_slot(loom_core::ModuleId(9))
            .is_none()
    );
    assert_eq!(ui.app().ui_snapshot().undo.as_deref(), Some("remove part from Filter"));
    let empty = part_item(&ui, 9);
    assert_eq!(empty.id, filled.id, "the item keeps its UI ID");
    assert_eq!(empty.value.as_deref(), Some("No part"));
    assert_eq!(part_step(&empty, false), None, "nothing below No part");
    assert_eq!(part_step(&empty, true), Some(first_part), "up swaps the first part in");
}
