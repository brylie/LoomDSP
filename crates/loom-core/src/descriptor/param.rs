//! Parameter descriptions: range, mapping, options, and text.

use super::format::{db_to_gain, decibel_text, gain_to_db, list_text, number_text, parse_decibels};
use super::lint::is_blank;

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
    /// What one arrow key press adds on a number widget, in the parameter's
    /// unit, for a value whose useful step is a whole unit, not 1% of its
    /// range. Shift is 10 times finer. Set with [`ParamSpec::key_step`]
    /// (panel-widgets.md, Number).
    pub key_step: Option<f32>,
}

/// A condition on one choice parameter of the same module (ADR-0030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        key_step: None,
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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum ParamMapping {
    /// Equal steps of the control give equal steps of the value.
    #[default]
    Linear,
    /// Equal steps of the control give equal ratios of the value, as suits
    /// frequencies and times. Needs a minimum above 0, and not a whole
    /// number; the registry refuses others.
    Logarithmic,
    /// The control turns and reads in decibels, but the value stays a linear
    /// gain: 0 at the bottom, then a straight line in dB from `floor` to the
    /// maximum. Needs a minimum of 0, a maximum above 0, not a whole number,
    /// and no control ranges; the registry refuses others.
    Decibels {
        /// The dB value just above the bottom of the control's travel.
        floor: f32,
    },
}

/// The floor of a decibel mapping unless a parameter sets another.
pub const DEFAULT_DECIBEL_FLOOR: f32 = -60.0;

/// Why typed text is not a value of a parameter. Its text is what the field
/// shows, such as "Not a number in Hz".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotANumber {
    unit: &'static str,
}

impl std::fmt::Display for NotANumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.unit.is_empty() {
            write!(f, "Not a number")
        } else {
            write!(f, "Not a number in {}", self.unit)
        }
    }
}

impl ParamSpec {
    /// How many values a whole-number parameter can take: its names, or the
    /// numbers in its range.
    pub fn value_count(&self) -> usize {
        if self.options.choices.is_empty() {
            (self.max - self.min).max(0.0) as usize + 1
        } else {
            self.options.choices.len()
        }
    }

    /// What a select list shows for each value of a whole-number parameter:
    /// its names, or its numbers with the unit, such as "4 voices"
    /// (panel-widgets.md, Select list).
    pub fn value_names(&self) -> Vec<String> {
        if self.options.choices.is_empty() {
            (0..self.value_count())
                .map(|i| self.value_text(self.min + i as f32))
                .collect()
        } else {
            self.options.choices.iter().map(ToString::to_string).collect()
        }
    }

    /// A typed value: a number in this parameter's unit, optionally followed
    /// by that unit, in any case and with or without a space. A `k` after the
    /// number multiplies by 1000, and a comma is read as a decimal point. The
    /// result is rounded for a whole-number parameter and clamped to this
    /// spec's range, so parse with the descriptor's spec to reach the whole
    /// range. Spec: `docs/design/panel-widgets.md`, Typing a value.
    pub fn parse_value_text(&self, text: &str) -> Result<f32, NotANumber> {
        if matches!(self.options.mapping, ParamMapping::Decibels { .. }) {
            return match parse_decibels(text) {
                Some(gain) => Ok(self.clamp(gain) + 0.0),
                None => Err(NotANumber { unit: "dB" }),
            };
        }
        let bad = NotANumber { unit: self.unit };
        let text = text.trim().replace(',', ".").to_lowercase();
        let unit = self.unit.to_lowercase();
        let number = match text.strip_suffix(unit.as_str()) {
            Some(rest) if !unit.is_empty() => rest.trim_end(),
            _ => text.as_str(),
        };
        let (digits, scale) = match number.strip_suffix('k') {
            Some(rest) => (rest.trim_end(), 1000.0),
            None => (number, 1.0),
        };
        let value = digits.parse::<f32>().map_err(|_| bad)? * scale;
        // Rust reads "NaN" and "inf" as numbers. A player never means them.
        if value.is_finite() {
            // Adding zero turns -0.0, from "-0.4" on a whole number, into 0.0,
            // so the control never shows "-0".
            Ok(self.clamp(value) + 0.0)
        } else {
            Err(bad)
        }
    }

    /// A value of this parameter with its unit, such as "1000 Hz", for help
    /// text. The in-app help and the generated pages share it.
    pub fn value_text(&self, value: f32) -> String {
        if matches!(self.options.mapping, ParamMapping::Decibels { .. }) {
            decibel_text(value)
        } else if self.unit.is_empty() {
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
            let lowest = if matches!(self.options.mapping, ParamMapping::Decibels { .. }) {
                "−∞".to_string()
            } else {
                number_text(self.min)
            };
            let mut text = format!("{lowest} to {}", self.value_text(self.max));
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

    /// Maps the parameter in decibels, for gains and levels, with the floor
    /// at −60 dB. The value stays a linear gain.
    pub const fn decibels(mut self) -> Self {
        self.options.mapping = ParamMapping::Decibels {
            floor: DEFAULT_DECIBEL_FLOOR,
        };
        self
    }

    /// Maps the parameter in decibels, as [`decibels`](Self::decibels) does,
    /// with the floor at `floor` dB.
    pub const fn decibels_from(mut self, floor: f32) -> Self {
        self.options.mapping = ParamMapping::Decibels { floor };
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

    /// Makes one arrow key press on a number widget add `step`, in the
    /// parameter's unit. The registry refuses it on a whole-number or
    /// logarithmic parameter.
    pub const fn key_step(mut self, step: f32) -> Self {
        self.options.key_step = Some(step);
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
            // A gain of 0 reads as negative infinity, so it clamps to 0 below.
            ParamMapping::Decibels { floor } => (gain_to_db(value) - floor) / (gain_to_db(self.max) - floor),
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
            ParamMapping::Decibels { .. } if t == 0.0 => 0.0,
            // The top of the travel is exactly the maximum, with no rounding
            // from going through dB and back.
            ParamMapping::Decibels { .. } if t == 1.0 => self.max,
            ParamMapping::Decibels { floor } => db_to_gain(floor + t * (gain_to_db(self.max) - floor)),
        };
        self.clamp(value)
    }
}

#[cfg(test)]
mod tests;
