//! Draws a [`PaintList`] over the scene: its boxes with the shape pipeline
//! in screen space, and its glyphs from the atlas, layer by layer and batch
//! by batch, in the order painted.
//!
//! The plan of what to draw (`TextPlan`) is plain data built on the CPU,
//! so its order is tested without a GPU. Instances upload only when their
//! bytes differ from the last upload, so a still frame uploads nothing.

use super::atlas::{Atlas, Full, GlyphQuad, Page, Upload};
use super::{InstanceBuffer, SHAPE_ATTRIBUTES, Shape, Target};
use crate::stats::{self, Counter};
use crate::text::PaintList;
use crate::text::list::Batch;
use loom_ui_core::{Point, Rect};
use std::ops::Range;

/// One draw call of the plan.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Step {
    /// The scissor, in the target's physical pixels: x, y, width, height.
    pub(crate) scissor: [u32; 4],
    /// What it draws.
    pub(crate) draw: Draw,
}

/// What one step draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Draw {
    /// A range of the screen-space box shapes.
    Boxes(Range<u32>),
    /// A range of the glyph quads.
    Glyphs(Range<u32>),
}

/// What the painted part of a frame draws, in order.
#[derive(Debug, Clone, Default)]
pub(crate) struct TextPlan {
    /// The box shapes, in screen space: logical pixels from the canvas's
    /// corner.
    pub(crate) boxes: Vec<Shape>,
    /// The glyph quads.
    pub(crate) glyphs: Vec<GlyphQuad>,
    /// The draw calls, back to front.
    pub(crate) steps: Vec<Step>,
}

