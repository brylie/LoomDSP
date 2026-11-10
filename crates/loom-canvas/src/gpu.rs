//! GPU data and the custom wgpu primitive that draws the canvas.

use crate::camera::Camera;
use crate::controls::ControlGeometry;
use crate::scene::{CLOCK_MARK_RADIUS, ModuleView, PORT_RADIUS, PortShape, Scene};
use crate::stats::{self, Counter};
use crate::theme::Palette;
use crate::tile_text::{self, TextKind};
use crate::trace::{Segment, TraceGeometry};
use bytemuck::{Pod, Zeroable};
use iced::widget::shader::{Pipeline, Primitive, Viewport};
use loom_ui_core::{Color, Point, Rect, Size};

/// Corner radius of tiles, in world units.
const TILE_RADIUS: f32 = 5.0;
/// Cable width, in world units.
const CABLE_WIDTH: f32 = 2.4;
/// Thinnest a cable is drawn, in physical pixels.
const CABLE_MIN_PX: f32 = 1.2;

/// A colour as the shaders read it: red, green, blue, and alpha, each
/// unchanged. The channels stay sRGB-encoded, and the shaders blend in that
/// space, which is how the rest of the window blends its colours.
pub(crate) fn packed(colour: Color) -> [f32; 4] {
    [colour.r, colour.g, colour.b, colour.a]
}

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
    pub fn new(rect: Rect, radius: f32, fill: Color, border: Color, border_width: f32) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.width, rect.height],
            fill: packed(fill),
            border: packed(border),
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
        let rect = Rect::new(
            Point::new(centre.x - radius, centre.y - radius),
            Size::new(radius * 2.0, radius * 2.0),
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
            colour: packed(colour),
            unused: [0.0; 4],
        }
    }

    /// The sweep in radians.
    #[cfg(test)]
    pub fn sweep(&self) -> f32 {
        self.angles[1]
    }

    /// The colour, packed as the GPU reads it.
    #[cfg(test)]
    pub fn colour(&self) -> [f32; 4] {
        self.colour
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
            colour_from: packed(colour_from),
            colour_to: packed(colour_to),
            params: [CABLE_WIDTH, if dashed { 1.0 } else { 0.0 }, CABLE_MIN_PX, 0.0],
        }
    }

    /// Draws the cable at a third of the usual width: a modulation line.
    pub fn thin(mut self) -> Self {
        self.params[0] = CABLE_WIDTH / 3.0;
        self
    }

    /// Draws a disabled modulation line at half the normal modulation width.
    pub fn half_width(mut self) -> Self {
        self.params[0] *= 0.5;
        self.params[2] *= 0.5;
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
    /// Number of bars for the text in a control's box. They come after the
    /// other bars, because they draw over the box (see [`Geometry::value_bars`]).
    pub value_bars: u32,
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
            shapes.push(Shape::new(
                bounds,
                TILE_RADIUS,
                palette.tile,
                if module.unknown {
                    Color::TRANSPARENT
                } else {
                    palette.tile_border
                },
                1.0,
            ));
            if module.unknown {
                let dash: f32 = 6.0;
                let mut x = 0.0;
                while x < bounds.width {
                    let width = dash.min(bounds.width - x);
                    for y in [bounds.y, bounds.y + bounds.height - 1.0] {
                        shapes.push(Shape::new(
                            Rect::new(Point::new(bounds.x + x, y), Size::new(width, 1.0)),
                            0.0,
                            palette.text_dim,
                            Color::TRANSPARENT,
                            0.0,
                        ));
                    }
                    x += dash * 2.0;
                }
                let mut y = 0.0;
                while y < bounds.height {
                    let height = dash.min(bounds.height - y);
                    for x in [bounds.x, bounds.x + bounds.width - 1.0] {
                        shapes.push(Shape::new(
                            Rect::new(Point::new(x, bounds.y + y), Size::new(1.0, height)),
                            0.0,
                            palette.text_dim,
                            Color::TRANSPARENT,
                            0.0,
                        ));
                    }
                    y += dash * 2.0;
                }
            }
            // Header: a rounded strip, then a square strip over its lower
            // half so only the top corners are round.
            let header = Rect::new(module.position, Size::new(module.width(), crate::scene::HEADER));
            shapes.push(Shape::new(header, TILE_RADIUS, module.accent, Color::TRANSPARENT, 0.0));
            let lower = Rect::new(
                Point::new(header.x, header.y + header.height * 0.5),
                Size::new(header.width, header.height * 0.5),
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

        let (mut captions, mut labels, mut titles, mut values) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for module in scene.modules() {
            tile_text::for_each(module, |item| {
                let bar = item.bar();
                if bar.width <= 0.0 {
                    return;
                }
                let (list, colour) = match item.kind {
                    TextKind::Title => (&mut titles, palette.title_bar),
                    TextKind::Port(_) => (&mut labels, palette.text_bar),
                    TextKind::Value(_) => (&mut values, palette.text_bar),
                    TextKind::Caption(_) | TextKind::Option(..) | TextKind::Unknown => {
                        (&mut captions, palette.text_bar)
                    }
                };
                list.push(Shape::new(bar, bar.height * 0.5, colour, Color::TRANSPARENT, 0.0));
            });
        }
        let (caption_bars, port_bars, title_bars) = (captions.len() as u32, labels.len() as u32, titles.len() as u32);
        let value_bars = values.len() as u32;
        shapes.append(&mut captions);
        shapes.append(&mut labels);
        shapes.append(&mut titles);
        shapes.append(&mut values);
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
            value_bars,
        }
    }

    /// Index of the first port marker in `shapes`.
    pub fn markers(&self) -> u32 {
        self.tiles
            + self.clock_marks
            + self.clock_dots
            + self.caption_bars
            + self.port_bars
            + self.title_bars
            + self.value_bars
    }

    /// The bars for text in a control's box, or nothing: they show only at
    /// middle zoom, and draw after the controls, so the box does not hide them.
    pub fn value_bars(&self, middle: bool) -> std::ops::Range<u32> {
        let end = self.markers();
        let start = if middle { end - self.value_bars } else { end };
        start..end
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
    Color::rgba(
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
        let ring = Rect::new(Point::new(c.x - r, c.y - r), Size::new(2.0 * r, 2.0 * r));
        marks.push(Shape::new(ring, r, Color::TRANSPARENT, colour, 1.0));
        // Hands from the centre: a long one up, a short one to the right.
        let hand = 0.9;
        let up = Rect::new(
            Point::new(c.x - hand * 0.5, c.y - r * 0.7),
            Size::new(hand, r * 0.7 + hand * 0.5),
        );
        let right = Rect::new(
            Point::new(c.x - hand * 0.5, c.y - hand * 0.5),
            Size::new(r * 0.5 + hand * 0.5, hand),
        );
        marks.push(Shape::new(up, hand * 0.5, colour, Color::TRANSPARENT, 0.0));
        marks.push(Shape::new(right, hand * 0.5, colour, Color::TRANSPARENT, 0.0));
        let d = r * 0.6;
        let dot = Rect::new(Point::new(c.x - d, c.y - d), Size::new(2.0 * d, 2.0 * d));
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

/// Where the canvas draws: the size of its viewport and how dense the
/// pixels are. The host that owns the window gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// The viewport's size in logical pixels.
    pub size: Size,
    /// Physical pixels per logical pixel.
    pub scale: f32,
}

/// What one frame of the canvas draws: plain data, built by the canvas and
/// given to [`CanvasRenderer::prepare`].
#[derive(Debug)]
pub struct CanvasFrame {
    pub(crate) geometry: std::sync::Arc<Geometry>,
    /// Panel controls. `None` when zoomed out too far to draw them.
    pub(crate) controls: Option<std::sync::Arc<ControlGeometry>>,
    /// Scope traces. `None` when zoomed out too far to draw them.
    pub(crate) traces: Option<std::sync::Arc<TraceGeometry>>,
    pub(crate) overlay: Overlay,
    /// The range of `geometry.shapes` that holds the text bars to draw.
    pub(crate) bars: std::ops::Range<u32>,
    /// The range of `geometry.shapes` that holds the bars drawn over boxes.
    pub(crate) value_bars: std::ops::Range<u32>,
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

/// What [`CanvasRenderer::draw`] draws, kept from the frame last prepared.
#[derive(Debug, Default)]
struct DrawPlan {
    /// The range of scene shapes that holds the text bars to draw.
    bars: std::ops::Range<u32>,
    /// The range of scene shapes that holds the bars drawn over boxes.
    value_bars: std::ops::Range<u32>,
    /// Whether panel controls are drawn at this zoom.
    controls: bool,
    /// Whether scope traces are drawn at this zoom.
    traces: bool,
}

/// Draws the patch canvas with `wgpu`: the pipelines for `canvas.wgsl` and
/// the instance buffers they read.
///
/// It names no UI toolkit. A host gives it a device, a queue, and a render
/// pass whose viewport is the canvas's place, and calls
/// [`prepare`](Self::prepare) then [`draw`](Self::draw) for each frame it
/// draws. One renderer owns its buffers, so two canvases need two
/// renderers.
///
/// It assumes a colour target with one sample per pixel, and blends with
/// premultiplied alpha. A host that draws into a multisampled pass would
/// need a sample count here; nothing asks for one yet.
pub struct CanvasRenderer {
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
    /// Result preview curves, uploaded with the controls.
    control_lines: InstanceBuffer,
    /// Scope trace pieces: the one buffer a moving scope uploads.
    trace_segments: InstanceBuffer,
    /// Filled shapes under the traces: the other buffer a moving display
    /// uploads, never the controls.
    trace_fills: InstanceBuffer,
    uploaded: u64,
    controls_uploaded: (u64, u64),
    traces_uploaded: (u64, u64, bool),
    /// The overlay last uploaded, as bytes: cables, then shapes. A frame
    /// whose overlay is the same uploads nothing.
    overlay_uploaded: (Vec<u8>, Vec<u8>),
    tiles: u32,
    markers: u32,
    plan: DrawPlan,
}

const SHAPE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

impl CanvasRenderer {
    /// Builds the pipelines and empty buffers for a colour target of
    /// `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
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
            control_lines: InstanceBuffer::new(device, "loom-canvas control lines"),
            trace_segments: InstanceBuffer::new(device, "loom-canvas trace segments"),
            trace_fills: InstanceBuffer::new(device, "loom-canvas trace fills"),
            uploaded: 0,
            controls_uploaded: (0, 0),
            traces_uploaded: (0, 0, false),
            overlay_uploaded: (Vec::new(), Vec::new()),
            tiles: 0,
            markers: 0,
            plan: DrawPlan::default(),
        }
    }
}

