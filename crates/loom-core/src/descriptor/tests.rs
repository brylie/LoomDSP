use super::*;

/// A parameter for test descriptors.
const fn param(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min,
        max,
        default,
        unit,
        options: ParamOptions::DEFAULT,
    }
}

const CUTOFF: ParamSpec = param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic();
const GAIN: ParamSpec = param("gain", -10.0, 10.0, 1.0, "");
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
fn polarity_remap_follows_the_override() {
    assert_eq!(Polarity::Bipolar.remap(Polarity::Bipolar, -0.4), -0.4);
    assert_eq!(Polarity::Unipolar.remap(Polarity::Bipolar, -1.0), 0.0);
    assert_eq!(Polarity::Unipolar.remap(Polarity::Bipolar, 1.0), 1.0);
    assert_eq!(Polarity::Bipolar.remap(Polarity::Unipolar, 0.0), -1.0);
    assert_eq!(Polarity::Bipolar.remap(Polarity::Unipolar, 1.0), 1.0);
    // A source past the nominal range is kept. Only the parameter is clamped.
    assert_eq!(Polarity::Bipolar.remap(Polarity::Bipolar, 3.0), 3.0);
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

static DOCUMENTED: ModuleDescriptor = ModuleDescriptor {
    id: "test.documented",
    version: 1,
    name: "Documented",
    category: "Test",
    family: crate::Family::Interface,
    description: "",
    inputs: &[],
    outputs: &[],
    params: &[GAIN],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

static UNDOCUMENTED: ModuleDescriptor = ModuleDescriptor {
    id: "test.undocumented",
    version: 1,
    name: "Undocumented",
    category: "Test",
    family: crate::Family::Interface,
    description: "",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "",
        default: PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "\t",
        signal: SignalType::Audio,
        description: "The input.",
        default: PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[ParamSpec {
        label: "",
        description: "",
        ..GAIN
    }],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

#[test]
fn lists_empty_labels_and_descriptions_in_order() {
    assert_eq!(DOCUMENTED.first_undocumented(), None);
    assert!(DOCUMENTED.undocumented().is_empty());
    let missing: Vec<String> = UNDOCUMENTED.undocumented().iter().map(ToString::to_string).collect();
    assert_eq!(
        missing,
        [
            "input `in` has no description",
            "output `out` has no label",
            "parameter `gain` has no label",
            "parameter `gain` has no description",
        ]
    );
    assert_eq!(
        UNDOCUMENTED.first_undocumented(),
        Some(Undocumented {
            kind: SpecKind::Input,
            name: "in",
            missing: MissingText::Description,
        })
    );
}

#[test]
fn assert_documented_passes_documented_modules() {
    assert_documented(&[&DOCUMENTED]);
}

#[test]
#[should_panic(
    expected = "test.undocumented: input `in` has no description. Every port, parameter, and display value needs"
)]
fn assert_documented_names_the_first_missing_text() {
    assert_documented(&[&DOCUMENTED, &UNDOCUMENTED]);
}

#[test]
fn button_mode_turns_a_gesture_into_a_value() {
    let toggle = ButtonMode::Toggle;
    assert_eq!(toggle.press(0.0), Some(1.0));
    assert_eq!(toggle.press(1.0), Some(0.0));
    assert_eq!(toggle.release(), None);
    assert_eq!(toggle.arrow(0.0, true), Some(1.0));
    assert_eq!(toggle.arrow(1.0, true), None);
    assert_eq!(toggle.arrow(1.0, false), Some(0.0));
    assert_eq!(toggle.arrow(0.0, false), None);

    let momentary = ButtonMode::Momentary;
    assert_eq!(momentary.press(0.0), Some(1.0));
    assert_eq!(momentary.release(), Some(0.0));
    assert_eq!(momentary.arrow(0.0, true), None);
    assert_eq!(momentary.arrow(1.0, false), None);
    assert_eq!(ButtonMode::from_value(0.0), ButtonMode::Toggle);
    assert_eq!(ButtonMode::from_value(1.0), ButtonMode::Momentary);
}

/// A button writes its latch in toggle mode and its hold in momentary
/// mode. Other controls write their bind.
#[test]
fn a_button_writes_the_parameter_its_mode_chooses() {
    const fn parameter(name: &'static str) -> ParamSpec {
        ParamSpec {
            name,
            label: name,
            description: "A test parameter.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
    }
    static BUTTONS: ModuleDescriptor = ModuleDescriptor {
        id: "test.buttons",
        version: 1,
        name: "Buttons",
        category: "Test",
        family: crate::Family::Interface,
        description: "A button and a toggle.",
        inputs: &[],
        outputs: &[],
        params: &[parameter("mode"), parameter("on"), parameter("hold").transient()],
        role: ModuleRole::Dsp(|_| unreachable!("never created")),
        panel: Panel::new(
            9,
            3,
            &[Widget::button(0, 0, "on", "hold", "mode"), Widget::toggle(6, 0, "mode")],
        ),
        displays: &[],
    };
    let button = &BUTTONS.panel.widgets[0];
    assert_eq!(BUTTONS.control_param(button, &[0.0, 0.0, 0.0]), Some(1));
    assert_eq!(BUTTONS.control_param(button, &[1.0, 0.0, 0.0]), Some(2));
    let toggle = &BUTTONS.panel.widgets[1];
    assert_eq!(BUTTONS.control_param(toggle, &[1.0, 0.0, 0.0]), Some(0));
    assert!(BUTTONS.params[2].is_transient());
    assert!(!BUTTONS.params[1].is_transient());
    // Built at run time too, not only in statics.
    assert!(parameter("x").transient().is_transient());
    assert_eq!(Widget::button(0, 0, "on", "hold", "mode"), *button);
}

/// A named mode can share a panel with a full-width result shape. The
/// registry rejects invalid choice and shape declarations before drawing.
#[test]
fn select_and_shape_panel_bindings_are_checked() {
    fn sample(_: &[f32], x: f32) -> f32 {
        x
    }
    static OUTPUT: [PortSpec; 1] = [PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The shaped signal.",
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::PerChannel,
    }];
    static PARAMS: [ParamSpec; 1] = [param("mode", 0.0, 1.0, 0.0, "").integer().choices(&["First", "Second"])];
    static NO_CHOICES: [ParamSpec; 1] = [param("mode", 0.0, 1.0, 0.0, "").integer()];
    static VALID: [Widget; 2] = [
        Widget::shape(0, 0, 6, "out", "Result", sample),
        Widget::select(0, 5, "mode"),
    ];
    static BAD_SHAPE: [Widget; 1] = [Widget::shape(0, 0, 0, "out", "Result", sample)];

    let descriptor = |widgets, params| ModuleDescriptor {
        id: "test.choice_shape",
        version: 1,
        name: "Choice and shape",
        category: "Test",
        family: crate::Family::Interface,
        description: "A panel validation test.",
        inputs: &[],
        outputs: &OUTPUT,
        params,
        role: ModuleRole::Dsp(|_| unreachable!("never created")),
        panel: Panel::new(6, 7, widgets),
        displays: &[],
    };
    let valid = descriptor(&VALID, &PARAMS);
    assert!(valid.check_panel().is_ok());
    assert_eq!(valid.widget_size(&VALID[0]), (6, 5));
    assert_eq!(valid.widget_size(&VALID[1]), (6, 2));
    assert_eq!(Widget::select(0, 5, "mode"), VALID[1]);
    assert_eq!(Widget::shape(0, 0, 6, "out", "Result", sample), VALID[0]);

    assert!(
        descriptor(&VALID, &NO_CHOICES)
            .check_panel()
            .unwrap_err()
            .contains("select list")
    );
    assert!(
        descriptor(&BAD_SHAPE, &PARAMS)
            .check_panel()
            .unwrap_err()
            .contains("shape needs")
    );
}