/// A clip in the host's logical pixels as a scissor in the target's
/// physical pixels, cut to the target. `None` when nothing of it is in it.
pub(crate) fn scissor(clip: Rect, target: Target) -> Option<[u32; 4]> {
    let (width, height) = (target.physical.0 as f32, target.physical.1 as f32);
    let x0 = (clip.x * target.scale).floor().clamp(0.0, width);
    let y0 = (clip.y * target.scale).floor().clamp(0.0, height);
    let x1 = ((clip.x + clip.width) * target.scale).ceil().clamp(0.0, width);
    let y1 = ((clip.y + clip.height) * target.scale).ceil().clamp(0.0, height);
    (x1 > x0 && y1 > y0).then_some([x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
}

impl TextPlan {
    /// The plan for `list`, placing its glyphs in `atlas`. When the atlas
    /// fills, it grows and the frame is placed again; at its largest size,
    /// a glyph that does not fit is left out and counted.
    pub(crate) fn build(list: &PaintList, atlas: &mut Atlas, target: Target) -> Self {
        atlas.begin_frame();
        let origin = target.origin;
        'frame: loop {
            let mut plan = TextPlan::default();
            for layer in &list.layers {
                let Some(scissor) = scissor(layer.clip, target) else {
                    continue;
                };
                for batch in &layer.batches {
                    let draw = match batch.clone() {
                        Batch::Boxes(range) => {
                            let start = plan.boxes.len() as u32;
                            plan.boxes
                                .extend(list.boxes[range.start as usize..range.end as usize].iter().map(|quad| {
                                    let rect = Rect::new(
                                        Point::new(quad.rect.x - origin.x, quad.rect.y - origin.y),
                                        quad.rect.size(),
                                    );
                                    Shape::new(rect, quad.radius, quad.fill, quad.border, quad.border_width)
                                }));
                            Draw::Boxes(start..plan.boxes.len() as u32)
                        }
                        Batch::Glyphs(range) => {
                            let start = plan.glyphs.len() as u32;
                            for glyph in &list.glyphs[range.start as usize..range.end as usize] {
                                let face = &list.faces[glyph.face as usize];
                                let clip = glyph.clip.intersection(layer.clip);
                                let Some(clip) = clip else { continue };
                                match atlas.place(glyph, face, clip, origin, target.scale) {
                                    Ok(Some(quad)) => plan.glyphs.push(quad),
                                    Ok(None) => {}
                                    Err(Full) if atlas.grow() => continue 'frame,
                                    Err(Full) => {
                                        atlas.skipped += 1;
                                        stats::count(Counter::GlyphSkips);
                                    }
                                }
                            }
                            Draw::Glyphs(start..plan.glyphs.len() as u32)
                        }
                    };
                    let empty = match &draw {
                        Draw::Boxes(range) | Draw::Glyphs(range) => range.is_empty(),
                    };
                    if !empty {
                        plan.steps.push(Step { scissor, draw });
                    }
                }
            }
            return plan;
        }
    }
}

/// The two atlas pages on the GPU, and the bind group that reads them.
struct Pages {
    mask: wgpu::Texture,
    colour: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// The GPU half of canvas text.
pub(crate) struct TextRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    pages: Option<Pages>,
    /// Globals for boxes in screen space: no camera.
    screen: wgpu::Buffer,
    screen_group: wgpu::BindGroup,
    boxes: InstanceBuffer,
    glyphs: InstanceBuffer,
    /// What was last uploaded, as bytes: boxes, then glyphs.
    uploaded: (Vec<u8>, Vec<u8>),
    pub(crate) atlas: Atlas,
    steps: Vec<Step>,
    /// The canvas's own scissor, to leave set after the steps.
    full: [u32; 4],
}

impl TextRenderer {
    /// Builds the glyph pipeline for a target of `format`, beside the
    /// canvas's pipelines, whose globals use `globals`.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        module: &wgpu::ShaderModule,
        globals: &wgpu::BindGroupLayout,
    ) -> Self {
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("loom-canvas glyph pages layout"),
            entries: &[texture(0), texture(1)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("loom-canvas glyph pipeline layout"),
            bind_group_layouts: &[globals, &layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("loom-canvas glyphs"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_glyph"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlyphQuad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &SHAPE_ATTRIBUTES,
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_glyph"),
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
        });
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("loom-canvas screen globals"),
            size: std::mem::size_of::<super::Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("loom-canvas screen globals"),
            layout: globals,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen.as_entire_binding(),
            }],
        });
        let max = device.limits().max_texture_dimension_2d.min(super::atlas::MAX_SIZE);
        Self {
            pipeline,
            layout,
            pages: None,
            screen,
            screen_group,
            boxes: InstanceBuffer::new(device, "loom-canvas painted boxes"),
            glyphs: InstanceBuffer::new(device, "loom-canvas glyphs"),
            uploaded: (Vec::new(), Vec::new()),
            atlas: Atlas::new(max),
            steps: Vec::new(),
            full: [0, 0, 0, 0],
        }
    }

    /// Places `list`'s glyphs, writes new glyph images, and uploads what
    /// changed.
    pub(crate) fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, list: &PaintList, target: Target) {
        let globals = super::Globals {
            origin: [0.0, 0.0],
            size: [target.size.width.max(1.0), target.size.height.max(1.0)],
            zoom: 1.0,
            scale: target.scale,
            cable_dim: 1.0,
            pad: 0.0,
        };
        queue.write_buffer(&self.screen, 0, bytemuck::bytes_of(&globals));
        let plan = TextPlan::build(list, &mut self.atlas, target);
        if self.atlas.resized || self.pages.is_none() {
            self.pages = Some(self.make_pages(device));
            self.atlas.resized = false;
        }
        if let Some(pages) = &self.pages {
            for upload in self.atlas.uploads.drain(..) {
                write(queue, pages, &upload);
                stats::count(Counter::GlyphRasters);
            }
        }
        let boxes: &[u8] = bytemuck::cast_slice(&plan.boxes);
        let glyphs: &[u8] = bytemuck::cast_slice(&plan.glyphs);
        if self.uploaded.0 != boxes || self.uploaded.1 != glyphs {
            stats::count(Counter::TextUploads);
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
    }

    /// Draws the steps last prepared, then leaves the scissor as the
    /// canvas's own.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, shapes: &wgpu::RenderPipeline) {
        let Some(pages) = &self.pages else { return };
        if self.steps.is_empty() {
            return;
        }
        for step in &self.steps {
            let [x, y, width, height] = step.scissor;
            pass.set_scissor_rect(x, y, width, height);
            match &step.draw {
                Draw::Boxes(range) => {
                    pass.set_pipeline(shapes);
                    pass.set_bind_group(0, &self.screen_group, &[]);
                    pass.set_vertex_buffer(0, self.boxes.slice());
                    pass.draw(0..4, range.clone());
                }
                Draw::Glyphs(range) => {
                    pass.set_pipeline(&self.pipeline);
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
        let mask = page("loom-canvas glyph coverage", wgpu::TextureFormat::R8Unorm);
        let colour = page("loom-canvas glyph colour", wgpu::TextureFormat::Rgba8Unorm);
        let view = |texture: &wgpu::Texture| texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("loom-canvas glyph pages"),
            layout: &self.layout,
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
