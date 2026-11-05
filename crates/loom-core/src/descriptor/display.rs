//! Display value descriptions: range, format, and how a value travels.

use super::format::note_text;

/// One value a module publishes for the lights on its panel (ADR-0013).
///
/// Write it as a struct literal, so every field is named and a missing one
/// is a compile error, as for [`PortSpec`](super::PortSpec):
///
/// ```
/// use loom_core::{DisplayKind, DisplayRange, DisplaySpec};
///
/// const GATE: DisplaySpec = DisplaySpec {
///     name: "gate",
///     label: "Gate",
///     description: "Lit while a key is held.",
///     kind: DisplayKind::Level,
///     range: DisplayRange::UNIT,
/// };
/// ```
///
/// The name is the stable ID that panels and [`Module::displays`](crate::Module::displays)
/// use. The label is the light's caption, and the description its hover
/// text. Spec: `docs/design/port-and-parameter-text.md`, Display values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplaySpec {
    /// Stable name that panel widgets bind to.
    pub name: &'static str,
    /// Short display name, for example "Gate". The caption of its light.
    pub label: &'static str,
    /// What the value shows, in one short sentence that fits a tooltip.
    pub description: &'static str,
    /// How the engine stores it and the UI reads it.
    pub kind: DisplayKind,
    /// The values it can take, for a meter bound to it.
    pub range: DisplayRange,
}

/// The values a display value can take. A meter bound to the value spans this
/// range. If the range holds 0, the meter's bar starts from there. Spec:
/// `docs/design/panel-widgets.md`, Meter on a display value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayRange {
    /// The lowest value.
    pub min: f32,
    /// The highest value.
    pub max: f32,
    /// The unit in hover text and on a readout, such as "dB". May be empty.
    pub unit: &'static str,
    /// How a readout writes the value.
    pub format: DisplayFormat,
}

/// How a readout writes a display value (panel-widgets.md, Readout).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayFormat {
    /// A plain number with this many decimals, and the unit.
    Number {
        /// Digits after the decimal point.
        decimals: u8,
    },
    /// A pitch as a note name and cents, at 0.1 per octave with 0.0 as middle
    /// C, such as "A4 +12¢".
    Note,
    /// A name for each whole value, counted from the range's lowest.
    Choices(&'static [&'static str]),
}

impl DisplayRange {
    /// From 0 to 1 with no unit: a brightness, or a level as a factor.
    pub const UNIT: Self = Self {
        min: 0.0,
        max: 1.0,
        unit: "",
        format: DisplayFormat::Number { decimals: 2 },
    };

    /// What a readout shows for `value`. Text that does not name a number,
    /// such as for NaN, is "--", because a readout never fails.
    pub fn text(&self, value: f32) -> String {
        if !value.is_finite() {
            return "--".to_string();
        }
        match self.format {
            DisplayFormat::Number { decimals } => {
                let digits = usize::from(decimals);
                let number = format!("{value:.digits$}");
                // A small negative value rounds to "-0.0". Around silence the
                // text would flip between that and "0.0".
                let number = match number.strip_prefix('-') {
                    Some(rest) if !rest.contains(|c: char| matches!(c, '1'..='9')) => rest,
                    _ => number.as_str(),
                };
                if self.unit.is_empty() {
                    number.to_string()
                } else {
                    format!("{number} {}", self.unit)
                }
            }
            DisplayFormat::Note => note_text(value),
            DisplayFormat::Choices(names) => {
                let index = (value - self.min).round().max(0.0) as usize;
                names
                    .get(index.min(names.len().saturating_sub(1)))
                    .copied()
                    .unwrap_or("--")
                    .to_string()
            }
        }
    }

    /// Where `value` falls in the range, from 0 to 1.
    pub fn position(&self, value: f32) -> f32 {
        ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// The value at `position`, the opposite of [`DisplayRange::position`].
    pub fn value(&self, position: f32) -> f32 {
        self.min + position * (self.max - self.min)
    }

    /// Where a meter's bar starts, from 0 to 1: the position of 0, or the
    /// bottom of the range if the range does not hold 0.
    pub fn zero(&self) -> f32 {
        self.position(0.0)
    }
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
