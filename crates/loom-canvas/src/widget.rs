//! The patch canvas widget: input handling and drawing.

use crate::camera::Camera;
use crate::controls::{ControlGeometry, PanelValues, focus_ring, knob_drag, knob_step, radio_option, select_box};
use crate::gpu::{Cable, CanvasPrimitive, Geometry, Overlay, Shape};
use crate::hover::{HOVER_PADDING, HOVER_TEXT_PX, HoverTarget, HoverTimer};
use crate::scene::{
    BADGE_TEXT_SIZE, GRID, HEADER, Hit, HoverText, PORT_HIT_RADIUS, PORT_RADIUS, PortRef, Scene, TILE_WIDTH,
    WidgetView, WidgetViewKind,
};
use crate::theme::Palette;
use crate::tile_text::{self, Align, LABEL_SIZE, TITLE_SIZE, TextKind};
use crate::trace::{ScopeTraces, TraceGeometry};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Text};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector};
use iced::{alignment, keyboard, mouse, window};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod draw;
mod event;
mod hover_text;
mod input;
mod panel_input;

pub use event::CanvasEvent;

/// Two presses on the same control within this time reset it.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// How far in screen pixels the pointer may move between the two clicks of
/// a double-click.
const DOUBLE_CLICK_SLOP: f32 = 4.0;
/// How far a right press may move, in screen pixels, and still count as a
/// click that opens the probe menu rather than a pan.
const CLICK_SLOP: f32 = 4.0;
/// Height of a row in the choice popup, in screen pixels.
const SELECT_ROW_PX: f32 = 23.0;
/// Minimum popup width in screen pixels, so names stay legible at middle zoom.
const SELECT_WIDTH_PX: f32 = 124.0;
/// Arrow presses within this time share one undo gesture (panels.md).
const ARROW_GROUP: Duration = Duration::from_millis(500);

/// Titles are drawn when they would be at least this tall on screen.
const TITLE_MIN_PX: f32 = 6.5;
/// Port labels are drawn when they would be at least this tall on screen.
const LABEL_MIN_PX: f32 = 7.5;
/// The smallest zoom where port labels and captions are drawn. Opening a
/// patch and framing it (F, Home) never zoom out past it (#455).
const READABLE_ZOOM: f32 = LABEL_MIN_PX / LABEL_SIZE;

/// Opacity of cables that are not in focus. It never changes with hover or
/// selection, so moving the pointer only changes the cables it points at,
/// never the whole patch.
const CABLE_OPACITY: f32 = 0.4;

/// What MIDI learn mode shows on the canvas. While the canvas has one, a
/// click on a control arms it instead of changing it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LearnView {
    /// The armed control: module ID and widget index.
    pub armed: Option<(u32, usize)>,
    /// Mapping labels, such as `CC 74`, by module ID and widget index. They
    /// show in place of the control's caption.
    pub labels: BTreeMap<(u32, usize), String>,
}

/// The patch canvas. Draws a [`Scene`] and reports gestures as
/// [`CanvasEvent`]s.
pub struct PatchCanvas<'a, Message> {
    scene: &'a Scene,
    selection: &'a BTreeSet<u32>,
    values: Option<&'a PanelValues>,
    traces: Option<&'a ScopeTraces>,
    /// Outputs that currently carry a signal; their cable inputs follow.
    activity: Vec<(u32, usize)>,
    palette: Palette,
    on_event: Box<dyn Fn(CanvasEvent) -> Message + 'a>,
    frame_key: u64,
    browser_key: u64,
    animate: bool,
    interactive: bool,
    learn: Option<&'a LearnView>,
    /// The row selected in the modulation matrix, without changing the pin.
    matrix_line: Option<crate::scene::ModulationView>,
    /// Lets Escape hide a badge pin while the matrix owns keyboard focus.
    matrix_escape: bool,
    /// Leaves unmodified keys to the focused matrix while keeping pointer input active.
    matrix_keyboard: bool,
    still: Option<Still>,
    window_drag_line: bool,
}

