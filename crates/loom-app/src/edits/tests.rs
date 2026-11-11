use super::*;
use loom_core::{Controller, EngineConfig, ModuleId, create_engine};
use std::collections::BTreeMap;

/// A controller with one filter, the filter's ID, its Cutoff index, and
/// an empty set of positions. The engine half is dropped: the controller
/// still sends its commands until the queue is full.
fn filter() -> (Controller, ModuleId, usize, BTreeMap<u32, (f32, f32)>) {
    let mut c = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0)).controller;
    let id = c.add_module("loom.filter.svf").unwrap();
    c.commit().unwrap();
    let cutoff = c.graph().node(id).unwrap().descriptor.param_index("cutoff").unwrap();
    (c, id, cutoff, BTreeMap::new())
}

/// Starts an edit, as `App::begin_edit` does.
fn begin(edits: &mut Edits, c: &Controller, positions: &BTreeMap<u32, (f32, f32)>) {
    edits.begin(c.has_uncommitted_changes(), || Snapshot::take(c, positions));
}

/// Sets a parameter, as `App::set_param` does inside an edit.
fn set(edits: &mut Edits, c: &mut Controller, id: ModuleId, index: usize, value: f32) {
    c.set_param(id, index, value).unwrap();
    edits.touch(id.0, index);
}

/// Records the edit, as `App::record` does.
fn record(edits: &mut Edits, c: &Controller, positions: &BTreeMap<u32, (f32, f32)>) {
    edits.record(None, || Snapshot::take(c, positions));
}

