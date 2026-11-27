use super::*;
use crate::ui_test::{Ui, key_press};
use loom_ui_core::{Key, Modifiers};

const NONE: Modifiers = Modifiers::NONE;

/// The examples keep their saved module IDs.
fn row(module: u32) -> UiId {
    module_id(module)
}

/// Reads the outline's focus through the same tree a screen reader gets.
fn focused(ui: &mut Ui) -> UiId {
    ui.a11y_tree().focused().ui_id().expect("a named focus").clone()
}

/// Pushes a key by position with the letter a particular layout gave it.
fn physical(ui: &mut Ui, code: Code, letter: char, modifiers: Modifiers) {
    let mut press = key_press(Key::Character(letter), modifiers, Some(&letter.to_string()), false);
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
    physical(&mut ui, Code::BracketRight, '+', NONE);
    assert_eq!(ui.app().session.selection, [filter].into());
    ui.redraw();
    let canvas = ui.canvas();
    let centre = ui.app().session.scene.module(filter).unwrap().bounds().center();
    let offset = canvas.camera.visible(canvas.bounds.size()).center() - centre;
    assert!(offset.x.hypot(offset.y) < 0.01);
    physical(&mut ui, Code::BracketRight, '+', NONE);
    assert!(ui.app().session.outline.has_jump());
    assert_eq!(ui.a11y_tree().focused().role(), accesskit::Role::ListBoxOption);
    ui.tap(Key::Escape, NONE);
    physical(&mut ui, Code::BracketLeft, 'ü', Modifiers::platform_shortcut());
    assert_eq!(ui.app().session.selection, [saw].into());
    physical(&mut ui, Code::BracketRight, '+', NONE);
    physical(&mut ui, Code::BracketLeft, 'ü', NONE);
    if ui.app().session.outline.has_jump() {
        let picked = ui
            .app()
            .session
            .outline
            .jump
            .as_ref()
            .unwrap()
            .choices
            .iter()
            .position(|choice| choice.target == Target::Tile(saw))
            .unwrap();
        for _ in 0..picked {
            ui.tap(Key::Down, NONE);
        }
        ui.tap(Key::Enter, NONE);
    }
    assert_eq!(ui.app().session.selection, [saw].into());
    assert_eq!(ui.app().session.edits.history.len(), initial);
}

/// Outline navigation preserves focus by row ID and keeps edit keys away from the canvas.
#[test]
fn outline_keyboard_moves_and_expands_rows_without_editing() {
    let mut ui = Ui::open("subtractive.ron");
    let first_count = ui.app().session.scene.modules().len();
    let saw = tile_named(ui.app(), "Saw");
    ui.tap(Key::Character('o'), NONE);
    assert_eq!(focused(&mut ui), row_id("summary".into()));
    ui.tap(Key::Down, NONE);
    assert_eq!(focused(&mut ui), row(tile_named(ui.app(), "MIDI Note")));
    ui.tap(Key::Up, NONE);
    assert_eq!(focused(&mut ui), row_id("summary".into()));
    ui.tap(Key::End, NONE);
    let end = ui.app().outline_rows().last().unwrap().id.clone();
    assert_eq!(focused(&mut ui), end);
    ui.tap(Key::Home, NONE);
    assert_eq!(focused(&mut ui), row_id("summary".into()));
    ui.tap(Key::Character('s'), NONE);
    assert_eq!(focused(&mut ui), row(saw));
    ui.tap(Key::Right, NONE);
    ui.tap(Key::Right, NONE);
    // On the Saw's first connection: a row of its own, one level in, with
    // the Saw as its parent (each row has one key under the list).
    let child = focused(&mut ui);
    assert!(
        ui.app()
            .outline_rows()
            .iter()
            .any(|r| r.id == child && r.parent.as_ref() == Some(&row(saw))),
        "{child}"
    );
    ui.tap(Key::Left, NONE);
    assert_eq!(focused(&mut ui), row(saw));
    ui.tap(Key::Space, NONE);
    ui.tap(Key::Delete, NONE);
    assert_eq!(ui.app().session.scene.modules().len(), first_count);
    ui.tap(Key::Escape, NONE);
    assert_eq!(ui.app().right_tab, crate::matrix::Tab::Outline);
    ui.tap(Key::Tab, Modifiers::CONTROL);
    assert_eq!(focused(&mut ui), row(saw));
}

