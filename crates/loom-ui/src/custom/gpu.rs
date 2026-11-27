//! A custom element's GPU half: its own `wgpu` pipelines, drawn at its
//! place in paint order (ui-layer.md, section 9, rule 4).
//!
//! The host owns the pass and the window. Before the pass, each element
//! the frame draws prepares its GPU data; inside it, the host sets the
//! element's viewport and scissor, the element draws, and the host sets
//! back the viewport and scissor the painting was prepared for. The
//! element never ends the pass or changes the target. It may narrow the
//! scissor, never widen it.

use super::{FramePlan, FrameStep};
use crate::app::App;
use crate::draw::Panels;
use crate::paint::Target;
use crate::view::Layer;
use loom_ui_core::{Rect, UiId};

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
    /// x, y, width, height. It may reach past the target's edges: x and y
    /// may be as low as minus twice the device's largest texture, and the
    /// far edge as high as twice it less one, as both wgpu
    /// (`wgpu-core` 27, `set_viewport`) and the WebGPU specification
    /// (`setViewport`) allow. Its width and height stay within the largest
    /// texture.
    pub viewport: [f32; 4],
    /// The scissor the host sets for it: what of it shows, in physical
    /// pixels, x, y, width, height.
    pub scissor: [u32; 4],
    /// True for a capture rather than a frame shown.
    pub capture: bool,
}

/// What a frame gives the custom elements that draw with the GPU: the
/// device, the target, and which device it is.
pub struct GpuFrame<'a> {
    /// The device.
    pub device: &'a wgpu::Device,
    /// Its queue.
    pub queue: &'a wgpu::Queue,
    /// The target's format.
    pub format: wgpu::TextureFormat,
    /// A number the host changes whenever it makes a new device, as when
    /// a plugin's window opens again: an element whose device or format
    /// changed is told its GPU data is lost ([`CustomGpu::gpu_lost`]).
    pub generation: u64,
    /// The target, as the painting was prepared for: the whole window.
    pub target: Target,
    /// True for a capture rather than a frame shown.
    pub capture: bool,
}

/// The device and format an element last prepared for.
pub(crate) type Seen = (u64, wgpu::TextureFormat);

impl<A: App> Panels<'_, A> {
    /// Prepares every custom element `plan` draws, before the frame's pass
    /// begins, once each. An element whose device or format changed since
    /// it last prepared hears [`CustomGpu::gpu_lost`] first, so it builds
    /// its GPU data again and keeps its input state.
    pub fn prepare_gpu(&mut self, plan: &FramePlan, frame: &GpuFrame<'_>) {
        let seen: Seen = (frame.generation, frame.format);
        let whole = whole(frame.target);
        let mut lost_checked: Vec<&UiId> = Vec::new();
        for step in plan.steps() {
            let (id, bounds, scissor, above) = match step {
                FrameStep::Custom { id, bounds, clip } => {
                    let Some(scissor) = crate::paint::scissor(*clip, frame.target) else {
                        continue;
                    };
                    (id, *bounds, scissor, false)
                }
                FrameStep::Above { id, .. } => {
                    let Some(bounds) = self.customs.bounds(id) else {
                        continue;
                    };
                    (id, bounds, whole, true)
                }
                _ => continue,
            };
            let Some(hosted) = self.customs.elements.get_mut(id) else {
                continue;
            };
            let Some(gpu) = hosted.state.as_deref_mut().and_then(|state| state.gpu_mut()) else {
                continue;
            };
            if !lost_checked.contains(&id) {
                lost_checked.push(id);
                if hosted.seen.is_some_and(|before| before != seen) {
                    gpu.gpu_lost();
                }
                hosted.seen = Some(seen);
            }
            let at = Gpu {
                device: frame.device,
                queue: frame.queue,
                format: frame.format,
                target: frame.target,
                bounds,
                viewport: viewport(bounds, frame.target, frame.device),
                scissor,
                capture: frame.capture,
            };
            if above {
                gpu.prepare_above(self.app, &at);
            } else {
                gpu.prepare(self.app, &at);
            }
        }
    }

    /// Draws `plan` in `pass`: each part of the painting with `renderer`,
    /// which prepared the list the plan was made from, and each custom
    /// element at its place, with its viewport and scissor set by the
    /// host, the same as `frame` prepared them for. After each element the
    /// host sets back the viewport and the scissor of the whole target, so
    /// nothing it set reaches what is drawn after. The pass's viewport must
    /// be the whole target, as the renderer's is.
    pub fn draw_frame(
        &self,
        plan: &FramePlan,
        renderer: &crate::paint::PaintRenderer,
        pass: &mut Pass<'_>,
        frame: &GpuFrame<'_>,
    ) {
        let target = frame.target;
        let whole = whole(target);
        let restore = |pass: &mut Pass<'_>| {
            pass.set_viewport(0.0, 0.0, target.physical.0 as f32, target.physical.1 as f32, 0.0, 1.0);
            let [x, y, width, height] = whole;
            pass.set_scissor_rect(x, y, width, height);
        };
        for step in plan.steps() {
            match step {
                FrameStep::Paint(layers) => renderer.draw_range(pass, layers.clone()),
                FrameStep::Custom { id, bounds, clip } => {
                    let (Some(gpu), Some([x, y, width, height])) =
                        (self.gpu_of(id), crate::paint::scissor(*clip, target))
                    else {
                        continue;
                    };
                    let [vx, vy, vw, vh] = viewport(*bounds, target, frame.device);
                    pass.set_viewport(vx, vy, vw, vh, 0.0, 1.0);
                    pass.set_scissor_rect(x, y, width, height);
                    gpu.draw(pass);
                    restore(pass);
                }
                FrameStep::Above { id, .. } => {
                    let Some(gpu) = self.gpu_of(id) else {
                        continue;
                    };
                    restore(pass);
                    gpu.draw_above(pass);
                    restore(pass);
                }
            }
        }
    }

    /// The GPU half of the custom element `id`.
    fn gpu_of(&self, id: &UiId) -> Option<&dyn CustomGpu<A>> {
        self.customs.state(id)?.gpu()
    }
}

/// The scissor of the whole target.
fn whole(target: Target) -> [u32; 4] {
    [0, 0, target.physical.0, target.physical.1]
}

/// The viewport for an element at `bounds`: its bounds in the target's
/// physical pixels, which may reach past the target's edges, as wgpu
/// allows (`set_viewport` takes from minus twice to twice the largest
/// texture), with its size kept within the device's largest texture.
fn viewport(bounds: Rect, target: Target, device: &wgpu::Device) -> [f32; 4] {
    let limit = device.limits().max_texture_dimension_2d as f32;
    let scale = target.scale;
    [
        bounds.x * scale,
        bounds.y * scale,
        (bounds.width * scale).min(limit),
        (bounds.height * scale).min(limit),
    ]
}

#[cfg(test)]
mod tests;
