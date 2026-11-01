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
}

impl PortDefault {
    /// What an unconnected input reads, in words, for help text.
    pub fn text(self) -> String {
        match self {
            Self::Value(v) => number_text(v),
            Self::BarPhase => "bar phase".to_string(),
            Self::BeatPhase => "beat phase".to_string(),
            Self::Playing => "play gate".to_string(),
        }
    }
}

/// Names joined as a sentence lists them: "A or B", "A, B, or C".
fn list_text(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [first, second] => format!("{first} or {second}"),
        [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
    }
}

/// A number as help text shows it: no trailing zeros, and 0 for -0, which
/// reads as 0 to people.
pub fn number_text(value: f32) -> String {
    if value == 0.0 {
        "0".to_string()
    } else {
        value.to_string()
    }
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
    /// True for a live gesture that is not part of the patch, such as a
    /// button's hold. It is sent to the engine like any parameter, but it is
    /// not saved, not an undo step, and not a target for modulation or MIDI
    /// learn. Only whole numbers with a default of 0 can be transient. Spec:
    /// `docs/design/transient-params.md`.
    pub transient: bool,
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
    /// When the parameter's widget shows on the tile: only while another
    /// choice parameter holds one of the listed choices, or always when
    /// `None`. Set with [`ParamSpec::shown_when`]. ADR-0030.
    pub shown_when: Option<ShownWhen>,
}

/// A condition on one choice parameter of the same module (ADR-0030).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShownWhen {
    /// The choice parameter's name.
    pub param: &'static str,
    /// The choices, by index from 0, under which the widget shows.
    pub choices: &'static [u32],
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
        shown_when: None,
        transient: false,
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
    /// A value of this parameter with its unit, such as "1000 Hz", for help
    /// text. The in-app help and the generated pages share it.
    pub fn value_text(&self, value: f32) -> String {
        if self.unit.is_empty() {
            number_text(value)
        } else {
            format!("{} {}", number_text(value), self.unit)
        }
    }

    /// The parameter's range in words, with its unit and options, such as
    /// "0.0005 to 10 s, logarithmic", for help text. A choice parameter's
    /// range is its choices, such as "ADSR or AD" (port-and-parameter-text.md,
    /// Module names and summaries).
    pub fn range_text(&self) -> String {
        let mut text = if self.options.choices.is_empty() {
            let mut text = format!("{} to {}", number_text(self.min), self.value_text(self.max));
            if self.options.integer {
                text.push_str(", whole numbers");
            }
            if self.options.mapping == ParamMapping::Logarithmic {
                text.push_str(", logarithmic");
            }
            text
        } else {
            list_text(self.options.choices)
        };
        if let Some(default) = self.default_range() {
            let names: Vec<String> = self.options.ranges.iter().map(ToString::to_string).collect();
            text.push_str(&format!(". Control ranges {} (default {default})", names.join(", ")));
        }
        if self.is_transient() {
            text.push_str(", not saved");
        }
        text
    }

    /// The default value in words: the default choice's name for a choice
    /// parameter, otherwise the value with its unit.
    pub fn default_text(&self) -> String {
        let index = (self.default - self.min).round();
        match self.options.choices.get(index as usize).filter(|_| index >= 0.0) {
            Some(name) => (*name).to_string(),
            None => self.value_text(self.default),
        }
    }

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

    /// Marks the parameter as transient: a live gesture that the patch does
    /// not save (`docs/design/transient-params.md`).
    pub const fn transient(mut self) -> Self {
        self.options.transient = true;
        self
    }

    /// True if the parameter is [transient](Self::transient).
    pub const fn is_transient(&self) -> bool {
        self.options.transient
    }

    /// Names each value of a whole-number parameter, from `min` up: one name
    /// per value, so `max - min + 1` names. The registry refuses names on a
    /// parameter that is not whole-number, a count that does not match, and
    /// an empty name. Spec: `docs/design/panels.md`, Choice names.
    pub const fn choices(mut self, names: &'static [&'static str]) -> Self {
        self.options.choices = names;
        self
    }

    /// Shows the parameter's widget only while the choice parameter `param`
    /// holds one of `choices`, counted from 0 (ADR-0030). The inspector
    /// still lists it. The registry refuses a `param` that is missing or
    /// has no choice names, and a choice it does not have.
    pub const fn shown_when(mut self, param: &'static str, choices: &'static [u32]) -> Self {
        self.options.shown_when = Some(ShownWhen { param, choices });
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

/// One of the eight groups a module belongs to (ADR-0029). It sets the
/// tile's header colour and the module's place in the browser. The
/// descriptor's `category` stays as a finer label for search and help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// Oscillators, noise, and other sound makers.
    Sources,
    /// Filters, shapers, delays, and other processors.
    FiltersAndEffects,
    /// Envelopes, LFOs, and random sources that move parameters.
    Modulation,
    /// The transport, clocks, phase, gates, and sequencers.
    TimeAndRhythm,
    /// Pitches, scales, and note effects.
    NotesAndPitch,
    /// Arithmetic, comparison, and gate logic.
    LogicAndMath,
    /// Levels, mixing, routing, and utilities that shape any signal.
    LevelAndRouting,
    /// Audio and MIDI in and out, displays, and controls for the player.
    Interface,
}

