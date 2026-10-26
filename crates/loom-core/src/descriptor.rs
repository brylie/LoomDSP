//! Static module descriptions.

use crate::module::Module;
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
/// use loom_core::{Channels, Polarity, PortDefault, PortSpec, SignalType};
///
/// const RESET: PortSpec = PortSpec {
///     name: "reset",
///     label: "Reset",
///     signal: SignalType::Gate,
///     description: "Starts the count again.",
///     default: PortDefault::Value(0.0),
///     polarity: Polarity::Unipolar,
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
    /// How an output moves a modulated control. Every port must name one.
    /// The usual choice: gates, phases, and envelopes are unipolar; audio,
    /// pitch, and LFOs are bipolar. Ignored on inputs; write the polarity of
    /// the signal the input expects there.
    pub polarity: Polarity,
    /// What the port does with stereo (ADR-0020). Every port must name one.
    /// The usual choice: audio inputs are `PerChannel`, other inputs are
    /// `Downmix`, and outputs are `PerChannel`.
    pub channels: Channels,
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
    /// Sets what the input reads while no cable is connected.
    pub const fn with_default(mut self, default: PortDefault) -> Self {
        self.default = default;
        self
    }

    /// Sets the polarity an output declares for modulation.
    pub const fn with_polarity(mut self, polarity: Polarity) -> Self {
        self.polarity = polarity;
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
}

/// One parameter.
///
/// Write it as a struct literal, so every field is named and a missing one
/// is a compile error. Optional settings live in [`ParamOptions`]; start
/// from [`ParamOptions::DEFAULT`] and use the builder methods:
///
/// ```
/// use loom_core::{ParamOptions, ParamSpec};
///
/// const N: ParamSpec = ParamSpec {
///     name: "n",
///     label: "Division",
///     description: "Fires on every Nth trigger.",
///     min: 1.0,
///     max: 64.0,
///     default: 2.0,
///     unit: "",
///     options: ParamOptions::DEFAULT,
/// }
/// .integer();
/// ```
///
/// The name is a stable ID: patch files store it, so never rename a
/// released parameter. The label and description are text for people. They
/// can be reworded at any time, because patch files never store them.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    /// Stable name used in patch files. Never rename a released parameter.
    pub name: &'static str,
    /// Short display name, for example "Cutoff". Not stored in patches.
    pub label: &'static str,
    /// What the parameter does, in one short sentence that fits a tooltip.
    /// Not stored in patches. Longer theory goes in `docs/learn/`.
    pub description: &'static str,
    /// Minimum value.
    pub min: f32,
    /// Maximum value.
    pub max: f32,
    /// Default value.
    pub default: f32,
    /// Display unit, for example "Hz", "s", or "".
    pub unit: &'static str,
    /// Optional settings. Write [`ParamOptions::DEFAULT`] and change them
    /// with the builder methods, such as [`ParamSpec::integer`].
    pub options: ParamOptions,
}

/// Optional settings of a [`ParamSpec`].
///
/// It is a separate struct with a [`ParamOptions::DEFAULT`] constant, so a
/// new option later breaks no module. `ParamSpec` as a whole has no default,
/// because its label and description must always be written. A constant,
/// not `Default::default()`, because descriptors are `static`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ParamOptions {
    /// True for whole-number parameters, such as a MIDI channel or a seed.
    /// Values are rounded, and editors step by 1.
    pub integer: bool,
    /// How the value maps to its control's position.
    pub mapping: ParamMapping,
    /// Seconds the engine spends smoothing this parameter's base while it
    /// is modulated. Zero means the base steps immediately. The glide is in
    /// the control's 0 to 1 position, and the modulation added on top is not
    /// smoothed. Spec: `docs/design/modulation.md`, rule 7.
    pub smooth_seconds: f32,
    /// Names of the values of a whole-number parameter, from `min` up in
    /// steps of 1, or empty. Set with [`ParamSpec::choices`].
    /// Spec: `docs/design/panels.md`, Choice names.
    pub choices: &'static [&'static str],
    /// The ranges a player can pick for this parameter's control, from the
    /// smallest up, or empty. Set with [`ParamSpec::ranges`].
    /// Spec: `docs/design/control-ranges.md`.
    pub ranges: &'static [ParamRange],
    /// The range a new module starts with. One of `ranges`.
    pub default_range: Option<ParamRange>,
}

