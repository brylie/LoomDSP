//! Tests for the input owner and the one input route.

use super::*;
use crate::ui_test::Ui;
use iced::keyboard::Modifiers;
use loom_canvas::stats::{FrameCause, requests_this_thread};
use loom_canvas::{CanvasEvent, PortRef};
use loom_ui_core::{Point, Rect, Size};

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
    let (id, _) = ui.app.session.controller.graph().nodes().next().expect("a module");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![id.0])));
    ui.redraw();
}

/// Opens an overlay as the app does, as far as a headless test can.
pub(crate) fn open_overlay(ui: &mut Ui, owner: InputOwner) {
    let area = Rect::new(Point::ORIGIN, Size::new(1000.0, 800.0));
    match owner {
        InputOwner::Question => {
            ui.app.edit(|app| {
                app.session.controller.add_module("loom.osc.sine").unwrap();
                app.commit();
            });
            ui.send(Message::File(crate::FileAction::New));
        }
        InputOwner::Help => ui.send(Message::ShowHelp),
        InputOwner::Chooser => ui.send(Message::PatchInfo(crate::patch_info::Message::ChooseLicence)),
        InputOwner::Patches => {
            let browser = patches::PatchBrowser::open(1, std::env::temp_dir().join("no-such-folder")).0;
            ui.app.overlays.open(crate::overlays::Overlay::Patches(browser));
        }
        InputOwner::Preferences => ui.send(Message::Preferences(settings::Message::Open)),
        InputOwner::Browser => ui.send(Message::OpenBrowser),
        InputOwner::RecentList => ui.app.overlays.open(crate::overlays::Overlay::RecentList(Vec::new())),
        InputOwner::ProbeMenu => {
            let (from, _) = ui.app.session.controller.graph().cables().next().expect("a cable");
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
            let (id, _) = ui.app.session.controller.graph().nodes().next().expect("a module");
            ui.send(Message::Canvas(CanvasEvent::TileMenu {
                module: id.0,
                at: Point::new(100.0, 100.0),
                area,
                by_key: false,
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
    assert!(!hears(iced::Event::Window(window::Event::FileDropped("a.ron".into()))));
    assert!(!hears(iced::Event::Mouse(mouse::Event::CursorMoved {
        position: iced::Point::ORIGIN
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
        let policy = ui.app.canvas_focus_policy();
        assert_eq!(policy.interactive, owner == InputOwner::Canvas, "{owner:?}");
        assert!(
            !policy.matrix_keyboard && !policy.matrix_escape,
            "{owner:?}: the matrix is not focused"
        );
    }
}

/// The canvas's focus policy follows the matrix: with the matrix focused it
/// leaves the keys to it, and Escape reaches a pinned badge only while no
/// cell editor is open.
#[test]
fn the_canvas_policy_follows_the_matrix() {
    let mut ui = Ui::open("subtractive.ron");
    let idle = ui.app.canvas_focus_policy();
    assert_eq!(
        idle,
        loom_canvas::CanvasFocusPolicy::default(),
        "the canvas has the keys"
    );

    ui.app.right_tab = crate::matrix::Tab::Matrix;
    ui.app.session.matrix.focused = true;
    let matrix = ui.app.canvas_focus_policy();
    assert!(matrix.interactive && matrix.matrix_keyboard && matrix.matrix_escape);
    assert!(!matrix.text_field_keys);
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
    let _ = app.send(Message::Input {
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
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    let _ = app.send(Message::File(crate::FileAction::New));
    assert!(app.overlays.question.is_some());
    let enter = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(Named::Enter),
        modified_key: Key::Named(Named::Enter),
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    });
    let _ = app.send(Message::Input {
        event: enter,
        status: Status::Captured,
        listening: before,
    });
    assert!(
        app.overlays.question.is_some(),
        "the question is still waiting for its answer"
    );
}

/// The panic key and dropped files go through the listener that never restarts.
#[test]
fn panic_and_drops_are_heard_whoever_has_the_input() {
    let hears = |event| always_event(event, Status::Captured, window::Id::unique());
    let panic = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character(".".into()),
        modified_key: Key::Character(".".into()),
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::COMMAND,
        text: None,
        repeat: false,
    });
    assert!(matches!(hears(panic), Some(Message::Panic)));
    assert!(matches!(
        hears(iced::Event::Window(window::Event::FileDropped("a.ron".into()))),
        Some(Message::FileDropped(_))
    ));
    assert!(
        hears(iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))).is_none(),
        "nothing else"
    );
}

/// The hash Iced keeps a subscription under. A new hash restarts it, and an
/// event queued for the old one is lost.
fn identity(subscription: iced::Subscription<Message>) -> u64 {
    use std::hash::Hasher as _;
    let recipes = iced_runtime::futures::subscription::into_recipes(subscription);
    assert_eq!(recipes.len(), 1, "one listener");
    let mut hasher = iced_runtime::futures::subscription::Hasher::default();
    recipes[0].hash(&mut hasher);
    hasher.finish()
}

/// When who is listening changes, the input listener restarts, and the one for
/// the panic key and dropped files does not. So Escape that closes the browser
/// cannot take a Panic pressed straight after it down with it.
#[test]
fn only_the_input_listener_restarts_when_who_is_listening_changes() {
    let mut ui = Ui::open("subtractive.ron");
    let input = identity(ui.app.input_subscription());
    let always = identity(crate::subscriptions::always_subscription());
    open_overlay(&mut ui, InputOwner::Browser);
    assert_ne!(identity(ui.app.input_subscription()), input);
    assert_eq!(identity(crate::subscriptions::always_subscription()), always);
}

/// Help opens on top of Your patches, the chooser, and Preferences
/// (help-pages.md, Help in the app), and has the input there: a click on its
/// backdrop closes it, and one Escape closes it and nothing else.
#[test]
fn help_is_on_top_of_a_dialog_and_closes_alone() {
    let owners = [InputOwner::Patches, InputOwner::Chooser, InputOwner::Preferences];
    for (owner, close) in owners
        .into_iter()
        .flat_map(|owner| [(owner, "escape"), (owner, "click")])
    {
        let mut ui = Ui::open("subtractive.ron");
        open_overlay(&mut ui, owner);
        ui.tap(Key::Named(Named::F1), Modifiers::empty());
        assert_eq!(ui.app.input_owner(), InputOwner::Help, "{owner:?}: F1 opened help");
        if close == "escape" {
            ui.tap(Key::Named(Named::Escape), Modifiers::empty());
        } else {
            ui.move_to(Point::new(5.0, 5.0));
            ui.press();
            ui.release();
        }
        assert!(ui.app.overlays.help.is_none(), "{owner:?}, {close}: help closed");
        assert_eq!(ui.app.input_owner(), owner, "{owner:?}, {close}: the dialog stayed");
    }
}

/// The chooser has two ways in, and F1 works over it from Preferences as well:
/// one Escape closes help, the next the chooser, and the window stays.
#[test]
fn help_opens_over_a_chooser_opened_from_preferences() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Preferences(settings::Message::Open));
    ui.send(Message::Preferences(settings::Message::ChooseLicence));
    ui.redraw();
    assert_eq!(ui.app.input_owner(), InputOwner::Chooser);
    ui.tap(Key::Named(Named::F1), Modifiers::empty());
    assert_eq!(ui.app.input_owner(), InputOwner::Help);
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(ui.app.input_owner(), InputOwner::Chooser, "help closed");
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(ui.app.input_owner(), InputOwner::Preferences, "the chooser closed");
}

/// While help is on top, the patch list behind it hears no keys.
#[test]
fn up_and_down_leave_the_patch_list_alone_behind_help() {
    let mut ui = Ui::open("subtractive.ron");
    open_overlay(&mut ui, InputOwner::Patches);
    ui.tap(Key::Named(Named::F1), Modifiers::empty());
    for named in [Named::ArrowUp, Named::ArrowDown] {
        let messages = ui.tap(Key::Named(named), Modifiers::empty());
        assert!(
            !messages.iter().any(|m| matches!(m, Message::Patches(_))),
            "{named:?}: {messages:?}"
        );
    }
}

/// Taps Cmd with a character, and returns the number of modules after it.
fn tap_command(ui: &mut Ui, c: &str) -> usize {
    ui.tap(Key::Character(c.into()), Modifiers::COMMAND);
    ui.app.session.controller.graph().nodes().count()
}

/// #581: with an overlay open and no text field in focus, Cmd+X and Cmd+D leave
/// the patch alone, and Cmd+C leaves the clipboard alone. With no field in
/// focus the key reaches the app, which is where it used to leak.
#[test]
fn clipboard_keys_do_nothing_behind_an_overlay() {
    let mut leaks = Vec::new();
    for owner in OWNERS.into_iter().filter(|owner| *owner != InputOwner::Canvas) {
        for c in ["x", "d", "c"] {
            let mut ui = Ui::open("subtractive.ron");
            select_first_module(&mut ui);
            open_overlay(&mut ui, owner);
            let modules = ui.app.session.controller.graph().nodes().count();
            let _ = ui.run_task(crate::iced_bridge::focus_moves(
                &loom_ui::effect::FocusTarget::Nothing,
                loom_ui::effect::Selection::Keep,
            ));
            ui.redraw();
            assert!(!ui.text_field_focused(), "{owner:?}: a field would take the key");
            ui.clipboard.0 = Some("before".to_string());
            let after = tap_command(&mut ui, c);
            if after != modules || ui.clipboard.0.as_deref() != Some("before") {
                leaks.push(format!("{owner:?} Cmd+{c}"));
            }
        }
    }
    assert!(leaks.is_empty(), "these keys acted behind the overlay: {leaks:?}");
}

/// #581: a focused search field takes Cmd+X and Cmd+C but not Cmd+D, so Cmd+D
/// is the key that could leak past one.
#[test]
fn duplicate_does_nothing_from_a_focused_search_field() {
    let mut ui = Ui::open("subtractive.ron");
    select_first_module(&mut ui);
    let modules = ui.app.session.controller.graph().nodes().count();
    open_overlay(&mut ui, InputOwner::Patches);
    ui.focus(&patches::text_fields()[0]);
    assert!(ui.text_field_focused());
    assert_eq!(tap_command(&mut ui, "d"), modules);
}

/// #581: with nothing open, the same keys still work.
#[test]
fn clipboard_keys_work_with_no_overlay() {
    let mut ui = Ui::open("subtractive.ron");
    select_first_module(&mut ui);
    let modules = ui.app.session.controller.graph().nodes().count();
    assert_eq!(tap_command(&mut ui, "d"), modules + 1, "Cmd+D duplicates");
    ui.clipboard.0 = None;
    ui.tap(Key::Character("c".into()), Modifiers::COMMAND);
    assert!(ui.clipboard.0.is_some(), "Cmd+C copies");
    let before = ui.app.session.controller.graph().nodes().count();
    assert!(tap_command(&mut ui, "x") < before, "Cmd+X cuts");
}

/// The real window's path: events go into the app's subscription as the runtime
/// sends them, and the messages that come out route as the other paths do. A
/// redraw sends nothing, so an idle app stays idle.
#[test]
fn the_subscription_sends_what_it_hears_with_who_was_listening() {
    use iced_runtime::futures::futures::{StreamExt, executor::block_on, stream};
    use iced_runtime::futures::subscription::{Event as Runtime, into_recipes};
    let ui = Ui::open("subtractive.ron");
    let window = window::Id::unique();
    let interaction = |event, status| Runtime::Interaction { window, event, status };
    let escape = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(Named::Escape),
        modified_key: Key::Named(Named::Escape),
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    });
    let events = vec![
        interaction(
            iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
            Status::Ignored,
        ),
        interaction(
            iced::Event::Mouse(mouse::Event::CursorMoved {
                position: iced::Point::ORIGIN,
            }),
            Status::Ignored,
        ),
        interaction(escape, Status::Ignored),
    ];
    let mut recipes = into_recipes(ui.app.input_subscription());
    assert_eq!(recipes.len(), 1, "one recipe for the input listener");
    let recipe = recipes.remove(0);
    let messages: Vec<Message> = block_on(recipe.stream(stream::iter(events).boxed()).collect());
    let [
        Message::Input {
            event,
            status,
            listening,
        },
    ] = messages.as_slice()
    else {
        panic!("only the key comes out: {messages:?}");
    };
    assert_eq!(*listening, ui.app.listening());
    let routed = listening.route(event.clone(), *status);
    assert!(
        matches!(
            routed.as_slice(),
            [Message::Key(_), Message::CloseHelp, Message::TextFocusCheck]
        ),
        "{routed:?}"
    );
}

/// For each overlay, a key sent the way the `Injector` and the headless `Ui`
/// send it makes the same message as the real window's subscription.
#[test]
fn the_injector_and_the_subscription_agree_for_each_overlay() {
    use iced_runtime::futures::futures::{StreamExt, executor::block_on, stream};
    use iced_runtime::futures::subscription::{Event as Runtime, into_recipes};
    let escape = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(Named::Escape),
        modified_key: Key::Named(Named::Escape),
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    });
    for owner in OWNERS {
        let mut ui = Ui::open("subtractive.ron");
        open_overlay(&mut ui, owner);

        let injected = heard_messages(&escape, Status::Ignored, ui.app.listening());

        let runtime = Runtime::Interaction {
            window: window::Id::unique(),
            event: escape.clone(),
            status: Status::Ignored,
        };
        let recipe = into_recipes(ui.app.input_subscription()).remove(0);
        let heard: Vec<Message> = block_on(recipe.stream(stream::iter(vec![runtime]).boxed()).collect());
        assert_eq!(format!("{injected:?}"), format!("{heard:?}"), "{owner:?}");
    }
}
