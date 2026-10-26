use super::*;
use iced::Point;
use loom_core::{Controller, EngineConfig, ModuleId, create_engine};
use std::collections::BTreeMap;

/// A controller with one filter, the filter's ID, its Cutoff index, and
/// an empty set of positions. The engine half is dropped: the controller
/// still sends its commands until the queue is full.
fn filter() -> (Controller, ModuleId, usize, BTreeMap<u32, Point>) {
    let mut c = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0)).controller;
    let id = c.add_module("loom.filter.svf").unwrap();
    c.commit().unwrap();
    let cutoff = c.graph().node(id).unwrap().descriptor.param_index("cutoff").unwrap();
    (c, id, cutoff, BTreeMap::new())
}

/// Starts an edit, as `App::begin_edit` does.
fn begin(edits: &mut Edits, c: &Controller, positions: &BTreeMap<u32, Point>) {
    edits.begin(c.has_uncommitted_changes(), || Snapshot::take(c, positions));
}

/// Sets a parameter, as `App::set_param` does inside an edit.
fn set(edits: &mut Edits, c: &mut Controller, id: ModuleId, index: usize, value: f32) {
    c.set_param(id, index, value).unwrap();
    edits.touch(id.0, index);
}

/// Records the edit, as `App::record` does.
fn record(edits: &mut Edits, c: &Controller, positions: &BTreeMap<u32, Point>) {
    edits.record(None, || Snapshot::take(c, positions));
}

/// Sets a parameter as one whole edit outside a gesture, as `App::edit`
/// does, and commits it.
fn edit(
    edits: &mut Edits,
    c: &mut Controller,
    positions: &BTreeMap<u32, Point>,
    id: ModuleId,
    index: usize,
    value: f32,
) {
    begin(edits, c, positions);
    set(edits, c, id, index, value);
    edits.committed(c.commit().is_ok());
    record(edits, c, positions);
}

/// A failed commit clears the history, drops the edit in progress, and
/// keeps recording off until the graph reaches the engine. The graph can
/// reach it by another path than an edit (as `App::sync_probes` sends
/// it), and the next edit then records again.
#[test]
fn a_failed_commit_clears_the_history_and_stops_recording_until_the_graph_is_sent() {
    let (mut c, id, cutoff, positions) = filter();
    let mut edits = Edits::default();
    edit(&mut edits, &mut c, &positions, id, cutoff, 500.0);
    assert_eq!(edits.history.len(), 1);

    // An edit whose commit fails: the graph keeps a change the engine
    // does not have (here a module the engine was never sent).
    begin(&mut edits, &c, &positions);
    c.add_module("loom.osc.sine").unwrap();
    edits.committed(false);
    record(&mut edits, &c, &positions);
    assert_eq!(edits.history.len(), 0, "the history is cleared");
    assert!(edits.history_off());

    // While the engine is behind, an edit makes no step.
    begin(&mut edits, &c, &positions);
    set(&mut edits, &mut c, id, cutoff, 700.0);
    record(&mut edits, &c, &positions);
    assert!(c.has_uncommitted_changes());
    assert_eq!(edits.history.len(), 0, "no step while the engine is behind");
    assert!(edits.history_off());

    // The graph reaches the engine without an edit's commit: the next
    // edit records again.
    c.commit().unwrap();
    begin(&mut edits, &c, &positions);
    assert!(!edits.history_off());
    set(&mut edits, &mut c, id, cutoff, 800.0);
    edits.committed(c.commit().is_ok());
    record(&mut edits, &c, &positions);
    assert_eq!(edits.history.len(), 1);
    assert_eq!(edits.history.undo_step().unwrap().name(), "change SVF Cutoff");
}

/// A commit that works turns recording back on, outside a gesture.
#[test]
fn a_commit_that_works_turns_recording_back_on() {
    let mut edits = Edits::default();
    edits.committed(false);
    assert!(edits.history_off());
    edits.committed(true);
    assert!(!edits.history_off());
}

