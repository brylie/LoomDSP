use super::roles::{ENVELOPE, FILTER, MODULATOR};
use super::*;
use crate::descriptor::{Channels, ModuleRole, Panel, ParamOptions, PortDefault, PortRange};

/// A port for test descriptors.
const fn port(name: &'static str, signal: SignalType) -> PortSpec {
    PortSpec {
        name,
        label: name,
        signal,
        description: "A test port.",
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// A parameter for test descriptors.
const fn param(name: &'static str, unit: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit,
        options: ParamOptions::DEFAULT,
    }
}

/// A module with `in`, `lowpass`, `cutoff` in Hz, and no resonance.
static LOWPASS: ModuleDescriptor = ModuleDescriptor {
    id: "test.lowpass",
    version: 1,
    name: "Lowpass",
    category: "Test",
    family: crate::Family::FiltersAndEffects,
    description: "",
    inputs: &[port("in", SignalType::Audio)],
    outputs: &[port("lowpass", SignalType::Audio)],
    params: &[param("cutoff", "Hz"), param("steps", "").integer()],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// A filter whose output is named `lowpass` fits with one rename; the
/// optional resonance is not required.
#[test]
fn a_renamed_output_fits_the_role() {
    let fills = FillsRole {
        outputs: &[("out", "lowpass")],
        ..FillsRole::of(&FILTER)
    };
    assert_eq!(fills.check(&LOWPASS), Ok(()));
    assert_eq!(fills.own_output("out"), "lowpass");
    assert_eq!(fills.own_output("other"), "other");
    assert_eq!(fills.role_output_of("lowpass").map(|p| p.name), Some("out"));
    assert_eq!(fills.role_param_of("cutoff").map(|p| p.name), Some("cutoff"));
    assert_eq!(fills.role_param_of("steps"), None);
}

/// A missing required output is named.
#[test]
fn a_missing_required_port_is_named() {
    let fills = FillsRole::of(&FILTER);
    assert_eq!(
        fills.check(&LOWPASS),
        Err("role loom.role.filter@1 needs output `out`; the module has none".to_string())
    );
}

/// A missing required parameter is named (the registry criterion).
#[test]
fn a_missing_required_parameter_is_named() {
    let fills = FillsRole {
        inputs: &[("gate", "in")],
        outputs: &[("out", "lowpass")],
        ..FillsRole::of(&ENVELOPE)
    };
    // `in` is audio, not a gate: the first mismatch is the input's kind.
    assert_eq!(
        fills.check(&LOWPASS),
        Err("role loom.role.envelope@1: input `gate` carries Audio, not Gate".to_string())
    );
    let mismatches = ENVELOPE.mismatches(
        |_| Some(SignalType::Gate),
        |_| Some(SignalType::Modulation),
        |name| {
            (name == "release").then_some(ParamKind {
                unit: "s",
                whole: false,
            })
        },
        "parameter",
    );
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].to_string(), "no parameter `attack`");
}

/// A parameter in the wrong unit, or a whole number where the role wants
/// a continuous one, does not fit.
#[test]
fn a_parameter_of_another_kind_is_refused() {
    let wrong_unit = FillsRole {
        outputs: &[("out", "lowpass")],
        params: &[("cutoff", "steps")],
        ..FillsRole::of(&FILTER)
    };
    assert_eq!(
        wrong_unit.check(&LOWPASS),
        Err("role loom.role.filter@1: parameter `cutoff` is in no unit, not Hz".to_string())
    );
    let whole = MODULATOR.mismatches(
        |_| None,
        |_| Some(SignalType::Modulation),
        |_| {
            Some(ParamKind {
                unit: "Hz",
                whole: true,
            })
        },
        "parameter",
    );
    assert_eq!(whole[0].to_string(), "parameter `rate` is a whole number");
}

/// A map entry to a name the module lacks, for a name the role lacks, or
/// two role names onto one own name, is refused.
#[test]
fn bad_maps_are_refused() {
    let missing_own = FillsRole {
        outputs: &[("out", "highpass")],
        ..FillsRole::of(&FILTER)
    };
    assert_eq!(
        missing_own.check(&LOWPASS),
        Err("role loom.role.filter@1 maps output `out` to `highpass`, which the module does not have".to_string())
    );
    let missing_role = FillsRole {
        outputs: &[("out", "lowpass")],
        params: &[("gain", "cutoff")],
        ..FillsRole::of(&FILTER)
    };
    assert_eq!(
        missing_role.check(&LOWPASS),
        Err("role loom.role.filter@1 has no parameter `gain` to map".to_string())
    );
    let doubled = FillsRole {
        outputs: &[("out", "lowpass")],
        params: &[("cutoff", "cutoff"), ("resonance", "cutoff")],
        ..FillsRole::of(&FILTER)
    };
    assert_eq!(
        doubled.check(&LOWPASS),
        Err("role loom.role.filter@1 maps two parameters onto `cutoff`".to_string())
    );
}

/// Every initial role has a kind string, and its names are documented.
#[test]
fn initial_roles_are_documented() {
    for role in roles::ALL {
        assert_eq!(role.kind(), format!("{}@{}", role.id, role.version));
        assert!(!role.name.is_empty() && !role.description.is_empty(), "{}", role.id);
        for port in role.inputs.iter().chain(role.outputs) {
            assert!(!port.label.is_empty() && !port.description.is_empty(), "{}", port.name);
        }
        for param in role.params {
            assert!(
                !param.label.is_empty() && !param.description.is_empty(),
                "{}",
                param.name
            );
        }
    }
    assert_eq!(FILTER.input("cutoff").map(|p| p.required), Some(false));
    assert_eq!(FILTER.output("out").map(|p| p.required), Some(true));
    assert_eq!(FILTER.param("drive").map(|p| p.required), Some(false));
}

/// A map onto a name that another role name already has as its own, through
/// the identity, is a collision too.
#[test]
fn a_map_onto_another_role_names_own_name_is_refused() {
    static TWO: ModuleDescriptor = ModuleDescriptor {
        id: "test.two",
        version: 1,
        name: "Two",
        category: "Test",
        family: crate::Family::FiltersAndEffects,
        description: "",
        inputs: &[port("in", SignalType::Audio)],
        outputs: &[port("out", SignalType::Audio)],
        params: &[param("cutoff", "Hz"), param("resonance", "")],
        role: ModuleRole::UnitDelay,
        panel: Panel::EMPTY,
        displays: &[],
        roles: &[],
    };
    let fills = FillsRole {
        params: &[("drive", "resonance")],
        ..FillsRole::of(&FILTER)
    };
    assert_eq!(
        fills.check(&TWO),
        Err("role loom.role.filter@1 maps two parameters onto `resonance`".to_string())
    );
}
