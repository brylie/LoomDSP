//! Drawing the app's views offscreen, with wgpu: for `--screenshot-window`,
//! `--screenshot`, and the headless harness's captures.
//!
//! The app's renderer type is Iced's fallback over wgpu and
//! [`Measure`](super::Measure). Only the wgpu half draws, so an offscreen
//! renderer is always that half. The canvas is drawn beside it, in the
//! app's own pass (`paint.rs`), so both need the same GPU device, and this
//! module makes it.

use super::paint::{self, CanvasPass};
use iced::{Color, Font, Pixels, Size};
use iced_wgpu::graphics::Shell;
use iced_wgpu::wgpu;
use loom_canvas::CanvasDrawing;

/// The texture format offscreen frames are drawn in: 8-bit RGBA, read back
/// as the PNG's bytes with no reordering. Not sRGB, because the app's
/// colours are web colours, given as the bytes to show (the workspace turns
/// on Iced's `web-colors`), as the window's surface format is chosen too. So
/// a capture has the window's colours.
const FORMAT: wgpu::TextureFormat = if iced_wgpu::graphics::color::GAMMA_CORRECTION {
    wgpu::TextureFormat::Rgba8UnormSrgb
} else {
    wgpu::TextureFormat::Rgba8Unorm
};

/// A GPU device with Iced's renderer and the canvas's, for drawing frames
/// offscreen.
pub(crate) struct Offscreen {
    /// The device.
    device: wgpu::Device,
    /// Its queue.
    queue: wgpu::Queue,
    /// Iced's renderer, always the wgpu half. Views are built and drawn on
    /// it before [`pixels`](Self::pixels) is called.
    pub(crate) renderer: crate::Renderer,
    /// The canvas's renderer.
    canvas: CanvasPass,
    /// The renderers for `loom-ui` panels below and above Iced.
    panels: paint::PanelPass,
    /// The last frame drew panels, so drawing it again draws them too.
    drew_panels: bool,
}

impl Offscreen {
    /// A device and both renderers, or `None` when no GPU adapter is found.
    /// Iced's renderer has the window's defaults: its default font at 16,
    /// and no antialiasing, since the app draws no meshes.
    pub(crate) fn new() -> Option<Self> {
        let (adapter, device, queue) = iced::futures::executor::block_on(gpu())?;
        let engine = iced_wgpu::Engine::new(
            &adapter,
            device.clone(),
            queue.clone(),
            FORMAT,
            None,
            // Offscreen frames load no images, so nothing redraws later.
            Shell::headless(),
        );
        Some(Self {
            canvas: CanvasPass::new(&device, FORMAT),
            panels: paint::PanelPass::new(&device, FORMAT),
            drew_panels: false,
            renderer: crate::Renderer::Primary(iced_wgpu::Renderer::new(engine, Font::DEFAULT, Pixels(16.0))),
            device,
            queue,
        })
    }

    /// Draws a frame `size` physical pixels at `scale` on `background`:
    /// the canvas from `drawing`, then what Iced's renderer was given. Gives
    /// its RGBA pixels, row by row, or `None` when they could not be read
    /// back from the GPU.
    ///
    /// # Panics
    ///
    /// If [`renderer`](Self::renderer) was replaced by the measure
    /// renderer, which draws nothing.
    pub(crate) fn pixels(
        &mut self,
        size: Size<u32>,
        scale: f32,
        background: Color,
        drawing: Option<CanvasDrawing>,
    ) -> Option<Vec<u8>> {
        self.pixels_with(size, scale, background, drawing, None)
    }

