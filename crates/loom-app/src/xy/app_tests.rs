use crate::Message;
use crate::ui_test::Ui;
use loom_canvas::CanvasEvent;
use loom_core::ModuleId;

/// A patch with one XY module, with its display on screen.
fn with_xy() -> (Ui, u32) {
    let mut ui = Ui::open("sine.ron");
    let xy = ui.app.session.controller.add_module("loom.display.xy").unwrap().0;
    ui.app.commit();
    ui.redraw();
    let _ = ui.app.send(Message::Canvas(CanvasEvent::VisibleDisplays(vec![xy])));
    (ui, xy)
}

/// The two outputs of an XY module.
fn outputs(ui: &Ui, xy: u32) -> (loom_core::OutputRef, loom_core::OutputRef) {
    let c = &ui.app.session.controller;
    (
        c.output_ref(ModuleId(xy), "x_out").unwrap(),
        c.output_ref(ModuleId(xy), "y_out").unwrap(),
    )
}

/// On screen it has one paired probe, and off screen none, so a scope nobody
/// sees costs no engine work.
#[test]
fn a_pair_follows_the_screen() {
    let (mut ui, xy) = with_xy();
    let (x, y) = outputs(&ui, xy);
    let probe = ui
        .app
        .session
        .engine
        .live
        .pair_probe(x, y)
        .expect("a probe while on screen");
    assert!(ui.app.session.engine.monitor.watched().xy.contains_key(&probe));
    let _ = ui.app.send(Message::Canvas(CanvasEvent::VisibleDisplays(Vec::new())));
    assert_eq!(ui.app.session.engine.live.pair_probe(x, y), None);
    assert!(!ui.app.session.controller.has_probe_pair(probe));
    assert!(ui.app.session.engine.monitor.watched().xy.is_empty());
}

/// The widget is in the UI snapshot, with its value as text and a place, and a
/// control-channel name finds it (panel-widgets.md, Acceptance criteria).
#[test]
fn the_snapshot_reports_it() {
    let (mut ui, xy) = with_xy();
    ui.redraw();
    let snapshot = ui.app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|tile| tile.id == xy).unwrap();
    let widget = tile.widgets.iter().find(|w| w.kind == "xy_scope").expect("the widget");
    assert_eq!(widget.text, "XY scope of X and Y");
    assert!(widget.bounds.is_some());
    let part = crate::targets::find(
        ui.app.session.controller.graph(),
        &ui.app.session.hierarchy,
        xy,
        "display:x_out",
    );
    assert_eq!(part, Ok(crate::targets::Part::Display(xy, 0)));
}

/// A press on it selects the module and changes nothing, because it is a
/// display: it is not a control and takes no focus.
#[test]
fn it_is_a_display() {
    let (mut ui, xy) = with_xy();
    let module = ui.app.session.scene.module(xy).unwrap();
    assert!(!module.panel.widgets[0].is_control());
    let rect = module.widget_bounds(0).unwrap();
    let at = ui.canvas().to_window(rect.center());
    ui.move_to(at);
    ui.press();
    ui.release();
    assert_eq!(ui.app.session.selection, [xy].into());
}

/// Its trace reaches the tile from the telemetry thread's update, for the
/// widget that shows its pair.
#[test]
fn a_trace_reaches_the_widget() {
    let (mut ui, xy) = with_xy();
    let (x, y) = outputs(&ui, xy);
    let probe = ui.app.session.engine.live.pair_probe(x, y).unwrap();
    let trace = loom_canvas::Trace {
        points: vec![[0.25, 0.5]],
        ..loom_canvas::Trace::default()
    };
    let _ = ui.app.send(Message::Telemetry(crate::monitor::Update {
        xy: vec![(probe, trace.clone())],
        ..crate::monitor::Update::default()
    }));
    assert_eq!(ui.app.session.traces.get(xy, 0), Some(&trace));
}
