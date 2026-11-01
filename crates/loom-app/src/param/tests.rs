use super::*;

/// A parameter for test descriptors.
const fn param(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> loom_core::ParamSpec {
    loom_core::ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min,
        max,
        default,
        unit,
        options: loom_core::ParamOptions::DEFAULT,
    }
}

const CUTOFF: ParamSpec = param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();
const GAIN: ParamSpec = param("gain", -10.0, 10.0, 1.0, "");

#[test]
fn whole_numbers_snap() {
    let channel = param("channel", 0.0, 16.0, 0.0, "").integer();
    assert_eq!(channel.denormalize(0.52), 8.0);
    assert_eq!(format(&channel, 3.0), "3");
    assert!((slider_step(&channel) - 1.0 / 16.0).abs() < 1e-6);
}

#[test]
fn whole_numbers_step_one_value_over_a_wide_range() {
    // The noise seed spans 1 to 65,535. One slider step is one seed.
    let seed = param("seed", 1.0, 65_535.0, 1.0, "").integer();
    assert!((slider_step(&seed) - 1.0 / 65_534.0).abs() < 1e-9);
}

/// A parameter that names its values shows the name, never the number.
#[test]
fn named_values_show_their_names() {
    let mode = param("mode", 0.0, 1.0, 0.0, "")
        .integer()
        .choices(&["Toggle", "Momentary"]);
    assert_eq!(format(&mode, 0.0), "Toggle");
    assert_eq!(format(&mode, 1.0), "Momentary");
    // Clamped and rounded first, as the engine does.
    assert_eq!(format(&mode, 0.7), "Momentary");
    assert_eq!(format(&mode, 5.0), "Momentary");
}

#[test]
fn formats_with_units() {
    assert_eq!(format(&CUTOFF, 1000.0), "1000 Hz");
    assert_eq!(format(&GAIN, 0.25), "0.250");
}