impl Family {
    /// Every family, in browser order.
    pub const ALL: [Self; 8] = [
        Self::Sources,
        Self::FiltersAndEffects,
        Self::Modulation,
        Self::TimeAndRhythm,
        Self::NotesAndPitch,
        Self::LogicAndMath,
        Self::LevelAndRouting,
        Self::Interface,
    ];

    /// The name users see.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sources => "Sources",
            Self::FiltersAndEffects => "Filters and effects",
            Self::Modulation => "Modulation",
            Self::TimeAndRhythm => "Time and rhythm",
            Self::NotesAndPitch => "Notes and pitch",
            Self::LogicAndMath => "Logic and math",
            Self::LevelAndRouting => "Level and routing",
            Self::Interface => "Interface",
        }
    }

    /// The family's place in [`Family::ALL`], which also indexes its
    /// colour in the theme's palette.
    pub const fn index(self) -> usize {
        self as usize
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
    /// A finer label than the family, for search and help.
    pub category: &'static str,
    /// The group that sets the tile's colour and browser place (ADR-0029).
    pub family: Family,
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
    /// for lights, an output name for meters and scopes.
    pub bind: &'static str,
    /// The tile's caption, when it differs from the bound item's label,
    /// such as "1" under each mixer strip's level (panels.md, Panel layout,
    /// principle 10). Hover text and the inspector keep the full label.
    /// `Some("")` shows no caption. Set with [`Widget::captioned`].
    pub caption: Option<&'static str>,
}

impl Widget {
    /// The same widget with its own tile caption (principle 10).
    pub const fn captioned(mut self, caption: &'static str) -> Self {
        self.caption = Some(caption);
        self
    }

