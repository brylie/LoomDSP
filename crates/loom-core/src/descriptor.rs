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
    /// What the module's tile shows between its port columns (ADR-0013).
    pub panel: Panel,
    /// Names of the values the module publishes for displays, at most
    /// [`MAX_DISPLAYS`]. It writes them in
    /// [`Module::displays`](crate::Module::displays).
    pub displays: &'static [&'static str],
}

/// Most display values one module can publish.
pub const MAX_DISPLAYS: usize = 4;

/// A module's panel: the controls and displays on its tile, laid out on the
/// canvas grid (ADR-0013). Spec: `docs/design/panels.md`, Module panels on
/// tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    /// Width in grid units.
    pub width: u8,
    /// Height in grid units.
    pub height: u8,
    /// Widgets, in keyboard focus order.
    pub widgets: &'static [Widget],
}

impl Panel {
    /// A panel with nothing on it.
    pub const EMPTY: Self = Self::new(0, 0, &[]);

    /// A panel of `width` by `height` grid units.
    pub const fn new(width: u8, height: u8, widgets: &'static [Widget]) -> Self {
        Self { width, height, widgets }
    }
}

/// One control or display on a panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Widget {
    /// What it is.
    pub kind: WidgetKind,
    /// Column of its top-left corner, in grid units from the panel's left.
    pub x: u8,
    /// Row of its top-left corner, in grid units from the panel's top.
    pub y: u8,
    /// What it shows: a parameter name for controls, a display value name
    /// for lights.
    pub bind: &'static str,
}

impl Widget {
    /// A knob for parameter `param`.
    pub const fn knob(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Knob,
            x,
            y,
            bind: param,
        }
    }

    /// A toggle for a whole-number parameter from 0 to 1.
    pub const fn toggle(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Toggle,
            x,
            y,
            bind: param,
        }
    }

    /// Radio buttons for a whole-number parameter with one value per option.
    /// The first option is the minimum value.
    pub const fn radio(x: u8, y: u8, param: &'static str, options: &'static [&'static str]) -> Self {
        Self {
            kind: WidgetKind::Radio { options },
            x,
            y,
            bind: param,
        }
    }

    /// A light for display value `display`.
    pub const fn light(x: u8, y: u8, display: &'static str) -> Self {
        Self {
            kind: WidgetKind::Light,
            x,
            y,
            bind: display,
        }
    }

    /// Width and height in grid units.
    pub const fn size(&self) -> (u8, u8) {
        match self.kind {
            WidgetKind::Knob => (3, 3),
            WidgetKind::Toggle | WidgetKind::Light => (3, 2),
            WidgetKind::Radio { options } => (3, options.len() as u8 + 1),
        }
    }

    /// True for widgets bound to a parameter.
    pub const fn is_control(&self) -> bool {
        !matches!(self.kind, WidgetKind::Light)
    }
}

/// The kinds of widget a panel can hold. More come with later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetKind {
    /// A dial for any parameter.
    Knob,
    /// Off or on, for a whole-number parameter from 0 to 1.
    Toggle,
    /// One button per option, for a whole-number parameter.
    Radio {
        /// Option labels, from the minimum value up.
        options: &'static [&'static str],
    },
    /// A display value as brightness, clamped to 0 to 1.
    Light,
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

    /// Index of a display value by name.
    pub fn display_index(&self, name: &str) -> Option<usize> {
        self.displays.iter().position(|&d| d == name)
    }

    /// Checks the display values and the panel: every widget binds to
    /// something this module has, fits its binding, lies inside the panel,
    /// and overlaps no other widget. The registry runs this check.
    pub fn check_panel(&self) -> Result<(), String> {
        if self.displays.len() > MAX_DISPLAYS {
            return Err(format!("more than {MAX_DISPLAYS} display values"));
        }
        let panel = &self.panel;
        for (i, widget) in panel.widgets.iter().enumerate() {
            let name = |problem: &str| format!("panel widget `{}` ({:?}): {problem}", widget.bind, widget.kind);
            if widget.is_control() {
                let Some(index) = self.param_index(widget.bind) else {
                    return Err(name("no such parameter"));
                };
                let param = &self.params[index];
                let values = param.max - param.min + 1.0;
                match widget.kind {
                    WidgetKind::Toggle if !(param.integer && param.min == 0.0 && param.max == 1.0) => {
                        return Err(name("a toggle needs a whole-number parameter from 0 to 1"));
                    }
                    WidgetKind::Radio { options } if !(param.integer && values == options.len() as f32) => {
                        return Err(name(
                            "radio buttons need a whole-number parameter with one value per option",
                        ));
                    }
                    _ => {}
                }
            } else if self.display_index(widget.bind).is_none() {
                return Err(name("no such display value"));
            }
            let (w, h) = widget.size();
            if u16::from(widget.x) + u16::from(w) > u16::from(panel.width)
                || u16::from(widget.y) + u16::from(h) > u16::from(panel.height)
            {
                return Err(name("outside the panel"));
            }
            for other in &panel.widgets[..i] {
                let (ow, oh) = other.size();
                let (x, y, w, h) = (u16::from(widget.x), u16::from(widget.y), u16::from(w), u16::from(h));
                let (ox, oy, ow, oh) = (u16::from(other.x), u16::from(other.y), u16::from(ow), u16::from(oh));
                let apart = x >= ox + ow || ox >= x + w || y >= oy + oh || oy >= y + h;
                if !apart {
                    return Err(name(&format!("overlaps `{}`", other.bind)));
                }
            }
        }
        Ok(())
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
