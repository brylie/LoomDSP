//! The patch canvas widget: input handling and drawing.

use crate::camera::Camera;
use crate::gpu::{Cable, CanvasPrimitive, Geometry, Overlay, Shape};
use crate::scene::{GRID, HEADER, Hit, PORT_HIT_RADIUS, PORT_RADIUS, PortRef, ROW, Scene, TILE_WIDTH};
use crate::theme::Palette;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Text};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector};
use iced::{alignment, keyboard, mouse, window};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

/// Titles are drawn when they would be at least this tall on screen.
const TITLE_MIN_PX: f32 = 6.5;
/// Port labels are drawn when they would be at least this tall on screen.
const LABEL_MIN_PX: f32 = 7.5;
/// Title size in world units.
const TITLE_SIZE: f32 = 11.0;
/// Port label size in world units.
const LABEL_SIZE: f32 = 9.0;

/// Opacity of cables that are not in focus. It never changes with hover or
/// selection, so moving the pointer only changes the cables it points at,
/// never the whole patch.
const CABLE_OPACITY: f32 = 0.4;

/// A gesture on the canvas. The application applies it to its model.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasEvent {
    /// Replace the selection.
    Select(Vec<u32>),
    /// Move modules by a world offset (always whole grid steps).
    Move {
        /// Modules to move.
        ids: Vec<u32>,
        /// Offset in world units.
        delta: Vector,
    },
    /// Connect an output to an input (replacing any cable into that input).
    Connect {
        /// The output.
        from: PortRef,
        /// The input.
        to: PortRef,
    },
    /// Remove the cable into this input.
    Disconnect(PortRef),
    /// Delete modules.
    Delete(Vec<u32>),
}

/// The patch canvas. Draws a [`Scene`] and reports gestures as
/// [`CanvasEvent`]s.
pub struct PatchCanvas<'a, Message> {
    scene: &'a Scene,
    selection: &'a BTreeSet<u32>,
    palette: Palette,
    on_event: Box<dyn Fn(CanvasEvent) -> Message + 'a>,
    frame_key: u64,
    animate: bool,
}

impl<'a, Message> PatchCanvas<'a, Message> {
    /// Creates a canvas for a scene and the current selection.
    pub fn new(scene: &'a Scene, selection: &'a BTreeSet<u32>, on_event: impl Fn(CanvasEvent) -> Message + 'a) -> Self {
        Self {
            scene,
            selection,
            palette: Palette::default(),
            on_event: Box::new(on_event),
            frame_key: 0,
            animate: false,
        }
    }

    /// Sets the colours.
    pub fn palette(mut self, palette: Palette) -> Self {
        self.palette = palette;
        self
    }

    /// The canvas frames the whole scene when it first appears and whenever
    /// this key changes (for example after loading a patch).
    pub fn frame_key(mut self, key: u64) -> Self {
        self.frame_key = key;
        self
    }

    /// Moves the camera continuously in a slow loop. For measuring frame
    /// rate while panning and zooming (ADR-0010 performance gates).
    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }

    fn emit(&self, shell: &mut Shell<'_, Message>, event: CanvasEvent) {
        shell.publish((self.on_event)(event));
    }
}

#[derive(Debug, Clone, Default)]
enum Interaction {
    #[default]
    Idle,
    Panning {
        last: Point,
    },
    Moving {
        ids: Vec<u32>,
        start: Point,
        applied: Vector,
    },
    Wiring {
        from: PortRef,
    },
    Selecting {
        start: Point,
        current: Point,
        base: BTreeSet<u32>,
    },
}

#[derive(Default)]
struct State {
    camera: Camera,
    framed_key: Option<u64>,
    interaction: Interaction,
    hover: Option<Hit>,
    /// Cursor in world coordinates, while over the canvas or dragging.
    cursor: Option<Point>,
    modifiers: keyboard::Modifiers,
    geometry: RefCell<Option<Arc<Geometry>>>,
    animation: Option<(Instant, Camera)>,
}

