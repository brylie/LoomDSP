use super::*;

/// A port for test descriptors.
const fn port(name: &'static str, signal: crate::SignalType) -> crate::PortSpec {
    crate::PortSpec {
        name,
        label: name,
        signal,
        description: "A test port.",
        default: crate::PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: crate::Channels::PerChannel,
    }
}

/// A parameter for test descriptors.
const fn param(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> crate::ParamSpec {
    crate::ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min,
        max,
        default,
        unit,
        options: crate::ParamOptions::DEFAULT,
    }
}

/// A level display value for test descriptors.
const fn display(name: &'static str) -> crate::DisplaySpec {
    crate::DisplaySpec {
        name,
        label: name,
        description: "A test display value.",
        kind: crate::DisplayKind::Level,
    }
}

#[test]
fn parses_kinds() {
    assert_eq!(
        ModuleKind::parse("loom.osc.sine@2").unwrap(),
        ModuleKind {
            id: "loom.osc.sine".into(),
            version: 2
        }
    );
    assert_eq!(ModuleKind::parse("loom.osc.sine").unwrap().version, 1);
    assert!(ModuleKind::parse("@1").is_err());
    assert!(ModuleKind::parse("x@y").is_err());
}

#[test]
fn refuses_input_defaults_that_are_not_finite() {
    static NAN_DEFAULT: ModuleDescriptor = ModuleDescriptor {
        id: "test.nan",
        version: 1,
        name: "NaN",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[port("in", crate::SignalType::Modulation).with_default(PortDefault::Value(f32::NAN))],
        outputs: &[],
        params: &[],
        role: ModuleRole::UnitDelay,
        panel: crate::Panel::EMPTY,
        displays: &[],
    };
    let error = Registry::new().register(&NAN_DEFAULT).unwrap_err();
    assert!(error.to_string().contains("finite"), "{error}");
}

/// A DSP module with the given ports, for the stereo checks.
macro_rules! with_ports {
    ($inputs:expr, $outputs:expr) => {{
        static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
            id: "test.ports",
            version: 1,
            name: "Ports",
            category: "Test",
            family: crate::Family::Interface,
            description: "",
            inputs: $inputs,
            outputs: $outputs,
            params: &[],
            role: ModuleRole::Dsp(|_| unreachable!("never created")),
            panel: crate::Panel::EMPTY,
            displays: &[],
        };
        Registry::new().register(&DESCRIPTOR)
    }};
}

/// Port spans must be finite and strictly increasing on both sides.
#[test]
fn rejects_invalid_port_ranges() {
    use crate::{PortRange, SignalType};
    for result in [
        with_ports!(
            &[port("in", SignalType::Audio).with_range(PortRange::Span(f32::NAN, 1.0))],
            &[]
        ),
        with_ports!(
            &[],
            &[port("out", SignalType::Audio).with_range(PortRange::Span(0.0, f32::INFINITY))]
        ),
        with_ports!(
            &[port("in", SignalType::Audio).with_range(PortRange::Span(1.0, 1.0))],
            &[]
        ),
        with_ports!(
            &[],
            &[port("out", SignalType::Audio).with_range(PortRange::Span(1.0, -1.0))]
        ),
    ] {
        assert!(result.unwrap_err().to_string().contains("port range"));
    }
    with_ports!(
        &[],
        &[port("out", SignalType::Audio).with_range(PortRange::Span(0.0, 5.0))]
    )
    .unwrap();
}

