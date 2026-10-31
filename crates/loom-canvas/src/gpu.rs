//! GPU data and the custom wgpu primitive that draws the canvas.

use crate::camera::Camera;
use crate::controls::ControlGeometry;
use crate::scene::{CLOCK_MARK_RADIUS, ModuleView, PORT_RADIUS, PortShape, Scene};
use crate::stats::{self, Counter};
use crate::theme::Palette;
use crate::tile_text::{self, TextKind};
use crate::trace::{Segment, TraceGeometry};
use bytemuck::{Pod, Zeroable};
use iced::advanced::graphics::color::pack;
use iced::wgpu;
use iced::widget::shader::{Pipeline, Primitive, Viewport};
use iced::{Color, Point, Rectangle};

/// Corner radius of tiles, in world units.
const TILE_RADIUS: f32 = 5.0;
/// Cable width, in world units.
const CABLE_WIDTH: f32 = 2.4;
/// Thinnest a cable is drawn, in physical pixels.
const CABLE_MIN_PX: f32 = 1.2;

/// One rounded rectangle.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Shape {
    rect: [f32; 4],
    fill: [f32; 4],
    border: [f32; 4],
    params: [f32; 4],
}

impl Shape {
    /// A filled rounded rectangle with an optional border.
    pub fn new(rect: Rectangle, radius: f32, fill: Color, border: Color, border_width: f32) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.width, rect.height],
            fill: pack(fill).components(),
            border: pack(border).components(),
            params: [radius, border_width, 1.0, 0.0],
        }
    }

    /// The rectangle: x, y, width, height in world units.
    #[cfg(test)]
    pub fn rect(&self) -> [f32; 4] {
        self.rect
    }

    /// The fill colour, as uploaded.
    #[cfg(test)]
    pub fn fill(&self) -> [f32; 4] {
        self.fill
    }

    /// The border colour, as uploaded.
    #[cfg(test)]
    pub fn border(&self) -> [f32; 4] {
        self.border
    }

    /// The border width, in world units.
    #[cfg(test)]
    pub fn border_width(&self) -> f32 {
        self.params[1]
    }

    /// A circle or square marker centred on a point.
    fn marker(centre: Point, radius: f32, shape: PortShape, fill: Color, border: Color, border_width: f32) -> Self {
        let rect = Rectangle::new(
            Point::new(centre.x - radius, centre.y - radius),
            iced::Size::new(radius * 2.0, radius * 2.0),
        );
        let corner = match shape {
            PortShape::Round => radius,
            PortShape::Square => radius * 0.3,
        };
        Self::new(rect, corner, fill, border, border_width)
    }
}

/// Part of a ring, for knobs. Its quad covers only the ring, so the
/// fragment shader never runs outside it (ADR-0010).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Arc {
    /// Centre x, centre y, radius, thickness, in world units.
    circle: [f32; 4],
    /// Start angle and sweep in radians, clockwise from the top; unused.
    angles: [f32; 4],
    colour: [f32; 4],
    unused: [f32; 4],
}

impl Arc {
    /// A ring segment from `start` through `sweep` radians, clockwise from
    /// the top.
    pub fn new(centre: Point, radius: f32, thickness: f32, start: f32, sweep: f32, colour: Color) -> Self {
        Self {
            circle: [centre.x, centre.y, radius, thickness],
            angles: [start, sweep, 0.0, 0.0],
            colour: pack(colour).components(),
            unused: [0.0; 4],
        }
    }
}

/// One cable.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Cable {
    ends: [f32; 4],
    colour_from: [f32; 4],
    colour_to: [f32; 4],
    params: [f32; 4],
}

impl Cable {
    /// A cable between two world points.
    pub fn new(from: Point, to: Point, colour_from: Color, colour_to: Color, dashed: bool) -> Self {
        Self {
            ends: [from.x, from.y, to.x, to.y],
            colour_from: pack(colour_from).components(),
            colour_to: pack(colour_to).components(),
            params: [CABLE_WIDTH, if dashed { 1.0 } else { 0.0 }, CABLE_MIN_PX, 0.0],
        }
    }