/// A gesture cut by a failed commit stays out of the history as a whole:
/// the part before the failure is dropped, and the part after it is not a
/// step either, even when a commit works again inside the gesture. After
/// the gesture, edits record again.
#[test]
fn a_gesture_cut_by_a_failed_commit_stays_out_of_the_history() {
    let (mut c, id, cutoff, positions) = filter();
    let mut edits = Edits::default();
    edits.start_gesture();
    begin(&mut edits, &c, &positions);
    set(&mut edits, &mut c, id, cutoff, 500.0);
    edits.committed(false);

    // The rest of the drag, and a commit that works inside it.
    for value in [600.0, 700.0] {
        begin(&mut edits, &c, &positions);
        set(&mut edits, &mut c, id, cutoff, value);
    }
    edits.committed(c.commit().is_ok());
    assert!(edits.history_off(), "off until the gesture ends");
    edits.end_gesture(c.has_uncommitted_changes(), None, || Snapshot::take(&c, &positions));
    assert_eq!(edits.history.len(), 0, "no part of the cut gesture is a step");
    assert!(!edits.history_off(), "the engine has the graph again");

    edit(&mut edits, &mut c, &positions, id, cutoff, 900.0);
    assert_eq!(edits.history.len(), 1, "the next edit records");
}

/// A gesture is one step, however many changes it makes. Recording in
/// the middle of it, as `App::finish_edits` does before a save, makes a
/// step of what was done so far and keeps the gesture open, so the rest
/// of it is one more step.
#[test]
fn a_gesture_is_one_step_and_finishing_edits_splits_it() {
    let (mut c, id, cutoff, positions) = filter();
    let mut edits = Edits::default();
    edits.start_gesture();
    for value in [500.0, 600.0, 700.0] {
        begin(&mut edits, &c, &positions);
        set(&mut edits, &mut c, id, cutoff, value);
    }
    assert_eq!(edits.history.len(), 0, "the gesture's end makes the step");
    record(&mut edits, &c, &positions);
    assert_eq!(edits.history.len(), 1);
    assert!(edits.gesture_open(), "the drag goes on");

    begin(&mut edits, &c, &positions);
    set(&mut edits, &mut c, id, cutoff, 800.0);
    edits.end_gesture(false, None, || Snapshot::take(&c, &positions));
    assert_eq!(edits.history.len(), 2, "the rest of the drag is one more step");
    assert!(!edits.gesture_open());
}

/// A gesture that ends at its starting value is not a step (undo.md,
/// rule 3).
#[test]
fn a_gesture_back_to_its_start_is_no_step() {
    let (mut c, id, cutoff, positions) = filter();
    let start = c.graph().node(id).unwrap().params[cutoff];
    let mut edits = Edits::default();
    edits.start_gesture();
    for value in [500.0, start] {
        begin(&mut edits, &c, &positions);
        set(&mut edits, &mut c, id, cutoff, value);
    }
    edits.end_gesture(false, None, || Snapshot::take(&c, &positions));
    assert_eq!(edits.history.len(), 0);
}

/// A slider gesture opens at its first change, does not open over a
/// canvas gesture, and closes with [`Edits::end_gesture`].
#[test]
fn a_slider_gesture_opens_at_the_first_change() {
    let mut edits = Edits::default();
    edits.start_slider_gesture();
    assert!(edits.gesture_open() && edits.slider_gesture_open());
    edits.end_gesture(false, None, || unreachable!("no edit is open"));
    assert!(!edits.gesture_open() && !edits.slider_gesture_open());

    edits.start_gesture();
    edits.start_slider_gesture();
    assert!(!edits.slider_gesture_open(), "a canvas gesture is open");
}

/// The saved mark follows the history: an edit after a save makes the
/// patch unsaved, its undo makes it saved again, and a failed commit,
/// which clears the history, leaves it unsaved.
#[test]
fn the_saved_mark_follows_the_history_through_a_failed_commit() {
    let (mut c, id, cutoff, positions) = filter();
    let mut edits = Edits::default();
    edits.history.mark_saved();
    edit(&mut edits, &mut c, &positions, id, cutoff, 500.0);
    assert!(!edits.history.is_saved());
    edits.history.undone();
    assert!(edits.history.is_saved(), "undo comes back to the saved patch");
    edits.history.redone();
    edits.committed(false);
    assert!(!edits.history.is_saved(), "no history, so no saved state in it");
}
