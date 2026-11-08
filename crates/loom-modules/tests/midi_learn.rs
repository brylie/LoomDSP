//! Engine acceptance tests for MIDI learn. Spec: `docs/design/midi-learn.md`.

use loom_core::{
    Channels, ControlChangeSeen, EngineConfig, EngineParts, MidiControl, MidiEncoding, MidiEvent, MidiMappingOptions,
    MidiMessage, MidiMode, Module, ModuleConfig, ModuleDescriptor, ModuleId, ModuleRole, Panel, ParamOptions,
    ParamRange, ParamSpec, PortDefault, PortIo, PortSpec, ProbeKind, ProcessContext, SignalType, create_engine,
};
use loom_modules::register_all;
use std::sync::Arc;

/// Writes its first parameter on `out` (the modulated value when it has an
/// assignment), and the number of control changes it has seen on `ccs`.
struct Probe {
    values: [f32; 2],
    ccs: f32,
}

impl Module for Probe {
    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for (_, event) in ctx.midi() {
            if matches!(event.message(), MidiMessage::ControlChange { .. }) {
                self.ccs += 1.0;
            }
        }
        let modulated = io.modulated(0);
        let ([], [out, ccs]) = io.split::<0, 2>();
        for (i, sample) in out.iter_mut().enumerate() {
            *sample = modulated.map_or(self.values[0], |values| values[i]);
        }
        ccs.fill(self.ccs);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        self.values[index] = value;
    }
}

fn probe(_config: &ModuleConfig) -> Box<dyn Module> {
    Box::new(Probe {
        values: [0.0; 2],
        ccs: 0.0,
    })
}

const fn output(name: &'static str) -> PortSpec {
    PortSpec {
        name,
        label: name,
        signal: SignalType::Modulation,
        description: "A test output.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Span(0.0, 1.0),
        channels: Channels::PerChannel,
    }
}

static PROBE: ModuleDescriptor = ModuleDescriptor {
    id: "test.probe",
    version: 1,
    name: "test.probe",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Writes its parameter and counts control changes.",
    inputs: &[],
    outputs: &[output("out"), output("ccs")],
    params: &[
        ParamSpec {
            name: "level",
            label: "Level",
            description: "A test parameter.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
        ParamSpec {
            name: "other",
            label: "Other",
            description: "Another test parameter.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(probe),
    panel: Panel::EMPTY,
    displays: &[],
};

const CC74: MidiControl = MidiControl {
    channel: 0,
    controller: 74,
};

const CC71: MidiControl = MidiControl {
    channel: 0,
    controller: 71,
};

fn engine() -> EngineParts {
    let mut registry = loom_core::Registry::new();
    register_all(&mut registry).expect("built-ins");
    registry.register(&PROBE).expect("probe");
    create_engine(Arc::new(registry), EngineConfig::with_sample_rate(48_000.0))
}

/// Adds a probe module with scope probes on both outputs.
fn add_probe(parts: &mut EngineParts) -> ModuleId {
    let id = parts.controller.add_module("test.probe").unwrap();
    for port in ["out", "ccs"] {
        let output = parts.controller.output_ref(id, port).unwrap();
        parts.controller.add_probe(output, ProbeKind::Scope).unwrap();
    }
    parts.controller.commit().unwrap();
    id
}

fn send_cc(parts: &mut EngineParts, control: MidiControl, value: u8) {
    assert!(
        parts
            .midi
            .send(MidiEvent::control_change(0, control.channel, control.controller, value))
    );
}

/// Runs one buffer and returns the last `out` and `ccs` values.
fn step(parts: &mut EngineParts) -> (f32, f32) {
    let mut left = [0.0; 64];
    let mut right = [0.0; 64];
    parts.engine.process(&mut left, &mut right);
    let chunks: Vec<_> = parts.telemetry.scope().collect();
    let last = |probe: usize| {
        chunks
            .iter()
            .filter(|chunk| chunk.probe.0 as usize == probe)
            .flat_map(|chunk| chunk.samples().to_vec())
            .last()
            .unwrap()
    };
    // Probes are numbered from 1 in the order they were added.
    (last(1), last(2))
}

#[test]
fn a_mapped_control_change_sets_the_base_value() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();

    send_cc(&mut parts, CC74, 127);
    assert_eq!(step(&mut parts).0, 1.0);
    send_cc(&mut parts, CC74, 0);
    assert_eq!(step(&mut parts).0, 0.0);
    send_cc(&mut parts, CC74, 64);
    let expected = 64.0 / 127.0;
    assert_eq!(step(&mut parts).0, expected);

    // The graph follows, so the UI shows and saves what plays.
    assert_eq!(parts.controller.sync_midi(), vec![(id, 0)]);
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], expected);
    assert!(parts.controller.sync_midi().is_empty(), "nothing new");
}

