//! One frame of the window, in five steps on one queue (plan for leaving
//! Iced, step 2.2, item 2, and step 3.1, item 2):
//!
//! 1. clear, in the theme's background;
//! 2. the patch canvas, with [`CanvasRenderer`], in the same pass, with the
//!    viewport and scissor Iced gave its primitive until step 2.2c: the
//!    canvas's bounds times the scale, and that cut to the target;
//! 3. the runtime's own panels below Iced (the canvas and panels layers),
//!    with one [`PaintRenderer`], with no clear;
//! 4. Iced's own layers, with no clear, over the same texture;
//! 5. the runtime's own panels above Iced (the overlays and question
//!    layers), with a second [`PaintRenderer`], with no clear.
//!
//! Each renderer keeps the one list it last prepared, and its own glyph
//! atlas, which is why there are two. A side with nothing painted
//! prepares nothing and starts no pass, so a window with no panels of the
//! runtime's draws what it drew before (ui-layer.md, section 5, rule 10).
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
use loom_canvas::{CanvasDrawing, CanvasRenderer};
use loom_ui::paint::{PaintList, PaintRenderer, Target as CanvasTarget};
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

/// The runtime's own panels in the frame: one renderer for those below
/// Iced and one for those above it, each with what it last prepared.
pub(crate) struct PanelPass {
    /// The canvas and panels layers, drawn before Iced.
    below: Side,
    /// The overlays and question layers, drawn after Iced.
    above: Side,
}

/// One side of Iced: its renderer, and the list it last prepared and for
/// which target, or `None` when nothing was painted there.
struct Side {
    renderer: PaintRenderer,
    last: Option<(PaintList, CanvasTarget)>,
}

impl Side {
    /// A side with nothing prepared.
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            renderer: PaintRenderer::new(device, format),
            last: None,
        }
    }

    /// Takes this frame's list and prepares it, unless it is empty, or the
    /// same as the last one for the same target: the renderer then holds
    /// it already, so an unchanged panel uploads nothing (the canvas keeps
    /// its GPU data the same way).
    fn set(&mut self, target: &Target<'_>, list: PaintList) {
        if list.is_empty() {
            self.last = None;
            return;
        }
        let to = whole(target);
        if let Some((last, at)) = &self.last
            && *at == to
            && last.same_as(&list)
        {
            return;
        }
        let _ = self.renderer.prepare(target.device, target.queue, &list, to);
        self.last = Some((list, to));
    }

    /// Prepares the last list again when `target` differs from the one it
    /// was prepared for, as the canvas does for a capture after a resize.
    fn again(&mut self, target: &Target<'_>) {
        let to = whole(target);
        if let Some((list, last)) = self.last.as_mut()
            && *last != to
        {
            let _ = self.renderer.prepare(target.device, target.queue, list, to);
            *last = to;
        }
    }

    /// Draws what was prepared over `target`, with no clear. Nothing, and
    /// no pass, when nothing was painted.
    fn draw(&self, target: &Target<'_>, encoder: &mut wgpu::CommandEncoder) {
        if self.last.is_none() {
            return;
        }
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("loom-app panels pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        self.renderer.draw(&mut render);
    }
}

impl PanelPass {
    /// Renderers for targets of `format`.
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            below: Side::new(device, format),
            above: Side::new(device, format),
        }
    }

    /// Takes this frame's two lists, painted from the runtime's panels.
    pub(crate) fn set(&mut self, target: &Target<'_>, below: PaintList, above: PaintList) {
        self.below.set(target, below);
        self.above.set(target, above);
    }
}

/// How the runtime's panels look from `theme`, for the window and its
/// still captures alike: text in the theme's text colour, and a field
/// looking like one of Iced's while both draw, its caret in the text
/// colour and its selection in the theme's weak primary, quiet text such
/// as a radio's note in the app's quiet colour, a radio's mark in the text
/// colour, a check box from the app's one check box look
/// (`style::check_look`), a slider's thumb and fill in the theme's primary, and a slider's track in its strong
/// background, as Iced's slider rail is.
pub(crate) fn panel_style(theme: &iced::Theme) -> loom_ui::paint::PanelStyle {
    use loom_canvas::iced_convert::ToCore as _;
    loom_ui::paint::PanelStyle {
        text: theme.palette().text.to_core(),
        caret: theme.palette().text.to_core(),
        selection: theme.extended_palette().primary.weak.color.to_core(),
        ring: crate::style::focus_ring(),
        quiet: crate::style::QUIET.to_core(),
        mark: theme.palette().text.to_core(),
        primary: theme.palette().primary.to_core(),
        track: theme.extended_palette().background.strong.color.to_core(),
        check: crate::style::check_look(),
    }
}

