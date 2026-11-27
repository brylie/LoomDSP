//! Tests for a custom element's GPU half: where its drawing goes in the
//! frame, that the host sets back the viewport and scissor after it, and
//! when it is told its GPU data is lost (ui-layer.md, section 9, rule 4).
//! The drawing test reads pixels back; on a machine with no GPU adapter
//! it skips, unless `LOOM_REQUIRE_GPU` is `1`.

use crate::app::{App, Context, WindowSpec};
use crate::custom::{Custom, CustomGpu, CustomKind, FramePlan, Gpu, GpuFrame, Pass};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::WindowState;
use crate::headless::Harness;
use crate::paint::{ListPainter, PaintList, PaintRenderer, Target, Typesetter};
use crate::routing::NoTarget;
use crate::view::{Layer, View};
use loom_ui_core::{Color, Point, Rect, Size, UiId};
use std::cell::RefCell;
use std::time::Instant;

/// The pad's kind.
const PAD: CustomKind = CustomKind::new("pad");

/// The target's width in pixels, at scale 1: 512 bytes a row, as a
/// read-back needs a multiple of 256.
const WIDTH: u32 = 128;

/// The target's height.
const HEIGHT: u32 = 80;

/// The format drawn into.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A pipeline that fills its whole viewport, inside the scissor, with
/// `colour`: one triangle over it.
fn fill(device: &wgpu::Device, format: wgpu::TextureFormat, [red, green, blue]: [f32; 3]) -> wgpu::RenderPipeline {
    let source = format!(
        "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
            let x = f32(i32(i & 1u) * 4 - 1);
            let y = f32(i32(i >> 1u) * 4 - 1);
            return vec4<f32>(x, y, 0.0, 1.0);
        }}
        @fragment fn fs() -> @location(0) vec4<f32> {{ return vec4<f32>({red:?}, {green:?}, {blue:?}, 1.0); }}"
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fill"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fill"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &[Some(format.into())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// The pad's state: its pipeline, built lazily for each device, and what
/// it was told.
#[derive(Default)]
struct PadState {
    pipeline: Option<wgpu::RenderPipeline>,
    /// Its scissor, narrowed to the left three quarters of the host's.
    narrow: [u32; 4],
    prepares: u32,
    lost: u32,
    /// Input state, which a lost device must leave alone.
    dragged: u32,
    /// The viewport it was last given to prepare its drawing above a
    /// layer with.
    above_viewport: Option<[f32; 4]>,
}

impl Custom<Pads> for PadState {
    fn gpu(&self) -> Option<&dyn CustomGpu<Pads>> {
        Some(self)
    }

    fn gpu_mut(&mut self) -> Option<&mut dyn CustomGpu<Pads>> {
        Some(self)
    }
}

impl CustomGpu<Pads> for PadState {
    fn prepare(&mut self, app: &Pads, gpu: &Gpu<'_>) {
        self.prepares += 1;
        // A pad that cannot build its pipeline prepares nothing.
        if app.broken {
            return;
        }
        let [x, y, width, height] = gpu.scissor;
        self.narrow = [x, y, width * 3 / 4, height];
        if self.pipeline.is_none() {
            self.pipeline = Some(fill(gpu.device, gpu.format, [0.0, 1.0, 0.0]));
        }
    }

    fn draw(&self, pass: &mut Pass<'_>) {
        let Some(pipeline) = &self.pipeline else { return };
        // It narrows its scissor, which the host must set back after.
        let [x, y, width, height] = self.narrow;
        pass.set_scissor_rect(x, y, width, height);
        pass.set_pipeline(pipeline);
        pass.draw(0..3, 0..1);
    }

    fn above(&self, app: &Pads) -> Option<Layer> {
        app.above.then_some(Layer::Panels)
    }

    fn prepare_above(&mut self, _app: &Pads, gpu: &Gpu<'_>) {
        self.above_viewport = Some(gpu.viewport);
    }

    fn draw_above(&self, pass: &mut Pass<'_>) {
        let Some(pipeline) = &self.pipeline else { return };
        // It narrows the scissor to a corner, which the host must set back
        // before it draws on in the same pass.
        pass.set_scissor_rect(0, 0, 4, 4);
        pass.set_pipeline(pipeline);
        pass.draw(0..3, 0..1);
    }

    fn gpu_lost(&mut self) {
        self.lost += 1;
        self.pipeline = None;
    }
}

/// The panel's background.
const RED: Color = Color::rgb(1.0, 0.0, 0.0);

/// The sibling painted after the pad, over it and below it.
const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

/// An app with a 60 by 40 pad on a red panel, and a blue box painted after
/// it, 30 by 60, over its left part and reaching below it.
struct Pads {
    /// True when the pad prepares nothing, as one whose pipeline failed.
    broken: bool,
    /// True when the pad draws above the Panels layer too.
    above: bool,
}

impl App for Pads {
    type Message = ();

    fn update(&mut self, _message: (), _cx: &Context<'_, ()>) -> Effects<()> {
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("main"), Layer::Panels, &(), |_| {
            Element::stack([
                Element::custom(PAD)
                    .key("pad")
                    .width(Length::Fixed(60.0))
                    .height(Length::Fixed(40.0)),
                Element::space(Length::Fixed(30.0), Length::Fixed(60.0)).background(BLUE),
            ])
            .width(Length::FILL)
            .height(Length::FILL)
            .background(RED)
        });
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == PAD).then(|| Box::new(PadState::default()) as Box<dyn Custom<Self>>)
    }
}

