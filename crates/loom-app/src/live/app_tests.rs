//! What a screen reader hears of a scope whose window holds a value that is
//! not finite (`docs/design/panels.md`, Drawing the scope trace, Values that
//! are not finite).

use super::*;
use crate::Message;
use crate::monitor::Update;
use crate::ui_test::Ui;
use loom_canvas::CanvasEvent;

/// The subtractive example, with its scope on screen: the scope's module and
/// widget, and its probe.
fn with_scope() -> (Ui, (u32, usize), ProbeId) {
    let mut ui = Ui::open("subtractive.ron");
    let scope = 11;
    let _ = ui
        .app_mut()
        .send(Message::Canvas(CanvasEvent::VisibleDisplays(vec![scope])));
    let app = ui.app();
    let shown = crate::scene::scope_outputs(&app.session.controller, &[scope]);
    let widget = shown[0].widget;
    let probe = app
        .session
        .engine
        .live
        .scope_probe(shown[0].output)
        .expect("a probe while on screen");
    (ui, widget, probe)
}

/// The full trace a scope draws of `samples`, as the telemetry thread
/// builds it.
fn trace_of(samples: &[f32]) -> loom_canvas::Trace {
    let mut scope = ScopeTrace::new(48_000.0);
    scope.history.push(0, samples);
    scope.update(240, Trigger::Auto, Slope::Rising);
    scope.canvas_trace(true)
}

/// Sends `trace` for the scope's probe, as the telemetry thread does, and
/// returns the scope's accessibility description.
fn description(
    ui: &mut Ui,
    (module, widget): (u32, usize),
    probe: ProbeId,
    trace: loom_canvas::Trace,
) -> Option<String> {
    let _ = ui.app_mut().send(Message::Telemetry(Update {
        traces: vec![(probe, trace)],
        ..Update::default()
    }));
    let snapshot = ui.app().ui_snapshot();
    let id = ui
        .app()
        .a11y_widget_ids(&snapshot)
        .get(module, widget)
        .expect("the scope has a UI ID")
        .clone();
    ui.a11y_tree().node_by_id(&id).description().map(str::to_string)
}

/// A sine with a NaN in its window describes finite numbers. A window of
/// only NaN shows the invalid mark and describes it. An infinity reads as
/// the frame's edge, 1.00.
#[test]
fn a_scope_describes_only_finite_numbers() {
    let (mut ui, widget, probe) = with_scope();
    let sine: Vec<f32> = (0..4800)
        .map(|i| (i as f32 * std::f32::consts::TAU / 480.0).sin())
        .collect();

    let mut with_nan = sine.clone();
    with_nan[4400] = f32::NAN; // inside the window from the trigger at 4320
    assert_eq!(trace_of(&with_nan).points.len(), 241, "one of 242 samples left out");
    let words = description(&mut ui, widget, probe, trace_of(&with_nan)).expect("a description");
    assert!(!words.contains("inf") && !words.contains("NaN"), "{words}");
    assert!(words.starts_with("Lowest -"), "{words}");

    assert_eq!(
        description(&mut ui, widget, probe, trace_of(&[f32::NAN; 4800])).as_deref(),
        Some("Not a number")
    );
    let snapshot = ui.app().ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == widget.0).unwrap();
    assert!(tile.widgets[widget.1].trace.unwrap().invalid);

    let mut with_infinity = sine;
    with_infinity[4700] = f32::INFINITY;
    let words = description(&mut ui, widget, probe, trace_of(&with_infinity)).expect("a description");
    assert!(words.ends_with("highest 1.00"), "{words}");
}
