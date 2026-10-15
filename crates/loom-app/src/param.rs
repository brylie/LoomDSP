//! Maps parameter values to slider positions and text.

use loom_core::ParamSpec;

/// True when the parameter reads better on a logarithmic scale: positive
/// and spanning at least two decades, like cutoff in Hz or times in seconds.
/// Whole numbers (channels, counts, seeds) stay linear, so every value is
/// one equal step away from the next.
fn is_logarithmic(spec: &ParamSpec) -> bool {
    !spec.integer && spec.min > 0.0 && spec.max / spec.min >= 100.0
}

/// Value to slider position (0 to 1).
pub fn to_slider(spec: &ParamSpec, value: f32) -> f32 {
    let value = spec.clamp(value);
    let t = if is_logarithmic(spec) {
        (value / spec.min).ln() / (spec.max / spec.min).ln()
    } else {
        (value - spec.min) / (spec.max - spec.min)
    };
    t.clamp(0.0, 1.0)
}

/// Slider position (0 to 1) to value.
pub fn from_slider(spec: &ParamSpec, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let value = if is_logarithmic(spec) {
        spec.min * (spec.max / spec.min).powf(t)
    } else {
        spec.min + t * (spec.max - spec.min)
    };
    spec.clamp(value)
}

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

    const CUTOFF: ParamSpec = ParamSpec::new("cutoff", 20.0, 20_000.0, 1_000.0, "Hz");
    const GAIN: ParamSpec = ParamSpec::new("gain", -10.0, 10.0, 1.0, "");

    #[test]
    fn round_trips() {
        for spec in [CUTOFF, GAIN] {
            for value in [spec.min, spec.default, spec.max] {
                let back = from_slider(&spec, to_slider(&spec, value));
                assert!((back - value).abs() <= value.abs() * 1e-4 + 1e-4, "{value} -> {back}");
            }
        }
    }

    #[test]
    fn cutoff_is_logarithmic() {
        // The middle of the slider is the geometric middle of the range.
        assert!((from_slider(&CUTOFF, 0.5) - 632.46).abs() < 0.1);
        assert!((from_slider(&GAIN, 0.5) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn whole_numbers_snap() {
        let channel = ParamSpec::new("channel", 0.0, 16.0, 0.0, "").integer();
        assert_eq!(from_slider(&channel, 0.52), 8.0);
        assert_eq!(format(&channel, 3.0), "3");
        assert!((slider_step(&channel) - 1.0 / 16.0).abs() < 1e-6);
    }

    #[test]
    fn whole_numbers_are_linear_even_over_a_wide_range() {
        // The noise seed spans 1 to 65,535. On a logarithmic scale, one
        // slider step near the top would skip hundreds of seeds.
        let seed = ParamSpec::new("seed", 1.0, 65_535.0, 1.0, "").integer();
        assert!((slider_step(&seed) - 1.0 / 65_534.0).abs() < 1e-9);
        assert_eq!(from_slider(&seed, 0.5), 32_768.0);
        assert_eq!(from_slider(&seed, to_slider(&seed, 65_000.0)), 65_000.0);
    }

    #[test]
    fn formats_with_units() {
        assert_eq!(format(&CUTOFF, 1000.0), "1000 Hz");
        assert_eq!(format(&GAIN, 0.25), "0.250");
    }
}
