//! The overview's projection and pointer handling.

use super::*;
use loom_ui::paint::{Align, Quad, TextPainter, TextRun, TextStyle, Vertical, Weight};

/// A connection to draw between two miniature tiles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapLine {
    /// The source tile.
    pub from: u32,
    /// The target tile.
    pub to: u32,
    /// The signal's colour.
    pub colour: Color,
    /// Thickness in logical pixels.
    pub width: f32,
    /// A repeating mark length in logical pixels, or zero for a solid line.
    pub dash: f32,
}

/// Data for the overview, taken from the same structure as the outline.
#[derive(Debug, Default)]
pub struct MapGraph {
    /// One line per connected pair, plus the modulation lines.
    pub lines: Vec<MapLine>,
    /// Tiles in compiled feedback loops.
    pub loops: BTreeSet<u32>,
}

/// How the overview is drawn and when it shows.
#[derive(Debug, Clone, Copy)]
pub struct MapView<'a> {
    /// The graph drawn over the tile rectangles.
    pub graph: &'a MapGraph,
    /// True to hide the map when the whole level is in view.
    pub auto: bool,
    /// Readable label size from the app's text tokens.
    pub text_size: f32,
}

/// A world-to-overview projection.
#[derive(Debug, Clone, Copy)]
struct Projection {
    area: Rect,
    origin: Point,
    scale: f32,
}

/// Static map drawings, rebuilt for geometry, line styles, or a new map size.
#[derive(Debug)]
pub(super) struct Artwork {
    generation: (u64, u64),
    area: Rect,
    palette: Palette,
    style: TextStyle,
    boxes: Vec<Quad>,
    labels: Vec<(Rect, String)>,
}

impl MapView<'_> {
    /// Map bounds in the window, if its visibility rule is satisfied.
    pub fn bounds(&self, scene: &Scene, camera: Camera, canvas: Rect) -> Option<Rect> {
        let visible = camera.visible(canvas.size());
        if self.auto
            && scene.bounds().is_none_or(|world| {
                visible.x <= world.x
                    && visible.y <= world.y
                    && world.x + world.width <= visible.x + visible.width
                    && world.y + world.height <= visible.y + visible.height
            })
        {
            return None;
        }
        let size = Size::new(200.0_f32.min(canvas.width * 0.25), 150.0_f32.min(canvas.height * 0.25));
        (size.width > 8.0 && size.height > 8.0).then(|| {
            Rect::new(
                Point::new(
                    canvas.x + canvas.width - size.width - 6.0,
                    canvas.y + canvas.height - size.height - 6.0,
                ),
                size,
            )
        })
    }

    /// Fits the patch into its miniature area, keeping its aspect ratio.
    fn project(self, scene: &Scene, camera: Camera, canvas: Rect) -> Option<Projection> {
        let area = self.bounds(scene, camera, canvas)?;
        let world = scene.bounds().unwrap_or_else(|| camera.visible(canvas.size()));
        let scale = ((area.width - 8.0) / world.width.max(1.0)).min((area.height - 8.0) / world.height.max(1.0));
        let origin = area.center() - (world.center() - Point::ORIGIN) * scale;
        Some(Projection { area, origin, scale })
    }
}

impl Projection {
    /// Projects a world point into the overview.
    fn point(self, point: Point) -> Point {
        self.origin + (point - Point::ORIGIN) * self.scale
    }

    /// Converts an overview point to the world it represents.
    fn world(self, point: Point) -> Point {
        Point::ORIGIN + (point - self.origin) * self.scale.recip()
    }

    /// Projects a world rectangle into the overview.
    fn rect(self, rect: Rect) -> Rect {
        Rect::new(self.point(rect.position()), rect.size() * self.scale)
    }
}

/// A press on the map. A click activates on release, a viewport drag pans.
#[derive(Debug, Clone, Copy)]
pub(super) struct MapPress {
    at: Point,
    camera: Camera,
    proj: Projection,
    tile: Option<u32>,
    dragging: bool,
}

