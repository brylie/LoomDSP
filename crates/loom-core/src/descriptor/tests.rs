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
    fn sample(_: &[f32], x: f32, _: f32) -> f32 {
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
    static NOT_WHOLE: [ParamSpec; 1] = [param("mode", 0.0, 1.0, 0.0, "")];
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
        descriptor(&VALID, &NOT_WHOLE)
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

/// A port for [`NAMED`].
const fn port(name: &'static str) -> PortSpec {
    PortSpec {
        name,
        label: name,
        signal: SignalType::Audio,
        description: "A test port.",
        default: PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// An input and a parameter share the name `cutoff`, like on a filter.
static NAMED: ModuleDescriptor = ModuleDescriptor {
    id: "test.named",
    version: 1,
    name: "Named",
    category: "Test",
    family: crate::Family::Interface,
    description: "",
    inputs: &[port("in"), port("cutoff")],
    outputs: &[port("low"), port("high")],
    params: &[GAIN, CUTOFF],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

/// Looked up at compile time, the way modules name their indices.
const NAMED_CUTOFF: usize = NAMED.param_index("cutoff").unwrap();
const NAMED_CUTOFF_IN: usize = NAMED.input_index("cutoff").unwrap();
const NAMED_HIGH_OUT: usize = NAMED.output_index("high").unwrap();

/// Each lookup searches its own list, at compile time and at run time.
#[test]
fn indices_are_found_by_name_in_their_own_list() {
    assert_eq!((NAMED_CUTOFF, NAMED_CUTOFF_IN, NAMED_HIGH_OUT), (1, 1, 1));
    assert_eq!(NAMED.param_index("gain"), Some(0));
    assert_eq!(NAMED.input_index("in"), Some(0));
    assert_eq!(NAMED.output_index("low"), Some(0));
    // `in` is an input, not a parameter or an output.
    assert_eq!(NAMED.param_index("in"), None);
    assert_eq!(NAMED.output_index("in"), None);
}

/// Only the whole name matches: not a prefix, a longer name, or another case.
#[test]
fn only_the_exact_name_matches() {
    for name in ["cut", "cutoffs", "Cutoff", ""] {
        assert_eq!(NAMED.param_index(name), None, "{name:?}");
        assert_eq!(NAMED.input_index(name), None, "{name:?}");
    }
}

/// A select list takes names or numbers, up to 128 values, and only on a
/// whole-number parameter (panel-widgets.md, Select list).
#[test]
fn a_select_list_needs_a_whole_number_with_at_most_128_values() {
    static SELECT: [Widget; 1] = [Widget::select(0, 0, "p")];
    let spec = |min, max| param("p", min, max, min, "voices");
    assert_eq!(spec(1.0, 4.0).integer().value_count(), 4);
    assert_eq!(
        spec(1.0, 2.0).integer().value_names(),
        ["1 voices", "2 voices"],
        "numbers with the unit"
    );
    let named = spec(0.0, 1.0).integer().choices(&["Off", "On"]);
    assert_eq!(named.value_names(), ["Off", "On"]);

    let check = |param: ParamSpec| -> Result<(), String> {
        let params: &'static [ParamSpec] = Box::leak(Box::new([param]));
        let descriptor = ModuleDescriptor {
            params,
            panel: Panel::new(6, 2, &SELECT),
            ..NAMED
        };
        descriptor.check_panel().map_err(|error| error.to_string())
    };
    assert!(check(spec(1.0, 128.0).integer()).is_ok());
    let too_long = check(spec(1.0, 129.0).integer()).unwrap_err();
    assert!(too_long.contains("select list"), "{too_long}");
    assert!(check(spec(1.0, 4.0)).is_err(), "not a whole number");
}

/// A fader binds to any parameter, and the registry names it when the
/// parameter does not exist or the fader leaves the panel.
#[test]
fn a_fader_needs_a_parameter_and_room() {
    static PARAMS: [ParamSpec; 1] = [CUTOFF];
    static TALL: [Widget; 1] = [Widget::fader(0, 0, "cutoff")];
    static WIDE: [Widget; 1] = [Widget::fader_horizontal(0, 0, "cutoff")];
    static MISSING: [Widget; 1] = [Widget::fader(0, 0, "nothing")];
    let descriptor = |widgets, width, height| ModuleDescriptor {
        params: &PARAMS,
        panel: Panel::new(width, height, widgets),
        ..NAMED
    };
    assert!(descriptor(&TALL, 2, 6).check_panel().is_ok());
    assert!(descriptor(&WIDE, 6, 2).check_panel().is_ok());
    assert!(descriptor(&TALL, 2, 5).check_panel().unwrap_err().contains("outside"));
    assert!(descriptor(&WIDE, 5, 2).check_panel().unwrap_err().contains("outside"));
    assert!(descriptor(&MISSING, 2, 6).check_panel().is_err());
}

/// An XY pad needs two different parameters that exist and hide together, and
/// its own caption. Its parameters may have different mappings
/// (panel-widgets.md, XY pad).
#[test]
fn an_xy_pad_needs_two_parameters_that_hide_together() {
    const MODE: ParamSpec = param("mode", 0.0, 1.0, 0.0, "").integer();
    static PARAMS: [ParamSpec; 4] = [
        CUTOFF,
        GAIN,
        MODE,
        param("hidden", 0.0, 1.0, 0.0, "").shown_when("mode", &[1]),
    ];
    static GOOD: [Widget; 1] = [Widget::xy_pad(0, 0, "cutoff", "gain", "Place")];
    static SAME: [Widget; 1] = [Widget::xy_pad(0, 0, "gain", "gain", "Place")];
    static MISSING: [Widget; 1] = [Widget::xy_pad(0, 0, "gain", "nothing", "Place")];
    static BLANK: [Widget; 1] = [Widget::xy_pad(0, 0, "cutoff", "gain", " ")];
    static HALF: [Widget; 1] = [Widget::xy_pad(0, 0, "gain", "hidden", "Place")];
    let check = |widgets, size| {
        ModuleDescriptor {
            params: &PARAMS,
            panel: Panel::new(size, size, widgets),
            ..NAMED
        }
        .check_panel()
    };
    assert!(check(&GOOD, 6).is_ok());
    assert!(check(&GOOD, 5).unwrap_err().contains("outside"));
    assert!(check(&SAME, 6).unwrap_err().contains("different"));
    assert!(check(&MISSING, 6).unwrap_err().contains("second parameter"));
    assert!(check(&BLANK, 6).unwrap_err().contains("caption"));
    assert!(check(&HALF, 6).unwrap_err().contains("same choices"));
    assert!(GOOD[0].binds("cutoff") && GOOD[0].binds("gain") && !GOOD[0].binds("mode"));
    assert_eq!(GOOD[0].params().collect::<Vec<_>>(), ["cutoff", "gain"]);
}

/// A pad or a range cannot bind a whole number, because its arrows move by a
/// share of the track (panel-widgets.md, XY pad, and Range).
#[test]
fn a_widget_with_two_values_refuses_whole_numbers() {
    static PARAMS: [ParamSpec; 3] = [
        param("low", 0.0, 127.0, 0.0, "").integer(),
        param("high", 0.0, 127.0, 127.0, "").integer(),
        param("free", 0.0, 127.0, 0.0, ""),
    ];
    static RANGE: [Widget; 1] = [Widget::range(0, 0, "low", "high", "Notes")];
    static PAD: [Widget; 1] = [Widget::xy_pad(0, 0, "free", "high", "Notes")];
    for (widgets, width, height) in [(&RANGE, 6, 2), (&PAD, 6, 6)] {
        let problem = ModuleDescriptor {
            params: &PARAMS,
            panel: Panel::new(width, height, widgets),
            ..NAMED
        }
        .check_panel()
        .unwrap_err();
        assert!(problem.contains("whole-number"), "{problem}");
    }
}

/// A key step is a size in the parameter's units, and a control's position is
/// a share of the selected control range, so the two cannot be combined
/// (panel-widgets.md, Number).
#[test]
fn a_key_step_refuses_control_ranges() {
    const OFFERED: &[ParamRange] = &[ParamRange::new(-1.0, 1.0), ParamRange::new(-10.0, 10.0)];
    static PARAMS: [ParamSpec; 1] = [param("level", -10.0, 10.0, 0.0, "")
        .ranges(OFFERED, ParamRange::new(-1.0, 1.0))
        .key_step(1.0)];
    static WIDGETS: [Widget; 1] = [Widget::number(0, 0, "level")];
    let problem = ModuleDescriptor {
        params: &PARAMS,
        panel: Panel::new(6, 2, &WIDGETS),
        ..NAMED
    }
    .check_panel()
    .unwrap_err();
    assert!(problem.contains("key step"), "{problem}");
}

/// A range needs two parameters with the same range, mapping, and control
/// ranges, because both handles sit on one track (panel-widgets.md, Range).
#[test]
fn a_range_needs_parameters_that_share_a_track() {
    const OFFERED: &[ParamRange] = &[ParamRange::new(-1.0, 1.0), ParamRange::new(-10.0, 10.0)];
    const LESS: &[ParamRange] = &[ParamRange::new(-10.0, 10.0)];
    static PARAMS: [ParamSpec; 7] = [
        param("low", 0.0, 1.0, 0.0, ""),
        param("high", 0.0, 1.0, 1.0, ""),
        param("wider", 0.0, 2.0, 1.0, ""),
        param("curved", 0.1, 1.0, 1.0, "").logarithmic(),
        param("a", -10.0, 10.0, 0.0, "").ranges(OFFERED, ParamRange::new(-1.0, 1.0)),
        param("b", -10.0, 10.0, 1.0, "").ranges(OFFERED, ParamRange::new(-1.0, 1.0)),
        param("c", -10.0, 10.0, 1.0, "").ranges(LESS, ParamRange::new(-10.0, 10.0)),
    ];
    static GOOD: [Widget; 1] = [Widget::range(0, 0, "low", "high", "Span")];
    static TALL: [Widget; 1] = [Widget::range_vertical(0, 0, "low", "high", "Span")];
    static RANGES: [Widget; 1] = [Widget::range(0, 0, "a", "b", "Span")];
    static WIDER: [Widget; 1] = [Widget::range(0, 0, "low", "wider", "Span")];
    static CURVED: [Widget; 1] = [Widget::range(0, 0, "low", "curved", "Span")];
    static LISTS: [Widget; 1] = [Widget::range(0, 0, "a", "c", "Span")];
    let check = |widgets, width, height| {
        ModuleDescriptor {
            params: &PARAMS,
            panel: Panel::new(width, height, widgets),
            ..NAMED
        }
        .check_panel()
    };
    assert!(check(&GOOD, 6, 2).is_ok());
    assert!(check(&TALL, 2, 6).is_ok());
    assert!(check(&RANGES, 6, 2).is_ok());
    assert!(check(&GOOD, 5, 2).unwrap_err().contains("outside"));
    for bad in [&WIDER, &CURVED, &LISTS] {
        assert!(check(bad, 6, 2).unwrap_err().contains("same range"));
    }
}

/// A range widget's two parameters load with one range: a saved one they agree
/// on that holds both, or else the smallest that does, and no saved value
/// changes (control-ranges.md, Ranges on a range widget).
#[test]
fn a_range_widgets_parameters_load_with_one_range() {
    const R1: ParamRange = ParamRange::new(-1.0, 1.0);
    const R5: ParamRange = ParamRange::new(-5.0, 5.0);
    const R10: ParamRange = ParamRange::new(-10.0, 10.0);
    const OFFERED: &[ParamRange] = &[R1, R5, R10];
    static PARAMS: [ParamSpec; 3] = [
        param("a", -10.0, 10.0, 0.0, "").ranges(OFFERED, R1),
        param("b", -10.0, 10.0, 1.0, "").ranges(OFFERED, R1),
        param("alone", -10.0, 10.0, 1.0, "").ranges(OFFERED, R1),
    ];
    static WIDGETS: [Widget; 1] = [Widget::range(0, 0, "a", "b", "Span")];
    let d = ModuleDescriptor {
        params: &PARAMS,
        panel: Panel::new(6, 2, &WIDGETS),
        ..NAMED
    };
    assert_eq!(d.range_partner(0), Some(1));
    assert_eq!(d.range_partner(1), Some(0));
    assert_eq!(d.range_partner(2), None);

    let load = |index, saved: [Option<ParamRange>; 2], values: [f32; 2]| {
        let at = |spec: &ParamSpec| usize::from(spec.name == "b");
        d.load_range(index, |spec| saved[at(spec)], |spec| values[at(spec)])
    };
    assert_eq!(load(0, [None, None], [0.0, 1.0]), Some((R1, false)), "the default");
    assert_eq!(
        load(0, [None, None], [0.0, 3.0]),
        Some((R5, false)),
        "holds both, no report"
    );
    assert_eq!(
        load(1, [None, None], [0.0, 3.0]),
        Some((R5, false)),
        "the same for both"
    );
    assert_eq!(
        load(0, [Some(R10), Some(R10)], [0.0, 1.0]),
        Some((R10, false)),
        "agreed"
    );
    assert_eq!(
        load(0, [Some(R1), Some(R10)], [0.0, 1.0]),
        Some((R1, true)),
        "they differ"
    );
    assert_eq!(
        load(1, [Some(R1), Some(R10)], [0.0, 1.0]),
        Some((R1, true)),
        "the same for both"
    );
    assert_eq!(
        load(0, [Some(R1), Some(R1)], [0.0, 3.0]),
        Some((R5, true)),
        "does not hold both"
    );
}

/// A button shows one parameter. Its mode and hold do not count as bound, so a
/// lookup by name finds what it found before widgets could bind two.
#[test]
fn a_button_binds_only_its_own_parameter() {
    let button = Widget::button(0, 0, "gate", "held", "mode");
    assert!(button.binds("gate") && !button.binds("held") && !button.binds("mode"));
    assert_eq!(button.params().collect::<Vec<_>>(), ["gate"]);
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

/// A readout writes a value by its display range: a number with its unit, a
/// note name with cents, or a choice name. It never fails on a bad value.
#[test]
fn a_readout_writes_its_value_by_format() {
    let number = DisplayRange {
        min: -50.0,
        max: 50.0,
        unit: "cents",
        format: DisplayFormat::Number { decimals: 1 },
    };
    assert_eq!(number.text(12.54), "12.5 cents");
    // Around silence the text does not flip between "-0.0" and "0.0".
    assert_eq!(number.text(-0.04), "0.0 cents");
    assert_eq!(number.text(-0.0), "0.0 cents");
    assert_eq!(number.text(-0.06), "-0.1 cents");
    let plain = DisplayRange {
        unit: "",
        format: DisplayFormat::Number { decimals: 0 },
        ..number
    };
    assert_eq!(plain.text(7.0), "7");

    let note = DisplayRange {
        format: DisplayFormat::Note,
        ..plain
    };
    assert_eq!(note.text(0.0), "C4", "0.0 is middle C");
    assert_eq!(note.text(0.075), "A4", "9 semitones at 0.1 per octave");
    assert_eq!(note.text(-0.1), "C3", "an octave down");
    assert_eq!(note.text(1.0 / 1200.0), "C4 +10¢");
    assert_eq!(note.text(-1.0 / 1200.0), "C4 -10¢");
    assert_eq!(note.text(0.1 / 12.0), "C#4");

    let choices = DisplayRange {
        min: 1.0,
        format: DisplayFormat::Choices(&["One", "Two"]),
        ..plain
    };
    assert_eq!(choices.text(1.0), "One");
    assert_eq!(choices.text(2.0), "Two");
    assert_eq!(choices.text(99.0), "Two", "past the end names the last");
    assert_eq!(choices.text(-99.0), "One");

    for range in [number, note, choices] {
        assert_eq!(range.text(f32::NAN), "--");
        assert_eq!(range.text(f32::INFINITY), "--");
    }
}

/// The registry refuses a readout with no display value, on a trigger, or of
/// another width, and names the widget.
#[test]
fn a_readout_needs_a_level_display_and_a_known_width() {
    static LEVEL: [DisplaySpec; 2] = [
        DisplaySpec {
            name: "level",
            label: "Level",
            description: "A level.",
            kind: DisplayKind::Level,
            range: DisplayRange::UNIT,
        },
        DisplaySpec {
            name: "hit",
            label: "Hit",
            description: "A trigger.",
            kind: DisplayKind::Trigger,
            range: DisplayRange::UNIT,
        },
    ];
    static GOOD: [Widget; 2] = [Widget::readout(0, 0, 3, "level"), Widget::readout(0, 2, 6, "level")];
    static MISSING: [Widget; 1] = [Widget::readout(0, 0, 3, "nothing")];
    static TRIGGER: [Widget; 1] = [Widget::readout(0, 0, 3, "hit")];
    static WIDTH: [Widget; 1] = [Widget::readout(0, 0, 4, "level")];
    let descriptor = |widgets| ModuleDescriptor {
        displays: &LEVEL,
        panel: Panel::new(6, 4, widgets),
        ..NAMED
    };
    assert!(descriptor(&GOOD).check_panel().is_ok());
    assert!(!GOOD[0].is_control(), "a readout is a display");
    for (widgets, why) in [
        (&MISSING[..], "needs a display value"),
        (&TRIGGER[..], "cannot show a trigger"),
        (&WIDTH[..], "3 or 6"),
    ] {
        let error = descriptor(widgets).check_panel().unwrap_err();
        assert!(error.contains(why) && error.contains("Readout"), "{error}");
    }
}

/// A level history needs an output, and a display's own condition must name a
/// choice its module has. A control follows its parameter's condition instead
/// (panel-widgets.md, Level history).
#[test]
fn a_level_history_needs_an_output_and_a_real_condition() {
    const VIEW: ParamSpec = param("view", 0.0, 1.0, 0.0, "").integer().choices(&["Off", "On"]);
    static PARAMS: [ParamSpec; 2] = [VIEW, CUTOFF];
    static OUTPUTS: [PortSpec; 1] = [PortSpec {
        name: "out",
        label: "Out",
        signal: crate::SignalType::Audio,
        description: "The signal.",
        default: PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: Channels::PerChannel,
    }];
    static GOOD: [Widget; 1] = [Widget::level_history(0, 0, "out").shown_when("view", &[1])];
    static NO_OUTPUT: [Widget; 1] = [Widget::level_history(0, 0, "nothing")];
    static NO_MODE: [Widget; 1] = [Widget::level_history(0, 0, "out").shown_when("nothing", &[1])];
    static NOT_A_CHOICE: [Widget; 1] = [Widget::level_history(0, 0, "out").shown_when("cutoff", &[1])];
    static NO_SUCH_CHOICE: [Widget; 1] = [Widget::level_history(0, 0, "out").shown_when("view", &[2])];
    static ON_A_CONTROL: [Widget; 1] = [Widget::knob(0, 0, "cutoff").shown_when("view", &[1])];
    let descriptor = |widgets| ModuleDescriptor {
        params: &PARAMS,
        outputs: &OUTPUTS,
        panel: Panel::new(9, 6, widgets),
        ..NAMED
    };
    assert!(descriptor(&GOOD).check_panel().is_ok());
    for bad in [&NO_OUTPUT, &NO_MODE, &NOT_A_CHOICE, &NO_SUCH_CHOICE, &ON_A_CONTROL] {
        assert!(descriptor(bad).check_panel().is_err());
    }
    let shown = descriptor(&GOOD);
    assert!(!shown.is_widget_shown(&GOOD[0], &[0.0, 0.0]));
    assert!(shown.is_widget_shown(&GOOD[0], &[1.0, 0.0]));
}
