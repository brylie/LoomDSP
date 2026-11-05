//! Arrow-key undo groups, through real key and pointer events (#429).
//!
//! An arrow key on a focused control edits at once, but its undo step stays
//! open for 500 ms after the release, so a quick tap, tap is one step
//! (`docs/design/undo.md`). These tests drive the whole app view, so a change
//! in who owns the group, or in the order of its events, shows up here. They
//! pass a redraw time instead of sleeping, so the deadline is exact.

use super::*;
use crate::question::{self, AfterDiscard, Answer};
use std::time::Duration;

/// The patch the tests play with, and its filter: module 4, whose Cutoff
/// knob takes the arrows.
const PATCH: &str = "subtractive.ron";
const FILTER: u32 = 4;

/// Past the 500 ms an arrow group stays open after a release.
const PAST_GROUP: Duration = Duration::from_millis(600);

fn right() -> keyboard::Key {
    keyboard::Key::Named(keyboard::key::Named::ArrowRight)
}

/// Opens the patch and gives the filter's Cutoff knob the keys, as a click does.
fn open_on_cutoff() -> Ui {
    let mut ui = Ui::open(PATCH);
    let at = ui.control(FILTER, "cutoff");
    ui.move_to(at);
    ui.press();
    ui.release();
    ui
}

/// The filter's cutoff, in Hz.
fn cutoff(ui: &Ui) -> f32 {
    let node = ui.app.session.controller.graph().node(ModuleId(FILTER)).unwrap();
    node.params[node.descriptor.param_index("cutoff").unwrap()]
}

/// The index of the cutoff parameter, for inspector messages.
fn cutoff_index(ui: &Ui) -> usize {
    let node = ui.app.session.controller.graph().node(ModuleId(FILTER)).unwrap();
    node.descriptor.param_index("cutoff").unwrap()
}

/// Turns the inspector's Cutoff slider, as a drag on it does, without
/// letting go.
fn slide_cutoff(ui: &mut Ui, value: f32) {
    let index = cutoff_index(ui);
    ui.send(Message::Inspector(crate::inspector::Message::ParamValue {
        module: FILTER,
        index,
        value,
    }));
    ui.redraw();
}

/// How many steps the history keeps.
fn steps(ui: &Ui) -> usize {
    ui.app.session.edits.history.len()
}

/// Toolbar Undo right after an arrow release acts on the arrow step, before
/// the 500 ms deadline, and Redo brings it back.
#[test]
fn undo_before_the_deadline_takes_the_arrow_step() {
    let mut ui = open_on_cutoff();
    let original = cutoff(&ui);
    ui.tap(right(), keyboard::Modifiers::default());
    let arrowed = cutoff(&ui);
    assert_ne!(arrowed, original, "the arrow moved the knob");

    ui.click_text("Undo");
    assert_eq!(cutoff(&ui), original, "Undo did not wait for the deadline");
    assert!(!ui.app.session.edits.gesture_open());
    ui.click_text("Redo");
    assert_eq!(cutoff(&ui), arrowed, "Redo did not wait either");
}

/// An inspector edit right after an arrow release is a step of its own:
/// Undo takes the inspector edit first, then the arrow edit.
#[test]
fn an_inspector_edit_after_a_release_is_its_own_step() {
    let mut ui = open_on_cutoff();
    let original = cutoff(&ui);
    ui.tap(right(), keyboard::Modifiers::default());
    let arrowed = cutoff(&ui);

    slide_cutoff(&mut ui, 3_000.0);
    ui.send(Message::SliderReleased);
    ui.redraw_at(Instant::now() + PAST_GROUP);
    assert_eq!(cutoff(&ui), 3_000.0);

    ui.click_text("Undo");
    assert_eq!(cutoff(&ui), arrowed, "the inspector edit came off alone");
    ui.click_text("Undo");
    assert_eq!(cutoff(&ui), original, "then the arrow edit");
}

/// Two quick arrow taps are one step.
#[test]
fn two_quick_arrows_are_one_step() {
    let mut ui = open_on_cutoff();
    let original = cutoff(&ui);
    let before = steps(&ui);
    ui.tap(right(), keyboard::Modifiers::default());
    ui.tap(right(), keyboard::Modifiers::default());
    ui.redraw_at(Instant::now() + PAST_GROUP);
    assert_eq!(steps(&ui), before + 1, "one step for both taps");
    assert!(!ui.app.session.edits.gesture_open(), "the deadline closed it");

    ui.click_text("Undo");
    assert_eq!(cutoff(&ui), original, "one Undo takes both taps");
}

/// Once the deadline has passed, the next arrow starts a new step.
#[test]
fn an_arrow_after_the_deadline_is_a_new_step() {
    let mut ui = open_on_cutoff();
    let before = steps(&ui);
    ui.tap(right(), keyboard::Modifiers::default());
    ui.redraw_at(Instant::now() + PAST_GROUP);
    let first = cutoff(&ui);
    ui.tap(right(), keyboard::Modifiers::default());
    ui.redraw_at(Instant::now() + PAST_GROUP);
    assert_eq!(steps(&ui), before + 2, "a step for each tap");

    ui.click_text("Undo");
    assert_eq!(cutoff(&ui), first, "Undo takes only the second tap");
}

