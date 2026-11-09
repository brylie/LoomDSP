//! Port descriptions: polarity, range, channels, and what an unconnected input reads.

use super::format::number_text;
use crate::signal::SignalType;

/// Whether an output swings around zero or sits above it.
///
/// Modulation uses this to decide how a source moves a control (ADR-0014).
/// Gate and phase outputs are unipolar. Audio, pitch, and modulation outputs
/// are bipolar unless the module says otherwise. An envelope is unipolar even
/// though its signal type is modulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    /// Nominally 0 to 1, such as an envelope or a gate.
    Unipolar,
    /// Nominally -1 to 1, such as an LFO or an oscillator.
    Bipolar,
}

impl Polarity {
    /// Remaps `sample` from the polarity the output declares to this one.
    ///
    /// The same polarity returns `sample` unchanged. A bipolar source forced
    /// unipolar becomes `(sample + 1) / 2`. A unipolar source forced bipolar
    /// becomes `2 * sample - 1`. The sample is not clamped.
    pub fn remap(self, declared: Self, sample: f32) -> f32 {
        if self == declared {
            return sample;
        }
        match self {
            Self::Unipolar => (sample + 1.0) / 2.0,
            Self::Bipolar => sample * 2.0 - 1.0,
        }
    }
}

/// One input or output port.
///
/// Write it as a struct literal, so every field is named and a missing one
/// is a compile error:
///
/// ```
/// use loom_core::{Channels, PortRange, PortDefault, PortSpec, SignalType};
///
/// const RESET: PortSpec = PortSpec {
///     name: "reset",
///     label: "Reset",
///     signal: SignalType::Gate,
///     description: "Starts the count again.",
///     default: PortDefault::Value(0.0),
///     range: PortRange::Signal,
///     channels: Channels::Downmix,
/// };
/// ```
///
/// The name is a stable ID: patch files store it, so never rename a
/// released port. The label and description are text for people. They can
/// be reworded at any time, because patch files never store them.
#[derive(Debug, Clone, Copy)]
pub struct PortSpec {
    /// Stable name used in patch files. Never rename a released port.
    pub name: &'static str,
    /// Short display name, for example "Reset". Not stored in patches.
    pub label: &'static str,
    /// Signal type, for colour and defaults.
    pub signal: SignalType,
    /// What the port does, in one short sentence that fits a tooltip.
    /// Not stored in patches. Longer theory goes in `docs/learn/`.
    pub description: &'static str,
    /// What an input reads while no cable is connected. Ignored on outputs;
    /// write `PortDefault::Value(0.0)` there.
    pub default: PortDefault,
    /// Nominal values sent by an output or expected by an input. This is a
    /// hint, never a limit on the signal carried by a cable.
    pub range: PortRange,
    /// What the port does with stereo (ADR-0020). Every port must name one.
    /// The usual choice: audio inputs are `PerChannel`, other inputs are
    /// `Downmix`, and outputs are `PerChannel`.
    pub channels: Channels,
}

/// A port's nominal signal range. See `docs/design/port-ranges.md`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortRange {
    /// Use the signal type's usual range.
    Signal,
    /// Use an explicit finite, increasing span.
    Span(f32, f32),
}

/// What a port does with stereo (ADR-0020).
///
/// The compiler works out each cable's width from these. A module never
/// sees a width it did not ask for: a `PerChannel` or `Downmix` port always
/// reads and writes one channel per call. Spec: `docs/design/stereo.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channels {
    /// On an input: stereo arriving here makes the engine run the module as
    /// two copies, one per channel. On an output: as many channels as the
    /// module runs, so mono unless the module is copied.
    PerChannel,
    /// Inputs only. Stereo arriving here does not make the module run per
    /// channel. While the module runs as one, the input reads one channel
    /// made from both sides, by a method its signal type picks.
    Downmix,
    /// The module reads or writes both channels itself. A module with any
    /// `Stereo` port is never copied, and its other inputs must be
    /// `Downmix`.
    Stereo,
}

impl PortSpec {
    /// Returns the nominal range without clipping the signal.
    pub const fn nominal_range(&self) -> (f32, f32) {
        match self.range {
            PortRange::Span(min, max) => (min, max),
            PortRange::Signal => match self.signal {
                SignalType::Audio | SignalType::Modulation => (-1.0, 1.0),
                SignalType::Pitch => (-0.5, 0.5),
                SignalType::Phase | SignalType::Gate => (0.0, 1.0),
            },
        }
    }

    /// Derives the modulation polarity from the nominal range.
    pub const fn polarity(&self) -> Polarity {
        if self.nominal_range().0 >= 0.0 {
            Polarity::Unipolar
        } else {
            Polarity::Bipolar
        }
    }

    /// Sets what the input reads while no cable is connected.
    pub const fn with_default(mut self, default: PortDefault) -> Self {
        self.default = default;
        self
    }

    /// Sets the nominal range of the port.
    pub const fn with_range(mut self, range: PortRange) -> Self {
        self.range = range;
        self
    }

    /// Sets what the port does with stereo.
    pub const fn with_channels(mut self, channels: Channels) -> Self {
        self.channels = channels;
        self
    }
}

/// What an unconnected input reads (ADR-0015).
///
/// The default is static: it does not change with the module's parameters.
/// The compiler binds inputs when the patch changes, not when a parameter
/// moves, so binding stays off the audio thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortDefault {
    /// A fixed number. Must be finite.
    Value(f32),
    /// The transport's bar phase, from 0 up to 1.
    BarPhase,
    /// The transport's beat phase (quarter notes), from 0 up to 1.
    BeatPhase,
    /// The transport's play gate: 1.0 while playing, 0.0 while stopped.
    Playing,
    /// The module's own phase. The input reads 0, and the module runs its
    /// own phase in place of it while no cable is connected, as an
    /// oscillator does from its pitch (`docs/design/oscillators.md`).
    OwnPhase,
}

impl PortDefault {
    /// What an unconnected input reads, in words, for help text.
    pub fn text(self) -> String {
        match self {
            Self::Value(v) => number_text(v),
            Self::BarPhase => "bar phase".to_string(),
            Self::BeatPhase => "beat phase".to_string(),
            Self::Playing => "play gate".to_string(),
            Self::OwnPhase => "own phase".to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
