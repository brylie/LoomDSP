use super::*;

/// Stereo cable widths appear on both ports and clear after disconnection.
#[test]
fn stereo_cable_hover_follows_the_compile_report() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let c = &mut parts.controller;
    let join = c.add_module("loom.mix.join").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(join, "out", out, "in").unwrap();
    c.commit().unwrap();
    let scene = build(c, &BTreeMap::new(), &BTreeMap::new());
    assert!(
        scene.module(join.0).unwrap().outputs[0]
            .hover
            .description
            .contains("Stereo")
    );
    assert!(
        scene.module(out.0).unwrap().inputs[0]
            .hover
            .description
            .contains("Stereo")
    );
    let input = c.input_ref(out, "in").unwrap();
    c.disconnect(input).unwrap();
    c.commit().unwrap();
    let scene = build(c, &BTreeMap::new(), &BTreeMap::new());
    assert!(
        !scene.module(out.0).unwrap().inputs[0]
            .hover
            .description
            .contains("Stereo")
    );
}

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

/// A modulated knob carries the engine's live value while an
/// assignment is on, and none when every assignment is off.
#[test]
fn modulated_knobs_carry_the_live_value() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let controller = &mut parts.controller;
    let source = controller.add_module("loom.math.constant").unwrap();
    controller.set_param_named(source, "value", 0.5).unwrap();
    let filter = controller.add_module("loom.filter.svf").unwrap();
    let id = controller
        .add_assignment_named(source, "out", filter, "cutoff", 0.2)
        .unwrap();
    let cutoff = controller.graph().node(filter).unwrap().descriptor;
    let knob = cutoff.panel.widgets.iter().position(|w| w.bind == "cutoff").unwrap();
    controller.commit().unwrap();
    let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
    parts.engine.process(&mut left, &mut right);

    let mut values = PanelValues::new();
    update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
    let live = values.get(filter.0, knob).unwrap().live.expect("a live value");
    assert_eq!(Some(live), parts.controller.modulated(filter, 0));
    // The resonance knob is not modulated.
    let other = 1 - knob;
    assert_eq!(values.get(filter.0, other).unwrap().live, None);

    parts.controller.set_assignment_enabled(id, false).unwrap();
    update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
    assert_eq!(values.get(filter.0, knob).unwrap().live, None);
}

/// The frame refresh changes only the live dot, and only when it moved,
/// so a still value keeps the panel generation.
#[test]
fn the_frame_refresh_touches_only_live_dots() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let controller = &mut parts.controller;
    let source = controller.add_module("loom.math.constant").unwrap();
    let filter = controller.add_module("loom.filter.svf").unwrap();
    controller
        .add_assignment_named(source, "out", filter, "cutoff", 0.2)
        .unwrap();
    let d = controller.graph().node(filter).unwrap().descriptor;
    let knob = d.panel.widgets.iter().position(|w| w.bind == "cutoff").unwrap();
    controller.commit().unwrap();

    let mut values = PanelValues::new();
    update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
    assert_eq!(values.get(filter.0, knob).unwrap().live, None, "not swapped in yet");
    let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
    parts.engine.process(&mut left, &mut right);
    let before = values.get(filter.0, knob).unwrap().clone();
    update_live_values(&mut values, &parts.controller);
    let after = values.get(filter.0, knob).unwrap().clone();
    assert!(after.live.is_some());
    assert_eq!(WidgetValue { live: None, ..after }, before, "only the dot changed");
    let generation = values.generation();
    update_live_values(&mut values, &parts.controller);
    assert_eq!(values.generation(), generation, "a still value changes nothing");
}

/// Dragging an assignment that is off shows only its arc, but the knob
/// keeps its live dot while another assignment is on.
#[test]
fn dragging_an_off_assignment_keeps_the_live_dot() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let controller = &mut parts.controller;
    let source = controller.add_module("loom.math.constant").unwrap();
    let filter = controller.add_module("loom.filter.svf").unwrap();
    controller
        .add_assignment_named(source, "out", filter, "cutoff", 0.2)
        .unwrap();
    let off = controller
        .add_assignment_named(filter, "lowpass", filter, "cutoff", 0.1)
        .unwrap();
    controller.set_assignment_enabled(off, false).unwrap();
    let d = controller.graph().node(filter).unwrap().descriptor;
    let knob = d.panel.widgets.iter().position(|w| w.bind == "cutoff").unwrap();
    controller.commit().unwrap();
    let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
    parts.engine.process(&mut left, &mut right);

    let mut values = PanelValues::new();
    update_panel_values_dragging(&mut values, &parts.controller, &BTreeMap::new(), Some(off));
    let value = values.get(filter.0, knob).unwrap();
    assert!(value.live.is_some());
    assert_eq!(
        value.modulation,
        loom_canvas::ModulationMark::Ring,
        "only the off one's arc"
    );
}