    /// Draws the cable at a third of the usual width: a modulation line.
    pub fn thin(mut self) -> Self {
        self.params[0] = CABLE_WIDTH / 3.0;
        self
    }

    /// Draws the cable at full opacity, in front of the tiles.
    pub fn focused(mut self) -> Self {
        self.params[3] = 1.0;
        self
    }
}

/// GPU instances for one scene. Built on the CPU when the scene changes,
/// uploaded once, then reused every frame.
#[derive(Debug, Default)]
pub struct Geometry {
    /// The scene generation this was built from.
    pub generation: u64,
    /// Cables, drawn behind the tiles and dimmed. Focused cables are drawn
    /// again in front, from the overlay.
    pub cables: Vec<Cable>,
    /// Tiles, then text bars, then ports. Tiles come first so cables can go
    /// between them.
    pub shapes: Vec<Shape>,
    /// Number of tile shapes at the start of `shapes`.
    pub tiles: u32,
    /// After the tiles come the shapes that only some zoom levels draw
    /// (panels.md, Level of detail), in this order, so each level draws one
    /// range (see [`Geometry::bars`]): clock marks, clock dots, caption bars,
    /// port label bars, title bars.
    ///
    /// Shapes of the full clock marks on transport inputs: a ring and two
    /// hands each, drawn close up.
    pub clock_marks: u32,
    /// Number of plain dots that stand in for clock marks at middle zoom.
    pub clock_dots: u32,
    /// Bars standing in for text too small to draw: caption bars.
    pub caption_bars: u32,
    /// Number of port label bars.
    pub port_bars: u32,
    /// Number of title bars.
    pub title_bars: u32,
}

impl Geometry {
    /// Builds instances for every module, port, and cable in a scene.
    pub fn build(scene: &Scene, palette: &Palette) -> Self {
        stats::count(Counter::SceneBuilds);
        let mut cables = Vec::with_capacity(scene.cables().len());
        for cable in scene.cables() {
            let (Some(from), Some(to)) = (
                scene.module(cable.from.0).filter(|m| cable.from.1 < m.outputs.len()),
                scene.module(cable.to.0).filter(|m| cable.to.1 < m.inputs.len()),
            ) else {
                continue;
            };
            cables.push(Cable::new(
                from.output_position(cable.from.1),
                to.input_position(cable.to.1),
                cable.colour_from,
                cable.colour_to,
                cable.delayed,
            ));
        }

        let ports: usize = scene.modules().iter().map(|m| m.inputs.len() + m.outputs.len()).sum();
        let mut shapes = Vec::with_capacity(scene.modules().len() * 3 + ports);
        let mut markers = Vec::with_capacity(ports);
        for module in scene.modules() {
            let bounds = module.bounds();
            shapes.push(Shape::new(bounds, TILE_RADIUS, palette.tile, palette.tile_border, 1.0));
            // Header: a rounded strip, then a square strip over its lower
            // half so only the top corners are round.
            let header = Rectangle::new(module.position, iced::Size::new(module.width(), crate::scene::HEADER));
            shapes.push(Shape::new(header, TILE_RADIUS, module.accent, Color::TRANSPARENT, 0.0));
            let lower = Rectangle::new(
                Point::new(header.x, header.y + header.height * 0.5),
                iced::Size::new(header.width, header.height * 0.5),
            );
            shapes.push(Shape::new(lower, 0.0, module.accent, Color::TRANSPARENT, 0.0));

            let module_ports = module
                .inputs
                .iter()
                .enumerate()
                .map(|(i, p)| (module.input_position(i), p, false, i))
                .chain(
                    module
                        .outputs
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (module.output_position(i), p, true, i)),
                );
            for (centre, port, _, _) in module_ports {
                // Activity fills connected ports from a separate buffer.
                // The scene's marker remains an outline while silent.
                markers.push(Shape::marker(
                    centre,
                    PORT_RADIUS,
                    port.shape,
                    palette.tile,
                    port.colour,
                    1.6,
                ));
            }
            // Assignment count badges sit with the markers, over the
            // cables. Their numbers are text.
            for index in 0..module.outputs.len() {
                if let Some(rect) = scene.badge_bounds(module.id, index) {
                    markers.push(Shape::new(
                        rect,
                        rect.height * 0.5,
                        palette.modulation,
                        Color::TRANSPARENT,
                        0.0,
                    ));
                }
            }
        }
        let tiles = shapes.len() as u32;

