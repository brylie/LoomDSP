//! What the canvas shows: modules, ports, and cables in world coordinates.
//!
//! The application builds a [`Scene`] from its own data model and passes it to
//! the canvas. The canvas never edits the scene. It reports gestures as
//! messages, and the application changes its model and the scene.

use iced::{Color, Point, Rectangle, Vector};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Size of the snap grid, in world units.
pub const GRID: f32 = 12.0;
/// Width of a module tile.
pub const TILE_WIDTH: f32 = 132.0;
/// Height of the title row at the top of a tile.
pub const HEADER: f32 = 22.0;
/// Height of one port row.
pub const ROW: f32 = 16.0;
/// Space below the last port row.
pub const PADDING: f32 = 6.0;
/// Radius of a port marker.
pub const PORT_RADIUS: f32 = 4.5;
/// How far from a port's centre a click still hits it.
pub const PORT_HIT_RADIUS: f32 = 9.0;

/// How a port marker is drawn. Shape is a second cue next to colour, so
/// signal types do not rely on colour alone (NFR-UX-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortShape {
    /// A circle.
    Round,
    /// A square with slightly rounded corners.
    Square,
}

/// One input or output port.
#[derive(Debug, Clone)]
pub struct PortView {
    /// Label shown next to the port when zoomed in.
    pub label: String,
    /// Port and cable colour.
    pub colour: Color,
    /// Marker shape.
    pub shape: PortShape,
}

/// One module tile.
#[derive(Debug, Clone)]
pub struct ModuleView {
    /// Stable ID, chosen by the application.
    pub id: u32,
    /// Title shown in the header.
    pub title: String,
    /// Header colour, usually by module category.
    pub accent: Color,
    /// Top-left corner in world coordinates.
    pub position: Point,
    /// Inputs, drawn on the left edge from the top.
    pub inputs: Vec<PortView>,
    /// Outputs, drawn on the right edge from the top.
    pub outputs: Vec<PortView>,
}

impl ModuleView {
    /// Tile height for this many port rows, rounded up to the grid so tiles
    /// stack neatly.
    pub fn height(&self) -> f32 {
        let rows = self.inputs.len().max(self.outputs.len()).max(1) as f32;
        ((HEADER + rows * ROW + PADDING) / GRID).ceil() * GRID
    }

    /// The tile rectangle in world coordinates.
    pub fn bounds(&self) -> Rectangle {
        Rectangle::new(self.position, iced::Size::new(TILE_WIDTH, self.height()))
    }

    /// Centre of an input port.
    pub fn input_position(&self, index: usize) -> Point {
        Point::new(self.position.x, self.position.y + HEADER + ROW * (index as f32 + 0.5))
    }

    /// Centre of an output port.
    pub fn output_position(&self, index: usize) -> Point {
        Point::new(
            self.position.x + TILE_WIDTH,
            self.position.y + HEADER + ROW * (index as f32 + 0.5),
        )
    }
}

/// A port address: module ID, direction, and index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortRef {
    /// Module ID.
    pub module: u32,
    /// True for outputs.
    pub output: bool,
    /// Port index within its direction.
    pub index: usize,
}

/// A cable from an output to an input.
#[derive(Debug, Clone)]
pub struct CableView {
    /// Source module and output index.
    pub from: (u32, usize),
    /// Destination module and input index.
    pub to: (u32, usize),
    /// Colour at the source end.
    pub colour_from: Color,
    /// Colour at the destination end.
    pub colour_to: Color,
    /// True if the engine runs this cable through a one-sample delay to
    /// break a feedback loop. Drawn dashed.
    pub delayed: bool,
}

/// Everything the canvas draws.
///
/// Each change gives the scene a new generation number. The canvas rebuilds
/// its GPU data only when the generation changes, so a still patch costs
/// almost nothing to draw.
#[derive(Debug, Clone)]
pub struct Scene {
    modules: Vec<ModuleView>,
    index: HashMap<u32, usize>,
    cables: Vec<CableView>,
    generation: u64,
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
    NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
}

impl Default for Scene {
    fn default() -> Self {
        Self::new(Vec::new(), Vec::new())
    }
}

impl Scene {
    /// Creates a scene. Modules are drawn in this order, last on top.
    /// Cables that refer to unknown modules or ports are skipped when drawn.
    pub fn new(modules: Vec<ModuleView>, cables: Vec<CableView>) -> Self {
        let index = modules.iter().enumerate().map(|(i, m)| (m.id, i)).collect();
        Self {
            modules,
            index,
            cables,
            generation: next_generation(),
        }
    }

