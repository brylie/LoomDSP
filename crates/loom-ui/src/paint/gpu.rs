//! Draws a [`PaintList`] with `wgpu`: its boxes with the shape stage of
//! `paint.wgsl` and its glyphs from the atlas's pages, layer by layer and
//! batch by batch, in the order painted ([`PaintPlan`]).
//!
//! Instances upload only when their bytes differ from the last upload, so
//! a still frame uploads nothing. The renderer never begins or ends a pass
//! and never presents: the host owns the target and the frame.

use super::atlas::{Atlas, GlyphQuad, Page, Upload};
use super::plan::{Draw, PaintPlan, Step, scissor};
use super::{PaintList, Shape, Target};
use bytemuck::{Pod, Zeroable};
use loom_ui_core::Rect;

/// The vertex layout of a [`Shape`], and of a glyph quad, which has the
/// same four vectors: four `vec4<f32>`s at locations 0 to 3.
pub const SHAPE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

/// The globals of `paint.wgsl`, as its `Globals` struct lays them out.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    origin: [f32; 2],
    size: [f32; 2],
    zoom: f32,
    scale: f32,
    dim: f32,
    pad: f32,
}

/// A vertex buffer of instances that grows as needed and is never shrunk.
pub struct InstanceBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
    len: u32,
    label: &'static str,
}

impl InstanceBuffer {
    /// An empty buffer, with room for 4 KiB of instances to start.
    pub fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self {
            buffer: Self::create(device, label, 4096),
            capacity: 4096,
            len: 0,
            label,
        }
    }

    /// A buffer of `size` bytes for instances.
    fn create(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Writes `items` from the buffer's start, growing it to the next power
    /// of two when they do not fit.
    pub fn upload<T: Pod>(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, items: &[T]) {
        let bytes: &[u8] = bytemuck::cast_slice(items);
        let needed = bytes.len() as u64;
        if needed > self.capacity {
            self.capacity = needed.next_power_of_two();
            self.buffer = Self::create(device, self.label, self.capacity);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
        self.len = items.len() as u32;
    }

    /// How many instances the last upload wrote.
    pub fn len(&self) -> u32 {
        self.len
    }

    /// True when the last upload wrote none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The whole buffer, to bind as a vertex buffer.
    pub fn slice(&self) -> wgpu::BufferSlice<'_> {
        self.buffer.slice(..)
    }
}

/// What one [`PaintRenderer::prepare`] did, for a host's frame counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaintCounts {
    /// Glyph images written to the atlas's pages.
    pub glyph_rasters: u64,
    /// 1 when the boxes or glyphs differed from the last frame and were
    /// uploaded, else 0.
    pub uploads: u64,
    /// Glyphs left out because the atlas was full at its largest size.
    pub glyph_skips: u64,
}

/// The two atlas pages on the GPU, and the bind group that reads them.
struct Pages {
    mask: wgpu::Texture,
    colour: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// Draws painted boxes and glyphs.
///
/// A host gives it a device, a queue, and a render pass, and calls
/// [`prepare`](Self::prepare) then [`draw`](Self::draw) for each frame it
/// draws. One renderer owns its buffers and its atlas, so two hosts need
/// two renderers. It assumes a colour target with one sample per pixel,
/// and blends with premultiplied alpha.
pub struct PaintRenderer {
    shapes: wgpu::RenderPipeline,
    glyphs_pipeline: wgpu::RenderPipeline,
    pages_layout: wgpu::BindGroupLayout,
    pages: Option<Pages>,
    /// Globals for logical pixels from the viewport's corner: no zoom.
    screen: wgpu::Buffer,
    screen_group: wgpu::BindGroup,
    boxes: InstanceBuffer,
    glyphs: InstanceBuffer,
    /// What was last uploaded, as bytes: boxes, then glyphs.
    uploaded: (Vec<u8>, Vec<u8>),
    atlas: Atlas,
    steps: Vec<Step>,
    /// The viewport's own scissor, to leave set after the steps.
    full: [u32; 4],
}

/// One pipeline over `paint.wgsl`'s stages `vertex` and `fragment`, reading
/// instances of `stride` bytes laid out as [`SHAPE_ATTRIBUTES`].
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    (label, vertex, fragment): (&str, &str, &str),
    stride: usize,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: stride as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &SHAPE_ATTRIBUTES,
            }],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