/// Sets a parameter as one whole edit outside a gesture, as `App::edit`
/// does, and commits it.
fn edit(
    edits: &mut Edits,
    c: &mut Controller,
    positions: &BTreeMap<u32, (f32, f32)>,
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

/// Every arrow-group transition, against the table in `docs/design/undo.md`
/// and #430: which state follows each event, and what happens to the
/// gesture. `t0` is a release time; its deadline is `t0 + 500 ms`.
#[test]
fn every_arrow_group_transition() {
    use ArrowEffect as Do;
    use ArrowEvent as E;
    let t0 = Instant::now();
    let due = t0 + ARROW_GROUP;
    let before = due - Duration::from_millis(1);
    let after = due + Duration::from_millis(1);
    let released = ArrowGroup::Released { due };
    let later = ArrowGroup::Released {
        due: before + ARROW_GROUP,
    };
    let cases = [
        // Closed: only a step opens a group.
        (ArrowGroup::Closed, E::Step(t0), ArrowGroup::Held, Do::Start),
        (ArrowGroup::Closed, E::Pressed(t0), ArrowGroup::Closed, Do::Nothing),
        (ArrowGroup::Closed, E::Released(t0), ArrowGroup::Closed, Do::Nothing),
        (ArrowGroup::Closed, E::End, ArrowGroup::Closed, Do::Nothing),
        (ArrowGroup::Closed, E::Due(after), ArrowGroup::Closed, Do::Nothing),
        (ArrowGroup::Closed, E::Close, ArrowGroup::Closed, Do::Nothing),
        // Held: repeats join, key up starts the 500 ms, the canvas can end it,
        // and the app's own close waits for the key.
        (ArrowGroup::Held, E::Step(t0), ArrowGroup::Held, Do::Nothing),
        (ArrowGroup::Held, E::Pressed(t0), ArrowGroup::Held, Do::Nothing),
        (ArrowGroup::Held, E::Released(t0), released, Do::Nothing),
        (ArrowGroup::Held, E::End, ArrowGroup::Closed, Do::End),
        (ArrowGroup::Held, E::Due(after), ArrowGroup::Held, Do::Nothing),
        (ArrowGroup::Held, E::Close, ArrowGroup::Held, Do::Nothing),
        // Released: in time joins, late ends, a second key-up moves the
        // deadline, and only a timer at or past the deadline ends it.
        (released, E::Pressed(before), ArrowGroup::Held, Do::Nothing),
        (released, E::Pressed(due), ArrowGroup::Closed, Do::End),
        (released, E::Step(before), ArrowGroup::Held, Do::Nothing),
        (released, E::Step(after), ArrowGroup::Held, Do::EndAndStart),
        (released, E::Released(before), later, Do::Nothing),
        (released, E::Due(before), released, Do::Nothing),
        (released, E::Due(due), ArrowGroup::Closed, Do::End),
        (released, E::End, ArrowGroup::Closed, Do::End),
        (released, E::Close, ArrowGroup::Closed, Do::End),
    ];
    for (from, event, to, effect) in cases {
        assert_eq!(from.on(event), (to, effect), "{event:?} from {from:?}");
    }
}

/// Ending any gesture closes the arrow group, so an open group always has an
/// open gesture, and a later end cannot close another gesture.
#[test]
fn ending_a_gesture_closes_the_arrow_group() {
    let (c, _, _, positions) = filter();
    let mut edits = Edits::default();
    edits.start_gesture();
    edits.set_arrow_group(ArrowGroup::Held);
    edits.end_gesture(false, None, || Snapshot::take(&c, &positions));
    assert_eq!(edits.arrow_group(), ArrowGroup::Closed);
}

/// The effects say whether they end or open a gesture.
#[test]
fn arrow_effects_end_and_start() {
    assert!(!ArrowEffect::Nothing.ends() && !ArrowEffect::Nothing.starts());
    assert!(ArrowEffect::End.ends() && !ArrowEffect::End.starts());
    assert!(!ArrowEffect::Start.ends() && ArrowEffect::Start.starts());
    assert!(ArrowEffect::EndAndStart.ends() && ArrowEffect::EndAndStart.starts());
}

/// A screen reader's action on `node` at `at`, and its release.
fn reader_step(group: ReaderGroup, node: u64, at: Instant) -> (ReaderGroup, ArrowEffect) {
    let (group, effect) = group.on(ReaderEvent::Action { node, at });
    assert!(matches!(group, ReaderGroup::Stepping { .. }));
    // While the step is applied, the edit it makes does not close it.
    assert_eq!(group.on(ReaderEvent::Close), (group, ArrowEffect::Nothing));
    assert_eq!(group.due(), None);
    let (group, released) = group.on(ReaderEvent::Applied);
    assert_eq!(released, ArrowEffect::Nothing);
    (group, effect)
}

/// A screen reader's actions on one control, each within 500 ms of the
/// last, stay in one group, and each one moves the deadline.
#[test]
fn reader_actions_on_one_control_join_a_group() {
    let t = Instant::now();
    let ms = Duration::from_millis;
    let (group, effect) = reader_step(ReaderGroup::Closed, 7, t);
    assert_eq!(effect, ArrowEffect::Start);
    assert_eq!(group.due(), Some(t + ARROW_GROUP));
    let (group, effect) = reader_step(group, 7, t + ms(400));
    assert_eq!(effect, ArrowEffect::Nothing);
    assert_eq!(group.due(), Some(t + ms(900)));
    // A wake before the new deadline leaves the group open.
    let (group, effect) = group.on(ReaderEvent::Due(t + ms(600)));
    assert_eq!(effect, ArrowEffect::Nothing);
    let (group, effect) = group.on(ReaderEvent::Due(t + ms(900)));
    assert_eq!((group, effect), (ReaderGroup::Closed, ArrowEffect::End));
}

/// An action on another control, or one that comes too late, ends the
/// group and opens a new one.
#[test]
fn a_reader_group_ends_on_another_control_or_a_late_action() {
    let t = Instant::now();
    let ms = Duration::from_millis;
    let (open, _) = reader_step(ReaderGroup::Closed, 7, t);
    let (group, effect) = reader_step(open, 8, t + ms(100));
    assert_eq!(effect, ArrowEffect::EndAndStart);
    assert_eq!(
        group,
        ReaderGroup::Open {
            node: 8,
            due: t + ms(600)
        }
    );
    let (_, effect) = reader_step(open, 7, t + ARROW_GROUP);
    assert_eq!(effect, ArrowEffect::EndAndStart);
}

/// Close ends an open group, and nothing happens to a closed one, which
/// asks for no wake.
#[test]
fn a_reader_group_closes_and_a_closed_one_is_quiet() {
    let t = Instant::now();
    let (open, _) = reader_step(ReaderGroup::Closed, 7, t);
    assert_eq!(open.on(ReaderEvent::Close), (ReaderGroup::Closed, ArrowEffect::End));
    assert_eq!(open.on(ReaderEvent::Applied), (open, ArrowEffect::Nothing));
    for event in [ReaderEvent::Close, ReaderEvent::Due(t), ReaderEvent::Applied] {
        assert_eq!(
            ReaderGroup::Closed.on(event),
            (ReaderGroup::Closed, ArrowEffect::Nothing)
        );
    }
    assert_eq!(ReaderGroup::Closed.due(), None);
}
