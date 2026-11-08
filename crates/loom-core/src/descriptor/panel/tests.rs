use super::*;
use crate::descriptor::fixtures::*;
use crate::descriptor::{
    Channels, DisplayRange, ModuleRole, ParamOptions, ParamRange, PortDefault, PortRange, PortSpec,
};
use crate::signal::SignalType;

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
        roles: &[],
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
        roles: &[],
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

/// A button shows one parameter. Its mode and hold do not count as bound, so a
/// lookup by name finds what it found before widgets could bind two.
#[test]
fn a_button_binds_only_its_own_parameter() {
    let button = Widget::button(0, 0, "gate", "held", "mode");
    assert!(button.binds("gate") && !button.binds("held") && !button.binds("mode"));
    assert_eq!(button.params().collect::<Vec<_>>(), ["gate"]);
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

/// An XY scope binds two different outputs of its own module, and the registry
/// names the widget when it does not (panel-widgets.md, XY scope).
#[test]
fn an_xy_scope_needs_two_outputs_of_its_module() {
    const fn output(name: &'static str) -> PortSpec {
        PortSpec {
            name,
            label: name,
            signal: crate::SignalType::Audio,
            description: "The signal.",
            default: PortDefault::Value(0.0),
            range: crate::PortRange::Signal,
            channels: Channels::PerChannel,
        }
    }
    static OUTPUTS: [PortSpec; 2] = [output("a"), output("b")];
    static GOOD: [Widget; 1] = [Widget::xy_scope(0, 0, "a", "b")];
    static NO_X: [Widget; 1] = [Widget::xy_scope(0, 0, "nothing", "b")];
    static NO_Y: [Widget; 1] = [Widget::xy_scope(0, 0, "a", "nothing")];
    static SAME: [Widget; 1] = [Widget::xy_scope(0, 0, "a", "a")];
    let descriptor = |widgets| ModuleDescriptor {
        outputs: &OUTPUTS,
        panel: Panel::new(6, 6, widgets),
        ..NAMED
    };
    assert!(descriptor(&GOOD).check_panel().is_ok());
    assert!(descriptor(&NO_X).check_panel().unwrap_err().contains("XyScope"));
    for bad in [&NO_Y, &SAME] {
        let problem = descriptor(bad).check_panel().unwrap_err();
        assert!(problem.contains("two different outputs"), "{problem}");
    }
}

/// Each widget builder sets its kind, place, and names. The registry only
/// calls them in `const` panels, which run at compile time, so this test is
/// where they run as code.
#[test]
fn widget_builders_set_kind_place_and_names() {
    use WidgetKind as K;
    let one = |w: Widget, bind: &str| {
        assert_eq!((w.x, w.y, w.bind()), (1, 2, bind), "{w:?}");
        assert_eq!((w.caption, w.shown_when), (None, None), "{w:?}");
        w
    };
    assert!(matches!(one(Widget::knob(1, 2, "p"), "p").kind, K::Knob));
    assert!(matches!(
        one(Widget::fader(1, 2, "p"), "p").kind,
        K::Fader { vertical: true }
    ));
    assert!(matches!(
        one(Widget::fader_horizontal(1, 2, "p"), "p").kind,
        K::Fader { vertical: false }
    ));
    assert!(matches!(one(Widget::number(1, 2, "p"), "p").kind, K::Number));
    assert!(matches!(one(Widget::toggle(1, 2, "p"), "p").kind, K::Toggle));
    assert!(matches!(one(Widget::radio(1, 2, "p"), "p").kind, K::Radio));
    assert!(matches!(
        one(Widget::segmented(1, 2, 3, "p"), "p").kind,
        K::Segmented { width: 3 }
    ));
    assert!(matches!(one(Widget::scope(1, 2, "out"), "out").kind, K::Scope));
    assert!(matches!(one(Widget::spectrum(1, 2, "out"), "out").kind, K::Spectrum));
    assert!(matches!(
        one(Widget::level_history(1, 2, "out"), "out").kind,
        K::LevelHistory
    ));
    assert!(matches!(
        one(Widget::xy_scope(1, 2, "l", "r"), "l").kind,
        K::XyScope { y: "r" }
    ));
    assert!(matches!(
        one(Widget::momentary(1, 2, "go"), "go").kind,
        K::Button { mode: "", hold: "go" }
    ));

    // The two-parameter widgets carry their caption and second name.
    let pad = Widget::xy_pad(1, 2, "h", "v", "Pad");
    assert_eq!((pad.bind(), pad.second(), pad.caption), ("h", Some("v"), Some("Pad")));
    let range = Widget::range(1, 2, "lo", "hi", "Band");
    assert!(matches!(
        range.kind,
        K::Range {
            high: "hi",
            vertical: false
        }
    ));
    assert_eq!((range.second(), range.caption), (Some("hi"), Some("Band")));
    let tall = Widget::range_vertical(1, 2, "lo", "hi", "Band");
    assert!(matches!(tall.kind, K::Range { vertical: true, .. }));

    // The modifiers change only what they name.
    let captioned = Widget::knob(1, 2, "p").captioned("1");
    assert_eq!(captioned.caption, Some("1"));
    let shown = Widget::scope(1, 2, "out").shown_when("view", &[1]);
    assert_eq!(
        shown.shown_when,
        Some(ShownWhen {
            param: "view",
            choices: &[1]
        })
    );
}