#[test]
fn halfway_up_the_cutoff_knob_is_632_hz() {
    let value = control_value(WidgetKind::Knob, &CUTOFF, 0.5);
    assert!((value - 632.46).abs() < 0.1, "{value}");
    let back = control_position(WidgetKind::Knob, &CUTOFF, value);
    assert!((back - 0.5).abs() < 1e-4);
}

/// A button shows its latch in toggle mode and its hold in momentary
/// mode, so a latched `on` does not show as a hold (clock.md, Button
/// details).
#[test]
fn a_button_shows_the_parameter_its_mode_chooses() {
    use loom_core::{EngineConfig, create_engine};

    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let mut controller = parts.controller;
    let id = controller.add_module("loom.ui.button").unwrap();
    controller.set_param_named(id, "on", 1.0).unwrap();
    let (index, _, _) = control_param(&controller, id.0, 0).unwrap();
    assert_eq!(controller.graph().node(id).unwrap().descriptor.params[index].name, "on");
    controller.set_param_named(id, "mode", 1.0).unwrap();
    let (index, _, spec) = control_param(&controller, id.0, 0).unwrap();
    assert_eq!(spec.name, "hold");
    let node = controller.graph().node(id).unwrap();
    // The latch is kept, and the hold is released.
    assert_eq!(node.params[node.descriptor.param_index("on").unwrap()], 1.0);
    assert_eq!(node.params[index], 0.0);
}

/// Every built-in radio option shows in full on its tile: radio buttons
/// are wide enough that no choice name is cut with "…" (#394).
#[test]
fn no_built_in_radio_option_is_cut() {
    let registry = loom_modules::registry();
    let mut checked = 0;
    for d in registry.iter() {
        if !d
            .panel
            .widgets
            .iter()
            .any(|w| matches!(w.kind, WidgetKind::Radio | WidgetKind::Segmented { .. }))
        {
            continue;
        }
        let module = ModuleView {
            size: None,
            unknown: false,
            id: 0,
            title: d.name.to_string(),
            hover: loom_canvas::HoverText::default(),
            accent: family_colour(d.family),
            position: Point::ORIGIN,
            inputs: d.inputs.iter().map(|spec| port_view(spec, false)).collect(),
            outputs: d.outputs.iter().map(|spec| port_view(spec, false)).collect(),
            panel: panel_view(d),
        };
        loom_canvas::for_each_tile_text(&module, |item| {
            if matches!(item.kind, loom_canvas::TextKind::Option(..)) {
                checked += 1;
                assert!(!item.content.ends_with('…'), "{}: {} is cut", d.id, item.content);
            }
        });
    }
    assert!(checked > 20, "only {checked} radio and segment options found");
}

/// Every built-in select list's choices fit its closed box, so the
/// current choice is never cut (panels.md, Panel layout, principle 8).
/// The box leaves 50 world units for text: 6 grid units, less its
/// margins and arrow (`select_box`, and the room the canvas gives it).
#[test]
fn every_select_choice_fits_its_box() {
    let room = 6.0 * loom_canvas::GRID - 4.0 - 18.0;
    for d in loom_modules::registry().iter() {
        for w in d.panel.widgets.iter().filter(|w| w.kind == WidgetKind::Select) {
            let param = &d.params[d.param_index(w.bind).unwrap()];
            for choice in param.options.choices {
                let shown = loom_canvas::fit_label(choice, room, 9.0);
                assert_eq!(shown, *choice, "{}: `{choice}` is cut in its select box", d.id);
            }
        }
    }
}

