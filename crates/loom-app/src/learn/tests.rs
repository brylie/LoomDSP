use super::*;
use loom_core::{EngineConfig, MidiControl, create_engine};

const CC74: MidiControl = MidiControl {
    channel: 0,
    controller: 74,
};

#[test]
fn labels_follow_mappings_and_the_armed_control() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let controller = &mut parts.controller;
    let filter = controller.add_module("loom.filter.svf").unwrap();
    let d = controller.graph().node(filter).unwrap().descriptor;
    let cutoff = d.param_index("cutoff").unwrap();
    let resonance = d.param_index("resonance").unwrap();
    let widget_of = |param: usize| {
        d.panel
            .widgets
            .iter()
            .position(|w| d.param_index(w.bind()) == Some(param))
            .unwrap()
    };
    controller.map_midi(CC74, filter, cutoff).unwrap();

    let view = view(
        controller,
        &loom_patch::Hierarchy::flat(),
        &loom_patch::ModulePath::root(),
        Some(Armed {
            param: (filter.0, resonance),
            serial: None,
        }),
    );
    assert_eq!(view.armed, Some((filter.0, widget_of(resonance))));
    assert_eq!(view.labels.len(), 1);
    assert_eq!(view.labels[&(filter.0, widget_of(cutoff))], "CC 74");
}

#[test]
fn the_status_names_the_parameter_a_control_left() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let controller = &mut parts.controller;
    let filter = controller.add_module("loom.filter.svf").unwrap();
    controller.map_midi_named(CC74, filter, "cutoff").unwrap();
    let (_, removed) = controller.map_midi_named(CC74, filter, "resonance").unwrap();
    let (_, mapping) = controller.graph().midi_mapping_on(CC74).unwrap();
    let status = mapped_status(controller, *mapping, &removed);
    assert!(status.starts_with("Mapped CC 74 · ch 1 to "), "{status}");
    assert!(status.contains("no longer moves"), "{status}");
    assert!(status.ends_with("Cutoff"), "{status}");

    // A new mapping on a free control says nothing about a move.
    let (_, removed) = controller
        .map_midi_named(MidiControl { controller: 71, ..CC74 }, filter, "cutoff")
        .unwrap();
    let (_, mapping) = controller.graph().midi_mapping_of(filter, 0).unwrap();
    let status = mapped_status(controller, *mapping, &removed);
    assert!(!status.contains("no longer"), "{status}");
}

/// Learn mode shows its view only while on, and turning it off disarms.
#[test]
fn stopping_learn_mode_disarms_and_hides_the_view() {
    let mut learn = Learn::default();
    assert!(!learn.is_on());
    assert!(learn.shown().is_none());
    learn.start();
    assert!(learn.shown().is_some());
    learn.arm((4, 1), Some(7));
    assert!(learn.is_armed((4, 1)));
    assert!(!learn.is_armed((4, 0)));
    assert_eq!(learn.armed().and_then(|armed| armed.serial), Some(7));
    learn.stop();
    assert!(!learn.is_on());
    assert!(learn.armed().is_none());
    assert!(learn.shown().is_none());
}

/// Removing the armed parameter's module disarms it at the next refresh,
/// and the view follows the armed control.
#[test]
fn a_removed_module_disarms_its_parameter() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let controller = &mut parts.controller;
    let filter = controller.add_module("loom.filter.svf").unwrap();
    let cutoff = controller
        .graph()
        .node(filter)
        .unwrap()
        .descriptor
        .param_index("cutoff")
        .unwrap();
    let mut learn = Learn::default();
    learn.start();
    learn.arm((filter.0, cutoff), None);
    learn.refresh(
        controller,
        &loom_patch::Hierarchy::flat(),
        &loom_patch::ModulePath::root(),
    );
    assert!(learn.shown().unwrap().armed.is_some());

    controller.remove_module(filter).unwrap();
    learn.refresh(
        controller,
        &loom_patch::Hierarchy::flat(),
        &loom_patch::ModulePath::root(),
    );
    assert!(learn.armed().is_none());
    assert!(learn.shown().unwrap().armed.is_none());
}

/// A waiting pickup's hardware at `value`, moved `sequence` times.
fn waiting(value: f32, sequence: u32) -> MidiWaiting {
    MidiWaiting {
        target: ModuleId(4),
        param: 0,
        value,
        sequence,
    }
}

/// A ghost shows for one second after its hardware last moved, and goes as
/// soon as the pickup catches (midi-learn.md, Mapping modes).
#[test]
fn a_ghost_lasts_one_second_after_the_last_move() {
    let start = Instant::now();
    let mut ghosts = Ghosts::default();
    assert!(ghosts.update(&[waiting(0.25, 1)], start), "it appears");
    assert_eq!(ghosts.shown().get(&(4, 0)), Some(&0.25));
    assert!(!ghosts.update(&[waiting(0.25, 1)], start + GHOST_TIME / 2), "no change");

    // Another move starts the second again.
    assert!(ghosts.update(&[waiting(0.5, 2)], start + GHOST_TIME / 2));
    assert_eq!(ghosts.shown().get(&(4, 0)), Some(&0.5));
    assert!(!ghosts.update(&[waiting(0.5, 2)], start + GHOST_TIME));
    assert!(
        ghosts.update(&[waiting(0.5, 2)], start + GHOST_TIME + GHOST_TIME / 2),
        "it runs out"
    );
    assert!(ghosts.shown().is_empty());
    assert!(
        !ghosts.update(&[waiting(0.5, 2)], start + GHOST_TIME * 3),
        "it stays gone while the hardware rests"
    );

    // A new move brings it back, and catching removes it at once.
    assert!(ghosts.update(&[waiting(0.75, 3)], start + GHOST_TIME * 3));
    assert!(ghosts.update(&[], start + GHOST_TIME * 3));
    assert!(ghosts.shown().is_empty());
}

/// Only one mapping's rows are open, and a draft does not outlive them.
#[test]
fn options_rows_open_one_at_a_time() {
    let mut learn = Learn::default();
    learn.toggle_options((4, 0));
    assert!(learn.options_open((4, 0)));
    learn.set_range_draft(Some(RangeDraft {
        param: (4, 0),
        low: "x".to_string(),
        high: "1".to_string(),
        error: None,
    }));
    assert!(learn.range_draft((4, 0)).is_some());
    assert!(learn.range_draft((4, 1)).is_none());
    learn.toggle_options((4, 1));
    assert!(!learn.options_open((4, 0)));
    assert!(learn.options_open((4, 1)));
    assert!(learn.range_draft((4, 0)).is_none());
    learn.toggle_options((4, 1));
    assert!(!learn.options_open((4, 1)));
}