/// Signal conventions and explicit spans derive the modulation polarity.
#[test]
fn nominal_ranges_derive_polarity() {
    use crate::{Polarity, PortRange, SignalType};
    for (signal, range, polarity) in [
        (SignalType::Audio, (-1.0, 1.0), Polarity::Bipolar),
        (SignalType::Modulation, (-1.0, 1.0), Polarity::Bipolar),
        (SignalType::Pitch, (-0.5, 0.5), Polarity::Bipolar),
        (SignalType::Phase, (0.0, 1.0), Polarity::Unipolar),
        (SignalType::Gate, (0.0, 1.0), Polarity::Unipolar),
    ] {
        let p = port("out", signal);
        assert_eq!(p.nominal_range(), range);
        assert_eq!(p.polarity(), polarity);
    }
    let p = port("out", SignalType::Modulation).with_range(PortRange::Span(0.0, 5.0));
    assert_eq!(p.nominal_range(), (0.0, 5.0));
    assert_eq!(p.polarity(), Polarity::Unipolar);
}

#[test]
fn refuses_downmix_outputs() {
    let error = with_ports!(
        &[],
        &[port("out", crate::SignalType::Audio).with_channels(Channels::Downmix)]
    )
    .unwrap_err();
    assert!(error.to_string().contains("output cannot be Downmix"), "{error}");
}

#[test]
fn refuses_per_channel_inputs_on_stereo_modules() {
    let error = with_ports!(
        &[port("in", crate::SignalType::Audio)],
        &[port("out", crate::SignalType::Audio).with_channels(Channels::Stereo)]
    )
    .unwrap_err();
    assert!(error.to_string().contains("Stereo port"), "{error}");
    let error = with_ports!(
        &[
            port("in", crate::SignalType::Audio).with_channels(Channels::Stereo),
            port("gain", crate::SignalType::Modulation),
        ],
        &[]
    )
    .unwrap_err();
    assert!(error.to_string().contains("Stereo port"), "{error}");
}

#[test]
fn accepts_stereo_modules_with_downmix_inputs() {
    with_ports!(
        &[
            port("in", crate::SignalType::Audio).with_channels(Channels::Stereo),
            port("gain", crate::SignalType::Modulation).with_channels(Channels::Downmix),
        ],
        &[port("out", crate::SignalType::Audio).with_channels(Channels::Stereo)]
    )
    .unwrap();
}

#[test]
fn registers_undocumented_modules_with_notes() {
    static QUIET: ModuleDescriptor = ModuleDescriptor {
        id: "test.quiet",
        version: 1,
        name: "Quiet",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[crate::PortSpec {
            name: "in",
            label: "In",
            signal: crate::SignalType::Audio,
            description: "",
            default: PortDefault::Value(0.0),

            range: crate::PortRange::Signal,
            channels: crate::Channels::PerChannel,
        }],
        outputs: &[port("out", crate::SignalType::Audio)],
        params: &[],
        role: ModuleRole::UnitDelay,
        panel: crate::Panel::EMPTY,
        displays: &[],
    };
    let mut registry = Registry::new();
    registry.register(&QUIET).unwrap();
    assert!(registry.get("test.quiet").is_some());
    let notes: Vec<String> = registry.notes().iter().map(ToString::to_string).collect();
    assert_eq!(notes, ["test.quiet: input `in` has no description"]);
}

/// A display value with no label or description loads, with a note,
/// as a port or parameter does.
#[test]
fn notes_undocumented_display_values() {
    static DARK: ModuleDescriptor = ModuleDescriptor {
        id: "test.dark",
        version: 1,
        name: "Dark",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[],
        outputs: &[],
        params: &[],
        role: ModuleRole::Dsp(|_| unreachable!("never created")),
        panel: crate::Panel::EMPTY,
        displays: &[crate::DisplaySpec {
            name: "busy",
            label: " ",
            description: "",
            kind: crate::DisplayKind::Trigger,
        }],
    };
    let mut registry = Registry::new();
    registry.register(&DARK).unwrap();
    let notes: Vec<String> = registry.notes().iter().map(ToString::to_string).collect();
    assert_eq!(
        notes,
        [
            "test.dark: display value `busy` has no label",
            "test.dark: display value `busy` has no description"
        ]
    );
}

