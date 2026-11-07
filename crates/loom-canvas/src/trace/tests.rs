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
                exposed: false,
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
        second: Vec::new(),
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
            second: Vec::new(),
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

/// A scene with one tile holding only a level history.
fn history_scene() -> Scene {
    let panel = PanelView {
        width: 9,
        height: 6,
        widgets: vec![WidgetView {
            kind: WidgetViewKind::LevelHistory,
            x: 0,
            y: 0,
            label: String::new(),
            hover: Default::default(),
            hidden: false,
            exposed: false,
        }],
    };
    let mut scene = scope_scene();
    let mut module = scene.modules()[0].clone();
    module.panel = panel;
    scene = Scene::new(vec![module], Vec::new());
    scene
}

/// A level history's RMS is flat columns in their own buffer, with its peak
/// a line drawn only close up (panel-widgets.md, Level history).
#[test]
fn a_level_history_fills_its_columns() {
    let scene = history_scene();
    let mut traces = ScopeTraces::new();
    traces.set(
        7,
        0,
        Trace {
            points: vec![[0.0, -1.0], [0.5, 0.0], [0.99, 1.0]],
            second: vec![[0.0, 0.5], [0.5, 0.5], [0.99, 1.0]],
            level: 0.0,
        },
    );
    let palette = Palette::default();
    let close = TraceGeometry::build(&scene, &traces, true, &palette);
    assert_eq!(close.fills.len(), 2, "a column at the floor draws nothing");
    assert_eq!(close.segments.len(), 2, "the peak line");
    let middle = TraceGeometry::build(&scene, &traces, false, &palette);
    assert_eq!(middle.fills.len(), 2, "the RMS shape stays");
    assert!(middle.segments.is_empty(), "no peak at the middle zoom");
}

/// A spectrum draws its line at both zooms, and its grid and hold only close up
/// (panel-widgets.md, Spectrum).
#[test]
fn a_spectrum_has_a_grid_only_close_up() {
    let mut scene_module = history_scene().modules()[0].clone();
    scene_module.panel.widgets[0].kind = WidgetViewKind::Spectrum;
    let scene = Scene::new(vec![scene_module], Vec::new());
    let mut traces = ScopeTraces::new();
    traces.set(
        7,
        0,
        Trace {
            points: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]],
            second: vec![[0.0, 0.5], [1.0, 0.5]],
            level: 0.0,
        },
    );
    let palette = Palette::default();
    let close = TraceGeometry::build(&scene, &traces, true, &palette);
    assert_eq!(close.segments.len(), 3 + 1 + 2, "grid, hold, and line");
    let middle = TraceGeometry::build(&scene, &traces, false, &palette);
    assert_eq!(middle.segments.len(), 2, "the line only");
}

/// An XY scope fades its older points, and at the middle zoom the one point the
/// app sends is drawn as a dot (panel-widgets.md, XY scope).
#[test]
fn an_xy_scope_fades_and_has_a_dot_at_the_middle_zoom() {
    let mut scene_module = history_scene().modules()[0].clone();
    scene_module.panel.widgets[0].kind = WidgetViewKind::XyScope;
    let scene = Scene::new(vec![scene_module], Vec::new());
    let palette = Palette::default();
    let mut traces = ScopeTraces::new();
    traces.set(
        7,
        0,
        Trace {
            points: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 0.0], [0.5, -0.5]],
            ..Trace::default()
        },
    );
    let close = TraceGeometry::build(&scene, &traces, true, &palette);
    assert_eq!(close.segments.len(), 3);
    assert_ne!(close.segments[0], close.segments[2]);
    traces.set(
        7,
        0,
        Trace {
            points: vec![[0.5, 0.5]],
            ..Trace::default()
        },
    );
    let middle = TraceGeometry::build(&scene, &traces, false, &palette);
    assert_eq!(middle.segments.len(), 1);
    let (from, to) = middle.segments[0].ends();
    assert_eq!(from, to, "a dot");
}

/// A grown XY scope stays square and centred, so a circle stays a circle, and
/// its trace is drawn inside that square (panel-widgets.md, XY scope).
#[test]
fn an_xy_scope_stays_square_when_it_grows() {
    let wide = xy_scope_area(Rectangle::new(Point::new(10.0, 20.0), iced::Size::new(400.0, 100.0)));
    assert_eq!(wide.width, wide.height);
    assert_eq!(wide.center(), Point::new(210.0, 70.0));
    assert_eq!(
        wide.height,
        trace_area(Rectangle::new(Point::new(10.0, 20.0), iced::Size::new(400.0, 100.0))).height
    );
    let tall = xy_scope_area(Rectangle::new(Point::ORIGIN, iced::Size::new(100.0, 400.0)));
    assert_eq!(tall.width, tall.height);
    assert_eq!(tall.center(), Point::new(50.0, 200.0));

    let mut module = history_scene().modules()[0].clone();
    module.panel.widgets[0].kind = WidgetViewKind::XyScope;
    module.size = Some((60, 20));
    let rect = module.widget_bounds(0).unwrap();
    assert!(rect.width > rect.height, "the widget itself is wide");
    let area = xy_scope_area(rect);
    let scene = Scene::new(vec![module], Vec::new());
    let mut traces = ScopeTraces::new();
    traces.set(
        7,
        0,
        Trace {
            points: vec![[0.0, -1.0], [1.0, 1.0]],
            ..Trace::default()
        },
    );
    let drawn = TraceGeometry::build(&scene, &traces, true, &Palette::default());
    let (from, to) = drawn.segments[0].ends();
    assert_eq!(from, Point::new(area.x, area.y + area.height));
    assert_eq!(to, Point::new(area.x + area.width, area.y));
}
