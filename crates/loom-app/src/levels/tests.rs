use crate::inspector::Inspector;
use crate::ui_test::Ui;
use crate::{App, Loaded, Message, Source};
use iced::keyboard::{self, Modifiers, key::Named};
use iced::{Event, Point, mouse};
use loom_core::{EngineConfig, ModuleId};
use loom_patch::{ModulePath, Patch};

/// A patch with one sub-patch, Voice: a pitch port into a sine, out
/// through an out port, into the audio output.
const NESTED: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Nested"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch"), position: Some((0.0, 0.0))),
                PatchModule(id: 2, kind: "loom.osc.sine@1", position: Some((200.0, 0.0))),
                PatchModule(id: 3, kind: "loom.struct.output@1", label: Some("out"), position: Some((400.0, 0.0))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "phase")),
                PatchCable(from: (2, "out"), to: (3, "in")),
            ],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), position: Some((200.0, 0.0))),
        PatchModule(id: 3, kind: "loom.io.audio-out@1", position: Some((400.0, 0.0))),
        PatchModule(id: 4, kind: "loom.clock.transport@1", position: Some((0.0, 200.0))),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// The app with [`NESTED`] open, no audio.
fn nested_app() -> App {
    let patch = Patch::from_ron(NESTED).unwrap();
    let loaded = Loaded::read(EngineConfig::default(), false, Source::Recovered(Box::new(patch))).unwrap();
    App::assemble(
        Err("test".to_string()),
        48_000.0,
        loaded,
        1,
        &crate::tests::app_args(),
        crate::settings::Rig::empty(),
        false,
    )
}

/// The Voice instance's path: its top-level id is 2.
fn voice() -> ModulePath {
    ModulePath::top(2)
}

/// The titles of the tiles the canvas shows.
fn titles(app: &App) -> Vec<String> {
    app.ui_snapshot().tiles.iter().map(|t| t.title.clone()).collect()
}

/// A point on a tile's header, in window coordinates.
fn header(ui: &mut Ui, id: u32) -> Point {
    let tile = ui.app.session.scene.module(id).expect("the tile is on the canvas");
    let world = Point::new(tile.position.x + tile.width() / 2.0, tile.position.y + 8.0);
    ui.canvas().to_window(world)
}

/// At the top level the sub-patch is one tile with its name and ports;
/// the inner sine is not drawn, and the breadcrumb is empty.
#[test]
fn the_top_level_shows_the_sub_patch_as_one_tile() {
    let app = nested_app();
    let snapshot = app.ui_snapshot();
    assert!(snapshot.level.is_empty());
    let voice = snapshot
        .tiles
        .iter()
        .find(|t| t.title == "Voice")
        .expect("the Voice tile");
    assert_eq!(voice.id, 2);
    assert_eq!(
        voice.inputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["pitch"]
    );
    assert_eq!(
        voice.outputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["out"]
    );
    assert!(!titles(&app).iter().any(|t| t == "Sine"));
    assert_eq!(snapshot.cables.len(), 2, "note to Voice, Voice to the output");
}

/// A double-click on the sub-patch's header shows its inside: the port
/// tiles, the sine, and the cables through the ports; the breadcrumb names
/// the level. Escape goes back up, and so does the Patch crumb.
#[test]
fn opening_a_sub_patch_shows_its_inside_and_the_breadcrumb() {
    let mut ui = Ui::with_app(nested_app());
    let at = header(&mut ui, 2);
    ui.move_to(at);
    ui.press();
    ui.release();
    ui.press();
    ui.release();
    ui.redraw();
    assert_eq!(ui.app.session.level, voice());
    let snapshot = ui.app.ui_snapshot();
    assert_eq!(snapshot.level, ["Voice"]);
    let mut shown = titles(&ui.app);
    shown.sort();
    assert_eq!(shown, ["Sine", "out", "pitch"]);
    assert_eq!(snapshot.cables.len(), 2);
    assert!(ui.text("Voice").is_some(), "the breadcrumb shows the level");
    // No drag was left open by the double-click: a move changes nothing.
    ui.move_to(Point::new(at.x + 40.0, at.y + 40.0));
    ui.redraw();
    assert_eq!(ui.app.session.level, voice());

    // Escape, with nothing selected, goes up one level.
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(Vec::new())));
    ui.tap(keyboard::Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(ui.app.session.level, ModulePath::root());
    assert!(ui.app.ui_snapshot().level.is_empty());

    // The breadcrumb's Patch crumb goes up too.
    ui.send(Message::OpenLevel(voice()));
    assert_eq!(ui.app.session.level, voice());
    ui.click_text("Patch");
    assert_eq!(ui.app.session.level, ModulePath::root());
}

