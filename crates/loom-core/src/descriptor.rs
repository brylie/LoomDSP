//! Static module descriptions.

use crate::module::Module;
use crate::signal::SignalType;

/// One input or output port.
#[derive(Debug, Clone, Copy)]
pub struct PortSpec {
    /// Stable name used in patch files. Never rename a released port.
    pub name: &'static str,
    /// Signal type, for colour and defaults.
    pub signal: SignalType,
    /// What an input reads while no cable is connected. Ignored on outputs.
    pub default: PortDefault,
}

impl PortSpec {
    /// Creates a port spec. Unconnected, it reads 0.
    pub const fn new(name: &'static str, signal: SignalType) -> Self {
        Self {
            name,
            signal,
            default: PortDefault::Value(0.0),
        }
    }

    /// Sets what the input reads while no cable is connected.
    pub const fn with_default(mut self, default: PortDefault) -> Self {
        self.default = default;
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
}

/// One parameter.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    /// Stable name used in patch files.
    pub name: &'static str,
    /// Minimum value.
    pub min: f32,
    /// Maximum value.
    pub max: f32,
    /// Default value.
    pub default: f32,
    /// Display unit, for example "Hz", "s", or "".
    pub unit: &'static str,
    /// True for whole-number parameters, such as a MIDI channel or a seed.
    /// Values are rounded, and editors step by 1.
    pub integer: bool,
    /// How the value maps to its control's position.
    pub mapping: ParamMapping,
}

/// How a parameter's value maps to its control's position, from 0 to 1.
///
/// The inspector, panel controls, and the engine's modulation all map
/// through [`ParamSpec::normalize`] and [`ParamSpec::denormalize`], so a
/// modulation arc on a knob shows exactly what the engine does.
/// Spec: `docs/design/modulation.md`, Parameter mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ParamMapping {
    /// Equal steps of the control give equal steps of the value.
    #[default]
    Linear,
    /// Equal steps of the control give equal ratios of the value, as suits
    /// frequencies and times. Needs a minimum above 0, and not a whole
    /// number; the registry refuses others.
    Logarithmic,
}

impl ParamSpec {
    /// Creates a parameter spec.
    pub const fn new(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> Self {
        Self {
            name,
            min,
            max,
            default,
            unit,
            integer: false,
            mapping: ParamMapping::Linear,
        }
    }

    /// Marks the parameter as a whole number.
    pub const fn integer(mut self) -> Self {
        self.integer = true;
        self
    }

    /// Maps the parameter logarithmically, for frequencies and times.
    pub const fn logarithmic(mut self) -> Self {
        self.mapping = ParamMapping::Logarithmic;
        self
    }

    /// Clamps a value into range, rounding whole-number parameters.
    /// NaN becomes the default.
    pub fn clamp(&self, value: f32) -> f32 {
        if value.is_nan() {
            self.default
        } else if self.integer {
            value.round().clamp(self.min, self.max)
        } else {
            value.clamp(self.min, self.max)
        }
    }

    /// Maps a value to its control's position, from 0 to 1. The value is
    /// clamped first, as by [`ParamSpec::clamp`].
    pub fn normalize(&self, value: f32) -> f32 {
        let value = self.clamp(value);
        if self.max <= self.min {
            return 0.0;
        }
        let position = match self.mapping {
            ParamMapping::Linear => (value - self.min) / (self.max - self.min),
            ParamMapping::Logarithmic => (value / self.min).ln() / (self.max / self.min).ln(),
        };
        position.clamp(0.0, 1.0)
    }

    /// Maps a control's position, from 0 to 1, to a value. The position is
    /// clamped to 0 to 1, and the result is in range and rounded for whole
    /// numbers, as by [`ParamSpec::clamp`]. NaN gives the default.
    pub fn denormalize(&self, position: f32) -> f32 {
        if position.is_nan() {
            return self.default;
        }
        let t = position.clamp(0.0, 1.0);
        let value = match self.mapping {
            ParamMapping::Linear => self.min + t * (self.max - self.min),
            ParamMapping::Logarithmic => self.min * (self.max / self.min).powf(t),
        };
        self.clamp(value)
    }
}

/// Settings passed to a module factory.
#[derive(Debug, Clone, Copy)]
pub struct ModuleConfig {
    /// Sample rate in Hz.
    pub sample_rate: f32,
    /// Largest number of frames the module will be asked to process at once.
    pub max_block: usize,
}

/// How the engine runs a module.
#[derive(Clone, Copy)]
pub enum ModuleRole {
    /// A normal module created by this factory.
    Dsp(fn(&ModuleConfig) -> Box<dyn Module>),
    /// A one-sample delay, handled by the engine itself. It can break
    /// feedback loops without adding extra delay (ADR-0002).
    /// Must have exactly one input and one output.
    UnitDelay,
    /// Sends its two inputs (left, right) to the engine output.
    /// Must have exactly two inputs and no outputs.
    AudioOutput,
}

impl std::fmt::Debug for ModuleRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleRole::Dsp(_) => f.write_str("Dsp"),
            ModuleRole::UnitDelay => f.write_str("UnitDelay"),
            ModuleRole::AudioOutput => f.write_str("AudioOutput"),
        }
    }
}