/// Vertices per cable strip. Must match `SEGMENTS` in the shader.
const CABLE_VERTICES: u32 = (crate::scene::CABLE_SEGMENTS + 1) * 2;

impl CanvasRenderer {
    /// Uploads what `frame` changed since the last frame, and remembers
    /// what [`draw`](Self::draw) is to draw. Call it once per drawn frame,
    /// before the pass begins.
    ///
    /// Each kind of data uploads only when its own key changes: the scene
    /// by its generation, the controls and the traces by theirs, and the
    /// overlay when its bytes differ. Pan and zoom write only the globals
    /// (ADR-0010).
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &CanvasFrame, target: Target) {
        let globals = Globals {
            origin: [frame.camera.origin.x, frame.camera.origin.y],
            size: [target.size.width.max(1.0), target.size.height.max(1.0)],
            zoom: frame.camera.zoom,
            scale: target.scale,
            cable_dim: frame.cable_dim,
            pad: 0.0,
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        stats::frame();

        if self.uploaded != frame.geometry.generation {
            self.scene_cables.upload(device, queue, &frame.geometry.cables);
            self.scene_shapes.upload(device, queue, &frame.geometry.shapes);
            stats::count(Counter::SceneUploads);
            self.uploaded = frame.geometry.generation;
            self.tiles = frame.geometry.tiles;
            self.markers = frame.geometry.markers();
        }
        // Control values have their own buffers: a knob turn uploads only
        // these, never the tiles, ports, or cables.
        if let Some(controls) = &frame.controls
            && self.controls_uploaded != controls.key
        {
            self.control_arcs.upload(device, queue, &controls.arcs);
            self.control_shapes.upload(device, queue, &controls.shapes);
            self.control_lines.upload(device, queue, &controls.lines);
            stats::count(Counter::ControlUploads);
            self.controls_uploaded = controls.key;
        }
        // A new trace uploads its own buffer and nothing else.
        if let Some(traces) = &frame.traces
            && self.traces_uploaded != traces.key
        {
            self.trace_segments.upload(device, queue, &traces.segments);
            self.trace_fills.upload(device, queue, &traces.fills);
            stats::count(Counter::TraceUploads);
            self.traces_uploaded = traces.key;
        }
        // The overlay is built every frame, but often does not change: a
        // live dot moving under still modulation lines changes only the
        // controls.
        let cables: &[u8] = bytemuck::cast_slice(&frame.overlay.cables);
        let shapes: &[u8] = bytemuck::cast_slice(&frame.overlay.shapes);
        if self.overlay_uploaded.0 != cables || self.overlay_uploaded.1 != shapes {
            stats::count(Counter::OverlayUploads);
        }
        if self.overlay_uploaded.0 != cables {
            self.overlay_cables.upload(device, queue, &frame.overlay.cables);
            self.overlay_uploaded.0 = cables.to_vec();
        }
        if self.overlay_uploaded.1 != shapes {
            self.overlay_shapes.upload(device, queue, &frame.overlay.shapes);
            self.overlay_uploaded.1 = shapes.to_vec();
        }
        self.plan = DrawPlan {
            bars: frame.bars.clone(),
            value_bars: frame.value_bars.clone(),
            controls: frame.controls.is_some(),
            traces: frame.traces.is_some(),
        };
    }

    /// Draws the frame last prepared into `pass`.
    ///
    /// The host sets the pass's viewport and scissor to the canvas's place
    /// in its target before the call. The renderer never sets them, never
    /// begins or ends a pass, and never presents: it does not own the
    /// window, the surface, or the frame.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let plan = &self.plan;
        pass.set_bind_group(0, &self.bind_group, &[]);

        pass.set_pipeline(&self.grid);
        pass.draw(0..4, 0..1);

        // Cables behind the tiles, so modules read as solid objects. Then
        // text bars on the tiles, then the focused cables in front, then
        // ports over every cable end, then panel controls and the preview
        // curves in their frames, then scope traces in theirs, then the
        // selection, hover, and focus outlines.
        let tiles = self.tiles.min(self.scene_shapes.len);
        let markers = self.markers.min(self.scene_shapes.len);
        let bars = plan.bars.start.min(markers)..plan.bars.end.min(markers);
        let value_bars = plan.value_bars.start.min(markers)..plan.value_bars.end.min(markers);
        let (arcs, controls, lines) = if plan.controls {
            (self.control_arcs.len, self.control_shapes.len, self.control_lines.len)
        } else {
            (0, 0, 0)
        };
        let (traces, fills) = if plan.traces {
            (self.trace_segments.len, self.trace_fills.len)
        } else {
            (0, 0)
        };
        let layers = [
            (
                &self.cables,
                &self.scene_cables,
                CABLE_VERTICES,
                0..self.scene_cables.len,
            ),
            (&self.shapes, &self.scene_shapes, 4, 0..tiles),
            (&self.shapes, &self.scene_shapes, 4, bars),
            (
                &self.cables,
                &self.overlay_cables,
                CABLE_VERTICES,
                0..self.overlay_cables.len,
            ),
            (&self.shapes, &self.scene_shapes, 4, markers..self.scene_shapes.len),
            (&self.arcs, &self.control_arcs, 4, 0..arcs),
            (&self.shapes, &self.control_shapes, 4, 0..controls),
            (&self.shapes, &self.scene_shapes, 4, value_bars),
            (&self.traces, &self.control_lines, 4, 0..lines),
            (&self.shapes, &self.trace_fills, 4, 0..fills),
            (&self.traces, &self.trace_segments, 4, 0..traces),
            (&self.shapes, &self.overlay_shapes, 4, 0..self.overlay_shapes.len),
        ];
        for (render, buffer, vertices, instances) in layers {
            if !instances.is_empty() {
                pass.set_pipeline(render);
                pass.set_vertex_buffer(0, buffer.slice());
                pass.draw(0..vertices, instances);
            }
        }
    }

    /// Uploads one cable in the target's own coordinates, for a renderer
    /// that draws nothing else: the cable dragged across the window. `zoom`
    /// is the canvas's zoom, so the cable is as thick as on the canvas.
    pub fn prepare_cable(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cable: Cable,
        zoom: f32,
        target: Target,
    ) {
        let globals = Globals {
            origin: [0.0, 0.0],
            size: [target.size.width.max(1.0), target.size.height.max(1.0)],
            zoom,
            scale: target.scale,
            cable_dim: 1.0,
            pad: 0.0,
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let bytes = bytemuck::bytes_of(&cable);
        if self.overlay_uploaded.0 != bytes {
            self.overlay_cables.upload(device, queue, &[cable]);
            self.overlay_uploaded.0 = bytes.to_vec();
        }
    }

    /// Draws the cable last given to [`prepare_cable`](Self::prepare_cable)
    /// into `pass`, under the same rules as [`draw`](Self::draw).
    pub fn draw_cable(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.cables);
        pass.set_vertex_buffer(0, self.overlay_cables.slice());
        pass.draw(0..CABLE_VERTICES, 0..1);
    }
}