/// How a still image of the canvas starts: its camera, the control with
/// keyboard focus, the pointer, the pinned badge, and a drag in progress.
/// See [`PatchCanvas::still`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Still {
    /// The view. It replaces the framing the canvas does when it first
    /// appears.
    pub camera: Camera,
    /// Module ID and widget index of the control with keyboard focus. It
    /// draws as after a Tab press: with the focus ring and its value.
    pub focus: Option<(u32, usize)>,
    /// Where the pointer is, in world coordinates. What is under it draws
    /// as hovered: the port ring, the cables and modulation lines in focus,
    /// and, during a wiring drag, the loose cable end and the drop
    /// highlight.
    pub pointer: Option<Point>,
    /// The output whose badge is pinned, as after a click on the badge.
    pub pinned: Option<PortRef>,
    /// A drag in progress.
    pub drag: Option<StillDrag>,
    /// Show hover text at once, as if the pointer or the keyboard focus had
    /// rested for the hover delay. A drag shows none.
    pub hover_text: bool,
}

/// A drag that a still image shows in progress. See [`Still::drag`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StillDrag {
    /// A cable being dragged from a port. Its loose end is at the pointer,
    /// or on the port under it.
    Wire {
        /// The port the cable starts at.
        from: PortRef,
        /// True when the cable was pulled off an input: `from` is the
        /// output it came from, and the scene no longer has the cable. A
        /// drop on a control then assigns nothing, so no control is
        /// highlighted.
        pulled: bool,
    },
    /// The range arc of this knob being dragged: module ID and widget
    /// index. The canvas draws no mark for it. The app shows the arc of
    /// the assignment being changed through [`PanelValues`].
    Depth(u32, usize),
}

/// Where the canvas is in the window and how it looks at the world. The
/// canvas reports it to widget operations as custom state, so an operation
/// can turn a world point (a port, a control) into window coordinates, for
/// example to send pointer events in a headless test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasView {
    /// The canvas bounds in the window.
    pub bounds: Rectangle,
    /// The current camera.
    pub camera: Camera,
}

/// What the pointer is on and what it does, as the canvas sees it. The
/// canvas reports it to widget operations as custom state, next to
/// [`CanvasView`], so the app can show it in its UI snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasPointer {
    /// What is under the pointer, while it is over the canvas.
    pub hover: Option<Hit>,
    /// The output whose badge is pinned, while its badge shows.
    pub pinned: Option<PortRef>,
    /// The drag in progress, if any.
    pub drag: Option<PointerDrag>,
}

/// A drag in progress on the canvas. See [`CanvasPointer::drag`].
#[derive(Debug, Clone, PartialEq)]
pub enum PointerDrag {
    /// The view is panned.
    Pan,
    /// Tiles are moved.
    Move(Vec<u32>),
    /// A growable tile is being resized.
    Resize(u32),
    /// A knob is turned: module ID and widget index.
    Knob(u32, usize),
    /// A button is held: module ID and widget index.
    Button(u32, usize),
    /// A cable is pulled from a port.
    Wire {
        /// The port the cable starts at.
        from: PortRef,
        /// True when the cable was pulled off an input.
        pulled: bool,
    },
    /// A knob's range arc is dragged: module ID and widget index.
    Depth(u32, usize),
    /// A selection box is drawn.
    Select,
}

impl PointerDrag {
    /// The drag an interaction is, or `None` when idle.
    fn of(interaction: &Interaction) -> Option<Self> {
        Some(match interaction {
            Interaction::Idle => return None,
            Interaction::Panning { .. } => Self::Pan,
            Interaction::Moving { ids, .. } => Self::Move(ids.clone()),
            Interaction::Resizing { module, .. } => Self::Resize(*module),
            Interaction::Turning { module, widget, .. } => Self::Knob(*module, *widget),
            Interaction::Holding { module, widget } => Self::Button(*module, *widget),
            Interaction::Wiring { from, pulled } => Self::Wire {
                from: *from,
                pulled: *pulled,
            },
            Interaction::Depth { module, widget, .. } => Self::Depth(*module, *widget),
            Interaction::Selecting { .. } => Self::Select,
        })
    }
}

