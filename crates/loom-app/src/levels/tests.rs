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

/// A flat patch: a note into a phase generator and a sine, into the audio
/// output, with the Transport.
const FLAT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Flat"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.phase.generator@1", position: Some((192.0, 0.0))),
        PatchModule(id: 3, kind: "loom.osc.sine@1", position: Some((384.0, 0.0))),
        PatchModule(id: 4, kind: "loom.io.audio-out@1", position: Some((576.0, 0.0))),
        PatchModule(id: 5, kind: "loom.clock.transport@1", position: Some((0.0, 240.0))),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "in")),
    ],
)"#;

/// The app with `text` open, no audio.
fn app_with(text: &str) -> App {
    let patch = Patch::from_ron(text).unwrap();
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

/// The graph's cables as (output, input) pairs.
fn graph_cables(app: &App) -> Vec<(loom_core::OutputRef, loom_core::InputRef)> {
    app.session.controller.graph().cables().collect()
}

/// Ctrl+G makes one sub-patch tile of the selection, with a port for each
/// cable that crossed, and changes no cable of the graph. Undo brings the
/// modules back; redo makes the tile again with the same id.
#[test]
fn ctrl_g_makes_a_sub_patch_and_undo_brings_the_modules_back() {
    let mut ui = Ui::with_app(app_with(FLAT));
    let before = graph_cables(&ui.app);
    let saved = ui.app.current_patch();
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![2, 3])));
    ui.tap(keyboard::Key::Character("g".into()), Modifiers::COMMAND);
    let snapshot = ui.app.ui_snapshot();
    let tile = snapshot
        .tiles
        .iter()
        .find(|t| t.title == "Sub-patch")
        .expect("the new tile");
    assert_eq!(
        tile.inputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["Pitch"]
    );
    assert_eq!(
        tile.outputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["Out"]
    );
    assert!(!snapshot.tiles.iter().any(|t| t.title == "Sine"));
    assert_eq!(snapshot.selection, [tile.id]);
    assert_eq!(snapshot.status, "Made Sub-patch from 2 modules");
    assert_eq!(snapshot.undo.as_deref(), Some("make sub-patch"));
    assert_eq!(graph_cables(&ui.app), before, "the sound did not change");
    assert_eq!(ui.app.current_patch().subpatches.len(), 1);
    // The tile stands where the members were.
    assert_eq!(ui.app.session.positions[&tile.id], (192.0, 0.0));

    ui.app.undo();
    assert!(ui.app.session.hierarchy.is_flat());
    assert_eq!(ui.app.current_patch(), saved);
    assert_eq!(graph_cables(&ui.app), before);
    let selected: Vec<u32> = ui.app.session.selection.iter().copied().collect();
    assert_eq!(selected, [2, 3], "the members are selected again");

    ui.app.redo();
    let again = ui.app.ui_snapshot();
    assert_eq!(again.tiles.iter().filter(|t| t.title == "Sub-patch").count(), 1);
    assert_eq!(again.selection, [tile.id]);
    assert_eq!(ui.app.current_patch().subpatches.len(), 1);
}

/// Dissolving the tile gives the modules and cables back, where the tile
/// was, as the patch was before.
#[test]
fn dissolve_gives_the_modules_back_where_the_tile_was() {
    let mut ui = Ui::with_app(app_with(FLAT));
    let saved = ui.app.current_patch();
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![2, 3])));
    ui.tap(keyboard::Key::Character("g".into()), Modifiers::COMMAND);
    assert_eq!(ui.app.current_patch().subpatches.len(), 1);
    ui.tap(
        keyboard::Key::Character("g".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
    );
    assert!(ui.app.session.hierarchy.is_flat());
    assert_eq!(ui.app.current_patch(), saved);
    let snapshot = ui.app.ui_snapshot();
    assert_eq!(snapshot.selection, [2, 3]);
    assert_eq!(snapshot.status, "Dissolved Sub-patch");
    assert_eq!(snapshot.undo.as_deref(), Some("dissolve Sub-patch"));
    // With a plain module selected, there is nothing to dissolve.
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![1])));
    ui.tap(
        keyboard::Key::Character("g".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
    );
    assert_eq!(ui.app.ui_snapshot().status, "Select one sub-patch to dissolve");
}

/// Selecting only the Transport makes nothing, and says why.
#[test]
fn the_transport_alone_makes_no_sub_patch() {
    let mut app = app_with(FLAT);
    app.session.selection = [5].into_iter().collect();
    app.make_subpatch();
    assert!(app.session.hierarchy.is_flat());
    assert_eq!(app.ui_snapshot().status, loom_patch::hierarchy::NOTHING_TO_COLLAPSE);
}

/// Inside a sub-patch the Transport is refused, and a Patch input from the
/// browser becomes a port of the sub-patch, which its tile shows outside.
#[test]
fn inside_a_sub_patch_the_transport_is_refused_and_a_patch_input_is_a_port() {
    let mut app = nested_app();
    app.show_level(&voice());
    let transport = app
        .modules
        .iter()
        .copied()
        .find(|d| d.id == "loom.clock.transport")
        .unwrap();
    app.add_module(transport, Point::new(0.0, 400.0));
    assert_eq!(app.ui_snapshot().status, super::NO_TRANSPORT_INSIDE);
    assert_eq!(app.session.controller.graph().nodes().count(), 4);

    let input = app
        .modules
        .iter()
        .copied()
        .find(|d| d.id == "loom.struct.input")
        .unwrap();
    app.add_module(input, Point::new(0.0, 400.0));
    assert_eq!(
        app.session.controller.graph().nodes().count(),
        4,
        "a port is not a module"
    );
    let added = *app.session.selection.iter().next().expect("the port is selected");
    assert_eq!(
        app.session
            .hierarchy
            .tile(app.session.controller.graph(), ModuleId(added)),
        Some(loom_patch::Tile::Port(loom_patch::Direction::Input, "in".to_string()))
    );
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("add Patch input"));
    app.show_level(&ModulePath::root());
    let tile = app
        .ui_snapshot()
        .tiles
        .into_iter()
        .find(|t| t.title == "Voice")
        .unwrap();
    assert_eq!(
        tile.inputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["pitch", "in"]
    );
    let definition = &app.current_patch().subpatches["voice"];
    assert_eq!(
        definition
            .modules
            .iter()
            .filter(|m| m.kind == loom_patch::INPUT_KIND)
            .count(),
        2
    );
}

/// A modulation from another level, or from a port tile, is refused.
#[test]
fn a_modulation_across_a_boundary_is_refused() {
    let mut app = nested_app();
    let sine = app.session.hierarchy.id_at(&voice(), 2).unwrap();
    let note_pitch = loom_core::OutputRef {
        module: ModuleId(1),
        port: 0,
    };
    app.assign(note_pitch, sine, 0);
    assert_eq!(app.ui_snapshot().status, super::NO_MODULATION_ACROSS);
    assert_eq!(app.session.controller.graph().assignments().count(), 0);
    let port = app.session.hierarchy.id_at(&voice(), 1).unwrap();
    app.assign(loom_core::OutputRef { module: port, port: 0 }, sine, 0);
    assert_eq!(app.session.controller.graph().assignments().count(), 0);
}