/// Map visibility cycles from the canvas and the map takes clicks over tiles.
#[test]
fn map_visibility_and_clicks_use_the_live_camera() {
    use crate::preferences::{MapMode, Store};
    let tmp = crate::recovery::tests::TempDir::new("map-mode");
    let preferences = tmp.0.join("stage.ron");
    let mut app = crate::ui_test::open_app("subtractive.ron");
    app.rig.store = Store::at(Some(preferences.clone()));
    let mut ui = Ui::with_app(app);
    let mode = |ui: &Ui| ui.app().rig.store.saved().patch_map;
    assert_eq!(mode(&ui), MapMode::Auto);
    let view = ui.canvas();
    let scene = &ui.app().session.scene;
    let map = ui.app().map_view().unwrap();
    let world = scene.bounds().unwrap();
    let mut camera = view.camera;
    camera.frame(world, view.bounds.size());
    assert!(map.bounds(scene, camera, view.bounds).is_none());
    camera.zoom_at(Point::new(view.bounds.width / 2.0, view.bounds.height / 2.0), 3.0);
    assert!(map.bounds(scene, camera, view.bounds).is_some());

    ui.tap(Key::Character('m'), NONE);
    let canvas = ui.canvas();
    let area = ui
        .app()
        .map_view()
        .unwrap()
        .bounds(&ui.app().session.scene, canvas.camera, canvas.bounds)
        .unwrap();
    assert_eq!(mode(&ui), MapMode::Always);
    assert_eq!(
        Store::open(Some(preferences.clone())).0.saved().patch_map,
        MapMode::Always
    );
    assert_eq!(ui.a11y_tree().node("Patch map").role(), accesskit::Role::Image);
    ui.move_to(Point::new(area.x + 2.0, area.y + 2.0));
    ui.press();
    ui.release();
    assert_ne!(ui.canvas().camera.origin, canvas.camera.origin);
    ui.tap(Key::Character('m'), NONE);
    assert_eq!(mode(&ui), MapMode::Hidden);
    assert_eq!(Store::open(Some(preferences)).0.saved().patch_map, MapMode::Hidden);
    assert!(ui.app().map_view().is_none());
    ui.tap(Key::Character('m'), NONE);
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
    let report = &app.session.controller.last_report().loops[0];
    assert_eq!(
        kids,
        report.run_order.iter().map(|id| module_id(id.0)).collect::<Vec<_>>()
    );
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

/// Presses the next or previous region shortcut.
fn region(ui: &mut Ui, backwards: bool) {
    let modifiers = if backwards {
        Modifiers::CONTROL | Modifiers::SHIFT
    } else {
        Modifiers::CONTROL
    };
    ui.tap(Key::Tab, modifiers);
}

/// Selects a tile, opens its multi-target jump list, and checks the spoken choice.
fn open_branch(ui: &mut Ui) {
    let filter = tile_named(ui.app(), "SVF");
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![filter])));
    physical(ui, Code::BracketRight, ']', NONE);
    let jump = ui.app().session.outline.jump.as_ref().expect("two destinations");
    assert_eq!(
        jump.choices
            .iter()
            .map(|choice| choice.name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Lowpass to Multiply 8 A", "Lowpass to Scope In"])
    );
}

/// Save and Run reach the app while a jump list is open. A canvas press dismisses it.
#[test]
fn the_jump_list_only_takes_its_navigation_keys_and_closes_on_click() {
    use crate::files::fake;
    let mut ui = Ui::with_app(crate::tests::app_with_text(include_str!(
        "../../../../examples/patches/subtractive.ron"
    )));
    open_branch(&mut ui);

    let _ = fake::asked();
    fake::answer_save(None);
    ui.tap(Key::Character('s'), Modifiers::platform_shortcut());
    assert!(matches!(fake::asked().as_slice(), [fake::Asked::Save(_)]));
    let log = ui.tap(Key::Space, NONE);
    assert!(
        log.iter()
            .any(|message| matches!(message, Message::Canvas(loom_canvas::CanvasEvent::ToggleRun)))
    );
    assert!(ui.app().session.outline.has_jump());

    let at = ui.canvas().bounds.center();
    ui.move_to(at);
    ui.press();
    ui.release();
    assert!(!ui.app().session.outline.has_jump());
    fake::answer_save(None);
    ui.tap(Key::Character('s'), Modifiers::platform_shortcut());
    assert!(matches!(fake::asked().as_slice(), [fake::Asked::Save(_)]));
}