    /// [`pixels`](Self::pixels), with `loom-ui` panels painted into two
    /// lists: those below Iced, and those above it. `None` draws none, as
    /// the app's views have no panel of the runtime's yet.
    pub(crate) fn pixels_with(
        &mut self,
        size: Size<u32>,
        scale: f32,
        background: Color,
        drawing: Option<CanvasDrawing>,
        panels: Option<(loom_ui::paint::PaintList, loom_ui::paint::PaintList)>,
    ) -> Option<Vec<u8>> {
        let crate::Renderer::Primary(iced) = &mut self.renderer else {
            unreachable!("only a wgpu renderer draws offscreen");
        };
        let extent = wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("loom-app offscreen frame"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let target = paint::Target {
            device: &self.device,
            queue: &self.queue,
            view: &view,
            format: FORMAT,
            physical: size,
            scale,
        };
        self.drew_panels = panels.is_some();
        let panels = panels.map(|(below, above)| {
            self.panels.set(&target, below, above);
            &self.panels
        });
        paint::draw(&target, iced, &mut self.canvas, drawing, background, panels);
        read_back(&self.device, &self.queue, &texture, extent)
    }
}

impl Offscreen {
    /// Draws the last frame again, `size` physical pixels at `scale`, as
    /// the window's capture does (`paint::draw_again`), and gives its RGBA
    /// pixels.
    #[cfg(test)]
    pub(crate) fn pixels_again(&mut self, size: Size<u32>, scale: f32, background: Color) -> Option<Vec<u8>> {
        let crate::Renderer::Primary(iced) = &mut self.renderer else {
            unreachable!("only a wgpu renderer draws offscreen");
        };
        let extent = wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("loom-app offscreen frame again"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let target = paint::Target {
            device: &self.device,
            queue: &self.queue,
            view: &view,
            format: FORMAT,
            physical: size,
            scale,
        };
        let panels = self.drew_panels.then_some(&mut self.panels);
        paint::draw_again(&target, iced, &mut self.canvas, background, panels);
        read_back(&self.device, &self.queue, &texture, extent)
    }
}

/// The RGBA rows of `texture`, without the padding wgpu's copies need.
/// `None` when the GPU could not map the buffer to read it, or failed
/// while waiting for the copy.
fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    extent: wgpu::Extent3d,
) -> Option<Vec<u8>> {
    let row = extent.width as usize * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let padded = row.div_ceil(align) * align;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("loom-app offscreen read-back"),
        size: (padded * extent.height as usize) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("loom-app offscreen copy"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: None,
            },
        },
        extent,
    );
    let index = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |mapped| {
        let _ = sender.send(mapped);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: None,
        })
        .ok()?;
    // The wait above has run the callback, so its answer is here.
    receiver.try_recv().ok()?.ok()?;
    let mapped = slice.get_mapped_range();
    Some(mapped.chunks(padded).flat_map(|line| &line[..row]).copied().collect())
}

/// Finds a GPU and opens a device on it, with no window to draw to.
///
/// - **Backends:** wgpu's main ones (Metal, Vulkan, DirectX 12), or those
///   `WGPU_BACKEND` names, so CI can choose its software Vulkan.
/// - **Adapter:** the high-performance one, so a machine with two GPUs uses
///   the same one on every run, and two runs' images can be compared.
/// - **Features and limits:** none beyond wgpu's defaults. The canvas uses
///   two bind groups and Iced's renderer no more; neither asks for a
///   feature.
async fn gpu() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        ..wgpu::InstanceDescriptor::default()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..wgpu::RequestAdapterOptions::default()
        })
        .await
        .ok()?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("loom-app offscreen"),
            ..wgpu::DeviceDescriptor::default()
        })
        .await
        .ok()?;
    Some((adapter, device, queue))
}

/// The app's own panels as a still picture of a window of `logical` size:
/// painted into the list below Iced (the canvas and panels layers) and the
/// one above it (the overlays and question layers), as the window paints
/// them (`window.rs`, `paint_panels`), with nothing hovered or focused.
pub(crate) fn still_panels(
    app: &crate::App,
    logical: iced::Size,
) -> (loom_ui::paint::PaintList, loom_ui::paint::PaintList) {
    use loom_ui::paint::{ListPainter, Typesetter};
    let size = loom_ui_core::Size::new(logical.width, logical.height);
    let mut view = loom_ui::View::new();
    app.bar_panels(&mut view);
    let text = std::cell::RefCell::new(Typesetter::new());
    let still = loom_ui::Still::new(&view, size, &mut *text.borrow_mut());
    let whole = loom_ui_core::Rect::new(loom_ui_core::Point::ORIGIN, size);
    // As the window paints them (`window.rs`, `paint_panels`).
    let style = paint::panel_style(&app.theme());
    paint::paint_sides(whole, |layer, list| {
        still.paint(layer, &mut ListPainter::new(&text, list), &style);
    })
}