/// Copy, cut, and duplicate refuse a selection with a sub-patch tile.
#[test]
fn copy_of_a_sub_patch_tile_is_refused() {
    let mut app = nested_app();
    app.session.selection = [1, 2].into_iter().collect();
    assert!(app.copy().is_none());
    assert_eq!(app.ui_snapshot().status, super::NO_COPY_YET);
    assert!(app.cut().is_none());
    assert_eq!(app.ui_snapshot().status, "Cannot cut a sub-patch yet");
    app.duplicate();
    assert_eq!(app.ui_snapshot().status, "Cannot duplicate a sub-patch yet");
    assert_eq!(app.session.controller.graph().nodes().count(), 4);
}

/// Inside a sub-patch, the cable from a port into a module can be taken
/// away and put back, and outside, a new cable into the sub-patch's port
/// replaces the one there. The graph follows each time.
#[test]
fn cables_through_a_port_are_removed_replaced_and_restored() {
    use loom_canvas::{CanvasEvent, PortRef};
    let mut app = nested_app();
    let sine = app.session.hierarchy.id_at(&voice(), 2).unwrap();
    let phase = loom_core::InputRef { module: sine, port: 0 };
    let into_phase = |app: &App| app.session.controller.graph().source_of(phase);
    assert_eq!(
        into_phase(&app),
        Some(loom_core::OutputRef {
            module: ModuleId(1),
            port: 0
        })
    );

    app.show_level(&voice());
    app.canvas_event(CanvasEvent::Disconnect(PortRef {
        module: sine.0,
        output: false,
        index: 0,
    }));
    assert_eq!(into_phase(&app), None, "the port no longer feeds the sine");
    assert_eq!(app.ui_snapshot().cables.len(), 1);
    let port = app.session.hierarchy.id_at(&voice(), 1).unwrap();
    app.canvas_event(CanvasEvent::Connect {
        from: PortRef {
            module: port.0,
            output: true,
            index: 0,
        },
        to: PortRef {
            module: sine.0,
            output: false,
            index: 0,
        },
    });
    assert_eq!(
        into_phase(&app),
        Some(loom_core::OutputRef {
            module: ModuleId(1),
            port: 0
        })
    );
    assert_eq!(app.ui_snapshot().cables.len(), 2);

    // Outside: the note's gate replaces its pitch on the Voice's port.
    app.show_level(&ModulePath::root());
    app.canvas_event(CanvasEvent::Connect {
        from: PortRef {
            module: 1,
            output: true,
            index: 1,
        },
        to: PortRef {
            module: 2,
            output: false,
            index: 0,
        },
    });
    assert_eq!(
        into_phase(&app),
        Some(loom_core::OutputRef {
            module: ModuleId(1),
            port: 1
        })
    );
    let saved = app.current_patch();
    assert!(
        saved
            .cables
            .iter()
            .any(|c| c.from == (1, "gate".to_string()) && c.to == (2, "pitch".to_string()))
    );
    assert!(!saved.cables.iter().any(|c| c.from == (1, "pitch".to_string())));
    // And taking it away outside leaves the sine unfed.
    app.canvas_event(CanvasEvent::Disconnect(PortRef {
        module: 2,
        output: false,
        index: 0,
    }));
    assert_eq!(into_phase(&app), None);
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("remove cable"));
}

/// Deleting a sub-patch tile removes everything inside it; undo brings
/// the tile, its inside, and its ports back.
#[test]
fn deleting_a_sub_patch_tile_removes_its_inside() {
    let mut app = nested_app();
    let sine = app.session.hierarchy.id_at(&voice(), 2).unwrap();
    app.session.selection = [2].into_iter().collect();
    app.edit(|app| {
        app.delete_modules(&[2]);
    });
    assert_eq!(app.session.controller.graph().nodes().count(), 3);
    assert!(app.session.hierarchy.is_flat());
    assert!(app.session.controller.graph().node(sine).is_none());
    assert!(!app.session.positions.contains_key(&2));
    assert!(graph_cables(&app).is_empty());

    app.undo();
    assert_eq!(app.session.controller.graph().nodes().count(), 4);
    assert!(!app.session.hierarchy.is_flat());
    assert!(app.session.positions.contains_key(&2));
    assert_eq!(graph_cables(&app).len(), 2);
    let tile = app
        .ui_snapshot()
        .tiles
        .into_iter()
        .find(|t| t.title == "Voice")
        .unwrap();
    assert_eq!(tile.inputs.len(), 1);
    assert_eq!(tile.outputs.len(), 1);
}

/// Duplicating a module inside a sub-patch keeps the cable that came in
/// through a port: the copy is fed from the same port.
#[test]
fn duplicate_inside_a_sub_patch_keeps_the_cable_from_the_port() {
    let mut app = nested_app();
    app.show_level(&voice());
    let sine = app.session.hierarchy.id_at(&voice(), 2).unwrap();
    app.session.selection = [sine.0].into_iter().collect();
    app.duplicate();
    assert_eq!(app.session.controller.graph().nodes().count(), 5);
    let copy = *app.session.selection.iter().next().expect("the copy is selected");
    assert_ne!(copy, sine.0);
    assert_eq!(app.session.hierarchy.level_of(ModuleId(copy)), voice());
    let fed = app.session.controller.graph().source_of(loom_core::InputRef {
        module: ModuleId(copy),
        port: 0,
    });
    assert_eq!(
        fed,
        Some(loom_core::OutputRef {
            module: ModuleId(1),
            port: 0
        })
    );
    let saved = app.current_patch();
    let definition = &saved.subpatches["voice"];
    assert_eq!(
        definition
            .cables
            .iter()
            .filter(|c| c.from == (1, "out".to_string()))
            .count(),
        2
    );
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("duplicate Sine"));
}

/// The tile menu offers Open and Dissolve on a sub-patch, and Make
/// sub-patch on a module.
#[test]
fn the_tile_menu_knows_sub_patches() {
    let app = nested_app();
    let labels = |id: u32| app.tile_menu_items(id).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
    assert_eq!(labels(2), ["Open", "Dissolve sub-patch"]);
    assert_eq!(labels(1), ["Help", "Make sub-patch"]);
}

