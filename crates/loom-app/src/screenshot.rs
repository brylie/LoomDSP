//! `loom-app --screenshot`: draws one frame of the canvas offscreen, saves
//! it as a PNG file, and exits. `--screenshot-window` does the same for the
//! whole window: toolbar, canvas, inspector, and stats bar.
//!
//! The frame goes through the same `Scene`, `PanelValues`, and wgpu
//! primitive as the window, drawn by Iced's headless wgpu renderer. So the
//! image is what a user sees, and an agent or a reviewer can check a UI
//! change without a person at the screen. It opens no window, no audio
//! device, and no MIDI port.

use iced::advanced::renderer::{Headless, Style};
use iced::advanced::widget::operation::Operation;
use iced::advanced::widget::{Id, Tree};
use iced::advanced::{Layout, layout, mouse};
use iced::{Color, Element, Font, Pixels, Point, Rectangle, Size, Theme};
use iced_runtime::user_interface::{Cache, UserInterface};
use loom_canvas::{
    Camera, CanvasView, MAX_ZOOM, MIN_ZOOM, Palette, PanelValues, PatchCanvas, PortRef, Scene, Still, StillDrag,
};
use loom_core::{AssignmentId, Controller, ModuleId, OutputRef};
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

/// The error when no GPU adapter is found. The smoke test looks for it to
/// skip on machines without a GPU.
pub const NO_GPU: &str = "no GPU adapter for offscreen drawing";

/// Command-line options for a screenshot.
#[derive(clap::Args, Debug, Clone)]
pub struct Options {
    /// Draw one frame of the canvas to this PNG file and exit, without a
    /// window, audio, or MIDI.
    #[arg(long, value_name = "PNG", group = "image")]
    pub screenshot: Option<std::path::PathBuf>,
    /// Draw one frame of the whole window to this PNG file and exit,
    /// without a window, audio, or MIDI. With --select, the inspector shows
    /// the selected modules. The canvas options below apply to the canvas
    /// inside it.
    #[arg(long, value_name = "PNG", groups = ["image", "inspect"])]
    pub screenshot_window: Option<std::path::PathBuf>,
    /// Image size in logical pixels, as WIDTHxHEIGHT. The default is
    /// 1280x800 for the canvas and the app's window size, 1440x900, for the
    /// whole window.
    #[arg(long, value_name = "WxH", requires = "image")]
    pub size: Option<ImageSize>,
    /// Physical pixels per logical pixel. 2 draws like a high-density
    /// screen, with sharper text.
    #[arg(long, value_name = "FACTOR", default_value_t = 1.0, value_parser = positive, requires = "image")]
    pub scale: f32,
    /// Zoom, in screen pixels per world unit. Without it, the whole patch
    /// fits in the image.
    #[arg(long, value_name = "ZOOM", value_parser = positive, requires = "image")]
    pub zoom: Option<f32>,
    /// Point at the centre of the image, as X,Y in world units, or as
    /// MODULE:NAME like --pointer. Without it, the pointer, or else the
    /// centre of the focused module, or else of the patch.
    #[arg(long, value_name = "X,Y|MODULE:NAME", requires = "image")]
    pub center: Option<Pointer>,
    /// Give one control keyboard focus, as MODULE:PARAM (for example
    /// 4:cutoff). It draws as after a Tab press: the module selected, the
    /// control with its focus ring and value.
    #[arg(long, value_name = "MODULE:PARAM", requires = "image")]
    pub focus: Option<Named>,
    /// Put the pointer here, as X,Y in world units, or as MODULE:NAME for
    /// the centre of a control (by parameter), else a port of that name
    /// (outputs first), else a light or a meter (by display value). Use
    /// MODULE:display:NAME for a light or meter whose name a port also
    /// has, such as 11:display:out on a Level Meter, and MODULE:input:NAME
    /// or MODULE:output:NAME for a port, such as 4:input:cutoff on the
    /// SVF. What is under it draws as hovered.
    #[arg(long, value_name = "X,Y|MODULE:NAME", requires = "image")]
    pub pointer: Option<Pointer>,
    /// Show hover text for what is under the pointer, or else for the
    /// focused control, as after resting there.
    #[arg(long, requires = "image")]
    pub hover_text: bool,
    /// Pin the badge of this output, as MODULE:OUTPUT, as after a click on
    /// it: its modulation lines show and the badge gets a ring.
    #[arg(long, value_name = "MODULE:OUTPUT", requires = "image")]
    pub pin: Option<Named>,
    /// Show a cable being wired from this port, as MODULE:PORT (outputs
    /// first). Its loose end is at --pointer. Over a control that takes
    /// modulation, the control shows the drop highlight. On a connected
    /// input, its cable is picked up as in the app: it leaves the input
    /// and hangs from its source output, and no control is highlighted.
    #[arg(long, value_name = "MODULE:PORT", requires = "pointer", conflicts_with = "depth")]
    pub wire: Option<Named>,
    /// Show the range arc of this knob being dragged, as MODULE:PARAM: only
    /// the assignment being changed shows on its arc. The assignment is
    /// picked as in the app: the pinned output's (--pin), then the only
    /// one from the --focus module, then the only enabled one.
    #[arg(long, value_name = "MODULE:PARAM", requires = "image")]
    pub depth: Option<Named>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            screenshot: None,
            screenshot_window: None,
            size: None,
            scale: 1.0,
            zoom: None,
            center: None,
            focus: None,
            pointer: None,
            hover_text: false,
            pin: None,
            wire: None,
            depth: None,
        }
    }
}

