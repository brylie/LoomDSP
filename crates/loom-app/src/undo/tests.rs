use super::*;
use loom_core::{EngineConfig, EngineParts, create_engine};

fn parts() -> EngineParts {
    create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0))
}

fn snap(c: &Controller, positions: &BTreeMap<u32, Point>) -> Snapshot {
    Snapshot::take(c, positions)
}

/// Runs `change`, and returns the step it made.
fn record(c: &mut Controller, positions: &mut BTreeMap<u32, Point>, change: impl FnOnce(&mut Controller)) -> Step {
    let before = snap(c, positions);
    change(c);
    c.commit().unwrap();
    diff(&before, &snap(c, positions), &BTreeSet::new()).expect("the edit changed the patch")
}

fn undo(step: &Step, c: &mut Controller, positions: &mut BTreeMap<u32, Point>) {
    check(step, false, c).unwrap();
    apply(step, false, c, positions).unwrap();
    c.commit().unwrap();
}

fn redo(step: &Step, c: &mut Controller, positions: &mut BTreeMap<u32, Point>) {
    check(step, true, c).unwrap();
    apply(step, true, c, positions).unwrap();
    c.commit().unwrap();
}

#[test]
fn undo_removes_an_added_module_and_redo_brings_it_back_with_its_id_and_place() {
    let mut c = parts().controller;
    let mut positions = BTreeMap::new();
    let before = snap(&c, &positions);
    let id = c.add_module("loom.osc.sine").unwrap();
    positions.insert(id.0, Point::new(40.0, 80.0));
    c.commit().unwrap();
    let step = diff(&before, &snap(&c, &positions), &BTreeSet::new()).unwrap();
    assert_eq!(step.name(), "add Sine");
    undo(&step, &mut c, &mut positions);
    assert!(c.graph().node(id).is_none());
    assert!(!positions.contains_key(&id.0));
    redo(&step, &mut c, &mut positions);
    assert!(c.graph().node(id).is_some());
    assert_eq!(positions[&id.0], Point::new(40.0, 80.0));
}

#[test]
fn a_change_that_ends_where_it_began_is_no_step() {
    let mut c = parts().controller;
    let id = c.add_module("loom.math.mul").unwrap();
    let positions = BTreeMap::new();
    let before = snap(&c, &positions);
    let start = c.graph().node(id).unwrap().params[0];
    c.set_param_named(id, "gain", 0.9).unwrap();
    c.set_param_named(id, "gain", start).unwrap();
    assert!(diff(&before, &snap(&c, &positions), &BTreeSet::new()).is_none());
}

#[test]
fn a_parameter_drag_is_one_step_that_undo_and_redo_swap() {
    let mut c = parts().controller;
    let id = c.add_module("loom.math.mul").unwrap();
    let mut positions = BTreeMap::new();
    let first = c.graph().node(id).unwrap().params.clone();
    let step = record(&mut c, &mut positions, |c| {
        for value in [0.1, 0.4, 0.8] {
            c.set_param_named(id, "gain", value).unwrap();
        }
    });
    assert_eq!(step.name(), "change Multiply Gain");
    undo(&step, &mut c, &mut positions);
    assert_eq!(c.graph().node(id).unwrap().params, first);
    redo(&step, &mut c, &mut positions);
    assert_eq!(c.graph().node(id).unwrap().params[0], 0.8);
}

#[test]
fn undo_of_a_connect_brings_back_the_cable_it_replaced() {
    let mut c = parts().controller;
    let a = c.add_module("loom.osc.sine").unwrap();
    let b = c.add_module("loom.osc.saw").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(a, "out", out, "in").unwrap();
    let mut positions = BTreeMap::new();
    let step = record(&mut c, &mut positions, |c| {
        c.connect_named(b, "out", out, "in").unwrap();
    });
    let input = c.input_ref(out, "in").unwrap();
    assert_eq!(c.graph().source_of(input), Some(c.output_ref(b, "out").unwrap()));
    undo(&step, &mut c, &mut positions);
    assert_eq!(c.graph().source_of(input), Some(c.output_ref(a, "out").unwrap()));
    redo(&step, &mut c, &mut positions);
    assert_eq!(c.graph().source_of(input), Some(c.output_ref(b, "out").unwrap()));
}

#[test]
fn removing_a_cable_and_adding_another_is_one_step() {
    let mut c = parts().controller;
    let a = c.add_module("loom.osc.sine").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    let join = c.add_module("loom.mix.join").unwrap();
    c.connect_named(join, "out", out, "in").unwrap();
    c.connect_named(a, "out", join, "left").unwrap();
    let mut positions = BTreeMap::new();
    let step = record(&mut c, &mut positions, |c| {
        let left = c.input_ref(join, "left").unwrap();
        c.disconnect(left).unwrap();
        c.connect_named(a, "out", join, "right").unwrap();
    });
    assert_eq!(step.name(), "move cable");
}

