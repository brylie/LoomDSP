//! Slider steps and text for parameters. The mapping between values and
//! slider positions is `ParamSpec::normalize` and `denormalize` in
//! `loom-core`, shared with the engine's modulation.

use loom_core::{ParamMapping, ParamSpec};

/// Slider step in slider positions (0 to 1). Whole-number parameters move
/// one value per step, so the slider snaps to them.
pub fn slider_step(spec: &ParamSpec) -> f32 {
    if spec.options.integer && spec.max > spec.min {
        1.0 / (spec.max - spec.min)
    } else {
        0.001
    }
}

/// A linear parameter whose span (`max - min`) is under this shows
/// [`FINE_DIGITS`] decimals, so one 1% step of a span of 0.01 or more
/// changes the text (control-ranges.md, Showing the value).
const FINE_SPAN: f64 = 0.1;

/// Decimals on a span under [`FINE_SPAN`].
const FINE_DIGITS: usize = 4;

/// Most decimals any value shows. The rule never asks for more than
/// [`FINE_DIGITS`]; this guards against a span it cannot otherwise reach.
const MAX_DIGITS: usize = 6;

/// Value with a sensible number of digits and its unit, or its choice name
/// when the parameter names its values (panels.md, Choice names).
///
/// The span comes from `spec`, so callers pass the ranged spec
/// (`ParamSpec::in_range`, as `Node::spec` gives it). A linear parameter
/// on a span under 0.1 shows 4 decimals; otherwise the value's size picks
/// them, as it always has (control-ranges.md, Showing the value).
pub fn format(spec: &ParamSpec, value: f32) -> String {
    // Only a parameter with names is asked, because naming clamps the
    // value, and a clamp to bounds that are not a span would panic.
    if let Some(name) = Some(spec)
        .filter(|spec| !spec.options.choices.is_empty())
        .and_then(|spec| spec.choice_name(value))
    {
        return name.to_string();
    }
    if matches!(spec.options.mapping, ParamMapping::Decibels { .. }) {
        return spec.value_text(value);
    }
    let digits = digits(spec, value);
    if spec.unit.is_empty() {
        format!("{value:.digits$}")
    } else {
        format!("{value:.digits$} {}", spec.unit)
    }
}

/// The number a screen reader reports in parameter units. Use the same
/// rounding as the widget, except for decibels, which display a different unit.
pub(crate) fn spoken_value(spec: &ParamSpec, value: f32) -> f64 {
    if matches!(spec.options.mapping, ParamMapping::Decibels { .. }) {
        return decimal(value);
    }
    let digits = digits(spec, value);
    let scale = 10_f64.powi(digits as i32);
    (f64::from(value) * scale).round_ties_even() / scale
}

/// Widens the shortest decimal spelling, so a single-precision bound does
/// not acquire extra spoken digits.
pub(crate) fn decimal(value: f32) -> f64 {
    value.to_string().parse().expect("formatted float")
}

/// Decimals for `value` of a parameter that is not a choice or in
/// decibels. A whole number shows none. Otherwise the value's size gives
/// the floor, and a linear parameter on a fine span shows at least
/// [`FINE_DIGITS`]. The span is worked out in `f64`, so two finite `f32`
/// bounds cannot overflow, and a span that is not finite or not above 0
/// keeps the floor: an exposed range comes from a patch file, so the span
/// is not trusted.
fn digits(spec: &ParamSpec, value: f32) -> usize {
    if spec.options.integer {
        return 0;
    }
    let floor = size_digits(value);
    let span = f64::from(spec.max) - f64::from(spec.min);
    let fine =
        matches!(spec.options.mapping, ParamMapping::Linear) && span.is_finite() && span > 0.0 && span < FINE_SPAN;
    let digits = if fine { floor.max(FINE_DIGITS) } else { floor };
    digits.min(MAX_DIGITS)
}

/// Decimals from the value's size alone: 0 for 1,000 and up, 1 from 100,
/// 2 from 1, and 3 below 1.
fn size_digits(value: f32) -> usize {
    match value.abs() {
        v if v >= 1000.0 => 0,
        v if v >= 100.0 => 1,
        v if v >= 1.0 => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests;