    /// A knob for parameter `param`.
    pub const fn knob(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Knob,
            x,
            y,
            bind: param,
            caption: None,
        }
    }

    /// A toggle for a whole-number parameter from 0 to 1.
    pub const fn toggle(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Toggle,
            x,
            y,
            bind: param,
            caption: None,
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
            caption: None,
        }
    }

    /// A segmented control: one joined button per choice name, `width`
    /// grid units wide, for a parameter with 2 to 4 named values.
    pub const fn segmented(x: u8, y: u8, width: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Segmented { width },
            x,
            y,
            bind: param,
            caption: None,
        }
    }

    /// A compact choice list for a whole-number parameter with named values.
    pub const fn select(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Select,
            x,
            y,
            bind: param,
            caption: None,
        }
    }

    /// A light for display value `display`.
    pub const fn light(x: u8, y: u8, display: &'static str) -> Self {
        Self {
            kind: WidgetKind::Light,
            x,
            y,
            bind: display,
            caption: None,
        }
    }

    /// A level meter on output `output`, fed by a port probe.
    pub const fn meter(x: u8, y: u8, output: &'static str) -> Self {
        Self {
            kind: WidgetKind::Meter,
            x,
            y,
            bind: output,
            caption: None,
        }
    }

    /// A scope on output `output`: the signal over time, from a port probe
    /// (`docs/design/panels.md`, Drawing the scope trace).
    pub const fn scope(x: u8, y: u8, output: &'static str) -> Self {
        Self {
            kind: WidgetKind::Scope,
            x,
            y,
            bind: output,
            caption: None,
        }
    }

    /// A read-only result shape above its controls. `sample` maps the module's
    /// parameter values and a horizontal position in 0..1 to a vertical
    /// position in 0..1. It runs on the UI thread, never in audio processing.
    pub const fn shape(
        x: u8,
        y: u8,
        width: u8,
        output: &'static str,
        label: &'static str,
        sample: fn(&[f32], f32) -> f32,
    ) -> Self {
        Self {
            kind: WidgetKind::Shape { width, label, sample },
            x,
            y,
            bind: output,
            caption: None,
        }
    }

    /// A large button for whole-number parameters from 0 to 1.
    ///
    /// `mode` names a whole-number parameter, from 0 to 1, that chooses the
    /// gesture: 0 flips `param` (the saved latch) on press, and 1 holds
    /// `hold` high while the button is held. `hold` is usually
    /// [transient](ParamSpec::transient), so a hold is never saved. The
    /// three are different parameters. The canvas does not read `mode`. The
    /// editor does, through [`ButtonMode`]. It does not have to be a widget.
    /// Spec: `docs/design/clock.md`, Button details.
    pub const fn button(x: u8, y: u8, param: &'static str, hold: &'static str, mode: &'static str) -> Self {
        Self {
            kind: WidgetKind::Button { mode, hold },
            x,
            y,
            bind: param,
            caption: None,
        }
    }

    /// A button that is always momentary: `param` is high while it is held.
    /// It has no mode parameter (its `mode` is empty) and writes `param`
    /// only, which should be transient. The Transport's Reset uses it.
    pub const fn momentary(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Button { mode: "", hold: param },
            x,
            y,
            bind: param,
            caption: None,
        }
    }

    /// True for widgets bound to a parameter.
    pub const fn is_control(&self) -> bool {
        !matches!(
            self.kind,
            WidgetKind::Light | WidgetKind::Meter | WidgetKind::Scope | WidgetKind::Shape { .. }
        )
    }

    /// True for widgets bound to one of the module's outputs, fed by a
    /// port probe: meters and scopes.
    pub const fn shows_output(&self) -> bool {
        matches!(self.kind, WidgetKind::Meter | WidgetKind::Scope)
    }
}

/// The kinds of widget a panel can hold. More come with later phases.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum WidgetKind {
    /// A dial for any parameter.
    Knob,
    /// Off or on, for a whole-number parameter from 0 to 1.
    Toggle,
    /// One button per choice name, for a whole-number parameter that names
    /// its values. The names come from the parameter.
    Radio,
    /// A compact box that opens a list of named choices.
    Select,
    /// A row of joined buttons, one per choice name, for 2 to 4 named
    /// choices (panel-widgets.md, Segmented control).
    Segmented {
        /// Width in panel grid units. The segments share it equally.
        width: u8,
    },
    /// A display value as brightness, clamped to 0 to 1.
    Light,
    /// The level of one of the module's outputs, from a port probe: an RMS
    /// bar and a peak line, -60 dB to 0 dB.
    Meter,
    /// One of the module's outputs over time, from a port probe: a
    /// triggered trace in a frame.
    Scope,
    /// Read-only shape sampled from the module's parameters on the UI thread.
    Shape {
        /// Width in panel grid units.
        width: u8,
        /// Caption shown beneath the read-only result.
        label: &'static str,
        /// Pure sampler: parameter values, horizontal position, vertical position.
        sample: fn(&[f32], f32) -> f32,
    },
    /// A large button for whole-number parameters from 0 to 1.
    ///
    /// The widget's `bind` is the toggle latch. `mode` names the parameter
    /// that chooses the gesture, and `hold` the parameter a momentary press
    /// holds high.
    Button {
        /// Whole-number parameter from 0 to 1: 0 flips the latch on press,
        /// 1 holds `hold` while pressed.
        mode: &'static str,
        /// Whole-number parameter from 0 to 1 that is high while a
        /// momentary press lasts. Usually transient.
        hold: &'static str,
    },
}

impl PartialEq for WidgetKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Knob, Self::Knob)
            | (Self::Toggle, Self::Toggle)
            | (Self::Radio, Self::Radio)
            | (Self::Select, Self::Select)
            | (Self::Light, Self::Light)
            | (Self::Meter, Self::Meter)
            | (Self::Scope, Self::Scope) => true,
            (Self::Segmented { width: a }, Self::Segmented { width: b }) => a == b,
            // Kinds compare their visible form. Function addresses are not
            // stable identities for a shape sampler across codegen units.
            (
                Self::Shape {
                    width: a, label: al, ..
                },
                Self::Shape {
                    width: b, label: bl, ..
                },
            ) => a == b && al == bl,
            (Self::Button { mode: am, hold: ah }, Self::Button { mode: bm, hold: bh }) => am == bm && ah == bh,
            _ => false,
        }
    }
}