#[test]
fn undo_of_a_learned_mapping_restores_the_mapping_and_the_old_value() {
    let mut c = parts().controller;
    let id = c.add_module("loom.math.mul").unwrap();
    let mut positions = BTreeMap::new();
    let param = c.graph().node(id).unwrap().descriptor.param_index("gain").unwrap();
    let old = c.graph().node(id).unwrap().params[param];
    let before = snap(&c, &positions);
    let control = MidiControl {
        channel: 0,
        controller: 74,
    };
    c.map_midi(control, id, param).unwrap();
    c.set_param(id, param, 0.9).unwrap();
    c.commit().unwrap();
    let touched = BTreeSet::from([(id.0, param)]);
    let step = diff(&before, &snap(&c, &positions), &touched).unwrap();
    assert_eq!(step.name(), format!("learn {control}"));
    undo(&step, &mut c, &mut positions);
    assert!(c.graph().midi_mapping_of(id, param).is_none());
    assert_eq!(c.graph().node(id).unwrap().params[param], old);
    redo(&step, &mut c, &mut positions);
    assert_eq!(c.graph().midi_mapping_of(id, param).unwrap().1.control, control);
    assert_eq!(c.graph().node(id).unwrap().params[param], 0.9);
}

#[test]
fn removing_and_moving_a_mapping_can_be_undone() {
    let mut c = parts().controller;
    let id = c.add_module("loom.filter.svf").unwrap();
    let mut positions = BTreeMap::new();
    let control = MidiControl {
        channel: 0,
        controller: 7,
    };
    c.map_midi(control, id, 0).unwrap();
    let removed = record(&mut c, &mut positions, |c| {
        c.unmap_midi(id, 0).unwrap();
    });
    assert_eq!(removed.name(), "remove MIDI mapping");
    undo(&removed, &mut c, &mut positions);
    assert_eq!(c.graph().midi_mapping_of(id, 0).unwrap().1.control, control);
    let moved = record(&mut c, &mut positions, |c| {
        c.map_midi(control, id, 1).unwrap();
    });
    undo(&moved, &mut c, &mut positions);
    assert!(c.graph().midi_mapping_of(id, 1).is_none());
    assert_eq!(c.graph().midi_mapping_of(id, 0).unwrap().1.control, control);
}

#[test]
fn a_hardware_move_of_a_mapped_parameter_is_not_part_of_a_step() {
    let mut c = parts().controller;
    let id = c.add_module("loom.math.mul").unwrap();
    let positions = BTreeMap::new();
    c.map_midi(
        MidiControl {
            channel: 0,
            controller: 7,
        },
        id,
        0,
    )
    .unwrap();
    let before = snap(&c, &positions);
    c.set_param(id, 0, 0.77).unwrap();
    let after = snap(&c, &positions);
    assert!(diff(&before, &after, &BTreeSet::new()).is_none());
    assert!(diff(&before, &after, &BTreeSet::from([(id.0, 0)])).is_some());
}

/// Renders a second of the patch at a block size.
fn render(parts: &mut EngineParts, block: usize) -> Vec<f32> {
    let frames = 4_096;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let mut at = 0;
    while at < frames {
        let n = block.min(frames - at);
        parts.engine.process(&mut left[at..at + n], &mut right[at..at + n]);
        at += n;
    }
    left
}

/// A voice whose gain is modulated by two assignments from a small
/// oscillator bank. Returns the controller IDs that a delete takes.
fn modulated_patch(c: &mut Controller) -> Vec<ModuleId> {
    let phase = c.add_module("loom.phase.generator").unwrap();
    let carrier = c.add_module("loom.osc.sine").unwrap();
    let lfo_phase = c.add_module("loom.phase.generator").unwrap();
    c.set_param_named(lfo_phase, "tune", 3.0).ok();
    let lfo_a = c.add_module("loom.osc.sine").unwrap();
    let lfo_b = c.add_module("loom.osc.saw").unwrap();
    let vca = c.add_module("loom.math.mul").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(phase, "phase", carrier, "phase").unwrap();
    c.connect_named(carrier, "out", vca, "a").unwrap();
    c.connect_named(vca, "out", out, "in").unwrap();
    c.connect_named(lfo_phase, "phase", lfo_a, "phase").unwrap();
    c.connect_named(lfo_phase, "phase", lfo_b, "phase").unwrap();
    c.add_assignment_named(lfo_a, "out", vca, "gain", 0.3).unwrap();
    c.add_assignment_named(lfo_b, "out", vca, "gain", 0.2).unwrap();
    c.commit().unwrap();
    vec![lfo_phase, lfo_a, lfo_b]
}

