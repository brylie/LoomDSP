use super::*;
use loom_core::{EngineConfig, EngineParts, create_engine};

fn parts() -> EngineParts {
    create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0))
}

fn snap(c: &Controller, positions: &BTreeMap<u32, (f32, f32)>) -> Snapshot {
    Snapshot::take(c, positions)
}

/// Runs `change`, and returns the step it made.
fn record(c: &mut Controller, positions: &mut BTreeMap<u32, (f32, f32)>, change: impl FnOnce(&mut Controller)) -> Step {
    let before = snap(c, positions);
    change(c);
    c.commit().unwrap();
    diff(&before, &snap(c, positions), &BTreeSet::new()).expect("the edit changed the patch")
}

fn undo(step: &Step, c: &mut Controller, positions: &mut BTreeMap<u32, (f32, f32)>) {
    check(step, false, c, &Hierarchy::flat()).unwrap();
    apply(step, false, c, positions).unwrap();
    c.commit().unwrap();
}

fn redo(step: &Step, c: &mut Controller, positions: &mut BTreeMap<u32, (f32, f32)>) {
    check(step, true, c, &Hierarchy::flat()).unwrap();
    apply(step, true, c, positions).unwrap();
    c.commit().unwrap();
}

#[test]
fn undo_removes_an_added_module_and_redo_brings_it_back_with_its_id_and_place() {
    let mut c = parts().controller;
    let mut positions = BTreeMap::new();
    let before = snap(&c, &positions);
    let id = c.add_module("loom.osc.sine").unwrap();
    positions.insert(id.0, (40.0, 80.0));
    c.commit().unwrap();
    let step = diff(&before, &snap(&c, &positions), &BTreeSet::new()).unwrap();
    assert_eq!(step.name(), "add Sine");
    undo(&step, &mut c, &mut positions);
    assert!(c.graph().node(id).is_none());
    assert!(!positions.contains_key(&id.0));
    redo(&step, &mut c, &mut positions);
    assert!(c.graph().node(id).is_some());
    assert_eq!(positions[&id.0], (40.0, 80.0));
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
        let mut positions: BTreeMap<u32, (f32, f32)> = edited
            .controller
            .graph()
            .nodes()
            .map(|(id, _)| (id.0, (id.0 as f32 * 10.0, 0.0)))
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
    let error = check(&step, false, &c, &Hierarchy::flat()).unwrap_err();
    assert_eq!(
        error,
        "the patch would have more than 2 modules. Delete modules to make room."
    );
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
    let mut positions = BTreeMap::from([(id.0, (0.0, 0.0))]);
    let before = snap(&c, &positions);
    positions.insert(id.0, (30.0, 0.0));
    let step = diff(&before, &snap(&c, &positions), &BTreeSet::new()).unwrap();
    assert_eq!(step.name(), "move SVF");
    // The performer turns the knob after the move.
    c.set_param(id, cutoff, 1234.0).unwrap();
    undo(&step, &mut c, &mut positions);
    assert_eq!(positions[&id.0], (0.0, 0.0));
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

/// Options that differ from the default on every field Absolute and Pickup
/// have.
fn pickup_options() -> loom_core::MidiMappingOptions {
    loom_core::MidiMappingOptions {
        mode: loom_core::MidiMode::Pickup,
        low: 0.25,
        high: 0.75,
        invert: true,
    }
}

/// The options on a parameter's mapping, if it has one.
fn options_of(c: &Controller, id: ModuleId, param: usize) -> Option<loom_core::MidiMappingOptions> {
    c.graph().midi_mapping_of(id, param).map(|(_, m)| m.options)
}

/// The controller on a new SVF with a mapping on its first parameter.
fn mapped_svf(options: loom_core::MidiMappingOptions) -> (Controller, ModuleId, MidiControl) {
    let mut c = parts().controller;
    let id = c.add_module("loom.filter.svf").unwrap();
    let control = MidiControl {
        channel: 0,
        controller: 74,
    };
    c.map_midi_with(control, id, 0, options).unwrap();
    c.commit().unwrap();
    (c, id, control)
}

/// Changing a mapping's options is one step of its own, and undo and redo
/// bring back the exact options (undo.md).
#[test]
fn a_change_of_midi_options_is_one_step() {
    let (mut c, id, _) = mapped_svf(loom_core::MidiMappingOptions::DEFAULT);
    let mut positions = BTreeMap::new();
    let step = record(&mut c, &mut positions, |c| {
        c.set_midi_options(id, 0, pickup_options()).unwrap();
    });
    assert_eq!(step.name(), "change MIDI mapping");
    undo(&step, &mut c, &mut positions);
    assert_eq!(options_of(&c, id, 0), Some(loom_core::MidiMappingOptions::DEFAULT));
    redo(&step, &mut c, &mut positions);
    assert_eq!(options_of(&c, id, 0), Some(pickup_options()));
}

/// Every step that brings a mapping back brings its options too: the ×, a
/// deleted module, learning over a mapping, and moving a control.
#[test]
fn undo_keeps_midi_options() {
    let mut positions = BTreeMap::new();

    // Remove with the ×.
    let (mut c, id, _) = mapped_svf(pickup_options());
    let step = record(&mut c, &mut positions, |c| {
        c.unmap_midi(id, 0).unwrap();
    });
    undo(&step, &mut c, &mut positions);
    assert_eq!(options_of(&c, id, 0), Some(pickup_options()));

    // Delete the module.
    let (mut c, id, _) = mapped_svf(pickup_options());
    let step = record(&mut c, &mut positions, |c| {
        c.remove_module(id).unwrap();
    });
    undo(&step, &mut c, &mut positions);
    assert_eq!(options_of(&c, id, 0), Some(pickup_options()));

    // A new control learned over the mapping starts at the defaults, and
    // undo brings back the old options.
    let (mut c, id, control) = mapped_svf(pickup_options());
    let other = MidiControl {
        controller: 75,
        ..control
    };
    let step = record(&mut c, &mut positions, |c| {
        c.map_midi(other, id, 0).unwrap();
    });
    undo(&step, &mut c, &mut positions);
    assert_eq!(options_of(&c, id, 0), Some(pickup_options()));

    // Moving a control to another parameter, and back with undo.
    let (mut c, id, control) = mapped_svf(pickup_options());
    let step = record(&mut c, &mut positions, |c| {
        c.map_midi_with(control, id, 1, pickup_options()).unwrap();
    });
    assert_eq!(step.name(), "move MIDI mapping");
    undo(&step, &mut c, &mut positions);
    assert_eq!(options_of(&c, id, 0), Some(pickup_options()));
    assert_eq!(options_of(&c, id, 1), None);
    redo(&step, &mut c, &mut positions);
    assert_eq!(options_of(&c, id, 1), Some(pickup_options()));
}

/// Undo of a delete brings back a known module's label (clipboard.md,
/// rule 9).
#[test]
fn undo_of_a_delete_brings_back_a_known_module_label() {
    let mut c = parts().controller;
    let mut positions = BTreeMap::new();
    let id = c.add_module("loom.osc.sine").unwrap();
    c.set_label(id, Some("Lead".into()));
    c.commit().unwrap();
    let step = record(&mut c, &mut positions, |c| {
        c.remove_module(id).unwrap();
    });
    undo(&step, &mut c, &mut positions);
    assert_eq!(c.graph().node(id).unwrap().label.as_deref(), Some("Lead"));
}

/// A change to only a label is one step, named for the module. A
/// placeholder keeps the name it had before labels moved to every module.
#[test]
fn a_label_change_is_a_step_named_for_the_module() {
    let mut c = parts().controller;
    let mut positions = BTreeMap::new();
    let sine = c.add_module("loom.osc.sine").unwrap();
    let missing = c
        .add_unknown(loom_core::UnknownModule {
            kind: "future.effect@1".into(),
            params: BTreeMap::new(),
            ranges: BTreeMap::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            parameters: Vec::new(),
        })
        .unwrap();
    c.commit().unwrap();

    let step = record(&mut c, &mut positions, |c| {
        c.set_label(missing, Some("Shimmer".into()));
    });
    assert_eq!(step.name(), "label unknown module");
    undo(&step, &mut c, &mut positions);
    assert_eq!(c.graph().node(missing).unwrap().label, None);
    redo(&step, &mut c, &mut positions);
    assert_eq!(c.graph().node(missing).unwrap().label.as_deref(), Some("Shimmer"));

    let step = record(&mut c, &mut positions, |c| {
        c.set_label(sine, Some("Lead".into()));
    });
    assert_eq!(step.name(), "label Sine");
}

/// Undo and redo of a depth change, of turning an assignment off, and of a
/// polarity flip blend the setting over 5 ms, as the change itself does
/// (live patching spec, rule 9). They reach the engine as a removal, a
/// restore with the same id, and a commit, so the engine compares what
/// plays with what arrives. A sine on the right side shows the source
/// sample the engine read; an Add on the left shows the contribution
/// through its offset. Checked frame by frame against
/// `old(x) · (1 − t) + new(x) · t`, at block sizes 1, 77, and 512.
#[test]
fn undo_of_a_depth_change_fades() {
    use loom_core::Polarity;
    let length = 240;
    let (change_at, undo_at, redo_at, frames) = (1_000, 1_600, 2_200, 2_800);
    // Each change, and what the setting makes of `x` before and after it.
    type Change = (&'static str, fn(&mut Controller, AssignmentId), fn(f32) -> f32);
    let changes: [Change; 3] = [
        (
            "depth",
            |c, id| {
                c.set_assignment_depth(id, -0.2).unwrap();
            },
            |x| -0.2 * x,
        ),
        ("off", |c, id| c.set_assignment_enabled(id, false).unwrap(), |_| 0.0),
        (
            "polarity",
            |c, id| c.set_assignment_polarity(id, Some(Polarity::Unipolar)).unwrap(),
            |x| 0.4 * (x + 1.0) / 2.0,
        ),
    ];
    let before = |x: f32| 0.4 * x;
    for (name, change, after) in changes {
        for block in [1, 77, 512] {
            let config = EngineConfig {
                output_safety: loom_core::OutputSafety::Off,
                ..EngineConfig::with_sample_rate(48_000.0)
            };
            let mut parts = create_engine(loom_modules::registry(), config);
            let c = &mut parts.controller;
            let phase = c.add_module("loom.phase.generator").unwrap();
            c.set_param_named(phase, "tune", -4.65).unwrap();
            let sine = c.add_module("loom.osc.sine").unwrap();
            c.connect_named(phase, "phase", sine, "phase").unwrap();
            let seen = c.add_module("loom.math.add").unwrap();
            let join = c.add_module("loom.mix.join").unwrap();
            let out = c.add_module("loom.io.audio-out").unwrap();
            c.connect_named(seen, "out", join, "left").unwrap();
            c.connect_named(sine, "out", join, "right").unwrap();
            c.connect_named(join, "out", out, "in").unwrap();
            let id = c.add_assignment_named(sine, "out", seen, "offset", 0.4).unwrap();
            c.commit().unwrap();
            let param = c.graph().node(seen).unwrap().descriptor.param_index("offset").unwrap();
            let spec = c.graph().node(seen).unwrap().spec(param).unwrap();
            let mut positions = BTreeMap::new();

            let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
            let mut play = |parts: &mut EngineParts, from: usize, to: usize| {
                let mut at = from;
                while at < to {
                    let n = block.min(to - at);
                    parts.engine.process(&mut left[at..at + n], &mut right[at..at + n]);
                    at += n;
                }
            };
            play(&mut parts, 0, change_at);
            let step = record(&mut parts.controller, &mut positions, |c| change(c, id));
            play(&mut parts, change_at, undo_at);
            undo(&step, &mut parts.controller, &mut positions);
            assert_eq!(parts.controller.graph().assignment(id).unwrap().depth, 0.4);
            play(&mut parts, undo_at, redo_at);
            redo(&step, &mut parts.controller, &mut positions);
            play(&mut parts, redo_at, frames);

            let blend = |k: usize, start: usize, old: f32, new: f32| {
                let t = ((k - start) as f32 / length as f32).min(1.0);
                old * (1.0 - t) + new * t
            };
            assert!(right[change_at..].iter().any(|x| x.abs() > 0.9), "the source moves");
            for k in change_at - 10..frames {
                let x = right[k];
                let contribution = if k < change_at {
                    before(x)
                } else if k < undo_at {
                    blend(k, change_at, before(x), after(x))
                } else if k < redo_at {
                    blend(k, undo_at, after(x), before(x))
                } else {
                    blend(k, redo_at, before(x), after(x))
                };
                let expected = spec.denormalize(spec.normalize(0.0) + contribution);
                assert!(
                    (left[k] - expected).abs() <= 1e-6,
                    "{name}, frame {k}, block {block}: {} != {expected}",
                    left[k]
                );
            }
        }
    }
}

/// The app with the patch `text` open and an engine with exactly `free`
/// slots left after the load (sub-patches.md, The module limit).
fn app_with_room(text: &str, free: usize) -> crate::App {
    let open = |max_modules: usize| {
        let config = EngineConfig {
            max_modules,
            ..EngineConfig::default()
        };
        let patch = loom_patch::Patch::from_ron(text).unwrap();
        let loaded = crate::Loaded::read(
            config,
            false,
            crate::Source::Recovered(Box::new(patch)),
            &loom_patch::NoLibrary,
        )
        .unwrap();
        crate::App::assemble(
            Err("test".to_string()),
            48_000.0,
            loaded,
            1,
            &crate::tests::app_args(),
            crate::settings::Rig::empty(),
            false,
        )
    };
    let max = EngineConfig::default().max_modules;
    let used = max - open(max).session.controller.free_module_slots();
    let app = open(used + free);
    assert_eq!(app.session.controller.free_module_slots(), free, "the room asked for");
    app
}

/// Leaves exactly `free` engine slots, taking the rest with modules outside
/// the history, once the slots of removed modules are free again.
fn leave_room(app: &mut crate::App, free: usize) {
    let mut fillers = Vec::new();
    loop {
        while app.session.controller.free_module_slots() > 0 {
            fillers.push(app.session.controller.add_module("loom.osc.noise").unwrap());
        }
        assert!(app.commit(), "{}", app.alerts.status());
        app.process(64);
        if app.session.controller.free_module_slots() == 0 {
            break;
        }
    }
    for id in fillers.into_iter().take(free) {
        app.session.controller.remove_module(id).unwrap();
    }
    assert!(app.commit());
    app.process(64);
    assert!(app.commit());
    assert_eq!(app.session.controller.free_module_slots(), free);
}

/// The patch as undo sees it in `app`.
fn snapshot_of(app: &crate::App) -> Snapshot {
    Snapshot::take_full(
        &app.session.controller,
        &app.session.hierarchy,
        &app.session.level,
        &app.session.positions,
        &app.session.sizes,
    )
}

/// Makes the history the one step from `before` to the patch now, as one
/// edit that did all of it would record.
fn one_step_since(app: &mut crate::App, before: &Snapshot) {
    let step = diff(before, &snapshot_of(app), &BTreeSet::new()).expect("the edits changed the patch");
    app.session.edits.history.clear();
    app.session.edits.history.push(step);
}

/// The graph's modules and cables, to compare before and after a refusal.
fn graph_of(app: &crate::App) -> (Vec<ModuleId>, Vec<(OutputRef, InputRef)>) {
    let graph = app.session.controller.graph();
    (graph.nodes().map(|(id, _)| id).collect(), graph.cables().collect())
}

/// How many modules of `kind` the graph holds.
fn count_of(app: &crate::App, kind: &str) -> usize {
    app.session
        .controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == kind)
        .count()
}

/// Swaps the slot tile `slot` to `to` as the player does.
fn swap(app: &mut crate::App, slot: u32, to: crate::slots::SwapTarget) {
    let _ = app.send(crate::Message::SwapSlot { slot, to });
    app.process(600);
}

/// A sine into an empty Filter slot into an Amplifier, a Noise beside, and
/// the output fed by the Amplifier.
const ONE_SLOT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "One slot"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 4, kind: "loom.clock.transport@1"),
        PatchModule(id: 5, kind: "loom.osc.noise@1"),
        PatchModule(id: 6, kind: "loom.level.amp@1"),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1"))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (6, "in")),
        PatchCable(from: (6, "out"), to: (3, "in")),
    ],
)"#;

