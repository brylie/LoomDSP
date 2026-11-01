//! Control ranges for wide parameters (`docs/design/control-ranges.md`):
//! a selected range limits the control, so the controller, modulation, and
//! MIDI all stay inside it, while a signal on a cable is never clamped.

use loom_core::{
    EngineConfig, EngineParts, LoomError, MidiControl, MidiEvent, ModuleId, OutputRef, OutputSafety, ParamRange,
    create_engine,
};

const FINE: ParamRange = ParamRange::new(-0.1, 0.1);
const DEFAULT: ParamRange = ParamRange::new(-1.0, 1.0);
const WIDE: ParamRange = ParamRange::new(-10.0, 10.0);

/// An engine with the output safety stage off, so outputs read exactly.
fn engine() -> EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    create_engine(loom_modules::registry(), config)
}

/// Multiply `gain` of `module`, and its selected range.
fn gain(parts: &EngineParts, module: ModuleId) -> (f32, Option<ParamRange>) {
    let node = parts.controller.graph().node(module).unwrap();
    (node.params[0], node.range(0))
}

/// Renders `frames` frames and returns the left channel.
fn render(parts: &mut EngineParts, frames: usize) -> Vec<f32> {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    parts.engine.process(&mut left, &mut right);
    left
}

/// Rule 6: a smaller range clamps the value, a wider one keeps it. Each
/// module keeps its own range, and edits are clamped to the range.
#[test]
fn a_smaller_range_clamps_and_a_wider_one_keeps_the_value() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let (one, two) = (
        c.add_module("loom.math.mul").unwrap(),
        c.add_module("loom.math.mul").unwrap(),
    );
    c.set_param(one, 0, 0.8).unwrap();
    c.set_param(two, 0, 0.8).unwrap();
    assert_eq!(gain(&parts, one), (0.8, Some(DEFAULT)));

    let c = &mut parts.controller;
    c.set_range(one, 0, FINE).unwrap();
    assert_eq!(gain(&parts, one), (0.1, Some(FINE)), "smaller: clamped");
    assert_eq!(
        gain(&parts, two),
        (0.8, Some(DEFAULT)),
        "the other module keeps its own"
    );

    let c = &mut parts.controller;
    c.set_range(one, 0, WIDE).unwrap();
    assert_eq!(gain(&parts, one), (0.1, Some(WIDE)), "wider: kept");
    // An edit is clamped to the selected range.
    let c = &mut parts.controller;
    c.set_range(one, 0, FINE).unwrap();
    assert_eq!(c.set_param(one, 0, 7.0).unwrap(), 0.1);

    let refused = parts.controller.set_range(one, 0, ParamRange::new(-2.0, 2.0));
    assert!(matches!(refused, Err(LoomError::NoSuchRange { .. })), "{refused:?}");
    assert_eq!(gain(&parts, one), (0.1, Some(FINE)), "a refusal changes nothing");
}

/// Rules 4 and 5: modulation depth is a share of the selected range, and
/// the modulated value stays inside it. Changing the range reaches the
/// engine with the next commit (rule 8).
#[test]
fn modulation_stays_inside_the_selected_range() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let one = c.add_module("loom.math.constant").unwrap();
    c.set_param(one, 0, 1.0).unwrap();
    let mul = c.add_module("loom.math.mul").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(one, "out", mul, "a").unwrap();
    c.connect_named(mul, "out", out, "in").unwrap();
    c.set_param(mul, 0, 0.0).unwrap();
    c.set_range(mul, 0, FINE).unwrap();
    // A full-scale source at full depth: far past the top of the range.
    c.add_assignment(OutputRef { module: one, port: 0 }, mul, 0, 1.0)
        .unwrap();
    c.commit().unwrap();
    let left = render(&mut parts, 4_800);
    let last = *left.last().unwrap();
    assert!((last - 0.1).abs() < 1e-6, "clamped to the top of -0.1 to 0.1: {last}");
    assert!(left.iter().all(|&x| x <= 0.1 + 1e-6));

    parts.controller.set_range(mul, 0, DEFAULT).unwrap();
    parts.controller.commit().unwrap();
    let last = *render(&mut parts, 4_800).last().unwrap();
    assert!((last - 1.0).abs() < 1e-6, "now the top of -1 to 1: {last}");
}