/// The canvas image size without --size.
const CANVAS: Size = Size::new(1280.0, 800.0);

/// Largest image side in pixels, logical or physical. Larger images are
/// slow to draw and may not fit in GPU memory.
const MAX_SIDE: u32 = 8192;

/// Parses a finite number above 0. `f32` parsing alone accepts `NaN` and
/// `inf`, which would make every screen position non-finite.
fn positive(s: &str) -> Result<f32, String> {
    match s.trim().parse::<f32>() {
        Ok(v) if v.is_finite() && v > 0.0 => Ok(v),
        _ => Err(format!("`{s}` is not a number above 0")),
    }
}

/// An image size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

impl FromStr for ImageSize {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT, for example 1280x800")?;
        let parse = |v: &str| match v.trim().parse::<u32>() {
            Ok(n) if (1..=MAX_SIDE).contains(&n) => Ok(n),
            _ => Err(format!("`{v}` is not a size from 1 to {MAX_SIDE}")),
        };
        Ok(Self {
            width: parse(w)?,
            height: parse(h)?,
        })
    }
}

impl ImageSize {
    fn logical(self) -> Size {
        Size::new(self.width as f32, self.height as f32)
    }
}

/// A point in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldPoint(pub Point);

impl FromStr for WorldPoint {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (x, y) = s.split_once(',').ok_or("expected X,Y, for example 400,300")?;
        let parse = |v: &str| match v.trim().parse::<f32>() {
            Ok(n) if n.is_finite() => Ok(n),
            _ => Err(format!("`{v}` is not a finite number")),
        };
        Ok(Self(Point::new(parse(x)?, parse(y)?)))
    }
}

/// A control, port, or parameter named by module ID and name, as
/// MODULE:NAME.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub module: u32,
    pub name: String,
}

impl FromStr for Named {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (module, name) = s.split_once(':').ok_or("expected MODULE:NAME, for example 4:cutoff")?;
        let module = module
            .trim()
            .parse()
            .map_err(|_| format!("`{module}` is not a module ID"))?;
        let name = name.trim();
        if name.is_empty() {
            return Err("the name after `:` is empty".to_string());
        }
        Ok(Self {
            module,
            name: name.to_string(),
        })
    }
}

/// Where the pointer or the image centre is: a world point, or a named
/// control or port.
#[derive(Debug, Clone, PartialEq)]
pub enum Pointer {
    World(Point),
    Named(Named),
}

impl FromStr for Pointer {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        if s.contains(':') {
            s.parse().map(Self::Named)
        } else {
            s.parse()
                .map(|p: WorldPoint| Self::World(p.0))
                .map_err(|_| "expected X,Y or MODULE:NAME, for example 400,300 or 4:cutoff".to_string())
        }
    }
}

/// What the canvas options ask the first frame to show.
struct Shown {
    still: Still,
    /// The assignment an arc drag changes, for its knob's arc.
    dragging: Option<AssignmentId>,
    /// The module with keyboard focus. It is selected, as the focus ring
    /// draws only on a selected module.
    selected: Option<u32>,
}