/// control-ranges.md: the registry refuses a range outside the
/// parameter, a range with `min >= max`, a default range not in the
/// list, a default value outside the default range, and ranges on a
/// logarithmic or whole-number parameter.
#[test]
fn refuses_bad_ranges() {
    use crate::ParamRange as R;
    macro_rules! with_param {
        ($param:expr) => {{
            static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
                id: "test.ranges",
                version: 1,
                name: "Ranges",
                category: "Test",
                family: crate::Family::Interface,
                description: "",
                inputs: &[],
                outputs: &[],
                params: &[$param],
                role: ModuleRole::Dsp(|_| unreachable!("never created")),
                panel: crate::Panel::EMPTY,
                displays: &[],
            };
            Registry::new().register(&DESCRIPTOR).map_err(|error| error.to_string())
        }};
    }
    const LIST: &[R] = &[R::new(-1.0, 1.0), R::new(-10.0, 10.0)];
    assert_eq!(
        with_param!(param("gain", -10.0, 10.0, 1.0, "").ranges(LIST, R::new(-1.0, 1.0))),
        Ok(())
    );
    let refused = [
        (
            with_param!(param("gain", -5.0, 5.0, 1.0, "").ranges(LIST, R::new(-1.0, 1.0))),
            "range -10 to 10 is outside",
        ),
        (
            with_param!(param("gain", -10.0, 10.0, 1.0, "").ranges(&[R::new(1.0, 1.0)], R::new(1.0, 1.0))),
            "not below its maximum",
        ),
        (
            with_param!(param("gain", -10.0, 10.0, 1.0, "").ranges(LIST, R::new(0.0, 1.0))),
            "the default range is not in the list",
        ),
        (
            with_param!(param("gain", -10.0, 10.0, 5.0, "").ranges(LIST, R::new(-1.0, 1.0))),
            "the default value 5 is outside the default range -1 to 1",
        ),
        (
            with_param!(
                param("cutoff", 1.0, 10.0, 1.0, "")
                    .logarithmic()
                    .ranges(&[R::new(1.0, 10.0)], R::new(1.0, 10.0))
            ),
            "only linear parameters",
        ),
        (
            with_param!(
                param("steps", -10.0, 10.0, 1.0, "")
                    .integer()
                    .ranges(LIST, R::new(-1.0, 1.0))
            ),
            "only linear parameters",
        ),
    ];
    for (result, expected) in refused {
        let error = result.unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

/// Choice names go on whole-number parameters only, one per value, and
/// none empty. The error names the parameter.
#[test]
fn refuses_bad_choice_names() {
    macro_rules! with_param {
        ($param:expr) => {{
            static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
                id: "test.choices",
                version: 1,
                name: "Choices",
                category: "Test",
                family: crate::Family::Interface,
                description: "",
                inputs: &[],
                outputs: &[],
                params: &[$param],
                role: ModuleRole::Dsp(|_| unreachable!("never created")),
                panel: crate::Panel::EMPTY,
                displays: &[],
            };
            Registry::new().register(&DESCRIPTOR).map_err(|error| error.to_string())
        }};
    }
    assert_eq!(
        with_param!(
            param("mode", 0.0, 1.0, 0.0, "")
                .integer()
                .choices(&["Toggle", "Momentary"])
        ),
        Ok(())
    );
    let refused = [
        (
            with_param!(
                param("mode", 0.0, 2.0, 0.0, "")
                    .integer()
                    .choices(&["Toggle", "Momentary"])
            ),
            "parameter `mode`: 2 choice names for 3 values",
        ),
        (
            with_param!(param("mode", 0.0, 1.0, 0.0, "").integer().choices(&["Toggle", " "])),
            "parameter `mode`: a choice name is empty",
        ),
        (
            with_param!(param("mix", 0.0, 1.0, 0.0, "").choices(&["Dry", "Wet"])),
            "parameter `mix`: choice names need a whole-number parameter",
        ),
    ];
    for (result, expected) in refused {
        let error = result.unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

/// A DSP module with the given parameters, for registry checks.
macro_rules! with_params {
        ($($param:expr),*) => {{
            static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
                id: "test.params",
                version: 1,
                name: "Params",
                category: "Test",
                family: crate::Family::Interface,
                description: "",
                inputs: &[],
                outputs: &[],
                params: &[$($param),*],
                role: ModuleRole::Dsp(|_| unreachable!("never created")),
                panel: crate::Panel::EMPTY,
                displays: &[],
            };
            &DESCRIPTOR
        }};
    }

#[test]
fn refuses_impossible_logarithmic_parameters() {
    let whole = with_params!(param("count", 1.0, 100.0, 1.0, "").integer().logarithmic());
    let error = Registry::new().register(whole).unwrap_err();
    assert!(error.to_string().contains("whole-number"), "{error}");
    let from_zero = with_params!(param("time", 0.0, 10.0, 1.0, "s").logarithmic());
    let error = Registry::new().register(from_zero).unwrap_err();
    assert!(error.to_string().contains("minimum above 0"), "{error}");
    let fine = with_params!(param("time", 0.001, 10.0, 1.0, "s").logarithmic());
    assert!(Registry::new().register(fine).is_ok());
}

/// A transient parameter must be a whole number with a default of 0
/// (transient-params.md, rules 1 and 2).
#[test]
fn refuses_impossible_transient_parameters() {
    let continuous = with_params!(param("hold", 0.0, 1.0, 0.0, "").transient());
    let error = Registry::new().register(continuous).unwrap_err();
    assert!(error.to_string().contains("whole number"), "{error}");
    let held = with_params!(param("hold", 0.0, 1.0, 1.0, "").integer().transient());
    let error = Registry::new().register(held).unwrap_err();
    assert!(error.to_string().contains("default to 0"), "{error}");
    let fine = with_params!(param("hold", 0.0, 1.0, 0.0, "").integer().transient());
    assert!(Registry::new().register(fine).is_ok());
}

/// A DSP module with two parameters, one display value, and a panel.
macro_rules! with_panel {
        ($width:expr, $height:expr, [$($widget:expr),*]) => {{
            use crate::Widget;
            static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
                id: "test.panel",
                version: 1,
                name: "Panel",
                category: "Test",
                family: crate::Family::Interface,
                description: "",
                inputs: &[],
                outputs: &[port("out", crate::SignalType::Audio)],
                params: &[
                    param("level", 0.0, 1.0, 0.5, ""),
                    param("on", 0.0, 1.0, 0.0, "").integer(),
                    param("slope", 0.0, 3.0, 0.0, "").integer().choices(&["6", "12", "18", "24"]),
                    param("mode", 0.0, 1.0, 0.0, "").integer(),
                    param("hold", 0.0, 1.0, 0.0, "").integer().transient(),
                ],
                role: ModuleRole::Dsp(|_| unreachable!("never created")),
                panel: crate::Panel::new($width, $height, &[$($widget),*]),
                displays: &[display("busy")],
            };
            Registry::new().register(&DESCRIPTOR).map_err(|error| error.to_string())
        }};
    }

