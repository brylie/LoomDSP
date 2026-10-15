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
        }
    }

    /// Marks the parameter as a whole number.
    pub const fn integer(mut self) -> Self {
        self.integer = true;
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
