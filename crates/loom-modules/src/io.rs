//! Modules the engine runs itself: the unit delay and the audio output.

use loom_core::{ModuleDescriptor, ModuleRole, Panel, PortSpec, SignalType};

/// `loom.time.delay1`: delays its input by exactly one sample.
///
/// Put one in a feedback loop to choose where the loop's delay sits. A loop
/// without one gets a delay inserted automatically (ADR-0002).
pub static UNIT_DELAY: ModuleDescriptor = ModuleDescriptor {
    id: "loom.time.delay1",
    version: 1,
    name: "1-Sample Delay",
    category: "Time",
    description: "Delays the input by one sample. Marks where a feedback loop is broken.",
    inputs: &[PortSpec::new("in", SignalType::Audio)],
    outputs: &[PortSpec::new("out", SignalType::Audio)],
    params: &[],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

/// `loom.io.audio-out`: sends audio to the speakers or host.
pub static AUDIO_OUT: ModuleDescriptor = ModuleDescriptor {
    id: "loom.io.audio-out",
    version: 1,
    name: "Audio Out",
    category: "I/O",
    description: "Sends audio out. A cable on only one side plays on both sides.",
    inputs: &[
        PortSpec::new("left", SignalType::Audio),
        PortSpec::new("right", SignalType::Audio),
    ],
    outputs: &[],
    params: &[],
    role: ModuleRole::AudioOutput,
    panel: Panel::EMPTY,
    displays: &[],
};