/// A tile's header takes its family's colour, and its hover text names
/// the family first and the type ID as the name (ADR-0029).
#[test]
fn headers_show_their_family() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let cases = [
        ("loom.osc.sine", loom_core::Family::Sources),
        ("loom.mod.lfo", loom_core::Family::Modulation),
        ("loom.logic.divide", loom_core::Family::TimeAndRhythm),
        ("loom.time.delay1", loom_core::Family::LevelAndRouting),
    ];
    for (kind, _) in cases {
        parts.controller.add_module(kind).unwrap();
    }
    let scene = build(&parts.controller, &BTreeMap::new(), &BTreeMap::new());
    for (tile, (kind, family)) in scene.modules().iter().zip(cases) {
        assert_eq!(tile.accent, family_colour(family), "{kind}");
        assert!(
            tile.hover.description.starts_with(&format!("{}. ", family.label())),
            "{kind}"
        );
        assert_eq!(tile.hover.name.as_deref(), Some(kind));
    }
}

/// No built-in tile cuts a port label or caption, and no two pieces of
/// text on a tile overlap or touch, now that labels may grow into empty space
/// beside them (#455). Resonance, Bandpass, Highpass, Hysteresis,
/// Controller, Smoothing, and Semitones were cut before.
#[test]
fn no_built_in_tile_text_is_cut_or_overlaps() {
    let registry = loom_modules::registry();
    let mut problems = Vec::new();
    for d in registry.iter() {
        let module = ModuleView {
            size: None,
            unknown: false,
            id: 0,
            title: d.name.to_string(),
            hover: loom_canvas::HoverText::default(),
            accent: family_colour(d.family),
            position: Point::ORIGIN,
            inputs: d.inputs.iter().map(|spec| port_view(spec, false)).collect(),
            outputs: d.outputs.iter().map(|spec| port_view(spec, false)).collect(),
            panel: panel_view(d),
        };
        let mut boxes: Vec<(String, iced::Rectangle)> = Vec::new();
        loom_canvas::for_each_tile_text(&module, |item| {
            if item.content.ends_with('…') {
                problems.push(format!("{}: {} is cut", d.id, item.content));
            }
            let text = item.bounds();
            // At least one unit apart side by side, so two labels never
            // read as one.
            let spaced = iced::Rectangle {
                x: text.x - 1.0,
                width: text.width + 2.0,
                ..text
            };
            for (other, b) in &boxes {
                if spaced
                    .intersection(b)
                    .is_some_and(|i| i.width > 0.01 && i.height > 0.01)
                {
                    problems.push(format!("{}: {} touches {other}", d.id, item.content));
                }
            }
            boxes.push((item.content.to_string(), text));
        });
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn toggles_and_radio_buttons_use_whole_values() {
    let on = param("on", 0.0, 1.0, 0.0, "").integer();
    assert_eq!(control_value(WidgetKind::Toggle, &on, 1.0), 1.0);
    assert_eq!(control_position(WidgetKind::Toggle, &on, 1.0), 1.0);
    let slope = param("slope", 1.0, 4.0, 1.0, "").integer();
    let radio = WidgetKind::Radio;
    assert_eq!(control_value(radio, &slope, 2.0), 3.0);
    assert_eq!(control_position(radio, &slope, 3.0), 2.0);
    assert_eq!(control_value(radio, &slope, 9.0), 4.0);
}

#[test]
fn tiles_show_labels_not_names() {
    // panels.md, Labels and hover text: Clock Divide's `n` knob says
    // "Division", and its hover text keeps the name, which differs.
    let divide = &loom_modules::logic::DIVIDE;
    let knob = &panel_view(divide).widgets[0];
    assert_eq!(knob.label, "Division");
    assert_eq!(knob.hover.description, "Fires on every Nth trigger.");
    assert_eq!(knob.hover.name.as_deref(), Some("n"));
    let reset = port_view(&divide.inputs[1], false);
    assert_eq!(reset.label, "Reset");
    assert_eq!(reset.hover.name, None, "`reset` reads the same as \"Reset\"");
    assert!(reset.hover.description.contains("Usually 0 to 1."));
}

/// port-and-parameter-text.md, Display values: a light's caption is its
/// display value's label, and its hover text the description. A meter
/// takes both from the output it shows.
#[test]
fn meters_and_lights_show_labels_and_descriptions() {
    let meter_module = &loom_modules::display::METER;
    let meter = &panel_view(meter_module).widgets[0];
    // The meter has no caption: "Out" read as a port (panels.md, Panel
    // layout). Its hover text still names the output.
    assert_eq!(meter.label, "");
    assert_eq!(meter.hover.description, meter_module.outputs[0].description);
    let light = panel_view(&loom_modules::env::ENVELOPE)
        .widgets
        .into_iter()
        .find(|widget| widget.kind == WidgetViewKind::Light && widget.label == "Level")
        .unwrap();
    assert_eq!(light.kind, WidgetViewKind::Light);
    assert_eq!(light.label, "Level");
    assert_eq!(
        light.hover,
        HoverText::new("Level", loom_modules::env::ENVELOPE.displays[0].description, "level")
    );
    assert_eq!(light.hover.name, None, "`level` reads the same as \"Level\"");
    let gate = &panel_view(&loom_modules::midi::NOTE_IN)
        .widgets
        .last()
        .cloned()
        .unwrap();
    assert_eq!(gate.label, "Gate");
    assert_eq!(gate.hover.description, "Lit while a key is held.");
    let fired = &panel_view(&loom_modules::logic::DIVIDE).widgets[2];
    assert_eq!(fired.label, "Fired");
}

/// The value line of a light's or meter's hover text.
#[test]
fn lights_and_meters_say_their_value() {
    let envelope = &loom_modules::env::ENVELOPE;
    let level = envelope
        .panel
        .widgets
        .iter()
        .find(|widget| widget.bind == "level")
        .unwrap();
    assert_eq!(display_text(envelope, level, 0.5, None), "Level: 0.50");
    let divide = &loom_modules::logic::DIVIDE;
    assert_eq!(display_text(divide, &divide.panel.widgets[2], 0.4, None), "Fired: lit");
    assert_eq!(display_text(divide, &divide.panel.widgets[2], 0.0, None), "Fired: dark");
    let meter = &loom_modules::display::METER;
    assert_eq!(display_text(meter, &meter.panel.widgets[0], 0.8, None), "Out: -12.0 dB");
    assert_eq!(
        display_text(meter, &meter.panel.widgets[0], 0.0, None),
        "Out: below -60 dB"
    );
    assert_eq!(
        display_text(meter, &meter.panel.widgets[0], 0.0, Some((0.8, 0.9))),
        "Out: L below -60 dB, R -12.0 dB"
    );
    assert_eq!(
        display_text(meter, &meter.panel.widgets[0], 0.8, Some((0.0, 0.0))),
        "Out: L -12.0 dB, R below -60 dB"
    );
    // A control has no value line here: its text is the parameter value.
    assert_eq!(display_text(divide, &divide.panel.widgets[0], 0.5, None), "");
}

/// A choice widget reads its names from the parameter (panels.md,
/// Choice names), so it shows the same words as the inspector.
#[test]
fn choice_widgets_show_the_parameter_choice_names() {
    let lfo = &loom_modules::lfo::LFO;
    let view = panel_view(lfo);
    let WidgetViewKind::Segmented { options, .. } = &view.widgets[1].kind else {
        panic!("the sync mode is a segmented control");
    };
    let shape = &lfo.params[lfo.param_index("sync").unwrap()];
    assert_eq!(
        options,
        &shape.options.choices.iter().map(|o| o.to_string()).collect::<Vec<_>>()
    );
}

#[test]
fn every_built_in_panel_reaches_the_canvas() {
    for d in loom_modules::ALL {
        let view = panel_view(d);
        assert_eq!(view.widgets.len(), d.panel.widgets.len(), "{}", d.id);
        for (w, v) in d.panel.widgets.iter().zip(&view.widgets) {
            // Sizes must agree, or hit-testing would miss the drawing.
            assert_eq!(d.widget_size(w), v.size(), "{}.{}", d.id, w.bind);
        }
    }
}

/// A parameter edit changes the result samples in panel values without
/// changing the module's panel declaration or rebuilding the scene.
#[test]
fn envelope_preview_follows_parameter_edits() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let envelope = parts.controller.add_module("loom.env.envelope").unwrap();
    let mut values = PanelValues::new();
    update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
    let before = values.get(envelope.0, 0).unwrap().shape.clone();
    let panel = panel_view(&loom_modules::env::ENVELOPE);
    assert_eq!(panel.widgets[0].label, "Shape");
    assert!(!panel.widgets[0].is_control());
    assert!(matches!(panel.widgets[1].kind, WidgetViewKind::Segmented { .. }));
    let still_generation = values.generation();
    update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
    assert_eq!(
        values.generation(),
        still_generation,
        "an unchanged preview needs no redraw"
    );
    parts.controller.set_param_named(envelope, "attack", 1.0).unwrap();
    update_panel_values(&mut values, &parts.controller, &BTreeMap::new());
    assert_ne!(values.get(envelope.0, 0).unwrap().shape, before);
    assert_eq!(panel, panel_view(&loom_modules::env::ENVELOPE));
}

/// Envelope in AD mode hides Sustain and Release: the tile keeps its
/// size, and Attack, Decay, and Curve close up, centred in their row
/// (ADR-0030). In ADSR mode all five show where they were declared.
#[test]
fn a_mode_hides_the_controls_it_does_not_use() {
    let d = &loom_modules::env::ENVELOPE;
    let mut values: Vec<f32> = d.params.iter().map(|p| p.default).collect();
    let adsr = panel_for(d, &values);
    assert!(adsr.widgets.iter().all(|w| !w.hidden));
    values[d.param_index("mode").unwrap()] = 1.0;
    let ad = panel_for(d, &values);
    assert_eq!((ad.width, ad.height), (adsr.width, adsr.height), "same size");
    let shown: Vec<_> = ad.widgets.iter().filter(|w| w.y == 7 && !w.hidden).collect();
    let labels: Vec<_> = shown.iter().map(|w| w.label.as_str()).collect();
    assert_eq!(labels, ["Attack", "Decay", "Curve"]);
    // Nine units of knobs, centred in the row's fifteen.
    assert_eq!(shown.iter().map(|w| w.x).collect::<Vec<_>>(), [3, 6, 9]);
    let hidden: Vec<_> = ad
        .widgets
        .iter()
        .filter(|w| w.hidden)
        .map(|w| w.label.as_str())
        .collect();
    assert_eq!(hidden, ["Sustain", "Release"]);
}

/// Rows that only hidden widgets used are removed, so the panel gets
/// shorter, and widgets below them move up; the width never changes.
#[test]
fn hiding_a_whole_row_shortens_the_panel() {
    let knob = |x, y, label: &str| WidgetView {
        kind: WidgetViewKind::Knob { steps: 0 },
        x,
        y,
        label: label.into(),
        hover: HoverText::default(),
        hidden: false,
    };
    let mut panel = PanelView {
        width: 6,
        height: 9,
        widgets: vec![knob(0, 0, "A"), knob(0, 3, "B"), knob(3, 3, "C"), knob(0, 6, "D")],
    };
    close_up(&mut panel, &[false, true, true, false]);
    assert_eq!((panel.width, panel.height), (6, 6));
    assert_eq!((panel.widgets[3].x, panel.widgets[3].y), (0, 3), "D moved up");
}

/// The inspector keeps a hidden parameter and says which choice does
/// not use it (ADR-0030).
#[test]
fn the_inspector_marks_a_hidden_parameter() {
    let mut app = crate::tests::app();
    let env = app.controller.add_module("loom.env.envelope").unwrap();
    app.controller.set_param_named(env, "mode", 1.0).unwrap();
    let _ = app.update(crate::Message::SelectModule(env.0));
    let crate::inspector::Inspector::Module(rows) = app.inspector_model() else {
        panic!("the envelope's rows");
    };
    let note = |label: &str| rows.params.iter().find(|p| p.label == label).unwrap().unused.clone();
    assert_eq!(note("Sustain").as_deref(), Some("Not used in AD"));
    assert_eq!(note("Attack"), None);
}

/// `examples/dev/all-modules.ron` holds every built-in module once, and
/// lays out with no tile over another, so panel changes can be checked
/// side by side (panels.md, Panel layout).
#[test]
fn the_all_modules_patch_has_every_module_without_overlap() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/dev/all-modules.ron");
    let app = crate::App::new(crate::Args {
        patch: Some(path),
        ..crate::tests::app_args()
    });
    let graph = app.controller.graph();
    let mut kinds: Vec<&str> = graph.nodes().map(|(_, node)| node.descriptor.id).collect();
    kinds.sort_unstable();
    let mut registry: Vec<&str> = loom_modules::registry().iter().map(|d| d.id).collect();
    registry.sort_unstable();
    assert_eq!(kinds, registry, "every built-in module, once");
    let tiles: Vec<_> = app
        .scene
        .modules()
        .iter()
        .map(|m| (m.title.clone(), m.bounds()))
        .collect();
    for (i, (a, ra)) in tiles.iter().enumerate() {
        for (b, rb) in &tiles[i + 1..] {
            assert!(ra.intersection(rb).is_none(), "{a} overlaps {b}");
        }
    }
}