/// A step built by hand that fills a slot and deletes a module: its undo
/// empties the slot and puts the module back. With room for the module
/// but not also the slot's fade, the removal yields: the module comes
/// back, the slot falls back to the 5 ms ramp, and the history stays
/// (sub-patches.md, The module limit).
#[test]
fn an_undo_that_empties_a_slot_leaves_room_for_the_rest_of_its_step() {
    let mut app = app_with_room(ONE_SLOT, 8);
    let before = snapshot_of(&app);
    swap(
        &mut app,
        9,
        crate::slots::SwapTarget::Module("loom.filter.svf@1".into()),
    );
    let _ = app.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Delete(vec![5])));
    one_step_since(&mut app, &before);
    leave_room(&mut app, 1);
    app.undo();
    assert!(app.alerts.status().starts_with("Undo: "), "{}", app.alerts.status());
    assert!(
        app.session.controller.graph().node(ModuleId(5)).is_some(),
        "the Noise is back"
    );
    assert_eq!(app.session.hierarchy.part_of_slot(ModuleId(9)), None);
    assert_eq!(count_of(&app, "loom.struct.fade"), 0, "the slot ramps over 5 ms");
    assert_eq!(count_of(&app, "loom.filter.svf"), 0);
    assert!(app.redo_hint().is_ok(), "the history stays");
}

