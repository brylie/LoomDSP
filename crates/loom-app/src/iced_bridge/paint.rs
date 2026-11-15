//! One frame of the window, in three steps on one queue (plan for leaving
//! Iced, step 2.2, item 2):
//!
//! 1. clear, in the theme's background;
//! 2. the patch canvas, with [`CanvasRenderer`], in the same pass, with the
//!    viewport and scissor Iced gave its primitive until step 2.2c: the
//!    canvas's bounds times the scale, and that cut to the target;
//! 3. Iced's own layers, with no clear, over the same texture.
//!
//! The window, its captures, `--screenshot-window`, and the harness's
//! captures all draw through [`draw`], each into its own texture.
//!
//! *Why this order works:* Iced draws each layer's boxes before its shader
//! primitives and its text after them, and the canvas is in the window's
//! base layer, where nothing else draws over its bounds. So until phase 3
//! nothing in the base layer may draw over the canvas, and what shows over
//! it (menus, the browser, tooltips, the drag line) is in a later layer,
//! which Iced draws after the canvas pass (ui-layer.md, section 9).

use iced::Color;
use iced_wgpu::graphics::Viewport;
use iced_wgpu::wgpu;
use loom_canvas::{CanvasDrawing, CanvasRenderer, Target as CanvasTarget};
use loom_ui_core::Rect;

/// What a frame draws into: a texture and its GPU.
pub(crate) struct Target<'a> {
    /// The GPU device.
    pub device: &'a wgpu::Device,
    /// Its queue.
    pub queue: &'a wgpu::Queue,
    /// The texture to draw into.
    pub view: &'a wgpu::TextureView,
    /// The texture's format, which the renderers were built for.
    pub format: wgpu::TextureFormat,
    /// The texture's size in physical pixels.
    pub physical: iced::Size<u32>,
    /// Physical pixels per logical pixel.
    pub scale: f32,
}

/// The canvas's renderer for one target format, and the frame it last
/// drew.
pub(crate) struct CanvasPass {
    /// The renderer, with what it last uploaded.
    renderer: CanvasRenderer,
    /// The frame last drawn, and the target it was prepared for, or `None`
    /// when the last frame had no canvas.
    last: Option<Prepared>,
}

/// A canvas frame kept after it was drawn, so a capture can draw it again.
struct Prepared {
    /// The frame, moved here from the slot: kept, not copied.
    drawing: CanvasDrawing,
    /// The target the renderer last prepared it for.
    target: CanvasTarget,
}

impl CanvasPass {
    /// A renderer for targets of `format`.
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            renderer: CanvasRenderer::new(device, format),
            last: None,
        }
    }
}

/// Draws one frame into `target`: the clear, the canvas from `drawing`,
/// then Iced's layers. The caller has run Iced's draw call on `iced`
/// already, which built the canvas's frame; `None` when the view drew no
/// canvas.
pub(crate) fn draw(
    target: &Target<'_>,
    iced: &mut iced_wgpu::Renderer,
    pass: &mut CanvasPass,
    drawing: Option<CanvasDrawing>,
    background: Color,
) {
    pass.last = drawing.map(|drawing| {
        let to = canvas_target(drawing.bounds, target);
        pass.renderer.prepare(target.device, target.queue, &drawing.frame, to);
        Prepared { drawing, target: to }
    });
    draw_prepared(target, iced, pass, background);
}

/// Draws the last frame again into `target`: the canvas's last frame, at
/// the bounds it last had, and what Iced's renderer still holds. A capture
/// of what the window last showed.
///
/// The canvas uploads nothing when the target is the one it was prepared
/// for. A capture can come after a resize, before any frame at the new
/// size, so when the target differs the frame is prepared again for it:
/// the canvas's text cuts its scissors to the target as it is prepared, and
/// wgpu rejects a scissor larger than the target it draws into.
pub(crate) fn draw_again(
    target: &Target<'_>,
    iced: &mut iced_wgpu::Renderer,
    pass: &mut CanvasPass,
    background: Color,
) {
    if let Some(last) = pass.last.as_mut() {
        let to = canvas_target(last.drawing.bounds, target);
        if to != last.target {
            pass.renderer
                .prepare(target.device, target.queue, &last.drawing.frame, to);
            last.target = to;
        }
    }
    draw_prepared(target, iced, pass, background);
}

/// The three steps, with the canvas as `pass` holds it.
fn draw_prepared(target: &Target<'_>, iced: &mut iced_wgpu::Renderer, pass: &CanvasPass, background: Color) {
    let bounds = pass.last.as_ref().map(|last| last.drawing.bounds);
    let mut encoder = target.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("loom-app canvas"),
    });
    {
        // The clear colour in raw components, as Iced's own clear was.
        let [r, g, b, a] = iced_wgpu::graphics::color::pack(background).components();
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("loom-app canvas pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(r),
                        g: f64::from(g),
                        b: f64::from(b),
                        a: f64::from(a),
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if let Some(bounds) = bounds
            && let Some(scissor) = scissor(bounds, target.physical, target.scale)
        {
            let s = target.scale;
            render.set_viewport(
                bounds.x * s,
                bounds.y * s,
                bounds.width * s,
                bounds.height * s,
                0.0,
                1.0,
            );
            render.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
            pass.renderer.draw(&mut render);
        }
    }
    target.queue.submit([encoder.finish()]);
    let viewport = Viewport::with_physical_size(target.physical, target.scale);
    iced.present(None, target.format, target.view, &viewport);
}

/// Where the canvas renderer draws, for a canvas at `bounds`.
fn canvas_target(bounds: Rect, target: &Target<'_>) -> CanvasTarget {
    CanvasTarget {
        size: bounds.size(),
        scale: target.scale,
        origin: loom_ui_core::Point::new(bounds.x, bounds.y),
        physical: (target.physical.width, target.physical.height),
    }
}

/// The canvas's scissor: its bounds times the scale `s`, cut to the target and
/// snapped to whole pixels, as Iced cut its primitive's. `None` when
/// nothing of it is in the target.
fn scissor(bounds: Rect, physical: iced::Size<u32>, s: f32) -> Option<iced::Rectangle<u32>> {
    let window = iced::Rectangle::<f32>::with_size(iced::Size::new(physical.width as f32, physical.height as f32));
    let canvas = iced::Rectangle::new(
        iced::Point::new(bounds.x * s, bounds.y * s),
        iced::Size::new(bounds.width * s, bounds.height * s),
    );
    canvas.intersection(&window)?.snap()
}

#[cfg(test)]
mod tests;