/// A span a control can cover: the values at the knob's minimum and maximum
/// (`docs/design/control-ranges.md`). It limits the control, never the
/// signal on a cable. Its name is its bounds, such as "-0.1 to 0.1".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamRange {
    /// Value at the knob's minimum.
    pub min: f32,
    /// Value at the knob's maximum.
    pub max: f32,
}

impl ParamRange {
    /// The range from `min` to `max`.
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    /// True when `value` lies inside the range, bounds included.
    pub fn contains(&self, value: f32) -> bool {
        value >= self.min && value <= self.max
    }
}

impl std::fmt::Display for ParamRange {
    /// "-0.1 to 0.1": plain numbers, which need no translation (rule 10).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // -0 reads as 0 to people.
        let number = |v: f32| if v == 0.0 { 0.0 } else { v };
        write!(f, "{} to {}", number(self.min), number(self.max))
    }
}

impl ParamOptions {
    /// No options: a linear parameter with any value in range.
    pub const DEFAULT: Self = Self {
        integer: false,
        mapping: ParamMapping::Linear,
        smooth_seconds: 0.0,
        choices: &[],
        ranges: &[],
        default_range: None,
    };
}

impl Default for ParamOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
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
    /// Marks the parameter as a whole number.
    pub const fn integer(mut self) -> Self {
        self.options.integer = true;
        self
    }

    /// Maps the parameter logarithmically, for frequencies and times.
    pub const fn logarithmic(mut self) -> Self {
        self.options.mapping = ParamMapping::Logarithmic;
        self
    }

    /// Smooths the base value over about `seconds` while the parameter is
    /// modulated. The time is the same one-pole glide modules already use
    /// for an unmodulated knob.
    pub const fn smooth(mut self, seconds: f32) -> Self {
        self.options.smooth_seconds = seconds;
        self
    }

    /// Names each value of a whole-number parameter, from `min` up: one name
    /// per value, so `max - min + 1` names. The registry refuses names on a
    /// parameter that is not whole-number, a count that does not match, and
    /// an empty name. Spec: `docs/design/panels.md`, Choice names.
    pub const fn choices(mut self, names: &'static [&'static str]) -> Self {
        self.options.choices = names;
        self
    }

    /// Offers `ranges` for the control, from the smallest up, and starts
    /// with `default`. The registry refuses ranges outside `min` and `max`,
    /// a range with `min >= max`, a default not in the list, a default value
    /// outside the default range, and ranges on a logarithmic or
    /// whole-number parameter. Spec: `docs/design/control-ranges.md`.
    pub const fn ranges(mut self, ranges: &'static [ParamRange], default: ParamRange) -> Self {
        self.options.ranges = ranges;
        self.options.default_range = Some(default);
        self
    }

    /// The range a new module starts with, if the parameter offers ranges.
    pub fn default_range(&self) -> Option<ParamRange> {
        if self.options.ranges.is_empty() {
            None
        } else {
            self.options.default_range
        }
    }

    /// This parameter with its control limited to `range`: `min` and `max`
    /// become the range's bounds, and the default is clamped into it. Every
    /// user of the control maps through the result, so the inspector, the
    /// tile knob, MIDI, and the engine's modulation agree (rule 4).
    #[must_use]
    pub fn in_range(&self, range: ParamRange) -> Self {
        let mut spec = *self;
        spec.min = range.min;
        spec.max = range.max;
        spec.default = self.default.clamp(range.min, range.max);
        spec
    }

    /// The smallest range in the list that holds `value`: the first one, as
    /// the list goes from the smallest up. `None` if none does, or the
    /// parameter offers no ranges.
    pub fn smallest_range_holding(&self, value: f32) -> Option<ParamRange> {
        self.options.ranges.iter().copied().find(|range| range.contains(value))
    }

    /// The range a loaded module gets for this parameter (rule 7), and
    /// whether the loader reports it. Ranges never change a saved value.
    ///
    /// No saved range: the default range if `value` fits, else the
    /// smallest that holds it, with no report. A saved range in the list
    /// that holds `value` is kept. Any other saved range gets the smallest
    /// range that holds `value`, and a report. `value` must already be
    /// clamped to `min` and `max`. `None` if the parameter offers no ranges.
    pub fn load_range(&self, saved: Option<ParamRange>, value: f32) -> Option<(ParamRange, bool)> {
        let default = self.default_range()?;
        let fallback = self
            .smallest_range_holding(value)
            .unwrap_or(ParamRange::new(self.min, self.max));
        Some(match saved {
            None if default.contains(value) => (default, false),
            None => (fallback, false),
            Some(range) if self.options.ranges.contains(&range) && range.contains(value) => (range, false),
            Some(_) => (fallback, true),
        })
    }

    /// Checks the ranges, as the registry does (rules 2 and 3).
    pub fn check_ranges(&self) -> Result<(), String> {
        let ranges = self.options.ranges;
        if ranges.is_empty() {
            return Ok(());
        }
        let problem = |text: &str| Err(format!("parameter `{}`: {text}", self.name));
        if self.options.integer || self.options.mapping != ParamMapping::Linear {
            return problem("only linear parameters that are not whole numbers can offer ranges");
        }
        for range in ranges {
            if range.min.is_nan() || range.max.is_nan() || range.min >= range.max {
                return problem(&format!("range {range} has a minimum that is not below its maximum"));
            }
            if range.min < self.min || range.max > self.max {
                return problem(&format!(
                    "range {range} is outside the parameter's {} to {}",
                    self.min, self.max
                ));
            }
        }
        let Some(default) = self.options.default_range.filter(|d| ranges.contains(d)) else {
            return problem("the default range is not in the list");
        };
        if !default.contains(self.default) {
            return problem(&format!(
                "the default value {} is outside the default range {default}",
                self.default
            ));
        }
        Ok(())
    }

    /// The name of `value`, if this parameter names its values. The value
    /// is clamped and rounded first, as by [`ParamSpec::clamp`].
    pub fn choice_name(&self, value: f32) -> Option<&'static str> {
        let index = self.clamp(value) - self.min;
        if self.options.choices.is_empty() || !index.is_finite() || index < 0.0 {
            return None;
        }
        self.options.choices.get(index as usize).copied()
    }

    /// Checks the choice names: only on a whole-number parameter, one per
    /// value, and none empty. The registry runs this check.
    pub fn check_choices(&self) -> Result<(), String> {
        let names = self.options.choices;
        if names.is_empty() {
            return Ok(());
        }
        let problem = |text: &str| Err(format!("parameter `{}`: {text}", self.name));
        if !self.options.integer {
            return problem("choice names need a whole-number parameter");
        }
        let values = self.max - self.min + 1.0;
        if values != names.len() as f32 {
            return problem(&format!(
                "{} choice names for {values} values; give one name per value",
                names.len()
            ));
        }
        if names.iter().any(|name| is_blank(name)) {
            return problem("a choice name is empty");
        }
        Ok(())
    }

    /// Clamps a value into range, rounding whole-number parameters.
    /// NaN becomes the default.
    pub fn clamp(&self, value: f32) -> f32 {
        if value.is_nan() {
            self.default
        } else if self.options.integer {
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
        let position = match self.options.mapping {
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
        let value = match self.options.mapping {
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
    /// Must have exactly one input and one output, and no parameters or
    /// display values.
    UnitDelay,
    /// Sends its two inputs (left, right) to the engine output.
    /// Must have exactly two inputs and no outputs, and no parameters or
    /// display values.
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
    /// The values the module publishes for displays, at most
    /// [`MAX_DISPLAYS`]. It writes them in
    /// [`Module::displays`](crate::Module::displays).
    pub displays: &'static [DisplaySpec],
}

/// One value a module publishes for the lights on its panel (ADR-0013).
///
/// Write it as a struct literal, so every field is named and a missing one
/// is a compile error, as for [`PortSpec`]:
///
/// ```
/// use loom_core::{DisplayKind, DisplaySpec};
///
/// const GATE: DisplaySpec = DisplaySpec {
///     name: "gate",
///     label: "Gate",
///     description: "Lit while a key is held.",
///     kind: DisplayKind::Level,
/// };
/// ```
///
/// The name is the stable ID that panels and [`Module::displays`](crate::Module::displays)
/// use. The label is the light's caption, and the description its hover
/// text. Spec: `docs/design/port-and-parameter-text.md`, Display values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySpec {
    /// Stable name that panel widgets bind to.
    pub name: &'static str,
    /// Short display name, for example "Gate". The caption of its light.
    pub label: &'static str,
    /// What the value shows, in one short sentence that fits a tooltip.
    pub description: &'static str,
    /// How the engine stores it and the UI reads it.
    pub kind: DisplayKind,
}

/// How a display value travels from the engine to a light.
/// Spec: `docs/design/panels.md`, Lights for one-sample triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayKind {
    /// The latest value: each block replaces the last one.
    Level,
    /// A trigger: the module writes 1.0 if a trigger arrived in the block,
    /// else 0.0. The display table keeps a 1.0 until the UI reads it, so a
    /// one-sample trigger between two frames still lights the light.
    Trigger,
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
    /// for lights, an output name for meters.
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

    /// Radio buttons for a whole-number parameter with choice names
    /// ([`ParamSpec::choices`]), one button per name. The first is the
    /// minimum value. The names come from the parameter, so a panel cannot
    /// show other words than the inspector.
    pub const fn radio(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Radio,
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

    /// A level meter on output `output`, fed by a port probe.
    pub const fn meter(x: u8, y: u8, output: &'static str) -> Self {
        Self {
            kind: WidgetKind::Meter,
            x,
            y,
            bind: output,
        }
    }

    /// A large button for a whole-number parameter from 0 to 1.
    ///
    /// `mode` names a different whole-number parameter, from 0 to 1, that
    /// chooses the gesture: 0 flips `param` on press, and 1 holds it high
    /// while the button is held. The canvas does not read it. The editor
    /// does, through [`ButtonMode`]. It does not have to be a widget.
    /// Spec: `docs/design/clock.md`, Button details.
    pub const fn button(x: u8, y: u8, param: &'static str, mode: &'static str) -> Self {
        Self {
            kind: WidgetKind::Button { mode },
            x,
            y,
            bind: param,
        }
    }

    /// True for widgets bound to a parameter.
    pub const fn is_control(&self) -> bool {
        !matches!(self.kind, WidgetKind::Light | WidgetKind::Meter)
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
    /// One button per choice name, for a whole-number parameter that names
    /// its values. The names come from the parameter.
    Radio,
    /// A display value as brightness, clamped to 0 to 1.
    Light,
    /// The level of one of the module's outputs, from a port probe: an RMS
    /// bar and a peak line, -60 dB to 0 dB.
    Meter,
    /// A large button for a whole-number parameter from 0 to 1.
    ///
    /// `mode` names the parameter that chooses the gesture. It is not the
    /// value the button shows.
    Button {
        /// Whole-number parameter from 0 to 1: 0 flips on press, 1 holds
        /// while pressed. Not the parameter the button shows.
        mode: &'static str,
    },
}

/// How a [`WidgetKind::Button`] answers a gesture.
///
/// The canvas reports a press, a release, or an arrow. The editor uses this
/// to choose the parameter value, so the canvas stays a reporter of input
/// (ADR-0010). Spec: `docs/design/clock.md`, Button details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonMode {
    /// A press flips the gate. A release leaves it.
    Toggle,
    /// The gate is high only while the button is held.
    Momentary,
}

impl ButtonMode {
    /// `1` is momentary. Any lower value is toggle.
    ///
    /// A mode parameter is a whole number from 0 to 1, so these are the only
    /// values the editor stores.
    #[must_use]
    pub fn from_value(value: f32) -> Self {
        if value >= 1.0 { Self::Momentary } else { Self::Toggle }
    }

    /// The value to write when the button is pressed.
    ///
    /// Toggle flips `current`. Momentary goes high.
    #[must_use]
    pub fn press(self, current: f32) -> Option<f32> {
        Some(match self {
            Self::Toggle => {
                if current >= 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
            Self::Momentary => 1.0,
        })
    }

    /// The value to write when the hold ends.
    ///
    /// Toggle keeps its gate, so this is `None`. Momentary goes low.
    #[must_use]
    pub fn release(self) -> Option<f32> {
        match self {
            Self::Toggle => None,
            Self::Momentary => Some(0.0),
        }
    }

    /// The value to write for an arrow key, if the key changes anything.
    ///
    /// `high` is up or right. Toggle sets 0 or 1. Momentary ignores arrows.
    /// A toggle that is already there returns `None`.
    #[must_use]
    pub fn arrow(self, current: f32, high: bool) -> Option<f32> {
        match self {
            Self::Momentary => None,
            Self::Toggle => {
                let next = if high { 1.0 } else { 0.0 };
                ((current >= 0.5) != high).then_some(next)
            }
        }
    }
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

    /// Width and height of `widget` in grid units. Radio buttons are one row
    /// per choice name of their parameter, plus one for the label.
    pub fn widget_size(&self, widget: &Widget) -> (u8, u8) {
        match widget.kind {
            WidgetKind::Knob => (3, 3),
            WidgetKind::Meter => (3, 6),
            WidgetKind::Toggle | WidgetKind::Light => (3, 2),
            WidgetKind::Button { .. } => (6, 3),
            WidgetKind::Radio => {
                let choices = self
                    .param_index(widget.bind)
                    .map_or(0, |index| self.params[index].options.choices.len());
                (3, u8::try_from(choices + 1).unwrap_or(u8::MAX))
            }
        }
    }

    /// Index of a display value by name.
    pub fn display_index(&self, name: &str) -> Option<usize> {
        self.displays.iter().position(|d| d.name == name)
    }

    /// Parameters to set to 0 when `name` changes from `previous` to `next`.
    ///
    /// A button whose mode moves to momentary (1) clears the parameter the
    /// button shows, so a latched gate does not stay high with nobody holding
    /// it. Setting momentary again, while it is already momentary, clears
    /// nothing. The rule follows the widget, so any module with a button
    /// gets it. Spec: `docs/design/clock.md`, Button details.
    pub fn button_gates_to_clear<'a>(
        &'a self,
        name: &'a str,
        previous: f32,
        next: f32,
    ) -> impl Iterator<Item = usize> + 'a {
        let switching = previous < 1.0 && next >= 1.0;
        self.panel.widgets.iter().filter_map(move |widget| {
            let WidgetKind::Button { mode } = widget.kind else {
                return None;
            };
            (switching && mode == name)
                .then(|| self.param_index(widget.bind))
                .flatten()
        })
    }

    /// Checks the display values, the choice names, the ranges, and the panel: every widget binds to
    /// something this module has, fits its binding, lies inside the panel,
    /// and overlaps no other widget. The registry runs this check.
    pub fn check_panel(&self) -> Result<(), String> {
        if self.displays.len() > MAX_DISPLAYS {
            return Err(format!("more than {MAX_DISPLAYS} display values"));
        }
        for param in self.params {
            param.check_choices()?;
            param.check_ranges()?;
        }
        let panel = &self.panel;
        for (i, widget) in panel.widgets.iter().enumerate() {
            let name = |problem: &str| format!("panel widget `{}` ({:?}): {problem}", widget.bind, widget.kind);
            if widget.is_control() {
                let Some(index) = self.param_index(widget.bind) else {
                    return Err(name("no such parameter"));
                };
                // Copied before `param` is borrowed: a button names a second
                // parameter, and the two borrows cannot overlap.
                let mode_range = if let WidgetKind::Button { mode } = widget.kind {
                    if mode == widget.bind {
                        return Err(name("mode must be a different parameter"));
                    }
                    let Some(mode_index) = self.param_index(mode) else {
                        return Err(name("no such mode parameter"));
                    };
                    let mode_param = &self.params[mode_index];
                    Some((mode_param.options.integer, mode_param.min, mode_param.max))
                } else {
                    None
                };
                let param = &self.params[index];
                let integer = param.options.integer;
                match widget.kind {
                    WidgetKind::Toggle if !(integer && param.min == 0.0 && param.max == 1.0) => {
                        return Err(name("a toggle needs a whole-number parameter from 0 to 1"));
                    }
                    WidgetKind::Radio if !integer || param.options.choices.is_empty() => {
                        return Err(name("radio buttons need a whole-number parameter with choice names"));
                    }
                    WidgetKind::Button { .. } if !(integer && param.min == 0.0 && param.max == 1.0) => {
                        return Err(name("a button needs a whole-number parameter from 0 to 1"));
                    }
                    WidgetKind::Button { .. }
                        if !mode_range.is_some_and(|(whole, min, max)| whole && min == 0.0 && max == 1.0) =>
                    {
                        return Err(name("mode must be a whole-number parameter from 0 to 1"));
                    }
                    _ => {}
                }
            } else if widget.kind == WidgetKind::Meter {
                if !self.outputs.iter().any(|port| port.name == widget.bind) {
                    return Err(name("no such output"));
                }
            } else if self.display_index(widget.bind).is_none() {
                return Err(name("no such display value"));
            }
            let (w, h) = self.widget_size(widget);
            if u16::from(widget.x) + u16::from(w) > u16::from(panel.width)
                || u16::from(widget.y) + u16::from(h) > u16::from(panel.height)
            {
                return Err(name("outside the panel"));
            }
            for other in &panel.widgets[..i] {
                let (ow, oh) = self.widget_size(other);
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

    /// The first port, parameter, or display value with an empty (or blank)
    /// label or description, in the order inputs, outputs, parameters,
    /// display values.
    pub const fn first_undocumented(&self) -> Option<Undocumented> {
        let mut index = 0;
        while let Some(text) = self.text(index) {
            if is_blank(text.value) {
                return Some(text.undocumented);
            }
            index += 1;
        }
        None
    }

    /// Every port, parameter, and display value with an empty (or blank)
    /// label or description, in the order inputs, outputs, parameters,
    /// display values.
    pub fn undocumented(&self) -> Vec<Undocumented> {
        (0..)
            .map_while(|index| self.text(index))
            .filter(|text| is_blank(text.value))
            .map(|text| text.undocumented)
            .collect()
    }

    /// Text field number `index`, counting the label and then the
    /// description of each input, output, parameter, and display value. One list for both
    /// checks above, so they cannot disagree.
    const fn text(&self, index: usize) -> Option<Text> {
        let (port, field) = (index / 2, index % 2);
        let (kind, name, label, description) = if port < self.inputs.len() {
            let spec = &self.inputs[port];
            (SpecKind::Input, spec.name, spec.label, spec.description)
        } else if port < self.inputs.len() + self.outputs.len() {
            let spec = &self.outputs[port - self.inputs.len()];
            (SpecKind::Output, spec.name, spec.label, spec.description)
        } else if port < self.inputs.len() + self.outputs.len() + self.params.len() {
            let spec = &self.params[port - self.inputs.len() - self.outputs.len()];
            (SpecKind::Parameter, spec.name, spec.label, spec.description)
        } else if port < self.inputs.len() + self.outputs.len() + self.params.len() + self.displays.len() {
            let spec = &self.displays[port - self.inputs.len() - self.outputs.len() - self.params.len()];
            (SpecKind::Display, spec.name, spec.label, spec.description)
        } else {
            return None;
        };
        let (missing, value) = if field == 0 {
            (MissingText::Label, label)
        } else {
            (MissingText::Description, description)
        };
        Some(Text {
            undocumented: Undocumented { kind, name, missing },
            value,
        })
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

/// A port, parameter, or display value with an empty label or description.
///
/// Every built-in module is checked at compile time with
/// [`assert_documented`]. The `loom-test` harness fails on one, and the
/// registry loads the module but keeps a note ([`Registry::notes`](crate::Registry::notes)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Undocumented {
    /// Whether it is an input, an output, a parameter, or a display value.
    pub kind: SpecKind,
    /// Its stable name.
    pub name: &'static str,
    /// Which text is empty.
    pub missing: MissingText,
}

/// Whether a spec is an input, an output, a parameter, or a display value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecKind {
    /// An input port.
    Input,
    /// An output port.
    Output,
    /// A parameter.
    Parameter,
    /// A display value, for a light.
    Display,
}

impl SpecKind {
    /// "input", "output", "parameter", or "display value".
    pub const fn as_str(self) -> &'static str {
        match self {
            SpecKind::Input => "input",
            SpecKind::Output => "output",
            SpecKind::Parameter => "parameter",
            SpecKind::Display => "display value",
        }
    }
}

/// Which text of a port, parameter, or display value is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingText {
    /// The `label` field.
    Label,
    /// The `description` field.
    Description,
}

impl MissingText {
    /// "label" or "description".
    pub const fn as_str(self) -> &'static str {
        match self {
            MissingText::Label => "label",
            MissingText::Description => "description",
        }
    }
}

impl std::fmt::Display for Undocumented {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} `{}` has no {}",
            self.kind.as_str(),
            self.name,
            self.missing.as_str()
        )
    }
}

