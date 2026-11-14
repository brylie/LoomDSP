//! The window's GPU device and surface, and drawing a frame into a view.
//!
//! The device and the surface are chosen as `iced_wgpu` chooses them today
//! (research note `docs/research/2026-11-winit-shell-facts.md`, section
//! 1), so a frame the app draws through the shell has today's pixels:
//!
//! - the adapter: high performance, unless `WGPU_POWER_PREF` says other;
//! - the format: the first with no extra GPU feature that is not sRGB,
//!   since LoomDSP's colours are written as raw components;
//! - the alpha mode: post-multiplied, then pre-multiplied, then the
//!   system's choice;
//! - the limits: the defaults, then the downlevel ones, each with at most
//!   two bind groups;
//! - vertical sync, and one frame of latency.
//!
//! The shell never draws a layer itself. It hands a [`Frame`] to the
//! layers, who draw everything into it, the clear included, so the same
//! call draws the window and a capture.

use super::Frame;
use super::pacing::Drawn;
use crate::effect::Image;
use loom_ui_core::Size;
use std::fmt;
use std::sync::Arc;
use winit::dpi::PhysicalSize;
use winit::window::Window;

/// Why the window has no GPU to draw with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuError {
    /// No adapter can draw to this window.
    NoAdapter,
    /// The adapter gave no device, with what it said for each limit set.
    NoDevice(String),
    /// The window's surface could not be made.
    Surface(String),
    /// The surface offers no format to draw in.
    NoFormat,
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter => write!(f, "no GPU adapter can draw to the window"),
            Self::NoDevice(why) => write!(f, "the GPU adapter gave no device: {why}"),
            Self::Surface(why) => write!(f, "the window's GPU surface could not be made: {why}"),
            Self::NoFormat => write!(f, "the window's GPU surface offers no format"),
        }
    }
}

impl std::error::Error for GpuError {}

/// A GPU device and the format it draws in.
pub(crate) struct Device {
    /// The device.
    pub device: wgpu::Device,
    /// Its queue.
    pub queue: wgpu::Queue,
    /// The format frames are drawn in.
    pub format: wgpu::TextureFormat,
}

/// The window's device and surface.
pub(crate) struct Gpu {
    /// The adapter the device came from, for layers that build their
    /// renderer from it.
    pub adapter: wgpu::Adapter,
    /// The device, for drawing and for captures.
    pub device: Device,
    /// The window's surface.
    surface: wgpu::Surface<'static>,
    /// How the surface is set up; its size follows the window.
    config: wgpu::SurfaceConfiguration,
}

/// The instance for every backend wgpu was built with, unless
/// `WGPU_BACKEND` names others.
fn instance() -> wgpu::Instance {
    wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all()),
        ..Default::default()
    })
}

/// A device from `adapter`, with the default limits or else the downlevel
/// ones, each held to two bind groups.
fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), GpuError> {
    let mut errors = Vec::new();
    for limits in [wgpu::Limits::default(), wgpu::Limits::downlevel_defaults()] {
        let required_limits = wgpu::Limits {
            max_bind_groups: 2,
            max_non_sampler_bindings: 2048,
            ..limits
        };
        let request = adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("loom-ui shell"),
            required_features: wgpu::Features::empty(),
            required_limits,
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        });
        match pollster::block_on(request) {
            Ok(found) => return Ok(found),
            Err(error) => errors.push(error.to_string()),
        }
    }
    Err(GpuError::NoDevice(errors.join("; ")))
}

/// The adapter options: high performance unless the environment says
/// other.
fn adapter_options<'a>(surface: Option<&'a wgpu::Surface<'a>>) -> wgpu::RequestAdapterOptions<'a, 'a> {
    wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::HighPerformance),
        compatible_surface: surface,
        force_fallback_adapter: false,
    }
}

/// The first format with no extra GPU feature that is not sRGB, or the
/// first at all.
fn pick_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .filter(|format| format.required_features() == wgpu::Features::empty())
        .find(|format| !format.is_srgb())
        .or_else(|| formats.first().copied())
}

