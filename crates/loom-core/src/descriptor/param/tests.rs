use super::*;
use crate::descriptor::fixtures::*;

const SEED: ParamSpec = param("seed", 1.0, 65_535.0, 1.0, "").integer();

#[test]
fn mappings_round_trip() {
    for spec in [CUTOFF, GAIN, SEED] {
        for value in [spec.min, spec.default, spec.max] {
            let back = spec.denormalize(spec.normalize(value));
            assert!(
                (back - value).abs() <= value.abs() * 1e-4 + 1e-4,
                "{}: {value} -> {back}",
                spec.name
            );
        }
    }
}

#[test]
fn linear_is_the_default() {
    assert_eq!(GAIN.options.mapping, ParamMapping::Linear);
    assert_eq!(GAIN.denormalize(0.5), 0.0);
    assert_eq!(GAIN.normalize(5.0), 0.75);
}

#[test]
fn logarithmic_steps_are_equal_ratios() {
    // The middle of the knob is the geometric middle of the range.
    assert!((CUTOFF.denormalize(0.5) - 632.46).abs() < 0.1);
    // A tenth of the knob is about one octave, anywhere on it.
    for start in [0.0, 0.3, 0.9] {
        let ratio = CUTOFF.denormalize(start + 0.1) / CUTOFF.denormalize(start);
        assert!((ratio - 1_000f32.powf(0.1)).abs() < 1e-3, "{ratio}");
        assert!((ratio - 2.0).abs() < 0.01);
    }
}

#[test]
fn whole_numbers_round_and_step_evenly() {
    assert_eq!(SEED.denormalize(0.5), 32_768.0);
    assert_eq!(SEED.denormalize(SEED.normalize(65_000.0)), 65_000.0);
    let step = 1.0 / (SEED.max - SEED.min);
    assert_eq!(SEED.denormalize(SEED.normalize(100.0) + step), 101.0);
}

const RANGES: &[ParamRange] = &[
    ParamRange::new(-0.1, 0.1),
    ParamRange::new(-1.0, 1.0),
    ParamRange::new(-5.0, 5.0),
];

const WIDE: ParamSpec = param("gain", -10.0, 10.0, 1.0, "").ranges(RANGES, ParamRange::new(-1.0, 1.0));

/// A selected range maps its bounds to positions 0 and 1, and clamps
/// values outside it and NaN as the full range does.
#[test]
fn a_selected_range_replaces_the_bounds() {
    let fine = WIDE.in_range(RANGES[0]);
    assert_eq!(fine.normalize(-0.1), 0.0);
    assert_eq!(fine.normalize(0.1), 1.0);
    assert_eq!(fine.normalize(0.05), 0.75);
    assert_eq!(fine.denormalize(0.5), 0.0);
    assert_eq!(fine.normalize(5.0), 1.0);
    assert_eq!(fine.clamp(5.0), 0.1);
    // The default is clamped into the range, for NaN and reset.
    assert_eq!(fine.denormalize(f32::NAN), 0.1);
    assert_eq!(fine.clamp(f32::NAN), 0.1);
    // 100 times finer than the full range.
    let full = WIDE.in_range(ParamRange::new(-10.0, 10.0));
    let step = |spec: ParamSpec| spec.denormalize(0.51) - spec.denormalize(0.5);
    assert!((step(full) / step(fine) - 100.0).abs() < 1e-2);
    assert_eq!(RANGES[0].to_string(), "-0.1 to 0.1");
    assert_eq!(ParamRange::new(0.0, 10.0).to_string(), "0 to 10");
}

/// Rule 7: a load never changes the value. It picks the default range,
/// the saved one, or the smallest that holds the value.
#[test]
fn a_loaded_range_holds_the_saved_value() {
    let (default, fine, half) = (RANGES[1], RANGES[0], RANGES[2]);
    assert_eq!(WIDE.default_range(), Some(default));
    assert_eq!(WIDE.load_range(None, 0.5), Some((default, false)));
    assert_eq!(WIDE.load_range(None, 3.0), Some((half, false)));
    assert_eq!(WIDE.load_range(Some(fine), 0.05), Some((fine, false)));
    // A saved range that does not hold the value, or is not offered.
    assert_eq!(WIDE.load_range(Some(fine), 5.0), Some((half, true)));
    assert_eq!(
        WIDE.load_range(Some(ParamRange::new(-2.0, 2.0)), 0.5),
        Some((default, true))
    );
    // No range in the list holds it: the full range.
    assert_eq!(WIDE.load_range(None, 8.0), Some((ParamRange::new(-10.0, 10.0), false)));
    assert_eq!(GAIN.load_range(None, 1.0), None);
    assert_eq!(WIDE.check_ranges(), Ok(()));
}

