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