impl CanvasView {
    /// A world point in window coordinates.
    pub fn to_window(&self, world: Point) -> Point {
        let s = self.camera.to_screen(world);
        Point::new(self.bounds.x + s.x, self.bounds.y + s.y)
    }
}

impl<'a, Message> PatchCanvas<'a, Message> {
    /// Creates a canvas for a scene and the current selection.
    pub fn new(scene: &'a Scene, selection: &'a BTreeSet<u32>, on_event: impl Fn(CanvasEvent) -> Message + 'a) -> Self {
        Self {
            scene,
            selection,
            values: None,
            traces: None,
            activity: Vec::new(),
            palette: Palette::default(),
            on_event: Box::new(on_event),
            frame_key: 0,
            browser_key: 0,
            animate: false,
            interactive: true,
            learn: None,
            matrix_line: None,
            matrix_escape: false,
            matrix_keyboard: false,
            still: None,
            window_drag_line: false,
        }
    }

    /// Starts the canvas with this camera and focus instead of framing the
    /// scene, so one drawn frame shows a chosen state. For saving the
    /// canvas as an image without a window (`loom-app --screenshot`).
    /// Only the first appearance uses it: later frames keep the state as
    /// usual.
    pub fn still(mut self, still: Still) -> Self {
        self.still = Some(still);
        self
    }

    /// Turns on MIDI learn mode, with what it shows.
    pub fn learn(mut self, learn: Option<&'a LearnView>) -> Self {
        self.learn = learn;
        self
    }

    /// Shows one matrix-selected assignment line without pinning its source.
    pub fn matrix_line(mut self, line: Option<crate::scene::ModulationView>) -> Self {
        self.matrix_line = line;
        self
    }

    /// Lets a focused matrix pass Escape to a pinned badge after its cell
    /// editor has closed.
    pub fn matrix_escape(mut self, enabled: bool) -> Self {
        self.matrix_escape = enabled;
        self
    }

    /// Leaves unmodified keyboard input to the matrix without blocking the pointer.
    pub fn matrix_keyboard(mut self, enabled: bool) -> Self {
        self.matrix_keyboard = enabled;
        self
    }

    /// Sets the live values of panel controls and lights.
    pub fn values(mut self, values: &'a PanelValues) -> Self {
        self.values = Some(values);
        self
    }

    /// The app draws output drags in its window layer, including the canvas.
    pub fn window_drag_line(mut self, enabled: bool) -> Self {
        self.window_drag_line = enabled;
        self
    }

    /// Sets the scope traces (`docs/design/panels.md`, Drawing the scope
    /// trace). They have their own GPU buffer: a new trace uploads only it.
    pub fn traces(mut self, traces: &'a ScopeTraces) -> Self {
        self.traces = Some(traces);
        self
    }

    /// Sets live port activity without changing the scene's GPU geometry.
    pub fn activity(mut self, outputs: impl IntoIterator<Item = (u32, usize)>) -> Self {
        self.activity = outputs.into_iter().collect();
        self
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

    /// The canvas sends [`CanvasEvent::OpenBrowser`] at the centre of the
    /// view whenever this key changes, but not when it first appears. The
    /// app uses it for a toolbar button, because only the canvas knows its
    /// camera.
    pub fn browser_key(mut self, key: u64) -> Self {
        self.browser_key = key;
        self
    }

    /// While false, the canvas ignores the keyboard and the pointer, and
    /// lets go of any hold or drag. For a pop-up that takes the keys, such
    /// as the module browser.
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// Moves the camera continuously in a slow loop. For measuring frame
    /// rate while panning and zooming (ADR-0010 performance gates).
    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }

    /// True if dropping `from` on this control makes an assignment: an
    /// output, a control that takes modulation, and not in MIDI learn mode,
    /// where touching a control arms it instead.
    fn takes_assignment(&self, from: PortRef, module: u32, widget: usize) -> bool {
        from.output
            && self.learn.is_none()
            && self
                .scene
                .module(module)
                .and_then(|m| m.panel.widgets.get(widget))
                .is_some_and(WidgetView::accepts_modulation)
    }