#[test]
fn other_channels_and_controllers_do_not_move_it() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();

    send_cc(&mut parts, MidiControl { channel: 1, ..CC74 }, 127);
    send_cc(&mut parts, CC71, 127);
    assert_eq!(step(&mut parts).0, 0.0);
    assert!(parts.controller.sync_midi().is_empty());
}

#[test]
fn values_follow_the_parameter_mapping() {
    let registry = loom_modules::registry();
    let svf = registry.resolve("loom.filter.svf").unwrap();
    let cutoff = &svf.params[svf.param_index("cutoff").unwrap()];
    let control_value = |value: u8| cutoff.denormalize(f32::from(value) / 127.0);
    assert_eq!(control_value(0), cutoff.min);
    assert_eq!(control_value(127), cutoff.max);
    // The middle of a logarithmic range is the geometric mean, within one
    // step of 127.
    let middle = (cutoff.min * cutoff.max).sqrt();
    let below = control_value(63);
    let above = control_value(64);
    assert!(below < middle && middle < above, "{below} < {middle} < {above}");
}

#[test]
fn a_parameter_and_a_control_each_have_one_mapping() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    let controller = &mut parts.controller;

    // Learning a mapped parameter again replaces its mapping.
    controller.map_midi(CC74, id, 0).unwrap();
    let (_, removed) = controller.map_midi(CC71, id, 0).unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].control, CC74);
    assert_eq!(controller.graph().midi_mapping_of(id, 0).unwrap().1.control, CC71);

    // Learning a mapped control moves it, and names the parameter it left.
    let (_, removed) = controller.map_midi(CC71, id, 1).unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!((removed[0].target, removed[0].param), (id, 0));
    assert!(controller.graph().midi_mapping_of(id, 0).is_none());
    assert_eq!(controller.graph().midi_mappings().count(), 1);
    controller.commit().unwrap();

    send_cc(&mut parts, CC71, 127);
    assert_eq!(step(&mut parts).0, 0.0, "the old parameter no longer follows");
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 0.0);
    assert_eq!(parts.controller.sync_midi(), vec![(id, 1)]);
}

#[test]
fn learning_uses_the_last_control_change_and_sets_the_value() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    assert_eq!(parts.controller.last_control_change(), None);

    send_cc(&mut parts, CC74, 100);
    step(&mut parts);
    let seen = parts.controller.last_control_change().unwrap();
    assert_eq!(seen.control, CC74);
    assert_eq!(seen.value, 100);

    parts.controller.learn_midi(seen, id, 0).unwrap();
    parts.controller.commit().unwrap();
    let expected = 100.0 / 127.0;
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], expected);
    assert_eq!(step(&mut parts).0, expected);

    // Each control change has a new serial, even with the same value.
    send_cc(&mut parts, CC74, 100);
    step(&mut parts);
    let again = parts.controller.last_control_change().unwrap();
    assert_eq!(
        again,
        ControlChangeSeen {
            serial: seen.serial + 1,
            ..seen
        }
    );
}

#[test]
fn the_same_value_after_a_screen_change_is_applied_again() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();
    send_cc(&mut parts, CC74, 127);
    step(&mut parts);
    parts.controller.sync_midi();

    // The knob on screen moves, then the hardware sends the same value.
    parts.controller.set_param(id, 0, 0.25).unwrap();
    assert_eq!(step(&mut parts).0, 0.25);
    assert!(parts.controller.sync_midi().is_empty(), "the screen change stays");
    send_cc(&mut parts, CC74, 127);
    assert_eq!(step(&mut parts).0, 1.0);
    assert_eq!(parts.controller.sync_midi(), vec![(id, 0)]);
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 1.0);
}

