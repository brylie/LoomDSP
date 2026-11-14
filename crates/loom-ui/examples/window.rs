//! A countdown in a window, driven by `loom_ui::shell`.
//!
//! The app is the countdown of `countdown.rs`, from five, one step every
//! 200 ms, with a piece of background work that reports through the
//! waker. Here it runs in a real window. Its layers clear to a dark colour
//! and draw one bar, as wide as the count that is left.
//!
//! - A click anywhere starts the count again: a pointer event, through the
//!   routing target, into `update`.
//! - Escape, or the window's close button, quits: the app answers with
//!   `Effects::quit`, and the shell ends the loop.
//! - `--capture out.png` saves the first frame shown, through the
//!   runtime's capture effect and the shell's offscreen draw, then quits.
//!   It checks the shell's drawing path with no one watching the window.
//!
//! It uses no LoomDSP crate besides `loom-ui` and `loom-ui-core`
//! (docs/design/ui-layer.md, section 1, rule 3). Run it with
//! `cargo run -p loom-ui --example window`, or
//! `cargo run -p loom-ui --example window -- --capture out.png`.

use loom_ui::effect::{FocusTarget, Image, NoGpu, Selection};
use loom_ui::shell::{self, Frame, Layers, NoServices, Options, Parts};
use loom_ui::{App, Context, Deadline, Effects, Layer, Target, View, WindowEvent, WindowSpec};
use loom_ui_core::{Event, Key, KeyEvent, Point, PointerEvent, Size, UiId};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

/// How long one step of the countdown lasts.
const STEP: Duration = Duration::from_millis(200);

/// Where the count starts.
const FROM: u32 = 5;

/// The countdown's messages.
#[derive(Debug)]
enum Message {
    /// Start, or start again: set the first deadline and start the work.
    Start,
    /// One step of the countdown.
    Step,
    /// The background work finished.
    Loaded(String),
    /// The user asked to quit.
    Quit,
    /// The first frame is shown: capture it.
    Shoot,
    /// The capture came back.
    Captured(Result<Image, NoGpu>),
}

/// The app: what is left, when the next step is due, and what the work
/// reported.
struct Countdown {
    left: u32,
    next_step: Option<Instant>,
    status: String,
    /// Where to save the first frame, for `--capture`.
    capture_to: Option<PathBuf>,
}

impl App for Countdown {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        match message {
            Message::Start => {
                self.left = FROM;
                self.next_step = Some(cx.now() + STEP);
                self.status = "loading".into();
                let work = Effects::work(|| {
                    // Stands for slow work, such as reading a folder.
                    thread::sleep(Duration::from_millis(450));
                    Message::Loaded("loaded on a worker thread".into())
                });
                match self.capture_to {
                    Some(_) => work.and(Effects::after_draw(Message::Shoot)),
                    None => work,
                }
            }
            Message::Shoot => Effects::capture(Message::Captured),
            Message::Captured(image) => {
                match (image, &self.capture_to) {
                    (Ok(image), Some(path)) => match save(&image, path) {
                        Ok(()) => println!("saved {}x{} to {}", image.width(), image.height(), path.display()),
                        Err(error) => eprintln!("could not save {}: {error}", path.display()),
                    },
                    (Err(NoGpu), _) => eprintln!("no GPU to capture with"),
                    (Ok(_), None) => {}
                }
                Effects::quit()
            }
            Message::Step => {
                self.left = self.left.saturating_sub(1);
                self.next_step = (self.left > 0).then(|| cx.now() + STEP);
                Effects::none()
            }
            Message::Loaded(status) => {
                println!("{status}");
                self.status = status;
                Effects::none()
            }
            Message::Quit => Effects::quit(),
        }
    }

    fn view(&self, view: &mut View) {
        view.panel(UiId::panel("left"), Layer::Panels, &self.left);
        view.panel(UiId::panel("status"), Layer::Panels, &self.status);
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: format!("Countdown: {} left", self.left),
            min_size: Some(Size::new(240.0, 120.0)),
        }
    }

    fn next_deadline(&self) -> Option<Deadline<Message>> {
        self.next_step.map(|at| Deadline {
            at,
            message: Message::Step,
        })
    }

    fn window_event(&self, event: &WindowEvent) -> Option<Message> {
        matches!(event, WindowEvent::CloseRequested).then_some(Message::Quit)
    }
}