/// A tab change and removing the branch's source also dismiss the choices.
#[test]
fn leaving_a_tab_or_deleting_the_jump_source_dismisses_the_list() {
    let mut ui = Ui::open("subtractive.ron");
    open_branch(&mut ui);
    ui.send(Message::Matrix(crate::matrix::Event::Tab(crate::matrix::Tab::Outline)));
    assert!(!ui.app().session.outline.has_jump());
    ui.send(Message::Matrix(crate::matrix::Event::Tab(
        crate::matrix::Tab::Inspector,
    )));
    open_branch(&mut ui);
    let filter = tile_named(ui.app(), "SVF");
    let centre = ui.canvas().bounds.center();
    ui.move_to(centre);
    ui.tap(Key::Delete, NONE);
    assert!(ui.app().session.scene.module(filter).is_none());
    assert!(!ui.app().session.outline.has_jump());
}

/// The region shortcut focuses a real Inspector stop and returns to the canvas control.
#[test]
fn ctrl_tab_restores_canvas_and_inspector_focus_in_both_directions() {
    let mut ui = Ui::open("subtractive.ron");
    let filter = tile_named(ui.app(), "SVF");
    ui.tap(Key::Character('o'), NONE);
    ui.send(Message::OutlineRow(row(filter)));
    ui.tap(Key::Enter, NONE);
    ui.redraw();
    let canvas = focused(&mut ui);
    assert!(canvas.is_under(&crate::a11y::tree::tile_id(filter)), "{canvas}");
    ui.send(Message::Matrix(crate::matrix::Event::Tab(
        crate::matrix::Tab::Inspector,
    )));

    region(&mut ui, false);
    let inspector = focused(&mut ui);
    assert!(inspector.is_under(&crate::inspector::ids::panel(filter)), "{inspector}");
    region(&mut ui, true);
    ui.redraw();
    assert_eq!(focused(&mut ui), canvas);
    region(&mut ui, true);
    assert!(focused(&mut ui).is_under(&crate::a11y::tree::toolbar_id()));
    region(&mut ui, true);
    assert_eq!(focused(&mut ui), inspector);
    region(&mut ui, false);
    region(&mut ui, false);
    ui.redraw();
    assert_eq!(focused(&mut ui), canvas);
}

/// Focus on the outline wraps through the toolbar and canvas without changing its row.
#[test]
fn region_keys_wrap_around_and_restore_the_outline_row() {
    let mut ui = Ui::open("subtractive.ron");
    ui.tap(Key::Character('o'), NONE);
    ui.tap(Key::Down, NONE);
    let outline = focused(&mut ui);
    region(&mut ui, true);
    assert!(!focused(&mut ui).is_under(&tree_id()));
    region(&mut ui, true);
    assert!(focused(&mut ui).is_under(&crate::a11y::tree::toolbar_id()));
    region(&mut ui, true);
    assert_eq!(focused(&mut ui), outline);
    region(&mut ui, false);
    assert!(focused(&mut ui).is_under(&crate::a11y::tree::toolbar_id()));
    region(&mut ui, false);
    assert!(!focused(&mut ui).is_under(&tree_id()));
    region(&mut ui, false);
    assert_eq!(focused(&mut ui), outline);
}

/// Returning with O remembers the module row after Enter focuses its first control.
#[test]
fn a_module_row_enters_the_canvas_and_o_returns_to_the_same_row() {
    let mut ui = Ui::open("subtractive.ron");
    let filter = tile_named(ui.app(), "SVF");
    ui.tap(Key::Character('o'), NONE);
    ui.send(Message::OutlineRow(row(filter)));
    ui.tap(Key::Enter, NONE);
    ui.redraw();
    assert!(ui.app().a11y.focus().is_some_and(|(id, _)| id == filter));
    ui.tap(Key::Escape, NONE);
    assert!(!focused(&mut ui).is_under(&tree_id()));
    ui.tap(Key::Character('o'), NONE);
    assert_eq!(focused(&mut ui), row(filter));
}

