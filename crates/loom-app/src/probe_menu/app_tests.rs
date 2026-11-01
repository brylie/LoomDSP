use super::*;
use crate::{App, Args, Message, tests};
use loom_canvas::CanvasEvent;
use loom_core::{InputRef, ModuleId, OutputRef};
use std::path::PathBuf;

/// The subtractive example patch, with no audio.
fn app() -> App {
    let patch = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/patches/subtractive.ron"
    ));
    App::new(Args {
        patch: Some(patch),
        ..tests::app_args()
    })
}

fn area() -> Rectangle {
    Rectangle::new(Point::ORIGIN, Size::new(1000.0, 800.0))
}

/// The first cable of the patch, as the canvas names its source.
fn first_cable(app: &App) -> (PortRef, InputRef) {
    let (from, to) = app.controller.graph().cables().next().expect("the patch has cables");
    let output = PortRef {
        module: from.module.0,
        output: true,
        index: from.port,
    };
    (output, to)
}

/// Opens the menu as a right-click on `output` would, at `world`.
fn open(app: &mut App, output: PortRef, world: Point) {
    let _ = app.update(Message::Canvas(CanvasEvent::ProbeMenu {
        output,
        world,
        at: Point::new(100.0, 100.0),
        area: area(),
    }));
    assert!(app.probe_menu.is_some());
}

/// The new module's cable: from the probed output into its `in`.
fn feeds(app: &App, module: u32, output: PortRef) -> bool {
    let source = OutputRef {
        module: ModuleId(output.module),
        port: output.index,
    };
    let input = InputRef {
        module: ModuleId(module),
        port: 0,
    };
    app.controller
        .graph()
        .cables()
        .any(|(from, to)| from == source && to == input)
}

/// Add scope and Add meter, on an output and on a cable, add a
/// connected module beside it, keep the cable, select the new module,
/// and overlap no tile. Undo takes the module and its cable back in one
/// step (panels.md, acceptance criteria: scope module).
#[test]
fn add_scope_and_meter_on_an_output_and_a_cable() {
    for probe in [Probe::Scope, Probe::Meter] {
        for on_cable in [false, true] {
            let mut app = app();
            let (output, to) = first_cable(&app);
            let port = app.scene.port_position(output).unwrap();
            let world = if on_cable {
                port + iced::Vector::new(20.0, 5.0)
            } else {
                port
            };
            let modules = app.controller.graph().nodes().count();
            let cables = app.controller.graph().cables().count();
            let tiles: Vec<Rectangle> = app.scene.modules().iter().map(|m| m.bounds()).collect();

            open(&mut app, output, world);
            let _ = app.update(Message::ProbeMenu(super::Message::Pick(probe)));
            assert!(app.probe_menu.is_none());

            let graph = app.controller.graph();
            assert_eq!(graph.nodes().count(), modules + 1);
            assert_eq!(graph.cables().count(), cables + 1);
            let id = *app.selection.iter().next().expect("the new module is selected");
            assert_eq!(app.selection.len(), 1);
            assert_eq!(graph.node(ModuleId(id)).unwrap().descriptor.id, probe.id());
            assert!(feeds(&app, id, output), "{probe:?} is fed by the output");
            let source = OutputRef {
                module: ModuleId(output.module),
                port: output.index,
            };
            assert!(graph.cables().any(|c| c == (source, to)), "the clicked cable stays");
            let tile = app.scene.module(id).unwrap().bounds();
            assert!(tile.x > world.x, "the new tile is right of where the menu opened");
            assert!(!tiles.iter().any(|t| t.intersects(&tile)), "{probe:?} overlaps a tile");
            assert_eq!(
                app.edits.history.undo_step().map(|s| s.name().to_string()).as_deref(),
                Some(probe.verb())
            );

            app.undo();
            assert_eq!(app.controller.graph().nodes().count(), modules);
            assert_eq!(app.controller.graph().cables().count(), cables);
        }
    }
}

/// Escape and a click outside close the menu and add nothing. While it
/// is open, other keys do not reach the canvas or the shortcuts.
#[test]
fn escape_and_a_click_outside_close_the_menu() {
    let mut app = app();
    let (output, _) = first_cable(&app);
    let port = app.scene.port_position(output).unwrap();
    let modules = app.controller.graph().nodes().count();
    let undo = app.edits.history.undo_step().map(|s| s.name().to_string());

    open(&mut app, output, port);
    let press = |key| {
        Message::Key(iced::keyboard::Event::KeyPressed {
            key,
            modified_key: iced::keyboard::Key::Unidentified,
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })
    };
    let _ = app.update(press(iced::keyboard::Key::Character("a".into())));
    assert!(
        app.probe_menu.is_some() && app.browser.is_none(),
        "A does not open the browser"
    );
    let _ = app.update(press(iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)));
    assert!(app.probe_menu.is_none());

    open(&mut app, output, port);
    let _ = app.update(Message::ProbeMenu(super::Message::Close));
    assert!(app.probe_menu.is_none());
    assert_eq!(app.controller.graph().nodes().count(), modules);
    assert_eq!(app.edits.history.undo_step().map(|s| s.name().to_string()), undo);
}

/// Cut and duplicate do not change the patch behind the open menu.
#[test]
fn clipboard_keys_do_nothing_while_the_menu_is_open() {
    let mut app = app();
    let (output, _) = first_cable(&app);
    let port = app.scene.port_position(output).unwrap();
    let _ = app.update(Message::Canvas(CanvasEvent::Select(vec![output.module])));
    let modules = app.controller.graph().nodes().count();
    open(&mut app, output, port);
    let _ = app.update(tests::command_key("d"));
    let _ = app.update(tests::command_key("x"));
    assert_eq!(app.controller.graph().nodes().count(), modules);
    assert!(app.probe_menu.is_some());
}

/// If the output's module was removed while the menu was open, picking
/// an item adds nothing, makes no step, and says why.
#[test]
fn a_removed_output_adds_nothing() {
    let mut app = app();
    let (output, _) = first_cable(&app);
    let port = app.scene.port_position(output).unwrap();
    open(&mut app, output, port);
    let _ = app.update(Message::Canvas(CanvasEvent::Delete(vec![output.module])));
    let modules = app.controller.graph().nodes().count();
    let undo = app.edits.history.undo_step().map(|s| s.name().to_string());
    let _ = app.update(Message::ProbeMenu(super::Message::Pick(Probe::Scope)));
    assert_eq!(app.controller.graph().nodes().count(), modules);
    assert_eq!(app.edits.history.undo_step().map(|s| s.name().to_string()), undo);
    assert!(
        app.alerts.status().contains("the output is gone"),
        "{}",
        app.alerts.status()
    );
}

/// The menu stays inside the canvas when opened near its edge.
#[test]
fn the_menu_stays_inside_the_canvas() {
    let output = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let menu = ProbeMenu::open(output, Point::ORIGIN, Point::new(990.0, 790.0), area());
    assert!(
        menu.at.x + WIDTH <= 1000.0 && menu.at.y + HEIGHT <= 800.0,
        "{:?}",
        menu.at
    );
}