/// The routing target: a click starts again, Escape quits. It has no
/// focus of its own.
struct Clicks;

impl Target<Message> for Clicks {
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, out: &mut Vec<Message>) {
        match event {
            Event::Pointer(PointerEvent::Pressed(_)) => out.push(Message::Start),
            Event::Key(KeyEvent::Pressed { key: Key::Escape, .. }) => out.push(Message::Quit),
            _ => {}
        }
    }

    fn focus(&mut self, _target: &FocusTarget, _select: Selection) {}

    fn scroll_into_view(&mut self, _id: &UiId) {}

    fn focused(&self) -> Option<UiId> {
        None
    }

    fn needs_redraw(&mut self, _updates: usize) -> bool {
        false
    }
}

/// The bar's shader: two triangles from the vertex index, placed and
/// coloured by one uniform.
const SHADER: &str = "
struct Bar { rect: vec4<f32>, colour: vec4<f32> };
@group(0) @binding(0) var<uniform> bar: Bar;

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let corner = corners[index];
    return vec4(mix(bar.rect.x, bar.rect.z, corner.x), mix(bar.rect.y, bar.rect.w, corner.y), 0.0, 1.0);
}

@fragment
fn fs() -> @location(0) vec4<f32> {
    return bar.colour;
}
";

/// What the bar is drawn with, made on the first frame for its format.
struct Pipeline {
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Pipeline {
    /// The bar's pipeline for `format`.
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bar"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bar"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bar"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bar"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bar"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bar"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        Self {
            format,
            pipeline,
            uniform,
            bind_group,
        }
    }
}

/// The layers: a clear and one bar.
#[derive(Default)]
struct Bar {
    pipeline: Option<Pipeline>,
}

impl Layers<Countdown, Clicks> for Bar {
    fn draw(&mut self, parts: Parts<'_, Countdown, Clicks>, frame: &Frame<'_>) {
        if self
            .pipeline
            .as_ref()
            .is_none_or(|pipeline| pipeline.format != frame.format)
        {
            self.pipeline = Some(Pipeline::new(frame.device, frame.format));
        }
        let Some(pipeline) = &self.pipeline else {
            return;
        };
        // In clip space: a bar across the middle, as wide as what is left.
        let share = parts.app.left as f32 / FROM as f32;
        let (left, right) = (-0.8, -0.8 + 1.6 * share);
        let values = [left, -0.2, right, 0.2, 0.3, 0.7, 0.9, 1.0];
        let bytes: Vec<u8> = values.iter().flat_map(|value: &f32| value.to_ne_bytes()).collect();
        frame.queue.write_buffer(&pipeline.uniform, 0, &bytes);
        let mut encoder = frame
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("bar") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bar"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.08,
                            g: 0.08,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if share > 0.0 {
                pass.set_pipeline(&pipeline.pipeline);
                pass.set_bind_group(0, &pipeline.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        frame.queue.submit([encoder.finish()]);
    }
}

/// Writes `image` to `path` as a PNG.
fn save(image: &Image, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(image.rgba())?;
    Ok(())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let capture_to = match (args.next().as_deref(), args.next()) {
        (Some("--capture"), Some(path)) => Some(PathBuf::from(path)),
        (None, _) => None,
        _ => {
            eprintln!("usage: window [--capture out.png]");
            std::process::exit(2);
        }
    };
    let app = Countdown {
        left: FROM,
        next_step: None,
        status: String::new(),
        capture_to,
    };
    let options = Options {
        size: Size::new(480.0, 240.0),
        first: vec![Message::Start],
    };
    if let Err(error) = shell::run(app, Clicks, Bar::default(), NoServices, options) {
        eprintln!("window: {error}");
        std::process::exit(1);
    }
}