/// After toolbar Undo closes a released group, a quick arrow is a new step,
/// not a part of the step that was undone. The click on Undo took the keys
/// from the knob, so that arrow moves the selected tile.
#[test]
fn an_arrow_right_after_undo_is_a_new_step() {
    let mut ui = open_on_cutoff();
    let original = cutoff(&ui);
    let place = ui.app.session.positions[&FILTER];
    ui.tap(right(), keyboard::Modifiers::default());
    ui.click_text("Undo");
    assert_eq!(cutoff(&ui), original);

    ui.tap(right(), keyboard::Modifiers::default());
    let moved = ui.app.session.positions[&FILTER];
    assert_ne!(moved, place, "the arrow moved the tile");
    ui.redraw_at(Instant::now() + PAST_GROUP);
    assert!(!ui.app.session.edits.gesture_open());
    ui.click_text("Undo");
    assert_eq!(ui.app.session.positions[&FILTER], place, "Undo takes the move back");
    assert_eq!(cutoff(&ui), original, "and leaves the knob as it was");
    ui.click_text("Redo");
    assert_eq!(ui.app.session.positions[&FILTER], moved, "Redo brings back the move");
    assert_eq!(cutoff(&ui), original, "not the undone knob step");
}

/// Holding an arrow down, so that it repeats, is one step.
#[test]
fn a_held_arrow_is_one_step() {
    let mut ui = open_on_cutoff();
    let original = cutoff(&ui);
    let before = steps(&ui);
    ui.key_down(right(), false);
    for _ in 0..3 {
        ui.key_down(right(), true);
    }
    ui.key_up(right());
    ui.redraw_at(Instant::now() + PAST_GROUP);
    assert!(cutoff(&ui) > original, "every repeat moved the knob");
    assert_eq!(steps(&ui), before + 1, "one step for the whole hold");
    ui.click_text("Undo");
    assert_eq!(cutoff(&ui), original);
}

/// An arrow press and the window losing focus in one frame: no key release
/// will come, so the group ends there, and Undo works at once.
#[test]
fn losing_focus_with_an_arrow_down_ends_the_group() {
    let mut ui = open_on_cutoff();
    let original = cutoff(&ui);
    ui.step(&[key_pressed(right(), false), Event::Window(window::Event::Unfocused)]);
    assert_ne!(cutoff(&ui), original, "the arrow moved the knob");
    assert!(!ui.app.session.edits.gesture_open(), "losing focus ended the group");

    ui.click_text("Undo");
    assert_eq!(cutoff(&ui), original);
}

/// What a test sees after replacing the patch while an old arrow group was
/// still released on the canvas: an inspector slider gesture on the new
/// patch, and the history's length. `replace` replaces the patch.
fn slide_after_replacing(mut ui: Ui, replace: impl FnOnce(&mut Ui)) {
    // Earlier than the old group's deadline, so this redraw cannot end it.
    let early = Instant::now();
    replace(&mut ui);
    ui.redraw_at(early);
    assert!(
        ui.app.session.controller.graph().node(ModuleId(FILTER)).is_some(),
        "the new patch has the filter"
    );
    ui.send(Message::Inspector(crate::inspector::Message::SelectModule(FILTER)));
    slide_cutoff(&mut ui, 2_500.0);
    assert!(ui.app.session.edits.slider_gesture_open());
    let before = steps(&ui);

    ui.redraw_at(Instant::now() + PAST_GROUP);
    assert!(
        ui.app.session.edits.slider_gesture_open(),
        "an old arrow group's end must not close the new patch's slider"
    );
    assert_eq!(steps(&ui), before, "and must make no step");
}

/// The path to the patch the tests open, to open it again.
fn patch_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/patches")
        .join(PATCH)
}

/// Replacing the patch while an arrow group is released cannot deliver the
/// old group's end into the new patch. Undo first makes the patch saved, so
/// no question opens and the canvas stays live across the replacement.
#[test]
fn a_new_patch_ignores_an_old_arrow_group() {
    let mut ui = open_on_cutoff();
    ui.tap(right(), keyboard::Modifiers::default());
    ui.click_text("Undo");
    assert!(!ui.app.unsaved(), "Undo made the patch saved, so no question opens");
    slide_after_replacing(ui, |ui| {
        ui.app.after_discard(AfterDiscard::OpenPath(patch_path()));
        assert!(ui.app.overlays.question.is_none());
    });
}

/// The same through the question that asks before losing the arrow edit.
#[test]
fn a_new_patch_after_the_question_ignores_an_old_arrow_group() {
    let mut ui = open_on_cutoff();
    ui.tap(right(), keyboard::Modifiers::default());
    slide_after_replacing(ui, |ui| {
        ui.app.after_discard(AfterDiscard::OpenPath(patch_path()));
        assert!(ui.app.overlays.question.is_some(), "the arrow edit is unsaved");
        // The question has the canvas's input for a frame, as in the app.
        ui.redraw();
        ui.send(Message::Question(question::Message::Answer(Answer::DontSave)));
        assert!(ui.app.overlays.question.is_none());
    });
}
