//! A custom element that draws with the GPU: an XY pad, in a window.
//!
//! The pad is a custom element (docs/design/ui-layer.md, section 9). It
//! draws its grid and its dot with a `wgpu` pipeline of its own, at its
//! place in the panel's paint order, and paints its border through the
//! runtime's paint walk, over its GPU drawing. The host sets its viewport
//! and scissor and sets them back after it.
//!
//! - Press and drag on it: it captures the pointer, so the drag goes on
//!   outside it and outside the window, until the button is up. If the
//!   window cancels the drag, as when it loses the keys, the dot goes back
//!   to where it was before the press.
//! - Tab to it, and the keys follow the WAI-ARIA slider pattern on two
//!   axes. The pad is one focus stop, so the axis is in the key: Left and
//!   Right move X, and Up and Down move Y, a twentieth at a time; Page Up
//!   and Page Down move Y a quarter, and with Shift they move X; Home and
//!   End set X to its least and most, and with Shift they set Y.
//! - Over it, or while dragging, the pointer is a crosshair.
//! - A screen reader meets a group named "XY pad" holding two sliders, X
//!   and Y, which it can move.
//! - Escape, or the window's close button, quits. `--capture out.png`
//!   saves the first frame shown, then quits.
//!
//! Run it with `cargo run -p loom-ui --example xy_pad`.

use accesskit::{NodeId, TreeId, TreeInfo, TreeUpdate};
use loom_ui::AccessEvent;
use loom_ui::access::{Access, Action, Role, Value};
use loom_ui::effect::{FocusTarget, Image, NoGpu, Selection};
use loom_ui::paint::{
    ListPainter, PaintList, PaintRenderer, PanelStyle, Quad, Target as PaintTarget, TextPainter, Typesetter,
};
use loom_ui::shell::{self, Frame, Layers, NoServices, Options, Parts};
use loom_ui::{
    AccessRequest, AccessValue, App, Context, Custom, CustomEvent, CustomGpu, CustomKind, CustomNodes, Effects,
    Element, Gpu, GpuFrame, Layer, Length, Moment, Out, Pass, Routed, Target, Used, View, WindowEvent, WindowSpec,
};
use loom_ui_core::{
    Color, Cursor, Event, IdKey, IdTable, Key, KeyEvent, Point, PointerButton, PointerEvent, Rect, Size, TextStyle,
    UiId, Weight,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;

/// The pad's kind.
const PAD: CustomKind = CustomKind::new("xy");

/// How far one arrow key moves the dot.
const STEP: f32 = 0.05;

/// How far Page Up and Page Down move it.
const PAGE: f32 = 0.25;

/// The grid and the dot, drawn over the pad's whole viewport: the dot
/// where the uniform puts it, and grid lines every quarter.
const SHADER: &str = "
struct Pad { rect: vec4<f32>, dot: vec2<f32>, radius: f32, pad: f32 };
@group(0) @binding(0) var<uniform> pad: Pad;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    if distance(at.xy, pad.dot) < pad.radius {
        return vec4<f32>(0.95, 0.95, 0.95, 1.0);
    }
    let local = (at.xy - pad.rect.xy) / pad.rect.zw;
    let line = step(vec2<f32>(0.985), fract(local * 4.0));
    let shade = 0.08 * max(line.x, line.y);
    return vec4<f32>(0.12 + shade, 0.13 + shade, 0.17 + shade, 1.0);
}
";

/// The example's messages.
#[derive(Debug)]
enum Message {
    /// Move the dot to `x`, `y`, each from 0 to 1, with `y` up.
    Set(f32, f32),
    /// The first frame is shown: capture it, for `--capture`.
    Shoot,
    /// The capture came back.
    Captured(Result<Image, NoGpu>),
    /// Send the screen reader the tree as it is now.
    Tree,
    /// A screen reader asked to focus the element with this UI ID.
    Focus(UiId),
    /// The user asked to quit.
    Quit,
}

/// The app: where the dot is, the screen reader's node table, and where
/// to save the first frame.
struct XyApp {
    x: f32,
    y: f32,
    /// The app's one table of node IDs, which maps a screen reader's node
    /// back to its UI ID.
    ids: RefCell<IdTable>,
    capture_to: Option<PathBuf>,
}