/// Post-multiplied alpha if offered, then pre-multiplied, then the
/// system's choice.
fn pick_alpha(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    [
        wgpu::CompositeAlphaMode::PostMultiplied,
        wgpu::CompositeAlphaMode::PreMultiplied,
    ]
    .into_iter()
    .find(|mode| modes.contains(mode))
    .unwrap_or(wgpu::CompositeAlphaMode::Auto)
}

impl Gpu {
    /// A device and a surface for `window`, sized to it.
    pub fn new(window: Arc<Window>) -> Result<Self, GpuError> {
        let instance = instance();
        let size = window.inner_size();
        let surface = instance
            .create_surface(window)
            .map_err(|error| GpuError::Surface(error.to_string()))?;
        let adapter = pollster::block_on(instance.request_adapter(&adapter_options(Some(&surface))))
            .map_err(|_| GpuError::NoAdapter)?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = pick_format(&capabilities.formats).ok_or(GpuError::NoFormat)?;
        let (device, queue) = request_device(&adapter)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode: pick_alpha(&capabilities.alpha_modes),
            view_formats: Vec::new(),
        };
        let gpu = Self {
            adapter,
            device: Device { device, queue, format },
            surface,
            config,
        };
        gpu.configure();
        Ok(gpu)
    }

    /// Sets the surface up at its size, unless the window has none.
    fn configure(&self) {
        if self.config.width > 0 && self.config.height > 0 {
            self.surface.configure(&self.device.device, &self.config);
        }
    }

    /// Follows the window to `size`. A window with no size (minimised)
    /// keeps its surface until it has one again.
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.configure();
    }

    /// Draws one frame into the window: `draw` draws everything into the
    /// surface's next texture, then `window` is told and the frame shown.
    /// Says whether the frame was shown, or why not.
    pub fn frame(&mut self, logical: Size, scale: f32, window: &Window, draw: impl FnOnce(&Frame<'_>)) -> Drawn {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Drawn::Skipped;
        }
        let texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.configure();
                return Drawn::Retry;
            }
            Err(_) => return Drawn::Skipped,
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        draw(&Frame {
            device: &self.device.device,
            queue: &self.device.queue,
            view: &view,
            format: self.device.format,
            size: logical,
            scale,
        });
        window.pre_present_notify();
        texture.present();
        Drawn::Shown
    }
}

/// Draws one frame into a texture of `physical` pixels in the device's
/// format, reads it back, and gives it in RGBA order. The layers built
/// their pipelines for that format, so the capture draws in it too, and
/// gives `None` for a format it cannot read as 8-bit RGBA or BGRA, or when
/// the read-back fails.
pub(crate) fn capture(
    device: &Device,
    physical: PhysicalSize<u32>,
    scale: f32,
    draw: impl FnOnce(&Frame<'_>),
) -> Option<Image> {
    let bgra = eight_bit_order(device.format)?;
    let (width, height) = (physical.width.max(1), physical.height.max(1));
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("loom-ui capture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: device.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    draw(&Frame {
        device: &device.device,
        queue: &device.queue,
        view: &view,
        format: device.format,
        size: Size::new(width as f32 / scale, height as f32 / scale),
        scale,
    });
    let row = width as usize * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let padded = row.div_ceil(align) * align;
    let buffer = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("loom-ui capture read-back"),
        size: (padded * height as usize) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("loom-ui capture copy"),
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
    let index = device.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: None,
        })
        .ok()?;
    // The poll waited for the copy, but only the callback says whether
    // the buffer was mapped; reading an unmapped one panics.
    receiver.try_recv().ok()?.ok()?;
    let mapped = slice.get_mapped_range();
    let mut rgba = Vec::with_capacity(row * height as usize);
    for line in mapped.chunks(padded) {
        rgba.extend_from_slice(&line[..row]);
    }
    drop(mapped);
    if bgra {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    Image::new(width, height, scale, rgba)
}

/// For a format of four 8-bit channels, whether its bytes are blue, green,
/// red, alpha (true) or red, green, blue, alpha (false). `None` for any
/// other format: wider channels, or packed ones such as 10-bit colour.
fn eight_bit_order(format: wgpu::TextureFormat) -> Option<bool> {
    use wgpu::TextureFormat as F;
    match format {
        F::Bgra8Unorm | F::Bgra8UnormSrgb => Some(true),
        F::Rgba8Unorm | F::Rgba8UnormSrgb => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