/// The single name across the tile header, inspector, matrix and outline includes local IDs.
#[test]
fn module_names_are_shared_across_navigation_views_and_the_matrix() {
    use loom_core::{ModuleId, OutputRef};
    let mut app = crate::ui_test::open_app("subtractive.ron");
    let source = OutputRef {
        module: ModuleId(5),
        port: 0,
    };
    let cutoff = loom_modules::filter::SVF.param_index("cutoff").unwrap();
    app.session
        .controller
        .add_assignment(source, ModuleId(4), cutoff, 0.5)
        .unwrap();
    let pitch = OutputRef {
        module: ModuleId(1),
        port: 0,
    };
    app.session
        .controller
        .add_assignment(pitch, ModuleId(4), cutoff, 0.5)
        .unwrap();
    app.commit();
    for (id, name) in [(5, "Envelope 5"), (1, "MIDI Note")] {
        assert_eq!(app.session.scene.module(id).unwrap().title, name);
        assert!(
            app.outline_rows()
                .iter()
                .any(|row| row.id == module_id(id) && row.name.starts_with(name))
        );
        app.session.selection = [id].into();
        let crate::inspector::Inspector::Module(module) = app.inspector_model() else {
            panic!("module inspector");
        };
        assert_eq!(module.name, name);
        assert!(
            app.ui_snapshot()
                .matrix
                .rows
                .iter()
                .any(|row| row.source.starts_with(&format!("{name}:")))
        );
    }
}

/// Depth edits keep the structure and drawing. Switching off keeps order and changes speech and line style.
#[test]
fn assignment_depth_and_enabled_state_have_separate_map_updates() {
    use loom_core::AssignmentId;
    let mut app = crate::ui_test::open_app("modulation.ron");
    let lfo = tile_named(&app, "LFO");
    let filter = tile_named(&app, "SVF");
    let map = &app.session.patch_map;
    let order = map.order.clone();
    let id = map
        .structure
        .modulations
        .iter()
        .find(|a| a.from.module == lfo)
        .unwrap()
        .id;
    let drawing = map.drawing.lines.clone();
    let generation = map.generation;
    let pos = |module| order.iter().position(|unit| *unit == Unit::Tile(module)).unwrap();
    assert!(pos(lfo) < pos(filter));
    assert!(
        app.outline_rows()
            .iter()
            .find(|r| r.id == row(filter))
            .unwrap()
            .description
            .contains("Cutoff modulated by LFO")
    );
    app.session.outline.set_expanded(row(lfo), true);
    let original_rows = app.outline_rows();
    assert!(Rc::ptr_eq(&original_rows, &app.outline_rows()));

    app.modulation_message(crate::modulation::Message::Depth {
        id: AssignmentId(id),
        depth: -0.5,
    });
    assert_eq!(app.session.patch_map.generation, generation);
    assert_eq!(app.session.patch_map.drawing.lines, drawing);
    assert!(!Rc::ptr_eq(&original_rows, &app.outline_rows()));
    let assignment_name = |app: &App| {
        app.outline_rows()
            .iter()
            .find(|r| r.target == Some(Target::Assignment { id, source: true }))
            .unwrap()
            .name
            .clone()
    };
    let enabled = assignment_name(&app);
    assert!(enabled.contains("-50%"), "{enabled}");

    app.modulation_message(crate::modulation::Message::Enabled {
        id: AssignmentId(id),
        enabled: false,
    });
    assert_eq!(app.session.patch_map.order, order);
    assert_eq!(assignment_name(&app), format!("{enabled}, off"));
    let line = |app: &App| {
        app.session
            .patch_map
            .drawing
            .lines
            .iter()
            .find(|line| line.from == lfo && line.to == filter)
            .unwrap()
            .dash
    };
    assert_eq!(line(&app), 1.0);

    app.modulation_message(crate::modulation::Message::Enabled {
        id: AssignmentId(id),
        enabled: true,
    });
    assert_eq!(assignment_name(&app), enabled);
    assert_eq!(line(&app), 4.0);
    assert_eq!(app.session.patch_map.order, order);
}

