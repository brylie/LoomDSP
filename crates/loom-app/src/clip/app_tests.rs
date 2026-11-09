use super::*;
use crate::Message;
use crate::monitor::Update;
use crate::ui_test::Ui;
use loom_canvas::CanvasEvent;
use loom_core::ModuleId;
use loom_ui_core::Point;

/// A patch with one Level Meter, with its display on screen.
fn with_meter() -> (Ui, u32) {
    let mut ui = Ui::open("sine.ron");
    let meter = ui.app.session.controller.add_module("loom.display.meter").unwrap().0;
    ui.app.session.positions.insert(meter, (600.0, 0.0));
    ui.app.commit();
    ui.redraw();
    show(&mut ui, meter);
    (ui, meter)
}

/// Tells the app that only this module's displays are on screen.
fn show(ui: &mut Ui, module: u32) {
    let _ = ui
        .app
        .update(Message::Canvas(CanvasEvent::VisibleDisplays(vec![module])));
}

/// Sends a peak on the meter's current probe, as the telemetry thread does.
fn send_peak(ui: &mut Ui, meter: u32, peak: f32) {
    let probe = ui
        .app
        .session
        .engine
        .live
        .probe(output(ui, meter))
        .expect("a probe while on screen");
    let after = ui.app.peak_resets().get(&probe).copied().unwrap_or(0);
    let news = PeakNews {
        peak,
        bad: false,
        after,
    };
    let _ = ui.app.update(Message::Telemetry(Update {
        peaks: vec![(probe, news)],
        ..Update::default()
    }));
    ui.redraw();
}

/// The Level Meter's output.
fn output(ui: &Ui, meter: u32) -> loom_core::OutputRef {
    ui.app.session.controller.output_ref(ModuleId(meter), "out").unwrap()
}

/// What the meter draws: the light, the readout.
fn drawn(ui: &Ui, meter: u32) -> (bool, String) {
    let value = ui.app.session.values.get(meter, 0).unwrap();
    (value.clip, value.hold.clone())
}

/// A window point in the clip light, and one in the bar.
fn targets(ui: &mut Ui, meter: u32) -> (Point, Point) {
    let rect = ui.app.session.scene.module(meter).unwrap().widget_bounds(0).unwrap();
    let view = ui.canvas();
    (
        view.to_window(Point::new(rect.center_x(), rect.y + 4.0)),
        view.to_window(rect.center()),
    )
}

/// A click: pointer there, press, release.
fn click(ui: &mut Ui, at: Point) {
    ui.move_to(at);
    ui.press();
    ui.release();
}

/// Before any signal the readout says so, and the light is off.
#[test]
fn a_new_meter_shows_no_peak() {
    let (ui, meter) = with_meter();
    assert_eq!(drawn(&ui, meter), (false, "--".to_string()));
}

/// A peak of 1.0 lights the light, and the readout holds the highest peak in
/// dB (panels.md, Acceptance criteria: scope module).
#[test]
fn a_peak_of_one_lights_the_light() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 0.5);
    assert_eq!(drawn(&ui, meter), (false, "-6.0 dB".to_string()));
    send_peak(&mut ui, meter, 1.0);
    assert_eq!(drawn(&ui, meter), (true, "+0.0 dB".to_string()));
    send_peak(&mut ui, meter, 2.0);
    send_peak(&mut ui, meter, 0.25);
    assert_eq!(drawn(&ui, meter), (true, "+6.0 dB".to_string()), "the highest stays");
    let text = &ui.app.session.values.get(meter, 0).unwrap().text;
    assert!(text.contains("Would clip at the output"), "{text}");
}

/// The light clears on a click, and the readout stays. A press alone does not
/// clear it.
#[test]
fn a_click_clears_the_light_on_release() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 2.0);
    let (light, _) = targets(&mut ui, meter);
    ui.move_to(light);
    ui.press();
    assert!(drawn(&ui, meter).0, "still lit while pressed");
    ui.release();
    assert_eq!(drawn(&ui, meter), (false, "+6.0 dB".to_string()));
}

/// A drag from the light moves the tile and leaves it lit.
#[test]
fn a_drag_from_the_light_moves_the_tile_and_leaves_it_lit() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 2.0);
    let before = ui.app.session.positions[&meter];
    let (light, _) = targets(&mut ui, meter);
    ui.drag(light, Point::new(light.x + 60.0, light.y + 60.0), 4);
    assert_ne!(ui.app.session.positions[&meter], before, "the tile moved");
    assert!(drawn(&ui, meter).0, "still lit");
}

/// A click on the bar selects the module and clears nothing.
#[test]
fn a_click_on_the_bar_clears_nothing() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 2.0);
    let (_, bar) = targets(&mut ui, meter);
    click(&mut ui, bar);
    assert_eq!(drawn(&ui, meter), (true, "+6.0 dB".to_string()));
    assert_eq!(ui.app.session.selection, [meter].into());
}

/// A double-click on the meter resets the light and the readout.
#[test]
fn a_double_click_resets_both() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 2.0);
    let (_, bar) = targets(&mut ui, meter);
    click(&mut ui, bar);
    click(&mut ui, bar);
    assert_eq!(drawn(&ui, meter), (false, "--".to_string()));
}