#[test]
fn undo_of_a_delete_brings_everything_back_and_the_output_is_bit_identical() {
    for block in [1, 77, 512] {
        let mut untouched = parts();
        modulated_patch(&mut untouched.controller);
        let expected = render(&mut untouched, block);

        let mut edited = parts();
        let doomed = modulated_patch(&mut edited.controller);
        let mut positions: BTreeMap<u32, Point> = edited
            .controller
            .graph()
            .nodes()
            .map(|(id, _)| (id.0, Point::new(id.0 as f32 * 10.0, 0.0)))
            .collect();
        let before = snap(&edited.controller, &positions);
        let ids: Vec<_> = edited.controller.graph().assignments().map(|(id, _)| id).collect();
        for id in &doomed {
            edited.controller.remove_module(*id).unwrap();
            positions.remove(&id.0);
        }
        edited.controller.commit().unwrap();
        let step = diff(&before, &snap(&edited.controller, &positions), &BTreeSet::new()).unwrap();
        assert_eq!(step.name(), "delete 3 modules");
        assert_eq!(edited.controller.graph().assignments().count(), 0);

        undo(&step, &mut edited.controller, &mut positions);
        assert!(before == snap(&edited.controller, &positions), "the patch is as it was");
        let restored: Vec<_> = edited.controller.graph().assignments().map(|(id, _)| id).collect();
        assert_eq!(restored, ids, "assignments keep their ids and order");
        assert_eq!(render(&mut edited, block), expected, "block size {block}");
    }
}

#[test]
fn undo_with_no_room_for_the_modules_changes_nothing() {
    let mut config = EngineConfig::with_sample_rate(48_000.0);
    config.max_modules = 2;
    let mut c = create_engine(loom_modules::registry(), config).controller;
    let a = c.add_module("loom.osc.sine").unwrap();
    let b = c.add_module("loom.osc.saw").unwrap();
    c.commit().unwrap();
    let mut positions = BTreeMap::new();
    let step = record(&mut c, &mut positions, |c| {
        c.remove_module(a).unwrap();
    });
    // Something else takes the freed place.
    let filler = c.add_module("loom.osc.noise").unwrap();
    c.commit().unwrap();
    let error = check(&step, false, &c).unwrap_err();
    assert!(error.contains("no room"), "{error}");
    assert!(c.graph().node(filler).is_some() && c.graph().node(b).is_some());
}

#[test]
fn a_new_step_drops_the_redo_steps_and_the_history_keeps_200() {
    let mut c = parts().controller;
    let id = c.add_module("loom.math.mul").unwrap();
    let mut positions = BTreeMap::new();
    let mut history = History::default();
    let mut step_for = |c: &mut Controller, value: f32| {
        record(c, &mut positions, |c| {
            c.set_param_named(id, "gain", value).unwrap();
        })
    };
    let first = step_for(&mut c, 0.11);
    let second = step_for(&mut c, 0.12);
    history.push(first);
    history.push(second);
    history.undone();
    assert!(history.redo_step().is_some());
    history.push(step_for(&mut c, 0.13));
    assert!(history.redo_step().is_none(), "redo steps are dropped");
    for n in 0..LIMIT + 5 {
        history.push(step_for(&mut c, 0.2 + n as f32 * 0.001));
    }
    assert_eq!(history.len(), LIMIT);
}

#[test]
fn undo_of_a_move_leaves_a_mapped_parameter_the_hardware_moved() {
    let mut c = parts().controller;
    let id = c.add_module("loom.filter.svf").unwrap();
    let cutoff = c.graph().node(id).unwrap().descriptor.param_index("cutoff").unwrap();
    c.map_midi(
        MidiControl {
            channel: 0,
            controller: 74,
        },
        id,
        cutoff,
    )
    .unwrap();
    let mut positions = BTreeMap::from([(id.0, Point::new(0.0, 0.0))]);
    let before = snap(&c, &positions);
    positions.insert(id.0, Point::new(30.0, 0.0));
    let step = diff(&before, &snap(&c, &positions), &BTreeSet::new()).unwrap();
    assert_eq!(step.name(), "move SVF");
    // The performer turns the knob after the move.
    c.set_param(id, cutoff, 1234.0).unwrap();
    undo(&step, &mut c, &mut positions);
    assert_eq!(positions[&id.0], Point::new(0.0, 0.0));
    assert_eq!(c.graph().node(id).unwrap().params[cutoff], 1234.0);
}