/// An undo step whose slot is gone, as after a dissolve, is refused before
/// anything changes, with the reason, and nothing panics (sub-patches.md,
/// The module limit).
#[test]
fn a_step_whose_slot_is_gone_is_refused() {
    let mut app = app_with_room(ONE_SLOT, 8);
    swap(
        &mut app,
        9,
        crate::slots::SwapTarget::Module("loom.filter.svf@1".into()),
    );
    let step = app.session.edits.history.undo_step().cloned().expect("the swap's step");
    app.dissolve_slot(9);
    assert!(app.session.hierarchy.slot(ModuleId(9)).is_none(), "dissolved");
    let refused = check(&step, false, &app.session.controller, &app.session.hierarchy);
    assert_eq!(refused, Err(loom_patch::hierarchy::SLOT_GONE_TEXT.to_string()));
    app.session.edits.history.push(step);
    let before = graph_of(&app);
    app.undo();
    assert_eq!(
        app.alerts.status(),
        "Cannot undo swap Filter: the slot it changes is gone"
    );
    assert_eq!(graph_of(&app), before);
}

/// A step that fills a slot at exactly the limit passes `check`, because
/// the part the swap adds is not counted again with the rest of the step,
/// and the undo goes through (sub-patches.md, The module limit).
#[test]
fn a_swapped_part_is_counted_once() {
    let mut app = app_with_room(ONE_SLOT, 8);
    swap(
        &mut app,
        9,
        crate::slots::SwapTarget::Module("loom.filter.svf@1".into()),
    );
    swap(&mut app, 9, crate::slots::SwapTarget::Empty);
    // Room for the part and its fade, and no more.
    leave_room(&mut app, 2);
    let step = app.session.edits.history.undo_step().cloned().unwrap();
    assert_eq!(
        check(&step, false, &app.session.controller, &app.session.hierarchy),
        Ok(0)
    );
    app.undo();
    assert_eq!(app.alerts.status(), "Undo: remove part from Filter");
    assert_eq!(app.session.hierarchy.fading_slots().len(), 1, "it crossfades");
}