#[test]
fn mapped_control_changes_still_reach_modules() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();
    send_cc(&mut parts, CC74, 10);
    send_cc(&mut parts, CC71, 10);
    assert_eq!(step(&mut parts).1, 2.0);
}

#[test]
fn removed_mappings_stop_working() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();
    let removed = parts.controller.unmap_midi(id, 0).unwrap().unwrap();
    assert_eq!(removed.control, CC74);
    assert_eq!(parts.controller.unmap_midi(id, 0).unwrap(), None);
    parts.controller.commit().unwrap();
    send_cc(&mut parts, CC74, 127);
    assert_eq!(step(&mut parts).0, 0.0);

    // Deleting a module deletes its mappings.
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.remove_module(id).unwrap();
    assert_eq!(parts.controller.graph().midi_mappings().count(), 0);
}

#[test]
fn invalid_controls_are_refused() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    let bad = MidiControl {
        channel: 16,
        controller: 1,
    };
    assert!(parts.controller.map_midi(bad, id, 0).is_err());
    let bad = MidiControl {
        channel: 0,
        controller: 128,
    };
    assert!(parts.controller.map_midi(bad, id, 0).is_err());
    assert!(parts.controller.map_midi(CC74, id, 9).is_err());
}

#[test]
fn a_screen_edit_is_not_undone_by_an_older_control_change() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();
    send_cc(&mut parts, CC74, 127);
    step(&mut parts);

    // The knob on screen moves before the UI reads the control change.
    parts.controller.set_param(id, 0, 0.25).unwrap();
    assert!(parts.controller.sync_midi().is_empty());
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 0.25);
    assert_eq!(step(&mut parts).0, 0.25);
}

#[test]
fn a_commit_keeps_the_value_a_control_change_set() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    // A zero-depth assignment makes the engine fill the parameter from the
    // compiled base value.
    parts
        .controller
        .add_assignment_named(id, "ccs", id, "level", 0.0)
        .unwrap();
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();
    send_cc(&mut parts, CC74, 127);
    assert_eq!(step(&mut parts).0, 1.0);

    // An edit elsewhere recompiles before the UI has read the change.
    parts.controller.add_module("test.probe").unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(step(&mut parts).0, 1.0);
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 1.0);
}

#[test]
fn mappings_work_after_save_and_reopen() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();
    let text = loom_patch::Patch::from_graph("Saved", parts.controller.graph())
        .to_ron()
        .unwrap();
    assert!(text.contains("midi:"), "{text}");

    let mut reopened = engine();
    loom_patch::Patch::from_ron(&text)
        .unwrap()
        .load_into(&mut reopened.controller)
        .unwrap();
    for port in ["out", "ccs"] {
        let output = reopened.controller.output_ref(id, port).unwrap();
        reopened.controller.add_probe(output, ProbeKind::Scope).unwrap();
    }
    reopened.controller.commit().unwrap();
    send_cc(&mut reopened, CC74, 127);
    assert_eq!(step(&mut reopened).0, 1.0);
}

#[test]
fn a_control_change_after_a_screen_edit_wins() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.map_midi(CC74, id, 0).unwrap();
    parts.controller.commit().unwrap();

    // The screen edit and then a control change reach the engine in the
    // same buffer: the edit first, as commands run before MIDI.
    parts.controller.set_param(id, 0, 0.25).unwrap();
    send_cc(&mut parts, CC74, 127);
    assert_eq!(step(&mut parts).0, 1.0);
    assert_eq!(parts.controller.sync_midi(), vec![(id, 0)]);
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 1.0);
}

/// Options in a mode, over the whole travel.
fn in_mode(mode: MidiMode) -> MidiMappingOptions {
    MidiMappingOptions {
        mode,
        ..MidiMappingOptions::DEFAULT
    }
}

/// Runs one buffer of `frames` and returns the last `out` value.
fn play(parts: &mut EngineParts, frames: usize) -> f32 {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    parts.engine.process(&mut left, &mut right);
    parts
        .telemetry
        .scope()
        .filter(|chunk| chunk.probe.0 == 1)
        .flat_map(|chunk| chunk.samples().to_vec())
        .last()
        .unwrap()
}

