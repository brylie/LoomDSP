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