    /// Reports an output drag in window coordinates for the inspector layer.
    fn assign_drag(
        &self,
        from: PortRef,
        at: Option<Point>,
        camera: Camera,
        bounds: Rectangle,
        shell: &mut Shell<'_, Message>,
    ) {
        if let Some(world) = self.scene.port_position(from) {
            let screen = camera.to_screen(world);
            self.emit(
                shell,
                CanvasEvent::AssignDrag {
                    from,
                    start: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                    at,
                    zoom: camera.zoom,
                },
            );
        }
    }

    fn emit(&self, shell: &mut Shell<'_, Message>, event: CanvasEvent) {
        shell.publish((self.on_event)(event));
    }

    /// Starts a gesture and sets the interaction that lasts through it.
    fn begin(&self, state: &mut State, interaction: Interaction, shell: &mut Shell<'_, Message>) {
        self.emit(shell, CanvasEvent::GestureStart);
        state.interaction = interaction;
    }

    /// Ends the gesture of the current interaction, if it has one, and the
    /// gesture of a held arrow key. Used when the interaction is replaced or
    /// cut off, so a start always has an end.
    fn end_gestures(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        if matches!(
            state.interaction,
            Interaction::Wiring {
                from: PortRef { output: true, .. },
                pulled: false
            }
        ) {
            self.emit(shell, CanvasEvent::AssignCancel);
        }
        if matches!(
            state.interaction,
            Interaction::Moving { .. }
                | Interaction::Resizing { .. }
                | Interaction::Turning { .. }
                | Interaction::Wiring { .. }
                | Interaction::Depth { .. }
        ) {
            state.interaction = Interaction::Idle;
            self.emit(shell, CanvasEvent::GestureEnd);
        }
        if std::mem::take(&mut state.arrow_gesture) {
            state.arrow_deadline = None;
            self.emit(shell, CanvasEvent::ArrowGroupEnd);
        }
    }
}

/// What the modulation lines depend on: the scene (assignments and module
/// positions), the pin, what the pointer is on, and the selection.
#[derive(Debug, Clone, PartialEq)]
struct LinesKey {
    scene: u64,
    overlay: u64,
    pinned: Option<PortRef>,
    hover: Option<Hit>,
    selection: Vec<u32>,
    matrix_line: Option<crate::scene::ModulationView>,
}

#[derive(Debug, Clone, Default)]
enum Interaction {
    #[default]
    Idle,
    Panning {
        last: Point,
        /// Where a right press began, on screen, until the pointer moves
        /// away from it. A release while it is set is a click, which may
        /// open the probe menu.
        click: Option<Point>,
    },
    Moving {
        ids: Vec<u32>,
        start: Point,
        applied: Vector,
        /// A choice press: applied on release if the tile did not move,
        /// so dragging a closed choice control moves the tile.
        click: Option<ChoiceClick>,
    },
    /// A resize preview, committed only when the pointer is released.
    Resizing {
        module: u32,
        start: Point,
        original: Size,
        current: Size,
        minimum: Size,
    },
    Turning {
        module: u32,
        widget: usize,
        /// Pointer height on screen at the last move.
        last_y: f32,
        position: f32,
    },
    /// A button is held. Pointer up reports a release, wherever the pointer
    /// is, so a release always follows the press. The tile does not move.
    /// The app decides whether that release changes a parameter.
    Holding { module: u32, widget: usize },
    Wiring {
        from: PortRef,
        /// True when the drag began by pulling a cable off an input. Then a
        /// drop on a control assigns nothing: the user is discarding the
        /// cable (modulation.md, Making an assignment).
        pulled: bool,
    },
    /// Dragging a knob's range arc: its depth, not its value.
    Depth {
        module: u32,
        widget: usize,
        /// Pointer height on screen at the last move.
        last_y: f32,
    },
    Selecting {
        start: Point,
        current: Point,
        base: BTreeSet<u32>,
    },
}

/// What a stationary click on a choice control does.
#[derive(Debug, Clone)]
enum ChoiceClick {
    Set(u32, usize, f32),
    Open(u32, usize),
}

