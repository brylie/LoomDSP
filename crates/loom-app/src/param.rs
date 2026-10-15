//! Slider steps and text for parameters. The mapping between values and
//! slider positions is `ParamSpec::normalize` and `denormalize` in
//! `loom-core`, shared with the engine's modulation.

use loom_core::ParamSpec;

/// Slider step in slider positions (0 to 1). Whole-number parameters move
/// one value per step, so the slider snaps to them.
pub fn slider_step(spec: &ParamSpec) -> f32 {
    if spec.integer && spec.max > spec.min {
        1.0 / (spec.max - spec.min)
    } else {
        0.001
    }
}

/// Value with a sensible number of digits and its unit.
pub fn format(spec: &ParamSpec, value: f32) -> String {
    let digits = match value.abs() {
        _ if spec.integer => 0,
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
mod tests {
    use super::*;

    const CUTOFF: ParamSpec = ParamSpec::new("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();
    const GAIN: ParamSpec = ParamSpec::new("gain", -10.0, 10.0, 1.0, "");

    #[test]
    fn whole_numbers_snap() {
        let channel = ParamSpec::new("channel", 0.0, 16.0, 0.0, "").integer();
        assert_eq!(channel.denormalize(0.52), 8.0);
        assert_eq!(format(&channel, 3.0), "3");
        assert!((slider_step(&channel) - 1.0 / 16.0).abs() < 1e-6);
    }

    #[test]
    fn whole_numbers_step_one_value_over_a_wide_range() {
        // The noise seed spans 1 to 65,535. One slider step is one seed.
        let seed = ParamSpec::new("seed", 1.0, 65_535.0, 1.0, "").integer();
        assert!((slider_step(&seed) - 1.0 / 65_534.0).abs() < 1e-9);
    }

    #[test]
    fn formats_with_units() {
        assert_eq!(format(&CUTOFF, 1000.0), "1000 Hz");
        assert_eq!(format(&GAIN, 0.25), "0.250");
    }
}