    /// Changes whenever the scene changes. Unique across all scenes.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// All modules, in drawing order.
    pub fn modules(&self) -> &[ModuleView] {
        &self.modules
    }

    /// All cables.
    pub fn cables(&self) -> &[CableView] {
        &self.cables
    }

    /// One module by ID.
    pub fn module(&self, id: u32) -> Option<&ModuleView> {
        self.index.get(&id).map(|&i| &self.modules[i])
    }

    /// Moves modules by a world offset.
    pub fn move_modules(&mut self, ids: &[u32], delta: Vector) {
        for id in ids {
            if let Some(&i) = self.index.get(id) {
                self.modules[i].position += delta;
            }
        }
        self.generation = next_generation();
    }

    /// Centre of a port, if it exists.
    pub fn port_position(&self, port: PortRef) -> Option<Point> {
        let module = self.module(port.module)?;
        if port.output {
            (port.index < module.outputs.len()).then(|| module.output_position(port.index))
        } else {
            (port.index < module.inputs.len()).then(|| module.input_position(port.index))
        }
    }

    /// The port view, if it exists.
    pub fn port(&self, port: PortRef) -> Option<&PortView> {
        let module = self.module(port.module)?;
        if port.output {
            module.outputs.get(port.index)
        } else {
            module.inputs.get(port.index)
        }
    }

    /// The bounding box of all modules, or `None` for an empty scene.
    pub fn bounds(&self) -> Option<Rectangle> {
        self.modules.iter().map(ModuleView::bounds).reduce(|a, b| a.union(&b))
    }

    /// What is under a world point. Ports win over tiles, because they sit
    /// on the tile edges. Tiles drawn later win over earlier ones.
    pub fn hit(&self, point: Point, port_radius: f32) -> Hit {
        let r2 = port_radius * port_radius;
        let near = |p: Point| {
            let d = point - p;
            d.x * d.x + d.y * d.y <= r2
        };
        let mut best: Option<(f32, PortRef)> = None;
        for module in self.modules.iter().rev() {
            let b = module.bounds();
            // Quick reject: ports stick out of the tile by at most the radius.
            if point.x < b.x - port_radius
                || point.x > b.x + b.width + port_radius
                || point.y < b.y
                || point.y > b.y + b.height
            {
                continue;
            }
            let ports = (0..module.inputs.len())
                .map(|i| {
                    (
                        module.input_position(i),
                        PortRef {
                            module: module.id,
                            output: false,
                            index: i,
                        },
                    )
                })
                .chain((0..module.outputs.len()).map(|i| {
                    (
                        module.output_position(i),
                        PortRef {
                            module: module.id,
                            output: true,
                            index: i,
                        },
                    )
                }));
            for (p, port) in ports {
                if near(p) {
                    let d = point - p;
                    let dist = d.x * d.x + d.y * d.y;
                    if best.is_none_or(|(bd, _)| dist < bd) {
                        best = Some((dist, port));
                    }
                }
            }
        }
        if let Some((_, port)) = best {
            return Hit::Port(port);
        }
        for module in self.modules.iter().rev() {
            if module.bounds().contains(point) {
                return Hit::Module(module.id);
            }
        }
        // Cables last, with a tighter tolerance than ports: they are thin,
        // and empty canvas near a cable should still start a selection box.
        if let Some(index) = self.cable_at(point, port_radius * 0.5) {
            return Hit::Cable(index);
        }
        Hit::Nothing
    }

    /// The topmost cable within `tolerance` of a world point.
    fn cable_at(&self, point: Point, tolerance: f32) -> Option<usize> {
        let t2 = tolerance * tolerance;
        self.cables.iter().enumerate().rev().find_map(|(index, cable)| {
            let from = self.port_position(PortRef {
                module: cable.from.0,
                output: true,
                index: cable.from.1,
            })?;
            let to = self.port_position(PortRef {
                module: cable.to.0,
                output: false,
                index: cable.to.1,
            })?;
            let curve = cable_curve(from, to);
            // Quick reject: a cubic Bezier lies inside its control points' box.
            let (min_x, max_x) = curve
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
            let (min_y, max_y) = curve
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
            if point.x < min_x - tolerance
                || point.x > max_x + tolerance
                || point.y < min_y - tolerance
                || point.y > max_y + tolerance
            {
                return None;
            }
            let mut previous = from;
            for i in 1..=CABLE_SEGMENTS {
                let next = bezier(&curve, i as f32 / CABLE_SEGMENTS as f32);
                if distance_squared_to_segment(point, previous, next) <= t2 {
                    return Some(index);
                }
                previous = next;
            }
            None
        })
    }

