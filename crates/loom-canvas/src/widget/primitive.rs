//! The canvas renderer as Iced shader primitives. Iced gives the device,
//! the queue, and a render pass whose viewport is the widget's bounds; the
//! plain renderer in `crate::gpu` does the rest.

use crate::gpu::{Cable, CanvasFrame, CanvasRenderer, Target};
use iced::widget::shader::{Pipeline, Primitive, Viewport};
use loom_ui_core::Size;

/// The canvas frame as an Iced shader primitive.
#[derive(Debug)]
pub struct CanvasPrimitive(pub(crate) CanvasFrame);

/// The canvas renderer as Iced's shared state for [`CanvasPrimitive`].
///
/// Note: Iced keeps one of these for the primitive type, so the app shows
/// one canvas at a time.
pub struct CanvasPipeline(CanvasRenderer);

impl Pipeline for CanvasPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self(CanvasRenderer::new(device, format))
    }
}

impl Primitive for CanvasPrimitive {
    type Pipeline = CanvasPipeline;

    fn prepare(
        &self,
        pipeline: &mut CanvasPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &iced::Rectangle,
        viewport: &Viewport,
    ) {
        let target = Target {
            size: Size::new(bounds.width, bounds.height),
            scale: viewport.scale_factor(),
        };
        pipeline.0.prepare(device, queue, &self.0, target);
    }

    fn draw(&self, pipeline: &CanvasPipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.0.draw(pass);
        true
    }
}

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
        let target = Target {
            size: Size::new(bounds.width, bounds.height),
            scale: viewport.scale_factor(),
        };
        pipeline.0.prepare_cable(device, queue, self.cable, self.zoom, target);
    }

    fn draw(&self, pipeline: &DragPipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.0.draw_cable(pass);
        true
    }
}
