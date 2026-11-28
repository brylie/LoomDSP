//! Tests for the input owner, the overlays' keys, and the two key hooks.

use super::*;
use crate::Message;
use crate::patches;
use crate::ui_test::Ui;
use loom_canvas::stats::{FrameCause, requests_this_thread};
use loom_canvas::{CanvasEvent, PortRef};
use loom_ui_core::Key;
use loom_ui_core::{Modifiers, Point, Rect, Size};

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
    let (id, _) = ui.app().session.controller.graph().nodes().next().expect("a module");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![id.0])));
    ui.redraw();
}

/// Opens an overlay as the app does, as far as a headless test can.
pub(crate) fn open_overlay(ui: &mut Ui, owner: InputOwner) {
    let area = Rect::new(Point::ORIGIN, Size::new(1000.0, 800.0));
    match owner {
        InputOwner::Question => {
            ui.app_mut().edit(|app| {
                app.session.controller.add_module("loom.osc.sine").unwrap();
                app.commit();
            });
            ui.send(Message::File(crate::FileAction::New));
        }
        InputOwner::Help => ui.send(Message::ShowHelp),
        InputOwner::Chooser => ui.send(Message::PatchInfo(crate::patch_info::Message::ChooseLicence)),
        InputOwner::Patches => {
            // A folder that does not exist, inside one that is removed after.
            let parent = crate::recovery::tests::TempDir::new("input-owner");
            let browser = patches::PatchBrowser::open(1, parent.join("no-such-folder")).0;
            ui.app_mut().overlays.open(crate::overlays::Overlay::Patches(browser));
        }
        InputOwner::Preferences => ui.send(Message::Preferences(settings::Message::Open)),
        InputOwner::Browser => ui.send(Message::OpenBrowser),
        InputOwner::RecentList => ui
            .app_mut()
            .overlays
            .open(crate::overlays::Overlay::RecentList(Vec::new())),
        InputOwner::ProbeMenu => {
            let (from, _) = ui.app().session.controller.graph().cables().next().expect("a cable");
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
            let (id, _) = ui.app().session.controller.graph().nodes().next().expect("a module");
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
    assert_eq!(ui.app().input_owner(), owner, "the overlay opened");
}

/// Each overlay is its own owner, and only the canvas with none is interactive.
#[test]
fn each_overlay_has_the_input_while_it_is_open() {
    for owner in OWNERS {
        let mut ui = Ui::open("subtractive.ron");
        select_first_module(&mut ui);
        open_overlay(&mut ui, owner);
        assert_eq!(ui.app().no_overlay(), owner == InputOwner::Canvas, "{owner:?}");
        let policy = ui.app().canvas_focus_policy();
        assert_eq!(policy.interactive, owner == InputOwner::Canvas, "{owner:?}");
        assert!(!policy.matrix_keyboard, "{owner:?}: the matrix is not focused");
    }
}

/// The canvas's focus policy follows the matrix: with a cell of it focused,
/// the runtime's focus, the canvas leaves the keys to it.
#[test]
fn the_canvas_policy_follows_the_matrix() {
    let ui = Ui::open("subtractive.ron");
    let idle = ui.app().canvas_focus_policy();
    assert_eq!(
        idle,
        loom_canvas::CanvasFocusPolicy::default(),
        "the canvas has the keys"
    );

    let mut ui = Ui::open("modulation.ron");
    ui.send(Message::Matrix(crate::matrix::Event::Tab(crate::matrix::Tab::Matrix)));
    let _ = ui.redraw();
    assert!(ui.app().focus_in_matrix());
    let matrix = ui.app().canvas_focus_policy();
    assert!(
        matrix.interactive && matrix.text_field_keys,
        "the keys are the matrix's"
    );
    assert!(matrix.matrix_keyboard, "a press on the canvas takes them back");
}

/// A plain key press is one input cause: the message for the event is not
/// counted as well as the `Key` it routes to.
#[test]
fn a_key_press_counts_one_input_cause() {
    let mut ui = Ui::open("subtractive.ron");
    let before = requests_this_thread(FrameCause::Input);
    ui.step(&[crate::ui_test::key_press(
        Key::Character('q'),
        Modifiers::NONE,
        None,
        false,
    )]);
    assert_eq!(requests_this_thread(FrameCause::Input) - before, 1);
}

/// A key that opens a question must not also answer it: the runtime routes
/// the key, and asks the app's shortcuts for it, before any of its own
/// updates run, so the question it opens hears nothing of it (ui-layer.md,
/// section 6, rule 6). Before this was so, opening a patch with unsaved
/// changes from the search field discarded them at once.
#[test]
fn a_key_that_opens_a_question_does_not_answer_it() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    // Cmd+N asks about the unsaved changes; its release reaches no answer.
    ui.tap(Key::Character('n'), Modifiers::platform_shortcut());
    assert!(
        ui.app().overlays.question.is_some(),
        "the question is still waiting for its answer"
    );
}

/// The panic key is the app's always-heard key (section 6, rule 6, step 1),
/// whoever has the input; nothing else is.
#[test]
fn panic_is_heard_whoever_has_the_input() {
    let app = crate::tests::app();
    let panic = crate::ui_test::pressed(Key::Character('.'), Modifiers::platform_shortcut());
    assert!(matches!(loom_ui::App::always_key(&app, &panic), Some(Message::Panic)));
    let other = crate::ui_test::pressed(Key::Character('a'), Modifiers::NONE);
    assert!(loom_ui::App::always_key(&app, &other).is_none(), "nothing else");
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
        ui.tap(Key::F1, Modifiers::NONE);
        assert_eq!(ui.app().input_owner(), InputOwner::Help, "{owner:?}: F1 opened help");
        if close == "escape" {
            ui.tap(Key::Escape, Modifiers::NONE);
        } else {
            ui.move_to(Point::new(5.0, 5.0));
            ui.press();
            ui.release();
        }
        assert!(ui.app().overlays.help.is_none(), "{owner:?}, {close}: help closed");
        assert_eq!(ui.app().input_owner(), owner, "{owner:?}, {close}: the dialog stayed");
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
    assert_eq!(ui.app().input_owner(), InputOwner::Chooser);
    ui.tap(Key::F1, Modifiers::NONE);
    assert_eq!(ui.app().input_owner(), InputOwner::Help);
    ui.tap(Key::Escape, Modifiers::NONE);
    assert_eq!(ui.app().input_owner(), InputOwner::Chooser, "help closed");
    ui.tap(Key::Escape, Modifiers::NONE);
    assert_eq!(ui.app().input_owner(), InputOwner::Preferences, "the chooser closed");
}

/// Help takes the search field's input and restores it when it closes.
#[test]
fn help_suspends_search_in_both_browsers() {
    for (owner, field) in [
        (InputOwner::Browser, crate::browser::search_id()),
        (InputOwner::Patches, patches::text_fields()[0].clone()),
    ] {
        let mut ui = Ui::open("subtractive.ron");
        open_overlay(&mut ui, owner);
        let at = ui.field(&field);
        ui.move_to(at);
        ui.press();
        ui.release();
        assert_eq!(ui.app().focus.as_ref(), Some(&field));
        let search = |ui: &Ui| match owner {
            InputOwner::Browser => ui.app().overlays.browser().unwrap().model().query.clone(),
            _ => ui.app().overlays.patches().unwrap().model().search.clone(),
        };
        ui.typewrite("a");
        assert_eq!(search(&ui), "a");
        ui.tap(Key::F1, Modifiers::NONE);
        ui.typewrite("b");
        assert_eq!(search(&ui), "a", "{owner:?}");
        ui.tap(Key::Escape, Modifiers::NONE);
        assert_eq!(ui.focused_field(), Some(field));
        ui.typewrite("c");
        assert_eq!(search(&ui), "ac", "{owner:?}");
    }
}

/// While help is on top, the patch list behind it hears no keys.
#[test]
fn up_and_down_leave_the_patch_list_alone_behind_help() {
    use loom_ui_core::{Key, Modifiers};
    let mut ui = Ui::open("subtractive.ron");
    open_overlay(&mut ui, InputOwner::Patches);
    ui.tap(Key::F1, Modifiers::NONE);
    for named in [Key::Up, Key::Down] {
        let messages = ui.tap(named, Modifiers::NONE);
        assert!(
            !messages.iter().any(|m| matches!(m, Message::Patches(_))),
            "{named:?}: {messages:?}"
        );
    }
}

/// Taps Cmd with a character, and returns the number of modules after it.
fn tap_command(ui: &mut Ui, c: char) -> usize {
    use loom_ui_core::{Key, Modifiers};
    ui.tap(Key::Character(c), Modifiers::platform_shortcut());
    ui.app().session.controller.graph().nodes().count()
}

/// #581: with an overlay open and no text field in focus, Cmd+X and Cmd+D leave
/// the patch alone, and Cmd+C leaves the clipboard alone. With no field in
/// focus the key reaches the app, which is where it used to leak.
#[test]
fn clipboard_keys_do_nothing_behind_an_overlay() {
    let mut leaks = Vec::new();
    for owner in OWNERS.into_iter().filter(|owner| *owner != InputOwner::Canvas) {
        for c in ['x', 'd', 'c'] {
            let mut ui = Ui::open("subtractive.ron");
            select_first_module(&mut ui);
            open_overlay(&mut ui, owner);
            let modules = ui.app().session.controller.graph().nodes().count();
            let _ = ui.unfocus_all();
            ui.redraw();
            // The module browser's search field is its dialog's only stop
            // and keeps the focus (module-browser.md, Focus and Tab), so
            // there the key meets the field, which must not pass it on.
            if owner != InputOwner::Browser {
                assert!(!ui.text_field_focused(), "{owner:?}: a field would take the key");
            }
            ui.set_clipboard(Some("before".to_string()));
            let after = tap_command(&mut ui, c);
            if after != modules || ui.clipboard() != Some("before") {
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
    let modules = ui.app().session.controller.graph().nodes().count();
    open_overlay(&mut ui, InputOwner::Patches);
    ui.focus(&patches::text_fields()[0]);
    assert!(ui.text_field_focused());
    assert_eq!(tap_command(&mut ui, 'd'), modules);
}

/// #581: with nothing open, the same keys still work.
#[test]
fn clipboard_keys_work_with_no_overlay() {
    let mut ui = Ui::open("subtractive.ron");
    select_first_module(&mut ui);
    let modules = ui.app().session.controller.graph().nodes().count();
    assert_eq!(tap_command(&mut ui, 'd'), modules + 1, "Cmd+D duplicates");
    ui.set_clipboard(None);
    ui.tap(Key::Character('c'), Modifiers::platform_shortcut());
    assert!(ui.clipboard().is_some(), "Cmd+C copies");
    let before = ui.app().session.controller.graph().nodes().count();
    assert!(tap_command(&mut ui, 'x') < before, "Cmd+X cuts");
}
