//! Modules the engine runs itself: the unit delay and the audio output.

use loom_core::{Channels, ModuleDescriptor, ModuleRole, Panel, PortDefault, PortSpec, SignalType};

/// `loom.time.delay1`: delays its input by exactly one sample.
///
/// Put one in a feedback loop to choose where the loop's delay sits. A loop
/// without one gets a delay inserted automatically (ADR-0002). See
/// `docs/learn/feedback-loops.md`.
pub static UNIT_DELAY: ModuleDescriptor = ModuleDescriptor {
    id: "loom.time.delay1",
    version: 1,
    name: "1-Sample Delay",
    category: "Time",
    description: "Delays the input by one sample. Marks where a feedback loop is broken.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal to delay.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The input, one sample later.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
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
    description: "Sends a mono or stereo signal to the audio output.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal to play. A mono cable plays on both sides.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::Stereo,
    }],
    outputs: &[],
    params: &[],
    role: ModuleRole::AudioOutput,
    panel: Panel::EMPTY,
    displays: &[],
};