/// None of the large neighbour list is lost from the mixer's expanded connections.
#[test]
fn a_twelve_input_mixer_reads_three_neighbours_and_lists_every_connection() {
    use crate::patch_map::{Cable, PortInfo, TileInfo};
    use loom_ui_core::Color;
    let mut map = PatchMap::default();
    let mixer = 13;
    for id in 1..mixer {
        let outgoing = PortRef {
            module: id,
            index: 0,
            output: true,
        };
        map.structure.tiles.insert(
            id,
            TileInfo {
                name: format!("Source {id}"),
                kind: "Generator".into(),
                instance: false,
                output: false,
                inputs: Vec::new(),
                outputs: vec![PortInfo {
                    key: "out".into(),
                    label: "Out".into(),
                }],
            },
        );
        map.structure.cables.push(Cable {
            from: outgoing,
            to: PortRef {
                module: mixer,
                index: (id - 1) as usize,
                output: false,
            },
            colour: Color::WHITE,
            stereo: false,
            signal: loom_core::SignalType::Audio,
        });
    }
    map.structure.tiles.insert(
        mixer,
        TileInfo {
            name: "Mixer".into(),
            kind: "Mixer".into(),
            instance: false,
            output: false,
            inputs: (1..=12)
                .map(|n| PortInfo {
                    key: format!("in{n}"),
                    label: format!("In {n}"),
                })
                .collect(),
            outputs: Vec::new(),
        },
    );
    let mut state = State::default();
    state.set_expanded(row(mixer), true);
    let mut rows = Vec::new();
    module_rows(&mut rows, &map, &state, mixer, 1, None);
    assert!(
        rows[0].name.contains("Source 1, Source 2, Source 3 and 9 more"),
        "{}",
        rows[0].name
    );
    assert_eq!(rows.len(), 13);
    for (offset, connection) in rows[1..].iter().enumerate() {
        let input = offset + 1;
        assert!(connection.name.contains(&format!("In {input}")), "{}", connection.name);
        assert!(
            connection.name.ends_with(&format!("Source {input} Out")),
            "{}",
            connection.name
        );
    }
}

/// Panning and moving tiles preserve the structure. A cable edit replaces it.
#[test]
fn drawing_changes_preserve_the_structure_and_a_cable_rebuilds_it() {
    use loom_canvas::CanvasEvent;
    use loom_ui_core::Vector;
    let mut ui = Ui::open("subtractive.ron");
    let app = ui.app();
    let initial = app.session.patch_map.generation;
    let scope = tile_named(app, "Scope");
    let saw = tile_named(app, "Saw");
    let multiply = tile_named(app, "Multiply 6");
    ui.tap(Key::Character('o'), NONE);
    ui.send(Message::OutlineRow(row(scope)));
    let mut camera = ui.canvas().camera;
    camera.origin += Vector::new(50.0, 10.0);
    ui.send(Message::Canvas(CanvasEvent::Camera(camera)));
    assert_eq!(ui.app().session.patch_map.generation, initial);
    let before_move = ui.app().session.patch_map.order.clone();
    ui.send(Message::Canvas(CanvasEvent::GestureStart));
    ui.send(Message::Canvas(CanvasEvent::Move {
        ids: vec![scope],
        delta: Vector::new(0.0, -600.0),
    }));
    assert_eq!(ui.app().session.patch_map.order, before_move);
    ui.send(Message::Canvas(CanvasEvent::GestureEnd));
    assert_eq!(ui.app().session.patch_map.generation, initial);
    assert_eq!(focused(&mut ui), row(scope));

    ui.app_mut()
        .session
        .controller
        .connect_named(loom_core::ModuleId(saw), "out", loom_core::ModuleId(multiply), "b")
        .unwrap();
    ui.app_mut().commit();
    assert_ne!(ui.app().session.patch_map.generation, initial);
}

/// Creates an audible feedback loop with a user delay or an assignment as its break.
fn feedback_fixture(with_delay: bool) -> (App, loom_core::ModuleId, loom_core::ModuleId) {
    let mut app = crate::tests::app();
    let c = &mut app.session.controller;
    let out = c.add_module("loom.io.audio-out").unwrap();
    let a = c.add_module("loom.math.mul").unwrap();
    let b = c
        .add_module(if with_delay {
            "loom.time.delay1"
        } else {
            "loom.math.mul"
        })
        .unwrap();
    c.connect_named(a, "out", out, "in").unwrap();
    c.connect_named(a, "out", b, if with_delay { "in" } else { "a" })
        .unwrap();
    if with_delay {
        c.connect_named(b, "out", a, "a").unwrap();
    } else {
        c.add_assignment_named(b, "out", a, "gain", 0.25).unwrap();
    }
    app.commit();
    (app, a, b)
}

/// The loop row names the actual delay module or the delayed modulation.
#[test]
fn loop_rows_describe_user_delays_and_assignment_delays() {
    for with_delay in [true, false] {
        let (app, a, b) = feedback_fixture(with_delay);
        let rows = app.outline_rows();
        let loop_row = rows
            .iter()
            .find(|row| row.name.starts_with("Feedback loop 1:"))
            .unwrap();
        let report = &app.session.controller.last_report().loops[0];
        let structure = &app.session.patch_map.structure;
        let expected = if with_delay {
            assert!(report.inserted_delays.is_empty() && report.inserted_assignment_delays.is_empty());
            format!("Unit delay: {}", structure.name(b.0))
        } else {
            assert_eq!(report.inserted_assignment_delays.len(), 1);
            format!(
                "Delay inserted on {}'s modulation of {} Gain",
                structure.name(b.0),
                structure.name(a.0)
            )
        };
        assert_eq!(loop_row.description, expected);
    }
}