impl PaintRenderer {
    /// Builds the shape and glyph pipelines for a colour target of
    /// `format`, from [`SHADER`](super::SHADER) alone.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("loom-ui paint shaders"),
            source: wgpu::ShaderSource::Wgsl(super::SHADER.into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("loom-ui paint globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Globals>() as u64),
                },
                count: None,
            }],
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let pages_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("loom-ui glyph pages layout"),
            entries: &[texture(0), texture(1)],
        });
        let layout = |label, groups: &[&wgpu::BindGroupLayout]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                push_constant_ranges: &[],
            })
        };
        let shapes = pipeline(
            device,
            &layout("loom-ui shape pipeline layout", &[&globals_layout]),
            &module,
            format,
            ("loom-ui shapes", "vs_shape", "fs_shape"),
            std::mem::size_of::<Shape>(),
        );
        let glyphs_pipeline = pipeline(
            device,
            &layout("loom-ui glyph pipeline layout", &[&globals_layout, &pages_layout]),
            &module,
            format,
            ("loom-ui glyphs", "vs_glyph", "fs_glyph"),
            std::mem::size_of::<GlyphQuad>(),
        );
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("loom-ui paint globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("loom-ui paint globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen.as_entire_binding(),
            }],
        });
        let max = device.limits().max_texture_dimension_2d.min(super::atlas::MAX_SIZE);
        Self {
            shapes,
            glyphs_pipeline,
            pages_layout,
            pages: None,
            screen,
            screen_group,
            boxes: InstanceBuffer::new(device, "loom-ui painted boxes"),
            glyphs: InstanceBuffer::new(device, "loom-ui glyphs"),
            uploaded: (Vec::new(), Vec::new()),
            atlas: Atlas::new(max),
            steps: Vec::new(),
            full: [0, 0, 0, 0],
        }
    }

    /// Places `list`'s glyphs, writes new glyph images, and uploads what
    /// changed. Call it once per drawn frame, before the pass begins.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        list: &PaintList,
        target: Target,
    ) -> PaintCounts {
        let mut counts = PaintCounts::default();
        let globals = Globals {
            origin: [0.0, 0.0],
            size: [target.size.width.max(1.0), target.size.height.max(1.0)],
            zoom: 1.0,
            scale: target.scale,
            dim: 1.0,
            pad: 0.0,
        };
        queue.write_buffer(&self.screen, 0, bytemuck::bytes_of(&globals));
        let plan = PaintPlan::build(list, &mut self.atlas, target);
        counts.glyph_skips = plan.skipped;
        if self.atlas.resized || self.pages.is_none() {
            self.pages = Some(self.make_pages(device));
            self.atlas.resized = false;
        }
        if let Some(pages) = &self.pages {
            for upload in self.atlas.uploads.drain(..) {
                write(queue, pages, &upload);
                counts.glyph_rasters += 1;
            }
        }
        let boxes: &[u8] = bytemuck::cast_slice(&plan.boxes);
        let glyphs: &[u8] = bytemuck::cast_slice(&plan.glyphs);
        if self.uploaded.0 != boxes || self.uploaded.1 != glyphs {
            counts.uploads = 1;
        }
        if self.uploaded.0 != boxes {
            self.boxes.upload(device, queue, &plan.boxes);
            self.uploaded.0 = boxes.to_vec();
        }
        if self.uploaded.1 != glyphs {
            self.glyphs.upload(device, queue, &plan.glyphs);
            self.uploaded.1 = glyphs.to_vec();
        }
        self.full = scissor(Rect::new(target.origin, target.size), target).unwrap_or([0, 0, 0, 0]);
        self.steps = plan.steps;
        counts
    }

    /// Draws the steps last prepared, then leaves the scissor as the
    /// viewport's own, for whatever the host draws after.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(pages) = &self.pages else { return };
        if self.steps.is_empty() {
            return;
        }
        for step in &self.steps {
            let [x, y, width, height] = step.scissor;
            pass.set_scissor_rect(x, y, width, height);
            match &step.draw {
                Draw::Boxes(range) => {
                    pass.set_pipeline(&self.shapes);
                    pass.set_bind_group(0, &self.screen_group, &[]);
                    pass.set_vertex_buffer(0, self.boxes.slice());
                    pass.draw(0..4, range.clone());
                }
                Draw::Glyphs(range) => {
                    pass.set_pipeline(&self.glyphs_pipeline);
                    pass.set_bind_group(0, &self.screen_group, &[]);
                    pass.set_bind_group(1, &pages.bind_group, &[]);
                    pass.set_vertex_buffer(0, self.glyphs.slice());
                    pass.draw(0..4, range.clone());
                }
            }
        }
        let [x, y, width, height] = self.full;
        if width > 0 && height > 0 {
            pass.set_scissor_rect(x, y, width, height);
        }
    }

    /// Empty pages at the atlas's size.
    fn make_pages(&self, device: &wgpu::Device) -> Pages {
        let size = self.atlas.size();
        let page = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let mask = page("loom-ui glyph coverage", wgpu::TextureFormat::R8Unorm);
        let colour = page("loom-ui glyph colour", wgpu::TextureFormat::Rgba8Unorm);
        let view = |texture: &wgpu::Texture| texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("loom-ui glyph pages"),
            layout: &self.pages_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view(&mask)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view(&colour)),
                },
            ],
        });
        Pages {
            mask,
            colour,
            bind_group,
        }
    }
}

/// Writes one glyph image into its page.
fn write(queue: &wgpu::Queue, pages: &Pages, upload: &Upload) {
    let texture = match upload.page {
        Page::Mask => &pages.mask,
        Page::Colour => &pages.colour,
    };
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: upload.x,
                y: upload.y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        &upload.data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(upload.width * upload.page.bytes()),
            rows_per_image: Some(upload.height),
        },
        wgpu::Extent3d {
            width: upload.width,
            height: upload.height,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests;