#[test]
fn a_momentary_button_hold_is_no_edit_and_a_toggle_press_is() {
    let mut c = parts().controller;
    let id = c.add_module("loom.ui.button").unwrap();
    let mode = c.graph().node(id).unwrap().descriptor.param_index("mode").unwrap();
    let press = CanvasEvent::ButtonPressed {
        module: id.0,
        widget: 0,
    };
    let release = CanvasEvent::ButtonReleased {
        module: id.0,
        widget: 0,
    };
    let arrow = CanvasEvent::ButtonArrow {
        module: id.0,
        widget: 0,
        high: true,
    };
    c.set_param(id, mode, 0.0).unwrap();
    assert!(is_edit(&c, &press), "a toggle press is a step");
    assert!(is_edit(&c, &arrow), "a toggle arrow is a step");
    c.set_param(id, mode, 1.0).unwrap();
    assert!(!is_edit(&c, &press), "a momentary press is no step");
    assert!(!is_edit(&c, &release), "a momentary release is no step");
    assert!(!is_edit(&c, &arrow), "a momentary arrow is no step");
}

/// A step recorded while a button is held leaves the hold out, so undo
/// never sets the gate high again (#161).
#[test]
fn a_step_that_spans_a_hold_does_not_hold_again() {
    let mut c = parts().controller;
    let id = c.add_module("loom.ui.button").unwrap();
    let d = c.graph().node(id).unwrap().descriptor;
    let (mode, hold) = (d.param_index("mode").unwrap(), d.param_index("hold").unwrap());
    let mut positions = BTreeMap::new();
    let before = snap(&c, &positions);
    c.set_param(id, hold, 1.0).unwrap();
    c.commit().unwrap();
    assert!(
        diff(&before, &snap(&c, &positions), &BTreeSet::new()).is_none(),
        "a hold is no step"
    );
    let step = record(&mut c, &mut positions, |c| {
        c.set_param(id, mode, 1.0).unwrap();
        c.set_param(id, hold, 0.0).unwrap();
    });
    c.set_param(id, hold, 1.0).unwrap();
    undo(&step, &mut c, &mut positions);
    let params = &c.graph().node(id).unwrap().params;
    assert_eq!(params[mode], 0.0, "undo puts the mode back");
    assert_eq!(params[hold], 1.0, "undo leaves the live hold alone");
}

#[test]
fn moving_a_mapping_to_another_parameter_is_named_a_move() {
    let mut c = parts().controller;
    let id = c.add_module("loom.filter.svf").unwrap();
    let control = MidiControl {
        channel: 0,
        controller: 74,
    };
    c.map_midi(control, id, 0).unwrap();
    let mut positions = BTreeMap::new();
    let step = record(&mut c, &mut positions, |c| {
        c.map_midi(control, id, 1).unwrap();
    });
    assert_eq!(step.name(), "move MIDI mapping");
}

#[test]
fn the_saved_mark_follows_the_history_position() {
    let mut c = parts().controller;
    let id = c.add_module("loom.math.mul").unwrap();
    let mut positions = BTreeMap::new();
    let mut step_for = |c: &mut Controller, value: f32| {
        record(c, &mut positions, |c| {
            c.set_param_named(id, "gain", value).unwrap();
        })
    };
    let mut history = History::default();
    history.mark_saved();
    assert!(history.is_saved(), "a patch just loaded is saved");
    history.push(step_for(&mut c, 0.3));
    assert!(!history.is_saved(), "an edit marks it unsaved");
    history.undone();
    assert!(history.is_saved(), "undo back to the saved state clears the mark");
    history.redone();
    history.mark_saved();
    history.undone();
    assert!(!history.is_saved(), "undo past the saved state marks it again");
    history.push(step_for(&mut c, 0.4));
    history.mark_saved();
    history.undone();
    history.push(step_for(&mut c, 0.5));
    history.undone();
    assert!(!history.is_saved(), "a new step drops the saved state from redo");
    history.clear();
    assert!(!history.is_saved(), "a cleared history cannot vouch for the file");
}

#[test]
fn a_trimmed_history_loses_the_saved_state() {
    let mut c = parts().controller;
    let id = c.add_module("loom.math.mul").unwrap();
    let mut positions = BTreeMap::new();
    let mut history = History::default();
    history.mark_saved();
    for n in 0..LIMIT + 1 {
        let step = record(&mut c, &mut positions, |c| {
            c.set_param_named(id, "gain", 0.1 + n as f32 * 0.001).unwrap();
        });
        history.push(step);
    }
    for _ in 0..LIMIT {
        history.undone();
    }
    assert!(!history.is_saved(), "the saved state fell off the start");
}