/// A probe with its level at `level` and a mapping on it with `options`,
/// after the engine has taken both.
fn mapped_probe(options: MidiMappingOptions, level: f32) -> (EngineParts, ModuleId) {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.set_param(id, 0, level).unwrap();
    parts.controller.map_midi_with(CC74, id, 0, options).unwrap();
    parts.controller.commit().unwrap();
    step(&mut parts);
    (parts, id)
}

/// Absolute over a span: 0 lands on 0.2 and 127 on 0.8, and Invert swaps
/// them, at every block size (midi-learn.md, Acceptance criteria: mapping
/// modes).
#[test]
fn a_range_and_invert_set_where_the_control_lands() {
    for frames in [1, 77, 512] {
        let options = MidiMappingOptions {
            low: 0.2,
            high: 0.8,
            ..MidiMappingOptions::DEFAULT
        };
        let (mut parts, id) = mapped_probe(options, 0.5);
        send_cc(&mut parts, CC74, 0);
        assert!((play(&mut parts, frames) - 0.2).abs() < 1e-6);
        send_cc(&mut parts, CC74, 127);
        assert!((play(&mut parts, frames) - 0.8).abs() < 1e-6);
        // The graph holds exactly what the engine set.
        parts.controller.sync_midi();
        let playing = play(&mut parts, frames);
        assert_eq!(parts.controller.graph().node(id).unwrap().params[0], playing);

        let inverted = MidiMappingOptions {
            invert: true,
            ..options
        };
        let (mut parts, _) = mapped_probe(inverted, 0.5);
        send_cc(&mut parts, CC74, 0);
        assert!((play(&mut parts, frames) - 0.8).abs() < 1e-6);
        send_cc(&mut parts, CC74, 127);
        assert!((play(&mut parts, frames) - 0.2).abs() < 1e-6);
    }
}

/// Pickup ignores a control above the current position until it comes down
/// past it, then follows, and a screen edit makes it wait again, at every
/// block size.
#[test]
fn a_pickup_waits_until_it_is_crossed_and_after_an_edit() {
    for frames in [1, 77, 512] {
        let (mut parts, id) = mapped_probe(in_mode(MidiMode::Pickup), 0.5);
        for above in [127, 100, 70] {
            send_cc(&mut parts, CC74, above);
            assert_eq!(play(&mut parts, frames), 0.5, "CC {above} is above the parameter");
        }
        parts.controller.sync_midi();
        assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 0.5);
        send_cc(&mut parts, CC74, 40);
        let caught = play(&mut parts, frames);
        assert!((caught - 40.0 / 127.0).abs() < 1e-6, "{caught}");
        send_cc(&mut parts, CC74, 90);
        assert!((play(&mut parts, frames) - 90.0 / 127.0).abs() < 1e-6);

        parts.controller.set_param(id, 0, 0.1).unwrap();
        play(&mut parts, frames);
        send_cc(&mut parts, CC74, 80);
        assert_eq!(play(&mut parts, frames), 0.1, "the edit makes it wait");
        send_cc(&mut parts, CC74, 5);
        assert!((play(&mut parts, frames) - 5.0 / 127.0).abs() < 1e-6, "crossed 0.1");
    }
}

/// Relative in each encoding moves by the right number of steps in each
/// direction, and stops at the ends, at every block size.
#[test]
fn relative_moves_by_steps_in_each_encoding() {
    for frames in [1, 77, 512] {
        for (encoding, up, down) in [
            (MidiEncoding::Offset64, 66, 62),
            (MidiEncoding::TwosComplement, 2, 126),
            (MidiEncoding::SignBit, 2, 66),
        ] {
            let (mut parts, _) = mapped_probe(in_mode(MidiMode::Relative(encoding)), 0.5);
            send_cc(&mut parts, CC74, up);
            assert_eq!(play(&mut parts, frames), 0.5 + 2.0 / 128.0, "{encoding:?} up");
            send_cc(&mut parts, CC74, down);
            send_cc(&mut parts, CC74, down);
            assert_eq!(play(&mut parts, frames), 0.5 - 2.0 / 128.0, "{encoding:?} down");
            for _ in 0..70 {
                send_cc(&mut parts, CC74, down);
                play(&mut parts, frames);
            }
            assert_eq!(play(&mut parts, frames), 0.0, "{encoding:?} stops at the bottom");
            for _ in 0..130 {
                send_cc(&mut parts, CC74, up);
                play(&mut parts, frames);
            }
            assert_eq!(play(&mut parts, frames), 1.0, "{encoding:?} stops at the top");
        }
    }
}

