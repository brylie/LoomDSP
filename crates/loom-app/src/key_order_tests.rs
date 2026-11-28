//! The key order of ui-layer.md, section 6, rule 6, step by step, through
//! the `Ui` harness and so through the same runtime and bridge as the
//! window. One test per step. Phase 4 changes who delivers the keys (plan
//! steps P2 and P4), and these tests must pass unchanged before and after:
//! a change to what they assert is a change to the order.
//!
//! Step 4 is retired (rule 6, "The order once Iced's listeners go"): no
//! view that Iced draws navigates by key, so it has no test.

use crate::ui_test::{Ui, key_press};
use crate::{FileAction, Message};
use loom_ui_core::{Key, Modifiers};

/// Cmd+. (Ctrl+. off a Mac), the panic key (panic.md).
fn tap_panic(ui: &mut Ui) -> Vec<Message> {
    ui.tap(Key::Character('.'), Modifiers::platform_shortcut())
}

/// Asserts that Panic ran: the message was sent and the status line says so.
fn assert_panicked(ui: &Ui, messages: &[Message]) {
    assert!(messages.iter().any(|m| matches!(m, Message::Panic)), "{messages:?}");
    assert!(
        ui.app().alerts.status().starts_with("Panic"),
        "{}",
        ui.app().alerts.status()
    );
}

/// How many modules the open patch has.
fn modules(ui: &Ui) -> usize {
    ui.app().session.controller.graph().nodes().count()
}

/// The example patch with one module more and the Close question open:
/// "Save your changes?" holds the window.
fn asking() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    ui.send(Message::File(FileAction::Close));
    ui.redraw();
    assert!(ui.app().overlays.question.is_some(), "the question is open");
    ui
}

/// The module browser open, with the focus on its search field.
fn browsing() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::OpenBrowser);
    ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&crate::browser::search_id()));
    ui
}

/// Step 1: Panic is heard whoever has the input. Under a focused text
/// field, the field does not take it as a typed `.`.
#[test]
fn step_1_panic_is_heard_under_a_focused_field() {
    let mut ui = browsing();
    ui.typewrite("sin");
    let messages = tap_panic(&mut ui);
    assert_panicked(&ui, &messages);
    let browser = ui.app().overlays.browser().expect("the browser stays open");
    assert_eq!(browser.model().query, "sin", "the field typed nothing");
    assert_eq!(ui.runtime().focused(), Some(&crate::browser::search_id()));
}

/// Step 1: Panic is heard under an open question, which stays open.
#[test]
fn step_1_panic_is_heard_under_a_question() {
    let mut ui = asking();
    let messages = tap_panic(&mut ui);
    assert_panicked(&ui, &messages);
    assert!(ui.app().overlays.question.is_some(), "the question stays open");
}

/// Step 1: Panic is heard during a held drag, and the drag goes on: the
/// release still drops the cable where it lands.
#[test]
fn step_1_panic_is_heard_during_a_drag() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Inspector(crate::inspector::Message::SelectModule(4)));
    ui.redraw();
    let row = ui.text("Cutoff").expect("the inspector's Cutoff row").center();
    let from = ui.output(7, "out");
    let before = ui.app().session.controller.graph().assignments().count();
    ui.move_to(from);
    ui.press();
    ui.move_to(row);
    assert!(ui.app().session.gestures.assign_drag().is_some(), "the drag is held");
    let messages = tap_panic(&mut ui);
    assert_panicked(&ui, &messages);
    assert!(
        ui.app().session.gestures.assign_drag().is_some(),
        "the drag is still held"
    );
    ui.release();
    assert_eq!(
        ui.app().session.controller.graph().assignments().count(),
        before + 1,
        "the drop after Panic assigns"
    );
}

/// Step 2: an open question has every other key. Delete, a shortcut, and
/// a letter reach nothing behind it; Escape gives the safe answer.
#[test]
fn step_2_a_question_has_every_other_key() {
    let mut ui = asking();
    ui.app_mut().session.selection = [4].into_iter().collect();
    let before = modules(&ui);
    let none = Modifiers::NONE;
    ui.tap(Key::Delete, none);
    ui.tap(Key::Character('d'), Modifiers::platform_shortcut());
    ui.step(&[key_press(Key::Character('a'), none, Some("a"), false)]);
    ui.step(&[crate::ui_test::key_released(Key::Character('a'), none)]);
    assert_eq!(modules(&ui), before, "no key reached the canvas or a shortcut");
    assert!(ui.app().overlays.question.is_some(), "the question stays open");
    ui.tap(Key::Escape, none);
    assert!(ui.app().overlays.question.is_none(), "Escape closed it");
    assert!(!ui.app().quit, "with the safe answer: the window stays");
}

/// Step 2: Escape under a question, with Space held on its safe answer,
/// cancels the held press and closes the question at once; the release
/// then clicks nothing (#926).
#[test]
fn step_2_escape_under_a_question_cancels_a_held_space_and_closes_it() {
    let mut ui = asking();
    let none = Modifiers::NONE;
    ui.key_down(Key::Space, false);
    assert!(ui.app().overlays.question.is_some(), "Space clicks on key up");
    ui.tap(Key::Escape, none);
    assert!(ui.app().overlays.question.is_none(), "one Escape closed it");
    let released = ui.key_up(Key::Space);
    assert!(
        !released.iter().any(|m| matches!(m, Message::Question(_))),
        "the release clicks nothing: {released:?}"
    );
    assert!(!ui.app().quit, "with the safe answer: the window stays");
}