/// Verifies the compiler run order through a loop whose IDs run in a different order.
#[test]
fn loop_rows_keep_run_order_when_it_differs_from_id_order() {
    let mut app = crate::tests::app();
    let c = &mut app.session.controller;
    let a = c.add_module("loom.math.mul").unwrap();
    let b = c.add_module("loom.math.mul").unwrap();
    let d = c.add_module("loom.math.mul").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(a, "out", d, "a").unwrap();
    c.connect_named(d, "out", b, "a").unwrap();
    c.connect_named(b, "out", a, "a").unwrap();
    c.connect_named(a, "out", out, "in").unwrap();
    app.commit();

    let report = &app.session.controller.last_report().loops[0];
    assert_ne!(report.run_order, report.modules);
    let rows = app.outline_rows();
    let group = rows
        .iter()
        .find(|row| row.name.starts_with("Feedback loop 1:"))
        .unwrap();
    let children: Vec<_> = rows
        .iter()
        .filter(|r| r.parent.as_ref() == Some(&group.id))
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(
        children,
        report.run_order.iter().map(|id| row(id.0)).collect::<Vec<_>>()
    );
}

/// Two assignments on one output have distinct source-side row IDs.
#[test]
fn assignments_from_an_output_keep_distinct_rows() {
    use loom_core::ModuleId;
    let mut app = crate::ui_test::open_app("modulation.ron");
    let lfo = tile_named(&app, "LFO");
    let filter = tile_named(&app, "SVF");
    let second = app
        .session
        .controller
        .add_assignment_named(ModuleId(lfo), "out", ModuleId(filter), "resonance", 0.5)
        .unwrap();
    app.commit();
    app.session.outline.set_expanded(row(lfo), true);
    let rows = app.outline_rows();
    let ids: Vec<_> = rows
        .iter()
        .filter(|r| r.parent == Some(row(lfo)))
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert!(ids.contains(&row_id(format!("module-{lfo}-modulation-out-{}", second.0))));
}

/// Downstream and Back work from outline rows. A port's choice can reveal a collapsed section.
#[test]
fn outline_cable_choices_reveal_a_destination_in_a_collapsed_section() {
    let mut ui = Ui::open("subtractive.ron");
    let saw = tile_named(ui.app(), "Saw");
    let filter = tile_named(ui.app(), "SVF");
    let scope = tile_named(ui.app(), "Scope");
    ui.tap(Key::Character('o'), NONE);
    ui.send(Message::OutlineRow(row(saw)));
    physical(&mut ui, Code::BracketRight, ']', NONE);
    assert_eq!(focused(&mut ui), row(filter));
    physical(&mut ui, Code::BracketLeft, '[', Modifiers::platform_shortcut());
    assert_eq!(focused(&mut ui), row(saw));

    let section = row_id("not-heard".into());
    ui.send(Message::OutlineRow(section.clone()));
    ui.tap(Key::Left, NONE);
    assert!(!ui.app().outline_rows().iter().any(|r| r.id == row(scope)));
    ui.send(Message::OutlineRow(row(filter)));
    ui.tap(Key::Right, NONE);
    let output = row_id(format!("module-{filter}-output-lowpass"));
    ui.send(Message::OutlineRow(output));
    ui.tap(Key::Enter, NONE);
    assert!(ui.app().session.outline.has_jump());
    ui.tap(Key::Down, NONE);
    ui.tap(Key::Up, NONE);
    assert_eq!(ui.a11y_tree().focused().name(), Some("Lowpass to Scope In"));
    ui.tap(Key::Enter, NONE);
    assert_eq!(focused(&mut ui), row(scope));
    assert!(
        ui.app()
            .outline_rows()
            .iter()
            .any(|r| r.id == section && r.expanded == Some(true))
    );
}