impl Eq for WidgetKind {}

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
    ///
    /// A `const fn`, so a module can name its ports by looking them up in
    /// its own descriptor at compile time. A wrong name is then a build
    /// error, and reordering the descriptor cannot leave an index behind:
    ///
    /// ```
    /// # static SVF: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
    /// #     id: "doc.svf", version: 1, name: "SVF", category: "Doc", family: loom_core::Family::FiltersAndEffects, description: "",
    /// #     inputs: &[], outputs: &[], params: &[], role: loom_core::ModuleRole::UnitDelay,
    /// #     panel: loom_core::Panel::EMPTY, displays: &[],
    /// # };
    /// # const _: Option<usize> = SVF.input_index("in");
    /// // In a module: const IN: usize = SVF.input_index("in").unwrap();
    /// ```
    pub const fn input_index(&self, name: &str) -> Option<usize> {
        port_position(self.inputs, name)
    }

    /// Index of the output with this name. A `const fn`, like
    /// [`input_index`](Self::input_index).
    pub const fn output_index(&self, name: &str) -> Option<usize> {
        port_position(self.outputs, name)
    }

    /// Index of the parameter with this name. A `const fn`, like
    /// [`input_index`](Self::input_index).
    pub const fn param_index(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.params.len() {
            if same_name(self.params[i].name, name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Width and height of `widget` in grid units. Radio buttons are one row
    /// per choice name of their parameter, plus one for the label, and wide
    /// enough that the built-in choice names are not cut (#394).
    pub fn widget_size(&self, widget: &Widget) -> (u8, u8) {
        match widget.kind {
            WidgetKind::Knob => (3, 3),
            WidgetKind::Meter => (3, 6),
            WidgetKind::Scope => (9, 5),
            WidgetKind::Shape { width, .. } => (width, 5),
            WidgetKind::Toggle | WidgetKind::Light => (3, 2),
            WidgetKind::Select => (6, 2),
            WidgetKind::Segmented { width } => (width, 2),
            WidgetKind::Button { .. } => (6, 3),
            WidgetKind::Radio => {
                let choices = self
                    .param_index(widget.bind)
                    .map_or(0, |index| self.params[index].options.choices.len());
                (6, u8::try_from(choices + 1).unwrap_or(u8::MAX))
            }
        }
    }

    /// True when parameter `index` shows its widget for these values
    /// (ADR-0030): always, unless it is `shown_when` a choice that is not
    /// the current one.
    pub fn is_shown(&self, index: usize, values: &[f32]) -> bool {
        let Some(when) = self.params.get(index).and_then(|p| p.options.shown_when) else {
            return true;
        };
        let Some(mode) = self.param_index(when.param) else {
            return true;
        };
        let value = values.get(mode).copied().unwrap_or(self.params[mode].default);
        let choice = (value - self.params[mode].min).round();
        when.choices.iter().any(|&c| c as f32 == choice)
    }

    /// Index of a display value by name.
    pub fn display_index(&self, name: &str) -> Option<usize> {
        self.displays.iter().position(|d| d.name == name)
    }

    /// Parameter values as the patch keeps them: each transient parameter
    /// at its default, every other value as given. Undo, copy, and paste
    /// use this, so a live gesture is never an edit or copied
    /// (transient-params.md).
    pub fn saved_values(&self, values: &[f32]) -> Vec<f32> {
        values
            .iter()
            .enumerate()
            .map(|(index, &value)| match self.params.get(index) {
                Some(spec) if spec.is_transient() => spec.default,
                _ => value,
            })
            .collect()
    }

    /// The parameter a control widget writes and shows, given the module's
    /// parameter values. A button in momentary mode uses its `hold`; every
    /// other control uses its `bind`. `None` if a name is missing or the
    /// widget is not a control. Spec: `docs/design/clock.md`, Button details.
    pub fn control_param(&self, widget: &Widget, params: &[f32]) -> Option<usize> {
        if !widget.is_control() {
            return None;
        }
        if let WidgetKind::Button { hold, .. } = widget.kind
            && self.button_mode(widget, params)? == ButtonMode::Momentary
        {
            return self.param_index(hold);
        }
        self.param_index(widget.bind)
    }

    /// How a button widget answers a gesture, from its mode parameter. A
    /// button with no mode parameter ([`Widget::momentary`]) is always
    /// momentary. `None` for another widget or a missing name.
    pub fn button_mode(&self, widget: &Widget, params: &[f32]) -> Option<ButtonMode> {
        let WidgetKind::Button { mode, .. } = widget.kind else {
            return None;
        };
        if mode.is_empty() {
            return Some(ButtonMode::Momentary);
        }
        let value = params.get(self.param_index(mode)?).copied()?;
        Some(ButtonMode::from_value(value))
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
            if let Some(when) = param.options.shown_when {
                let problem = |text: &str| {
                    Err(format!(
                        "parameter `{}`: shown when `{}`: {text}",
                        param.name, when.param
                    ))
                };
                let Some(mode) = self.param_index(when.param).map(|i| &self.params[i]) else {
                    return problem("no such parameter");
                };
                // Choice names, or an on and off switch (0 or 1).
                let count = if mode.options.choices.is_empty() {
                    if mode.options.integer && mode.min == 0.0 && mode.max == 1.0 {
                        2
                    } else {
                        0
                    }
                } else {
                    mode.options.choices.len()
                };
                if count == 0 {
                    return problem("needs a parameter with choice names, or a whole number from 0 to 1");
                }
                if when.choices.is_empty() || when.choices.iter().any(|&c| c as usize >= count) {
                    return problem("names a choice the parameter does not have");
                }
            }
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
                let gate = |p: &ParamSpec| p.options.integer && p.min == 0.0 && p.max == 1.0;
                if let WidgetKind::Button { mode, hold } = widget.kind
                    && mode.is_empty()
                {
                    // Always momentary: one parameter, the hold.
                    if hold != widget.bind {
                        return Err(name("a button without a mode must hold its own parameter"));
                    }
                } else if let WidgetKind::Button { mode, hold } = widget.kind {
                    if mode == widget.bind || mode == hold {
                        return Err(name("mode must be a different parameter"));
                    }
                    if hold == widget.bind {
                        return Err(name("hold must be a different parameter"));
                    }
                    let Some(mode_index) = self.param_index(mode) else {
                        return Err(name("no such mode parameter"));
                    };
                    if !gate(&self.params[mode_index]) {
                        return Err(name("mode must be a whole-number parameter from 0 to 1"));
                    }
                    let Some(hold_index) = self.param_index(hold) else {
                        return Err(name("no such hold parameter"));
                    };
                    if !gate(&self.params[hold_index]) {
                        return Err(name("hold must be a whole-number parameter from 0 to 1"));
                    }
                }
                let param = &self.params[index];
                let integer = param.options.integer;
                match widget.kind {
                    WidgetKind::Toggle if !(integer && param.min == 0.0 && param.max == 1.0) => {
                        return Err(name("a toggle needs a whole-number parameter from 0 to 1"));
                    }
                    WidgetKind::Radio if !integer || param.options.choices.is_empty() => {
                        return Err(name("radio buttons need a whole-number parameter with choice names"));
                    }
                    WidgetKind::Segmented { width }
                        if !integer || !(2..=4).contains(&param.options.choices.len()) || width == 0 =>
                    {
                        return Err(name(
                            "a segmented control needs a width and 2 to 4 named whole-number choices",
                        ));
                    }
                    WidgetKind::Select
                        if !integer || param.options.choices.is_empty() || param.options.choices.len() > 12 =>
                    {
                        return Err(name("a select list needs 1 to 12 named whole-number choices"));
                    }
                    WidgetKind::Button { .. } if !(integer && param.min == 0.0 && param.max == 1.0) => {
                        return Err(name("a button needs a whole-number parameter from 0 to 1"));
                    }
                    _ => {}
                }
            } else if widget.shows_output() || matches!(widget.kind, WidgetKind::Shape { .. }) {
                if !self.outputs.iter().any(|port| port.name == widget.bind) {
                    return Err(name("no such output"));
                }
                if let WidgetKind::Shape { width, label, .. } = widget.kind
                    && (width == 0 || label.trim().is_empty())
                {
                    return Err(name("a shape needs a width and a caption"));
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

/// Index of the port named `name` in `ports`.
const fn port_position(ports: &[PortSpec], name: &str) -> Option<usize> {
    let mut i = 0;
    while i < ports.len() {
        if same_name(ports[i].name, name) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `a == b`, in a `const fn`, where `str` equality is not available yet.
const fn same_name(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
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
mod tests;