/// An unrelated commit keeps a relative mapping's unrounded position. The
/// value in the graph is what the state holds, so nothing changed.
#[test]
fn an_unrelated_commit_keeps_a_relative_position() {
    let (mut parts, _) = mapped_probe(in_mode(MidiMode::Relative(MidiEncoding::Offset64)), 0.25);
    send_cc(&mut parts, CC74, 65);
    assert_eq!(step(&mut parts).0, 0.25 + 1.0 / 128.0);
    parts.controller.add_module("test.probe").unwrap();
    parts.controller.commit().unwrap();
    step(&mut parts);
    send_cc(&mut parts, CC74, 65);
    assert_eq!(step(&mut parts).0, 0.25 + 2.0 / 128.0);
}

/// A waiting entry only moves a ghost mark. After a screen edit, a waiting
/// pickup's control change must not bring the old value back: not in the
/// graph, and not in the engine at the next swap, where the assignment makes
/// the compiled base value what plays (midi-learn.md, rule 2).
#[test]
fn a_waiting_pickup_never_overwrites_a_newer_edit() {
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts
        .controller
        .add_assignment_named(id, "ccs", id, "level", 0.0)
        .unwrap();
    parts.controller.set_param(id, 0, 0.5).unwrap();
    parts
        .controller
        .map_midi_with(CC74, id, 0, in_mode(MidiMode::Pickup))
        .unwrap();
    parts.controller.commit().unwrap();
    step(&mut parts);
    // Catch it, so the table holds a position of about 0.5.
    send_cc(&mut parts, CC74, 64);
    step(&mut parts);
    parts.controller.sync_midi();

    // The player types 0.9, and the engine applies it.
    parts.controller.set_param(id, 0, 0.9).unwrap();
    assert_eq!(step(&mut parts).0, 0.9);

    // The hardware moves while the pickup waits.
    send_cc(&mut parts, CC74, 66);
    assert_eq!(step(&mut parts).0, 0.9);
    let waiting = parts.controller.midi_waiting();
    assert_eq!(waiting.len(), 1, "the hardware moved, so a ghost shows");
    assert!((waiting[0].value - 66.0 / 127.0).abs() < 1e-6);
    assert_eq!(parts.controller.sync_midi(), vec![], "nothing to copy");
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 0.9);

    // A commit recompiles and swaps. The waiting entry is not set again.
    parts.controller.add_module("test.probe").unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(step(&mut parts).0, 0.9);
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 0.9);

    // Once it catches, the ghost goes.
    send_cc(&mut parts, CC74, 127);
    assert_eq!(step(&mut parts).0, 1.0);
    assert!(parts.controller.midi_waiting().is_empty());
}

/// Changing a mapping's options gives it a new start: Pickup waits at once.
#[test]
fn changing_options_makes_a_pickup_wait() {
    let (mut parts, id) = mapped_probe(MidiMappingOptions::DEFAULT, 0.5);
    let (old, _) = parts.controller.graph().midi_mapping_of(id, 0).unwrap();
    let new = parts
        .controller
        .set_midi_options(id, 0, in_mode(MidiMode::Pickup))
        .unwrap()
        .unwrap();
    assert_ne!(old, new);
    parts.controller.commit().unwrap();
    step(&mut parts);
    send_cc(&mut parts, CC74, 127);
    assert_eq!(step(&mut parts).0, 0.5, "it waits");
    assert_eq!(
        parts
            .controller
            .set_midi_options(id, 1, in_mode(MidiMode::Pickup))
            .unwrap(),
        None,
        "no mapping on that parameter"
    );
    let bad = MidiMappingOptions {
        low: 0.9,
        high: 0.1,
        ..MidiMappingOptions::DEFAULT
    };
    assert!(parts.controller.set_midi_options(id, 0, bad).is_err());
}