/// [`ONE_SLOT`] with a stereo signal from a Join into an SVF of its own,
/// which runs once per side and feeds nothing, so no other module's width
/// follows it.
fn one_slot_and_stereo() -> String {
    ONE_SLOT
        .replace(
            r#"PatchModule(id: 6, kind: "loom.level.amp@1"),"#,
            r#"PatchModule(id: 6, kind: "loom.level.amp@1"),
        PatchModule(id: 7, kind: "loom.mix.join@1"),
        PatchModule(id: 8, kind: "loom.filter.svf@1"),"#,
        )
        .replace(
            r#"PatchCable(from: (6, "out"), to: (3, "in")),"#,
            r#"PatchCable(from: (6, "out"), to: (3, "in")),
        PatchCable(from: (1, "out"), to: (7, "left")),
        PatchCable(from: (7, "out"), to: (8, "in")),"#,
        )
}

/// A step that empties a slot and puts back an SVF that ran stereo, at a
/// limit where the SVF's two slots fit but not also the slot's fade: the
/// SVF comes back with both sides, the slot falls back to the 5 ms ramp,
/// and the commit does not fail (sub-patches.md, The module limit).
#[test]
fn an_undo_that_restores_a_stereo_module_counts_its_copies() {
    let mut app = app_with_room(&one_slot_and_stereo(), 8);
    assert_eq!(app.session.controller.slots_of(ModuleId(8)), 2, "the SVF runs stereo");
    let before = snapshot_of(&app);
    swap(
        &mut app,
        9,
        crate::slots::SwapTarget::Module("loom.filter.svf@1".into()),
    );
    let _ = app.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Delete(vec![8])));
    one_step_since(&mut app, &before);
    leave_room(&mut app, 2);
    app.undo();
    assert!(app.alerts.status().starts_with("Undo: "), "{}", app.alerts.status());
    assert_eq!(app.session.controller.slots_of(ModuleId(8)), 2, "back with both sides");
    assert_eq!(app.session.hierarchy.part_of_slot(ModuleId(9)), None);
    assert_eq!(count_of(&app, "loom.struct.fade"), 0, "the slot ramps over 5 ms");
}