/// Down visits every tile while modules start collapsed. Pan and activity keep the same sections.
#[test]
fn down_visits_every_tile_and_monitor_only_sections_survive_a_pan() {
    let mut app = crate::ui_test::open_app("subtractive.ron");
    let scope = tile_named(&app, "Scope");
    let c = &mut app.session.controller;
    let monitor = c.add_module("loom.osc.sine").unwrap();
    c.connect_named(monitor, "out", loom_core::ModuleId(scope), "in")
        .unwrap();
    app.commit();
    let mut ui = Ui::with_app(app);
    let heard = &ui.app().session.patch_map.structure.heard;
    assert!(!heard.contains(&scope) && !heard.contains(&monitor.0));
    ui.tap(Key::Character('o'), NONE);
    let rows = ui.app().outline_rows();
    let mut visited = BTreeSet::new();
    for expected in rows.iter().skip(1) {
        ui.tap(Key::Down, NONE);
        assert_eq!(focused(&mut ui), expected.id);
        if let Some(Target::Tile(module)) = expected.target {
            assert_ne!(expected.expanded, Some(true));
            visited.insert(module);
        }
    }
    assert_eq!(visited.len(), ui.app().session.scene.modules().len());

    let mut camera = ui.canvas().camera;
    camera.origin.x += 600.0;
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Camera(camera)));
    for active in [true, false] {
        ui.send(Message::Preferences(crate::settings::Message::Activity(active)));
        assert_eq!(&*rows, &*ui.app().outline_rows());
    }
}

/// When the entire patch reaches the output, there is no Not heard section.
#[test]
fn an_all_heard_patch_has_no_not_heard_section() {
    let app = crate::tests::app_with_text(
        r#"Patch(
        version: 1,
        metadata: Metadata(name: "Heard"),
        modules: [
            PatchModule(id: 1, kind: "loom.osc.sine@1"),
            PatchModule(id: 2, kind: "loom.io.audio-out@1"),
            PatchModule(id: 3, kind: "loom.clock.transport@1"),
        ],
        cables: [
            PatchCable(from: (1, "out"), to: (2, "in")),
            PatchCable(from: (3, "beat"), to: (1, "phase")),
        ],
    )"#,
    );
    assert_eq!(app.session.patch_map.structure.heard, [1, 2, 3].into());
    assert!(!app.outline_rows().iter().any(|r| r.id == row_id("not-heard".into())));
}

/// Reading-order ties update when a move ends. Swapping two tiles preserves row identity.
#[test]
fn tile_moves_reorder_independent_modules_only_at_the_end() {
    use loom_canvas::CanvasEvent;
    use loom_ui_core::Vector;
    let mut app = crate::tests::app();
    let first = app.session.controller.add_module("loom.osc.sine").unwrap();
    let second = app.session.controller.add_module("loom.osc.sine").unwrap();
    app.session.positions.insert(first.0, (0.0, 0.0));
    app.session.positions.insert(second.0, (0.0, 500.0));
    app.commit();
    let mut ui = Ui::with_app(app);
    ui.tap(Key::Character('o'), NONE);
    ui.send(Message::OutlineRow(row(second.0)));
    let order = [Unit::Tile(first.0), Unit::Tile(second.0)];
    let moved_tiles = |ui: &Ui| {
        ui.app()
            .session
            .patch_map
            .order
            .iter()
            .copied()
            .filter(|unit| order.contains(unit))
            .collect::<Vec<_>>()
    };
    assert_eq!(moved_tiles(&ui), order);
    ui.send(Message::Canvas(CanvasEvent::GestureStart));
    ui.send(Message::Canvas(CanvasEvent::Move {
        ids: vec![second.0],
        delta: Vector::new(0.0, -700.0),
    }));
    assert_eq!(moved_tiles(&ui), order);
    ui.send(Message::Canvas(CanvasEvent::GestureEnd));
    assert_eq!(moved_tiles(&ui), [order[1], order[0]]);
    assert_eq!(focused(&mut ui), row(second.0));
}

/// The right column returns to the Inspector field last reached with plain Tab.
#[test]
fn region_keys_remember_a_later_inspector_field() {
    let mut ui = Ui::open("subtractive.ron");
    region(&mut ui, false);
    assert_eq!(focused(&mut ui), crate::patch_info::authors_part());
    ui.tap(Key::Tab, NONE);
    assert_eq!(focused(&mut ui), crate::patch_info::tags_part());
    region(&mut ui, false);
    assert!(focused(&mut ui).is_under(&crate::a11y::tree::toolbar_id()));
    region(&mut ui, true);
    assert_eq!(focused(&mut ui), crate::patch_info::tags_part());
}

