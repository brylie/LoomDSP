use super::*;
use loom_canvas::PortRef;

/// Output 0 of module `module`.
fn port(module: u32) -> PortRef {
    PortRef {
        module,
        index: 0,
        output: true,
    }
}

/// A drag from `from`, over the window.
fn drag_from(from: PortRef) -> AssignDrag {
    AssignDrag {
        from,
        start: Point::new(10.0, 10.0),
        at: Some(Point::new(200.0, 50.0)),
        zoom: 1.0,
    }
}

/// The Add modulation list opens on one parameter at a time, and a second
/// click on the same one closes it.
#[test]
fn the_add_list_toggles_per_parameter() {
    let mut gestures = Gestures::default();
    gestures.toggle_adding((4, 0));
    assert_eq!(gestures.adding(), Some((4, 0)));
    gestures.toggle_adding((4, 1));
    assert_eq!(gestures.adding(), Some((4, 1)));
    gestures.toggle_adding((4, 1));
    assert_eq!(gestures.adding(), None);
}

/// A row under the pointer counts only during an output drag, and not in
/// learn mode.
#[test]
fn a_row_counts_only_during_a_drag() {
    let mut gestures = Gestures::default();
    gestures.hover_row(Some((4, 0)), true);
    assert_eq!(gestures.drop_row(), None, "no drag yet");
    gestures.drag(drag_from(port(2)));
    gestures.hover_row(Some((4, 0)), false);
    assert_eq!(gestures.drop_row(), None, "not allowed in learn mode");
    gestures.hover_row(Some((4, 0)), true);
    assert_eq!(gestures.drop_row(), Some((4, 0)));
}

/// A drop assigns to the row under the drag only when it landed and came
/// from the same output. The drag ends either way.
#[test]
fn a_drop_assigns_only_its_own_landed_drag() {
    let mut gestures = Gestures::default();
    gestures.drag(drag_from(port(2)));
    gestures.hover_row(Some((4, 0)), true);
    assert_eq!(gestures.finish_drag(port(3), true), None, "another output's drop");
    assert!(gestures.assign_drag().is_none());
    assert!(gestures.drop_row().is_none());

    gestures.drag(drag_from(port(2)));
    gestures.hover_row(Some((4, 0)), true);
    assert_eq!(gestures.finish_drag(port(2), false), None, "it did not land");

    gestures.drag(drag_from(port(2)));
    gestures.hover_row(Some((4, 0)), true);
    assert_eq!(gestures.finish_drag(port(2), true), Some((4, 0)));
    assert!(gestures.assign_drag().is_none());
}

/// An arc drag picks its assignment once, and keeps that choice, even
/// none, until it ends.
#[test]
fn an_arc_drag_keeps_its_first_pick() {
    let mut gestures = Gestures::default();
    assert!(!gestures.end_depth(), "no drag yet");
    assert_eq!(gestures.pick_depth(|| None), None);
    assert!(gestures.depth_dragging());
    assert_eq!(
        gestures.pick_depth(|| Some(AssignmentId(7))),
        None,
        "the first pick holds"
    );
    assert!(gestures.end_depth());
    assert_eq!(gestures.pick_depth(|| Some(AssignmentId(7))), Some(AssignmentId(7)));
    assert_eq!(gestures.depth_target(), Some(AssignmentId(7)));
}

/// A label draft belongs to one placeholder: taking it for another drops it.
#[test]
fn a_label_draft_belongs_to_its_module() {
    let mut gestures = Gestures::default();
    gestures.draft_label(5, "Bass".to_string());
    assert_eq!(gestures.label_draft(5), Some("Bass"));
    assert_eq!(gestures.label_draft(6), None);
    assert_eq!(gestures.take_label(6), None);
    assert_eq!(gestures.label_draft(5), None, "another module's take dropped it");
    gestures.draft_label(5, "Lead".to_string());
    assert_eq!(gestures.take_label(5).as_deref(), Some("Lead"));
}