/// One text field of a port, parameter, or display value, for the checks.
struct Text {
    undocumented: Undocumented,
    value: &'static str,
}

const fn is_blank(text: &str) -> bool {
    text.trim_ascii().is_empty()
}

/// Panics if any port, parameter, or display value of `modules` has an
/// empty label or description. Call it in a `const` item, so a missing text is a compile
/// error:
///
/// ```
/// # static ALL: &[&loom_core::ModuleDescriptor] = &[];
/// const _: () = loom_core::assert_documented(ALL);
/// ```
pub const fn assert_documented(modules: &[&ModuleDescriptor]) {
    let mut i = 0;
    while i < modules.len() {
        let module = modules[i];
        if let Some(missing) = module.first_undocumented() {
            // A `const` cannot format, so the message is copied into a
            // fixed buffer by hand.
            let message = ConstMessage::new()
                .push(module.id)
                .push(": ")
                .push(missing.kind.as_str())
                .push(" `")
                .push(missing.name)
                .push("` has no ")
                .push(missing.missing.as_str())
                .push(". Every port, parameter, and display value needs a short label and a one-sentence description.");
            panic!("{}", message.as_str(module.id));
        }
        i += 1;
    }
}

/// A message built in a `const fn`, cut at 256 bytes.
struct ConstMessage {
    bytes: [u8; 256],
    len: usize,
}

