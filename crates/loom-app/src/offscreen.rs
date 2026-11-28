//! Drawing offscreen, with wgpu: for `--screenshot-window`, `--screenshot`,
//! and the `Ui` harness's captures.
//!
//! A whole window is drawn by the window's own painter (`drawing.rs`), so a
//! capture is the frame the window shows. `--screenshot` draws the canvas
//! alone, with its renderer, filling the image.

use crate::App;
use crate::drawing::{FrameTarget, WindowPainter};
use loom_canvas::{CanvasDrawing, CanvasRenderer};
use loom_ui::paint::Typesetter;
use loom_ui_core::{Color, Point, Size};
use std::cell::RefCell;

/// The texture format offscreen frames are drawn in: 8-bit RGBA, read back
/// as the PNG's bytes with no reordering. Not sRGB, because the app's
/// colours are given as the bytes to show, as the window's surface format
/// is chosen too (`loom_ui::shell`). So a capture has the window's colours.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A GPU device with the window's painter and the canvas's renderer, for
/// drawing frames offscreen.
pub(crate) struct Offscreen {
    /// The device.
    device: wgpu::Device,
    /// Its queue.
    queue: wgpu::Queue,
    /// The window's renderers, for a whole window.
    painter: WindowPainter,
    /// The canvas's renderer, for the canvas alone.
    canvas: CanvasRenderer,
}

impl Offscreen {
    /// A device and the renderers, or `None` when no GPU adapter is found.
    pub(crate) fn new() -> Option<Self> {
        let (device, queue) = pollster::block_on(gpu())?;
        Some(Self {
            painter: WindowPainter::new(&device, FORMAT),
            canvas: CanvasRenderer::new(&device, FORMAT),
            device,
            queue,
        })
    }

    /// Lays out and draws the whole window `panels` show, `size` logical
    /// pixels at `scale`, as the window draws a frame, with `text` for its
    /// text. Gives its RGBA pixels, row by row, or `None` when they could
    /// not be read back from the GPU.
    pub(crate) fn window(
        &mut self,
        panels: &mut loom_ui::Panels<'_, App>,
        text: &RefCell<Typesetter>,
        size: Size,
        scale: f32,
    ) -> Option<Vec<u8>> {
        let (texture, extent) = self.texture(physical(size, scale));
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let target = FrameTarget {
            device: &self.device,
            queue: &self.queue,
            view: &view,
            format: FORMAT,
            size,
            scale,
            capture: true,
        };
        self.painter.draw(panels, text, &target);
        read_back(&self.device, &self.queue, &texture, extent)
    }

    /// Draws the canvas alone from `drawing`, filling an image `size`
    /// physical pixels at `scale`, on `background`. Gives its RGBA pixels.
    pub(crate) fn canvas(
        &mut self,
        size: (u32, u32),
        scale: f32,
        background: Color,
        drawing: &CanvasDrawing,
    ) -> Option<Vec<u8>> {
        let (texture, extent) = self.texture(size);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let target = loom_ui::paint::Target {
            size: drawing.bounds.size(),
            scale,
            origin: Point::new(drawing.bounds.x, drawing.bounds.y),
            physical: size,
        };
        self.canvas.prepare(&self.device, &self.queue, &drawing.frame, target);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("loom-app canvas"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("loom-app canvas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(background.r),
                            g: f64::from(background.g),
                            b: f64::from(background.b),
                            a: f64::from(background.a),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.canvas.draw(&mut pass);
        }
        self.queue.submit([encoder.finish()]);
        read_back(&self.device, &self.queue, &texture, extent)
    }

    /// A texture to draw into and read back, `size` physical pixels.
    fn texture(&self, (width, height): (u32, u32)) -> (wgpu::Texture, wgpu::Extent3d) {
        let extent = wgpu::Extent3d {
            width,
            height,
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
        (texture, extent)
    }
}

/// `size` logical pixels at `scale`, in whole physical pixels.
pub(crate) fn physical(size: Size, scale: f32) -> (u32, u32) {
    // A texture has at least one pixel on each side, as an image does
    // (`screenshot::physical_size`); a window shrunk to nothing still
    // draws one.
    let side = |v: f32| ((v * scale).round() as u32).max(1);
    (side(size.width), side(size.height))
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
/// - **Features and limits:** none beyond wgpu's defaults.
async fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
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
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("loom-app offscreen"),
            ..wgpu::DeviceDescriptor::default()
        })
        .await
        .ok()
}

#[cfg(test)]
mod tests;