impl State {
    fn hit_radius(&self) -> f32 {
        // At least 7 screen pixels, so ports stay easy to grab when zoomed
        // out. At most a quarter of a tile's width, so the middle of a tile
        // always grabs the tile, not a port.
        PORT_HIT_RADIUS.max(7.0 / self.camera.zoom).min(TILE_WIDTH * 0.25)
    }
}

fn relative(cursor: mouse::Cursor, bounds: Rectangle) -> Option<Point> {
    cursor.position().map(|p| Point::new(p.x - bounds.x, p.y - bounds.y))
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for PatchCanvas<'_, Message>
where
    Renderer: iced_wgpu::primitive::Renderer + text::Renderer<Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let over = cursor.is_over(bounds);
        let screen = relative(cursor, bounds);

        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                if state.framed_key != Some(self.frame_key) && bounds.width > 0.0 {
                    if let Some(world) = self.scene.bounds() {
                        state.camera.frame(world, bounds.size());
                    }
                    state.framed_key = Some(self.frame_key);
                    state.animation = None;
                    shell.request_redraw();
                }
                if self.animate {
                    let (start, base) = *state.animation.get_or_insert((*now, state.camera));
                    let t = (*now - start).as_secs_f32();
                    state.camera = animated(base, bounds.size(), t);
                    shell.request_redraw();
                } else if let Some((_, base)) = state.animation.take() {
                    state.camera = base;
                    shell.request_redraw();
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) if over => {
                use keyboard::key::Named;
                match key.as_ref() {
                    keyboard::Key::Named(Named::Delete | Named::Backspace) if !self.selection.is_empty() => {
                        self.emit(shell, CanvasEvent::Delete(self.selection.iter().copied().collect()));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Home) | keyboard::Key::Character("f") => {
                        if let Some(world) = self.scene.bounds() {
                            state.camera.frame(world, bounds.size());
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    keyboard::Key::Character("a") if modifiers.command() => {
                        let all = self.scene.modules().iter().map(|m| m.id).collect();
                        self.emit(shell, CanvasEvent::Select(all));
                        shell.capture_event();
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if over => {
                let Some(screen) = screen else { return };
                match *delta {
                    // A mouse wheel zooms.
                    mouse::ScrollDelta::Lines { y, .. } => {
                        state.camera.zoom_at(screen, 1.15_f32.powf(y));
                    }
                    // A trackpad pans, or zooms with Ctrl or Cmd held.
                    mouse::ScrollDelta::Pixels { x, y } => {
                        if state.modifiers.command() || state.modifiers.control() {
                            state.camera.zoom_at(screen, (y * 0.01).exp());
                        } else {
                            state.camera.pan(Vector::new(x, y));
                        }
                    }
                }
                state.cursor = Some(state.camera.to_world(screen));
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) if over => {
                let Some(screen) = screen else { return };
                let world = state.camera.to_world(screen);
                match button {
                    mouse::Button::Middle | mouse::Button::Right => {
                        state.interaction = Interaction::Panning { last: screen };
                    }
                    mouse::Button::Left => {
                        self.press(state, world, shell);
                    }
                    _ => return,
                }
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(screen) = screen else { return };
                let world = state.camera.to_world(screen);
                state.cursor = Some(world);
                let mut redraw = false;
                match &mut state.interaction {
                    Interaction::Idle => {}
                    Interaction::Panning { last } => {
                        let delta = screen - *last;
                        *last = screen;
                        state.camera.pan(delta);
                        redraw = true;
                    }
                    Interaction::Moving { ids, start, applied } => {
                        let total = world - *start;
                        let snapped = Vector::new((total.x / GRID).round() * GRID, (total.y / GRID).round() * GRID);
                        if snapped != *applied {
                            let delta = snapped - *applied;
                            *applied = snapped;
                            let ids = ids.clone();
                            self.emit(shell, CanvasEvent::Move { ids, delta });
                        }
                    }
                    Interaction::Wiring { .. } => redraw = true,
                    Interaction::Selecting { current, .. } => {
                        *current = world;
                        redraw = true;
                    }
                }
                let hover = over.then(|| self.scene.hit(world, state.hit_radius()));
                if hover != state.hover {
                    state.hover = hover;
                    redraw = true;
                }
                if redraw {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                let interaction = std::mem::take(&mut state.interaction);
                let world = screen.map(|s| state.camera.to_world(s));
                match interaction {
                    Interaction::Wiring { from } => {
                        if let Some(Hit::Port(to)) = world.map(|w| self.scene.hit(w, state.hit_radius())) {
                            let pair = if from.output { (from, to) } else { (to, from) };
                            if pair.0.output && !pair.1.output {
                                self.emit(
                                    shell,
                                    CanvasEvent::Connect {
                                        from: pair.0,
                                        to: pair.1,
                                    },
                                );
                            }
                        }
                    }
                    Interaction::Selecting { start, current, base } => {
                        let area = Rectangle::new(
                            Point::new(start.x.min(current.x), start.y.min(current.y)),
                            Size::new((start.x - current.x).abs(), (start.y - current.y).abs()),
                        );
                        let mut selection = base;
                        selection.extend(self.scene.modules_in(area));
                        self.emit(shell, CanvasEvent::Select(selection.into_iter().collect()));
                    }
                    Interaction::Idle | Interaction::Panning { .. } | Interaction::Moving { .. } => {}
                }
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorLeft) if state.hover.take().is_some() => {
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        match state.interaction {
            Interaction::Panning { .. } | Interaction::Moving { .. } => mouse::Interaction::Grabbing,
            Interaction::Wiring { .. } => mouse::Interaction::Crosshair,
            Interaction::Selecting { .. } => mouse::Interaction::Crosshair,
            Interaction::Idle if cursor.is_over(layout.bounds()) => match state.hover {
                Some(Hit::Port(_)) => mouse::Interaction::Pointer,
                Some(Hit::Module(_)) => mouse::Interaction::Grab,
                _ => mouse::Interaction::default(),
            },
            Interaction::Idle => mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();

        let geometry = {
            let mut cache = state.geometry.borrow_mut();
            match cache.as_ref() {
                Some(g) if g.generation == self.scene.generation() => g.clone(),
                _ => {
                    let built = Arc::new(Geometry::build(self.scene, &self.palette));
                    *cache = Some(built.clone());
                    built
                }
            }
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            self.palette.background,
        );
        renderer.draw_primitive(
            bounds,
            CanvasPrimitive {
                geometry,
                overlay: self.overlay(state),
                camera: state.camera,
                cable_dim: CABLE_OPACITY,
            },
        );
        self.draw_text(state, renderer, bounds);
    }
}

impl<Message> PatchCanvas<'_, Message> {
    fn press(&self, state: &mut State, world: Point, shell: &mut Shell<'_, Message>) {
        let shift = state.modifiers.shift();
        match self.scene.hit(world, state.hit_radius()) {
            Hit::Port(port) if !port.output => {
                // Grabbing a connected input picks up its cable, like pulling
                // a patch cable out of a jack.
                let source = self
                    .scene
                    .cables()
                    .iter()
                    .find(|c| c.to == (port.module, port.index))
                    .map(|c| PortRef {
                        module: c.from.0,
                        output: true,
                        index: c.from.1,
                    });
                if let Some(source) = source {
                    self.emit(shell, CanvasEvent::Disconnect(port));
                    state.interaction = Interaction::Wiring { from: source };
                } else {
                    state.interaction = Interaction::Wiring { from: port };
                }
            }
            Hit::Port(port) => {
                state.interaction = Interaction::Wiring { from: port };
            }
            Hit::Module(id) => {
                let mut selection = self.selection.clone();
                if shift {
                    if !selection.remove(&id) {
                        selection.insert(id);
                    }
                } else if !selection.contains(&id) {
                    selection = BTreeSet::from([id]);
                }
                if &selection != self.selection {
                    self.emit(shell, CanvasEvent::Select(selection.iter().copied().collect()));
                }
                if selection.contains(&id) {
                    state.interaction = Interaction::Moving {
                        ids: selection.into_iter().collect(),
                        start: world,
                        applied: Vector::ZERO,
                    };
                }
            }
            Hit::Cable(_) | Hit::Nothing => {
                let base = if shift { self.selection.clone() } else { BTreeSet::new() };
                state.interaction = Interaction::Selecting {
                    start: world,
                    current: world,
                    base,
                };
            }
        }
    }

    /// The module tile under the cursor, if any (not counting its ports).
    fn hovered_module(state: &State) -> Option<u32> {
        match state.hover? {
            Hit::Module(id) => Some(id),
            _ => None,
        }
    }

    /// The port under the cursor, if any.
    fn hovered_port(state: &State) -> Option<PortRef> {
        match state.hover? {
            Hit::Port(port) => Some(port),
            _ => None,
        }
    }

    /// The cable under the cursor, if any.
    fn hovered_cable(state: &State) -> Option<usize> {
        match state.hover? {
            Hit::Cable(index) => Some(index),
            _ => None,
        }
    }

    fn overlay(&self, state: &State) -> Overlay {
        let mut overlay = Overlay::default();
        let p = &self.palette;

        // Cables in focus come to the front at full opacity: the cable under
        // the cursor, the cables on the port under the cursor, and the cables
        // of the module under the cursor and of selected modules. Everything
        // else stays at the same dim level, so the patch never flickers.
        let hovered = Self::hovered_module(state);
        let hovered_port = Self::hovered_port(state);
        let hovered_cable = Self::hovered_cable(state);
        let module_in_focus = |id: u32| self.selection.contains(&id) || hovered == Some(id);
        let on_port = |cable: &crate::scene::CableView| {
            hovered_port.is_some_and(|p| {
                if p.output {
                    cable.from == (p.module, p.index)
                } else {
                    cable.to == (p.module, p.index)
                }
            })
        };
        if !self.selection.is_empty() || hovered.is_some() || hovered_port.is_some() || hovered_cable.is_some() {
            for (index, cable) in self.scene.cables().iter().enumerate() {
                let focused = hovered_cable == Some(index)
                    || on_port(cable)
                    || module_in_focus(cable.from.0)
                    || module_in_focus(cable.to.0);
                if !focused {
                    continue;
                }
                let from = PortRef {
                    module: cable.from.0,
                    output: true,
                    index: cable.from.1,
                };
                let to = PortRef {
                    module: cable.to.0,
                    output: false,
                    index: cable.to.1,
                };
                if let (Some(a), Some(b)) = (self.scene.port_position(from), self.scene.port_position(to)) {
                    overlay
                        .cables
                        .push(Cable::new(a, b, cable.colour_from, cable.colour_to, cable.delayed).focused());
                }
            }
        }

        for id in self.selection {
            if let Some(module) = self.scene.module(*id) {
                let b = module.bounds().expand(3.0);
                overlay
                    .shapes
                    .push(Shape::new(b, 7.0, Color::TRANSPARENT, p.selection, 1.5));
            }
        }
        if let Some(Hit::Port(port)) = state.hover
            && let Some(centre) = self.scene.port_position(port)
        {
            let r = PORT_RADIUS + 3.0;
            let rect = Rectangle::new(Point::new(centre.x - r, centre.y - r), Size::new(2.0 * r, 2.0 * r));
            overlay
                .shapes
                .push(Shape::new(rect, r, Color::TRANSPARENT, p.hover, 1.2));
        }
        match &state.interaction {
            Interaction::Wiring { from } => {
                if let (Some(start), Some(port), Some(cursor)) =
                    (self.scene.port_position(*from), self.scene.port(*from), state.cursor)
                {
                    // Snap the loose end onto a port under the cursor.
                    let end = match state.hover {
                        Some(Hit::Port(target)) if target.output != from.output => {
                            self.scene.port_position(target).unwrap_or(cursor)
                        }
                        _ => cursor,
                    };
                    let cable = if from.output {
                        Cable::new(start, end, port.colour, port.colour, false)
                    } else {
                        Cable::new(end, start, port.colour, port.colour, false)
                    }
                    .focused();
                    overlay.cables.push(cable);
                }
            }
            Interaction::Selecting { start, current, .. } => {
                let rect = Rectangle::new(
                    Point::new(start.x.min(current.x), start.y.min(current.y)),
                    Size::new((start.x - current.x).abs(), (start.y - current.y).abs()),
                );
                overlay
                    .shapes
                    .push(Shape::new(rect, 2.0, p.marquee, p.selection, 1.0 / state.camera.zoom));
            }
            _ => {}
        }
        overlay
    }

    fn draw_text<Renderer>(&self, state: &State, renderer: &mut Renderer, bounds: Rectangle)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        let zoom = state.camera.zoom;
        // Quantise sizes so zooming does not create a new text layout every
        // frame for every label.
        let quantise = |size: f32| (size * 2.0).round() / 2.0;
        let title_px = quantise(TITLE_SIZE * zoom);
        if title_px < TITLE_MIN_PX {
            return;
        }
        let label_px = quantise(LABEL_SIZE * zoom);
        let show_labels = label_px >= LABEL_MIN_PX;
        let visible = state.camera.visible(bounds.size());
        let font = renderer.default_font();
        let to_screen = |world: Point| {
            let s = state.camera.to_screen(world);
            Point::new(bounds.x + s.x, bounds.y + s.y)
        };
        let text = |content: &str, size: f32, width: f32, align: text::Alignment| Text {
            content: content.to_string(),
            bounds: Size::new(width, size * 1.4),
            size: Pixels(size),
            line_height: text::LineHeight::Relative(1.2),
            font,
            align_x: align,
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        };

        for module in self.scene.modules() {
            let tile = module.bounds();
            if !tile.intersects(&visible) {
                continue;
            }
            let screen_tile = Rectangle::new(to_screen(tile.position()), tile.size() * zoom);
            let Some(clip) = screen_tile.intersection(&bounds) else {
                continue;
            };
            let pad = 7.0 * zoom;
            let title_pos = to_screen(Point::new(tile.x, tile.y + HEADER * 0.5)) + Vector::new(pad, 0.0);
            renderer.fill_text(
                text(
                    &module.title,
                    title_px,
                    TILE_WIDTH * zoom - 2.0 * pad,
                    text::Alignment::Left,
                ),
                title_pos,
                self.palette.text,
                clip,
            );
            if !show_labels {
                continue;
            }
            let half = (TILE_WIDTH * 0.5 - 10.0) * zoom;
            for (i, port) in module.inputs.iter().enumerate() {
                let y = tile.y + HEADER + ROW * (i as f32 + 0.5);
                let at = to_screen(Point::new(tile.x + 9.0, y));
                renderer.fill_text(
                    text(&port.label, label_px, half, text::Alignment::Left),
                    at,
                    self.palette.text_dim,
                    clip,
                );
            }
            for (i, port) in module.outputs.iter().enumerate() {
                let y = tile.y + HEADER + ROW * (i as f32 + 0.5);
                let at = to_screen(Point::new(tile.x + TILE_WIDTH - 9.0, y));
                renderer.fill_text(
                    text(&port.label, label_px, half, text::Alignment::Right),
                    at,
                    self.palette.text_dim,
                    clip,
                );
            }
        }
    }
}

/// A slow figure-eight pan with a zoom swing, around the starting view.
fn animated(base: Camera, size: Size, t: f32) -> Camera {
    let centre = base.to_world(Point::new(size.width * 0.5, size.height * 0.5));
    let zoom = (base.zoom * (1.0 + 0.4 * (t * 0.45).sin())).clamp(crate::camera::MIN_ZOOM, crate::camera::MAX_ZOOM);
    let reach = 0.25 * size.width / base.zoom;
    let target = Point::new(
        centre.x + reach * (t * 0.31).sin(),
        centre.y + 0.5 * reach * (t * 0.62).sin(),
    );
    Camera {
        origin: Point::new(target.x - size.width * 0.5 / zoom, target.y - size.height * 0.5 / zoom),
        zoom,
    }
}

impl<'a, Message, Theme, Renderer> From<PatchCanvas<'a, Message>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Renderer: iced_wgpu::primitive::Renderer + text::Renderer<Font = Font> + 'a,
    Theme: 'a,
{
    fn from(canvas: PatchCanvas<'a, Message>) -> Self {
        Element::new(canvas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{CableView, ModuleView, PortShape, PortView};

    fn module(id: u32, x: f32) -> ModuleView {
        let port = || PortView {
            label: "p".into(),
            colour: Color::WHITE,
            shape: PortShape::Round,
        };
        ModuleView {
            id,
            title: String::new(),
            accent: Color::BLACK,
            position: Point::new(x, 0.0),
            inputs: vec![port()],
            outputs: vec![port()],
        }
    }

    fn cable(from: u32, to: u32) -> CableView {
        CableView {
            from: (from, 0),
            to: (to, 0),
            colour_from: Color::WHITE,
            colour_to: Color::WHITE,
            delayed: false,
        }
    }

    /// Modules 1 -> 2 -> 3, so selecting 1 focuses one cable and selecting
    /// 2 focuses both.
    fn chain() -> Scene {
        Scene::new(
            vec![module(1, 0.0), module(2, 200.0), module(3, 400.0)],
            vec![cable(1, 2), cable(2, 3)],
        )
    }

    #[test]
    fn selected_modules_bring_their_cables_forward() {
        let scene = chain();
        let state = State::default();

        let none = BTreeSet::new();
        let canvas = PatchCanvas::new(&scene, &none, |e| e);
        assert!(canvas.overlay(&state).cables.is_empty());

        let first = BTreeSet::from([1]);
        let canvas = PatchCanvas::new(&scene, &first, |e| e);
        assert_eq!(canvas.overlay(&state).cables.len(), 1);

        let middle = BTreeSet::from([2]);
        let canvas = PatchCanvas::new(&scene, &middle, |e| e);
        assert_eq!(canvas.overlay(&state).cables.len(), 2);
    }

    #[test]
    fn tiles_stay_draggable_when_zoomed_out() {
        // At the smallest zoom, the port hit radius must not cover the whole
        // tile, or pressing anywhere on it would start a cable.
        let scene = chain();
        let mut state = State::default();
        state.camera.zoom = crate::camera::MIN_ZOOM;
        let centre = scene.module(2).unwrap().bounds().center();
        assert_eq!(scene.hit(centre, state.hit_radius()), Hit::Module(2));
    }

    #[test]
    fn hovering_a_module_brings_its_cables_forward() {
        let scene = chain();
        let none = BTreeSet::new();
        let canvas = PatchCanvas::new(&scene, &none, |e| e);
        let state = State {
            hover: Some(Hit::Module(3)),
            ..State::default()
        };
        assert_eq!(canvas.overlay(&state).cables.len(), 1);
    }

    #[test]
    fn hovering_a_port_brings_only_its_cables_forward() {
        let scene = chain();
        let none = BTreeSet::new();
        let canvas = PatchCanvas::new(&scene, &none, |e| e);
        // Module 2's input carries cable 1 -> 2; its output carries 2 -> 3.
        let state = State {
            hover: Some(Hit::Port(PortRef {
                module: 2,
                output: false,
                index: 0,
            })),
            ..State::default()
        };
        assert_eq!(canvas.overlay(&state).cables.len(), 1);
        // A port with no cable highlights nothing.
        let state = State {
            hover: Some(Hit::Port(PortRef {
                module: 1,
                output: false,
                index: 0,
            })),
            ..State::default()
        };
        assert!(canvas.overlay(&state).cables.is_empty());
    }

    #[test]
    fn hovering_a_cable_brings_only_that_cable_forward() {
        let scene = chain();
        let none = BTreeSet::new();
        let canvas = PatchCanvas::new(&scene, &none, |e| e);
        let state = State {
            hover: Some(Hit::Cable(1)),
            ..State::default()
        };
        assert_eq!(canvas.overlay(&state).cables.len(), 1);
    }
}