/// Duplicating a selection that holds the Transport pairs each copy with
/// its own original, so a module fed from a sub-patch tile keeps that
/// cable on its copy: the Transport sorts first here, and a group leaves
/// it out.
#[test]
fn duplicate_with_the_transport_selected_pairs_copies_with_their_originals() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "voice": SubPatch(
                metadata: Metadata(name: "Voice"),
                modules: [
                    PatchModule(id: 1, kind: "loom.osc.sine@1"),
                    PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
                ],
                cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
            ),
        },
        modules: [
            PatchModule(id: 1, kind: "loom.clock.transport@1", position: Some((0.0, 240.0))),
            PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), position: Some((0.0, 0.0))),
            PatchModule(id: 4, kind: "loom.level.clamp@1", position: Some((240.0, 0.0))),
        ],
        cables: [PatchCable(from: (2, "out"), to: (4, "in"))],
    )"#;
    let mut app = app_with(text);
    app.session.selection = [1, 4].into_iter().collect();
    app.duplicate();
    assert_eq!(app.ui_snapshot().status, "Duplicated Clamp");
    let copy = *app.session.selection.iter().next().expect("the copy is selected");
    assert_ne!(copy, 4);
    let fed = app.session.controller.graph().source_of(loom_core::InputRef {
        module: ModuleId(copy),
        port: 0,
    });
    let sine = app.session.hierarchy.id_at(&voice(), 1).unwrap();
    assert_eq!(fed, Some(loom_core::OutputRef { module: sine, port: 0 }));
    let saved = app.current_patch();
    assert_eq!(
        saved.cables.iter().filter(|c| c.from == (2, "out".to_string())).count(),
        2
    );
}

/// Deleting everything keeps the Transport, its place, and its cable into
/// a sub-patch port: the controller would refuse it, so it is never part
/// of the delete.
#[test]
fn deleting_everything_keeps_the_transport_and_its_cable_into_a_port() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "clocked": SubPatch(
                metadata: Metadata(name: "Clocked"),
                modules: [
                    PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("phase")),
                    PatchModule(id: 2, kind: "loom.osc.sine@1"),
                ],
                cables: [PatchCable(from: (1, "out"), to: (2, "phase"))],
            ),
        },
        modules: [
            PatchModule(id: 1, kind: "loom.clock.transport@1", position: Some((0.0, 240.0))),
            PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("clocked"), position: Some((240.0, 0.0))),
            PatchModule(id: 3, kind: "loom.io.audio-out@1", position: Some((480.0, 0.0))),
        ],
        cables: [PatchCable(from: (1, "beat-phase"), to: (2, "phase"))],
    )"#;
    let mut app = app_with(text);
    let sine = app.session.hierarchy.id_at(&ModulePath::top(2), 2).unwrap();
    assert_eq!(graph_cables(&app).len(), 1);
    app.session.selection = [1, 3].into_iter().collect();
    app.edit(|app| {
        app.delete_modules(&[1, 3]);
    });
    assert_eq!(
        app.session.controller.graph().nodes().count(),
        2,
        "the Transport and the sine"
    );
    assert_eq!(
        app.session.positions.get(&1),
        Some(&(0.0, 240.0)),
        "the Transport keeps its place"
    );
    assert_eq!(
        app.session
            .controller
            .graph()
            .source_of(loom_core::InputRef { module: sine, port: 0 }),
        Some(loom_core::OutputRef {
            module: ModuleId(1),
            port: 1
        }),
        "the Transport still feeds the port"
    );
    assert!(
        app.current_patch()
            .cables
            .iter()
            .any(|c| c.to == (2, "phase".to_string()))
    );
}

/// A cable from a module replaces the record from a port that nothing
/// feeds outside: the record goes although the graph never held a cable
/// for it, so the canvas shows one cable and the file keeps the module's.
#[test]
fn a_direct_cable_replaces_a_record_from_an_unfed_port() {
    use loom_canvas::{CanvasEvent, PortRef};
    let mut app = nested_app();
    app.show_level(&voice());
    let sine = app.session.hierarchy.id_at(&voice(), 2).unwrap();
    // A new Patch input, unconnected outside, into the sine's phase.
    let input = app
        .modules
        .iter()
        .copied()
        .find(|d| d.id == "loom.struct.input")
        .unwrap();
    app.add_module(input, Point::new(0.0, 400.0));
    let port = *app.session.selection.iter().next().unwrap();
    app.canvas_event(CanvasEvent::Connect {
        from: PortRef {
            module: port,
            output: true,
            index: 0,
        },
        to: PortRef {
            module: sine.0,
            output: false,
            index: 0,
        },
    });
    let phase = loom_core::InputRef { module: sine, port: 0 };
    assert_eq!(
        app.session.controller.graph().source_of(phase),
        None,
        "nothing stands behind the new port"
    );
    assert_eq!(app.ui_snapshot().cables.len(), 2, "the record is drawn all the same");

    // An LFO inside the Voice, cabled into the same input.
    let lfo = app.modules.iter().copied().find(|d| d.id == "loom.mod.lfo").unwrap();
    app.add_module(lfo, Point::new(0.0, 600.0));
    let lfo = *app.session.selection.iter().next().unwrap();
    app.canvas_event(CanvasEvent::Connect {
        from: PortRef {
            module: lfo,
            output: true,
            index: 0,
        },
        to: PortRef {
            module: sine.0,
            output: false,
            index: 0,
        },
    });
    assert_eq!(
        app.session.controller.graph().source_of(phase),
        Some(loom_core::OutputRef {
            module: ModuleId(lfo),
            port: 0
        })
    );
    let shown = app.ui_snapshot().cables;
    assert_eq!(
        shown.iter().filter(|c| c.to.module == sine.0).count(),
        1,
        "one cable into the sine"
    );
    let saved = app.current_patch();
    let into_phase: Vec<_> = saved.subpatches["voice"]
        .cables
        .iter()
        .filter(|c| c.to == (2, "phase".to_string()))
        .collect();
    assert_eq!(into_phase.len(), 1);
    assert_eq!(
        into_phase[0].from.0,
        app.session.hierarchy.path_of(ModuleId(lfo)).last()
    );
    // And the file loads with that cable.
    let (reloaded, _) = {
        let patch = saved.clone();
        let loaded = Loaded::read(EngineConfig::default(), false, Source::Recovered(Box::new(patch))).unwrap();
        (loaded.controller, loaded.hierarchy)
    };
    assert_eq!(
        reloaded.graph().cables().count(),
        app.session.controller.graph().cables().count()
    );
}

/// The nested example open in the harness, with the filter's flat id and
/// its cutoff index: the one parameter the Voice tile's surface shows.
fn nested_example_ui() -> (Ui, ModuleId, usize) {
    use loom_core::ModuleNames;
    let ui = Ui::open("nested-subtractive.ron");
    let graph = ui.app.session.controller.graph();
    let (svf, node) = graph
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
        .expect("the filter two levels down");
    let cutoff = node.parameter_index("cutoff").unwrap();
    (ui, svf, cutoff)
}

/// The filter's live cutoff.
fn cutoff_of(ui: &Ui, svf: ModuleId, cutoff: usize) -> f32 {
    ui.app.session.controller.graph().node(svf).unwrap().params[cutoff]
}