/// In the toolbar region, the arrow keys, Home, and End move the runtime's
/// focus among the turned-on buttons, Ctrl+Tab comes back to the button
/// last focused, and Escape returns to the canvas. The toolbar's focus is
/// the runtime's, so the ring and the screen reader agree on one button.
#[test]
fn toolbar_region_keys_move_the_runtime_focus() {
    let mut ui = Ui::open("subtractive.ron");
    let stops = ui.app().toolbar_stops();
    assert!(stops.len() > 2, "the toolbar has buttons to walk");
    // From the canvas, the toolbar is the region before it.
    region(&mut ui, true);
    assert_eq!(
        ui.runtime().focused(),
        Some(&stops[0]),
        "the first button the first time"
    );
    assert_eq!(focused(&mut ui), stops[0]);
    ui.tap(Key::Right, NONE);
    assert_eq!(ui.runtime().focused(), Some(&stops[1]));
    ui.tap(Key::Left, NONE);
    ui.tap(Key::Left, NONE);
    assert_eq!(ui.runtime().focused(), Some(&stops[0]), "Left stops at the first");
    ui.tap(Key::End, NONE);
    let last = stops.last().unwrap();
    assert_eq!(ui.runtime().focused(), Some(last));
    assert_eq!(focused(&mut ui), *last);
    ui.tap(Key::Home, NONE);
    ui.tap(Key::Right, NONE);
    // Away to the right column and back: the toolbar remembers its button.
    region(&mut ui, true);
    assert!(!ui.app().toolbar_has_focus());
    region(&mut ui, false);
    assert_eq!(ui.runtime().focused(), Some(&stops[1]), "the button last focused");
    ui.tap(Key::Escape, NONE);
    assert!(!ui.app().toolbar_has_focus(), "Escape leaves the toolbar");
    assert!(!focused(&mut ui).is_under(&crate::a11y::tree::toolbar_id()));
}

/// Ctrl+Shift+Tab from the toolbar into a module's Inspector, whose rows
/// are the runtime's since plan 3.2, row 4d.2, focuses one of the column's
/// stops, so the toolbar lets go and the arrows, Enter, and Space reach the
/// column, not a toolbar button (CodeRabbit, #850). Tried with each module
/// of the patch selected.
#[test]
fn ctrl_tab_from_the_toolbar_to_a_module_inspector_focuses_its_column() {
    let modules: Vec<u32> = crate::ui_test::open_app("subtractive.ron")
        .ui_snapshot()
        .tiles
        .iter()
        .map(|tile| tile.id)
        .collect();
    let inspector = crate::inspector::panel::inspector_id();
    let mut tried = 0;
    for module in modules {
        let mut ui = Ui::open("subtractive.ron");
        ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![module])));
        // Back from the canvas to the toolbar, then back to the right column.
        region(&mut ui, true);
        if !ui.app().toolbar_has_focus() {
            continue;
        }
        region(&mut ui, true);
        ui.redraw();
        tried += 1;
        assert!(!ui.app().toolbar_has_focus(), "module {module}: the toolbar let go");
        assert!(
            ui.runtime().focused().is_some_and(|focus| focus.is_under(&inspector)),
            "module {module}: the column has the focus: {:?}",
            ui.runtime().focused()
        );
    }
    assert!(tried > 0, "a module was tried");
}

/// Ctrl+Shift+Tab from the toolbar into an Inspector with a field (no
/// module selected: patch info's Authors) focuses that field, and nothing
/// lets it go after (CodeRabbit, #855).
#[test]
fn ctrl_tab_from_the_toolbar_to_an_inspector_field_focuses_it() {
    let mut ui = Ui::open("subtractive.ron");
    region(&mut ui, true);
    assert!(ui.app().toolbar_has_focus(), "in the toolbar");
    region(&mut ui, true);
    ui.redraw();
    assert_eq!(ui.app().focus, Some(crate::patch_info::authors_part()));
    assert_eq!(focused(&mut ui), crate::patch_info::authors_part());
    assert!(!ui.app().toolbar_has_focus());
}

/// A screen reader's Focus on a toolbar button, while an outline row has
/// the keys, is the tree's focus: the outline lets go, since the tree
/// reports its row before the runtime's focus (CodeRabbit, #850).
#[test]
fn a_reader_focus_on_the_toolbar_takes_it_from_the_outline() {
    let mut ui = Ui::open("subtractive.ron");
    ui.tap(Key::Character('o'), NONE);
    assert!(focused(&mut ui).is_under(&tree_id()), "an outline row");
    let new = crate::a11y::tree::toolbar_id().with("new");
    ui.a11y_action(&new, accesskit::Action::Focus);
    assert_eq!(focused(&mut ui), new);
    assert!(!ui.app().session.outline.active);
}