        let (mut marks, mut dots) = (Vec::new(), Vec::new());
        for module in scene.modules() {
            clock_marks(module, palette, &mut marks, &mut dots);
        }
        let (clock_marks, clock_dots) = (marks.len() as u32, dots.len() as u32);
        shapes.append(&mut marks);
        shapes.append(&mut dots);

        let (mut captions, mut labels, mut titles) = (Vec::new(), Vec::new(), Vec::new());
        for module in scene.modules() {
            tile_text::for_each(module, |item| {
                let bar = item.bar();
                if bar.width <= 0.0 {
                    return;
                }
                let (list, colour) = match item.kind {
                    TextKind::Title => (&mut titles, palette.title_bar),
                    TextKind::Port(_) => (&mut labels, palette.text_bar),
                    TextKind::Caption(_) | TextKind::Option(..) => (&mut captions, palette.text_bar),
                };
                list.push(Shape::new(bar, bar.height * 0.5, colour, Color::TRANSPARENT, 0.0));
            });
        }
        let (caption_bars, port_bars, title_bars) = (captions.len() as u32, labels.len() as u32, titles.len() as u32);
        shapes.append(&mut captions);
        shapes.append(&mut labels);
        shapes.append(&mut titles);
        shapes.append(&mut markers);

        Self {
            generation: scene.generation(),
            cables,
            shapes,
            tiles,
            clock_marks,
            clock_dots,
            caption_bars,
            port_bars,
            title_bars,
        }
    }

    /// Index of the first port marker in `shapes`.
    pub fn markers(&self) -> u32 {
        self.tiles + self.clock_marks + self.clock_dots + self.caption_bars + self.port_bars + self.title_bars
    }

    /// The shapes each zoom level adds to the tiles. Close up (`shown`
    /// false): the full clock marks, and no text bars, since the text is
    /// drawn. At middle zoom (`titles` false): clock dots, and bars for port
    /// labels and captions. Far out (`titles` true): bars for titles and
    /// port labels, where controls, captions, and clock marks are not drawn.
    pub fn bars(&self, shown: bool, titles: bool) -> std::ops::Range<u32> {
        let marks = self.tiles;
        let dots = marks + self.clock_marks;
        let captions = dots + self.clock_dots;
        let ports = captions + self.caption_bars;
        match (shown, titles) {
            (false, _) => marks..dots,
            (true, false) => dots..ports + self.port_bars,
            (true, true) => ports..ports + self.port_bars + self.title_bars,
        }
    }
}

/// Colour of a clock mark: the module's accent at reduced strength, mixed
/// halfway towards the dim text colour, so it reads on the tile without
/// competing with the port marker (panels.md, Clock mark on transport inputs).
fn clock_mark_colour(accent: Color, palette: &Palette) -> Color {
    let mix = |a: f32, b: f32| a + (b - a) * 0.5;
    Color::from_rgba(
        mix(accent.r, palette.text_dim.r),
        mix(accent.g, palette.text_dim.g),
        mix(accent.b, palette.text_dim.b),
        0.8,
    )
}