/// A Voice stacked four times on a stereo signal, whose Filter slot holds
/// an SVF, and a second, empty Filter slot after it.
const STEREO_STACK: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Stereo stack"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            mode: Stack,
            copies: 4,
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 3, kind: "loom.filter.svf@1"),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(3)))),
                PatchModule(id: 10, kind: "loom.struct.slot@1", label: Some("Tone"), slot: Some(Slot(role: "loom.role.filter@1"))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (9, "in")),
                PatchCable(from: (9, "out"), to: (10, "in")),
                PatchCable(from: (10, "out"), to: (4, "in")),
            ],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice")),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 5, kind: "loom.mix.join@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (5, "left")),
        PatchCable(from: (1, "out"), to: (5, "right")),
        PatchCable(from: (5, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// The flat id of the tile `lid` inside the Voice tile.
fn in_voice(app: &crate::App, lid: u32) -> u32 {
    app.session
        .hierarchy
        .id_at(&ModulePath::top(2), lid)
        .expect("a tile inside Voice")
        .0
}

/// Undo of Remove part inside a stereo stack, at a limit that `check`
/// passes (the part is the swap's) and that the parts and fades fit by
/// count, but not their right sides: the measure after the sync refuses it,
/// the swaps are taken back, the graph's modules and cables are as before,
/// the status says why, and the history stays (sub-patches.md, The module
/// limit).
#[test]
fn an_undo_whose_swap_does_not_fit_is_taken_back() {
    let mut app = app_with_room(STEREO_STACK, 16);
    let slot = in_voice(&app, 9);
    swap(&mut app, slot, crate::slots::SwapTarget::Empty);
    leave_room(&mut app, 8);
    let step = app.session.edits.history.undo_step().cloned().unwrap();
    assert_eq!(
        check(&step, false, &app.session.controller, &app.session.hierarchy),
        Ok(0)
    );
    let before = graph_of(&app);
    app.undo();
    let limit = app.session.controller.config().max_modules;
    assert_eq!(
        app.alerts.status(),
        format!(
            "Cannot undo remove part from Filter: the patch would have more than {limit} modules. Delete modules or lower Copies on Voice to make room."
        )
    );
    assert_eq!(graph_of(&app), before, "taken back whole");
    assert_eq!(app.session.controller.free_module_slots(), 8);
    assert_eq!(app.undo_hint(), Ok("Undo: remove part from Filter".to_string()));
}

/// A step built by hand that, undone, empties a slot inside a stack and
/// fills another: with no room for the fill, it is refused before the
/// removal runs, so the emptied slot's running copies are never retired
/// and made again from reset. Every module keeps its id, and no slot is
/// retired (sub-patches.md, The module limit).
#[test]
fn a_refused_fill_leaves_the_slot_a_step_empties_untouched() {
    let text = STEREO_STACK.replace(", part: Some(3)", "").replace(
        r#"label: Some("Tone"), slot: Some(Slot(role: "loom.role.filter@1")))"#,
        r#"label: Some("Tone"), slot: Some(Slot(role: "loom.role.filter@1", part: Some(3))))"#,
    );
    // Now the first slot is empty and the second, Tone, holds the SVF.
    let mut app = app_with_room(&text, 24);
    let (first, tone) = (in_voice(&app, 9), in_voice(&app, 10));
    let before = snapshot_of(&app);
    swap(
        &mut app,
        first,
        crate::slots::SwapTarget::Module("loom.filter.svf@1".into()),
    );
    swap(&mut app, tone, crate::slots::SwapTarget::Empty);
    one_step_since(&mut app, &before);
    leave_room(&mut app, 2);
    let running = graph_of(&app);
    app.undo();
    assert!(
        app.alerts.status().starts_with("Cannot undo"),
        "{}",
        app.alerts.status()
    );
    assert!(app.alerts.status().ends_with("lower Copies on Voice to make room."));
    assert_eq!(graph_of(&app), running, "every module keeps its id and cables");
    assert_eq!(app.session.controller.free_module_slots(), 2, "nothing was retired");
    assert!(app.session.hierarchy.part_of_slot(ModuleId(first)).is_some());
    assert!(app.undo_hint().is_ok(), "the history stays");
}

/// A sine into the left of a Join, the Join into an SVF, the SVF into the
/// output.
const STEREO_INTO_SVF: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Stereo into SVF"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.mix.join@1"),
        PatchModule(id: 3, kind: "loom.filter.svf@1"),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "left")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "lowpass"), to: (4, "in")),
    ],
)"#;