#[test]
fn accepts_a_panel_that_fits() {
    let result = with_panel!(
        15,
        6,
        [
            Widget::knob(0, 0, "level"),
            Widget::toggle(3, 0, "on"),
            Widget::radio(6, 0, "slope"),
            Widget::light(0, 3, "busy"),
            Widget::meter(12, 0, "out")
        ]
    );
    assert_eq!(result, Ok(()));
}

#[test]
fn refuses_panels_that_do_not_fit() {
    let refused = [
        (
            with_panel!(3, 3, [Widget::knob(0, 0, "volume")]),
            "`volume` (Knob): no such parameter",
        ),
        (with_panel!(3, 2, [Widget::toggle(0, 0, "level")]), "a toggle needs"),
        (
            with_panel!(3, 4, [Widget::radio(0, 0, "on")]),
            "radio buttons need a whole-number parameter with choice names",
        ),
        (
            with_panel!(3, 4, [Widget::radio(0, 0, "slope")]),
            "`slope` (Radio): outside the panel",
        ),
        (
            with_panel!(3, 2, [Widget::light(0, 0, "idle")]),
            "no such display value",
        ),
        (
            with_panel!(3, 6, [Widget::meter(0, 0, "wet")]),
            "`wet` (Meter): no such output",
        ),
        (
            with_panel!(9, 5, [Widget::scope(0, 0, "wet")]),
            "`wet` (Scope): no such output",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "level", "hold", "mode")]),
            "a button needs",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "on", "hold", "missing")]),
            "no such mode parameter",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "on", "hold", "level")]),
            "mode must be a whole-number parameter from 0 to 1",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "on", "hold", "slope")]),
            "mode must be a whole-number parameter from 0 to 1",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "on", "hold", "on")]),
            "mode must be a different parameter",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "on", "mode", "mode")]),
            "mode must be a different parameter",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "on", "on", "mode")]),
            "hold must be a different parameter",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "on", "missing", "mode")]),
            "no such hold parameter",
        ),
        (
            with_panel!(6, 3, [Widget::button(0, 0, "on", "slope", "mode")]),
            "hold must be a whole-number parameter from 0 to 1",
        ),
        (with_panel!(3, 2, [Widget::knob(0, 0, "level")]), "outside the panel"),
        (
            with_panel!(6, 3, [Widget::knob(0, 0, "level"), Widget::toggle(2, 1, "on")]),
            "`on` (Toggle): overlaps `level`",
        ),
    ];
    for (result, expected) in refused {
        let error = result.unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn refuses_too_many_display_values() {
    static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
        id: "test.displays",
        version: 1,
        name: "Displays",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[],
        outputs: &[],
        params: &[],
        role: ModuleRole::Dsp(|_| unreachable!("never created")),
        panel: crate::Panel::EMPTY,
        displays: &[display("a"), display("b"), display("c"), display("d"), display("e")],
    };
    let error = Registry::new().register(&DESCRIPTOR).unwrap_err();
    assert!(error.to_string().contains("more than 4 display values"), "{error}");
}