/// Everything the engine and editor need to know about a module type.
#[derive(Debug)]
pub struct ModuleDescriptor {
    /// Stable, namespaced ID, for example `"loom.osc.sine"`.
    pub id: &'static str,
    /// Version of the module's ports and parameters. Increase it when they
    /// change in a way that needs a patch migration.
    pub version: u32,
    /// Display name.
    pub name: &'static str,
    /// Category for the module browser.
    pub category: &'static str,
    /// One-line description.
    pub description: &'static str,
    /// Inputs (at most 64).
    pub inputs: &'static [PortSpec],
    /// Outputs.
    pub outputs: &'static [PortSpec],
    /// Parameters.
    pub params: &'static [ParamSpec],
    /// How the engine runs it.
    pub role: ModuleRole,
}

impl ModuleDescriptor {
    /// Index of the input with this name.
    pub fn input_index(&self, name: &str) -> Option<usize> {
        self.inputs.iter().position(|p| p.name == name)
    }

    /// Index of the output with this name.
    pub fn output_index(&self, name: &str) -> Option<usize> {
        self.outputs.iter().position(|p| p.name == name)
    }

    /// Index of the parameter with this name.
    pub fn param_index(&self, name: &str) -> Option<usize> {
        self.params.iter().position(|p| p.name == name)
    }

    /// The `"id@version"` string used in patch files.
    pub fn kind(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    /// Default parameter values.
    pub fn default_params(&self) -> Vec<f32> {
        self.params.iter().map(|p| p.default).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: ParamSpec = ParamSpec::new("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();
    const GAIN: ParamSpec = ParamSpec::new("gain", -10.0, 10.0, 1.0, "");
    const SEED: ParamSpec = ParamSpec::new("seed", 1.0, 65_535.0, 1.0, "").integer();

    #[test]
    fn mappings_round_trip() {
        for spec in [CUTOFF, GAIN, SEED] {
            for value in [spec.min, spec.default, spec.max] {
                let back = spec.denormalize(spec.normalize(value));
                assert!(
                    (back - value).abs() <= value.abs() * 1e-4 + 1e-4,
                    "{}: {value} -> {back}",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn linear_is_the_default() {
        assert_eq!(GAIN.mapping, ParamMapping::Linear);
        assert_eq!(GAIN.denormalize(0.5), 0.0);
        assert_eq!(GAIN.normalize(5.0), 0.75);
    }

    #[test]
    fn logarithmic_steps_are_equal_ratios() {
        // The middle of the knob is the geometric middle of the range.
        assert!((CUTOFF.denormalize(0.5) - 632.46).abs() < 0.1);
        // A tenth of the knob is about one octave, anywhere on it.
        for start in [0.0, 0.3, 0.9] {
            let ratio = CUTOFF.denormalize(start + 0.1) / CUTOFF.denormalize(start);
            assert!((ratio - 1_000f32.powf(0.1)).abs() < 1e-3, "{ratio}");
            assert!((ratio - 2.0).abs() < 0.01);
        }
    }

    #[test]
    fn whole_numbers_round_and_step_evenly() {
        assert_eq!(SEED.denormalize(0.5), 32_768.0);
        assert_eq!(SEED.denormalize(SEED.normalize(65_000.0)), 65_000.0);
        let step = 1.0 / (SEED.max - SEED.min);
        assert_eq!(SEED.denormalize(SEED.normalize(100.0) + step), 101.0);
    }

    #[test]
    fn results_are_always_valid() {
        for spec in [CUTOFF, GAIN, SEED] {
            assert_eq!(spec.normalize(spec.max * 10.0), 1.0);
            assert_eq!(spec.normalize(-1e9), 0.0);
            assert_eq!(spec.denormalize(2.0), spec.max);
            assert_eq!(spec.denormalize(-1.0), spec.min);
            assert_eq!(spec.denormalize(f32::NAN), spec.default);
            assert_eq!(spec.normalize(f32::NAN), spec.normalize(spec.default));
        }
        let fixed = ParamSpec::new("fixed", 1.0, 1.0, 1.0, "");
        assert_eq!(fixed.normalize(1.0), 0.0);
        assert_eq!(fixed.denormalize(0.5), 1.0);
    }
}