/// Selecting another control range moves the same value under the hardware,
/// so a mapping on it starts again and a Pickup waits (midi-learn.md).
#[test]
fn a_range_change_makes_a_pickup_wait() {
    let mut parts = engine();
    let id = parts.controller.add_module("loom.math.constant").unwrap();
    let index = 0;
    let spec = parts.controller.graph().node(id).unwrap().descriptor.params[index];
    let ranges: &[ParamRange] = spec.options.ranges;
    assert!(ranges.len() > 1, "constant offers ranges");
    parts.controller.set_range(id, index, ranges[0]).unwrap();
    parts.controller.set_param(id, index, 0.05).unwrap();
    parts
        .controller
        .map_midi_with(CC74, id, index, in_mode(MidiMode::Pickup))
        .unwrap();
    parts.controller.commit().unwrap();
    step_silently(&mut parts);
    // CC 96 is within one step of 0.05 in this range, so it catches.
    send_cc(&mut parts, CC74, 96);
    step_silently(&mut parts);
    parts.controller.sync_midi();
    assert!(parts.controller.midi_waiting().is_empty(), "caught");
    let value = |parts: &EngineParts| parts.controller.graph().node(id).unwrap().params[index];
    let caught = value(&parts);

    let (old, _) = parts.controller.graph().midi_mapping_of(id, index).unwrap();
    parts.controller.set_range(id, index, ranges[1]).unwrap();
    let (new, mapping) = parts.controller.graph().midi_mapping_of(id, index).unwrap();
    assert_ne!(old, new);
    assert_eq!(mapping.options.mode, MidiMode::Pickup);
    // Selecting the same range again changes nothing.
    parts.controller.set_range(id, index, ranges[1]).unwrap();
    assert_eq!(parts.controller.graph().midi_mapping_of(id, index).unwrap().0, new);

    // A caught mapping would jump to the top of the new range. This one waits.
    parts.controller.commit().unwrap();
    step_silently(&mut parts);
    send_cc(&mut parts, CC74, 127);
    step_silently(&mut parts);
    parts.controller.sync_midi();
    assert_eq!(value(&parts), caught, "the value does not move");
    assert_eq!(
        parts.controller.midi_waiting().len(),
        1,
        "the engine reports it waiting"
    );
}

/// A relative step after a range change starts from the value's position in
/// the new range, not the position it had in the old one.
#[test]
fn a_relative_step_after_a_range_change_uses_the_new_range() {
    let mut parts = engine();
    let id = parts.controller.add_module("loom.math.constant").unwrap();
    let index = 0;
    let ranges = parts.controller.graph().node(id).unwrap().descriptor.params[index]
        .options
        .ranges;
    parts.controller.set_range(id, index, ranges[0]).unwrap();
    parts.controller.set_param(id, index, 0.05).unwrap();
    parts
        .controller
        .map_midi_with(CC74, id, index, in_mode(MidiMode::Relative(MidiEncoding::Offset64)))
        .unwrap();
    parts.controller.commit().unwrap();
    step_silently(&mut parts);

    parts.controller.set_range(id, index, ranges[1]).unwrap();
    parts.controller.commit().unwrap();
    step_silently(&mut parts);
    send_cc(&mut parts, CC74, 65);
    step_silently(&mut parts);
    parts.controller.sync_midi();
    let node = parts.controller.graph().node(id).unwrap();
    let spec = node.spec(index).unwrap();
    let expected = spec.denormalize(spec.normalize(0.05) + 1.0 / 128.0);
    assert_eq!(node.params[index], expected);
}