/// Turning the Voice tile's surface control moves the filter's cutoff two
/// levels down, as one undo step, and the surface and inner controls show
/// one value. Inside, turning the filter's own knob moves the surface
/// control, and one undo step reverts that too (panels.md, Exposed
/// controls, acceptance criteria).
#[test]
fn surface_and_inner_controls_move_together_and_undo_as_one() {
    let (mut ui, svf, cutoff) = nested_example_ui();
    assert_eq!(cutoff_of(&ui, svf, cutoff), 300.0);
    let steps_before = ui.app.session.edits.history.len();

    // Outside: the Voice tile (id 2) shows "cutoff" as its one control.
    let knob = ui.control(2, "cutoff");
    ui.drag(knob, Point::new(knob.x, knob.y - 40.0), 8);
    let turned = cutoff_of(&ui, svf, cutoff);
    assert!(turned > 300.0, "the surface knob moved the inner cutoff: {turned}");
    assert_eq!(
        ui.app.session.edits.history.len(),
        steps_before + 1,
        "one step for the drag"
    );
    let surface = ui.app.session.values.get(2, 0).unwrap().clone();
    ui.app.undo();
    assert_eq!(
        cutoff_of(&ui, svf, cutoff),
        300.0,
        "one undo step reverts the surface drag"
    );
    ui.app.redo();
    assert_eq!(cutoff_of(&ui, svf, cutoff), turned);

    // Inside the Filter: the knob itself shows the same value and moves it.
    let filter = ModulePath::top(2).child(5);
    ui.app.show_level(&filter);
    ui.redraw();
    let inner_widget = crate::surface::find_control(
        ui.app.session.controller.graph(),
        &ui.app.session.hierarchy,
        svf.0,
        "cutoff",
    )
    .unwrap();
    assert_eq!(
        ui.app.session.values.get(svf.0, inner_widget).unwrap().text,
        surface.text,
        "both controls say the same value"
    );
    let inner = ui.control(svf.0, "cutoff");
    ui.drag(inner, Point::new(inner.x, inner.y - 40.0), 8);
    let turned_inside = cutoff_of(&ui, svf, cutoff);
    assert!(turned_inside > turned);
    ui.app.show_level(&ModulePath::root());
    ui.redraw();
    assert_eq!(
        ui.app.session.values.get(2, 0).unwrap().text,
        crate::param::format(
            &ui.app
                .control_target(2, 0, loom_canvas::ControlPart::Main)
                .unwrap()
                .spec,
            turned_inside
        ),
        "the surface follows the inner knob"
    );
    ui.app.undo();
    assert_eq!(
        cutoff_of(&ui, svf, cutoff),
        turned,
        "one undo step reverts the inner drag"
    );
}

/// The nested example with the Filter's exposed cutoff range narrowed to
/// 200 to 2000 Hz, inside the SVF's own 20 to 20000 Hz.
fn narrowed_nested() -> App {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    let text = std::fs::read_to_string(root.join("nested-subtractive.ron")).unwrap();
    app_with(&text.replace("range: Some((20.0, 20000.0))", "range: Some((200.0, 2000.0))"))
}

/// A range from the file narrows the surface control and clamps every
/// write to the parameter: typed on the surface or on the inner knob,
/// dragged inside, set from an inspector row, or reset. The file then
/// saves what was heard (sub-patches.md, Exposing).
#[test]
fn an_exposed_range_clamps_writes_from_both_sides() {
    use loom_core::ModuleNames;
    let mut ui = Ui::with_app(narrowed_nested());
    let graph = ui.app.session.controller.graph();
    let (svf, node) = graph
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
        .unwrap();
    let cutoff = node.parameter_index("cutoff").unwrap();
    let default = node.descriptor.params[cutoff].default;
    let target = ui.app.control_target(2, 0, loom_canvas::ControlPart::Main).unwrap();
    assert_eq!(
        (target.spec.min, target.spec.max),
        (200.0, 2000.0),
        "the surface spans the range"
    );
    let inner_widget = crate::surface::find_control(
        ui.app.session.controller.graph(),
        &ui.app.session.hierarchy,
        svf.0,
        "cutoff",
    )
    .unwrap();

    // Typed on the surface, past the range.
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::TypedValue {
        module: 2,
        widget: 0,
        text: "30000".to_string(),
    }));
    assert_eq!(cutoff_of(&ui, svf, cutoff), 2000.0);
    // Typed on the inner knob, below it.
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::TypedValue {
        module: svf.0,
        widget: inner_widget,
        text: "5".to_string(),
    }));
    assert_eq!(cutoff_of(&ui, svf, cutoff), 200.0);
    assert_eq!(
        ui.app
            .session
            .controller
            .graph()
            .node(svf)
            .unwrap()
            .spec(cutoff)
            .map(|s| (s.min, s.max)),
        Some((20.0, 20000.0)),
        "the inner control range is not widened for a clamped value"
    );
    // Dragged inside, far past the top.
    let filter = ModulePath::top(2).child(5);
    ui.app.show_level(&filter);
    ui.redraw();
    let inner = ui.control(svf.0, "cutoff");
    ui.drag(inner, Point::new(inner.x, inner.y - 400.0), 20);
    let dragged = cutoff_of(&ui, svf, cutoff);
    assert!(dragged > 200.0 && dragged <= 2000.0, "{dragged}");
    // Set from the inspector row.
    let _ = ui.app.inspector_message(crate::inspector::Message::ParamValue {
        module: svf.0,
        index: cutoff,
        value: 10000.0,
    });
    assert_eq!(cutoff_of(&ui, svf, cutoff), 2000.0);
    // Reset on the surface: the default, kept inside the range.
    ui.app.show_level(&ModulePath::root());
    ui.redraw();
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::ResetControl {
        module: 2,
        widget: 0,
        part: loom_canvas::ControlPart::Main,
    }));
    assert_eq!(cutoff_of(&ui, svf, cutoff), default.clamp(200.0, 2000.0));
    // The file saves what plays.
    let saved = ui.app.current_patch();
    let instance = saved.modules.iter().find(|m| m.id == 2).unwrap();
    assert_eq!(instance.params.get("cutoff"), Some(&cutoff_of(&ui, svf, cutoff)));
}

/// A sub-patch holding a Mixer, whose levels are faders inside.
const MIXED: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Mixed"),
    subpatches: {
        "mix": SubPatch(
            metadata: Metadata(name: "Mix"),
            modules: [
                PatchModule(id: 1, kind: "loom.mix.mixer@1", position: Some((0.0, 0.0))),
                PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out"), position: Some((400.0, 0.0))),
            ],
            cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("mix"), position: Some((200.0, 0.0))),
        PatchModule(id: 2, kind: "loom.io.audio-out@1", position: Some((500.0, 0.0))),
        PatchModule(id: 3, kind: "loom.clock.transport@1", position: Some((0.0, 300.0))),
    ],
    cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
)"#;

