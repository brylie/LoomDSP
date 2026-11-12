//! Building what the renderer draws for one frame, from the canvas's state
//! and the app's view model.

use super::*;
use crate::controls::{ControlGeometry, PanelValues};
use crate::gpu::{CanvasFrame, Geometry};
use crate::trace::TraceGeometry;
use std::sync::Arc;

/// The drawing data last built from the scene, the control values, and the
/// scope traces. Each is rebuilt only when what it was built from changes,
/// not every frame (ADR-0010). Whoever hosts the canvas keeps one beside
/// its [`CanvasInput`].
#[derive(Default)]
pub struct FrameCache {
    geometry: RefCell<Option<Arc<Geometry>>>,
    controls: RefCell<Option<Arc<ControlGeometry>>>,
    traces: RefCell<Option<Arc<TraceGeometry>>>,
    lines: LinesCache,
    /// The canvas's text and its layouts. Borrowed only inside each call
    /// that sets text (`crate::text::ListPainter`).
    text: RefCell<crate::text::CanvasText>,
}

impl CanvasInput {
    /// What the renderer draws this frame, for a canvas of `size` logical
    /// pixels: the scene's shapes, the controls and traces the zoom shows,
    /// the overlay, and the camera. Give it to
    /// [`CanvasRenderer::prepare`](crate::CanvasRenderer::prepare).
    pub fn frame(&self, model: &CanvasModel<'_>, size: Size, cache: &FrameCache) -> CanvasFrame {
        let camera = self.camera;

        let geometry = {
            let mut cache = cache.geometry.borrow_mut();
            match cache.as_ref() {
                Some(g) if g.generation == model.scene.generation() => g.clone(),
                _ => {
                    let built = Arc::new(Geometry::build(model.scene, &model.palette));
                    *cache = Some(built.clone());
                    built
                }
            }
        };

        // Controls follow the titles: drawn as shapes once titles show, and
        // not at all when zoomed further out (panels.md, Level of detail).
        let empty = PanelValues::default();
        let values = model.values.unwrap_or(&empty);
        let controls = (detail(camera.zoom) != Detail::Far).then(|| {
            let key = (model.scene.generation(), values.generation());
            let mut cache = cache.controls.borrow_mut();
            match cache.as_ref() {
                Some(c) if c.key == key => c.clone(),
                _ => {
                    let built = Arc::new(ControlGeometry::build(model.scene, values, &model.palette));
                    *cache = Some(built.clone());
                    built
                }
            }
        });

        // Scope traces: full close up, a flat line at the middle zoom, and
        // not drawn far out (panels.md, Drawing the scope trace).
        let traces = model
            .traces
            .filter(|_| detail(camera.zoom) != Detail::Far)
            .map(|traces| {
                let full = detail(camera.zoom) == Detail::Close;
                let key = (model.scene.generation(), traces.generation(), full);
                let mut cache = cache.traces.borrow_mut();
                match cache.as_ref() {
                    Some(t) if t.key == key => t.clone(),
                    _ => {
                        let built = Arc::new(TraceGeometry::build(model.scene, traces, full, &model.palette));
                        *cache = Some(built.clone());
                        built
                    }
                }
            });

        // Text too small to draw leaves a bar in its place.
        let bars = match detail(camera.zoom) {
            Detail::Close => geometry.bars(false, false),
            Detail::Middle => geometry.bars(true, false),
            Detail::Far => geometry.bars(true, true),
        };
        // A box's text bar draws over the box, and only at middle zoom.
        let value_bars = geometry.value_bars(detail(camera.zoom) == Detail::Middle);

        CanvasFrame {
            geometry,
            controls,
            traces,
            overlay: model.overlay_in(self, &cache.lines, camera.visible(size)),
            bars,
            value_bars,
            camera,
            cable_dim: CABLE_OPACITY,
            paint: crate::text::PaintList::default(),
        }
    }

    /// Paints the frame's text and floating boxes (see [`CanvasInput::paint`])
    /// with the canvas's own text, for a canvas at `bounds` in the host's
    /// logical pixels. A layout no frame used since the last is dropped.
    pub fn paint_list(&self, model: &CanvasModel<'_>, bounds: Rect, cache: &FrameCache) -> crate::text::PaintList {
        cache.text.borrow_mut().begin_frame();
        let mut list = crate::text::PaintList::new(bounds);
        self.paint(
            model,
            bounds,
            &mut crate::text::ListPainter::new(&cache.text, &mut list),
        );
        list
    }
}

#[cfg(test)]
mod tests;
