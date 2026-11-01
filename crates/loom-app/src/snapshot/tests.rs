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
    };
    // Two long captions on touching knobs, filling the panel.
    let module = ModuleView {
        size: None,
        unknown: false,
        id: 1,
        title: "Cramped".into(),
        hover: loom_canvas::HoverText::default(),
        accent: iced::Color::BLACK,
        position: iced::Point::ORIGIN,
        inputs: Vec::new(),
        outputs: Vec::new(),
        panel: loom_canvas::PanelView {
            width: 6,
            height: 3,
            widgets: vec![knob(0, "Much longer caption"), knob(3, "Another long caption")],
        },
    };
    let cut = cut_labels(&module);
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
    let source = app.controller.add_module("loom.math.constant").unwrap();
    let join = app.controller.add_module("loom.mix.join").unwrap();
    let meter = app.controller.add_module("loom.display.meter").unwrap();
    let output = app.controller.add_module("loom.io.audio-out").unwrap();
    app.controller.set_param_named(source, "value", 0.5).unwrap();
    app.controller.connect_named(source, "out", join, "right").unwrap();
    app.controller.connect_named(join, "out", meter, "in").unwrap();
    app.controller.connect_named(meter, "out", output, "in").unwrap();
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

    app.controller.set_param_named(source, "value", 0.00005).unwrap();
    app.run_engine(9_600);
    let quiet = app.ui_snapshot();
    assert!(!quiet.tiles.iter().find(|tile| tile.id == join.0).unwrap().outputs[0].carrying);

    app.controller.set_param_named(source, "value", -0.5).unwrap();
    app.run_engine(4_800);
    let negative = app.ui_snapshot();
    assert!(negative.tiles.iter().find(|tile| tile.id == join.0).unwrap().outputs[0].carrying);
}

/// Empty and nonempty traces give bounded, ordered snapshot values.
#[test]
fn trace_snapshot_bounds() {
    let empty = TraceShown::from(&loom_canvas::Trace {
        points: Vec::new(),
        level: 0.0,
    });
    assert_eq!((empty.points, empty.low, empty.high), (0, 0.0, 0.0));
    let shown = TraceShown::from(&loom_canvas::Trace {
        points: vec![[0.0, -0.5], [1.0, 0.75]],
        level: 0.8,
    });
    assert_eq!((shown.points, shown.low, shown.high, shown.level), (2, -0.5, 0.75, 0.8));
}
