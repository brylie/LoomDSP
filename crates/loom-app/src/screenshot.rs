//! `loom-app --screenshot`: draws one frame of the canvas offscreen, saves
//! it as a PNG file, and exits. `--screenshot-window` does the same for the
//! whole window: toolbar, canvas, inspector, and stats bar.
//!
//! The frame goes through the same `Scene`, `PanelValues`, canvas
//! renderer, and frame drawing (`drawing.rs`) as the window, drawn
//! offscreen (`offscreen.rs`). So the image is what a
//! user sees, and an agent or a reviewer can check a UI
//! change without a person at the screen. It opens no window, no audio
//! device, and no MIDI port.

use crate::offscreen::Offscreen;
use loom_canvas::{
    Camera, CanvasInput, CanvasModel, FrameCache, MAX_ZOOM, MIN_ZOOM, Palette, PanelValues, PortRef, Scene, Still,
    StillDrag,
};
use loom_core::{AssignmentId, Controller, ModuleId, OutputRef};
use loom_ui_core::{Point, Size};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

/// The error when no GPU adapter is found. The smoke test looks for it to
/// skip on machines without a GPU.
pub const NO_GPU: &str = "no GPU adapter for offscreen drawing";

/// The error when the GPU drew a frame but it could not be read back.
const NO_READ_BACK: &str = "the GPU could not read the frame back";

/// An image's size in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Physical {
    /// Its width.
    pub(crate) width: u32,
    /// Its height.
    pub(crate) height: u32,
}