impl XyApp {
    /// The whole tree: a window holding the runtime's nodes, or, before
    /// the first layout, a message to build it after the next draw.
    fn tree(&mut self, cx: &Context<'_, Message>) -> Effects<Message> {
        if !cx.laid_out() {
            return Effects::after_draw(Message::Tree);
        }
        let mut ids = self.ids.borrow_mut();
        let nodes = cx.access_nodes(&mut ids);
        let root = NodeId(ids.number(&UiId::panel("window")));
        let mut window = accesskit::Node::new(accesskit::Role::Window);
        window.set_label("XY pad");
        window.set_children(
            nodes
                .roots
                .iter()
                .flat_map(|(_, _, tops)| tops.iter().copied())
                .collect::<Vec<_>>(),
        );
        let mut all = vec![(root, window)];
        all.extend(nodes.nodes);
        Effects::accessibility(
            TreeUpdate {
                nodes: all,
                tree: Some(TreeInfo::new(root)),
                tree_id: TreeId::ROOT,
                focus: nodes.focus.unwrap_or(root),
            },
            None,
        )
    }
}

impl App for XyApp {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        match message {
            Message::Set(x, y) => {
                self.x = x.clamp(0.0, 1.0);
                self.y = y.clamp(0.0, 1.0);
                Effects::none()
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
            Message::Tree => self.tree(cx),
            Message::Focus(id) => Effects::focus(id),
            Message::Quit => Effects::quit(),
        }
    }

    fn view(&self, view: &mut View) {
        let shown = (rounded(self.x), rounded(self.y));
        view.panel_with(UiId::panel("main"), Layer::Panels, &shown, |&(x, y)| {
            Element::column([
                Element::text(format!("X {:.2}   Y {:.2}", x as f32 / 100.0, y as f32 / 100.0), LABEL),
                Element::custom(PAD)
                    .key("pad")
                    .width(Length::Fixed(240.0))
                    .height(Length::Fixed(240.0))
                    .focusable()
                    .access(Access::new(Role::Group, "XY pad")),
            ])
            .spacing(10.0)
            .padding(20.0)
            .width(Length::FILL)
            .height(Length::FILL)
            .background(Color::rgb(0.08, 0.08, 0.1))
        });
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "XY pad".into(),
            min_size: Some(Size::new(280.0, 320.0)),
        }
    }

    fn window_event(&self, event: &WindowEvent) -> Option<Message> {
        matches!(event, WindowEvent::CloseRequested).then_some(Message::Quit)
    }

    fn access_changed(&self) -> Option<Message> {
        Some(Message::Tree)
    }

    fn focus_moved(&self, _id: Option<&UiId>) -> Option<Message> {
        // The focus is in the tree, so a move sends it again.
        Some(Message::Tree)
    }

    fn access_target(&self, node: NodeId) -> Option<UiId> {
        self.ids.borrow().id(node.0).cloned()
    }

    fn accessibility_event(&self, event: &AccessEvent) -> Option<Message> {
        // The sliders' own actions reach the pad; Focus comes here.
        match event {
            AccessEvent::Activated => Some(Message::Tree),
            AccessEvent::Action(request) if request.action == accesskit::Action::Focus => {
                self.access_target(request.target_node).map(Message::Focus)
            }
            _ => None,
        }
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == PAD).then(|| Box::new(PadState::default()) as Box<dyn Custom<Self>>)
    }
}

/// `value` in hundredths, so the label and the nodes change only when
/// what they show does.
fn rounded(value: f32) -> i32 {
    (value * 100.0).round() as i32
}

/// The label's text style.
const LABEL: TextStyle = TextStyle {
    size: 15.0,
    line_height: 1.3,
    weight: Weight::Regular,
};

/// The pad's state: where the dot was when a drag began, while one holds
/// it, and its GPU data, built for one device and format and dropped when
/// either changes.
#[derive(Default)]
struct PadState {
    /// The dot's place before the press, kept so a cancelled drag can put
    /// it back (section 9: a cancel undoes what the gesture began).
    drag_from: Option<(f32, f32)>,
    gpu: Option<PadGpu>,
}

/// The pad's GPU data.
struct PadGpu {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
}

impl PadState {
    /// The dot's place for the pointer at `at` in a pad at `bounds`.
    fn place(at: Point, bounds: Rect) -> Message {
        let x = (at.x - bounds.x) / bounds.width;
        let y = 1.0 - (at.y - bounds.y) / bounds.height;
        Message::Set(x, y)
    }
}

