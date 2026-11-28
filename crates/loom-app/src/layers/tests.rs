//! Tests for the window's layers (`layers.rs`): what each report hands the
//! shell.

use super::AppLayers;
use crate::pointer::Step;
use crate::ui_test::Ui;
use loom_ui::shell::{Layers, Redraw};
use loom_ui_core::{Key, KeyEvent, Modifiers};

/// A scripted key press.
fn key(key: Key) -> Step {
    Step::Key(KeyEvent::Pressed {
        key,
        modifiers: Modifiers::NONE,
        repeat: false,
        text: None,
        code: loom_ui_core::Code::Other,
    })
}

/// With nothing scripted, a report hands the shell no input and asks for
/// no frame, so an idle window does not spin.
#[test]
fn a_report_with_nothing_scripted_asks_for_nothing() {
    let mut ui = Ui::open("subtractive.ron");
    let mut layers = AppLayers::new();
    let report = ui.with_parts(|parts| layers.report(parts));
    assert!(report.input.is_empty());
    assert_eq!(report.redraw, Redraw::No);
}

/// Scripted steps go in one a report, each after the last one's frame is
/// drawn: the report after a step hands none until a frame the window
/// shows is drawn, and a capture's frame does not count.
#[test]
fn scripted_steps_go_in_one_a_report_after_each_frame() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app()
        .pointer
        .push(vec![key(Key::Character('a')), key(Key::Character('b'))]);
    let mut layers = AppLayers::new();
    let mut report = |layers: &mut AppLayers| ui.with_parts(|parts| layers.report(parts));
    let first = report(&mut layers);
    assert_eq!(first.input.len(), 1, "one step");
    assert_eq!(first.redraw, Redraw::NextFrame);
    assert!(report(&mut layers).input.is_empty(), "none before its frame");
    layers.drawn(true);
    assert!(report(&mut layers).input.is_empty(), "a capture is not its frame");
    layers.drawn(false);
    assert_eq!(report(&mut layers).input.len(), 1, "the next, after the frame");
    layers.drawn(false);
    assert!(report(&mut layers).input.is_empty(), "and nothing is left");
}