/// A named value reads its name, after clamping and rounding. A
/// parameter without names has none.
#[test]
fn choice_names_follow_the_value() {
    let slope = param("slope", 1.0, 4.0, 1.0, "")
        .integer()
        .choices(&["6 dB", "12 dB", "18 dB", "24 dB"]);
    assert_eq!(slope.choice_name(1.0), Some("6 dB"));
    assert_eq!(slope.choice_name(3.6), Some("24 dB"));
    assert_eq!(slope.choice_name(-5.0), Some("6 dB"));
    assert_eq!(slope.choice_name(f32::NAN), Some("6 dB"), "NaN is the default");
    assert_eq!(GAIN.choice_name(1.0), None);
    assert_eq!(slope.check_choices(), Ok(()));
    assert_eq!(GAIN.check_choices(), Ok(()));
}

#[test]
fn results_are_always_valid() {
    for spec in [CUTOFF, GAIN, SEED] {
        assert_eq!(spec.normalize(spec.max * 10.0), 1.0);
        assert_eq!(spec.normalize(-1e9), 0.0);
        assert_eq!(spec.denormalize(2.0), spec.max);
        assert_eq!(spec.denormalize(-1.0), spec.min);
        assert_eq!(spec.denormalize(f32::NAN), spec.default);
        assert_eq!(spec.normalize(f32::NAN), spec.normalize(spec.default));
    }
    let fixed = param("fixed", 1.0, 1.0, 1.0, "");
    assert_eq!(fixed.normalize(1.0), 0.0);
    assert_eq!(fixed.denormalize(0.5), 1.0);
}

/// A typed value takes its unit and `k` in the forms players type, reads a
/// comma as a decimal point, rounds whole numbers, and clamps. Anything else is
/// refused by name (panel-widgets.md, Typing a value).
#[test]
fn typed_values_read_units_k_and_commas() {
    let hz = param("f", 20.0, 20_000.0, 1_000.0, "Hz");
    for (text, value) in [
        ("440", 440.0),
        ("440 Hz", 440.0),
        ("440Hz", 440.0),
        ("440 hz", 440.0),
        ("  440  ", 440.0),
        ("2k", 2000.0),
        ("2K", 2000.0),
        ("2k Hz", 2000.0),
        ("2kHz", 2000.0),
        ("2 kHz", 2000.0),
        ("1,5k", 1500.0),
        ("1000000", 20_000.0),
        ("-5", 20.0),
    ] {
        assert_eq!(hz.parse_value_text(text), Ok(value), "{text}");
    }
    assert_eq!(param("g", -60.0, 6.0, 0.0, "dB").parse_value_text("-6 dB"), Ok(-6.0));
    assert_eq!(
        param("n", 0.0, 10.0, 0.0, "").integer().parse_value_text("3.6"),
        Ok(4.0)
    );
    let whole = param("n", 0.0, 10.0, 0.0, "").integer();
    assert!(whole.parse_value_text("-0.4").unwrap().is_sign_positive(), "never -0");
    let around_zero = param("p", -1.0, 1.0, 0.0, "");
    assert!(around_zero.parse_value_text("-0").unwrap().is_sign_positive());

    for text in ["", " ", "-", "k", "abc", "NaN", "inf", "-inf", "Hz", "1 2", "1e99"] {
        let error = hz.parse_value_text(text).unwrap_err();
        assert_eq!(error.to_string(), "Not a number in Hz", "{text}");
    }
    let plain = param("p", 0.0, 1.0, 0.0, "");
    assert_eq!(plain.parse_value_text("abc").unwrap_err().to_string(), "Not a number");
}

/// A 0 to 2 level in decibels, as on the Mixer.
const LEVEL: ParamSpec = param("level", 0.0, 2.0, 1.0, "").decibels();

