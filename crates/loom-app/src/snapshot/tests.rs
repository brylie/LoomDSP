use super::*;
use std::path::PathBuf;

/// The app with a patch from `examples/patches` and no audio.
fn open(patch: &str) -> App {
    App::new(Args {
        patch: Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/patches")
                .join(patch),
        ),
        ..crate::tests::app_args()
    })
}

/// The same focus hint reaches a command-line dump as a keyboard focus change.
#[test]
fn a_dump_hints_at_typing_only_on_a_one_number_control() {
    use clap::Parser;
    let patch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/dev/test-widgets.ron");
    for (control, can_type) in [("2:tempo", true), ("2:on", false), ("2:long", false)] {
        let args = Args::try_parse_from([
            "loom-app",
            patch.to_str().unwrap(),
            "--test-modules",
            "--dump-ui",
            "--focus",
            control,
        ])
        .unwrap();
        let snapshot = headless(&args).unwrap().ui_snapshot();
        assert_eq!(snapshot.status.contains("Enter types a value"), can_type, "{control}");
    }
}

/// The dump reports the exact focused caption string the tile draws.
#[test]
fn a_dump_shows_the_focused_value_in_place_of_the_caption() {
    use clap::Parser;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
    let path = path.to_str().unwrap();
    let args = Args::try_parse_from(["loom-app", path, "--dump-ui", "--focus", "4:cutoff"]).unwrap();
    let app = headless(&args).unwrap();
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|tile| tile.id == 4).unwrap();
    let (index, cutoff) = tile
        .widgets
        .iter()
        .enumerate()
        .find(|(_, w)| w.label == "Cutoff")
        .unwrap();
    let module = app.session.scene.module(4).unwrap();
    loom_canvas::for_each_tile_text(module, |item| {
        if item.kind == TextKind::Caption(index) {
            assert_eq!(
                cutoff.shown.as_deref(),
                Some(item.fit_value(module, &cutoff.text).as_ref())
            );
        }
    });
}

/// A learned mapping replaces the focused value in the snapshot while Learn is on.
#[test]
fn a_focused_mapped_caption_shows_its_mapping_in_learn_mode() {
    let mut app = open("subtractive.ron");
    let id = loom_core::ModuleId(4);
    let parameter = app
        .session
        .controller
        .graph()
        .node(id)
        .unwrap()
        .descriptor
        .param_index("cutoff")
        .unwrap();
    app.session
        .controller
        .map_midi(
            loom_core::MidiControl {
                channel: 0,
                controller: 74,
            },
            id,
            parameter,
        )
        .unwrap();
    app.commit();
    let index =
        crate::surface::find_control(app.session.controller.graph(), &app.session.hierarchy, id.0, "cutoff").unwrap();
    app.a11y.set_focus(Some((id.0, index)));
    let shown = |app: &App| {
        app.ui_snapshot()
            .tiles
            .into_iter()
            .find(|tile| tile.id == id.0)
            .unwrap()
            .widgets[index]
            .shown
            .clone()
    };

    let value = shown(&app);
    assert!(value.as_deref().is_some_and(|text| text != "CC 74"));
    app.learn_message(crate::learn::Message::Toggle);
    assert_eq!(shown(&app).as_deref(), Some("CC 74"));
    app.learn_message(crate::learn::Message::Toggle);
    assert_eq!(shown(&app), value);
}

/// A label that fits has no `shown`. SVF's Resonance caption was cut
/// until labels could use the empty space beside them (#455).
#[test]
fn a_label_that_fits_has_no_shown() {
    let snapshot = open("subtractive.ron").ui_snapshot();
    let svf = snapshot.tiles.iter().find(|t| t.title == "SVF").unwrap();
    for label in ["Resonance", "Cutoff"] {
        let widget = svf.widgets.iter().find(|w| w.label == label).unwrap();
        assert_eq!(widget.shown, None, "{label}");
        assert!(serde_json::to_value(widget).unwrap().get("shown").is_none());
    }
}

/// A caption with no room even beside it is cut, and the snapshot says
/// what is drawn: the canvas's own text layout decides.
#[test]
fn a_cut_label_shows_what_the_tile_draws() {
    let knob = |x, label: &str| loom_canvas::WidgetView {
        kind: WidgetViewKind::Knob { steps: 0 },
        x,
        y: 0,
        label: label.into(),
        hover: loom_canvas::HoverText::default(),
        hidden: false,
        exposed: false,
        inert: false,
    };
    // Two long captions on touching knobs, filling the panel.
    let module = ModuleView {
        size: None,
        unknown: false,
        id: 1,
        title: "Cramped".into(),
        hover: loom_canvas::HoverText::default(),
        accent: loom_ui_core::Color::BLACK,
        position: loom_ui_core::Point::ORIGIN,
        inputs: Vec::new(),
        outputs: Vec::new(),
        panel: loom_canvas::PanelView {
            width: 6,
            height: 3,
            widgets: vec![knob(0, "Much longer caption"), knob(3, "Another long caption")],
        },
    };
    let cut = cut_labels(&module, None, None);
    for index in 0..2 {
        let shown = cut.get(&TextKind::Caption(index)).expect("cut");
        assert!(shown.ends_with('…'), "{shown}");
    }
}