    /// The output port a click on `hit` should probe: the port itself for an
    /// output, the source of the cable for a connected input or a cable.
    /// `None` for anything else.
    pub fn probe_target(&self, hit: Hit) -> Option<PortRef> {
        let from = match hit {
            Hit::Port(port) if port.output => return Some(port),
            Hit::Port(port) => self.cables.iter().find(|c| c.to == (port.module, port.index))?.from,
            Hit::Cable(index) => self.cables.get(index)?.from,
            Hit::Module(_) | Hit::Nothing => return None,
        };
        Some(PortRef {
            module: from.0,
            output: true,
            index: from.1,
        })
    }

    /// IDs of modules whose tiles intersect a world rectangle.
    pub fn modules_in(&self, area: Rectangle) -> Vec<u32> {
        self.modules
            .iter()
            .filter(|m| m.bounds().intersects(&area))
            .map(|m| m.id)
            .collect()
    }
}

/// Live signal levels of output ports, from 0.0 (silent) to 1.0 (full).
///
/// The application fills this from engine meters once per frame. The canvas
/// draws a glow on each cable and port from its source output's level. It is
/// kept apart from [`Scene`] on purpose: levels change every frame, and a new
/// scene generation would rebuild all GPU data (ADR-0010).
#[derive(Debug, Clone, Default)]
pub struct PortLevels {
    levels: HashMap<(u32, usize), f32>,
}

impl PortLevels {
    /// Sets the level of an output port. Values are clamped to 0.0..=1.0.
    pub fn set(&mut self, module: u32, output: usize, level: f32) {
        self.levels.insert((module, output), level.clamp(0.0, 1.0));
    }

    /// The level of an output port, 0.0 if unknown.
    pub fn get(&self, module: u32, output: usize) -> f32 {
        self.levels.get(&(module, output)).copied().unwrap_or(0.0)
    }

    /// Forgets every level.
    pub fn clear(&mut self) {
        self.levels.clear();
    }

    /// True if any port is above silence.
    pub fn any_active(&self) -> bool {
        self.levels.values().any(|&l| l > 0.0)
    }

    /// Every port with a level above zero.
    pub fn iter(&self) -> impl Iterator<Item = ((u32, usize), f32)> + '_ {
        self.levels.iter().filter(|(_, l)| **l > 0.0).map(|(&k, &l)| (k, l))
    }
}

/// Result of [`Scene::hit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A port.
    Port(PortRef),
    /// A module tile (not on a port).
    Module(u32),
    /// A cable, by index in [`Scene::cables`].
    Cable(usize),
    /// Empty canvas.
    Nothing,
}

/// Segments per cable. Must match `SEGMENTS` in `canvas.wgsl`.
pub(crate) const CABLE_SEGMENTS: u32 = 32;

/// Control points of a cable's cubic Bezier curve.
///
/// Must match `vs_cable` in `canvas.wgsl`, so a cable is hit where it is
/// drawn. Horizontal tangents at both ends; cables that run backwards (right
/// to left, usually feedback) loop out and sag below the modules.
pub(crate) fn cable_curve(from: Point, to: Point) -> [Point; 4] {
    let span = (to.x - from.x).abs();
    let (reach, sag) = if to.x < from.x {
        (40.0 + span * 0.15, 30.0 + (span * 0.2).min(120.0))
    } else {
        ((span * 0.5).max(30.0), 0.0)
    };
    [
        from,
        Point::new(from.x + reach, from.y + sag),
        Point::new(to.x - reach, to.y + sag),
        to,
    ]
}

