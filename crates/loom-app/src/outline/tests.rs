use super::*;
use crate::ui_test::{Ui, key_press};
use iced::keyboard::Modifiers;

const NONE: Modifiers = Modifiers::empty();

/// The examples keep their saved module IDs.
fn row(module: u32) -> UiId {
    module_id(module)
}

/// Reads the outline's focus through the same tree a screen reader gets.
fn focused(ui: &mut Ui) -> UiId {
    ui.a11y_tree().focused().ui_id().expect("a named focus").clone()
}

/// Pushes a key by position with the letter a particular layout gave it.
fn physical(ui: &mut Ui, code: Code, letter: &str, modifiers: Modifiers) {
    let mut press = key_press(Key::Character(letter.into()), modifiers, Some(letter), false);
    if let iced::Event::Keyboard(keyboard::Event::KeyPressed { physical_key, .. }) = &mut press {
        *physical_key = Physical::Code(code);
    }
    ui.step(&[press]);
}

/// The subtractive patch reads in signal order, with monitors and Transport at the end.
#[test]
fn subtractive_outline_starts_with_the_source_and_ends_with_the_output() {
    let app = crate::ui_test::open_app("subtractive.ron");
    let rows = app.outline_rows();
    let ids: Vec<_> = rows
        .iter()
        .filter_map(|row| match row.target {
            Some(Target::Tile(id)) => Some(id),
            _ => None,
        })
        .collect();
    assert!(rows[0].name.contains("10 modules, 10 cables, no feedback loops"));
    let structure = &app.session.patch_map.structure;
    let heard: Vec<_> = ids.iter().copied().filter(|id| structure.heard.contains(id)).collect();
    assert_eq!(structure.name(heard[0]), "MIDI Note");
    assert_eq!(structure.name(heard[heard.len() - 2]), "Multiply 8");
    assert_eq!(structure.name(heard[heard.len() - 1]), "Audio Out");
    for (from, to) in structure.edges() {
        if structure.heard.contains(&from) && structure.heard.contains(&to) {
            assert!(heard.iter().position(|&i| i == from) < heard.iter().position(|&i| i == to));
        }
    }
    assert!(
        rows.iter()
            .any(|row| row.name == "Not heard, 2 modules" && row.expanded == Some(true))
    );
    assert!(rows.iter().any(|row| row.name.starts_with("Scope") && row.level == 2));
    assert!(
        rows.iter()
            .any(|row| row.name.starts_with("Transport") && row.level == 2)
    );
    assert!(
        rows.iter()
            .any(|row| row.id == row_id_for_saw(&app) && row.expanded == Some(false))
    );
}

/// Finds the Saw by its title so the test still follows it if its local ID changes.
fn row_id_for_saw(app: &App) -> UiId {
    row(tile_named(app, "Saw"))
}

/// Finds a tile by the shared title.
fn tile_named(app: &App, title: &str) -> u32 {
    app.session
        .patch_map
        .structure
        .tiles
        .iter()
        .find(|(_, tile)| tile.name == title)
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("no {title} tile"))
}

/// The same physical keys trace a cable and go back when they type German characters.
#[test]
fn physical_brackets_follow_a_cable_and_return_to_the_start() {
    let mut ui = Ui::open("subtractive.ron");
    let saw = tile_named(ui.app(), "Saw");
    let filter = tile_named(ui.app(), "SVF");
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![saw])));
    let initial = ui.app().session.edits.history.len();
    physical(&mut ui, Code::BracketRight, "+", NONE);
    assert_eq!(ui.app().session.selection, [filter].into());
    ui.redraw();
    let canvas = ui.canvas();
    let centre = ui.app().session.scene.module(filter).unwrap().bounds().center();
    assert!((canvas.camera.visible(canvas.bounds.size()).center() - centre).x.abs() < 0.01);
    physical(&mut ui, Code::BracketRight, "+", NONE);
    assert!(ui.app().session.outline.has_jump());
    assert_eq!(ui.a11y_tree().focused().role(), accesskit::Role::ListBoxOption);
    ui.tap(Key::Named(Named::Escape), NONE);
    physical(&mut ui, Code::BracketLeft, "ü", Modifiers::COMMAND);
    assert_eq!(ui.app().session.selection, [saw].into());
    assert_eq!(ui.app().session.edits.history.len(), initial);
}

