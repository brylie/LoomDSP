//! Tests for what the canvas hands the renderer for one frame.

use super::*;
use crate::Camera;
use crate::scene::{HoverText, ModuleView, PanelView, WidgetView};

/// One module with one knob.
fn scene() -> Scene {
    Scene::new(
        vec![ModuleView {
            size: None,
            unknown: false,
            id: 1,
            title: "Osc".into(),
            hover: HoverText::default(),
            accent: Color::BLACK,
            position: Point::ORIGIN,
            inputs: Vec::new(),
            outputs: Vec::new(),
            panel: PanelView {
                width: 3,
                height: 3,
                widgets: vec![WidgetView {
                    kind: WidgetViewKind::Knob { steps: 0 },
                    x: 0,
                    y: 0,
                    label: "gain".into(),
                    hover: HoverText::default(),
                    hidden: false,
                    exposed: false,
                    inert: false,
                }],
            },
        }],
        Vec::new(),
    )
}

/// The canvas held still at `zoom`.
fn at_zoom(model: &CanvasModel<'_>, zoom: f32) -> CanvasInput {
    let still = Still {
        camera: Camera {
            origin: Point::new(3.0, 4.0),
            zoom,
        },
        ..Still::default()
    };
    CanvasInput::still(model, still, Instant::now())
}

/// A frame carries the camera and the cable opacity, and draws controls
/// close up and at the middle zoom but not far out.
#[test]
fn a_frame_carries_the_camera_and_the_detail_the_zoom_shows() {
    let scene = scene();
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let size = Size::new(800.0, 600.0);

    let close = at_zoom(&model, 1.0).frame(&model, size, &FrameCache::default());
    assert_eq!(close.camera.origin, Point::new(3.0, 4.0));
    assert_eq!(close.camera.zoom, 1.0);
    assert_eq!(close.cable_dim, CABLE_OPACITY);
    assert!(close.controls.is_some());
    assert!(close.traces.is_none(), "the model has no traces");
    assert!(close.bars.is_empty(), "text is drawn, so no bars stand in for it");

    let middle = at_zoom(&model, 0.7).frame(&model, size, &FrameCache::default());
    assert!(middle.controls.is_some());
    assert!(!middle.bars.is_empty(), "bars stand in for the labels");

    let far = at_zoom(&model, 0.2).frame(&model, size, &FrameCache::default());
    assert!(far.controls.is_none());
    assert!(far.bars.end > middle.bars.end, "a bar stands in for the title too");
}

/// With the same cache, a second frame reuses the scene's shapes and the
/// controls: nothing is rebuilt while the scene and values are unchanged
/// (ADR-0010). A pan changes only the camera.
#[test]
fn a_second_frame_reuses_what_the_cache_holds() {
    let scene = scene();
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let size = Size::new(800.0, 600.0);
    let cache = FrameCache::default();

    let first = at_zoom(&model, 1.0).frame(&model, size, &cache);
    let second = at_zoom(&model, 1.5).frame(&model, size, &cache);
    assert!(Arc::ptr_eq(&first.geometry, &second.geometry));
    assert!(Arc::ptr_eq(
        first.controls.as_ref().unwrap(),
        second.controls.as_ref().unwrap()
    ));
    assert_eq!(second.camera.zoom, 1.5);

    let other = FrameCache::default();
    let rebuilt = at_zoom(&model, 1.0).frame(&model, size, &other);
    assert!(
        !Arc::ptr_eq(&first.geometry, &rebuilt.geometry),
        "a new cache builds its own"
    );
}

/// Each kind of drawing data is rebuilt when what it was built from
/// changes, and only then: new control values rebuild the controls alone,
/// and a new scene rebuilds the shapes and the controls.
#[test]
fn new_values_rebuild_only_the_controls_and_a_new_scene_rebuilds_both() {
    let scene = scene();
    let none = BTreeSet::new();
    let size = Size::new(800.0, 600.0);
    let cache = FrameCache::default();
    let mut values = PanelValues::default();

    let first = {
        let model = CanvasModel::new(&scene, &none).values(&values);
        at_zoom(&model, 1.0).frame(&model, size, &cache)
    };
    values.set(
        1,
        0,
        crate::WidgetValue {
            position: 0.75,
            ..crate::WidgetValue::default()
        },
    );
    let turned = {
        let model = CanvasModel::new(&scene, &none).values(&values);
        at_zoom(&model, 1.0).frame(&model, size, &cache)
    };
    assert!(
        Arc::ptr_eq(&first.geometry, &turned.geometry),
        "a knob turn keeps the shapes"
    );
    assert!(!Arc::ptr_eq(
        first.controls.as_ref().unwrap(),
        turned.controls.as_ref().unwrap()
    ));

    let edited = self::scene();
    let model = CanvasModel::new(&edited, &none).values(&values);
    let rebuilt = at_zoom(&model, 1.0).frame(&model, size, &cache);
    assert!(!Arc::ptr_eq(&turned.geometry, &rebuilt.geometry));
    assert!(!Arc::ptr_eq(
        turned.controls.as_ref().unwrap(),
        rebuilt.controls.as_ref().unwrap()
    ));
}

/// Scope traces are rebuilt when a trace changes and when the zoom crosses
/// between the full trace and the flat line, kept otherwise, and not built
/// far out.
#[test]
fn traces_rebuild_on_a_new_trace_and_on_a_change_of_detail() {
    let scene = scene();
    let none = BTreeSet::new();
    let size = Size::new(800.0, 600.0);
    let cache = FrameCache::default();
    let mut traces = crate::ScopeTraces::new();
    let frame = |traces: &crate::ScopeTraces, zoom: f32| {
        let model = CanvasModel::new(&scene, &none).traces(traces);
        at_zoom(&model, zoom).frame(&model, size, &cache)
    };

    let close = frame(&traces, 1.0).traces.expect("drawn close up");
    let closer = frame(&traces, 1.5).traces.expect("drawn close up");
    assert!(
        Arc::ptr_eq(&close, &closer),
        "a zoom at the same detail keeps the traces"
    );

    let middle = frame(&traces, 0.7).traces.expect("a flat line at the middle zoom");
    assert!(!Arc::ptr_eq(&close, &middle), "the flat line is other data");
    assert!(frame(&traces, 0.2).traces.is_none(), "not drawn far out");

    traces.set(
        1,
        0,
        crate::Trace {
            points: vec![[0.0, 0.0], [1.0, 0.5]],
            second: Vec::new(),
            level: 0.5,
        },
    );
    let moved = frame(&traces, 0.7).traces.expect("still drawn");
    assert!(!Arc::ptr_eq(&middle, &moved), "a new trace rebuilds");
}