/// Adds the clock mark of each input of `module` that follows the
/// transport: a ring with two short hands to `marks`, and the plain dot
/// that stands in for it at middle zoom to `dots`.
fn clock_marks(module: &ModuleView, palette: &Palette, marks: &mut Vec<Shape>, dots: &mut Vec<Shape>) {
    let colour = clock_mark_colour(module.accent, palette);
    let r = CLOCK_MARK_RADIUS;
    for index in 0..module.inputs.len() {
        let Some(c) = module.clock_mark_centre(index) else {
            continue;
        };
        let ring = Rectangle::new(Point::new(c.x - r, c.y - r), iced::Size::new(2.0 * r, 2.0 * r));
        marks.push(Shape::new(ring, r, Color::TRANSPARENT, colour, 1.0));
        // Hands from the centre: a long one up, a short one to the right.
        let hand = 0.9;
        let up = Rectangle::new(
            Point::new(c.x - hand * 0.5, c.y - r * 0.7),
            iced::Size::new(hand, r * 0.7 + hand * 0.5),
        );
        let right = Rectangle::new(
            Point::new(c.x - hand * 0.5, c.y - hand * 0.5),
            iced::Size::new(r * 0.5 + hand * 0.5, hand),
        );
        marks.push(Shape::new(up, hand * 0.5, colour, Color::TRANSPARENT, 0.0));
        marks.push(Shape::new(right, hand * 0.5, colour, Color::TRANSPARENT, 0.0));
        let d = r * 0.6;
        let dot = Rectangle::new(Point::new(c.x - d, c.y - d), iced::Size::new(2.0 * d, 2.0 * d));
        dots.push(Shape::new(dot, d, colour, Color::TRANSPARENT, 0.0));
    }
}

/// Things drawn on top of the scene that change often: selection, hover,
/// the cable being dragged, the selection box.
#[derive(Debug, Default, Clone)]
pub struct Overlay {
    /// Cables in front of the tiles: those of selected and hovered modules,
    /// and the one being dragged.
    pub cables: Vec<Cable>,
    /// Shapes (outlines, highlights).
    pub shapes: Vec<Shape>,
}

/// What one frame of the canvas draws.
#[derive(Debug)]
pub struct CanvasPrimitive {
    pub(crate) geometry: std::sync::Arc<Geometry>,
    /// Panel controls. `None` when zoomed out too far to draw them.
    pub(crate) controls: Option<std::sync::Arc<ControlGeometry>>,
    /// Scope traces. `None` when zoomed out too far to draw them.
    pub(crate) traces: Option<std::sync::Arc<TraceGeometry>>,
    pub(crate) overlay: Overlay,
    /// The range of `geometry.shapes` that holds the text bars to draw.
    pub(crate) bars: std::ops::Range<u32>,
    pub(crate) camera: Camera,
    /// Opacity of cables that are not in focus.
    pub(crate) cable_dim: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    origin: [f32; 2],
    size: [f32; 2],
    zoom: f32,
    scale: f32,
    cable_dim: f32,
    pad: f32,
}

/// A vertex buffer that grows as needed and is never shrunk.
struct InstanceBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
    len: u32,
    label: &'static str,
}

impl InstanceBuffer {
    fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self {
            buffer: Self::create(device, label, 4096),
            capacity: 4096,
            len: 0,
            label,
        }
    }

    fn create(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn upload<T: Pod>(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, items: &[T]) {
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

    fn slice(&self) -> wgpu::BufferSlice<'_> {
        self.buffer.slice(..)
    }
}

/// GPU state shared by all canvas primitives.
///
/// Note: there is one set of buffers, so the app shows one canvas at a time.
/// A second canvas needs per-widget buffers.
pub struct CanvasPipeline {
    shapes: wgpu::RenderPipeline,
    cables: wgpu::RenderPipeline,
    arcs: wgpu::RenderPipeline,
    traces: wgpu::RenderPipeline,
    grid: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    scene_cables: InstanceBuffer,
    scene_shapes: InstanceBuffer,
    overlay_cables: InstanceBuffer,
    overlay_shapes: InstanceBuffer,
    control_arcs: InstanceBuffer,
    control_shapes: InstanceBuffer,
    /// Scope trace pieces: the one buffer a moving scope uploads.
    trace_segments: InstanceBuffer,
    uploaded: u64,
    controls_uploaded: (u64, u64),
    traces_uploaded: (u64, u64, bool),
    /// The overlay last uploaded, as bytes: cables, then shapes. A frame
    /// whose overlay is the same uploads nothing.
    overlay_uploaded: (Vec<u8>, Vec<u8>),
    tiles: u32,
    markers: u32,
}