/// Command-line options for a screenshot.
#[derive(clap::Args, Debug, Clone)]
pub struct Options {
    /// Draw one frame of the canvas to this PNG file and exit, without a
    /// window, audio, or MIDI.
    #[arg(long, value_name = "PNG", groups = ["image", "focusable_output"])]
    pub screenshot: Option<std::path::PathBuf>,
    /// Draw one frame of the whole window to this PNG file and exit,
    /// without a window, audio, or MIDI. With --select, the inspector shows
    /// the selected modules. The canvas options below apply to the canvas
    /// inside it.
    #[arg(long, value_name = "PNG", groups = ["image", "inspect", "focusable_output"])]
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
    #[arg(long, value_name = "MODULE:PARAM", requires = "focusable_output")]
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
    hierarchy: &loom_patch::Hierarchy,
    level: &loom_patch::ModulePath,
    positions: &BTreeMap<u32, (f32, f32)>,
    sizes: &BTreeMap<u32, (u8, u8)>,
    size: loom_ui_core::Size,
) -> Result<(Scene, Shown), String> {
    // A wire from a connected input picks up its cable, as in the app: the
    // cable leaves the patch and its loose end hangs from the source.
    let wire = match &options.wire {
        Some(target) => Some(pick_up(controller, target)?),
        None => None,
    };
    let controller = &*controller;
    let scene = crate::scene::build(controller, hierarchy, level, positions, sizes);

    let focus = options
        .focus
        .as_ref()
        .map(|target| find_control(controller, hierarchy, target))
        .transpose()?;
    let point = |p: &Pointer| match p {
        Pointer::World(p) => Ok(*p),
        Pointer::Named(target) => find_point(controller, hierarchy, &scene, target),
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
        .map(|t| find_control(controller, hierarchy, t))
        .transpose()?;

    // During an arc drag, the knob's arc shows only the assignment being
    // changed, as in the app.
    let dragging = depth
        .map(|knob| depth_assignment(controller, hierarchy, knob, pinned, focus))
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
    // A headless run reads no preferences, so it has no library folder:
    // linked sub-patches draw from their stored copies.
    let mut loaded = crate::Loaded::read(config, args.test_modules, args.source(), &loom_patch::NoLibrary)?;
    let options = &args.view;
    let logical = options.size.map_or(CANVAS, ImageSize::logical);
    let (scene, shown) = shown(
        options,
        &mut loaded.controller,
        &loaded.hierarchy,
        &loom_patch::ModulePath::root(),
        &loaded.positions,
        &loaded.sizes,
        logical,
    )?;
    let mut values = PanelValues::new();
    crate::scene::update_panel_values_dragging(
        &mut values,
        &loaded.controller,
        &loaded.hierarchy,
        &BTreeMap::new(),
        &crate::clip::ClipHolds::default(),
        shown.dragging,
    );
    let physical = physical_size(logical, options.scale)?;
    let selection = shown.selected.into_iter().collect();
    let pixels = render(&scene, &values, &selection, shown.still, physical, options.scale)?;
    write_png(path, physical, &pixels)
}

/// `loom-app --screenshot-window`: builds the app as `--dump-ui` does, with
/// no audio and no MIDI, runs it in a headless runtime, draws the whole
/// window as the window draws a frame (`drawing.rs`), and writes the PNG.
pub fn run_window(args: &crate::Args, path: &Path) -> Result<(), String> {
    let options = &args.view;
    let logical = options.size.map_or(crate::WINDOW, ImageSize::logical);
    let physical = physical_size(logical, options.scale)?;
    let mut offscreen = Offscreen::new().ok_or(NO_GPU)?;
    let text = std::cell::RefCell::new(loom_ui::paint::Typesetter::new());

    // The camera fits the canvas, not the window, so a first run lays the
    // window out to find the canvas's size. The app changes only through
    // `update` (ui-layer.md, section 2, rule 1), so the still state goes
    // into a second app before its runtime starts.
    let canvas = {
        // Only the layout is read here, so the engine runs no frames: the
        // app below runs them once, for the state the image shows.
        let probe = crate::Args {
            frames: None,
            hold_note: false,
            ..args.clone()
        };
        let runtime = window_runtime(crate::snapshot::headless(&probe)?, logical, options.scale, &text);
        runtime
            .custom_bounds(&crate::canvas::canvas_id())
            .ok_or("the window has no patch canvas")?
    };
    let mut app = crate::snapshot::headless(args)?;
    let (scene, shown) = shown(
        options,
        &mut app.session.controller,
        &app.session.hierarchy,
        &app.session.level,
        &app.session.positions,
        &app.session.sizes,
        canvas.size(),
    )?;
    app.session.scene = scene;
    app.session.gestures.show_depth(shown.dragging);
    app.refresh_values();
    app.session.selection.extend(shown.selected);
    app.still = Some(shown.still);
    let pixels = window_pixels(app, &mut offscreen, logical, options.scale).ok_or(NO_READ_BACK)?;
    write_png(path, physical, &pixels)
}

/// `app` run in a headless runtime with a window `logical` pixels at
/// `scale`, and drawn as the window draws a frame: its RGBA pixels, or
/// `None` when they could not be read back from the GPU.
pub(crate) fn window_pixels(app: crate::App, offscreen: &mut Offscreen, logical: Size, scale: f32) -> Option<Vec<u8>> {
    let text = std::cell::RefCell::new(loom_ui::paint::Typesetter::new());
    let mut runtime = window_runtime(app, logical, scale, &text);
    let (_, _, mut panels) = runtime.parts();
    offscreen.window(&mut panels, &text, logical, scale)
}

/// `app` in a headless runtime with a window `logical` pixels at `scale`,
/// run until it is laid out and its canvas has synced, as a window shows
/// its first frame.
fn window_runtime(
    app: crate::App,
    logical: Size,
    scale: f32,
    text: &std::cell::RefCell<loom_ui::paint::Typesetter>,
) -> loom_ui::Runtime<crate::App, crate::layers::NoToolkit> {
    let mut window = loom_ui::WindowState::new(logical);
    window.scale = scale;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let wakes = std::sync::Arc::new(AtomicUsize::new(0));
    let woken = std::sync::Arc::clone(&wakes);
    let wake = std::sync::Arc::new(move || {
        woken.fetch_add(1, Ordering::SeqCst);
    });
    let mut runtime = loom_ui::Runtime::new(app, crate::layers::NoToolkit, window, wake);
    let mut driver = loom_ui::headless::Headless::new();
    let now = std::time::Instant::now();
    // As the `Ui` harness settles a window: the first turn builds the view,
    // and each layout may place the canvas, which then hears it and syncs
    // with the app. Turns run while a message, a wake, or a due deadline
    // waits, so the image is the frame the window would show.
    let mut next = runtime.handle(loom_ui::DriverEvent::Woken, now, &mut driver);
    for _ in 0..MOST_TURNS {
        runtime.lay_out(&mut *text.borrow_mut());
        let woke = wakes.swap(0, Ordering::SeqCst) > 0;
        let due = next.deadline.is_some_and(|at| at <= now);
        if !next.again && !woke && !due {
            return runtime;
        }
        let event = if due && !next.again && !woke {
            loom_ui::DriverEvent::DeadlineReached
        } else {
            loom_ui::DriverEvent::Woken
        };
        next = runtime.handle(event, now, &mut driver);
    }
    debug_assert!(false, "the window kept running turns before its first frame");
    runtime.lay_out(&mut *text.borrow_mut());
    runtime
}

/// The most turns a screenshot's window runs before its first frame, as
/// the `Ui` harness allows one event.
const MOST_TURNS: usize = 1000;

/// The image size in physical pixels. The scale can push it to zero or past
/// [`MAX_SIDE`] even when the logical size is fine, so it is checked again.
fn physical_size(logical: Size, scale: f32) -> Result<Physical, String> {
    let side = |v: f32| {
        let v = (v * scale).round();
        (1.0..=MAX_SIDE as f32)
            .contains(&v)
            .then_some(v as u32)
            .ok_or_else(|| format!("at scale {scale}, a side of {v} pixels is not from 1 to {MAX_SIDE}"))
    };
    Ok(Physical {
        width: side(logical.width)?,
        height: side(logical.height)?,
    })
}

/// The camera for an image of `size`: the whole scene framed, as when a
/// patch opens, then the given zoom and centre in place of the framing's.
fn camera(scene: &Scene, size: loom_ui_core::Size, zoom: Option<f32>, centre: Option<Point>) -> Camera {
    let mut framed = Camera::default();
    if let Some(world) = scene.bounds() {
        framed.frame_readable(world, size, loom_canvas::READABLE_ZOOM);
    }
    if zoom.is_none() && centre.is_none() {
        return framed;
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

/// The widget index of the control bound to a parameter, by its name on a
/// module tile or its exposed name on a sub-patch tile. Panel widgets on
/// the canvas keep the descriptor's order, and surface widgets the order
/// the items were exposed.
pub(crate) fn find_control(
    controller: &Controller,
    hierarchy: &loom_patch::Hierarchy,
    target: &Named,
) -> Result<(u32, usize), String> {
    let graph = controller.graph();
    crate::surface::find_control(graph, hierarchy, target.module, &target.name)
        .map(|index| (target.module, index))
        .ok_or_else(|| {
            let tile = hierarchy.tile_name(ModuleId(target.module)).or_else(|| {
                graph
                    .node(ModuleId(target.module))
                    .map(|n| n.descriptor.name.to_string())
            });
            match tile {
                Some(name) => format!(
                    "module {} ({name}) has no control for parameter `{}`",
                    target.module, target.name
                ),
                None => format!("no module {} in the patch", target.module),
            }
        })
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
fn find_point(
    controller: &Controller,
    hierarchy: &loom_patch::Hierarchy,
    scene: &Scene,
    target: &Named,
) -> Result<Point, String> {
    let part = crate::targets::find(controller.graph(), hierarchy, target.module, &target.name)?;
    let mut values = PanelValues::new();
    crate::scene::update_panel_values(&mut values, controller, hierarchy, &BTreeMap::new());
    crate::targets::world_bounds(scene, &values, part)
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
    hierarchy: &loom_patch::Hierarchy,
    knob: (u32, usize),
    pinned: Option<PortRef>,
    focus: Option<(u32, usize)>,
) -> Result<loom_core::AssignmentId, String> {
    let (module, widget) = knob;
    let target = crate::surface::control_target(
        controller.graph(),
        hierarchy,
        module,
        widget,
        loom_canvas::ControlPart::Main,
    )
    .ok_or("not a parameter control")?;
    // Only a control that shows a range has one to drag. Other controls set
    // their depth in the inspector.
    if !target.kind.has_range() {
        return Err(format!(
            "the control for {module}:{} is not a knob, a fader, an XY pad, or a range, so it has no range to drag",
            target.spec.name
        ));
    }
    let pinned = pinned.map(|p| OutputRef {
        module: ModuleId(p.module),
        port: p.index,
    });
    let selected = focus.map(|(module, _)| ModuleId(module));
    crate::modulation::drag_target(controller.graph(), target.module, target.index, pinned, selected)
        .map_err(|why| format!("no arc drag on {module}: {why}"))
}

/// Draws the canvas offscreen and returns its pixels, RGBA, row by row.
/// `size` is in physical pixels.
pub fn render(
    scene: &Scene,
    values: &PanelValues,
    selection: &BTreeSet<u32>,
    still: Still,
    size: Physical,
    scale: f32,
) -> Result<Vec<u8>, String> {
    let mut offscreen = Offscreen::new().ok_or(NO_GPU)?;
    let palette = Palette::default();
    let model = CanvasModel::new(scene, selection)
        .values(values)
        .palette(palette)
        .still(still);
    // The canvas's own state, started from the still frame, as the widget
    // starts it; no Iced view is built, since the image is the canvas alone.
    let input = CanvasInput::still(&model, still, std::time::Instant::now());
    let cache = FrameCache::default();
    let logical = loom_ui_core::Size::new(size.width as f32 / scale, size.height as f32 / scale);
    let bounds = loom_ui_core::Rect::new(Point::ORIGIN, logical);
    let drawing = input.drawing(&model, bounds, &cache);
    offscreen
        .canvas((size.width, size.height), scale, palette.background, &drawing)
        .ok_or_else(|| NO_READ_BACK.to_string())
}

pub(crate) fn write_png(path: &Path, size: Physical, rgba: &[u8]) -> Result<(), String> {
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