/// Runs `future` on this thread until it is done.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    /// Wakes the waiting thread.
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Unpark(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::park();
    }
}

/// A device and queue, or `None` on a machine with no GPU adapter.
fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// True when `LOOM_REQUIRE_GPU` is `1`, so a missing adapter fails.
fn gpu_required() -> bool {
    std::env::var("LOOM_REQUIRE_GPU").is_ok_and(|value| value == "1")
}

/// The whole window as a paint target.
fn target(height: u32) -> Target {
    Target {
        size: Size::new(WIDTH as f32, height as f32),
        scale: 1.0,
        origin: Point::ORIGIN,
        physical: (WIDTH, height),
    }
}

/// The pad app, laid out.
fn harness(text: &RefCell<Typesetter>, app: Pads) -> Harness<Pads, NoTarget> {
    let window = WindowState::new(Size::new(WIDTH as f32, HEIGHT as f32));
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.lay_out(&mut *text.borrow_mut());
    harness
}

/// Paints the Panels layer into a list, and gives it with its plan.
fn paint(harness: &mut Harness<Pads, NoTarget>, text: &RefCell<Typesetter>, height: u32) -> (PaintList, FramePlan) {
    let mut list = PaintList::new(Rect::new(Point::ORIGIN, Size::new(WIDTH as f32, height as f32)));
    let (_, _, mut panels) = harness.parts();
    panels.paint(
        Layer::Panels,
        &mut ListPainter::new(text, &mut list),
        &crate::draw::tests::PANEL,
    );
    let plan = panels.frame_plan(&list);
    (list, plan)
}

/// What one frame is drawn on: which device, in which format, how tall,
/// and whether the host draws on after the frame in the same pass.
#[derive(Clone, Copy)]
struct On {
    generation: u64,
    format: wgpu::TextureFormat,
    height: u32,
    /// The host fills the whole target white after the frame, setting no
    /// scissor, as a host that draws in the same pass would.
    host_after: bool,
}

/// The first device, in the test's format, at the window's height.
const FIRST: On = On {
    generation: 1,
    format: FORMAT,
    height: HEIGHT,
    host_after: false,
};