const SHAPE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

impl Pipeline for CanvasPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("loom-canvas shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("canvas.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("loom-canvas globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("loom-canvas globals layout"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("loom-canvas globals"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("loom-canvas pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let make = |label: &str, vs: &str, fs: &str, buffers: &[wgpu::VertexBufferLayout<'_>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    buffers,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
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
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
        };

        let instance_layout = |stride: usize| wgpu::VertexBufferLayout {
            array_stride: stride as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &SHAPE_ATTRIBUTES,
        };

        Self {
            shapes: make(
                "loom-canvas shapes",
                "vs_shape",
                "fs_shape",
                &[instance_layout(std::mem::size_of::<Shape>())],
            ),
            cables: make(
                "loom-canvas cables",
                "vs_cable",
                "fs_cable",
                &[instance_layout(std::mem::size_of::<Cable>())],
            ),
            arcs: make(
                "loom-canvas arcs",
                "vs_arc",
                "fs_arc",
                &[instance_layout(std::mem::size_of::<Arc>())],
            ),
            traces: make(
                "loom-canvas traces",
                "vs_trace",
                "fs_trace",
                &[instance_layout(std::mem::size_of::<Segment>())],
            ),
            grid: make("loom-canvas grid", "vs_grid", "fs_grid", &[]),
            globals,
            bind_group,
            scene_cables: InstanceBuffer::new(device, "loom-canvas scene cables"),
            scene_shapes: InstanceBuffer::new(device, "loom-canvas scene shapes"),
            overlay_cables: InstanceBuffer::new(device, "loom-canvas overlay cables"),
            overlay_shapes: InstanceBuffer::new(device, "loom-canvas overlay shapes"),
            control_arcs: InstanceBuffer::new(device, "loom-canvas control arcs"),
            control_shapes: InstanceBuffer::new(device, "loom-canvas control shapes"),
            trace_segments: InstanceBuffer::new(device, "loom-canvas trace segments"),
            uploaded: 0,
            controls_uploaded: (0, 0),
            traces_uploaded: (0, 0, false),
            overlay_uploaded: (Vec::new(), Vec::new()),
            tiles: 0,
            markers: 0,
        }
    }
}

/// Vertices per cable strip. Must match `SEGMENTS` in the shader.
const CABLE_VERTICES: u32 = (crate::scene::CABLE_SEGMENTS + 1) * 2;

impl Primitive for CanvasPrimitive {
    type Pipeline = CanvasPipeline;