/// The canvas frame as an Iced shader primitive.
#[derive(Debug)]
pub struct CanvasPrimitive(pub(crate) CanvasFrame);

/// The canvas renderer as Iced's shared state for [`CanvasPrimitive`].
///
/// Note: Iced keeps one of these for the primitive type, so the app shows
/// one canvas at a time.
pub struct CanvasPipeline(CanvasRenderer);

impl Pipeline for CanvasPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self(CanvasRenderer::new(device, format))
    }
}

impl Primitive for CanvasPrimitive {
    type Pipeline = CanvasPipeline;

    fn prepare(
        &self,
        pipeline: &mut CanvasPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &iced::Rectangle,
        viewport: &Viewport,
    ) {
        let target = Target {
            size: Size::new(bounds.width, bounds.height),
            scale: viewport.scale_factor(),
        };
        pipeline.0.prepare(device, queue, &self.0, target);
    }

    fn draw(&self, pipeline: &CanvasPipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.0.draw(pass);
        true
    }
}

/// The window-wide output drag as an Iced shader primitive. Its renderer is
/// separate from the patch canvas's, so preparing two primitives cannot
/// overwrite either one.
#[derive(Debug, Clone, Copy)]
pub struct DragPrimitive {
    pub cable: Cable,
    pub zoom: f32,
}

/// A second canvas renderer, for the window-wide drag.
pub struct DragPipeline(CanvasRenderer);

impl Pipeline for DragPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self(CanvasRenderer::new(device, format))
    }
}

impl Primitive for DragPrimitive {
    type Pipeline = DragPipeline;

    fn prepare(
        &self,
        pipeline: &mut DragPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &iced::Rectangle,
        viewport: &Viewport,
    ) {
        let target = Target {
            size: Size::new(bounds.width, bounds.height),
            scale: viewport.scale_factor(),
        };
        pipeline.0.prepare_cable(device, queue, self.cable, self.zoom, target);
    }

    fn draw(&self, pipeline: &DragPipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.0.draw_cable(pass);
        true
    }
}

#[cfg(test)]
mod tests;
