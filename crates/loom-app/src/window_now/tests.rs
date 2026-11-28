//! Tests for what the window shows now (`window_now.rs`).

use crate::Message;
use crate::ui_test::Ui;

/// An update reads what the window holds: the canvas once it is laid out,
/// and no field while none has the keys; with a field focused, that field.
#[test]
fn an_update_reads_what_the_window_holds() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let _ = ui.deliver(Message::Tick);
    assert!(ui.app().window_now.canvas.is_some(), "the canvas");
    assert_eq!(ui.app().focused_field_now(), None);
    let tags = crate::patch_info::tags_part();
    ui.focus(&tags);
    let _ = ui.deliver(Message::Tick);
    assert_eq!(ui.app().focused_field_now(), Some(&tags));
}