/// Undo of deleting a Join, at a limit with room for the Join alone: the
/// Join's recorded width passes `check`, but the cable it puts back makes
/// the kept SVF stereo, which needs a slot more. The measure after the rest
/// of the step refuses it, everything is taken back, and the history
/// stays (sub-patches.md, The module limit).
#[test]
fn an_undo_that_widens_a_kept_module_at_the_limit() {
    let mut app = app_with_room(STEREO_INTO_SVF, 4);
    assert_eq!(app.session.controller.slots_of(ModuleId(3)), 2, "the SVF runs stereo");
    let _ = app.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Delete(vec![2])));
    let name = app.undo_hint().unwrap();
    assert_eq!(app.session.controller.slots_of(ModuleId(3)), 1, "mono without the Join");
    leave_room(&mut app, 1);
    let before = graph_of(&app);
    app.undo();
    let limit = app.session.controller.config().max_modules;
    assert_eq!(
        app.alerts.status(),
        format!(
            "Cannot {}: the patch would have more than {limit} modules. Delete modules to make room.",
            name.replacen("Undo: ", "undo ", 1)
        )
    );
    assert_eq!(graph_of(&app), before, "taken back whole");
    assert_eq!(app.session.controller.free_module_slots(), 1);
    assert_eq!(app.undo_hint(), Ok(name), "the history stays");
}