/// Rule 4: a MIDI control change spans the selected range, as the knob does.
#[test]
fn a_control_change_spans_the_selected_range() {
    const CC: MidiControl = MidiControl {
        channel: 0,
        controller: 20,
    };
    let mut parts = engine();
    let c = &mut parts.controller;
    let mul = c.add_module("loom.math.mul").unwrap();
    c.set_range(mul, 0, FINE).unwrap();
    c.map_midi(CC, mul, 0).unwrap();
    c.commit().unwrap();
    for (value, expected) in [(127, 0.1), (0, -0.1)] {
        assert!(
            parts
                .midi
                .send(MidiEvent::control_change(0, CC.channel, CC.controller, value))
        );
        render(&mut parts, 64);
        parts.controller.sync_midi();
        let (gain, _) = gain(&parts, mul);
        assert!((gain - expected).abs() < 1e-6, "CC {value}: {gain}");
    }
}

/// `set_range` refuses a module or parameter that does not exist, and a
/// range the parameter does not offer, naming the parameter and the range.
/// The parameter's full range is accepted: a loaded value that no listed
/// range holds keeps it (rule 7).
#[test]
fn set_range_refuses_what_is_not_offered() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let mul = c.add_module("loom.math.mul").unwrap();
    let lfo = c.add_module("loom.mod.lfo").unwrap();
    assert!(matches!(
        c.set_range(ModuleId(99), 0, FINE),
        Err(LoomError::UnknownModule(_))
    ));
    assert!(matches!(c.set_range(mul, 7, FINE), Err(LoomError::NoSuchParam { .. })));
    let error = c.set_range(lfo, 0, FINE).unwrap_err();
    assert!(matches!(error, LoomError::NoSuchRange { .. }), "{error:?}");
    let error = c.set_range(mul, 0, ParamRange::new(0.0, 2.0)).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("parameter `gain` of module {mul} offers no range 0 to 2")
    );

    c.set_range(mul, 0, WIDE).unwrap();
    c.set_param(mul, 0, 9.0).unwrap();
    // -10 to 10 is both listed and the full range; the full range of a
    // parameter is always accepted.
    let full = ParamRange::new(-10.0, 10.0);
    c.set_range(mul, 0, full).unwrap();
    assert_eq!(gain(&parts, mul), (9.0, Some(full)));
}

/// A range change that must clamp the value, while the command queue is
/// full, fails and leaves both the range and the value as they were.
#[test]
fn a_failed_clamp_keeps_the_old_range() {
    let config = EngineConfig {
        command_capacity: 4,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let c = &mut parts.controller;
    let mul = c.add_module("loom.math.mul").unwrap();
    c.set_param(mul, 0, 0.8).unwrap();
    // In the engine now, so its edits are commands. A new module takes its
    // values directly until the commit (live patching spec, rule 7).
    c.commit().unwrap();
    // Fill the queue: the engine is not running, so nothing drains it.
    while c.set_param(mul, 0, 0.8).is_ok() {}
    let error = c.set_range(mul, 0, FINE).unwrap_err();
    assert!(matches!(error, LoomError::QueueFull), "{error:?}");
    assert_eq!(gain(&parts, mul), (0.8, Some(DEFAULT)));
    // Editing a parameter that does not exist names it.
    let c = &mut parts.controller;
    assert!(matches!(c.set_param(mul, 9, 0.0), Err(LoomError::NoSuchParam { .. })));
    assert!(matches!(
        c.set_params(mul, &[(9, 0.0)]),
        Err(LoomError::NoSuchParam { .. })
    ));
}
