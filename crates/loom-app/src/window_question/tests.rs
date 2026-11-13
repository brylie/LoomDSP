//! Tests for window questions: asked during a turn, answered after it, in
//! the order asked.

use super::*;
use crate::ui_test::Ui;

/// Questions asked in one turn are answered after it, each as a message on
/// a later update, in the order asked. The `Ui` harness answers them on its
/// own view, as the bridge asks Iced.
#[test]
fn questions_are_answered_after_the_turn_in_the_order_asked() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    ui.app.ask_window(WindowQuestion::places(|_| Message::ShowShortcuts));
    ui.app.ask_window(WindowQuestion::focus(|_| Message::ShowHelp));
    ui.app
        .ask_window(WindowQuestion::find("4".to_string(), |_| Message::CloseHelp));
    let log = ui.deliver(Message::Tick);
    let answers: Vec<_> = log
        .iter()
        .filter(|message| matches!(message, Message::ShowShortcuts | Message::ShowHelp | Message::CloseHelp))
        .map(|message| format!("{message:?}"))
        .collect();
    assert_eq!(answers, ["ShowShortcuts", "ShowHelp", "CloseHelp"]);
    assert!(ui.app.take_window_questions().is_empty(), "each asked once");
}

/// The answers carry what the window holds: the canvas's places, and a
/// focus of none while no field has the keys.
#[test]
fn answers_carry_what_the_window_holds() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let places = seen.clone();
    ui.app.ask_window(WindowQuestion::places(move |found| {
        places
            .lock()
            .unwrap()
            .push(format!("canvas: {}", found.canvas.is_some()));
        Message::Tick
    }));
    let focus = seen.clone();
    ui.app.ask_window(WindowQuestion::focus(move |part| {
        focus.lock().unwrap().push(format!("focus: {part:?}"));
        Message::Tick
    }));
    let _ = ui.deliver(Message::Tick);
    assert_eq!(*seen.lock().unwrap(), ["canvas: true", "focus: None"]);
}
