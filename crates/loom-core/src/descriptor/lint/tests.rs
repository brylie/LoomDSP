use super::*;
use crate::descriptor::fixtures::*;
use crate::descriptor::{Channels, ModuleRole, Panel, ParamSpec, PortDefault, PortSpec};
use crate::signal::SignalType;

static DOCUMENTED: ModuleDescriptor = ModuleDescriptor {
    id: "test.documented",
    version: 1,
    name: "Documented",
    category: "Test",
    family: crate::Family::Interface,
    description: "",
    inputs: &[],
    outputs: &[],
    params: &[GAIN],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

static UNDOCUMENTED: ModuleDescriptor = ModuleDescriptor {
    id: "test.undocumented",
    version: 1,
    name: "Undocumented",
    category: "Test",
    family: crate::Family::Interface,
    description: "",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "",
        default: PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "\t",
        signal: SignalType::Audio,
        description: "The input.",
        default: PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        label: "",
        description: "",
        ..GAIN
    }],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

#[test]
fn lists_empty_labels_and_descriptions_in_order() {
    assert_eq!(DOCUMENTED.first_undocumented(), None);
    assert!(DOCUMENTED.undocumented().is_empty());
    let missing: Vec<String> = UNDOCUMENTED.undocumented().iter().map(ToString::to_string).collect();
    assert_eq!(
        missing,
        [
            "input `in` has no description",
            "output `out` has no label",
            "parameter `gain` has no label",
            "parameter `gain` has no description",
        ]
    );
    assert_eq!(
        UNDOCUMENTED.first_undocumented(),
        Some(Undocumented {
            kind: SpecKind::Input,
            name: "in",
            missing: MissingText::Description,
        })
    );
}

#[test]
fn assert_documented_passes_documented_modules() {
    assert_documented(&[&DOCUMENTED]);
}

#[test]
#[should_panic(
    expected = "test.undocumented: input `in` has no description. Every port, parameter, and display value needs"
)]
fn assert_documented_names_the_first_missing_text() {
    assert_documented(&[&DOCUMENTED, &UNDOCUMENTED]);
}