impl CanvasModel<'_> {
    /// Offers pointer events to the map before any tile or cable.
    pub(super) fn map_event(
        &self,
        state: &mut CanvasInput,
        event: &Event,
        pointer: Option<Point>,
        bounds: Rect,
        out: &mut Reaction,
    ) -> bool {
        if matches!(event, Event::WindowUnfocused) {
            state.map_press = None;
        }
        let Event::Pointer(event) = event else {
            return false;
        };
        if let Some(mut press) = state.map_press {
            if matches!(event, PointerEvent::Moved)
                && let Some(point) = pointer
            {
                let delta = point - press.at;
                press.dragging |= delta.x.abs().max(delta.y.abs()) >= 3.0;
                if press.dragging && press.proj.rect(press.camera.visible(bounds.size())).contains(press.at) {
                    state.camera.origin = press.camera.origin + delta * press.proj.scale.recip();
                    self.emit(out, CanvasEvent::Camera(state.camera));
                    out.redraw();
                }
                state.map_press = Some(press);
            }
            if matches!(event, PointerEvent::Released(PointerButton::Left)) {
                state.map_press = None;
                if !press.dragging && pointer.is_some_and(|point| press.proj.area.contains(point)) {
                    let world = press
                        .tile
                        .and_then(|id| self.scene.module(id))
                        .map_or_else(|| press.proj.world(press.at), |tile| tile.bounds().center());
                    if let Some(id) = press.tile {
                        state.focus.clear();
                        self.emit(out, CanvasEvent::Select(vec![id]));
                    }
                    state.camera.centre(world, bounds.size());
                    self.emit(out, CanvasEvent::Camera(state.camera));
                    out.redraw();
                }
            }
            out.take();
            return true;
        }
        let Some(point) = pointer else { return false };
        let Some(project) = self.map.and_then(|map| map.project(self.scene, state.camera, bounds)) else {
            return false;
        };
        if !project.area.contains(point) || !matches!(state.interaction, Interaction::Idle) {
            return false;
        }
        state.hover = None;
        state.cursor = None;
        match event {
            PointerEvent::Pressed(PointerButton::Left) => {
                let world = project.world(point);
                let tile = self
                    .scene
                    .modules()
                    .iter()
                    .rev()
                    .find(|tile| tile.bounds().contains(world))
                    .map(|tile| tile.id);
                state.map_press = Some(MapPress {
                    at: point,
                    camera: state.camera,
                    proj: project,
                    tile,
                    dragging: false,
                });
                self.emit(out, CanvasEvent::PointerDown);
            }
            PointerEvent::Scrolled(delta) => {
                let world = project.world(point);
                let factor = match delta {
                    ScrollDelta::Lines { y, .. } => 1.15_f32.powf(*y),
                    ScrollDelta::Pixels { y, .. } => (*y * 0.01).exp(),
                };
                state.camera.zoom_at(state.camera.to_screen(world), factor);
                self.emit(out, CanvasEvent::Camera(state.camera));
            }
            _ => {}
        }
        out.redraw();
        out.take();
        true
    }

    /// Paints the overview before foreground canvas editors and hints.
    pub(super) fn paint_map(&self, state: &CanvasInput, canvas: Rect, painter: &mut dyn TextPainter) {
        let Some(map) = self.map else { return };
        let Some(project) = map.project(self.scene, state.camera, canvas) else {
            return;
        };
        painter.begin_layer(project.area);
        painter.quad(Quad {
            rect: project.area,
            fill: self.palette.control_body,
            border: self.palette.control_track,
            border_width: 1.0,
            radius: 4.0,
        });
        let mut cache = state.map_artwork.borrow_mut();
        let generation = (self.scene.generation(), self.scene.overlay_generation());
        let style = TextStyle {
            size: map.text_size,
            weight: Weight::Regular,
            line_height: 1.1,
        };
        if cache.as_ref().is_none_or(|art| {
            art.generation != generation
                || art.area != project.area
                || art.palette != self.palette
                || art.style != style
        }) {
            *cache = Some(self.map_artwork(map, project, generation, style));
        }
        let art = cache.as_ref().expect("the map artwork was built");
        for &shape in &art.boxes {
            painter.quad(shape);
        }
        for (rect, text) in &art.labels {
            painter.text(&TextRun {
                text,
                anchor: rect.center(),
                bounds: Size::new(rect.width - 4.0, rect.height),
                align: Align::Center,
                vertical: Vertical::Center,
                style,
                wrap: false,
                colour: self.palette.text,
                clip: *rect,
            });
        }
        let view = project.rect(state.camera.visible(canvas.size()));
        painter.quad(Quad {
            rect: view,
            fill: Color::TRANSPARENT,
            border: self.palette.text,
            border_width: 1.0,
            radius: 0.0,
        });
        painter.end_layer();
    }
    /// Builds the static marks of the map apart from the moving view rectangle.
    fn map_artwork(&self, map: MapView<'_>, project: Projection, generation: (u64, u64), style: TextStyle) -> Artwork {
        let mut boxes = Vec::new();
        let mut labels = Vec::new();
        for line in &map.graph.lines {
            let (Some(a), Some(b)) = (self.scene.module(line.from), self.scene.module(line.to)) else {
                continue;
            };
            line_marks(
                &mut boxes,
                project.point(a.bounds().center()),
                project.point(b.bounds().center()),
                *line,
            );
        }
        for tile in self.scene.modules() {
            let rect = project.rect(tile.bounds());
            boxes.push(Quad::filled(rect, tile.accent, 1.0));
            if map.graph.loops.contains(&tile.id) {
                boxes.push(Quad::filled(
                    Rect::new(rect.position(), Size::new(4.0, 4.0)),
                    self.palette.text,
                    1.0,
                ));
            }
            if rect.height >= style.size && rect.width > 4.0 {
                let text = crate::scene::fit_label(&tile.title, rect.width - 4.0, style.size);
                if text != "…" {
                    labels.push((rect, text.into_owned()));
                }
            }
        }
        Artwork {
            generation,
            area: project.area,
            palette: self.palette,
            style,
            boxes,
            labels,
        }
    }
}

/// Builds a line in the map's pixel grid using small overlapping marks.
fn line_marks(boxes: &mut Vec<Quad>, a: Point, b: Point, line: MapLine) {
    let delta = b - a;
    let steps = delta.x.abs().max(delta.y.abs()).ceil() as usize;
    if steps == 0 {
        return;
    }
    let size = Size::new(line.width, line.width);
    for step in 0..=steps {
        if line.dash > 0.0 && (step as f32 % (2.0 * line.dash)) >= line.dash {
            continue;
        }
        let centre = a + delta * (step as f32 / steps as f32);
        let origin = centre - Vector::new(line.width * 0.5, line.width * 0.5);
        boxes.push(Quad::filled(Rect::new(origin, size), line.colour, 0.0));
    }
}
