//! The headless `Ui` from outside the crate: real key events, read back from
//! what the window shows.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use loom_app::ui_test::Ui;

/// F1 with nothing selected opens the shortcuts in help, and Escape closes it.
#[test]
fn f1_opens_help_and_escape_closes_it() {
    let mut ui = Ui::headless("subtractive.ron").expect("the example loads");
    assert!(ui.snapshot_json()["help"].is_null());

    ui.tap_key(Key::Named(Named::F1), Modifiers::empty());
    assert!(!ui.snapshot_json()["help"].is_null());

    ui.tap_key(Key::Named(Named::Escape), Modifiers::empty());
    assert!(ui.snapshot_json()["help"].is_null());
}

/// Command shortcuts reach real files and dialogs from here, so they are
/// refused: Cmd+S would overwrite the example patch.
#[test]
#[should_panic(expected = "command shortcuts reach real files and dialogs")]
fn a_command_shortcut_is_refused() {
    let mut ui = Ui::headless("subtractive.ron").expect("the example loads");
    ui.tap_key(Key::Character("s".into()), Modifiers::COMMAND);
}

/// Under `tests/` the fake dialogs are off, so the harness's driver has no
/// answer for a dialog, and it panics, naming it, rather than open a real
/// one or skip it (ui-layer.md, section 2, rule 3). Open is pressed as a
/// player presses it, on the toolbar.
#[test]
#[should_panic(expected = "the headless driver has no answer for file dialog")]
fn a_dialog_with_no_answer_panics_naming_it() {
    let mut ui = Ui::headless("subtractive.ron").expect("the example loads");
    ui.press_open();
}