/// The decibel mapping keeps its bottom, its 0 dB, and every gain above the
/// floor (modulation.md, Acceptance criteria: parameter mapping).
#[test]
fn decibels_map_position_and_gain() {
    assert_eq!(LEVEL.denormalize(0.0), 0.0);
    assert_eq!(LEVEL.normalize(0.0), 0.0);
    assert_eq!(LEVEL.denormalize(1.0), 2.0);
    let top = 20.0 * 2.0_f32.log10();
    assert!((LEVEL.normalize(1.0) - 60.0 / (60.0 + top)).abs() < 1e-5);
    for db in [-59.0_f32, -40.0, -12.0, -6.0, -0.5, 0.0, 3.0, 6.0] {
        let gain = 10.0_f32.powf(db / 20.0);
        let back = 20.0 * LEVEL.denormalize(LEVEL.normalize(gain)).log10();
        assert!((back - db).abs() < 1e-4, "{db} dB came back as {back} dB");
    }
}

/// Just above the bottom of the travel is the floor, and a gain below it
/// puts the control at the bottom.
#[test]
fn decibels_start_at_the_floor() {
    let just_above = LEVEL.denormalize(1e-6);
    assert!((20.0 * just_above.log10() + 60.0).abs() < 0.01, "{just_above}");
    assert_eq!(LEVEL.normalize(1e-4), 0.0);
    let deep: ParamSpec = param("level", 0.0, 1.0, 1.0, "").decibels_from(-80.0);
    assert!((20.0 * deep.denormalize(1e-6).log10() + 80.0).abs() < 0.01);
}

/// Bad inputs give a valid value, as for the other mappings.
#[test]
fn decibels_handle_bad_inputs() {
    assert_eq!(LEVEL.denormalize(f32::NAN), 1.0);
    assert_eq!(LEVEL.denormalize(-1.0), 0.0);
    assert_eq!(LEVEL.denormalize(2.0), 2.0);
    assert_eq!(LEVEL.normalize(9.0), 1.0);
    assert_eq!(LEVEL.normalize(-1.0), 0.0);
}

/// Decibel text uses the true minus sign, a plus for a boost, and one plain
/// zero, and it names the range for help.
#[test]
fn decibel_text_reads_as_the_spec_shows() {
    assert_eq!(LEVEL.value_text(1.0), "0.0 dB");
    assert_eq!(LEVEL.value_text(0.0), "−∞ dB");
    assert_eq!(LEVEL.value_text(2.0), "+6.0 dB");
    assert_eq!(LEVEL.value_text(0.5), "−6.0 dB");
    assert_eq!(LEVEL.value_text(1e-4), "−80.0 dB");
    assert_eq!(LEVEL.value_text(0.999_999), "0.0 dB");
    assert_eq!(LEVEL.range_text(), "−∞ to +6.0 dB");
    assert_eq!(LEVEL.default_text(), "0.0 dB");
}

/// A player can type a value in dB in every form the spec names, a plain
/// number is in dB, and text that is not a number is refused.
#[test]
fn typed_decibels_set_the_gain() {
    let close = |text: &str, gain: f32| {
        let got = LEVEL.parse_value_text(text).unwrap();
        assert!((got - gain).abs() < 1e-3, "{text} gave {got}");
    };
    close("-6 dB", 0.501);
    close("-6dB", 0.501);
    close("−6.0 dB", 0.501);
    close("+3 db", 1.413);
    close("-6,0", 0.501);
    close("0", 1.0);
    close("-inf", 0.0);
    close("−∞", 0.0);
    close("−∞ dB", 0.0);
    close("12 dB", 2.0);
    let below = LEVEL.parse_value_text("-70 dB").unwrap();
    assert!((below - 10.0_f32.powf(-3.5)).abs() < 1e-6, "{below}");
    for bad in ["", "dB", "loud", "inf", "NaN", "3 Hz"] {
        let error = LEVEL.parse_value_text(bad).unwrap_err();
        assert_eq!(error.to_string(), "Not a number in dB", "{bad}");
    }
}

/// Whatever the UI shows parses back to the same text, so committing what a
/// field shows never moves the value by more than the text's rounding.
#[test]
fn decibel_text_parses_back() {
    for gain in [0.0_f32, 1e-4, 0.001, 0.25, 0.5, 0.7, 1.0, 1.5, 2.0] {
        let text = LEVEL.value_text(gain);
        let back = LEVEL.parse_value_text(&text).unwrap();
        assert_eq!(LEVEL.value_text(back), text, "{gain}");
    }
}
