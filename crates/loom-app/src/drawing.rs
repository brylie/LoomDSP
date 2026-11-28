//! One frame of the window, drawn by the app's own renderers (ui-layer.md,
//! section 9, rule 8, "the order with no Iced"; plan for leaving Iced,
//! step P4).
//!
//! The window, its captures, the `Ui` harness's captures, and
//! `--screenshot-window` all draw through [`WindowPainter::draw`], each
//! into its own texture, so a capture is the frame the window shows. A
//! frame, in one render pass:
//!
//! 1. clears to the window's background (`style::PANEL_FILL`);
//! 2. draws the Canvas and Panels layers in paint order, with the canvas,
//!    a custom element, cut into its place by the frame plan, and the
//!    cable dragged from a port drawn above the Panels layer;
//! 3. draws the Overlays and Question layers over them.
//!
//! Each renderer keeps the list it last prepared and its own glyph atlas,
//! which is why there are two.

use crate::App;
use loom_ui::GpuFrame;
use loom_ui::Panels;
use loom_ui::paint::{ListPainter, PaintList, PaintRenderer, PanelStyle, Target, Typesetter};
use loom_ui_core::{Point, Rect, Size};
use std::cell::RefCell;

/// What a frame draws into: a texture and its GPU.
pub(crate) struct FrameTarget<'a> {
    /// The GPU device.
    pub(crate) device: &'a wgpu::Device,
    /// Its queue.
    pub(crate) queue: &'a wgpu::Queue,
    /// The texture to draw into.
    pub(crate) view: &'a wgpu::TextureView,
    /// The texture's format, which the renderers are built for.
    pub(crate) format: wgpu::TextureFormat,
    /// The window's inside, in logical pixels.
    pub(crate) size: Size,
    /// Physical pixels per logical pixel.
    pub(crate) scale: f32,
    /// True for a capture rather than a frame shown.
    pub(crate) capture: bool,
}

/// The renderers for the window's frames, for one texture format.
pub(crate) struct WindowPainter {
    /// The format the renderers were built for.
    format: wgpu::TextureFormat,
    /// The Canvas and Panels layers.
    below: PaintRenderer,
    /// The Overlays and Question layers.
    above: PaintRenderer,
}

impl WindowPainter {
    /// Renderers for targets of `format`.
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            below: PaintRenderer::new(device, format),
            above: PaintRenderer::new(device, format),
        }
    }

    /// The format these renderers draw.
    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Lays out `panels` with `text`, then draws one whole frame of them
    /// into `target`.
    pub(crate) fn draw(&mut self, panels: &mut Panels<'_, App>, text: &RefCell<Typesetter>, target: &FrameTarget<'_>) {
        text.borrow_mut().begin_frame();
        panels.lay_out(&mut *text.borrow_mut());
        let whole = Rect::new(Point::ORIGIN, target.size);
        let style = panel_style();
        let mut below = PaintList::new(whole);
        for layer in [loom_ui::Layer::Canvas, loom_ui::Layer::Panels] {
            panels.paint(layer, &mut ListPainter::new(text, &mut below), &style);
        }
        let plan = panels.frame_plan(&below);
        let mut above = PaintList::new(whole);
        for layer in [loom_ui::Layer::Overlays, loom_ui::Layer::Question] {
            panels.paint(layer, &mut ListPainter::new(text, &mut above), &style);
        }
        let paint_target = Target {
            size: target.size,
            scale: target.scale,
            origin: Point::ORIGIN,
            physical: (
                (target.size.width * target.scale).round() as u32,
                (target.size.height * target.scale).round() as u32,
            ),
        };
        let gpu = GpuFrame {
            device: target.device,
            queue: target.queue,
            format: target.format,
            // One device for the window's life, and one for the harness's.
            generation: 0,
            target: paint_target,
            capture: target.capture,
        };
        let _ = self.below.prepare(target.device, target.queue, &below, paint_target);
        let _ = self.above.prepare(target.device, target.queue, &above, paint_target);
        panels.prepare_gpu(&plan, &gpu);
        let mut encoder = target.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("loom-app frame"),
        });
        {
            let [r, g, b, a] = [
                crate::style::PANEL_FILL.r,
                crate::style::PANEL_FILL.g,
                crate::style::PANEL_FILL.b,
                crate::style::PANEL_FILL.a,
            ];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("loom-app frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // The background in raw components, as Iced's clear
                        // was: the colours are the bytes to show.
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
            panels.draw_frame(&plan, &self.below, &mut pass, &gpu);
            self.above.draw(&mut pass);
        }
        target.queue.submit([encoder.finish()]);
    }
}

/// How the runtime's panels look: text in the text colour, and a field with
/// its caret in the text colour and its selection in `style::SELECTION`,
/// quiet text in the app's quiet colour, a radio's mark in the text colour,
/// a check box from the app's one check box look (`style::check_look`), a
/// slider's thumb and fill in the accent, and a slider's track in the edge
/// colour (`style.rs`).
pub(crate) fn panel_style() -> PanelStyle {
    PanelStyle {
        text: crate::style::TEXT,
        caret: crate::style::TEXT,
        selection: crate::style::SELECTION,
        ring: crate::style::focus_ring(),
        quiet: crate::style::QUIET,
        mark: crate::style::TEXT,
        primary: crate::style::ACCENT,
        track: crate::style::EDGE,
        check: crate::style::check_look(),
    }
}
