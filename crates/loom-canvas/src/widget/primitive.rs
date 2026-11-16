//! The window-wide output drag as an Iced shader primitive. Iced gives the
//! device, the queue, and a render pass whose viewport is the widget's
//! bounds; the plain renderer in `crate::gpu` does the rest.
//!
//! The patch canvas itself is no longer a primitive: the host draws it in
//! a pass of its own (`crate::slot`). The drag line stays one: it is in a
//! layer of the window above the canvas, and Iced draws its layers after
//! the host's canvas pass.

use crate::gpu::{Cable, CanvasRenderer};
use iced::widget::shader::{Pipeline, Primitive, Viewport};
use loom_ui::paint::Target;
use loom_ui_core::{Point, Size};

/// The window-wide output drag as an Iced shader primitive. Its renderer is
/// separate from the patch canvas's, so preparing two primitives cannot
/// overwrite either one.
#[derive(Debug, Clone, Copy)]
pub struct DragPrimitive {
    pub cable: Cable,
    pub zoom: f32,
}

/// A second canvas renderer, for the window-wide drag.
pub struct DragPipeline(CanvasRenderer);

impl Pipeline for DragPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self(CanvasRenderer::new(device, format))
    }
}

impl Primitive for DragPrimitive {
    type Pipeline = DragPipeline;

    fn prepare(
        &self,
        pipeline: &mut DragPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &iced::Rectangle,
        viewport: &Viewport,
    ) {
        let physical = viewport.physical_size();
        let target = Target {
            size: Size::new(bounds.width, bounds.height),
            scale: viewport.scale_factor(),
            origin: Point::new(bounds.x, bounds.y),
            physical: (physical.width, physical.height),
        };
        pipeline.0.prepare_cable(device, queue, self.cable, self.zoom, target);
    }

    fn draw(&self, pipeline: &DragPipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.0.draw_cable(pass);
        true
    }
}