/// Ctrl+Enter (Cmd+Enter) with one sub-patch selected opens it; with a
/// plain module selected, nothing happens.
#[test]
fn ctrl_enter_opens_the_selected_sub_patch() {
    let mut ui = Ui::with_app(nested_app());
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![1])));
    ui.tap(keyboard::Key::Named(Named::Enter), Modifiers::COMMAND);
    assert_eq!(ui.app.session.level, ModulePath::root());
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![2])));
    ui.tap(keyboard::Key::Named(Named::Enter), Modifiers::COMMAND);
    assert_eq!(ui.app.session.level, voice());
    assert!(
        ui.app.session.selection.is_empty(),
        "the selection belongs to the level it was made on"
    );
}

/// Each level keeps its camera for the session: a zoom made inside is
/// there again after going up and coming back, and the top keeps its own.
#[test]
fn each_level_keeps_its_camera() {
    let mut ui = Ui::with_app(nested_app());
    let top = ui.canvas().camera;
    ui.send(Message::OpenLevel(voice()));
    ui.redraw();
    let framed = ui.canvas().camera;
    // Zoom in over the canvas.
    let centre = ui.canvas().bounds.center();
    ui.move_to(centre);
    ui.step(&[Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: 3.0 },
    })]);
    ui.redraw();
    let zoomed = ui.canvas().camera;
    assert!(zoomed.zoom > framed.zoom, "{zoomed:?} vs {framed:?}");
    assert_eq!(ui.app.session.cameras.get(&voice()), Some(&zoomed));

    ui.send(Message::OpenLevel(ModulePath::root()));
    ui.redraw();
    assert_eq!(ui.canvas().camera, top, "the top keeps its camera");
    ui.send(Message::OpenLevel(voice()));
    ui.redraw();
    assert_eq!(ui.canvas().camera, zoomed, "the inside keeps its zoom");
}

/// Undo of an edit made inside a sub-patch, while the top level is shown,
/// moves the canvas inside first and then reverts it (sub-patches.md,
/// Undo across levels).
#[test]
fn undo_of_an_edit_made_inside_moves_the_canvas_inside() {
    let mut app = nested_app();
    app.show_level(&voice());
    let lfo = app.modules.iter().copied().find(|d| d.id == "loom.mod.lfo").unwrap();
    app.add_module(lfo, Point::new(100.0, 300.0));
    let added = *app.session.selection.iter().next().expect("the new module is selected");
    assert_eq!(app.session.hierarchy.level_of(ModuleId(added)), voice());
    assert_eq!(app.session.controller.graph().nodes().count(), 5);
    app.show_level(&ModulePath::root());
    assert!(!titles(&app).iter().any(|t| t == "LFO"));

    app.undo();
    assert_eq!(app.session.level, voice(), "the canvas moved inside");
    assert_eq!(app.session.controller.graph().nodes().count(), 4);
    assert!(app.session.controller.graph().node(ModuleId(added)).is_none());

    app.redo();
    assert_eq!(app.session.level, voice());
    assert_eq!(app.session.selection, [added].into_iter().collect());
    assert!(titles(&app).iter().any(|t| t == "LFO"));
}

/// The inspector shows a sub-patch tile by name with its ports, and a port
/// tile by its name and direction.
#[test]
fn the_inspector_shows_a_sub_patch_and_a_port() {
    let mut app = nested_app();
    app.session.selection = [2].into_iter().collect();
    match app.inspector_model() {
        Inspector::SubPatch(rows) => {
            assert_eq!(rows.id, 2);
            assert_eq!(rows.name, "Voice");
            assert_eq!(rows.inputs, ["pitch"]);
            assert_eq!(rows.outputs, ["out"]);
        }
        other => panic!("{other:?}"),
    }
    app.show_level(&voice());
    let port = app.session.hierarchy.id_at(&voice(), 3).unwrap();
    app.session.selection = [port.0].into_iter().collect();
    match app.inspector_model() {
        Inspector::Port(rows) => {
            assert_eq!(rows.name, "out");
            assert_eq!(rows.direction, "Patch output");
        }
        other => panic!("{other:?}"),
    }
}

/// The patch saves as it was opened after a visit inside: showing a level
/// is not an edit.
#[test]
fn visiting_a_level_is_not_an_edit() {
    let mut app = nested_app();
    let before = app.current_patch();
    app.show_level(&voice());
    app.show_level(&ModulePath::root());
    assert_eq!(app.current_patch(), before);
    assert!(app.session.edits.history.undo_step().is_none());
}