/// The state lasts while the meter leaves the screen and comes back, though
/// its probe does not. A pan away ends the probe, and the new one has a new ID.
#[test]
fn the_state_survives_panning_away() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 2.0);
    let first = ui.app.session.engine.live.probe(output(&ui, meter)).unwrap();
    let _ = ui.app.update(Message::Canvas(CanvasEvent::VisibleDisplays(Vec::new())));
    show(&mut ui, meter);
    let second = ui.app.session.engine.live.probe(output(&ui, meter)).unwrap();
    assert_ne!(first, second, "a new probe");
    ui.redraw();
    assert_eq!(drawn(&ui, meter), (true, "+6.0 dB".to_string()));
    send_peak(&mut ui, meter, 0.25);
    assert_eq!(
        drawn(&ui, meter),
        (true, "+6.0 dB".to_string()),
        "a quieter probe changes nothing"
    );
}

/// A peak sent before the latest reset is dropped, so a reset holds though a
/// message was on its way.
#[test]
fn a_peak_from_before_a_reset_is_dropped() {
    let (mut ui, meter) = with_meter();
    let probe = ui.app.session.engine.live.probe(output(&ui, meter)).unwrap();
    let old = PeakNews {
        peak: 2.0,
        bad: false,
        after: ui.app.peak_resets()[&probe],
    };
    ui.app.reset_clip((meter, 0), true);
    let _ = ui.app.update(Message::Telemetry(Update {
        peaks: vec![(probe, old)],
        ..Update::default()
    }));
    assert_eq!(drawn(&ui, meter), (false, "--".to_string()));
    assert_eq!(
        ui.app.session.engine.monitor.watched().peak_resets[&probe],
        1,
        "the thread is told"
    );
}

/// A meter on a display value has no light and no readout.
#[test]
fn a_meter_on_a_display_value_has_neither() {
    let mut ui = Ui::open("sine.ron");
    let dynamics = ui.app.session.controller.add_module("loom.dyn.dynamics").unwrap().0;
    ui.app.commit();
    ui.redraw();
    let module = ui.app.session.scene.module(dynamics).unwrap();
    let index = module
        .panel
        .widgets
        .iter()
        .position(|w| matches!(w.kind, loom_canvas::WidgetViewKind::Meter { .. }))
        .unwrap();
    assert!(matches!(
        module.panel.widgets[index].kind,
        loom_canvas::WidgetViewKind::Meter { clips: false, .. }
    ));
    assert_eq!(ui.app.session.values.get(dynamics, index).unwrap().hold, "");
}

/// A keyboard user resets it from the inspector.
#[test]
fn the_inspector_resets_it() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 2.0);
    let (_, bar) = targets(&mut ui, meter);
    click(&mut ui, bar);
    ui.redraw();
    let reset = ui.text("Reset clip").expect("the button").center();
    click(&mut ui, reset);
    assert_eq!(drawn(&ui, meter), (false, "--".to_string()));
}

/// A module that has no meter on an output has no such button.
#[test]
fn other_modules_have_no_reset_button() {
    let mut ui = Ui::open("sine.ron");
    let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![3])));
    ui.redraw();
    assert!(ui.text("Reset clip").is_none());
}

/// A removed module's state goes with it.
#[test]
fn a_removed_module_forgets_its_state() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 2.0);
    ui.app.session.controller.remove_module(ModuleId(meter)).unwrap();
    ui.app.commit();
    show(&mut ui, 3);
    assert_eq!(ui.app.session.clip_holds.get((meter, 0)), Hold::default());
}

/// News for one meter, sent before another meter's reset, still counts, because
/// each meter has its own reset count.
#[test]
fn a_reset_of_one_meter_keeps_a_clip_on_another() {
    let (mut ui, first) = with_meter();
    let second = ui.app.session.controller.add_module("loom.display.meter").unwrap().0;
    ui.app.session.positions.insert(second, (900.0, 0.0));
    ui.app.commit();
    ui.redraw();
    let _ = ui
        .app
        .update(Message::Canvas(CanvasEvent::VisibleDisplays(vec![first, second])));
    let probe = ui.app.session.engine.live.probe(output(&ui, second)).unwrap();
    let news = PeakNews {
        peak: 2.0,
        bad: false,
        after: ui.app.peak_resets()[&probe],
    };
    ui.app.reset_clip((first, 0), false);
    let _ = ui.app.update(Message::Telemetry(Update {
        peaks: vec![(probe, news)],
        ..Update::default()
    }));
    ui.redraw();
    assert_eq!(drawn(&ui, second), (true, "+6.0 dB".to_string()));
}

/// At far zoom neither the light nor the readout is drawn, so a click or a
/// double-click there only selects the module. The light may be the only record
/// of a clip, because no probe runs out there.
#[test]
fn far_zoom_clicks_clear_nothing() {
    let (mut ui, meter) = with_meter();
    send_peak(&mut ui, meter, 2.0);
    let (light, _) = targets(&mut ui, meter);
    ui.move_to(light);
    for _ in 0..40 {
        ui.step(&[iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        })]);
    }
    assert!(ui.canvas().camera.zoom < 0.35, "far out: {}", ui.canvas().camera.zoom);
    let (light, bar) = targets(&mut ui, meter);
    click(&mut ui, light);
    assert_eq!(ui.app.session.selection, [meter].into());
    assert_eq!(drawn(&ui, meter), (true, "+6.0 dB".to_string()), "a click");
    click(&mut ui, bar);
    click(&mut ui, bar);
    assert_eq!(drawn(&ui, meter), (true, "+6.0 dB".to_string()), "a double-click");
}
