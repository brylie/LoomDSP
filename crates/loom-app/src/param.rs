//! Slider steps and text for parameters. The mapping between values and
//! slider positions is `ParamSpec::normalize` and `denormalize` in
//! `loom-core`, shared with the engine's modulation.

use loom_core::ParamSpec;

/// Slider step in slider positions (0 to 1). Whole-number parameters move
/// one value per step, so the slider snaps to them.
pub fn slider_step(spec: &ParamSpec) -> f32 {
    if spec.options.integer && spec.max > spec.min {
        1.0 / (spec.max - spec.min)
    } else {
        0.001
    }
}

/// Value with a sensible number of digits and its unit, or its choice name
/// when the parameter names its values (panels.md, Choice names).
pub fn format(spec: &ParamSpec, value: f32) -> String {
    if let Some(name) = spec.choice_name(value) {
        return name.to_string();
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