    fn prepare(
        &self,
        pipeline: &mut CanvasPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let globals = Globals {
            origin: [self.camera.origin.x, self.camera.origin.y],
            size: [bounds.width.max(1.0), bounds.height.max(1.0)],
            zoom: self.camera.zoom,
            scale: viewport.scale_factor(),
            cable_dim: self.cable_dim,
            pad: 0.0,
        };
        queue.write_buffer(&pipeline.globals, 0, bytemuck::bytes_of(&globals));
        stats::frame();

        if pipeline.uploaded != self.geometry.generation {
            pipeline.scene_cables.upload(device, queue, &self.geometry.cables);
            pipeline.scene_shapes.upload(device, queue, &self.geometry.shapes);
            stats::count(Counter::SceneUploads);
            pipeline.uploaded = self.geometry.generation;
            pipeline.tiles = self.geometry.tiles;
            pipeline.markers = self.geometry.markers();
        }
        // Control values have their own buffers: a knob turn uploads only
        // these, never the tiles, ports, or cables.
        if let Some(controls) = &self.controls
            && pipeline.controls_uploaded != controls.key
        {
            pipeline.control_arcs.upload(device, queue, &controls.arcs);
            pipeline.control_shapes.upload(device, queue, &controls.shapes);
            stats::count(Counter::ControlUploads);
            pipeline.controls_uploaded = controls.key;
        }
        // A new trace uploads its own buffer and nothing else.
        if let Some(traces) = &self.traces
            && pipeline.traces_uploaded != traces.key
        {
            pipeline.trace_segments.upload(device, queue, &traces.segments);
            stats::count(Counter::TraceUploads);
            pipeline.traces_uploaded = traces.key;
        }
        // The overlay is built every frame, but often does not change: a
        // live dot moving under still modulation lines changes only the
        // controls.
        let cables: &[u8] = bytemuck::cast_slice(&self.overlay.cables);
        let shapes: &[u8] = bytemuck::cast_slice(&self.overlay.shapes);
        if pipeline.overlay_uploaded.0 != cables || pipeline.overlay_uploaded.1 != shapes {
            stats::count(Counter::OverlayUploads);
        }
        if pipeline.overlay_uploaded.0 != cables {
            pipeline.overlay_cables.upload(device, queue, &self.overlay.cables);
            pipeline.overlay_uploaded.0 = cables.to_vec();
        }
        if pipeline.overlay_uploaded.1 != shapes {
            pipeline.overlay_shapes.upload(device, queue, &self.overlay.shapes);
            pipeline.overlay_uploaded.1 = shapes.to_vec();
        }
    }