/// Applies the canvas options to a loaded patch, for a canvas of `size`
/// logical pixels. A wire from a connected input disconnects its cable, so
/// this returns the scene built after that.
fn shown(
    options: &Options,
    controller: &mut Controller,
    positions: &BTreeMap<u32, Point>,
    sizes: &BTreeMap<u32, (u8, u8)>,
    size: Size,
) -> Result<(Scene, Shown), String> {
    // A wire from a connected input picks up its cable, as in the app: the
    // cable leaves the patch and its loose end hangs from the source.
    let wire = match &options.wire {
        Some(target) => Some(pick_up(controller, target)?),
        None => None,
    };
    let controller = &*controller;
    let scene = crate::scene::build(controller, positions, sizes);

    let focus = options
        .focus
        .as_ref()
        .map(|target| find_control(controller, target))
        .transpose()?;
    let point = |p: &Pointer| match p {
        Pointer::World(p) => Ok(*p),
        Pointer::Named(target) => find_point(controller, &scene, target),
    };
    let pointer = options.pointer.as_ref().map(point).transpose()?;
    let pinned = options
        .pin
        .as_ref()
        .map(|t| find_pin(controller, &scene, t))
        .transpose()?;
    let depth = options
        .depth
        .as_ref()
        .map(|t| find_control(controller, t))
        .transpose()?;

    // During an arc drag, the knob's arc shows only the assignment being
    // changed, as in the app.
    let dragging = depth
        .map(|knob| depth_assignment(controller, knob, pinned, focus))
        .transpose()?;

    let centre = options.center.as_ref().map(point).transpose()?.or(pointer).or_else(|| {
        let (module, _) = focus?;
        Some(scene.module(module)?.bounds().center())
    });
    let still = Still {
        camera: camera(&scene, size, options.zoom, centre),
        focus,
        pointer,
        pinned,
        drag: match (wire, depth) {
            (Some((from, pulled)), _) => Some(StillDrag::Wire { from, pulled }),
            (None, Some((module, widget))) => Some(StillDrag::Depth(module, widget)),
            (None, None) => None,
        },
        hover_text: options.hover_text,
    };
    let shown = Shown {
        still,
        dragging,
        selected: focus.map(|(module, _)| module),
    };
    Ok((scene, shown))
}

/// Loads the patch the way the app does, draws the canvas, and writes the
/// PNG.
pub fn run(args: &crate::Args, path: &Path) -> Result<(), String> {
    let config = loom_core::EngineConfig::with_sample_rate(48_000.0);
    let mut session = crate::App::session(config, args.test_modules, args.source())?;
    let options = &args.view;
    let logical = options.size.map_or(CANVAS, ImageSize::logical);
    let (scene, shown) = shown(
        options,
        &mut session.controller,
        &session.positions,
        &session.sizes,
        logical,
    )?;
    let mut values = PanelValues::new();
    crate::scene::update_panel_values_dragging(&mut values, &session.controller, &BTreeMap::new(), shown.dragging);
    let physical = physical_size(logical, options.scale)?;
    let selection = shown.selected.into_iter().collect();
    let pixels = render(&scene, &values, &selection, shown.still, physical, options.scale)?;
    write_png(path, physical, &pixels)
}

/// `loom-app --screenshot-window`: builds the app as `--dump-ui` does, with
/// no audio and no MIDI, draws the whole window, and writes the PNG.
pub fn run_window(args: &crate::Args, path: &Path) -> Result<(), String> {
    let mut app = crate::snapshot::headless(args)?;
    let options = &args.view;
    let logical = options.size.map_or(crate::WINDOW, ImageSize::logical);
    let physical = physical_size(logical, options.scale)?;
    // The window holds Iced's own widgets and the canvas's wgpu primitive,
    // which only the wgpu renderer draws.
    let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))
    .ok_or(NO_GPU)?;

    // The camera fits the canvas, not the window, so lay the window out
    // once to find the canvas size.
    let mut ui = UserInterface::build(app.view(), logical, Cache::default(), &mut renderer);
    let canvas = canvas_view(&mut ui, &mut renderer).ok_or("the window has no patch canvas")?;
    drop(ui);
    let (scene, shown) = shown(
        options,
        &mut app.controller,
        &app.positions,
        &app.sizes,
        canvas.bounds.size(),
    )?;
    app.scene = scene;
    app.gestures.show_depth(shown.dragging);
    app.refresh_values();
    app.selection.extend(shown.selected);
    app.still = Some(shown.still);

    let pixels = draw_window(&app, &mut renderer, logical, options.scale);
    write_png(path, physical, &pixels)
}

