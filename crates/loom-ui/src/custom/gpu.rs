//! A custom element's GPU half: its own `wgpu` pipelines, drawn at its
//! place in paint order (ui-layer.md, section 9, rule 4).
//!
//! The host owns the pass and the window. Before the pass, each element
//! the frame draws prepares its GPU data; inside it, the host sets the
//! element's viewport and scissor, the element draws, and the host sets
//! back the viewport and scissor the painting was prepared for. The
//! element never ends the pass or changes the target. It may narrow the
//! scissor, never widen it.

use crate::app::App;
use crate::paint::Target;
use crate::view::Layer;
use loom_ui_core::Rect;

/// The host's render pass, as [`CustomGpu::draw`] gets it: `wgpu`'s, named
/// here so the rest of the crate never names `wgpu`.
pub type Pass<'a> = wgpu::RenderPass<'a>;

/// What a custom element draws with the GPU.
pub trait CustomGpu<A: App> {
    /// Builds or updates its GPU data for this frame, before the pass
    /// begins. Called only for a frame drawn, and only when it shows.
    fn prepare(&mut self, app: &A, gpu: &Gpu<'_>);

    /// Draws inside the host's pass. The viewport is its bounds and the
    /// scissor what of it shows, both already set, in physical pixels.
    fn draw(&self, pass: &mut Pass<'_>);

    /// The window layer it draws above a second time, for something that
    /// leaves its bounds while a gesture lasts, as a cable dragged to a
    /// panel. That drawing is cut to the window, and takes no input and
    /// has no nodes. `None`, by default, when it draws only in place.
    fn above(&self, app: &A) -> Option<Layer> {
        let _ = app;
        None
    }

    /// Prepares what it draws above, as [`CustomGpu::prepare`] does.
    fn prepare_above(&mut self, app: &A, gpu: &Gpu<'_>) {
        let _ = (app, gpu);
    }

    /// Draws above, with the viewport the whole window and the scissor too.
    fn draw_above(&self, pass: &mut Pass<'_>) {
        let _ = pass;
    }

    /// The device or the target's format changed, as when a plugin's
    /// window opens again or a web page loses its GPU: drop every GPU
    /// resource and build them again in the next prepare. Its input state
    /// stays. By default it keeps nothing to drop.
    fn gpu_lost(&mut self) {}
}

/// What a custom element prepares with.
pub struct Gpu<'a> {
    /// The device.
    pub device: &'a wgpu::Device,
    /// Its queue.
    pub queue: &'a wgpu::Queue,
    /// The format of the target it draws into.
    pub format: wgpu::TextureFormat,
    /// The frame's target, as its painting was prepared for.
    pub target: Target,
    /// Where it is in the window, in logical pixels.
    pub bounds: Rect,
    /// The viewport the host sets for it: its bounds in physical pixels,
    /// x, y, width, height. It may reach past the target's edges.
    pub viewport: [f32; 4],
    /// The scissor the host sets for it: what of it shows, in physical
    /// pixels, x, y, width, height.
    pub scissor: [u32; 4],
    /// True for a capture rather than a frame shown.
    pub capture: bool,
}