impl ConstMessage {
    const fn new() -> Self {
        Self {
            bytes: [0; 256],
            len: 0,
        }
    }

    const fn push(mut self, text: &str) -> Self {
        let text = text.as_bytes();
        let mut i = 0;
        while i < text.len() && self.len < self.bytes.len() {
            self.bytes[self.len] = text[i];
            self.len += 1;
            i += 1;
        }
        self
    }

    /// The message, or `fallback` if the cut split a character.
    const fn as_str<'a>(&'a self, fallback: &'a str) -> &'a str {
        match std::str::from_utf8(self.bytes.split_at(self.len).0) {
            Ok(text) => text,
            Err(_) => fallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parameter for test descriptors.
    const fn param(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> ParamSpec {
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

    const CUTOFF: ParamSpec = param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();
    const GAIN: ParamSpec = param("gain", -10.0, 10.0, 1.0, "");
    const SEED: ParamSpec = param("seed", 1.0, 65_535.0, 1.0, "").integer();

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
        assert_eq!(GAIN.options.mapping, ParamMapping::Linear);
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

    const RANGES: &[ParamRange] = &[
        ParamRange::new(-0.1, 0.1),
        ParamRange::new(-1.0, 1.0),
        ParamRange::new(-5.0, 5.0),
    ];
    const WIDE: ParamSpec = param("gain", -10.0, 10.0, 1.0, "").ranges(RANGES, ParamRange::new(-1.0, 1.0));

    /// A selected range maps its bounds to positions 0 and 1, and clamps
    /// values outside it and NaN as the full range does.
    #[test]
    fn a_selected_range_replaces_the_bounds() {
        let fine = WIDE.in_range(RANGES[0]);
        assert_eq!(fine.normalize(-0.1), 0.0);
        assert_eq!(fine.normalize(0.1), 1.0);
        assert_eq!(fine.normalize(0.05), 0.75);
        assert_eq!(fine.denormalize(0.5), 0.0);
        assert_eq!(fine.normalize(5.0), 1.0);
        assert_eq!(fine.clamp(5.0), 0.1);
        // The default is clamped into the range, for NaN and reset.
        assert_eq!(fine.denormalize(f32::NAN), 0.1);
        assert_eq!(fine.clamp(f32::NAN), 0.1);
        // 100 times finer than the full range.
        let full = WIDE.in_range(ParamRange::new(-10.0, 10.0));
        let step = |spec: ParamSpec| spec.denormalize(0.51) - spec.denormalize(0.5);
        assert!((step(full) / step(fine) - 100.0).abs() < 1e-2);
        assert_eq!(RANGES[0].to_string(), "-0.1 to 0.1");
        assert_eq!(ParamRange::new(0.0, 10.0).to_string(), "0 to 10");
    }

    /// Rule 7: a load never changes the value. It picks the default range,
    /// the saved one, or the smallest that holds the value.
    #[test]
    fn a_loaded_range_holds_the_saved_value() {
        let (default, fine, half) = (RANGES[1], RANGES[0], RANGES[2]);
        assert_eq!(WIDE.default_range(), Some(default));
        assert_eq!(WIDE.load_range(None, 0.5), Some((default, false)));
        assert_eq!(WIDE.load_range(None, 3.0), Some((half, false)));
        assert_eq!(WIDE.load_range(Some(fine), 0.05), Some((fine, false)));
        // A saved range that does not hold the value, or is not offered.
        assert_eq!(WIDE.load_range(Some(fine), 5.0), Some((half, true)));
        assert_eq!(
            WIDE.load_range(Some(ParamRange::new(-2.0, 2.0)), 0.5),
            Some((default, true))
        );
        // No range in the list holds it: the full range.
        assert_eq!(WIDE.load_range(None, 8.0), Some((ParamRange::new(-10.0, 10.0), false)));
        assert_eq!(GAIN.load_range(None, 1.0), None);
        assert_eq!(WIDE.check_ranges(), Ok(()));
    }

    /// A named value reads its name, after clamping and rounding. A
    /// parameter without names has none.
    #[test]
    fn choice_names_follow_the_value() {
        let slope = param("slope", 1.0, 4.0, 1.0, "")
            .integer()
            .choices(&["6 dB", "12 dB", "18 dB", "24 dB"]);
        assert_eq!(slope.choice_name(1.0), Some("6 dB"));
        assert_eq!(slope.choice_name(3.6), Some("24 dB"));
        assert_eq!(slope.choice_name(-5.0), Some("6 dB"));
        assert_eq!(slope.choice_name(f32::NAN), Some("6 dB"), "NaN is the default");
        assert_eq!(GAIN.choice_name(1.0), None);
        assert_eq!(slope.check_choices(), Ok(()));
        assert_eq!(GAIN.check_choices(), Ok(()));
    }

    #[test]
    fn polarity_remap_follows_the_override() {
        assert_eq!(Polarity::Bipolar.remap(Polarity::Bipolar, -0.4), -0.4);
        assert_eq!(Polarity::Unipolar.remap(Polarity::Bipolar, -1.0), 0.0);
        assert_eq!(Polarity::Unipolar.remap(Polarity::Bipolar, 1.0), 1.0);
        assert_eq!(Polarity::Bipolar.remap(Polarity::Unipolar, 0.0), -1.0);
        assert_eq!(Polarity::Bipolar.remap(Polarity::Unipolar, 1.0), 1.0);
        // A source past the nominal range is kept. Only the parameter is clamped.
        assert_eq!(Polarity::Bipolar.remap(Polarity::Bipolar, 3.0), 3.0);
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
        let fixed = param("fixed", 1.0, 1.0, 1.0, "");
        assert_eq!(fixed.normalize(1.0), 0.0);
        assert_eq!(fixed.denormalize(0.5), 1.0);
    }

    static DOCUMENTED: ModuleDescriptor = ModuleDescriptor {
        id: "test.documented",
        version: 1,
        name: "Documented",
        category: "Test",
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
        description: "",
        inputs: &[PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
            channels: Channels::PerChannel,
        }],
        outputs: &[PortSpec {
            name: "out",
            label: "\t",
            signal: SignalType::Audio,
            description: "The input.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Bipolar,
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

    #[test]
    fn button_mode_turns_a_gesture_into_a_value() {
        let toggle = ButtonMode::Toggle;
        assert_eq!(toggle.press(0.0), Some(1.0));
        assert_eq!(toggle.press(1.0), Some(0.0));
        assert_eq!(toggle.release(), None);
        assert_eq!(toggle.arrow(0.0, true), Some(1.0));
        assert_eq!(toggle.arrow(1.0, true), None);
        assert_eq!(toggle.arrow(1.0, false), Some(0.0));
        assert_eq!(toggle.arrow(0.0, false), None);

        let momentary = ButtonMode::Momentary;
        assert_eq!(momentary.press(0.0), Some(1.0));
        assert_eq!(momentary.release(), Some(0.0));
        assert_eq!(momentary.arrow(0.0, true), None);
        assert_eq!(momentary.arrow(1.0, false), None);
        assert_eq!(ButtonMode::from_value(0.0), ButtonMode::Toggle);
        assert_eq!(ButtonMode::from_value(1.0), ButtonMode::Momentary);
    }

    #[test]
    fn switching_a_button_to_momentary_clears_its_gate() {
        const fn parameter(name: &'static str) -> ParamSpec {
            ParamSpec {
                name,
                label: name,
                description: "A test parameter.",
                min: 0.0,
                max: 1.0,
                default: 0.0,
                unit: "",
                options: ParamOptions::DEFAULT,
            }
            .integer()
        }
        static BUTTONS: ModuleDescriptor = ModuleDescriptor {
            id: "test.buttons",
            version: 1,
            name: "Buttons",
            category: "Test",
            description: "Two buttons that share a mode.",
            inputs: &[],
            outputs: &[],
            params: &[parameter("mode"), parameter("on"), parameter("reset")],
            role: ModuleRole::Dsp(|_| unreachable!("never created")),
            panel: Panel::new(
                12,
                3,
                &[
                    Widget::button(0, 0, "on", "mode"),
                    Widget::button(6, 0, "reset", "mode"),
                ],
            ),
            displays: &[],
        };
        let cleared: Vec<usize> = BUTTONS.button_gates_to_clear("mode", 0.0, 1.0).collect();
        assert_eq!(cleared, vec![1, 2]);
        assert!(BUTTONS.button_gates_to_clear("mode", 1.0, 1.0).next().is_none());
        assert!(BUTTONS.button_gates_to_clear("mode", 1.0, 0.0).next().is_none());
        assert!(BUTTONS.button_gates_to_clear("on", 0.0, 1.0).next().is_none());
    }
}
