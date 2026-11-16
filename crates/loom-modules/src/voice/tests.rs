//! Unit tests for the voice modules' value conversions.

use super::Assign;

/// `unison` becomes a voice count from 1 to 4 for any value, including the
/// non-finite ones a direct `set_param` can send. The harness clamps a
/// parameter before `set_param`, so only a direct call reaches this.
#[test]
fn unison_becomes_a_count_from_one_to_four() {
    assert_eq!(Assign::unison_from(f32::NAN), 1);
    assert_eq!(Assign::unison_from(f32::NEG_INFINITY), 1);
    assert_eq!(Assign::unison_from(f32::INFINITY), 4);
    assert_eq!(Assign::unison_from(2.6), 3);
    assert_eq!(Assign::unison_from(0.0), 1);
}

/// `mode` becomes a stored choice from 0 to 3, and NaN gives the default,
/// Legato.
#[test]
fn mode_becomes_a_choice_with_nan_at_the_default() {
    assert_eq!(Assign::mode_from(f32::NAN), 2);
    assert_eq!(Assign::mode_from(f32::NEG_INFINITY), 0);
    assert_eq!(Assign::mode_from(f32::INFINITY), 3);
    assert_eq!(Assign::mode_from(1.4), 1);
}

/// Note input and Voice assigner build `voices` and `mode` from one shared
/// definition, so the two note sources cannot drift. Called here at run
/// time, the shared builders are also measured by coverage: in a `static`
/// they only run at compile time.
#[test]
fn note_input_and_voice_assigner_share_voices_and_mode() {
    use crate::midi::NOTE_IN;
    for (spec, name) in [(super::voices_param(), "voices"), (super::mode_param(), "mode")] {
        let note = NOTE_IN.params[NOTE_IN.param_index(name).unwrap()];
        let assign = super::ASSIGN.params[super::ASSIGN.param_index(name).unwrap()];
        for other in [note, assign] {
            assert_eq!(other.name, spec.name);
            assert_eq!(
                (other.min, other.max, other.default),
                (spec.min, spec.max, spec.default)
            );
            assert_eq!(other.options.choices, spec.options.choices, "{name}");
            assert_eq!(other.options.shapes_graph, spec.options.shapes_graph, "{name}");
            assert_eq!(
                other.options.count_choices.is_some(),
                spec.options.count_choices.is_some(),
                "{name}"
            );
        }
    }
}

/// Every Voice assigner port is a bundle, and each input's declared default
/// is the one its note values start from.
#[test]
fn voice_assigners_ports_are_bundles_with_their_defaults() {
    let port = super::assign_port(
        "pitch",
        "Pitch",
        "A test port.",
        loom_core::SignalType::Pitch,
        0.25,
        loom_core::PortRange::Signal,
    );
    assert_eq!(port.channels, loom_core::Channels::Voices);
    assert!(matches!(port.default, loom_core::PortDefault::Value(v) if v == 0.25));
    for (name, value) in [
        ("pitch", Assign::DEFAULTS.pitch),
        ("velocity", Assign::DEFAULTS.velocity),
        ("pressure", Assign::DEFAULTS.pressure),
        ("timbre", Assign::DEFAULTS.timbre),
    ] {
        let index = super::ASSIGN.input_index(name).unwrap();
        assert_eq!(Assign::port_default(index), value, "{name}");
    }
}