/// Outline navigation preserves focus by row ID and keeps edit keys away from the canvas.
#[test]
fn outline_keyboard_moves_and_expands_rows_without_editing() {
    let mut ui = Ui::open("subtractive.ron");
    let first_count = ui.app().session.scene.modules().len();
    let saw = tile_named(ui.app(), "Saw");
    ui.tap(Key::Character("o".into()), NONE);
    assert_eq!(focused(&mut ui), tree_id().with("summary"));
    ui.tap(Key::Named(Named::ArrowDown), NONE);
    assert_eq!(focused(&mut ui), row(tile_named(ui.app(), "MIDI Note")));
    ui.tap(Key::Character("s".into()), NONE);
    assert_eq!(focused(&mut ui), row(saw));
    ui.tap(Key::Named(Named::ArrowRight), NONE);
    ui.tap(Key::Named(Named::ArrowRight), NONE);
    assert!(focused(&mut ui).is_under(&row(saw)));
    ui.tap(Key::Named(Named::ArrowLeft), NONE);
    assert_eq!(focused(&mut ui), row(saw));
    ui.tap(Key::Named(Named::Space), NONE);
    ui.tap(Key::Named(Named::Delete), NONE);
    assert_eq!(ui.app().session.scene.modules().len(), first_count);
    ui.tap(Key::Named(Named::Escape), NONE);
    assert_eq!(ui.app().right_tab, crate::matrix::Tab::Outline);
    ui.tap(Key::Named(Named::Tab), Modifiers::CTRL);
    assert_eq!(focused(&mut ui), row(saw));
}

/// Map visibility cycles from the canvas and the map takes clicks over tiles.
#[test]
fn map_visibility_and_clicks_use_the_live_camera() {
    let mut ui = Ui::open("subtractive.ron");
    let mode = |ui: &Ui| ui.app().rig.store.saved().patch_map;
    use crate::preferences::MapMode;
    assert_eq!(mode(&ui), MapMode::Auto);
    ui.tap(Key::Character("m".into()), NONE);
    let canvas = ui.canvas();
    let area = ui
        .app()
        .map_view()
        .unwrap()
        .bounds(&ui.app().session.scene, canvas.camera, canvas.bounds)
        .unwrap();
    assert_eq!(mode(&ui), MapMode::Always);
    assert_eq!(ui.a11y_tree().node("Patch map").role(), accesskit::Role::Image);
    ui.move_to(Point::new(area.x + 2.0, area.y + 2.0));
    ui.press();
    ui.release();
    assert_ne!(ui.canvas().camera.origin, canvas.camera.origin);
    ui.tap(Key::Character("m".into()), NONE);
    assert_eq!(mode(&ui), MapMode::Hidden);
    assert!(ui.app().map_view().is_none());
    ui.tap(Key::Character("m".into()), NONE);
    assert_eq!(mode(&ui), MapMode::Auto);
}

/// Ready tiles are chosen by reading order as soon as their last feeder is placed.
#[test]
fn a_newly_ready_tile_can_precede_an_independent_source() {
    use loom_core::{InputRef, OutputRef};
    let mut app = crate::tests::app();
    let controller = &mut app.session.controller;
    let a = controller.add_module("loom.osc.sine").unwrap();
    let b = controller.add_module("loom.filter.svf").unwrap();
    let c = controller.add_module("loom.osc.sine").unwrap();
    controller
        .connect(OutputRef { module: a, port: 0 }, InputRef { module: b, port: 0 })
        .unwrap();
    for (id, x) in [(a, 0.0), (b, 200.0), (c, 400.0)] {
        app.session.positions.insert(id.0, (x, 0.0));
    }
    app.commit();
    let wanted: Vec<_> = app
        .session
        .patch_map
        .order
        .iter()
        .copied()
        .filter(|unit| matches!(unit, Unit::Tile(id) if [a.0, b.0, c.0].contains(id)))
        .collect();
    assert_eq!(wanted, [Unit::Tile(a.0), Unit::Tile(b.0), Unit::Tile(c.0)]);
}

/// A loop is one expanded row, whose children follow the compiler's run order.
#[test]
fn feedback_loops_group_their_members_and_report_the_inserted_delay() {
    let app = crate::ui_test::open_app("fm-feedback.ron");
    let rows = app.outline_rows();
    let group = app
        .session
        .patch_map
        .structure
        .loops
        .first()
        .expect("one feedback loop");
    let id = loop_id(group);
    let row = rows.iter().find(|row| row.id == id).unwrap();
    assert!(row.name.starts_with("Feedback loop 1:"));
    assert_eq!(row.expanded, Some(true));
    assert!(row.description.starts_with("Delay inserted"), "{}", row.description);
    let kids: Vec<_> = rows
        .iter()
        .filter(|row| row.parent.as_ref() == Some(&id))
        .map(|row| row.id.clone())
        .collect();
    assert_eq!(kids, group.members.iter().copied().map(module_id).collect::<Vec<_>>());
}

/// Dragging the map's viewport pans the canvas and leaves patch history intact.
#[test]
fn the_map_viewport_can_be_dragged_to_pan() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Preferences(crate::settings::Message::Map(Some(
        crate::preferences::MapMode::Always,
    ))));
    let view = ui.canvas();
    let map = ui.app().map_view().unwrap();
    let area = map.bounds(&ui.app().session.scene, view.camera, view.bounds).unwrap();
    let at = area.center();
    let steps = ui.app().session.edits.history.len();
    ui.drag(at, Point::new(at.x + 10.0, at.y), 3);
    let moved = ui.canvas().camera;
    assert!(moved.origin.x > view.camera.origin.x);
    assert_eq!(moved.zoom, view.camera.zoom);
    assert_eq!(ui.app().session.edits.history.len(), steps);
}