/// Inside the Mix sub-patch, the Mixer's first level row offers Expose;
/// pressing it puts a fader, the widget the level has inside, on the Mix
/// tile's surface, named by the parameter's label, as one undo step
/// (panels.md, Exposed controls, acceptance criteria).
#[test]
fn exposing_a_fader_puts_a_fader_on_the_surface() {
    let mut ui = Ui::with_app(app_with(MIXED));
    let inside = ModulePath::top(1);
    let mixer = ui.app.session.hierarchy.id_at(&inside, 1).unwrap();
    ui.app.show_level(&inside);
    ui.app.session.selection = [mixer.0].into_iter().collect();
    ui.redraw();
    // The first row is the first level, and its button is the first Expose.
    ui.click_text("Expose on Mix");
    assert_eq!(ui.app.ui_snapshot().status, "Level 1 is exposed on Mix");
    assert_eq!(ui.app.ui_snapshot().undo.as_deref(), Some("expose Level 1"));
    let definition = ui.app.session.hierarchy.definition_of(ModuleId(1)).unwrap();
    assert_eq!(definition.exposed.len(), 1);
    assert_eq!(definition.exposed[0].name, "Level 1");
    // The inner fader now carries the mark and the row offers Unexpose.
    let level1 = crate::surface::find_control(
        ui.app.session.controller.graph(),
        &ui.app.session.hierarchy,
        mixer.0,
        "level1",
    )
    .unwrap();
    let snapshot = ui.app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == mixer.0).unwrap();
    assert!(tile.widgets[level1].exposed);
    assert!(ui.text("Unexpose on Mix").is_some());

    ui.app.show_level(&ModulePath::root());
    ui.redraw();
    let snapshot = ui.app.ui_snapshot();
    let mix = snapshot.tiles.iter().find(|t| t.id == 1).expect("the Mix tile");
    assert_eq!(mix.widgets.len(), 1);
    assert_eq!(
        (mix.widgets[0].kind, mix.widgets[0].label.as_str()),
        ("fader", "Level 1")
    );

    ui.app.undo();
    assert_eq!(
        ui.app.session.level, inside,
        "undo shows the level the step was made on"
    );
    assert!(
        ui.app
            .session
            .hierarchy
            .definition_of(ModuleId(1))
            .unwrap()
            .exposed
            .is_empty()
    );
    ui.app.show_level(&ModulePath::root());
    ui.redraw();
    let snapshot = ui.app.ui_snapshot();
    let mix = snapshot.tiles.iter().find(|t| t.id == 1).unwrap();
    assert!(mix.widgets.is_empty(), "undo takes the fader off the surface");
    ui.app.redo();
    assert_eq!(
        ui.app
            .session
            .hierarchy
            .definition_of(ModuleId(1))
            .unwrap()
            .exposed
            .len(),
        1
    );
}

/// A modulation from the top level reaches the filter's cutoff two levels
/// down through the Voice tile's surface; one onto a parameter no tile
/// exposes, or from a port tile, is still refused, and so is a matrix
/// retarget onto one (sub-patches.md, Editor model).
#[test]
fn a_modulation_reaches_inside_only_through_a_surface() {
    use loom_core::ModuleNames;
    let (mut ui, svf, cutoff) = nested_example_ui();
    let app = &mut ui.app;
    let note_pitch = loom_core::OutputRef {
        module: ModuleId(1),
        port: 0,
    };
    let resonance = app
        .session
        .controller
        .graph()
        .node(svf)
        .unwrap()
        .parameter_index("resonance")
        .unwrap();
    app.assign(note_pitch, svf, resonance);
    assert_eq!(app.ui_snapshot().status, super::NO_MODULATION_ACROSS);
    assert_eq!(app.session.controller.graph().assignments().count(), 0);

    app.assign(note_pitch, svf, cutoff);
    assert_eq!(
        app.session.controller.graph().assignments().count(),
        1,
        "through the exposed cutoff"
    );
    assert!(
        app.ui_snapshot().status.contains("modulates"),
        "{}",
        app.ui_snapshot().status
    );
    let id = app.session.controller.graph().assignments().next().unwrap().0;
    // The line ends at the Voice tile's surface control.
    let lines = app.session.scene.modulations();
    assert_eq!((lines[0].to, lines[0].widget), (2, Some(0)));
    // The file writes it through the exposed name.
    let saved = app.current_patch();
    assert_eq!(saved.modulation.len(), 1);
    assert_eq!(saved.modulation[0].target, (2, "cutoff".to_string()));

    // A retarget onto the resonance is refused and changes nothing.
    app.matrix_retarget(id, None, Some((svf, resonance)));
    assert_eq!(app.ui_snapshot().status, super::NO_MODULATION_ACROSS);
    assert_eq!(app.session.controller.graph().assignment(id).unwrap().param, cutoff);

    // A port tile is never a source.
    let voice = ModulePath::top(2);
    let port = app.session.hierarchy.id_at(&voice, 1).unwrap();
    app.assign(loom_core::OutputRef { module: port, port: 0 }, svf, cutoff);
    assert_eq!(app.session.controller.graph().assignments().count(), 1);
}

/// Unexpose on the Voice tile's row takes the cutoff off its surface and
/// removes the assignment from the top level in the same step; the Filter's
/// own exposure stays. Undo brings both the exposure and the assignment
/// back.
#[test]
fn unexposing_removes_the_assignment_from_outside_in_one_step() {
    let (mut ui, svf, cutoff) = nested_example_ui();
    let app = &mut ui.app;
    let note_pitch = loom_core::OutputRef {
        module: ModuleId(1),
        port: 0,
    };
    app.assign(note_pitch, svf, cutoff);
    assert_eq!(app.session.controller.graph().assignments().count(), 1);
    let steps = app.session.edits.history.len();

    app.unexpose(2, 0);
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("unexpose cutoff"));
    assert_eq!(app.session.edits.history.len(), steps + 1);
    assert!(
        app.session
            .hierarchy
            .definition_of(ModuleId(2))
            .unwrap()
            .exposed
            .is_empty()
    );
    let filter = app.session.hierarchy.id_at(&ModulePath::top(2), 5).unwrap();
    assert_eq!(app.session.hierarchy.definition_of(filter).unwrap().exposed.len(), 1);
    assert_eq!(
        app.session.controller.graph().assignments().count(),
        0,
        "the outer assignment went with it"
    );
    assert!(
        app.ui_snapshot()
            .tiles
            .iter()
            .find(|t| t.id == 2)
            .unwrap()
            .widgets
            .is_empty()
    );

    app.undo();
    assert_eq!(
        app.session.hierarchy.definition_of(ModuleId(2)).unwrap().exposed.len(),
        1
    );
    assert_eq!(app.session.controller.graph().assignments().count(), 1);
}