/// Every label the example patches cut, so a change that cuts more is
/// noticed. Update the list when a cut is meant.
#[test]
fn the_example_patches_cut_only_these_labels() {
    let mut cut = Vec::new();
    for patch in [
        "fm-feedback-delay1.ron",
        "fm-feedback.ron",
        "plucked-string.ron",
        "polysynth.ron",
        "sine.ron",
        "subtractive.ron",
    ] {
        for tile in open(patch).ui_snapshot().tiles {
            let ports = tile.inputs.iter().map(|p| (&p.label, &p.shown));
            let ports = ports.chain(tile.outputs.iter().map(|p| (&p.label, &p.shown)));
            let all = ports.chain(tile.widgets.iter().map(|w| (&w.label, &w.shown)));
            for (label, shown) in all {
                if let Some(shown) = shown {
                    cut.push(format!("{patch} {}: {label} -> {shown}", tile.title));
                }
            }
        }
    }
    // None since labels use the empty space beside them (#455).
    let expected: [&str; 0] = [];
    assert_eq!(cut, expected);
}

/// With no audio the stats show the label and zero load, and the
/// snapshot has no pointer: nothing drew one.
#[test]
fn the_stats_without_audio() {
    let snapshot = open("sine.ron").ui_snapshot();
    assert!(!snapshot.stats.playing);
    assert_eq!(snapshot.stats.load, 0.0);
    assert_eq!(snapshot.stats.max_load, 0.0);
    assert_eq!(snapshot.stats.dropouts, 0);
    assert!(snapshot.stats.label.contains("no audio"), "{:?}", snapshot.stats);
    assert_eq!(snapshot.pointer, None);
}

/// With `--fake-audio` the clock plays: the label names it, the bar
/// shows the load, and no dropouts are counted.
#[test]
fn the_stats_name_the_clock_with_fake_audio() {
    let app = App::new(Args {
        fake_audio: true,
        no_audio: false,
        ..crate::tests::app_args()
    });
    let stats = app.ui_snapshot().stats;
    assert!(stats.playing);
    assert!(stats.label.contains("clock"), "{stats:?}");
    assert_eq!(stats.dropouts, 0);
    // The fake clock plays on its own thread, so the load can move
    // between the two reads. Everything else must match.
    let steady = |s: &Stats| Stats {
        load: 0.0,
        max_load: 0.0,
        ..s.clone()
    };
    assert_eq!(
        steady(&stats),
        steady(&app.stats_model()),
        "the bar and the snapshot read one model"
    );
    assert!(stats.load_text().starts_with("load "), "{}", stats.load_text());
}

/// A headless run carries the scope's trace into the UI snapshot.
#[test]
fn running_the_subtractive_patch_shows_scope_trace() {
    let mut app = open("subtractive.ron");
    app.run_engine(4_800);
    let scope = app
        .ui_snapshot()
        .tiles
        .into_iter()
        .find(|tile| tile.title == "Scope")
        .expect("the example has a scope");
    let trace = scope
        .widgets
        .iter()
        .find_map(|widget| widget.trace)
        .expect("scope trace");
    assert!(trace.points > 100);
    assert!(trace.high > trace.low);
}