    fn draw(&self, pipeline: &CanvasPipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pass.set_bind_group(0, &pipeline.bind_group, &[]);

        pass.set_pipeline(&pipeline.grid);
        pass.draw(0..4, 0..1);

        // Cables behind the tiles, so modules read as solid objects. Then
        // text bars on the tiles, then the focused cables in front, then
        // ports over every cable end, then panel controls, then scope
        // traces in their frames, then the selection, hover, and focus
        // outlines.
        let tiles = pipeline.tiles.min(pipeline.scene_shapes.len);
        let markers = pipeline.markers.min(pipeline.scene_shapes.len);
        let bars = self.bars.start.min(markers)..self.bars.end.min(markers);
        let (arcs, controls) = if self.controls.is_some() {
            (pipeline.control_arcs.len, pipeline.control_shapes.len)
        } else {
            (0, 0)
        };
        let traces = if self.traces.is_some() {
            pipeline.trace_segments.len
        } else {
            0
        };
        let layers = [
            (
                &pipeline.cables,
                &pipeline.scene_cables,
                CABLE_VERTICES,
                0..pipeline.scene_cables.len,
            ),
            (&pipeline.shapes, &pipeline.scene_shapes, 4, 0..tiles),
            (&pipeline.shapes, &pipeline.scene_shapes, 4, bars),
            (
                &pipeline.cables,
                &pipeline.overlay_cables,
                CABLE_VERTICES,
                0..pipeline.overlay_cables.len,
            ),
            (
                &pipeline.shapes,
                &pipeline.scene_shapes,
                4,
                markers..pipeline.scene_shapes.len,
            ),
            (&pipeline.arcs, &pipeline.control_arcs, 4, 0..arcs),
            (&pipeline.shapes, &pipeline.control_shapes, 4, 0..controls),
            (&pipeline.traces, &pipeline.trace_segments, 4, 0..traces),
            (
                &pipeline.shapes,
                &pipeline.overlay_shapes,
                4,
                0..pipeline.overlay_shapes.len,
            ),
        ];
        for (render, buffer, vertices, instances) in layers {
            if !instances.is_empty() {
                pass.set_pipeline(render);
                pass.set_vertex_buffer(0, buffer.slice());
                pass.draw(0..vertices, instances);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{CableView, ModuleView, PortView};

    fn port() -> PortView {
        PortView {
            label: "p".into(),
            hover: crate::scene::HoverText::default(),
            colour: Color::WHITE,
            shape: PortShape::Round,
            badge: 0,
            follows_transport: None,
        }
    }

    const SHADER: &str = include_str!("canvas.wgsl");

    #[test]
    fn shader_is_valid() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parses");
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .expect("WGSL validates");
        // Translate to Metal Shading Language, as wgpu does on macOS.
        let options = naga::back::msl::Options::default();
        let pipeline = naga::back::msl::PipelineOptions::default();
        naga::back::msl::write_string(&module, &info, &options, &pipeline).expect("translates to Metal");
    }

    /// wgpu passes some WGSL names straight through to Metal and HLSL, where
    /// they are reserved. `half` (a Metal type) crashed the app on macOS.
    #[test]
    fn shader_names() {
        const RESERVED: &[&str] = &[
            "half", "half2", "half3", "half4", "in", "out", "inout", "vector", "matrix", "sample", "texture",
            "sampler", "kernel", "vertex", "fragment", "device", "constant", "thread", "packed",
        ];
        for (number, line) in SHADER.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            // `@vertex` and `@fragment` are WGSL attributes, not names.
            let words = code
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '@'))
                .filter(|w| !w.starts_with('@'));
            for word in words {
                assert!(
                    !RESERVED.contains(&word),
                    "canvas.wgsl line {}: `{word}` is reserved in Metal or HLSL",
                    number + 1
                );
            }
        }
    }

    #[test]
    fn instance_sizes_match_the_shader() {
        // Four vec4 attributes each.
        assert_eq!(std::mem::size_of::<Shape>(), 64);
        assert_eq!(std::mem::size_of::<Cable>(), 64);
        assert_eq!(std::mem::size_of::<Arc>(), 64);
        assert_eq!(std::mem::size_of::<Segment>(), 64);
        assert_eq!(std::mem::size_of::<Globals>(), 32);
    }

    #[test]
    fn builds_one_instance_per_item() {
        let module = |id, x| ModuleView {
            id,
            title: String::new(),
            accent: Color::BLACK,
            position: Point::new(x, 0.0),
            inputs: vec![port(), port()],
            outputs: vec![port()],
            panel: crate::scene::PanelView::default(),
        };
        let scene = Scene::new(
            vec![module(1, 0.0), module(2, 200.0)],
            vec![
                CableView {
                    from: (1, 0),
                    to: (2, 1),
                    colour_from: Color::WHITE,
                    colour_to: Color::WHITE,
                    delayed: false,
                },
                // Unknown port: skipped.
                CableView {
                    from: (1, 5),
                    to: (2, 0),
                    colour_from: Color::WHITE,
                    colour_to: Color::WHITE,
                    delayed: false,
                },
            ],
        );
        let geometry = Geometry::build(&scene, &Palette::default());
        assert_eq!(geometry.cables.len(), 1);
        // Three shapes per tile, one bar per port label (the titles are
        // empty, so they have none), and one marker per port.
        assert_eq!(geometry.shapes.len(), 2 * 3 + 6 + 6);
        assert_eq!(geometry.tiles, 2 * 3);
        assert_eq!(
            (geometry.caption_bars, geometry.port_bars, geometry.title_bars),
            (0, 6, 0)
        );
        assert_eq!(geometry.markers(), 2 * 3 + 6);
        assert_eq!(geometry.generation, scene.generation());
    }

    /// An output that drives assignments gets one badge pill, drawn with
    /// the markers over the cables. Its count is text.
    #[test]
    fn a_badge_is_one_shape_with_the_markers() {
        let plain = ModuleView {
            id: 1,
            title: String::new(),
            accent: Color::BLACK,
            position: Point::ORIGIN,
            inputs: vec![],
            outputs: vec![port(), port()],
            panel: Default::default(),
        };
        let mut badged = plain.clone();
        badged.outputs[1].badge = 3;
        let shapes = |m: ModuleView| Geometry::build(&Scene::new(vec![m], vec![]), &Palette::default());
        let (before, after) = (shapes(plain), shapes(badged));
        assert_eq!(after.shapes.len(), before.shapes.len() + 1);
        assert_eq!(after.markers(), before.markers());
    }

    #[test]
    fn each_zoom_draws_the_bars_for_the_text_it_hides() {
        let module = ModuleView {
            id: 1,
            title: "Saw".into(),
            accent: Color::BLACK,
            position: Point::ORIGIN,
            inputs: vec![port()],
            outputs: vec![port()],
            panel: crate::scene::PanelView {
                width: 3,
                height: 3,
                widgets: vec![crate::scene::WidgetView {
                    kind: crate::scene::WidgetViewKind::Knob { steps: 0 },
                    x: 0,
                    y: 0,
                    label: "Gain".into(),
                    hover: crate::scene::HoverText::default(),
                }],
            },
        };
        let palette = Palette::default();
        let geometry = Geometry::build(&Scene::new(vec![module], Vec::new()), &palette);
        assert_eq!(
            (geometry.caption_bars, geometry.port_bars, geometry.title_bars),
            (1, 2, 1)
        );
        let colours = |range: std::ops::Range<u32>| -> Vec<[f32; 4]> {
            geometry.shapes[range.start as usize..range.end as usize]
                .iter()
                .map(|s| s.fill)
                .collect()
        };
        let label = pack(palette.text_bar).components();
        let title = pack(palette.title_bar).components();
        // Close up: all text is drawn, so no bars.
        assert!(geometry.bars(false, false).is_empty());
        // Middle: the caption and both port labels, not the title.
        assert_eq!(colours(geometry.bars(true, false)), vec![label; 3]);
        // Far: both port labels and the title. Controls are not drawn, so
        // neither is the caption's bar.
        assert_eq!(colours(geometry.bars(true, true)), vec![label, label, title]);
    }

    /// A transport input gets a clock mark (a ring and two hands) close up,
    /// a plain dot at middle zoom, and nothing far out. Ports keep their
    /// markers, and the text bars their ranges.
    #[test]
    fn a_clock_mark_shows_close_up_and_as_a_dot_at_middle_zoom() {
        let mut followed = port();
        followed.follows_transport = Some(crate::scene::TransportSignal::BarPhase);
        let module = |inputs: Vec<PortView>| ModuleView {
            id: 1,
            title: "LFO".into(),
            accent: Color::from_rgb8(0x5A, 0x4A, 0x82),
            position: Point::ORIGIN,
            inputs,
            outputs: vec![port()],
            panel: Default::default(),
        };
        let palette = Palette::default();
        let build = |m: ModuleView| Geometry::build(&Scene::new(vec![m], Vec::new()), &palette);
        let plain = build(module(vec![port(), port()]));
        let marked = build(module(vec![port(), followed]));
        assert_eq!((plain.clock_marks, plain.clock_dots), (0, 0));
        assert_eq!((marked.clock_marks, marked.clock_dots), (3, 1));
        assert_eq!(marked.shapes.len(), plain.shapes.len() + 4);
        // Close up: only the mark's three shapes, a ring first.
        let close = marked.bars(false, false);
        assert_eq!(close.len(), 3);
        let ring = &marked.shapes[close.start as usize];
        assert_eq!(ring.fill(), pack(Color::TRANSPARENT).components());
        assert!(ring.border_width() > 0.0);
        // Middle: the dot, then the same bars as without the mark.
        let middle = marked.bars(true, false);
        assert_eq!(middle.len(), plain.bars(true, false).len() + 1);
        // Far: no mark, just the bars.
        assert_eq!(marked.bars(true, true).len(), plain.bars(true, true).len());
        assert!(marked.bars(true, true).start > middle.start);
        // The mark sits between the port marker and the label.
        let [x, _, w, _] = ring.rect();
        assert!(x > PORT_RADIUS && x + w < 9.0 + crate::scene::CLOCK_MARK_SPACE);
    }
}