/// MIDI learn on the Voice tile's surface control arms the filter's cutoff
/// inside; the mapping lands on the inner parameter, is written inside the
/// Filter's definition, and covers the exposed range, so hardware cannot
/// leave it (sub-patches.md, Exposing).
#[test]
fn midi_learn_on_a_surface_control_maps_the_inner_parameter_within_its_range() {
    /// Sends CC 74 on channel 1 with `value` to the idle engine.
    fn send_cc(midi: &loom_host_desktop::SharedMidi, value: u8) -> bool {
        loom_host_desktop::send_midi(midi, loom_core::MidiEvent::from_bytes(0, &[0xB0, 74, value]))
    }
    use loom_core::ModuleNames;
    let mut app = narrowed_nested();
    let graph = app.session.controller.graph();
    let (svf, node) = graph
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
        .unwrap();
    let cutoff = node.parameter_index("cutoff").unwrap();

    app.learn_message(crate::learn::Message::Toggle);
    app.canvas_event(loom_canvas::CanvasEvent::Learn { module: 2, widget: 0 });
    assert_eq!(app.session.learn.armed().map(|a| a.param), Some((svf.0, cutoff)));
    let view = crate::learn::view(
        &app.session.controller,
        &app.session.hierarchy,
        &app.session.level,
        app.session.learn.armed(),
    );
    assert_eq!(view.armed, Some((2, 0)), "the surface control shows as armed");

    let midi = app.rig.midi_in.clone().expect("the idle engine's MIDI input");
    assert!(loom_host_desktop::send_midi(
        &midi,
        loom_core::MidiEvent::from_bytes(0, &[0xB0, 74, 64])
    ));
    app.process(128);
    let _ = app.update(Message::Midi);
    let (_, mapping) = app
        .session
        .controller
        .graph()
        .midi_mapping_of(svf, cutoff)
        .expect("mapped inside");
    assert_eq!(mapping.control.controller, 74);
    assert_eq!(
        mapping.options,
        loom_core::MidiMappingOptions::DEFAULT,
        "a learned mapping keeps its whole travel"
    );
    // The engine maps the travel onto the exposed range (sub-patches.md,
    // MIDI follows the range).
    for (value, expected) in [(127, 2000.0), (0, 200.0)] {
        assert!(send_cc(&midi, value));
        app.process(128);
        let _ = app.update(Message::Midi);
        let cutoff_now = app.session.controller.graph().node(svf).unwrap().params[cutoff];
        assert!((cutoff_now - expected).abs() < 1e-2, "CC {value}: {cutoff_now}");
    }
    let view = crate::learn::view(
        &app.session.controller,
        &app.session.hierarchy,
        &app.session.level,
        None,
    );
    assert_eq!(view.labels.get(&(2, 0)).map(String::as_str), Some("CC 74"));
    let saved = app.current_patch();
    assert!(saved.midi.is_empty() && saved.subpatches["voice"].midi.is_empty());
    assert_eq!(saved.subpatches["filter"].midi[0].target, (3, "cutoff".to_string()));
}

/// The nested example's flat filter and its cutoff index.
fn svf_cutoff(app: &App) -> (ModuleId, usize) {
    use loom_core::ModuleNames;
    let (svf, node) = app
        .session
        .controller
        .graph()
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
        .unwrap();
    (svf, node.parameter_index("cutoff").unwrap())
}

/// Maps CC 74 on the filter's cutoff as a Pickup that waits, sends `value`,
/// and lets the app follow, as a player moving the hardware far from the
/// value would.
fn waiting_pickup_at(app: &mut App, value: u8) {
    let (svf, cutoff) = svf_cutoff(app);
    let options = loom_core::MidiMappingOptions {
        mode: loom_core::MidiMode::Pickup,
        ..loom_core::MidiMappingOptions::DEFAULT
    };
    let control = loom_core::MidiControl {
        channel: 0,
        controller: 74,
    };
    app.session
        .controller
        .map_midi_with(control, svf, cutoff, options)
        .unwrap();
    app.commit();
    let midi = app.rig.midi_in.clone().expect("the idle engine's MIDI input");
    assert!(loom_host_desktop::send_midi(
        &midi,
        loom_core::MidiEvent::from_bytes(0, &[0xB0, 74, value])
    ));
    app.process(128);
    let _ = app.update(Message::Midi);
    assert_eq!(app.session.controller.midi_waiting().len(), 1, "the pickup waits");
}

/// Where the ghost mark shows on tile `tile`'s control `widget`.
fn ghost_on(app: &App, tile: u32, widget: usize) -> f32 {
    app.session
        .values
        .get(tile, widget)
        .and_then(|value| value.ghost)
        .unwrap_or_else(|| panic!("no ghost on tile {tile}, control {widget}"))
}

/// The cutoff knob's index on the filter tile.
fn cutoff_knob(app: &App) -> usize {
    let (svf, _) = svf_cutoff(app);
    let node = app.session.controller.graph().node(svf).unwrap();
    node.descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.binds("cutoff"))
        .unwrap()
}

/// A waiting pickup's ghost mark sits at the hardware's value on every
/// control that shows the parameter, each on its own scale: the surface
/// knob and the sub-patch inspector row on the exposed 200 to 2000 Hz, the
/// inner knob on 20 to 20 000 Hz (sub-patches.md, MIDI follows the range).
/// The ends are checked, because the middle lands near the middle of both.
#[test]
fn a_ghost_mark_sits_on_each_controls_own_scale() {
    for (value, surface, inner) in [(0, 0.0, 1.0 / 3.0), (127, 1.0, 2.0 / 3.0)] {
        let mut app = narrowed_nested();
        waiting_pickup_at(&mut app, value);
        let (svf, _) = svf_cutoff(&app);
        assert!(
            (ghost_on(&app, 2, 0) - surface).abs() < 1e-3,
            "CC {value} on the surface"
        );
        let knob = cutoff_knob(&app);
        assert!((ghost_on(&app, svf.0, knob) - inner).abs() < 1e-3, "CC {value} inside");

        app.session.selection = [2].into_iter().collect();
        let Inspector::SubPatch(rows) = app.inspector_model() else {
            panic!("the sub-patch tile's inspector");
        };
        let ghost = rows.params[0].row.mapping_options.as_ref().and_then(|m| m.ghost);
        assert!(
            ghost.is_some_and(|g| (g - surface).abs() < 1e-3),
            "CC {value} on the inspector row: {ghost:?}"
        );
    }
}

/// With a range on each level, each surface puts the mark on its own span:
/// the Voice's 500 to 1000 Hz, and the Filter's 200 to 2000 Hz inside it.
#[test]
fn each_nested_surface_marks_the_ghost_on_its_own_span() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    let text = std::fs::read_to_string(root.join("nested-subtractive.ron")).unwrap();
    let text = text
        .replace("range: Some((20.0, 20000.0))", "range: Some((200.0, 2000.0))")
        .replace(
            r#"exposed: [Exposed(name: "cutoff", module: 5, param: "cutoff")]"#,
            r#"exposed: [Exposed(name: "cutoff", module: 5, param: "cutoff", range: Some((500.0, 1000.0)))]"#,
        );
    let mut app = app_with(&text);
    // The value sits at 500 Hz, the bottom of the Voice's range, so CC 127
    // is far away and stands for 1000 Hz.
    waiting_pickup_at(&mut app, 127);
    let filter_tile = app
        .session
        .hierarchy
        .id_at(&ModulePath::top(2), 5)
        .expect("the Filter tile inside the Voice");
    assert!((ghost_on(&app, 2, 0) - 1.0).abs() < 1e-3, "the Voice surface");
    let on_filter = 5f32.log10();
    assert!(
        (ghost_on(&app, filter_tile.0, 0) - on_filter).abs() < 1e-3,
        "the Filter surface: 1000 Hz of 200 to 2000"
    );
}