/// Draws one frame of the whole window and returns its RGBA pixels.
///
/// The frame first gets a redraw event, as every frame in the app does.
/// Iced's buttons choose their look (enabled, hovered, disabled) while they
/// handle a redraw, and draw as disabled before the first one (#460).
fn draw_window(app: &crate::App, renderer: &mut iced::Renderer, logical: Size, scale: f32) -> Vec<u8> {
    let physical = physical_size(logical, scale).expect("the caller checked the size");
    // A new cache, so the canvas starts from the still state.
    let theme = app.theme();
    let mut ui = UserInterface::build(app.view(), logical, Cache::default(), renderer);
    let redraw = iced::Event::Window(iced::window::Event::RedrawRequested(std::time::Instant::now()));
    // The image is one still frame, so what the widgets ask for is dropped.
    let mut messages = Vec::new();
    let _ = ui.update(
        &[redraw],
        mouse::Cursor::Unavailable,
        renderer,
        &mut iced::advanced::clipboard::Null,
        &mut messages,
    );
    let style = Style {
        text_color: theme.palette().text,
    };
    ui.draw(renderer, &theme, &style, mouse::Cursor::Unavailable);
    // The renderer holds text paragraphs weakly, and the widget tree owns
    // them, so the tree must live until the frame is drawn.
    let pixels = Headless::screenshot(renderer, physical, scale, theme.palette().background);
    drop(ui);
    pixels
}

/// Where the canvas is in the window, and its camera. The canvas reports a
/// [`CanvasView`] to widget operations.
pub(crate) fn canvas_view<M>(
    ui: &mut UserInterface<'_, M, Theme, iced::Renderer>,
    renderer: &mut iced::Renderer,
) -> Option<CanvasView> {
    #[derive(Default)]
    struct Find(Option<CanvasView>);
    impl Operation for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
            if let Some(view) = state.downcast_ref::<CanvasView>() {
                self.0 = Some(*view);
            }
        }
    }
    let mut find = Find::default();
    ui.operate(renderer, &mut find);
    find.0
}

/// The image size in physical pixels. The scale can push it to zero or past
/// [`MAX_SIDE`] even when the logical size is fine, so it is checked again.
fn physical_size(logical: Size, scale: f32) -> Result<Size<u32>, String> {
    let side = |v: f32| {
        let v = (v * scale).round();
        (1.0..=MAX_SIDE as f32)
            .contains(&v)
            .then_some(v as u32)
            .ok_or_else(|| format!("at scale {scale}, a side of {v} pixels is not from 1 to {MAX_SIDE}"))
    };
    Ok(Size::new(side(logical.width)?, side(logical.height)?))
}

/// The camera for an image of `size`: the whole scene framed, as when a
/// patch opens, then the given zoom and centre in place of the framing's.
fn camera(scene: &Scene, size: Size, zoom: Option<f32>, centre: Option<Point>) -> Camera {
    let mut framed = Camera::default();
    if let Some(world) = scene.bounds() {
        framed.frame(world, size);
    }
    let zoom = zoom.unwrap_or(framed.zoom).clamp(MIN_ZOOM, MAX_ZOOM);
    let centre = centre.unwrap_or_else(|| framed.to_world(Point::new(size.width / 2.0, size.height / 2.0)));
    Camera {
        origin: Point::new(
            centre.x - size.width / (2.0 * zoom),
            centre.y - size.height / (2.0 * zoom),
        ),
        zoom,
    }
}

fn descriptor(controller: &Controller, module: u32) -> Result<&'static loom_core::ModuleDescriptor, String> {
    controller
        .graph()
        .node(ModuleId(module))
        .map(|node| node.descriptor)
        .ok_or_else(|| format!("no module {module} in the patch"))
}

/// The widget index of the control bound to a parameter. Panel widgets on
/// the canvas keep the descriptor's order.
pub(crate) fn find_control(controller: &Controller, target: &Named) -> Result<(u32, usize), String> {
    let descriptor = descriptor(controller, target.module)?;
    control_index(descriptor, &target.name)
        .map(|index| (target.module, index))
        .ok_or_else(|| {
            format!(
                "module {} ({}) has no control for parameter `{}`",
                target.module, descriptor.name, target.name
            )
        })
}

fn control_index(descriptor: &loom_core::ModuleDescriptor, param: &str) -> Option<usize> {
    descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.is_control() && w.bind == param)
}

/// The port with this name. An output wins over an input of the same name.
fn find_port(controller: &Controller, target: &Named) -> Result<PortRef, String> {
    let descriptor = descriptor(controller, target.module)?;
    let index = |ports: &[loom_core::PortSpec]| ports.iter().position(|p| p.name == target.name);
    let port = |output, index| PortRef {
        module: target.module,
        output,
        index,
    };
    index(descriptor.outputs)
        .map(|i| port(true, i))
        .or_else(|| index(descriptor.inputs).map(|i| port(false, i)))
        .ok_or_else(|| {
            format!(
                "module {} ({}) has no port `{}`",
                target.module, descriptor.name, target.name
            )
        })
}