fn bezier(c: &[Point; 4], t: f32) -> Point {
    let u = 1.0 - t;
    let (a, b, cc, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point::new(
        a * c[0].x + b * c[1].x + cc * c[2].x + d * c[3].x,
        a * c[0].y + b * c[1].y + cc * c[2].y + d * c[3].y,
    )
}

fn distance_squared_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let length2 = ab.x * ab.x + ab.y * ab.y;
    let t = if length2 > 0.0 {
        ((ap.x * ab.x + ap.y * ab.y) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let closest = Point::new(a.x + ab.x * t, a.y + ab.y * t);
    let d = p - closest;
    d.x * d.x + d.y * d.y
}

/// Rounds a world point to the snap grid.
pub fn snap(point: Point) -> Point {
    Point::new((point.x / GRID).round() * GRID, (point.y / GRID).round() * GRID)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(label: &str) -> PortView {
        PortView {
            label: label.into(),
            colour: Color::WHITE,
            shape: PortShape::Round,
        }
    }

    fn module(id: u32, x: f32, y: f32) -> ModuleView {
        ModuleView {
            id,
            title: format!("m{id}"),
            accent: Color::BLACK,
            position: Point::new(x, y),
            inputs: vec![port("a"), port("b")],
            outputs: vec![port("out")],
        }
    }

    #[test]
    fn height_snaps_to_grid() {
        let m = module(1, 0.0, 0.0);
        assert_eq!(m.height() % GRID, 0.0);
        assert!(m.height() >= HEADER + 2.0 * ROW);
    }

    #[test]
    fn hits_ports_before_tiles() {
        let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 300.0, 0.0)], Vec::new());
        let m = scene.module(1).unwrap();
        let input = m.input_position(1);
        assert_eq!(
            scene.hit(input + Vector::new(2.0, 1.0), PORT_HIT_RADIUS),
            Hit::Port(PortRef {
                module: 1,
                output: false,
                index: 1
            })
        );
        let output = m.output_position(0);
        assert_eq!(
            scene.hit(output, PORT_HIT_RADIUS),
            Hit::Port(PortRef {
                module: 1,
                output: true,
                index: 0
            })
        );
        assert_eq!(scene.hit(Point::new(60.0, 10.0), PORT_HIT_RADIUS), Hit::Module(1));
        assert_eq!(scene.hit(Point::new(200.0, 10.0), PORT_HIT_RADIUS), Hit::Nothing);
    }

    #[test]
    fn later_tiles_win() {
        let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 40.0, 0.0)], Vec::new());
        assert_eq!(scene.hit(Point::new(60.0, 10.0), PORT_HIT_RADIUS), Hit::Module(2));
    }

    #[test]
    fn moving_changes_generation() {
        let mut scene = Scene::new(vec![module(1, 0.0, 0.0)], Vec::new());
        let before = scene.generation();
        scene.move_modules(&[1], Vector::new(GRID, 0.0));
        assert_ne!(scene.generation(), before);
        assert_eq!(scene.module(1).unwrap().position, Point::new(GRID, 0.0));
    }

    #[test]
    fn hits_cables_where_they_are_drawn() {
        let cable = CableView {
            from: (1, 0),
            to: (2, 0),
            colour_from: Color::WHITE,
            colour_to: Color::WHITE,
            delayed: false,
        };
        let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 400.0, 200.0)], vec![cable]);
        let from = scene.module(1).unwrap().output_position(0);
        let to = scene.module(2).unwrap().input_position(0);
        // The middle of a forward curve is the midpoint of its ends.
        let middle = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
        assert_eq!(scene.hit(middle, PORT_HIT_RADIUS), Hit::Cable(0));
        assert_eq!(
            scene.hit(middle + Vector::new(0.0, 40.0), PORT_HIT_RADIUS),
            Hit::Nothing
        );
    }

    #[test]
    fn probe_targets_are_outputs() {
        let cable = CableView {
            from: (1, 0),
            to: (2, 1),
            colour_from: Color::WHITE,
            colour_to: Color::WHITE,
            delayed: false,
        };
        let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 300.0, 0.0)], vec![cable]);
        let source = PortRef {
            module: 1,
            output: true,
            index: 0,
        };
        assert_eq!(scene.probe_target(Hit::Port(source)), Some(source));
        assert_eq!(scene.probe_target(Hit::Cable(0)), Some(source));
        let connected = PortRef {
            module: 2,
            output: false,
            index: 1,
        };
        assert_eq!(scene.probe_target(Hit::Port(connected)), Some(source));
        let free = PortRef { index: 0, ..connected };
        assert_eq!(scene.probe_target(Hit::Port(free)), None);
        assert_eq!(scene.probe_target(Hit::Cable(7)), None);
        assert_eq!(scene.probe_target(Hit::Module(1)), None);
    }

    #[test]
    fn port_levels_clamp() {
        let mut levels = PortLevels::default();
        assert!(!levels.any_active());
        levels.set(1, 0, 3.0);
        levels.set(2, 0, 0.0);
        assert_eq!(levels.get(1, 0), 1.0);
        assert_eq!(levels.get(9, 9), 0.0);
        assert_eq!(levels.iter().count(), 1);
        assert!(levels.any_active());
    }

    #[test]
    fn box_selection() {
        let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 300.0, 0.0)], Vec::new());
        let area = Rectangle::new(Point::new(-10.0, -10.0), iced::Size::new(150.0, 20.0));
        assert_eq!(scene.modules_in(area), vec![1]);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap(Point::new(13.0, -7.0)), Point::new(12.0, -12.0));
    }
}