/// Headless activity and meter text include a signal only on the right.
#[test]
fn headless_snapshot_shows_right_only_stereo_activity() {
    let mut app = crate::tests::app();
    let source = app.session.controller.add_module("loom.math.constant").unwrap();
    let join = app.session.controller.add_module("loom.mix.join").unwrap();
    let meter = app.session.controller.add_module("loom.display.meter").unwrap();
    let output = app.session.controller.add_module("loom.io.audio-out").unwrap();
    app.session.controller.set_param_named(source, "value", 0.5).unwrap();
    app.session
        .controller
        .connect_named(source, "out", join, "right")
        .unwrap();
    app.session.controller.connect_named(join, "out", meter, "in").unwrap();
    app.session
        .controller
        .connect_named(meter, "out", output, "in")
        .unwrap();
    assert!(app.commit());
    app.run_engine(4_800);
    let snapshot = app.ui_snapshot();
    let join_tile = snapshot.tiles.iter().find(|tile| tile.id == join.0).unwrap();
    assert!(join_tile.outputs[0].carrying);
    let meter_tile = snapshot.tiles.iter().find(|tile| tile.id == meter.0).unwrap();
    assert!(meter_tile.inputs[0].carrying);
    assert!(meter_tile.outputs[0].carrying);
    let value = &meter_tile.widgets[0];
    assert_eq!(value.position, 0.0);
    assert!(value.right.unwrap().0 > 0.0);
    assert!(value.text.starts_with("Out: L below -60 dB, R "));

    app.session
        .controller
        .set_param_named(source, "value", 0.00005)
        .unwrap();
    app.run_engine(9_600);
    let quiet = app.ui_snapshot();
    assert!(!quiet.tiles.iter().find(|tile| tile.id == join.0).unwrap().outputs[0].carrying);

    app.session.controller.set_param_named(source, "value", -0.5).unwrap();
    app.run_engine(4_800);
    let negative = app.ui_snapshot();
    assert!(negative.tiles.iter().find(|tile| tile.id == join.0).unwrap().outputs[0].carrying);
}

/// Empty and nonempty traces give bounded, ordered snapshot values.
#[test]
fn trace_snapshot_bounds() {
    let empty = TraceShown::from(&loom_canvas::Trace {
        points: Vec::new(),
        second: Vec::new(),
        level: 0.0,
        ..Default::default()
    });
    assert_eq!((empty.points, empty.low, empty.high), (0, 0.0, 0.0));
    let shown = TraceShown::from(&loom_canvas::Trace {
        points: vec![[0.0, -0.5], [1.0, 0.75]],
        second: Vec::new(),
        level: 0.8,
        ..Default::default()
    });
    assert_eq!((shown.points, shown.low, shown.high, shown.level), (2, -0.5, 0.75, 0.8));
}

/// `--frames` for longer than the telemetry queues hold, with a scope and an
/// XY scope on screen, drops nothing: the run reads the queues after every
/// block of its lead-in too (control-channel.md, Stats and cut labels in the
/// snapshot). Four seconds is past what each queue holds: about 1.4 s of
/// scope or pair chunks, and 2.7 s of meter readings at 128 frames a block.
#[test]
fn a_long_run_drops_no_telemetry() {
    let mut app = open("subtractive.ron");
    let c = &mut app.session.controller;
    let xy = c.add_module("loom.display.xy").unwrap();
    let lfo = c.add_module("loom.mod.lfo").unwrap();
    let meter = c.add_module("loom.display.meter").unwrap();
    c.connect_named(lfo, "out", xy, "x").unwrap();
    c.connect_named(lfo, "out90", xy, "y").unwrap();
    c.connect_named(lfo, "out", meter, "in").unwrap();
    assert!(app.commit());
    app.run_engine(4 * 48_000);
    let watch = app.session.engine.monitor.watched();
    assert!(
        !watch.scopes.is_empty() && !watch.xy.is_empty(),
        "a scope and an XY scope on screen"
    );
    assert!(!watch.meters.is_empty(), "and meters");
    assert_eq!(app.ui_snapshot().stats.telemetry_dropped, 0);
}

/// A clip during the lead-in lights the meter's clip light, and its peak
/// stays in the readout. An AD envelope fires once as its gate goes high at
/// the start, added to itself so it peaks at 2.0, and is back at 0 after a
/// fifth of a second. The last second of a 3 s run is silent, so only the
/// lead-in can light the light.
#[test]
fn a_clip_in_the_lead_in_lights_the_meter() {
    let mut app = crate::tests::app();
    let c = &mut app.session.controller;
    let gate = c.add_module("loom.math.constant").unwrap();
    c.set_param_named(gate, "value", 1.0).unwrap();
    let envelope = c.add_module("loom.env.envelope").unwrap();
    c.set_param_named(envelope, "mode", 1.0).unwrap(); // AD: once
    let twice = c.add_module("loom.math.add").unwrap();
    let meter = c.add_module("loom.display.meter").unwrap();
    c.connect_named(gate, "out", envelope, "gate").unwrap();
    c.connect_named(envelope, "out", twice, "a").unwrap();
    c.connect_named(envelope, "out", twice, "b").unwrap();
    c.connect_named(twice, "out", meter, "in").unwrap();
    assert!(app.commit());
    app.run_engine(3 * 48_000);
    let tile = app.ui_snapshot().tiles.into_iter().find(|t| t.id == meter.0).unwrap();
    assert_eq!(tile.widgets[0].position, 0.0, "the run ends silent");
    let hold = app.session.clip_holds.get((meter.0, 0));
    assert!(hold.lit, "the lead-in's clip lights the light");
    assert!(hold.peak > 1.9, "the readout holds the lead-in's peak: {}", hold.peak);
}
