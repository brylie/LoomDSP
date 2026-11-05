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