impl Custom<XyApp> for PadState {
    fn event(&mut self, app: &XyApp, event: &CustomEvent, moment: &Moment, out: &mut Out<'_, Message>) -> Used {
        match event {
            CustomEvent::Pointer(PointerEvent::Pressed(PointerButton::Left)) => {
                self.drag_from = Some((app.x, app.y));
                if let Some(at) = moment.pointer {
                    out.send(Self::place(at, moment.bounds));
                }
                Used::Capture
            }
            CustomEvent::Pointer(PointerEvent::Moved) if self.drag_from.is_some() => {
                if let Some(at) = moment.pointer {
                    out.send(Self::place(at, moment.bounds));
                }
                Used::Yes
            }
            // A release keeps where the drag ended.
            CustomEvent::Pointer(PointerEvent::Released(PointerButton::Left)) => {
                self.drag_from = None;
                Used::Yes
            }
            // A cancel puts the dot back where it was before the press.
            CustomEvent::CaptureLost => {
                if let Some((x, y)) = self.drag_from.take() {
                    out.send(Message::Set(x, y));
                }
                Used::Yes
            }
            CustomEvent::Key(KeyEvent::Pressed { key, modifiers, .. }) => {
                let (x, y) = (app.x, app.y);
                let shift = modifiers.shift;
                let (x, y) = match key {
                    Key::Left => (x - STEP, y),
                    Key::Right => (x + STEP, y),
                    Key::Up => (x, y + STEP),
                    Key::Down => (x, y - STEP),
                    Key::PageUp if shift => (x + PAGE, y),
                    Key::PageDown if shift => (x - PAGE, y),
                    Key::PageUp => (x, y + PAGE),
                    Key::PageDown => (x, y - PAGE),
                    Key::Home if shift => (x, 0.0),
                    Key::End if shift => (x, 1.0),
                    Key::Home => (0.0, y),
                    Key::End => (1.0, y),
                    _ => return Used::No,
                };
                out.send(Message::Set(x, y));
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn cursor(&self, _app: &XyApp, _moment: &Moment) -> Cursor {
        Cursor::Crosshair
    }

    fn access(&self, app: &XyApp, bounds: Rect, nodes: &mut CustomNodes) {
        for (key, name, value) in [("x", "X", app.x), ("y", "Y", app.y)] {
            // The same hundredths `access_version` keys on, so a move too
            // small to change the version never changes a node.
            let value = f64::from(rounded(value)) / 100.0;
            let slider = Access::new(Role::Slider, name)
                .value(Value::Number {
                    value,
                    min: 0.0,
                    max: 1.0,
                    step: Some(f64::from(STEP)),
                })
                .description(format!("{value:.2}"))
                .action(Action::Increment)
                .action(Action::Decrement)
                .action(Action::SetValue);
            nodes.add([key], slider, bounds);
        }
    }

    fn access_version(&self, app: &XyApp) -> u64 {
        // The nodes read the app's place, so they change when it does.
        (rounded(app.x) as u64) << 32 | rounded(app.y) as u64
    }

    fn access_action(
        &mut self,
        app: &XyApp,
        rest: &[IdKey],
        request: &AccessRequest,
        out: &mut Out<'_, Message>,
    ) -> bool {
        let axis = match rest {
            [IdKey::Name(name)] if &**name == "x" => 0,
            [IdKey::Name(name)] if &**name == "y" => 1,
            _ => return false,
        };
        let now = [app.x, app.y][axis];
        let to = match (request.action, &request.value) {
            (Action::Increment, _) => now + STEP,
            (Action::Decrement, _) => now - STEP,
            (Action::SetValue, Some(AccessValue::Number(value))) => *value as f32,
            _ => return false,
        };
        out.send(if axis == 0 {
            Message::Set(to, app.y)
        } else {
            Message::Set(app.x, to)
        });
        true
    }

    fn paint(&self, _app: &XyApp, bounds: Rect, painter: &mut dyn TextPainter) {
        // A border, painted over the GPU drawing.
        painter.quad(Quad {
            rect: bounds,
            fill: Color::TRANSPARENT,
            border: Color::rgb(0.4, 0.45, 0.55),
            border_width: 1.0,
            radius: 0.0,
        });
    }

    fn gpu(&self) -> Option<&dyn CustomGpu<XyApp>> {
        Some(self)
    }

    fn gpu_mut(&mut self) -> Option<&mut dyn CustomGpu<XyApp>> {
        Some(self)
    }
}

impl CustomGpu<XyApp> for PadState {
    fn prepare(&mut self, app: &XyApp, gpu: &Gpu<'_>) {
        let made = self.gpu.get_or_insert_with(|| make(gpu));
        let [x, y, width, height] = gpu.viewport;
        let dot = [x + app.x * width, y + (1.0 - app.y) * height];
        let radius = 8.0 * gpu.target.scale;
        let values = [x, y, width, height, dot[0], dot[1], radius, 0.0];
        let bytes: Vec<u8> = values.iter().flat_map(|value| value.to_ne_bytes()).collect();
        gpu.queue.write_buffer(&made.uniform, 0, &bytes);
    }

    fn draw(&self, pass: &mut Pass<'_>) {
        let Some(made) = &self.gpu else { return };
        pass.set_pipeline(&made.pipeline);
        pass.set_bind_group(0, &made.group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn gpu_lost(&mut self) {
        self.gpu = None;
    }
}

/// The pad's pipeline and uniform for `gpu`'s device and format.
fn make(gpu: &Gpu<'_>) -> PadGpu {
    let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("xy pad"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("xy pad"),
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
            targets: &[Some(gpu.format.into())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("xy pad"),
        size: 32,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("xy pad"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    });
    PadGpu {
        pipeline,
        uniform,
        group,
    }
}

/// The routing target: Escape quits; everything else goes on.
struct Keys;

impl Target<Message> for Keys {
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, out: &mut Vec<Message>) -> Routed {
        match event {
            Event::Key(KeyEvent::Pressed { key: Key::Escape, .. }) => {
                out.push(Message::Quit);
                Routed::Taken
            }
            _ => Routed::Passed,
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

/// The layers: a clear, then the panels with the pad cut into them.
#[derive(Default)]
struct Window {
    text: RefCell<Typesetter>,
    renderer: Option<(wgpu::TextureFormat, PaintRenderer)>,
}

impl Layers<XyApp, Keys> for Window {
    fn draw(&mut self, mut parts: Parts<'_, XyApp, Keys>, frame: &Frame<'_>) {
        if self.renderer.as_ref().is_none_or(|(format, _)| *format != frame.format) {
            self.renderer = Some((frame.format, PaintRenderer::new(frame.device, frame.format)));
        }
        let Some((_, renderer)) = &mut self.renderer else {
            return;
        };
        self.text.borrow_mut().begin_frame();
        parts.panels.lay_out(&mut *self.text.borrow_mut());
        let mut list = PaintList::new(Rect::new(Point::ORIGIN, frame.size));
        parts
            .panels
            .paint(Layer::Panels, &mut ListPainter::new(&self.text, &mut list), &style());
        let plan = parts.panels.frame_plan(&list);
        let target = PaintTarget {
            size: frame.size,
            scale: frame.scale,
            origin: Point::ORIGIN,
            physical: (
                (frame.size.width * frame.scale).round() as u32,
                (frame.size.height * frame.scale).round() as u32,
            ),
        };
        let gpu = GpuFrame {
            device: frame.device,
            queue: frame.queue,
            format: frame.format,
            // The shell keeps one device for the window's life.
            generation: 0,
            target,
            capture: frame.capture,
        };
        renderer.prepare(frame.device, frame.queue, &list, target);
        parts.panels.prepare_gpu(&plan, &gpu);
        let mut encoder = frame
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("xy pad") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("xy pad"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame.view,
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
            parts.panels.draw_frame(&plan, renderer, &mut pass, &gpu);
        }
        frame.queue.submit([encoder.finish()]);
    }
}

/// The panels' style.
fn style() -> PanelStyle {
    let light = Color::rgb(0.93, 0.94, 0.96);
    let grey = Color::rgb(0.6, 0.6, 0.6);
    PanelStyle {
        text: light,
        caret: light,
        selection: Color::rgb(0.25, 0.4, 0.65),
        ring: loom_ui::RingStyle {
            color: Color::rgb(0.4, 0.7, 1.0),
            width: 2.0,
            offset: 2.0,
        },
        quiet: grey,
        mark: light,
        primary: Color::rgb(0.3, 0.6, 1.0),
        track: grey,
        check: loom_ui::paint::CheckLook {
            edge: grey,
            fill: Color::rgb(0.3, 0.5, 0.9),
            mark: Color::rgb(0.1, 0.1, 0.1),
        },
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
            eprintln!("usage: xy_pad [--capture out.png]");
            std::process::exit(2);
        }
    };
    let first = if capture_to.is_some() {
        vec![Message::Set(0.7, 0.3), Message::Shoot]
    } else {
        vec![Message::Set(0.5, 0.5)]
    };
    let app = XyApp {
        x: 0.5,
        y: 0.5,
        ids: RefCell::new(IdTable::default()),
        capture_to,
    };
    let options = Options {
        size: Size::new(280.0, 320.0),
        first,
    };
    if let Err(error) = shell::run(app, Keys, Window::default(), NoServices, options) {
        eprintln!("xy_pad: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests;
