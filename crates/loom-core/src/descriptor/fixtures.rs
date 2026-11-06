//! Parameters, ports, and a module description shared by the descriptor tests.

use super::*;
use crate::signal::SignalType;

/// A parameter for test descriptors.
pub(super) const fn param(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min,
        max,
        default,
        unit,
        options: ParamOptions::DEFAULT,
    }
}

pub(super) const CUTOFF: ParamSpec = param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();

pub(super) const GAIN: ParamSpec = param("gain", -10.0, 10.0, 1.0, "");

/// A port for [`NAMED`].
const fn port(name: &'static str) -> PortSpec {
    PortSpec {
        name,
        label: name,
        signal: SignalType::Audio,
        description: "A test port.",
        default: PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// An input and a parameter share the name `cutoff`, like on a filter.
pub(crate) static NAMED: ModuleDescriptor = ModuleDescriptor {
    id: "test.named",
    version: 1,
    name: "Named",
    category: "Test",
    family: crate::Family::Interface,
    description: "",
    inputs: &[port("in"), port("cutoff")],
    outputs: &[port("low"), port("high")],
    params: &[GAIN, CUTOFF],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};
