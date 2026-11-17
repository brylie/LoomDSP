//! A countdown in a window, driven by `loom_ui::shell`.
//!
//! The app is the countdown of `countdown.rs`, from five, one step every
//! 200 ms, with a piece of background work that reports through the
//! waker. Here it runs in a real window. It shows one panel, a card with
//! the count that is left and what the work reported, laid out by the
//! runtime and painted with `loom_ui::paint`: a rounded box with a border,
//! and text set with Parley.
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
use loom_ui::paint::{ListPainter, PaintList, PaintRenderer, PanelStyle, Target as PaintTarget, Typesetter};
use loom_ui::shell::{self, Frame, Layers, NoServices, Options, Parts};
use loom_ui::{App, Context, Deadline, Effects, Element, Layer, Length, Routed, Target, View, WindowEvent, WindowSpec};
use loom_ui_core::{Color, Event, Key, KeyEvent, Point, PointerEvent, Rect, Size, TextStyle, UiId, Weight};
use std::cell::RefCell;
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
        view.panel_with(
            UiId::panel("card"),
            Layer::Panels,
            &(self.left, self.status.clone()),
            card,
        );
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
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, out: &mut Vec<Message>) -> Routed {
        match event {
            Event::Pointer(PointerEvent::Pressed(_)) => out.push(Message::Start),
            Event::Key(KeyEvent::Pressed { key: Key::Escape, .. }) => out.push(Message::Quit),
            _ => return Routed::Passed,
        }
        Routed::Taken
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

/// The card's text styles: the count, large, and the status under it.
const COUNT: TextStyle = TextStyle {
    size: 28.0,
    line_height: 1.2,
    weight: Weight::Bold,
};
const STATUS: TextStyle = TextStyle {
    size: 14.0,
    line_height: 1.3,
    weight: Weight::Regular,
};

/// The card: the count left and the status, in a rounded box with a
/// border, centred in the window.
fn card((left, status): &(u32, String)) -> Element {
    let card = Element::column([
        Element::text(format!("{left} left"), COUNT),
        Element::text(status.clone(), STATUS).colour(Color::rgb(0.7, 0.75, 0.8)),
    ])
    .spacing(6.0)
    .padding(16.0)
    .background(Color::rgb(0.16, 0.17, 0.21))
    .border(1.0, Color::rgb(0.35, 0.38, 0.45))
    .radius(10.0);
    Element::column([card])
        .width(Length::FILL)
        .height(Length::FILL)
        .align_x(loom_ui::Align::Center)
        .align_y(loom_ui::Align::Center)
}

/// The layers: a clear, then the panels, painted.
#[derive(Default)]
struct Card {
    /// The text the panels are measured and drawn with.
    text: RefCell<Typesetter>,
    /// The paint renderer, made on the first frame for its format.
    renderer: Option<(wgpu::TextureFormat, PaintRenderer)>,
}

impl Layers<Countdown, Clicks> for Card {
    fn draw(&mut self, mut parts: Parts<'_, Countdown, Clicks>, frame: &Frame<'_>) {
        if self.renderer.as_ref().is_none_or(|(format, _)| *format != frame.format) {
            self.renderer = Some((frame.format, PaintRenderer::new(frame.device, frame.format)));
        }
        let Some((_, renderer)) = &mut self.renderer else {
            return;
        };
        // Lay out and paint with one typesetter, so what is measured is
        // what is drawn.
        self.text.borrow_mut().begin_frame();
        parts.panels.lay_out(&mut *self.text.borrow_mut());
        let whole = Rect::new(Point::ORIGIN, frame.size);
        let mut list = PaintList::new(whole);
        let style = PanelStyle {
            text: Color::rgb(0.93, 0.94, 0.96),
        };
        parts
            .panels
            .paint(Layer::Panels, &mut ListPainter::new(&self.text, &mut list), &style);
        let target = PaintTarget {
            size: frame.size,
            scale: frame.scale,
            origin: Point::ORIGIN,
            physical: (
                (frame.size.width * frame.scale).round() as u32,
                (frame.size.height * frame.scale).round() as u32,
            ),
        };
        renderer.prepare(frame.device, frame.queue, &list, target);
        let mut encoder = frame
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("card") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("card"),
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
            renderer.draw(&mut pass);
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
    if let Err(error) = shell::run(app, Clicks, Card::default(), NoServices, options) {
        eprintln!("window: {error}");
        std::process::exit(1);
    }
}