/// The choice list that is open above the canvas.
#[derive(Debug, Clone, Copy)]
struct SelectPopup {
    module: u32,
    widget: usize,
    highlight: usize,
}

#[derive(Default)]
struct State {
    camera: Camera,
    framed_key: Option<u64>,
    interaction: Interaction,
    /// A release after CursorLeft cannot target the inspector until the
    /// window reports CursorEntered again.
    outside_window: bool,
    hover: Option<Hit>,
    /// Cursor in world coordinates, while over the canvas or dragging.
    cursor: Option<Point>,
    modifiers: keyboard::Modifiers,
    geometry: RefCell<Option<Arc<Geometry>>>,
    controls: RefCell<Option<Arc<ControlGeometry>>>,
    traces: RefCell<Option<Arc<TraceGeometry>>>,
    /// Whether scopes were last reported as drawing full traces.
    reported_full_traces: Option<bool>,
    animation: Option<(Instant, Camera)>,
    /// The control with keyboard focus: module ID and widget index.
    focus: Option<(u32, usize)>,
    /// Open select list, if any.
    select_popup: Option<SelectPopup>,
    /// True when the focus came from Tab, so its hover text shows. A click
    /// also focuses a control, but the pointer is on it then.
    tab_focus: bool,
    /// True from a Tab press until the pointer moves: the focused control's
    /// hover text wins over the port or control under a resting pointer.
    focus_first: bool,
    /// The output whose modulation lines stay on screen: its badge was
    /// clicked (`docs/design/modulation.md`, Lines).
    pinned: Option<PortRef>,
    /// The modulation lines and outlines last built, and what they were
    /// built from. They are rebuilt only when that changes, not every
    /// frame (ADR-0010).
    lines: RefCell<Option<(LinesKey, Overlay)>>,
    /// When hover text shows (panels.md, Labels and hover text).
    hover_text: HoverTimer,
    /// The last press on a control, for double-click.
    last_click: Option<(Instant, u32, usize)>,
    /// The last press on empty canvas, in screen coordinates, for
    /// double-click.
    last_empty_click: Option<(Instant, Point)>,
    /// The browser key last seen.
    browser_key: Option<u64>,
    /// Buttons held from the keyboard, one slot for Space and one for Enter.
    ///
    /// Each key is its own hold. Releasing one leaves a button high while the
    /// other key is still down on it. Replacing a key's hold releases the
    /// button that key was holding, when nothing else is holding it.
    key_holds: [Option<KeyHold>; 2],
    /// Modules with lights on screen, as last reported.
    reported_lights: Option<Vec<u32>>,
    /// True while an arrow group is open, including its 500 ms release gap.
    arrow_gesture: bool,
    /// A released arrow keeps its gesture open for the next quick press.
    arrow_deadline: Option<Instant>,
}

impl State {
    fn hit_radius(&self) -> f32 {
        // At least 7 screen pixels, so ports stay easy to grab when zoomed
        // out. At most a quarter of a tile's width, so the middle of a tile
        // always grabs the tile, not a port.
        PORT_HIT_RADIUS.max(7.0 / self.camera.zoom).min(TILE_WIDTH * 0.25)
    }
}

/// One key holding one button down.
#[derive(Clone, Copy)]
struct KeyHold {
    key: keyboard::key::Named,
    module: u32,
    widget: usize,
}

