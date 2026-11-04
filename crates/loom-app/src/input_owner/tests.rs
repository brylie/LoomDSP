//! Tests for the input owner and the one input route.

use super::*;
use crate::ui_test::Ui;
use iced::keyboard::Modifiers;
use iced::{Point, Rectangle, Size};
use loom_canvas::stats::{FrameCause, requests_this_thread};
use loom_canvas::{CanvasEvent, PortRef};

/// Every overlay, and the canvas with none.
pub(crate) const OWNERS: [InputOwner; 10] = [
    InputOwner::Question,
    InputOwner::Help,
    InputOwner::Chooser,
    InputOwner::Patches,
    InputOwner::Preferences,
    InputOwner::Browser,
    InputOwner::RecentList,
    InputOwner::ProbeMenu,
    InputOwner::TileMenu,
    InputOwner::Canvas,
];

/// Selects the first module, which help and the clipboard keys need.
pub(crate) fn select_first_module(ui: &mut Ui) {
    let (id, _) = ui.app.controller.graph().nodes().next().expect("a module");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![id.0])));
    ui.redraw();
}

/// Opens an overlay as the app does, as far as a headless test can.
pub(crate) fn open_overlay(ui: &mut Ui, owner: InputOwner) {
    let area = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 800.0));
    match owner {
        InputOwner::Question => {
            ui.app.edit(|app| {
                app.controller.add_module("loom.osc.sine").unwrap();
                app.commit();
            });
            ui.send(Message::File(crate::FileAction::New));
        }
        InputOwner::Help => ui.send(Message::ShowHelp),
        InputOwner::Chooser => ui.send(Message::PatchInfo(crate::patch_info::Message::ChooseLicence)),
        InputOwner::Patches => {
            ui.app.patches = Some(patches::PatchBrowser::open(1, std::env::temp_dir().join("no-such-folder")).0);
        }
        InputOwner::Preferences => ui.send(Message::Preferences(settings::Message::Open)),
        InputOwner::Browser => ui.send(Message::OpenBrowser),
        InputOwner::RecentList => ui.app.recent_rows = Some(Vec::new()),
        InputOwner::ProbeMenu => {
            let (from, _) = ui.app.controller.graph().cables().next().expect("a cable");
            let output = PortRef {
                module: from.module.0,
                output: true,
                index: from.port,
            };
            ui.send(Message::Canvas(CanvasEvent::ProbeMenu {
                output,
                world: Point::new(100.0, 100.0),
                at: Point::new(100.0, 100.0),
                area,
            }));
        }
        InputOwner::TileMenu => {
            let (id, _) = ui.app.controller.graph().nodes().next().expect("a module");
            ui.send(Message::Canvas(CanvasEvent::TileMenu {
                module: id.0,
                at: Point::new(100.0, 100.0),
                area,
            }));
        }
        InputOwner::Canvas => {}
    }
    ui.redraw();
    assert_eq!(ui.app.input_owner(), owner, "the overlay opened");
}

/// The listener passes what a row reads, and nothing else, so that moving the
/// pointer and redrawing send no message and an idle app stays idle.
#[test]
fn the_listener_passes_only_what_a_row_can_use() {
    let hears = |event| input_event(event, Status::Ignored, window::Id::unique()).is_some();
    assert!(hears(iced::Event::Keyboard(keyboard::Event::ModifiersChanged(
        Modifiers::SHIFT
    ))));
    assert!(hears(iced::Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Left
    ))));
    assert!(hears(iced::Event::Window(window::Event::FileDropped("a.ron".into()))));
    assert!(!hears(iced::Event::Mouse(mouse::Event::CursorMoved {
        position: Point::ORIGIN
    })));
    assert!(!hears(iced::Event::Mouse(mouse::Event::ButtonReleased(
        mouse::Button::Left
    ))));
    assert!(!hears(iced::Event::Window(window::Event::RedrawRequested(
        std::time::Instant::now()
    ))));
}

/// Each overlay is its own owner, and only the canvas with none is interactive.
#[test]
fn each_overlay_has_the_input_while_it_is_open() {
    for owner in OWNERS {
        let mut ui = Ui::open("subtractive.ron");
        select_first_module(&mut ui);
        open_overlay(&mut ui, owner);
        assert_eq!(
            ui.app.no_overlay(),
            matches!(owner, InputOwner::Canvas | InputOwner::ProbeMenu | InputOwner::TileMenu),
            "{owner:?}"
        );
    }
}

/// A plain key press is one input cause: the message for the event is not
/// counted as well as the `Key` it routes to.
#[test]
fn a_key_press_counts_one_input_cause() {
    let mut ui = Ui::open("subtractive.ron");
    let before = requests_this_thread(FrameCause::Input);
    ui.step(&[iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character("q".into()),
        modified_key: Key::Character("q".into()),
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })]);
    assert_eq!(requests_this_thread(FrameCause::Input) - before, 1);
}

/// A route with no message still redraws, so it still counts as input. A key a
/// widget used, and that no row reads, is such a route.
#[test]
fn an_empty_route_counts_as_input() {
    let mut app = crate::ui_test::Ui::open("subtractive.ron").app;
    let before = requests_this_thread(FrameCause::Input);
    let released = keyboard::Event::KeyReleased {
        key: Key::Character("q".into()),
        modified_key: Key::Character("q".into()),
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
    };
    let listening = app.listening();
    let event = iced::Event::Keyboard(released);
    assert!(listening.route(event.clone(), Status::Captured).is_empty());
    let _ = app.update(Message::Input {
        event,
        status: Status::Captured,
        listening,
    });
    assert_eq!(requests_this_thread(FrameCause::Input) - before, 1);
}

/// Enter that makes a widget open a question must not also answer it: the
/// route uses who was listening before the event. Before this was so, opening
/// a patch with unsaved changes from the search field discarded them at once.
#[test]
fn a_route_reads_who_was_listening_before_the_event() {
    let mut app = crate::ui_test::Ui::open("subtractive.ron").app;
    let before = app.listening();
    app.edit(|app| {
        app.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    let _ = app.update(Message::File(crate::FileAction::New));
    assert!(app.question.is_some());
    let enter = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(Named::Enter),
        modified_key: Key::Named(Named::Enter),
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    });
    let _ = app.update(Message::Input {
        event: enter,
        status: Status::Captured,
        listening: before,
    });
    assert!(app.question.is_some(), "the question is still waiting for its answer");
}

/// With `Key` first, one Escape closes help and leaves the window under it. The
/// toolbar can open Preferences while help is open.
#[test]
fn one_escape_closes_help_over_preferences() {
    let mut ui = Ui::open("subtractive.ron");
    select_first_module(&mut ui);
    open_overlay(&mut ui, InputOwner::Help);
    ui.send(Message::Preferences(settings::Message::Open));
    ui.redraw();
    assert!(ui.app.help.is_some() && ui.app.rig.window.is_some());
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert!(ui.app.help.is_none(), "help closed");
    assert!(ui.app.rig.window.is_some(), "Preferences stayed open");
}