/// The inspector of tile `tile`, as the snapshot shows it, as text.
fn inspector_text(app: &mut App, tile: u32) -> String {
    app.session.selection = [tile].into_iter().collect();
    serde_json::to_string(&app.ui_snapshot().inspector).unwrap()
}

/// A mapping row on a parameter a surface narrows says, in one friendly
/// sentence, that Low and High are a share of the exposed range, and names
/// the surface that gives it: on the inner row and on the sub-patch tile's
/// row. With no surface left, the note goes (sub-patches.md, MIDI follows
/// the range).
#[test]
fn a_mapping_row_says_its_range_is_a_share_of_the_exposed_range() {
    let mut app = narrowed_nested();
    let (svf, cutoff) = svf_cutoff(&app);
    let control = loom_core::MidiControl {
        channel: 0,
        controller: 74,
    };
    app.session.controller.map_midi(control, svf, cutoff).unwrap();
    app.commit();
    let note = "Low and High are a share of 200.0 Hz to 2000 Hz, the range exposed on Filter.";
    let inner = inspector_text(&mut app, svf.0);
    assert!(inner.contains(note), "the inner row: {inner}");
    let surface = inspector_text(&mut app, 2);
    assert!(surface.contains(note), "the Voice tile's row: {surface}");

    // Off the Voice's surface, the Filter's own exposure still gives it.
    app.unexpose(2, 0);
    assert!(inspector_text(&mut app, svf.0).contains(note));
    let filter = app.session.hierarchy.id_at(&ModulePath::top(2), 5).unwrap();
    app.unexpose(filter.0, 0);
    let inner = inspector_text(&mut app, svf.0);
    assert!(!inner.contains("range_note"), "no surface, no note: {inner}");
}

/// The note names the span MIDI sweeps, which is the overlap of the exposed
/// range and the selected range: a gain exposed at -1 to 1 with -0.1 to 0.1
/// selected sweeps -0.1 to 0.1, so the note says that (sub-patches.md,
/// MIDI follows the range).
#[test]
fn the_range_note_names_the_span_midi_sweeps() {
    let mut app = app_with(
        r#"Patch(
            version: 1,
            subpatches: {
                "amp": SubPatch(
                    metadata: Metadata(name: "Amp"),
                    modules: [PatchModule(id: 1, kind: "loom.math.mul@1")],
                    exposed: [Exposed(name: "gain", module: 1, param: "gain", range: Some((-1.0, 1.0)))],
                ),
            },
            modules: [PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("amp"))],
        )"#,
    );
    use loom_core::ModuleNames;
    let (mul, node) = app
        .session
        .controller
        .graph()
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.math.mul")
        .unwrap();
    let gain = node.parameter_index("gain").unwrap();
    let spec = node.descriptor.params[gain];
    let control = loom_core::MidiControl {
        channel: 0,
        controller: 20,
    };
    app.session
        .controller
        .set_range(mul, gain, loom_core::ParamRange::new(-0.1, 0.1))
        .unwrap();
    app.session.controller.map_midi(control, mul, gain).unwrap();
    app.commit();
    let fine = spec.in_range(loom_core::ParamRange::new(-0.1, 0.1));
    let note = format!(
        "Low and High are a share of {} to {}, the range exposed on Amp.",
        crate::param::format(&fine, -0.1),
        crate::param::format(&fine, 0.1)
    );
    let inner = inspector_text(&mut app, mul.0);
    assert!(inner.contains(&note), "{note} in {inner}");
}

/// The note names the surface whose range takes part in the span, by the
/// rule `fold_ranges` uses: the outer Voice when its range overlaps the
/// Filter's, the Filter when the Voice's range lies outside it (only a
/// hand-written file makes that).
#[test]
fn the_range_note_names_the_surface_that_sets_the_span() {
    for (voice, named) in [("(500.0, 1000.0)", "Voice"), ("(1.0, 10.0)", "Filter")] {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
        let text = std::fs::read_to_string(root.join("nested-subtractive.ron")).unwrap();
        let text = text
            .replace("range: Some((20.0, 20000.0))", "range: Some((200.0, 2000.0))")
            .replace(
                r#"exposed: [Exposed(name: "cutoff", module: 5, param: "cutoff")]"#,
                &format!(r#"exposed: [Exposed(name: "cutoff", module: 5, param: "cutoff", range: Some({voice}))]"#),
            );
        let mut app = app_with(&text);
        let (svf, cutoff) = svf_cutoff(&app);
        let control = loom_core::MidiControl {
            channel: 0,
            controller: 74,
        };
        app.session.controller.map_midi(control, svf, cutoff).unwrap();
        app.commit();
        let inner = inspector_text(&mut app, svf.0);
        assert!(
            inner.contains(&format!("the range exposed on {named}.")),
            "Voice at {voice}: {inner}"
        );
    }
}

/// The name typed in the sub-patch's inspector renames it as one undo
/// step: the tile and the breadcrumb follow, and the key stays.
#[test]
fn renaming_a_sub_patch_in_the_inspector() {
    let mut app = nested_app();
    app.session.selection = [2].into_iter().collect();
    let _ = app.inspector_message(crate::inspector::Message::SubPatchNameDraft {
        module: 2,
        value: "Lead".to_string(),
    });
    match app.inspector_model() {
        Inspector::SubPatch(rows) => assert_eq!(rows.name, "Lead", "the draft shows while typing"),
        other => panic!("{other:?}"),
    }
    assert_eq!(app.session.hierarchy.tile_name(ModuleId(2)), Some("Voice".to_string()));
    let _ = app.inspector_message(crate::inspector::Message::SubPatchNameApply { module: 2 });
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("rename sub-patch"));
    assert!(titles(&app).iter().any(|t| t == "Lead"));
    assert_eq!(app.session.hierarchy.path_names(&voice()), ["Lead"]);
    assert!(app.current_patch().subpatches.contains_key("voice"), "the key stays");
    app.undo();
    assert!(titles(&app).iter().any(|t| t == "Voice"));
}