/// What a learning control change does for each mode on a moved mapping: it
/// sets the value as that mode would (midi-learn.md, Making a mapping).
#[test]
fn learning_sets_the_value_as_the_mode_would() {
    let seen = |value| ControlChangeSeen {
        control: CC74,
        value,
        serial: 1,
    };
    // Each mapping starts on `other`, then moves to `level`, keeping its
    // options.
    let moved = |parts: &mut EngineParts, id, options, value| {
        parts.controller.map_midi_with(CC74, id, 1, options).unwrap();
        parts.controller.commit().unwrap();
        step(parts);
        parts.controller.learn_midi(seen(value), id, 0).unwrap();
        parts.controller.commit().unwrap();
        step(parts);
    };

    // Absolute goes through its range and invert.
    let mut parts = engine();
    let id = add_probe(&mut parts);
    let options = MidiMappingOptions {
        low: 0.2,
        high: 0.8,
        invert: true,
        ..MidiMappingOptions::DEFAULT
    };
    moved(&mut parts, id, options, 0);
    assert!((step(&mut parts).0 - 0.8).abs() < 1e-6);
    assert_eq!(
        parts.controller.graph().midi_mapping_of(id, 0).unwrap().1.options,
        options
    );

    // Relative applies its step from the current value. Read as a position,
    // 65 would jump to about the middle.
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.set_param(id, 0, 0.25).unwrap();
    moved(&mut parts, id, in_mode(MidiMode::Relative(MidiEncoding::Offset64)), 65);
    assert_eq!(step(&mut parts).0, 0.25 + 1.0 / 128.0);

    // A value with no step sets nothing.
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts.controller.set_param(id, 0, 0.25).unwrap();
    moved(&mut parts, id, in_mode(MidiMode::Relative(MidiEncoding::Offset64)), 64);
    assert_eq!(step(&mut parts).0, 0.25);

    // A Pickup starts caught, follows at once, and an unrelated commit does
    // not make it wait.
    let mut parts = engine();
    let id = add_probe(&mut parts);
    moved(&mut parts, id, in_mode(MidiMode::Pickup), 100);
    assert!((step(&mut parts).0 - 100.0 / 127.0).abs() < 1e-6);
    parts.controller.add_module("test.probe").unwrap();
    parts.controller.commit().unwrap();
    step(&mut parts);
    send_cc(&mut parts, CC74, 10);
    assert!((step(&mut parts).0 - 10.0 / 127.0).abs() < 1e-6, "it follows at once");

    // A new control starts at the defaults.
    let mut parts = engine();
    let id = add_probe(&mut parts);
    parts
        .controller
        .map_midi_with(CC71, id, 0, in_mode(MidiMode::Pickup))
        .unwrap();
    parts.controller.commit().unwrap();
    step(&mut parts);
    parts.controller.learn_midi(seen(127), id, 0).unwrap();
    assert_eq!(
        parts.controller.graph().midi_mapping_of(id, 0).unwrap().1.options,
        MidiMappingOptions::DEFAULT
    );
    assert_eq!(parts.controller.graph().node(id).unwrap().params[0], 1.0);
}

/// A pickup works with a decibel mapping: it compares positions, so the
/// hardware catches 0 dB where the control shows it, and the top and bottom
/// of the travel are the range's ends.
#[test]
fn a_decibel_level_takes_mappings() {
    let mut parts = engine();
    let id = parts.controller.add_module("loom.mix.mixer").unwrap();
    let level = parts
        .controller
        .graph()
        .node(id)
        .unwrap()
        .descriptor
        .param_index("level1")
        .unwrap();
    parts.controller.map_midi(CC74, id, level).unwrap();
    parts.controller.commit().unwrap();
    step_silently(&mut parts);
    let value = |parts: &EngineParts| parts.controller.graph().node(id).unwrap().params[level];
    send_cc(&mut parts, CC74, 0);
    step_silently(&mut parts);
    parts.controller.sync_midi();
    assert_eq!(value(&parts), 0.0);
    send_cc(&mut parts, CC74, 127);
    step_silently(&mut parts);
    parts.controller.sync_midi();
    assert_eq!(value(&parts), 2.0);

    parts
        .controller
        .set_midi_options(id, level, in_mode(MidiMode::Pickup))
        .unwrap();
    parts.controller.set_param(id, level, 1.0).unwrap();
    parts.controller.commit().unwrap();
    step_silently(&mut parts);
    // 0 dB is at about 91% of the travel, CC 115.4.
    send_cc(&mut parts, CC74, 127);
    step_silently(&mut parts);
    send_cc(&mut parts, CC74, 120);
    step_silently(&mut parts);
    parts.controller.sync_midi();
    assert_eq!(value(&parts), 1.0, "still above 0 dB");
    send_cc(&mut parts, CC74, 110);
    step_silently(&mut parts);
    parts.controller.sync_midi();
    let spec = parts.controller.graph().node(id).unwrap().descriptor.params[level];
    assert_eq!(value(&parts), spec.denormalize(110.0 / 127.0));
}

/// Runs one buffer and ignores what it plays.
fn step_silently(parts: &mut EngineParts) {
    let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
    parts.engine.process(&mut left, &mut right);
}