/// A unit delay or audio output with a default the engine would ignore
/// is refused, and the error names the module and the port.
#[test]
fn refuses_ignored_input_defaults_on_delay_and_output_roles() {
    static DELAY_VALUE: ModuleDescriptor = ModuleDescriptor {
        id: "test.delay_default",
        version: 1,
        name: "Delay Default",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[port("in", crate::SignalType::Audio).with_default(PortDefault::Value(0.5))],
        outputs: &[port("out", crate::SignalType::Audio)],
        params: &[],
        role: ModuleRole::UnitDelay,
        panel: crate::Panel::EMPTY,
        displays: &[],
    };
    static OUTPUT_TRANSPORT: ModuleDescriptor = ModuleDescriptor {
        id: "test.output_default",
        version: 1,
        name: "Output Default",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[crate::PortSpec {
            channels: Channels::Stereo,
            ..port("in", crate::SignalType::Audio).with_default(PortDefault::Playing)
        }],
        outputs: &[],
        params: &[],
        role: ModuleRole::AudioOutput,
        panel: crate::Panel::EMPTY,
        displays: &[],
    };
    for (descriptor, port_name) in [(&DELAY_VALUE, "in"), (&OUTPUT_TRANSPORT, "in")] {
        let text = Registry::new().register(descriptor).unwrap_err().to_string();
        assert!(text.contains(descriptor.id), "{text}");
        assert!(text.contains(&format!("\"{port_name}\"")), "{text}");
        assert!(text.contains("ignores"), "{text}");
    }
}