/// The sub-patch tile's inspector lists its exposed parameters as rows of
/// the inner parameter, named by the surface name, each with Unexpose on
/// that tile; inside, an exposed parameter's row offers Unexpose on the
/// enclosing sub-patch and hides its Range selector, and the others offer
/// Expose on it. At the top level there is nothing to expose onto.
#[test]
fn the_inspector_offers_expose_and_shows_surface_rows() {
    use loom_core::ModuleNames;
    let (mut ui, svf, cutoff) = nested_example_ui();
    let app = &mut ui.app;
    let filter_tile = app.session.hierarchy.id_at(&ModulePath::top(2), 5).unwrap();
    let parts = |e: &crate::inspector::ExposureRow| (e.module, e.index, e.name.clone(), e.surface.clone());
    app.session.selection = [2].into_iter().collect();
    match app.inspector_model() {
        Inspector::SubPatch(rows) => {
            assert_eq!(rows.params.len(), 1);
            assert_eq!(rows.params[0].label, "cutoff");
            assert_eq!((rows.params[0].module, rows.params[0].row.index), (svf.0, cutoff));
            assert_eq!(
                rows.params[0].row.exposure, None,
                "the top level has nothing to expose onto"
            );
            assert_eq!(rows.params[0].row.range, None);
            assert_eq!(
                parts(&rows.params[0].own),
                (2, 0, Some("cutoff".to_string()), "Voice".to_string()),
                "the tile's own surface offers Unexpose"
            );
        }
        other => panic!("{other:?}"),
    }
    app.session.selection = [1].into_iter().collect();
    match app.inspector_model() {
        Inspector::Module(rows) => assert!(rows.params.iter().all(|p| p.exposure.is_none())),
        other => panic!("{other:?}"),
    }
    let filter = ModulePath::top(2).child(5);
    app.show_level(&filter);
    app.session.selection = [svf.0].into_iter().collect();
    let resonance = app
        .session
        .controller
        .graph()
        .node(svf)
        .unwrap()
        .parameter_index("resonance")
        .unwrap();
    match app.inspector_model() {
        Inspector::Module(rows) => {
            let row = |index: usize| rows.params.iter().find(|p| p.index == index).unwrap().clone();
            assert_eq!(
                parts(&row(cutoff).exposure.unwrap()),
                (filter_tile.0, 0, Some("cutoff".to_string()), "Filter".to_string()),
                "Unexpose names the Filter's surface item"
            );
            assert_eq!(
                row(cutoff).range,
                None,
                "the surface's range stands in for the selector"
            );
            assert_eq!(
                parts(&row(resonance).exposure.unwrap()),
                (svf.0, resonance, None, "Filter".to_string())
            );
        }
        other => panic!("{other:?}"),
    }
    // Inside the Voice, the Filter tile's surface row is exposed on the
    // Voice too, and can be taken off either surface.
    app.show_level(&ModulePath::top(2));
    app.session.selection = [filter_tile.0].into_iter().collect();
    match app.inspector_model() {
        Inspector::SubPatch(rows) => {
            assert_eq!(
                parts(&rows.params[0].row.exposure.clone().unwrap()),
                (2, 0, Some("cutoff".to_string()), "Voice".to_string()),
                "Unexpose on Voice names the Voice's surface item"
            );
            assert_eq!(
                parts(&rows.params[0].own),
                (filter_tile.0, 0, Some("cutoff".to_string()), "Filter".to_string())
            );
        }
        other => panic!("{other:?}"),
    }
}

/// `--select` takes a sub-patch tile, so `--dump-ui --select 2` on the
/// nested example shows the Voice tile's inspector; an id no tile has is
/// still refused.
#[test]
fn select_takes_a_sub_patch_tile() {
    let mut args = crate::tests::app_args();
    args.patch = Some(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/nested-subtractive.ron"),
    );
    args.no_audio = true;
    args.select = vec![2];
    let app = crate::snapshot::headless(&args).unwrap();
    assert!(matches!(app.inspector_model(), Inspector::SubPatch(_)));
    args.select = vec![999];
    assert_eq!(
        crate::snapshot::headless(&args).err().as_deref(),
        Some("no module 999 in the patch")
    );
}

/// Adds an LFO inside the Voice and assigns it to the filter's cutoff
/// through the Filter tile's surface. Returns the LFO's id.
fn lfo_inside_voice(app: &mut App, svf: ModuleId, cutoff: usize) -> ModuleId {
    app.show_level(&voice());
    let lfo = app.modules.iter().copied().find(|d| d.id == "loom.mod.lfo").unwrap();
    app.add_module(lfo, Point::new(100.0, 300.0));
    let id = ModuleId(*app.session.selection.iter().next().expect("the new module is selected"));
    app.assign(loom_core::OutputRef { module: id, port: 0 }, svf, cutoff);
    id
}

/// Unexpose on the Voice's surface removes the top-level assignment that
/// reached the cutoff through it, and keeps the one from an LFO inside the
/// Voice, which still reaches the cutoff through the Filter's surface.
#[test]
fn unexposing_on_an_outer_surface_keeps_assignments_still_exposed_below() {
    let (mut ui, svf, cutoff) = nested_example_ui();
    let app = &mut ui.app;
    app.assign(
        loom_core::OutputRef {
            module: ModuleId(1),
            port: 0,
        },
        svf,
        cutoff,
    );
    let lfo = lfo_inside_voice(app, svf, cutoff);
    assert_eq!(app.session.controller.graph().assignments().count(), 2);

    app.unexpose(2, 0);
    let sources: Vec<ModuleId> = app
        .session
        .controller
        .graph()
        .assignments()
        .map(|(_, a)| a.source.module)
        .collect();
    assert_eq!(sources, [lfo], "only the assignment with no surface left goes");
    assert!(
        app.session
            .hierarchy
            .definition_of(ModuleId(2))
            .unwrap()
            .exposed
            .is_empty()
    );
    let filter = app.session.hierarchy.id_at(&voice(), 5).unwrap();
    assert_eq!(app.session.hierarchy.definition_of(filter).unwrap().exposed.len(), 1);
    app.undo();
    assert_eq!(app.session.controller.graph().assignments().count(), 2);
}

/// A retarget onto an inner parameter puts the assignment behind the inner
/// ones in the file's order, which gives it a new id; the matrix selection
/// follows it, and the old id is gone.
#[test]
fn a_retarget_onto_an_inner_parameter_keeps_the_matrix_selection() {
    use loom_core::ModuleNames;
    let (mut ui, svf, cutoff) = nested_example_ui();
    let app = &mut ui.app;
    let note_pitch = loom_core::OutputRef {
        module: ModuleId(1),
        port: 0,
    };
    // First an outer assignment onto a top-level parameter, then an inner
    // one onto the cutoff, so the inner one has the higher id.
    let scope = ModuleId(11);
    let window = app
        .session
        .controller
        .graph()
        .node(scope)
        .unwrap()
        .parameter_index("window")
        .unwrap();
    app.assign(note_pitch, scope, window);
    let outer = app.session.controller.graph().assignments().next().unwrap().0;
    let lfo = lfo_inside_voice(app, svf, cutoff);
    app.show_level(&ModulePath::root());
    assert_eq!(app.session.controller.graph().assignments().count(), 2);
    app.session.matrix.selected = Some(outer);

    app.matrix_retarget(outer, None, Some((svf, cutoff)));
    let graph = app.session.controller.graph();
    assert!(graph.assignment(outer).is_none(), "the moved assignment got a new id");
    let (moved, _) = graph
        .assignments()
        .find(|(_, a)| a.source == note_pitch && a.target == svf && a.param == cutoff)
        .expect("retargeted onto the cutoff");
    assert!(moved > outer);
    assert_eq!(
        app.session.matrix.selected,
        Some(moved),
        "the matrix follows the new id"
    );
    let order: Vec<ModuleId> = graph.assignments().map(|(_, a)| a.source.module).collect();
    assert_eq!(order, [lfo, ModuleId(1)], "inner first, as the file loads them");
}
