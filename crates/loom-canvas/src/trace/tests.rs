use super::*;
use crate::scene::{ModuleView, PanelView, WidgetView};

fn scope_scene() -> Scene {
    let module = ModuleView {
        size: None,
        unknown: false,
        id: 7,
        title: "Scope".into(),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position: Point::ORIGIN,
        inputs: vec![],
        outputs: vec![],
        panel: PanelView {
            width: 9,
            height: 5,
            widgets: vec![WidgetView {
                kind: WidgetViewKind::Scope,
                x: 0,
                y: 0,
                label: String::new(),
                hover: crate::scene::HoverText::default(),
                hidden: false,
            }],
        },
    };
    Scene::new(vec![module], vec![])
}

/// Setting the same trace again keeps the generation, so nothing
/// redraws. A new trace, or one taken away, changes it.
#[test]
fn the_generation_changes_only_with_the_traces() {
    let mut traces = ScopeTraces::new();
    let trace = Trace {
        points: vec![[0.0, 0.0], [1.0, 0.5]],
        level: 0.5,
    };
    traces.set(7, 0, trace.clone());
    let generation = traces.generation();
    traces.set(7, 0, trace.clone());
    assert_eq!(traces.generation(), generation);
    traces.retain(|_, _| true);
    assert_eq!(traces.generation(), generation);
    traces.retain(|_, _| false);
    assert_ne!(traces.generation(), generation);
    assert!(traces.get(7, 0).is_none());
}

/// Close up, the points become pieces in the frame, top at 1 and
/// bottom at -1. At the middle zoom, one flat line at the centre.
#[test]
fn traces_fill_the_frame_close_up_and_flatten_further_out() {
    let scene = scope_scene();
    let module = &scene.modules()[0];
    let area = trace_area(module.widget_bounds(0).unwrap());
    let mut traces = ScopeTraces::new();
    traces.set(
        7,
        0,
        Trace {
            points: vec![[0.0, -1.0], [0.5, 1.0], [1.0, 3.0]],
            level: 0.25,
        },
    );
    let palette = Palette::default();
    let close = TraceGeometry::build(&scene, &traces, true, &palette);
    assert_eq!(close.segments.len(), 2);
    let (a, b) = close.segments[0].ends();
    assert_eq!(a, Point::new(area.x, area.y + area.height));
    assert_eq!(b, Point::new(area.center_x(), area.y));
    // Clamped to the top.
    assert_eq!(close.segments[1].ends().1, Point::new(area.x + area.width, area.y));
    assert_eq!(close.key, (scene.generation(), traces.generation(), true));

    let middle = TraceGeometry::build(&scene, &traces, false, &palette);
    assert_eq!(middle.segments.len(), 1);
    let (a, b) = middle.segments[0].ends();
    assert_eq!((a.y, b.y), (area.center_y(), area.center_y()));
    assert_eq!((a.x, b.x), (area.x, area.x + area.width));

    // A silent scope draws no flat line.
    traces.set(7, 0, Trace::default());
    assert!(
        TraceGeometry::build(&scene, &traces, false, &palette)
            .segments
            .is_empty()
    );
}