/// True when a press at `screen` is the second click of a double-click
/// after the `last` press.
fn double_click(last: Option<(Instant, Point)>, screen: Point) -> bool {
    last.is_some_and(|(at, point)| at.elapsed() < DOUBLE_CLICK && point.distance(screen) < DOUBLE_CLICK_SLOP)
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
        let state = match self.still {
            Some(still) => self.still_state(still),
            None => State::default(),
        };
        tree::State::new(state)
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
        self.handle(state, event, bounds, cursor, shell);
        self.sync_hover_text(state, event, shell);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let mut view = CanvasView {
            bounds: layout.bounds(),
            camera: state.camera,
        };
        operation.custom(None, layout.bounds(), &mut view);
        // What the pointer is on, for the control channel's snapshot.
        let mut pointer = CanvasPointer {
            hover: state.hover.filter(|hit| *hit != Hit::Nothing),
            pinned: self.pin(state),
            drag: PointerDrag::of(&state.interaction),
        };
        operation.custom(None, layout.bounds(), &mut pointer);
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
            Interaction::Resizing { .. } => mouse::Interaction::ResizingDiagonallyDown,
            Interaction::Turning { .. } | Interaction::Depth { .. } => mouse::Interaction::ResizingVertically,
            Interaction::Holding { .. } => mouse::Interaction::Pointer,
            Interaction::Wiring { .. } => mouse::Interaction::Crosshair,
            Interaction::Selecting { .. } => mouse::Interaction::Crosshair,
            Interaction::Idle if cursor.is_over(layout.bounds()) => self.hover_interaction(state),
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

        // Controls follow the titles: drawn as shapes once titles show, and
        // not at all when zoomed further out (panels.md, Level of detail).
        let empty = PanelValues::default();
        let values = self.values.unwrap_or(&empty);
        let controls = (detail(state.camera.zoom) != Detail::Far).then(|| {
            let key = (self.scene.generation(), values.generation());
            let mut cache = state.controls.borrow_mut();
            match cache.as_ref() {
                Some(c) if c.key == key => c.clone(),
                _ => {
                    let built = Arc::new(ControlGeometry::build(self.scene, values, &self.palette));
                    *cache = Some(built.clone());
                    built
                }
            }
        });

        // Scope traces: full close up, a flat line at the middle zoom, and
        // not drawn far out (panels.md, Drawing the scope trace).
        let traces = self
            .traces
            .filter(|_| detail(state.camera.zoom) != Detail::Far)
            .map(|traces| {
                let full = detail(state.camera.zoom) == Detail::Close;
                let key = (self.scene.generation(), traces.generation(), full);
                let mut cache = state.traces.borrow_mut();
                match cache.as_ref() {
                    Some(t) if t.key == key => t.clone(),
                    _ => {
                        let built = Arc::new(TraceGeometry::build(self.scene, traces, full, &self.palette));
                        *cache = Some(built.clone());
                        built
                    }
                }
            });

        // Text too small to draw leaves a bar in its place.
        let bars = match detail(state.camera.zoom) {
            Detail::Close => geometry.bars(false, false),
            Detail::Middle => geometry.bars(true, false),
            Detail::Far => geometry.bars(true, true),
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
                controls,
                traces,
                overlay: self.overlay_in(state, state.camera.visible(bounds.size())),
                bars,
                camera: state.camera,
                cable_dim: CABLE_OPACITY,
            },
        );
        self.draw_text(state, values, renderer, bounds);
        self.draw_select_popup(state, renderer, bounds);
        self.draw_hover_text(state, renderer, bounds);
    }
}

/// How much a zoom level shows (panels.md, Level of detail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Detail {
    /// Port labels and panel text.
    Close,
    /// Titles and control shapes, no other text.
    Middle,
    /// Plain tiles: no text, no controls.
    Far,
}

fn detail(zoom: f32) -> Detail {
    let quantise = |size: f32| (size * 2.0).round() / 2.0;
    if quantise(LABEL_SIZE * zoom) >= LABEL_MIN_PX {
        Detail::Close
    } else if quantise(TITLE_SIZE * zoom) >= TITLE_MIN_PX {
        Detail::Middle
    } else {
        Detail::Far
    }
}

/// Modules with a light or meter on screen, at a zoom where they are drawn.
fn visible_lights(scene: &Scene, camera: &Camera, size: Size) -> Vec<u32> {
    if detail(camera.zoom) == Detail::Far {
        return Vec::new();
    }
    let visible = camera.visible(size);
    scene
        .modules()
        .iter()
        .filter(|m| m.panel.widgets.iter().any(|w| !w.is_control()) && m.bounds().intersects(&visible))
        .map(|m| m.id)
        .collect()
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
mod tests;