/// Step 2b: an element around the focus takes its keys first. The module
/// browser hears Up and Down before its search field, so they move the
/// highlight and never the caret.
#[test]
fn step_2b_the_browser_takes_up_and_down_before_its_field() {
    let mut ui = browsing();
    let none = Modifiers::NONE;
    ui.tap(Key::Down, none);
    ui.tap(Key::Down, none);
    let browser = ui.app().overlays.browser().expect("the browser is open");
    assert_eq!(browser.model().highlight, Some(2));
    assert_eq!(ui.runtime().focused(), Some(&crate::browser::search_id()));
}

/// Step 3: the focused element takes the keys it uses, and they go no
/// further. Cmd+Z in a text field undoes the typing, not the patch.
#[test]
fn step_3_a_focused_field_keeps_its_undo() {
    let mut ui = browsing();
    let step = ui.app().session.edits.history.undo_step().map(|s| s.name().to_owned());
    ui.typewrite("sin");
    ui.tap(Key::Character('z'), Modifiers::platform_shortcut());
    let browser = ui.app().overlays.browser().expect("the browser is open");
    assert_ne!(browser.model().query, "sin", "the field undid its typing");
    assert_eq!(
        ui.app().session.edits.history.undo_step().map(|s| s.name().to_owned()),
        step,
        "the patch's undo history is untouched"
    );
}

/// Step 5: the open overlay has the keys before the canvas and the
/// shortcuts. Over Preferences, Cmd+D duplicates nothing; Escape closes it.
#[test]
fn step_5_the_open_overlay_has_its_keys() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().session.selection = [4].into_iter().collect();
    ui.tap(Key::Character(','), Modifiers::platform_shortcut());
    assert!(ui.app().overlays.preferences().is_some(), "Cmd+, opened Preferences");
    let before = modules(&ui);
    ui.tap(Key::Character('d'), Modifiers::platform_shortcut());
    assert_eq!(modules(&ui), before, "the selection behind it was not duplicated");
    ui.tap(Key::Escape, Modifiers::NONE);
    assert!(ui.app().overlays.preferences().is_none(), "Escape closed it");
}

/// Step 5: with nothing open and no field focused, the canvas has the
/// keys: Space toggles Run.
#[test]
fn step_5_the_canvas_has_the_keys_when_nothing_else_does() {
    let mut ui = Ui::open("subtractive.ron");
    ui.redraw();
    let messages = ui.step(&[key_press(Key::Space, Modifiers::NONE, Some(" "), false)]);
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Canvas(loom_canvas::CanvasEvent::ToggleRun))),
        "{messages:?}"
    );
}

/// Step 6: a key nothing took reaches the app's shortcuts. Cmd+D
/// duplicates the selection; while a field has focus, the same key does
/// nothing to the patch, even though the field leaves it.
#[test]
fn step_6_a_shortcut_acts_unless_a_field_has_focus() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().session.selection = [4].into_iter().collect();
    ui.redraw();
    let field = ui.value_field(4, 0);
    ui.move_to(field);
    ui.press();
    ui.release();
    assert!(ui.text_field_focused(), "the value field has focus");
    let before = modules(&ui);
    ui.tap(Key::Character('d'), Modifiers::platform_shortcut());
    assert_eq!(modules(&ui), before, "held back while a field has focus");
    ui.unfocus_all();
    ui.app_mut().session.selection = [4].into_iter().collect();
    ui.redraw();
    ui.tap(Key::Character('d'), Modifiers::platform_shortcut());
    assert_eq!(modules(&ui), before + 1, "with no field, Cmd+D duplicates");
}

/// Step 6: a shortcut runs in its key's own turn, not one after: the
/// runtime runs one turn for the press, and Cmd+D has duplicated the tile
/// by its end (Agreed, rule 6, step 6).
#[test]
fn step_6_a_shortcut_runs_in_its_keys_own_turn() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().session.selection = [4].into_iter().collect();
    ui.redraw();
    let before = modules(&ui);
    let turns = ui.runtime().counts().turns;
    ui.step(&[key_press(
        Key::Character('d'),
        Modifiers::platform_shortcut(),
        Some("d"),
        false,
    )]);
    assert_eq!(modules(&ui), before + 1, "Cmd+D duplicated the tile");
    assert_eq!(ui.runtime().counts().turns, turns + 1, "in one turn");
}

/// Step 1: Panic is heard while an input method composes in a field,
/// though the composition takes every other key, Escape included; the
/// composition goes on, and its commit still types the text (section 6,
/// rule 6, step 1; section 8, rule 4).
#[test]
fn step_1_panic_is_heard_while_composing() {
    use iced::advanced::input_method::Event as Ime;
    let mut ui = browsing();
    ui.step(&[iced::Event::InputMethod(Ime::Opened)]);
    ui.step(&[iced::Event::InputMethod(Ime::Preedit("か".to_string(), None))]);
    let messages = tap_panic(&mut ui);
    assert_panicked(&ui, &messages);
    ui.step(&[iced::Event::InputMethod(Ime::Commit("か".to_string()))]);
    let browser = ui.app().overlays.browser().expect("the browser stays open");
    assert_eq!(browser.model().query, "か", "the composition went on");
}