/// The port a wire starts from, and whether it was pulled off an input.
/// A connected input is disconnected, and the wire starts at the output
/// that fed it.
fn pick_up(controller: &mut Controller, target: &Named) -> Result<(PortRef, bool), String> {
    let port = find_port(controller, target)?;
    if port.output {
        return Ok((port, false));
    }
    let input = loom_core::InputRef {
        module: ModuleId(port.module),
        port: port.index,
    };
    match controller.disconnect(input).map_err(|e| e.to_string())? {
        Some(source) => Ok((
            PortRef {
                module: source.module.0,
                output: true,
                index: source.port,
            },
            true,
        )),
        None => Ok((port, false)),
    }
}

/// The world point of a named control, port, light, or meter's centre,
/// looked up in the order of rule 10 (`crate::targets`).
fn find_point(controller: &Controller, scene: &Scene, target: &Named) -> Result<Point, String> {
    descriptor(controller, target.module)?;
    let part = crate::targets::find(controller.graph(), target.module, &target.name)?;
    crate::targets::world_bounds(scene, part)
        .map(|bounds| bounds.center())
        .map_err(|e| format!("{}:{}: {e}", target.module, target.name))
}

/// The output to pin. It must have a badge, so at least one assignment.
fn find_pin(controller: &Controller, scene: &Scene, target: &Named) -> Result<PortRef, String> {
    let port = find_port(controller, target)?;
    let badge = scene.badge_bounds(port.module, port.index);
    if !port.output || badge.is_none() {
        return Err(format!(
            "module {} has no output `{}` with assignments, so no badge to pin",
            target.module, target.name
        ));
    }
    Ok(port)
}

/// The assignment an arc drag on `knob` changes, picked as the app picks
/// it. The focused module stands in for the selection.
fn depth_assignment(
    controller: &Controller,
    knob: (u32, usize),
    pinned: Option<PortRef>,
    focus: Option<(u32, usize)>,
) -> Result<loom_core::AssignmentId, String> {
    let (module, widget) = knob;
    let (param, kind, spec) =
        crate::scene::control_param(controller, module, widget).ok_or("not a parameter control")?;
    // Only a knob has a range arc to drag. Toggles, radio buttons, and
    // buttons set their depth in the inspector.
    if kind != loom_core::WidgetKind::Knob {
        return Err(format!(
            "the control for {module}:{} is not a knob, so it has no range arc to drag",
            spec.name
        ));
    }
    let pinned = pinned.map(|p| OutputRef {
        module: ModuleId(p.module),
        port: p.index,
    });
    let selected = focus.map(|(module, _)| ModuleId(module));
    crate::modulation::drag_target(controller.graph(), ModuleId(module), param, pinned, selected)
        .map_err(|why| format!("no arc drag on {module}: {why}"))
}

/// Draws the canvas offscreen and returns its pixels, RGBA, row by row.
/// `size` is in physical pixels.
pub fn render(
    scene: &Scene,
    values: &PanelValues,
    selection: &BTreeSet<u32>,
    still: Still,
    size: Size<u32>,
    scale: f32,
) -> Result<Vec<u8>, String> {
    // The same font and text size as Iced's window defaults.
    let mut renderer = iced::futures::executor::block_on(<iced_wgpu::Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))
    .ok_or(NO_GPU)?;

    let palette = Palette::default();
    let mut canvas: Element<'_, (), Theme, iced_wgpu::Renderer> = PatchCanvas::new(scene, selection, |_| ())
        .values(values)
        .palette(palette)
        .still(still)
        .into();

    let logical = Size::new(size.width as f32 / scale, size.height as f32 / scale);
    let mut tree = Tree::new(&canvas);
    let node = canvas
        .as_widget_mut()
        .layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, logical));
    canvas.as_widget().draw(
        &tree,
        &mut renderer,
        &Theme::Dark,
        &Style {
            text_color: Color::WHITE,
        },
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &Rectangle::with_size(logical),
    );
    Ok(Headless::screenshot(&mut renderer, size, scale, palette.background))
}

pub(crate) fn write_png(path: &Path, size: Size<u32>, rgba: &[u8]) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("cannot write {}: {e}", path.display());
    let file = std::fs::File::create(path).map_err(|e| error(&e))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size.width, size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| error(&e))?;
    writer.write_image_data(rgba).map_err(|e| error(&e))?;
    writer.finish().map_err(|e| error(&e))
}

#[cfg(test)]
mod tests;