#[test]
fn refuses_parameters_and_displays_on_non_dsp_modules() {
    static DELAY_WITH_PARAMS: ModuleDescriptor = ModuleDescriptor {
        id: "test.delay_params",
        version: 1,
        name: "Delay With Params",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[port("in", crate::SignalType::Audio)],
        outputs: &[port("out", crate::SignalType::Audio)],
        params: &[param("time", 0.0, 1.0, 0.5, "s")],
        role: ModuleRole::UnitDelay,
        panel: crate::Panel::EMPTY,
        displays: &[],
    };
    let error = Registry::new().register(&DELAY_WITH_PARAMS).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("only DSP modules can have parameters or display values"),
        "{error}"
    );

    static OUTPUT_WITH_PARAMS: ModuleDescriptor = ModuleDescriptor {
        id: "test.output_params",
        version: 1,
        name: "Output With Params",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[crate::PortSpec {
            channels: Channels::Stereo,
            ..port("in", crate::SignalType::Audio)
        }],
        outputs: &[],
        params: &[param("level", 0.0, 1.0, 0.5, "")],
        role: ModuleRole::AudioOutput,
        panel: crate::Panel::EMPTY,
        displays: &[],
    };
    let error = Registry::new().register(&OUTPUT_WITH_PARAMS).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("only DSP modules can have parameters or display values"),
        "{error}"
    );

    static DELAY_WITH_DISPLAYS: ModuleDescriptor = ModuleDescriptor {
        id: "test.delay_displays",
        version: 1,
        name: "Delay With Displays",
        category: "Test",
        family: crate::Family::Interface,
        description: "",
        inputs: &[port("in", crate::SignalType::Audio)],
        outputs: &[port("out", crate::SignalType::Audio)],
        params: &[],
        role: ModuleRole::UnitDelay,
        panel: crate::Panel::EMPTY,
        displays: &[display("level")],
    };
    let error = Registry::new().register(&DELAY_WITH_DISPLAYS).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("only DSP modules can have parameters or display values"),
        "{error}"
    );
}

/// `shown_when` names a choice parameter, or an on and off switch, of
/// the same module, and choices it has (ADR-0030). A segmented control
/// binds 2 to 4 named choices. The errors name what is wrong.
#[test]
fn refuses_bad_mode_conditions_and_segmented_controls() {
    macro_rules! with {
        ($params:expr, $widgets:expr) => {{
            static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
                id: "test.modes",
                version: 1,
                name: "Modes",
                category: "Test",
                family: crate::Family::Interface,
                description: "",
                inputs: &[],
                outputs: &[],
                params: $params,
                role: ModuleRole::Dsp(|_| unreachable!("never created")),
                panel: crate::Panel::new(9, 5, $widgets),
                displays: &[],
            };
            Registry::new().register(&DESCRIPTOR).map_err(|error| error.to_string())
        }};
    }
    const MODE: crate::ParamSpec = param("mode", 0.0, 1.0, 0.0, "").integer().choices(&["A", "B"]);
    assert_eq!(
        with!(
            &[MODE, param("depth", 0.0, 1.0, 0.0, "").shown_when("mode", &[1])],
            &[
                crate::Widget::segmented(0, 0, 6, "mode"),
                crate::Widget::knob(0, 2, "depth")
            ]
        ),
        Ok(())
    );
    let refused = [
        (
            with!(&[MODE, param("depth", 0.0, 1.0, 0.0, "").shown_when("nope", &[0])], &[]),
            "shown when `nope`: no such parameter",
        ),
        (
            with!(&[MODE, param("depth", 0.0, 1.0, 0.0, "").shown_when("mode", &[2])], &[]),
            "names a choice the parameter does not have",
        ),
        (
            with!(
                &[
                    param("gain", 0.0, 2.0, 0.0, ""),
                    param("depth", 0.0, 1.0, 0.0, "").shown_when("gain", &[0])
                ],
                &[]
            ),
            "needs a parameter with choice names",
        ),
        (
            with!(
                &[param("level", 0.0, 1.0, 0.0, "")],
                &[crate::Widget::segmented(0, 0, 6, "level")]
            ),
            "a segmented control needs a width and 2 to 4 named whole-number choices",
        ),
    ];
    for (result, expected) in refused {
        let error = result.unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}