/// The runtime's layers painted into two lists over `whole` by `paint`: the
/// canvas and panels layers, drawn below Iced, then the overlays and
/// question layers, drawn above it (ui-layer.md, section 5, rule 10). The
/// window and its still captures both split the layers here.
pub(crate) fn paint_sides(
    whole: Rect,
    mut paint: impl FnMut(loom_ui::Layer, &mut PaintList),
) -> (PaintList, PaintList) {
    use loom_ui::Layer;
    let mut lists = (PaintList::new(whole), PaintList::new(whole));
    for layer in [Layer::Canvas, Layer::Panels] {
        paint(layer, &mut lists.0);
    }
    for layer in [Layer::Overlays, Layer::Question] {
        paint(layer, &mut lists.1);
    }
    lists
}

/// The whole of `target` for the panels' renderers: they draw in window
/// coordinates, from its corner.
fn whole(target: &Target<'_>) -> CanvasTarget {
    CanvasTarget {
        size: iced_to_size(target),
        scale: target.scale,
        origin: loom_ui_core::Point::ORIGIN,
        physical: (target.physical.width, target.physical.height),
    }
}

/// `target`'s size in logical pixels.
fn iced_to_size(target: &Target<'_>) -> loom_ui_core::Size {
    loom_ui_core::Size::new(
        target.physical.width as f32 / target.scale,
        target.physical.height as f32 / target.scale,
    )
}

/// Draws one frame into `target`: the clear, the canvas from `drawing`,
/// the runtime's panels below Iced, Iced's layers, then the runtime's
/// panels above Iced. The caller has run Iced's draw call on `iced`
/// already, which built the canvas's frame; `None` when the view drew no
/// canvas. `panels` holds this frame's lists, set already; `None` where
/// there is no runtime to draw from, such as an offscreen capture.
pub(crate) fn draw(
    target: &Target<'_>,
    iced: &mut iced_wgpu::Renderer,
    pass: &mut CanvasPass,
    drawing: Option<CanvasDrawing>,
    background: Color,
    panels: Option<&PanelPass>,
) {
    pass.last = drawing.map(|drawing| {
        let to = canvas_target(drawing.bounds, target);
        pass.renderer.prepare(target.device, target.queue, &drawing.frame, to);
        Prepared { drawing, target: to }
    });
    draw_prepared(target, iced, pass, background, panels);
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
    mut panels: Option<&mut PanelPass>,
) {
    if let Some(panels) = panels.as_deref_mut() {
        panels.below.again(target);
        panels.above.again(target);
    }
    if let Some(last) = pass.last.as_mut() {
        let to = canvas_target(last.drawing.bounds, target);
        if to != last.target {
            pass.renderer
                .prepare(target.device, target.queue, &last.drawing.frame, to);
            last.target = to;
        }
    }
    draw_prepared(target, iced, pass, background, panels.as_deref());
}

/// The five steps, with the canvas as `pass` holds it and the panels as
/// `panels` holds them.
fn draw_prepared(
    target: &Target<'_>,
    iced: &mut iced_wgpu::Renderer,
    pass: &CanvasPass,
    background: Color,
    panels: Option<&PanelPass>,
) {
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
    if let Some(panels) = panels {
        panels.below.draw(target, &mut encoder);
    }
    target.queue.submit([encoder.finish()]);
    let viewport = Viewport::with_physical_size(target.physical, target.scale);
    iced.present(None, target.format, target.view, &viewport);
    if let Some(panels) = panels.filter(|panels| panels.above.last.is_some()) {
        let mut encoder = target.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("loom-app panels above"),
        });
        panels.above.draw(target, &mut encoder);
        target.queue.submit([encoder.finish()]);
    }
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