/// When a recorded width is stale, the slot a step empties still yields its
/// fade before the step is refused: putting back an SVF that ran stereo
/// also makes the Amplifier after it stereo again, a slot `check` did not
/// count. With room for both but not the fade too, the undo goes through
/// with the 5 ms ramp (sub-patches.md, The module limit).
#[test]
fn a_removal_yields_its_fade_when_a_width_was_stale() {
    let text = one_slot_and_stereo()
        .replace(
            r#"PatchModule(id: 8, kind: "loom.filter.svf@1"),"#,
            r#"PatchModule(id: 8, kind: "loom.filter.svf@1"),
        PatchModule(id: 10, kind: "loom.level.amp@1"),"#,
        )
        .replace(
            r#"PatchCable(from: (7, "out"), to: (8, "in")),"#,
            r#"PatchCable(from: (7, "out"), to: (8, "in")),
        PatchCable(from: (8, "lowpass"), to: (10, "in")),"#,
        );
    let mut app = app_with_room(&text, 8);
    assert_eq!(
        app.session.controller.slots_of(ModuleId(10)),
        2,
        "the Amplifier runs stereo"
    );
    let before = snapshot_of(&app);
    swap(
        &mut app,
        9,
        crate::slots::SwapTarget::Module("loom.filter.svf@1".into()),
    );
    let _ = app.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Delete(vec![8])));
    one_step_since(&mut app, &before);
    // The SVF's two slots and the Amplifier's right side, not the fade.
    leave_room(&mut app, 3);
    app.undo();
    assert!(app.alerts.status().starts_with("Undo: "), "{}", app.alerts.status());
    assert_eq!(app.session.controller.slots_of(ModuleId(8)), 2);
    assert_eq!(app.session.controller.slots_of(ModuleId(10)), 2);
    assert_eq!(count_of(&app, "loom.struct.fade"), 0, "the slot ramps over 5 ms");
}
