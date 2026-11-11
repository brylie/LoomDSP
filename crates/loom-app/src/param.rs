//! Slider steps and text for parameters. The mapping between values and
//! slider positions is `ParamSpec::normalize` and `denormalize` in
//! `loom-core`, shared with the engine's modulation.

use loom_core::{ParamMapping, ParamSpec};

/// Slider step in slider positions (0 to 1): what one arrow key press moves
/// the inspector's slider, as on the tile's knob, 1% of its travel.
/// Whole-number parameters move one value per step, so the slider snaps to
/// them. The toolkit's slider also snaps a drag to this step.
pub fn slider_step(spec: &ParamSpec) -> f32 {
    if spec.options.integer && spec.max > spec.min {
        1.0 / (spec.max - spec.min)
    } else {
        loom_canvas::ARROW_SHARE
    }
}

/// The slider's step with Shift held, as on the tile's knob: 0.1% of its
/// travel, for the keys and for a drag. One value for a whole-number
/// parameter.
pub fn slider_fine_step(spec: &ParamSpec) -> f32 {
    if spec.options.integer && spec.max > spec.min {
        slider_step(spec)
    } else {
        loom_canvas::ARROW_SHARE_FINE
    }
}

/// Value with a sensible number of digits and its unit, or its choice name
/// when the parameter names its values (panels.md, Choice names).
pub fn format(spec: &ParamSpec, value: f32) -> String {
    if let Some(name) = spec.choice_name(value) {
        return name.to_string();
    }
    if matches!(spec.options.mapping, ParamMapping::Decibels { .. }) {
        return spec.value_text(value);
    }
    let digits = match value.abs() {
        _ if spec.options.integer => 0,
        v if v >= 1000.0 => 0,
        v if v >= 100.0 => 1,
        v if v >= 1.0 => 2,
        _ => 3,
    };
    if spec.unit.is_empty() {
        format!("{value:.digits$}")
    } else {
        format!("{value:.digits$} {}", spec.unit)
    }
}

#[cfg(test)]
mod tests;