/// Draws one frame of the pad app over black, as a host does, and reads
/// the pixels back, row by row.
fn draw(harness: &mut Harness<Pads, NoTarget>, device: &wgpu::Device, queue: &wgpu::Queue, on: On) -> Vec<u8> {
    let On {
        generation,
        format,
        height,
        host_after,
    } = on;
    let host = host_after.then(|| fill(device, format, [1.0, 1.0, 1.0]));
    let text = RefCell::new(Typesetter::new());
    let (list, plan) = paint(harness, &text, height);
    let frame = GpuFrame {
        device,
        queue,
        format,
        generation,
        target: target(height),
        capture: true,
    };
    let mut renderer = PaintRenderer::new(device, format);
    renderer.prepare(device, queue, &list, target(height));
    let (_, _, mut panels) = harness.parts();
    panels.prepare_gpu(&plan, &frame);
    let size = wgpu::Extent3d {
        width: WIDTH,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("custom gpu test target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("custom gpu test pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        panels.draw_frame(&plan, &renderer, &mut pass, &frame);
        if let Some(host) = &host {
            pass.set_pipeline(host);
            pass.draw(0..3, 0..1);
        }
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("custom gpu test read-back"),
        size: u64::from(WIDTH * height * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(height),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.expect("the read-back maps"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU finishes");
    buffer.slice(..).get_mapped_range().to_vec()
}

/// The colour at `x`, `y`, as red, green, or blue, or `None` for any
/// other.
fn colour(pixels: &[u8], x: u32, y: u32) -> Option<char> {
    let at = ((y * WIDTH + x) * 4) as usize;
    match [pixels[at], pixels[at + 1], pixels[at + 2]] {
        [255, 0, 0] => Some('r'),
        [0, 255, 0] => Some('g'),
        [0, 0, 255] => Some('b'),
        [255, 255, 255] => Some('w'),
        _ => None,
    }
}

/// The pad's GPU drawing goes over its parent's background and under the
/// box painted after it, inside its own scissor. The host sets the
/// viewport and scissor back after it, so the box below the pad, outside
/// the pad's viewport and its narrowed scissor, still shows.
#[test]
fn gpu_drawing_goes_in_paint_order_and_leaves_the_pass_as_it_was() {
    let Some((device, queue)) = device() else {
        assert!(!gpu_required(), "LOOM_REQUIRE_GPU is set, but there is no GPU adapter");
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let text = RefCell::new(Typesetter::new());
    let mut harness = harness(
        &text,
        Pads {
            broken: false,
            above: false,
        },
    );
    let pixels = draw(&mut harness, &device, &queue, FIRST);
    // The box painted after it is over it.
    assert_eq!(colour(&pixels, 10, 10), Some('b'));
    // Its own drawing, over the background.
    assert_eq!(colour(&pixels, 40, 10), Some('g'));
    // Outside the scissor it narrowed itself: the background.
    assert_eq!(colour(&pixels, 50, 10), Some('r'));
    // Below it, outside its viewport: the box, which a viewport left set
    // would have squeezed into the pad.
    assert_eq!(colour(&pixels, 10, 50), Some('b'));
    // Away from both: the background.
    assert_eq!(colour(&pixels, 100, 70), Some('r'));
}

/// An element that prepares nothing, as one whose pipeline failed, draws
/// nothing, and the frame draws everything else.
#[test]
fn an_element_that_prepares_nothing_leaves_the_rest() {
    let Some((device, queue)) = device() else {
        assert!(!gpu_required(), "LOOM_REQUIRE_GPU is set, but there is no GPU adapter");
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let text = RefCell::new(Typesetter::new());
    let mut harness = harness(
        &text,
        Pads {
            broken: true,
            above: false,
        },
    );
    let pixels = draw(&mut harness, &device, &queue, FIRST);
    assert_eq!(colour(&pixels, 40, 10), Some('r'), "the background where it would draw");
    assert_eq!(colour(&pixels, 10, 10), Some('b'));
    assert_eq!(colour(&pixels, 10, 50), Some('b'));
}

/// A new device, or a new format, tells the element once, before it
/// prepares again, and leaves its input state. A frame on the same device,
/// or a resize, tells it nothing.
#[test]
fn a_new_device_or_format_is_told_once_and_keeps_input_state() {
    let Some((device, queue)) = device() else {
        assert!(!gpu_required(), "LOOM_REQUIRE_GPU is set, but there is no GPU adapter");
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let text = RefCell::new(Typesetter::new());
    let mut harness = harness(
        &text,
        Pads {
            broken: false,
            above: false,
        },
    );
    let pad = UiId::panel("main").with("pad");
    let lost = |harness: &Harness<Pads, NoTarget>| {
        let state = harness.runtime().custom::<PadState>(&pad).expect("the pad");
        (state.lost, state.dragged)
    };
    harness
        .runtime_mut()
        .customs_mut()
        .state_mut::<PadState>(&pad)
        .expect("the pad")
        .dragged = 7;
    draw(&mut harness, &device, &queue, FIRST);
    draw(&mut harness, &device, &queue, FIRST);
    assert_eq!(lost(&harness), (0, 7), "the same device twice: nothing lost");
    // A resize: the window and the target grow.
    let taller = HEIGHT + 16;
    harness.step(crate::event::DriverEvent::Window(crate::event::WindowEvent::Resized(
        Size::new(WIDTH as f32, taller as f32),
    )));
    harness.lay_out(&mut *text.borrow_mut());
    draw(
        &mut harness,
        &device,
        &queue,
        On {
            height: taller,
            ..FIRST
        },
    );
    assert_eq!(lost(&harness), (0, 7), "a resize: nothing lost");
    // A new format on the same device.
    let bgra = On {
        format: wgpu::TextureFormat::Bgra8Unorm,
        height: taller,
        ..FIRST
    };
    draw(&mut harness, &device, &queue, bgra);
    assert_eq!(lost(&harness), (1, 7), "a new format: lost once, input kept");
    draw(&mut harness, &device, &queue, bgra);
    assert_eq!(lost(&harness).0, 1);
    // A new device.
    let second = On {
        generation: 2,
        height: taller,
        ..FIRST
    };
    let pixels = draw(&mut harness, &device, &queue, second);
    assert_eq!(lost(&harness), (2, 7), "a new device: lost once more, input kept");
    assert_eq!(colour(&pixels, 40, 10), Some('g'), "it built its pipeline again");
}

/// After an element draws above a layer, the host sets the whole target's
/// viewport and scissor back, so what a host draws next in the same pass,
/// with no scissor of its own, reaches every corner.
#[test]
fn drawing_above_leaves_the_pass_for_the_host() {
    let Some((device, queue)) = device() else {
        assert!(!gpu_required(), "LOOM_REQUIRE_GPU is set, but there is no GPU adapter");
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let text = RefCell::new(Typesetter::new());
    let mut harness = harness(
        &text,
        Pads {
            broken: false,
            above: true,
        },
    );
    let pixels = draw(
        &mut harness,
        &device,
        &queue,
        On {
            host_after: true,
            ..FIRST
        },
    );
    for (x, y) in [(0, 0), (WIDTH - 1, 0), (0, HEIGHT - 1), (WIDTH - 1, HEIGHT - 1)] {
        assert_eq!(colour(&pixels, x, y), Some('w'), "the host's fill at ({x}, {y})");
    }
    // With no host fill after it, its drawing above shows in the corner it
    // set its scissor to, over the blue box painted after the pad there,
    // so the white above was the host drawing over a drawing that ran.
    let pixels = draw(&mut harness, &device, &queue, FIRST);
    assert_eq!(colour(&pixels, 0, 0), Some('g'), "its drawing above, over the box");
    assert_eq!(colour(&pixels, 40, 10), Some('g'), "the pad itself");
}

/// An element prepares its drawing above a layer with the viewport it
/// draws it with: the whole target's, not its own bounds.
#[test]
fn drawing_above_prepares_with_the_whole_targets_viewport() {
    let Some((device, queue)) = device() else {
        assert!(!gpu_required(), "LOOM_REQUIRE_GPU is set, but there is no GPU adapter");
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let text = RefCell::new(Typesetter::new());
    let mut harness = harness(
        &text,
        Pads {
            broken: false,
            above: true,
        },
    );
    draw(&mut harness, &device, &queue, FIRST);
    let pad = UiId::panel("main").with("pad");
    let state = harness.runtime().custom::<PadState>(&pad).expect("the pad");
    assert_eq!(
        state.above_viewport,
        Some([0.0, 0.0, WIDTH as f32, HEIGHT as f32]),
        "the whole target, not the pad's 60 by 40"
    );
}
