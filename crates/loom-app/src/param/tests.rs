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

/// The UI's text for the Mixer's levels is what the spec shows, because this
/// is the function the inspector and the tile call (modulation.md, Parameter
/// mapping).
#[test]
fn the_mixer_levels_read_in_decibels() {
    let mixer = loom_modules::ALL
        .iter()
        .find(|descriptor| descriptor.id == "loom.mix.mixer")
        .expect("the Mixer is built in");
    let level = mixer.params.iter().find(|spec| spec.name == "level1").expect("level1");
    assert_eq!(format(level, 1.0), "0.0 dB");
    assert_eq!(format(level, 0.0), "−∞ dB");
    assert_eq!(format(level, 2.0), "+6.0 dB");
}

/// The range from -0.01 to 0.01, finer than any module offers today; only a
/// sub-patch surface's exposed range gives it (control-ranges.md, Showing
/// the value).
const FINE: loom_core::ParamRange = loom_core::ParamRange::new(-0.01, 0.01);

/// On a span under 0.1 a linear value shows 4 decimals, so 0.0004 reads as
/// itself and not as "0.000" (control-ranges.md, Showing the value).
#[test]
fn a_fine_range_shows_four_decimals() {
    let fine = GAIN.in_range(FINE);
    assert_eq!(format(&fine, 0.0004), "0.0004");
    assert_eq!(format(&fine, 0.00041), "0.0004");
    assert_eq!(format(&fine, -0.01), "-0.0100");
    assert_eq!(
        format(&param("drift", -10.0, 10.0, 0.0, "st").in_range(FINE), 0.0004),
        "0.0004 st"
    );
    // Under 0.01, which only an exposed range gives, it still shows 4.
    let finer = GAIN.in_range(loom_core::ParamRange::new(0.0, 0.001));
    assert_eq!(format(&finer, 0.0004), "0.0004");
}

/// One 1% step of the knob, the arrow-key step, changes the text from any
/// place on the fine range and on every range Multiply's gain offers
/// (control-ranges.md, Which step is visible).
#[test]
fn every_one_percent_step_changes_the_text() {
    let gain = loom_modules::ALL
        .iter()
        .find(|descriptor| descriptor.id == "loom.math.mul")
        .and_then(|mul| mul.params.iter().find(|spec| spec.name == "gain"))
        .expect("Multiply's gain");
    assert!(!gain.options.ranges.is_empty(), "the gain offers ranges");
    for range in std::iter::once(FINE).chain(gain.options.ranges.iter().copied()) {
        let spec = gain.in_range(range);
        for step in 0..100 {
            let here = spec.denormalize(step as f32 / 100.0);
            let next = spec.denormalize((step + 1) as f32 / 100.0);
            assert_ne!(format(&spec, here), format(&spec, next), "{range} at step {step}");
        }
    }
}

/// Every span of 0.1 or more keeps today's text: 0 to 1 shows "1.00", and
/// -0.1 to 0.1 shows 3 decimals.
#[test]
fn other_ranges_keep_todays_text() {
    let unit = GAIN.in_range(loom_core::ParamRange::new(0.0, 1.0));
    assert_eq!(format(&unit, 1.0), "1.00");
    assert_eq!(format(&unit, 0.25), "0.250");
    let tenth = GAIN.in_range(loom_core::ParamRange::new(-0.1, 0.1));
    assert_eq!(format(&tenth, 0.05), "0.050");
    assert_eq!(format(&tenth, 0.0004), "0.000");
    let wide = GAIN.in_range(loom_core::ParamRange::new(-10.0, 10.0));
    assert_eq!(format(&wide, 5.0), "5.00");
    assert_eq!(format(&GAIN, 0.0004), "0.000", "the full range too");
}

/// A whole number shows no decimals on any range, fine or not.
#[test]
fn a_whole_number_shows_no_decimals_on_any_range() {
    let first = param("first", 0.0, 16.0, 1.0, "").integer();
    assert_eq!(format(&first, 1.0), "1");
    assert_eq!(format(&first.in_range(loom_core::ParamRange::new(1.0, 1.05)), 1.0), "1");
    assert_eq!(format(&first.in_range(loom_core::ParamRange::new(0.0, 2.0)), 1.0), "1");
}

/// A logarithmic or decibel parameter keeps today's text, even on a span
/// under 0.1.
#[test]
fn logarithmic_and_decibel_parameters_keep_todays_text() {
    let narrow = CUTOFF.in_range(loom_core::ParamRange::new(100.0, 100.05));
    assert_eq!(format(&narrow, 100.0), format(&CUTOFF, 100.0));
    assert_eq!(format(&narrow, 100.0), "100.0 Hz");
    let low = CUTOFF.in_range(loom_core::ParamRange::new(0.5, 0.55));
    assert_eq!(format(&low, 0.5004), "0.500 Hz");
    let level = param("level", 0.0, 2.0, 1.0, "").decibels();
    let narrow = level.in_range(loom_core::ParamRange::new(0.0, 0.05));
    assert_eq!(format(&narrow, 1.0), format(&level, 1.0));
}

/// A span that is 0, a subnormal, as wide as `f32` allows, or not a
/// number gives a short string, never a panic: an exposed range comes from
/// a patch file and is checked only by `is_span` (control-ranges.md, Bad
/// spans).
#[test]
fn odd_spans_give_a_short_string() {
    let spans = [
        (0.5, 0.5),
        (0.0, 1e-45),
        (-3e38, 3e38),
        (f32::MIN, f32::MAX),
        (f32::NAN, 1.0),
        (0.0, f32::NAN),
        (0.0, f32::INFINITY),
        (1.0, 0.0),
    ];
    for (min, max) in spans {
        let spec = ParamSpec { min, max, ..GAIN };
        for value in [0.0, 0.0004, 0.5, -0.5, f32::NAN] {
            let text = format(&spec, value);
            assert!(!text.is_empty() && text.len() <= 8, "{min} to {max}, {value}: {text:?}");
        }
    }
    let subnormal = ParamSpec {
        min: 0.0,
        max: 1e-45,
        ..GAIN
    };
    assert_eq!(format(&subnormal, 0.0), "0.0000", "a tiny span is still a span");
    let flat = ParamSpec {
        min: 0.5,
        max: 0.5,
        ..GAIN
    };
    assert_eq!(format(&flat, 0.5), "0.500", "a span of 0 uses today's rules");
}
