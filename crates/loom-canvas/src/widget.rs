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

/// A gesture on the canvas. The application applies it to its model.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasEvent {
    /// The pointer pressed on the canvas while matrix keys had focus.
    PointerDown,
    /// Replace the selection.
    Select(Vec<u32>),
    /// Move modules by a world offset (always whole grid steps).
    Move {
        /// Modules to move.
        ids: Vec<u32>,
        /// Offset in world units.
        delta: Vector,
    },
    /// Set a growable tile's dimensions in grid units.
    Resize {
        /// Module to resize.
        module: u32,
        /// Width and height in grid units.
        size: (u8, u8),
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
    /// The range arc of a knob was dragged: change the depth of one of its
    /// assignments by `delta`, as a share of the knob from -1 to 1
    /// (`docs/design/modulation.md`, Arc drag). The app picks the
    /// assignment, from `pinned` first. Sent between a
    /// [`CanvasEvent::GestureStart`] and [`CanvasEvent::GestureEnd`].
    DragDepth {
        /// Module ID of the knob.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
        /// The output whose lines are pinned, if any.
        pinned: Option<PortRef>,
        /// Change of depth since the last event.
        delta: f32,
    },
    /// An output was dropped on a control that takes modulation: assign the
    /// output to the control's parameter (`docs/design/modulation.md`,
    /// Making an assignment). Not sent in MIDI learn mode.
    Assign {
        /// The output.
        from: PortRef,
        /// Module ID of the control.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// An output is being dragged at window coordinates. `None` means the
    /// pointer left the window, so a later release cannot make an assignment.
    AssignDrag {
        from: PortRef,
        start: Point,
        at: Option<Point>,
        zoom: f32,
    },
    /// An output was released away from a canvas target. The app may assign
    /// it to the inspector row under the pointer.
    AssignDrop { from: PortRef, at: Option<Point> },
    /// The output drag ended without an inspector drop.
    AssignCancel,
    /// Delete modules.
    Delete(Vec<u32>),
    /// Set a control on a module's panel. `position` is the knob position
    /// from 0 to 1, 0 or 1 for a toggle, or the option index for radio
    /// buttons. A button reports [`CanvasEvent::ButtonPressed`] instead.
    SetControl {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
        /// The new position.
        position: f32,
    },
    /// A button was pressed, by the pointer or by Space or Enter.
    ///
    /// The app decides the parameter value from the button's mode. The
    /// canvas does not.
    ButtonPressed {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// A button hold ended.
    ///
    /// Pointer up sends this, including after the pointer has left the
    /// button. So does another mouse button, key up, and the window losing
    /// focus. The app ignores it for a toggle and clears a momentary gate.
    ButtonReleased {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// An arrow key on a focused button. `high` is up or right.
    ButtonArrow {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
        /// Up or right. Down or left is false.
        high: bool,
    },
    /// Reset a control to its parameter's default (double-click).
    ResetControl {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// Open the module browser (`docs/design/module-browser.md`): `A` was
    /// pressed with the pointer over the canvas, empty canvas was
    /// double-clicked, or the browser key changed.
    OpenBrowser {
        /// Where a new module goes, in world units, not yet snapped.
        world: Point,
        /// Where the browser opens, in window coordinates.
        at: Point,
        /// The canvas area, in window coordinates, so the browser can stay
        /// inside it.
        area: Rectangle,
    },
    /// An output or a cable was right-clicked without moving: open the menu
    /// with Add scope and Add meter (`docs/design/panels.md`, Add scope and
    /// Add meter).
    ProbeMenu {
        /// The output to probe. On a cable, its source.
        output: PortRef,
        /// Where the menu was opened, in world units: the output port, or
        /// the point clicked on the cable.
        world: Point,
        /// Where the menu opens, in window coordinates.
        at: Point,
        /// The canvas area, in window coordinates, so the menu can stay
        /// inside it.
        area: Rectangle,
    },
    /// A tile was right-clicked without moving.
    TileMenu {
        /// The module under the pointer.
        module: u32,
        /// Menu position in window coordinates.
        at: Point,
        /// Canvas area for positioning the menu.
        area: Rectangle,
    },
    /// Ctrl+V or Cmd+V: paste the clipboard (`docs/design/clipboard.md`).
    /// `world` is the pointer when it is over the canvas, and the centre of
    /// the view when it is not, in world units, not yet snapped.
    Paste {
        /// Where the top-left corner of the pasted modules goes.
        world: Point,
    },
    /// The modules whose lights or meters are on screen, at a zoom where
    /// they are drawn, changed. Only these need their values read.
    VisibleDisplays(Vec<u32>),
    /// Scopes on screen now draw their full traces (true, close up) or a
    /// flat line for their level (false, further out). The app then reads
    /// only what is drawn (`docs/design/panels.md`, Drawing the scope trace).
    TraceDetail(bool),
    /// In MIDI learn mode, a control was clicked: arm it, or disarm it if it
    /// is armed. Its value does not change (`docs/design/midi-learn.md`).
    Learn {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// Space was pressed without a control that uses it (including a focused
    /// knob). The app toggles transport Run. Enter does not send this.
    ToggleRun,
    /// A drag began: turning a knob, moving tiles, or pulling a cable. Every
    /// edit until [`CanvasEvent::GestureEnd`] is one undo step
    /// (`docs/design/undo.md`, Gestures). Arrow edits end with
    /// [`CanvasEvent::ArrowGroupEnd`] after their grouping window.
    GestureStart,
    /// The gesture ended. It is always sent after a start, also when the
    /// pointer is released outside the window or the window loses focus.
    GestureEnd,
    /// The arrow key was released; another quick press may share its step.
    ArrowGroupPending,
    /// An arrow key was pressed again before the grouping deadline.
    ArrowGroupResumed,
    /// The arrow group ended, including when another interaction replaces it.
    ArrowGroupEnd,
}

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

impl<Message> PatchCanvas<'_, Message> {
    /// The first state for a still image: as if the user had framed the
    /// view, focused a control with Tab, moved the pointer, pinned a badge,
    /// and started a drag, in that order.
    fn still_state(&self, still: Still) -> State {
        let mut state = State {
            camera: still.camera,
            // Already framed, so the first redraw keeps this camera.
            framed_key: Some(self.frame_key),
            focus: still.focus,
            tab_focus: still.focus.is_some(),
            pinned: still.pinned,
            ..State::default()
        };
        if let Some(world) = still.pointer {
            state.cursor = Some(world);
            state.hover = Some(self.scene.hit(world, state.hit_radius()));
        }
        state.interaction = match still.drag {
            Some(StillDrag::Wire { from, pulled }) => Interaction::Wiring { from, pulled },
            Some(StillDrag::Depth(module, widget)) => Interaction::Depth {
                module,
                widget,
                last_y: still.pointer.map_or(0.0, |p| state.camera.to_screen(p).y),
            },
            None => Interaction::Idle,
        };
        if still.hover_text {
            // Start the wait in the past, so the text is due now and shows
            // in the first frame. One frame cannot wait for the delay.
            let now = Instant::now();
            state.hover_text.rest(self.hover_target(&state), now);
            state.hover_text.tick(now + crate::hover::HOVER_DELAY);
        }
        state
    }

    /// Handles one event: everything but hover text.
    fn handle(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
    ) {
        let over = cursor.is_over(bounds);
        let screen = relative(cursor, bounds);

        if self.matrix_escape
            && matches!(
                event,
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                })
            )
            && self.pin(state).is_some()
        {
            state.pinned = None;
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        if self.matrix_keyboard
            && matches!(
                event,
                Event::Keyboard(keyboard::Event::KeyPressed { modifiers, .. }) if !modifiers.command()
            )
        {
            self.release_every_key_hold(state, shell);
            if std::mem::take(&mut state.arrow_gesture) {
                self.emit(shell, CanvasEvent::GestureEnd);
            }
            return;
        }

        if !self.interactive {
            // Modifiers still count, so Shift held while the pop-up closes
            // is known on the next click.
            if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
                state.modifiers = *modifiers;
            }
            // A pop-up has the keys. Nothing may stay held or half dragged
            // behind it. This runs on any event, redraws too, because the
            // pop-up takes the pointer and key events before the canvas
            // sees them. Each step does nothing once nothing is held.
            self.release_pointer_hold(state, shell);
            self.release_every_key_hold(state, shell);
            self.end_gestures(state, shell);
            state.interaction = Interaction::Idle;
            if !matches!(event, Event::Window(_)) {
                return;
            }
        }

        match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) if matches!(state.interaction, Interaction::Wiring { .. }) => {
                self.end_gestures(state, shell);
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                let new_frame = state.framed_key != Some(self.frame_key);
                if state.framed_key.is_some_and(|key| key != self.frame_key) {
                    // The app has replaced the patch and its gesture history.
                    // Do not deliver the previous patch's delayed arrow end.
                    state.arrow_deadline = None;
                    state.arrow_gesture = false;
                }
                if state.arrow_deadline.is_some_and(|due| *now >= due) {
                    state.arrow_deadline = None;
                    if std::mem::take(&mut state.arrow_gesture) {
                        self.emit(shell, CanvasEvent::ArrowGroupEnd);
                    }
                }
                if state
                    .browser_key
                    .replace(self.browser_key)
                    .is_some_and(|key| key != self.browser_key)
                {
                    let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
                    self.open_browser(state.camera.to_world(centre), centre, bounds, shell);
                }
                if new_frame && bounds.width > 0.0 {
                    if let Some(world) = self.scene.bounds() {
                        state.camera.frame_readable(world, bounds.size(), READABLE_ZOOM);
                    }
                    state.framed_key = Some(self.frame_key);
                    // A new frame key means a new patch: its lights have to
                    // be reported again, even if the list looks the same.
                    state.reported_lights = None;
                    state.animation = None;
                    // Module IDs repeat between patches, so an old pin could
                    // land on an unrelated output.
                    state.pinned = None;
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
                let lights = visible_lights(self.scene, &state.camera, bounds.size());
                if state.reported_lights.as_ref() != Some(&lights) {
                    state.reported_lights = Some(lights.clone());
                    self.emit(shell, CanvasEvent::VisibleDisplays(lights));
                }
                let full = detail(state.camera.zoom) == Detail::Close;
                if state.reported_full_traces.replace(full) != Some(full) {
                    self.emit(shell, CanvasEvent::TraceDetail(full));
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key, modifiers, repeat, ..
            }) if self.focus_keys(state, key, *modifiers, *repeat, over, shell) => {}
            Event::Keyboard(keyboard::Event::KeyReleased { key, .. }) => {
                self.release_key(state, key, shell);
            }
            // Paste works with the pointer anywhere in the window. Only the
            // canvas knows where the pointer is in the patch, and where the
            // view's centre is.
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(c),
                modifiers,
                ..
            }) if modifiers.command() && c.eq_ignore_ascii_case("v") => {
                let at = screen
                    .filter(|_| over)
                    .unwrap_or(Point::new(bounds.width / 2.0, bounds.height / 2.0));
                self.emit(
                    shell,
                    CanvasEvent::Paste {
                        world: state.camera.to_world(at),
                    },
                );
                shell.capture_event();
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
                            state.camera.frame_readable(world, bounds.size(), READABLE_ZOOM);
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    // Cmd/Ctrl+0 resets the zoom to 100%, about the pointer
                    // like the wheel, so what the user points at stays put.
                    keyboard::Key::Character("0") if modifiers.command() => {
                        let at = screen.unwrap_or(Point::new(bounds.width / 2.0, bounds.height / 2.0));
                        state.camera.zoom_to(at, 1.0);
                        shell.request_redraw();
                        shell.capture_event();
                    }
                    keyboard::Key::Character("a") if modifiers.command() => {
                        let all = self.scene.modules().iter().map(|m| m.id).collect();
                        self.emit(shell, CanvasEvent::Select(all));
                        shell.capture_event();
                    }
                    keyboard::Key::Character("a") if modifiers.is_empty() => {
                        if let Some(screen) = screen {
                            self.open_browser(state.camera.to_world(screen), screen, bounds, shell);
                        }
                        shell.capture_event();
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if over => {
                let Some(screen) = screen else { return };
                if state.select_popup.take().is_some() {
                    state.last_click = None;
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
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
                // The view moved under the pointer: hover text hides, and
                // shows again only after the pointer rests anew.
                state.hover_text.restart(Instant::now());
                state.hover = Some(self.scene.hit(state.camera.to_world(screen), state.hit_radius()));
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) if !over && state.select_popup.is_some() => {
                state.select_popup = None;
                state.last_click = None;
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) if over => {
                let Some(screen) = screen else { return };
                if let Some(popup) = state.select_popup.take() {
                    let double = *button == mouse::Button::Left
                        && self.scene.hit(state.camera.to_world(screen), state.hit_radius())
                            == Hit::Widget {
                                module: popup.module,
                                widget: popup.widget,
                            }
                        && state.last_click.is_some_and(|(at, module, widget)| {
                            module == popup.module && widget == popup.widget && Instant::now() - at < DOUBLE_CLICK
                        });
                    state.last_click = None;
                    if double {
                        self.emit(
                            shell,
                            CanvasEvent::ResetControl {
                                module: popup.module,
                                widget: popup.widget,
                            },
                        );
                    } else if *button == mouse::Button::Left
                        && let Some(rect) = self.select_popup_bounds_for(state, bounds, popup)
                        && rect.contains(screen)
                    {
                        let index = ((screen.y - rect.y) / SELECT_ROW_PX).floor() as usize;
                        self.set_control(shell, popup.module, popup.widget, index as f32);
                    }
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
                if self.matrix_keyboard {
                    self.emit(shell, CanvasEvent::PointerDown);
                }
                // A second button must end a hold. Replacing the interaction
                // without a release would leave a momentary gate high.
                self.release_pointer_hold(state, shell);
                self.end_gestures(state, shell);
                let world = state.camera.to_world(screen);
                match button {
                    mouse::Button::Middle => {
                        state.interaction = Interaction::Panning {
                            last: screen,
                            click: None,
                        };
                    }
                    mouse::Button::Right => {
                        state.interaction = Interaction::Panning {
                            last: screen,
                            click: Some(screen),
                        };
                    }
                    mouse::Button::Left => {
                        self.press(state, world, screen, bounds, shell);
                    }
                    _ => {}
                }
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                state.focus_first = false;
                let Some(screen) = screen else { return };
                let world = state.camera.to_world(screen);
                state.cursor = Some(world);
                let hover = over.then(|| self.scene.hit(world, state.hit_radius()));
                let mut redraw = false;
                match &mut state.interaction {
                    Interaction::Panning { last, click } => {
                        let delta = screen - *last;
                        *last = screen;
                        if click.is_some_and(|start| start.distance(screen) > CLICK_SLOP) {
                            *click = None;
                        }
                        state.camera.pan(delta);
                        redraw = true;
                    }
                    Interaction::Moving {
                        ids, start, applied, ..
                    } => {
                        let total = world - *start;
                        let snapped = Vector::new((total.x / GRID).round() * GRID, (total.y / GRID).round() * GRID);
                        if snapped != *applied {
                            let delta = snapped - *applied;
                            *applied = snapped;
                            let ids = ids.clone();
                            self.emit(shell, CanvasEvent::Move { ids, delta });
                        }
                    }
                    Interaction::Resizing {
                        start,
                        original,
                        current,
                        minimum,
                        ..
                    } => {
                        let delta = world - *start;
                        let snap = |value: f32, min: f32| {
                            (value / GRID).round().clamp(min / GRID, (64.0_f32).max(min / GRID)) * GRID
                        };
                        let next = Size::new(
                            snap(original.width + delta.x, minimum.width),
                            snap(original.height + delta.y, minimum.height),
                        );
                        if *current != next {
                            *current = next;
                            redraw = true;
                        }
                    }
                    Interaction::Turning {
                        module,
                        widget,
                        last_y,
                        position,
                    } => {
                        let next = knob_drag(*position, screen.y - *last_y, state.modifiers.shift());
                        *last_y = screen.y;
                        if next != *position {
                            *position = next;
                            let event = CanvasEvent::SetControl {
                                module: *module,
                                widget: *widget,
                                position: next,
                            };
                            self.emit(shell, event);
                        }
                    }
                    Interaction::Depth { module, widget, last_y } => {
                        let dy = screen.y - *last_y;
                        *last_y = screen.y;
                        // The knob's own drag scale: up raises, down lowers.
                        let delta = knob_drag(0.5, dy, state.modifiers.shift()) - 0.5;
                        if delta != 0.0 {
                            let event = CanvasEvent::DragDepth {
                                module: *module,
                                widget: *widget,
                                pinned: self.pin(state),
                                delta,
                            };
                            self.emit(shell, event);
                        }
                    }
                    Interaction::Holding { .. } | Interaction::Idle => {}
                    Interaction::Wiring { from, pulled } => {
                        if from.output && !*pulled && !state.outside_window {
                            let at = match hover {
                                Some(Hit::Port(target)) if target.output != from.output => self
                                    .scene
                                    .port_position(target)
                                    .map(|world| state.camera.to_screen(world))
                                    .map(|point| Point::new(bounds.x + point.x, bounds.y + point.y)),
                                _ => None,
                            }
                            .unwrap_or(Point::new(bounds.x + screen.x, bounds.y + screen.y));
                            self.assign_drag(*from, Some(at), state.camera, bounds, shell);
                        }
                        redraw = true;
                    }
                    Interaction::Selecting { current, .. } => {
                        *current = world;
                        redraw = true;
                    }
                }
                if hover != state.hover {
                    state.hover = hover;
                    redraw = true;
                }
                if redraw {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                // The left button is what started the hold. Releasing another
                // button, one that was already down, must not end it.
                if *button != mouse::Button::Left && matches!(state.interaction, Interaction::Holding { .. }) {
                    return;
                }
                // Only the right button's own release ends a right click. A
                // left button let go meanwhile must not open the menu.
                if *button != mouse::Button::Right
                    && matches!(state.interaction, Interaction::Panning { click: Some(_), .. })
                {
                    return;
                }
                let interaction = std::mem::take(&mut state.interaction);
                let gesture = matches!(
                    interaction,
                    Interaction::Moving { .. }
                        | Interaction::Resizing { .. }
                        | Interaction::Turning { .. }
                        | Interaction::Wiring { .. }
                        | Interaction::Depth { .. }
                );
                let world = screen.map(|s| state.camera.to_world(s));
                match interaction {
                    Interaction::Resizing {
                        module,
                        original,
                        current,
                        ..
                    } => {
                        if current != original {
                            self.emit(
                                shell,
                                CanvasEvent::Resize {
                                    module,
                                    size: ((current.width / GRID) as u8, (current.height / GRID) as u8),
                                },
                            );
                        }
                    }
                    Interaction::Wiring { from, pulled } => {
                        let mut handled = false;
                        match world.filter(|_| over).map(|w| self.scene.hit(w, state.hit_radius())) {
                            Some(Hit::Port(to)) => {
                                let pair = if from.output { (from, to) } else { (to, from) };
                                if pair.0.output && !pair.1.output {
                                    self.emit(
                                        shell,
                                        CanvasEvent::Connect {
                                            from: pair.0,
                                            to: pair.1,
                                        },
                                    );
                                    handled = true;
                                }
                            }
                            Some(Hit::Widget { module, widget })
                                if !pulled && self.takes_assignment(from, module, widget) =>
                            {
                                self.emit(shell, CanvasEvent::Assign { from, module, widget });
                                handled = true;
                            }
                            _ => {}
                        }
                        if from.output && !pulled {
                            if handled
                                || screen.is_some_and(|p| bounds.contains(Point::new(bounds.x + p.x, bounds.y + p.y)))
                            {
                                self.emit(shell, CanvasEvent::AssignCancel);
                            } else {
                                self.emit(
                                    shell,
                                    CanvasEvent::AssignDrop {
                                        from,
                                        at: if state.outside_window { None } else { cursor.position() },
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
                    Interaction::Moving {
                        applied,
                        click: Some(action),
                        ..
                    } if applied == Vector::ZERO => match action {
                        ChoiceClick::Set(module, widget, position) => self.set_control(shell, module, widget, position),
                        ChoiceClick::Open(module, widget) => self.open_select(state, module, widget),
                    },
                    Interaction::Holding { module, widget } => {
                        if !Self::key_holds_button(state, module, widget) {
                            self.emit(shell, CanvasEvent::ButtonReleased { module, widget });
                        }
                    }
                    Interaction::Panning { click: Some(_), .. } => {
                        if let (Some(world), Some(screen)) = (world, screen) {
                            self.open_probe_menu(state, world, screen, bounds, shell);
                        }
                    }
                    Interaction::Idle
                    | Interaction::Panning { .. }
                    | Interaction::Moving { .. }
                    | Interaction::Turning { .. }
                    | Interaction::Depth { .. } => {}
                }
                if gesture {
                    self.emit(shell, CanvasEvent::GestureEnd);
                }
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.outside_window = true;
                if let Interaction::Wiring { from, pulled: false } = state.interaction
                    && from.output
                {
                    self.assign_drag(from, None, state.camera, bounds, shell);
                }
                self.release_pointer_hold(state, shell);
                if state.hover.take().is_some() {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorEntered) => {
                state.outside_window = false;
                if let Interaction::Wiring { from, pulled: false } = state.interaction
                    && from.output
                {
                    self.assign_drag(from, cursor.position(), state.camera, bounds, shell);
                }
            }
            Event::Window(window::Event::Unfocused) => {
                state.select_popup = None;
                // A key release never arrives if the window loses focus
                // while Space or Enter is held. End every hold here.
                self.release_pointer_hold(state, shell);
                self.release_every_key_hold(state, shell);
                self.end_gestures(state, shell);
                shell.request_redraw();
            }
            _ => {}
        }
    }

    /// Updates hover text after an event (panels.md, Labels and hover
    /// text). It asks for a frame only when the text appears or hides, and
    /// for a single timed frame while it waits, so it never draws while
    /// nothing changes.
    fn sync_hover_text(&self, state: &mut State, event: &Event, shell: &mut Shell<'_, Message>) {
        let (now, redrawing) = match event {
            Event::Window(window::Event::RedrawRequested(now)) => (*now, true),
            _ => (Instant::now(), false),
        };
        let target = self.hover_target(state);
        let mut changed = state.hover_text.rest(target, now);
        // This frame is being drawn already, so text due now needs no other.
        changed |= state.hover_text.tick(now) && !redrawing;
        if changed {
            shell.request_redraw();
        }
        if let Some(due) = state.hover_text.due() {
            shell.request_redraw_at(due);
        }
    }

    /// What hover text would be about now: the port or control under the
    /// pointer, or else the control focused with Tab. Right after a Tab
    /// press, the focused control comes first, until the pointer moves.
    /// Nothing during a drag.
    fn hover_target(&self, state: &State) -> Option<HoverTarget> {
        if state.select_popup.is_some() || !matches!(state.interaction, Interaction::Idle) {
            return None;
        }
        let focused = state.focus.filter(|_| state.tab_focus).and_then(|(module, widget)| {
            let selected = self.selection.len() == 1 && self.selection.contains(&module);
            (selected && self.is_control(module, widget)).then_some(HoverTarget::Control(module, widget))
        });
        if state.focus_first && focused.is_some() {
            return focused;
        }
        match state.hover {
            // A port in hit range wins over a label under the pointer, even
            // another port's label. That port has the hover ring, and a
            // press starts a cable from it, so the hover text must name it
            // too.
            Some(Hit::Port(port)) => Some(HoverTarget::Port(port)),
            Some(Hit::Widget { module, widget }) if self.is_control(module, widget) => {
                Some(HoverTarget::Control(module, widget))
            }
            // A light or meter belongs to the tile for presses, so the hit
            // is the module. Its hover text still shows. A port's label, or
            // the bar standing in for it, hovers like the port (panels.md,
            // Hover text).
            Some(Hit::Module(id)) => state
                .cursor
                .and_then(|world| {
                    let display = self
                        .scene
                        .module(id)
                        .and_then(|m| m.display_at(world))
                        .filter(|&widget| self.has_hover_text(id, widget))
                        .map(|widget| HoverTarget::Control(id, widget));
                    // The header names the module's family (ADR-0029).
                    let title = self
                        .scene
                        .module(id)
                        .filter(|m| world.y < m.bounds().y + HEADER)
                        .map(|_| HoverTarget::Title(id));
                    // Text is drawn above every tile, so a port label that
                    // shows over this tile's header hovers before the title.
                    display
                        .or_else(|| tile_text::port_label_at(self.scene, world).map(HoverTarget::Port))
                        .or(title)
                })
                .or(focused),
            _ => focused,
        }
    }

    /// True for controls, and for lights and meters with a description
    /// (port-and-parameter-text.md, Display values).
    fn has_hover_text(&self, module: u32, widget: usize) -> bool {
        self.scene
            .module(module)
            .and_then(|m| m.panel.widgets.get(widget))
            .is_some_and(|w| w.is_control() || !w.hover.description.is_empty())
    }

    fn is_control(&self, module: u32, widget: usize) -> bool {
        self.scene
            .module(module)
            .and_then(|m| m.panel.widgets.get(widget))
            .is_some_and(|w| w.is_control())
    }

    fn widget_kind(&self, module: u32, widget: usize) -> Option<&WidgetViewKind> {
        Some(&self.scene.module(module)?.panel.widgets.get(widget)?.kind)
    }

    /// The pinned output, while its badge shows. A pin on an output that
    /// lost its last assignment, or whose module is gone, counts as none:
    /// it draws nothing, so it must not take Escape or pick an arc drag.
    fn pin(&self, state: &State) -> Option<PortRef> {
        state
            .pinned
            .filter(|p| self.scene.badge_bounds(p.module, p.index).is_some())
    }

    /// The cursor for the pointer resting over the canvas with no gesture.
    /// The ring of a modulated knob reaches a little past the knob's cell,
    /// so it is checked first, as `press` does: there a press drags depth,
    /// and the cursor says so (modulation.md, Arc drag).
    fn hover_interaction(&self, state: &State) -> mouse::Interaction {
        if state.cursor.is_some_and(|point| self.resize_at(point).is_some()) {
            return mouse::Interaction::ResizingDiagonallyDown;
        }
        let on_ring = state
            .hover
            .zip(state.cursor)
            .is_some_and(|(hit, world)| self.depth_drag_at(&hit, world).is_some());
        if on_ring {
            return mouse::Interaction::ResizingVertically;
        }
        match state.hover {
            Some(Hit::Port(_) | Hit::Badge(_)) => mouse::Interaction::Pointer,
            Some(Hit::Widget { module, widget }) => match self.widget_kind(module, widget) {
                Some(WidgetViewKind::Knob { .. }) => mouse::Interaction::ResizingVertically,
                _ => mouse::Interaction::Pointer,
            },
            Some(Hit::Module(_)) => mouse::Interaction::Grab,
            _ => mouse::Interaction::default(),
        }
    }

    /// Finds the topmost growable tile whose bottom-right grip is under the pointer.
    fn resize_at(&self, point: Point) -> Option<&crate::scene::ModuleView> {
        self.scene
            .modules()
            .iter()
            .rev()
            .find(|tile| tile.bounds().contains(point))
            .filter(|tile| {
                let bounds = tile.bounds();
                tile.is_growable()
                    && Rectangle::new(
                        Point::new(bounds.x + bounds.width - GRID, bounds.y + bounds.height - GRID),
                        Size::new(GRID, GRID),
                    )
                    .contains(point)
            })
    }

    /// The modulated knob whose ring a press at `world` would grab to drag
    /// depth, given what the hit test found there. Not in MIDI learn mode.
    /// `press` and the hover cursor both ask this one method, so the cursor
    /// always tells the truth about the press (#198 was the two disagreeing).
    fn depth_drag_at(&self, hit: &Hit, world: Point) -> Option<(u32, usize)> {
        if self.learn.is_some() || !matches!(hit, Hit::Widget { .. } | Hit::Module(_)) {
            return None;
        }
        self.range_ring_at(world)
    }

    /// The modulated knob whose range ring is under `world`, if any.
    fn range_ring_at(&self, world: Point) -> Option<(u32, usize)> {
        let module = self.scene.modules().iter().rev().find(|m| m.bounds().contains(world))?;
        (0..module.panel.widgets.len())
            .find(|&w| self.on_range_ring(module.id, w, world))
            .map(|w| (module.id, w))
    }

    /// True if `world` is on the ring around a knob where its range arc is
    /// drawn, and the knob has an enabled assignment: there a drag changes
    /// depth, not the value. This holds when the arc has zero length and
    /// the knob shows the dot, so depth 0 can be dragged back out of.
    fn on_range_ring(&self, module: u32, widget: usize, world: Point) -> bool {
        let Some(m) = self.scene.module(module) else {
            return false;
        };
        let (Some(rect), Some(WidgetViewKind::Knob { .. })) =
            (m.widget_bounds(widget), self.widget_kind(module, widget))
        else {
            return false;
        };
        let modulated = self.values.and_then(|v| v.get(module, widget)).is_some_and(|v| {
            matches!(
                v.modulation,
                crate::controls::ModulationMark::Range { .. } | crate::controls::ModulationMark::Dot
            )
        });
        let d = world - crate::controls::knob_centre(rect);
        let distance = (d.x * d.x + d.y * d.y).sqrt();
        modulated && (crate::controls::RANGE_RING_INNER..=crate::controls::RANGE_RING_OUTER).contains(&distance)
    }

    fn position(&self, module: u32, widget: usize) -> f32 {
        self.values.map_or(0.0, |v| v.position(module, widget))
    }

    /// True when Space or Enter is still holding this button.
    fn key_holds_button(state: &State, module: u32, widget: usize) -> bool {
        state
            .key_holds
            .iter()
            .flatten()
            .any(|hold| hold.module == module && hold.widget == widget)
    }

    /// True when the pointer is still down on this button.
    fn pointer_holds_button(state: &State, module: u32, widget: usize) -> bool {
        matches!(
            state.interaction,
            Interaction::Holding {
                module: held,
                widget: held_widget,
            } if held == module && held_widget == widget
        )
    }

    /// Ends a pointer hold, if there is one. The release is reported only
    /// when no key is still holding that button.
    fn release_pointer_hold(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        if let Interaction::Holding { module, widget } = state.interaction {
            state.interaction = Interaction::Idle;
            if !Self::key_holds_button(state, module, widget) {
                self.emit(shell, CanvasEvent::ButtonReleased { module, widget });
            }
        }
    }

    /// Space or Enter, the keys that hold a button.
    fn button_key(key: &keyboard::Key) -> Option<keyboard::key::Named> {
        use keyboard::key::Named;
        match key.as_ref() {
            keyboard::Key::Named(named @ (Named::Space | Named::Enter)) => Some(named),
            _ => None,
        }
    }

    /// Starts a hold for one key. If that key was holding another button, and
    /// nothing else is, that button is released first.
    fn press_button_key(
        &self,
        state: &mut State,
        key: keyboard::key::Named,
        module: u32,
        widget: usize,
        shell: &mut Shell<'_, Message>,
    ) {
        let same = state
            .key_holds
            .iter()
            .flatten()
            .any(|hold| hold.key == key && hold.module == module && hold.widget == widget);
        if same {
            return;
        }
        self.release_button_key(state, key, shell);
        self.emit(shell, CanvasEvent::ButtonPressed { module, widget });
        if let Some(slot) = state.key_holds.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(KeyHold { key, module, widget });
        }
    }

    /// Ends one key's hold. The button stays down while another key holds it.
    fn release_button_key(&self, state: &mut State, key: keyboard::key::Named, shell: &mut Shell<'_, Message>) {
        let Some(index) = state
            .key_holds
            .iter()
            .position(|slot| slot.is_some_and(|hold| hold.key == key))
        else {
            return;
        };
        let Some(hold) = state.key_holds[index].take() else {
            return;
        };
        let still_held = Self::key_holds_button(state, hold.module, hold.widget)
            || Self::pointer_holds_button(state, hold.module, hold.widget);
        if !still_held {
            self.emit(
                shell,
                CanvasEvent::ButtonReleased {
                    module: hold.module,
                    widget: hold.widget,
                },
            );
            shell.request_redraw();
            shell.capture_event();
        }
    }

    /// Ends every keyboard hold. Used when the window loses focus, because
    /// the key releases never arrive.
    fn release_every_key_hold(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        let holds = std::mem::take(&mut state.key_holds);
        let mut reported = [(0_u32, 0_usize); 2];
        let mut count = 0;
        for hold in holds.into_iter().flatten() {
            if reported[..count].contains(&(hold.module, hold.widget))
                || Self::pointer_holds_button(state, hold.module, hold.widget)
            {
                continue;
            }
            reported[count] = (hold.module, hold.widget);
            count += 1;
            self.emit(
                shell,
                CanvasEvent::ButtonReleased {
                    module: hold.module,
                    widget: hold.widget,
                },
            );
        }
        if count > 0 {
            shell.request_redraw();
            shell.capture_event();
        }
    }

    /// Ends the hold for the key that came up.
    fn release_key(&self, state: &mut State, key: &keyboard::Key, shell: &mut Shell<'_, Message>) {
        if let Some(named) = Self::button_key(key) {
            self.release_button_key(state, named, shell);
        }
        if state.arrow_gesture
            && matches!(
                key.as_ref(),
                keyboard::Key::Named(
                    keyboard::key::Named::ArrowUp
                        | keyboard::key::Named::ArrowRight
                        | keyboard::key::Named::ArrowDown
                        | keyboard::key::Named::ArrowLeft
                )
            )
        {
            let due = Instant::now() + ARROW_GROUP;
            state.arrow_deadline = Some(due);
            self.emit(shell, CanvasEvent::ArrowGroupPending);
            shell.request_redraw_at(due);
        }
    }

    /// Selects the module of a control being edited, so the inspector
    /// follows.
    fn select_for_edit(&self, module: u32, shell: &mut Shell<'_, Message>) {
        if self.selection.len() != 1 || !self.selection.contains(&module) {
            self.emit(shell, CanvasEvent::Select(vec![module]));
        }
    }

    fn set_control(&self, shell: &mut Shell<'_, Message>, module: u32, widget: usize, position: f32) {
        self.emit(
            shell,
            CanvasEvent::SetControl {
                module,
                widget,
                position,
            },
        );
    }

    /// Screen rectangle of the open list, kept readable at any canvas zoom.
    fn select_popup_bounds(&self, state: &State, bounds: Rectangle) -> Option<Rectangle> {
        let popup = state.select_popup?;
        self.select_popup_bounds_for(state, bounds, popup)
    }

    /// Screen rectangle for a specified select popup.
    fn select_popup_bounds_for(&self, state: &State, bounds: Rectangle, popup: SelectPopup) -> Option<Rectangle> {
        let module = self.scene.module(popup.module)?;
        let rect = module.widget_bounds(popup.widget)?;
        let WidgetViewKind::Select { options } = &module.panel.widgets.get(popup.widget)?.kind else {
            return None;
        };
        let box_rect = select_box(rect);
        let anchor = state.camera.to_screen(box_rect.position());
        let width = (box_rect.width * state.camera.zoom)
            .max(SELECT_WIDTH_PX)
            .min(bounds.width);
        let height = SELECT_ROW_PX * options.len() as f32;
        let x = anchor.x.clamp(0.0, (bounds.width - width).max(0.0));
        let below = anchor.y + box_rect.height * state.camera.zoom + 3.0;
        let y = if below + height <= bounds.height {
            below
        } else {
            (anchor.y - height - 3.0).max(0.0)
        };
        Some(Rectangle::new(Point::new(x, y), Size::new(width, height)))
    }

    /// Opens a select list at its current value.
    fn open_select(&self, state: &mut State, module: u32, widget: usize) {
        state.select_popup = Some(SelectPopup {
            module,
            widget,
            highlight: self.position(module, widget).round().max(0.0) as usize,
        });
    }

    /// Pointer down on a button. The tile stays put. Pointer up reports a
    /// release, whichever mode the app has stored.
    fn press_button(&self, state: &mut State, module: u32, widget: usize, shell: &mut Shell<'_, Message>) {
        self.emit(shell, CanvasEvent::ButtonPressed { module, widget });
        state.interaction = Interaction::Holding { module, widget };
    }

    /// The keyboard path to panel controls (NFR-UX-01). Returns true if the
    /// key was used.
    fn focus_keys(
        &self,
        state: &mut State,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        repeat: bool,
        _over: bool,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        use keyboard::key::Named;
        if let Some(due) = state.arrow_deadline.take() {
            let arrow = matches!(
                key.as_ref(),
                keyboard::Key::Named(Named::ArrowUp | Named::ArrowRight | Named::ArrowDown | Named::ArrowLeft)
            );
            if !arrow || Instant::now() >= due {
                state.arrow_gesture = false;
                self.emit(shell, CanvasEvent::ArrowGroupEnd);
            } else {
                self.emit(shell, CanvasEvent::ArrowGroupResumed);
            }
        }
        // Focus is only valid on a control of the one selected module.
        let selected = (self.selection.len() == 1).then(|| *self.selection.iter().next().unwrap_or(&0));
        let hidden = |m: u32, w: usize| {
            self.scene
                .module(m)
                .and_then(|module| module.panel.widgets.get(w))
                .is_some_and(|widget| widget.hidden)
        };
        // A mode change (from MIDI, say) can hide the focused control. The
        // key meant for it is used up, so it never moves the tile instead.
        let lost_to_mode = state.focus.is_some_and(|(m, w)| Some(m) == selected && hidden(m, w));
        if state.focus.is_some_and(|(m, w)| {
            Some(m) != selected
                || !self
                    .scene
                    .module(m)
                    .and_then(|module| module.panel.widgets.get(w))
                    .is_some_and(|widget| widget.is_control() && !widget.hidden)
        }) {
            state.focus = None;
        }
        // A hidden select is not drawn, so its list closes too.
        if state
            .select_popup
            .is_some_and(|popup| hidden(popup.module, popup.widget))
        {
            state.select_popup = None;
        }
        if lost_to_mode
            && matches!(
                key.as_ref(),
                keyboard::Key::Named(
                    Named::ArrowUp
                        | Named::ArrowRight
                        | Named::ArrowDown
                        | Named::ArrowLeft
                        | Named::Space
                        | Named::Enter
                )
            )
        {
            shell.capture_event();
            return true;
        }
        if let Some(mut popup) = state.select_popup
            && let Some(WidgetViewKind::Select { options }) = self.widget_kind(popup.module, popup.widget)
        {
            let used = match key.as_ref() {
                keyboard::Key::Named(Named::Escape) => {
                    state.select_popup = None;
                    state.last_click = None;
                    true
                }
                keyboard::Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                    let delta = if matches!(key.as_ref(), keyboard::Key::Named(Named::ArrowUp)) {
                        -1
                    } else {
                        1
                    };
                    popup.highlight =
                        (popup.highlight as isize + delta).clamp(0, options.len().saturating_sub(1) as isize) as usize;
                    state.select_popup = Some(popup);
                    true
                }
                keyboard::Key::Named(Named::Enter | Named::Space) => {
                    if !repeat {
                        self.set_control(shell, popup.module, popup.widget, popup.highlight as f32);
                        state.select_popup = None;
                    }
                    true
                }
                keyboard::Key::Named(Named::ArrowLeft | Named::ArrowRight | Named::Tab) => true,
                _ => false,
            };
            if used {
                shell.request_redraw();
                shell.capture_event();
                return true;
            }
        } else {
            state.select_popup = None;
        }
        let used = match key.as_ref() {
            // Escape hides pinned lines before it does anything else
            // (modulation.md, Lines).
            keyboard::Key::Named(Named::Escape) if self.pin(state).is_some() => {
                state.pinned = None;
                true
            }
            keyboard::Key::Named(Named::Tab) => {
                let mut order: Vec<_> = self.scene.modules().iter().collect();
                order.sort_by(|a, b| {
                    a.position
                        .y
                        .total_cmp(&b.position.y)
                        .then_with(|| a.position.x.total_cmp(&b.position.x))
                        .then_with(|| a.id.cmp(&b.id))
                });
                if order.is_empty() {
                    return false;
                }
                let back = modifiers.shift();
                if let Some((module, widget)) = state.focus {
                    let controls = self
                        .scene
                        .module(module)
                        .map(|m| focusable(&m.panel))
                        .unwrap_or_default();
                    let next = controls.iter().position(|&i| i == widget).and_then(|at| {
                        if back {
                            at.checked_sub(1)
                        } else {
                            (at + 1 < controls.len()).then_some(at + 1)
                        }
                    });
                    if let Some(next) = next {
                        state.focus = Some((module, controls[next]));
                        state.tab_focus = true;
                        state.focus_first = true;
                        shell.request_redraw();
                        shell.capture_event();
                        return true;
                    }
                }
                let at = selected.and_then(|id| order.iter().position(|m| m.id == id));
                let next = match at {
                    Some(at) if back => at.checked_sub(1).unwrap_or(order.len() - 1),
                    Some(at) => (at + 1) % order.len(),
                    None if back => order.len() - 1,
                    None => 0,
                };
                state.focus = None;
                self.emit(shell, CanvasEvent::Select(vec![order[next].id]));
                true
            }
            keyboard::Key::Named(Named::Escape) if state.focus.is_some() => {
                state.focus = None;
                // In MIDI learn mode, Escape also ends the mode, so the app
                // must see it too.
                self.learn.is_none()
            }
            keyboard::Key::Named(Named::Escape) if !self.selection.is_empty() && self.learn.is_none() => {
                self.emit(shell, CanvasEvent::Select(Vec::new()));
                true
            }
            keyboard::Key::Named(Named::Enter) if state.focus.is_none() => {
                let Some(module) = selected.and_then(|id| self.scene.module(id)) else {
                    return false;
                };
                let Some(widget) = next_focus(&module.panel, None, false) else {
                    return false;
                };
                state.focus = Some((module.id, widget));
                state.tab_focus = true;
                true
            }
            // In MIDI learn mode the keyboard arms the focused control, like
            // a click, and never changes a value (midi-learn.md).
            // With nothing focused, Space still toggles Run below: that is
            // the transport, not a parameter.
            keyboard::Key::Named(Named::Space | Named::Enter) if self.learn.is_some() && state.focus.is_some() => {
                if !repeat && let Some((module, widget)) = state.focus {
                    self.emit(shell, CanvasEvent::Learn { module, widget });
                }
                true
            }
            keyboard::Key::Named(Named::ArrowUp | Named::ArrowRight | Named::ArrowDown | Named::ArrowLeft)
                if self.learn.is_some() =>
            {
                false
            }
            keyboard::Key::Named(
                named @ (Named::ArrowUp | Named::ArrowRight | Named::ArrowDown | Named::ArrowLeft),
            ) => {
                if modifiers.alt() && !modifiers.shift() && self.selection.len() == 1 {
                    let id = *self.selection.iter().next().expect("one selected module");
                    if let Some(tile) = self.scene.module(id).filter(|tile| tile.is_growable()) {
                        let width = (tile.width() / GRID) as u8;
                        let height = (tile.height() / GRID) as u8;
                        let size = match named {
                            Named::ArrowUp => (width, height.saturating_sub(1)),
                            Named::ArrowDown => (width, height.saturating_add(1)),
                            Named::ArrowLeft => (width.saturating_sub(1), height),
                            Named::ArrowRight => (width.saturating_add(1), height),
                            _ => unreachable!(),
                        };
                        if !state.arrow_gesture {
                            state.arrow_gesture = true;
                            self.emit(shell, CanvasEvent::GestureStart);
                        }
                        self.emit(shell, CanvasEvent::Resize { module: id, size });
                        shell.request_redraw();
                        shell.capture_event();
                        return true;
                    }
                    return false;
                }
                let Some((module, widget)) = state.focus else {
                    if self.selection.is_empty() {
                        return false;
                    }
                    let step = if modifiers.shift() { 10.0 } else { 1.0 } * GRID;
                    let delta = match named {
                        Named::ArrowUp => Vector::new(0.0, -step),
                        Named::ArrowRight => Vector::new(step, 0.0),
                        Named::ArrowDown => Vector::new(0.0, step),
                        Named::ArrowLeft => Vector::new(-step, 0.0),
                        _ => unreachable!(),
                    };
                    if !state.arrow_gesture {
                        state.arrow_gesture = true;
                        self.emit(shell, CanvasEvent::GestureStart);
                    }
                    self.emit(
                        shell,
                        CanvasEvent::Move {
                            ids: self.selection.iter().copied().collect(),
                            delta,
                        },
                    );
                    shell.request_redraw();
                    shell.capture_event();
                    return true;
                };
                let up = matches!(named, Named::ArrowUp | Named::ArrowRight);
                if matches!(self.widget_kind(module, widget), Some(WidgetViewKind::Button)) {
                    self.emit(
                        shell,
                        CanvasEvent::ButtonArrow {
                            module,
                            widget,
                            high: up,
                        },
                    );
                } else {
                    let position = self.position(module, widget);
                    let next = match self.widget_kind(module, widget) {
                        Some(WidgetViewKind::Knob { steps }) => knob_step(position, up, modifiers.shift(), *steps),
                        Some(WidgetViewKind::Toggle) => {
                            if up {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        Some(
                            WidgetViewKind::Radio { options }
                            | WidgetViewKind::Select { options }
                            | WidgetViewKind::Segmented { options, .. },
                        ) => {
                            let last = options.len().saturating_sub(1) as f32;
                            (position.round() + if up { 1.0 } else { -1.0 }).clamp(0.0, last)
                        }
                        _ => return false,
                    };
                    if !state.arrow_gesture {
                        state.arrow_gesture = true;
                        self.emit(shell, CanvasEvent::GestureStart);
                    }
                    if next != position {
                        self.set_control(shell, module, widget, next);
                    }
                }
                true
            }
            keyboard::Key::Named(named @ (Named::Space | Named::Enter)) => {
                if let Some((module, widget)) = state.focus {
                    let position = self.position(module, widget);
                    match self.widget_kind(module, widget) {
                        Some(WidgetViewKind::Toggle) => {
                            self.set_control(shell, module, widget, if position > 0.5 { 0.0 } else { 1.0 })
                        }
                        // A repeated key must not press again. The first press is
                        // still held, so key up can release it.
                        Some(WidgetViewKind::Button) if repeat => {}
                        Some(WidgetViewKind::Button) => {
                            self.press_button_key(state, named, module, widget, shell);
                        }
                        Some(WidgetViewKind::Radio { options } | WidgetViewKind::Segmented { options, .. }) => {
                            let next = (position.round() as usize + 1) % options.len().max(1);
                            self.set_control(shell, module, widget, next as f32);
                        }
                        Some(WidgetViewKind::Select { .. }) if !repeat => self.open_select(state, module, widget),
                        Some(WidgetViewKind::Knob { .. }) if matches!(named, Named::Space) && !repeat => {
                            self.emit(shell, CanvasEvent::ToggleRun);
                        }
                        _ => {}
                    }
                    !matches!(self.widget_kind(module, widget), Some(WidgetViewKind::Knob { .. }) if matches!(named, Named::Enter))
                } else if matches!(named, Named::Space) {
                    // Space with nothing focused is the Run shortcut. A
                    // repeat is swallowed so holding the key does not toggle
                    // again. Enter is not that shortcut.
                    if !repeat {
                        self.emit(shell, CanvasEvent::ToggleRun);
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if used {
            shell.request_redraw();
            shell.capture_event();
        }
        used
    }

    fn press(&self, state: &mut State, world: Point, screen: Point, bounds: Rectangle, shell: &mut Shell<'_, Message>) {
        if let Some(tile) = self.resize_at(world) {
            self.select_for_edit(tile.id, shell);
            state.focus = None;
            self.begin(
                state,
                Interaction::Resizing {
                    module: tile.id,
                    start: world,
                    original: tile.bounds().size(),
                    current: tile.bounds().size(),
                    minimum: tile.minimum_size(),
                },
                shell,
            );
            return;
        }
        let shift = state.modifiers.shift();
        let hit = self.scene.hit(world, state.hit_radius());
        state.tab_focus = false;
        state.focus_first = false;
        // The ring of a modulated knob reaches a little past the knob's
        // cell, so it is found before the hit test's answer is used.
        if let Some((module, widget)) = self.depth_drag_at(&hit, world) {
            // The selection stays as it is: a selected source module is one
            // way the drag picks its assignment (modulation.md, Arc drag).
            self.begin(
                state,
                Interaction::Depth {
                    module,
                    widget,
                    last_y: screen.y,
                },
                shell,
            );
            return;
        }
        if !matches!(hit, Hit::Widget { .. }) {
            state.focus = None;
        }
        match hit {
            Hit::Widget { module, widget } if self.learn.is_some() && self.is_control(module, widget) => {
                self.select_for_edit(module, shell);
                // The clicked control takes the keyboard focus, so Space or
                // Enter acts on it next, not on a control focused earlier.
                state.focus = Some((module, widget));
                self.emit(shell, CanvasEvent::Learn { module, widget });
            }
            Hit::Badge(port) => {
                // A click pins this output's lines, or unpins them. Only one
                // output is pinned at a time.
                state.pinned = if state.pinned == Some(port) { None } else { Some(port) };
            }
            Hit::Widget { module, widget } => {
                self.select_for_edit(module, shell);
                state.focus = Some((module, widget));
                let now = Instant::now();
                let double = state
                    .last_click
                    .is_some_and(|(at, m, w)| m == module && w == widget && now - at < DOUBLE_CLICK);
                // A button's second press is another press. Other controls
                // reset to the default (panels.md, Interaction).
                let button = matches!(self.widget_kind(module, widget), Some(WidgetViewKind::Button));
                if double && !button {
                    state.last_click = None;
                    self.emit(shell, CanvasEvent::ResetControl { module, widget });
                    return;
                }
                state.last_click = Some((now, module, widget));
                if button {
                    self.press_button(state, module, widget, shell);
                    return;
                }
                let position = self.position(module, widget);
                let click = match self.widget_kind(module, widget) {
                    Some(WidgetViewKind::Knob { .. }) => {
                        self.begin(
                            state,
                            Interaction::Turning {
                                module,
                                widget,
                                last_y: screen.y,
                                position,
                            },
                            shell,
                        );
                        return;
                    }
                    Some(WidgetViewKind::Toggle) => {
                        Some(ChoiceClick::Set(module, widget, if position > 0.5 { 0.0 } else { 1.0 }))
                    }
                    Some(WidgetViewKind::Radio { options }) => self
                        .scene
                        .module(module)
                        .and_then(|m| m.widget_bounds(widget))
                        .and_then(|rect| radio_option(rect, options.len(), world))
                        .map(|option| ChoiceClick::Set(module, widget, option as f32)),
                    Some(WidgetViewKind::Segmented { options, .. }) => self
                        .scene
                        .module(module)
                        .and_then(|m| m.widget_bounds(widget))
                        .and_then(|rect| crate::controls::segment_at(rect, options.len(), world))
                        .map(|option| ChoiceClick::Set(module, widget, option as f32)),
                    Some(WidgetViewKind::Select { .. }) => Some(ChoiceClick::Open(module, widget)),
                    _ => None,
                };
                // Choice controls act on release, so a drag that starts on
                // them still moves the tile.
                self.begin(
                    state,
                    Interaction::Moving {
                        ids: vec![module],
                        start: world,
                        applied: Vector::ZERO,
                        click,
                    },
                    shell,
                );
            }
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
                    // Pulling a cable out and dropping it elsewhere is one
                    // step, so the gesture starts before the disconnect.
                    self.emit(shell, CanvasEvent::GestureStart);
                    self.emit(shell, CanvasEvent::Disconnect(port));
                    state.interaction = Interaction::Wiring {
                        from: source,
                        pulled: true,
                    };
                } else {
                    self.begin(
                        state,
                        Interaction::Wiring {
                            from: port,
                            pulled: false,
                        },
                        shell,
                    );
                }
            }
            Hit::Port(port) => {
                if port.output {
                    state.outside_window = false;
                    self.assign_drag(
                        port,
                        Some(Point::new(bounds.x + screen.x, bounds.y + screen.y)),
                        state.camera,
                        bounds,
                        shell,
                    );
                }
                self.begin(
                    state,
                    Interaction::Wiring {
                        from: port,
                        pulled: false,
                    },
                    shell,
                );
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
                    self.begin(
                        state,
                        Interaction::Moving {
                            ids: selection.into_iter().collect(),
                            start: world,
                            applied: Vector::ZERO,
                            click: None,
                        },
                        shell,
                    );
                }
            }
            Hit::Nothing if double_click(state.last_empty_click.take(), screen) => {
                self.open_browser(world, screen, bounds, shell);
            }
            Hit::Cable(_) | Hit::Nothing => {
                if matches!(hit, Hit::Nothing) {
                    state.last_empty_click = Some((Instant::now(), screen));
                }
                let base = if shift { self.selection.clone() } else { BTreeSet::new() };
                state.interaction = Interaction::Selecting {
                    start: world,
                    current: world,
                    base,
                };
            }
        }
    }

    /// Sends [`CanvasEvent::OpenBrowser`] for a point given in world and
    /// canvas-relative screen coordinates.
    fn open_browser(&self, world: Point, screen: Point, bounds: Rectangle, shell: &mut Shell<'_, Message>) {
        self.emit(
            shell,
            CanvasEvent::OpenBrowser {
                world,
                at: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                area: bounds,
            },
        );
    }

    /// Sends [`CanvasEvent::ProbeMenu`] if an output or a cable is at this
    /// point. Anything else opens no menu.
    fn open_probe_menu(
        &self,
        state: &State,
        world: Point,
        screen: Point,
        bounds: Rectangle,
        shell: &mut Shell<'_, Message>,
    ) {
        let (output, anchor) = match self.scene.hit(world, state.hit_radius()) {
            Hit::Port(port) if port.output => match self.scene.port_position(port) {
                Some(position) => (port, position),
                None => return,
            },
            Hit::Cable(i) => match self.scene.cables().get(i) {
                Some(cable) => (
                    PortRef {
                        module: cable.from.0,
                        output: true,
                        index: cable.from.1,
                    },
                    world,
                ),
                None => return,
            },
            Hit::Module(module) => {
                self.emit(
                    shell,
                    CanvasEvent::TileMenu {
                        module,
                        at: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                        area: bounds,
                    },
                );
                return;
            }
            _ => return,
        };
        self.emit(
            shell,
            CanvasEvent::ProbeMenu {
                output,
                world: anchor,
                at: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                area: bounds,
            },
        );
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

    /// The overlay for a view whose visible world area is `visible`. Learn
    /// mode outlines only the controls there, so its cost follows what is on
    /// screen, not the size of the patch.
    fn overlay_in(&self, state: &State, visible: Rectangle) -> Overlay {
        let mut overlay = self.overlay(state);
        let p = &self.palette;
        // MIDI learn mode outlines every control on screen, and the armed
        // one more strongly, so the user sees what a click will do.
        if let Some(learn) = self.learn {
            let visible_modules = self
                .scene
                .modules()
                .iter()
                .filter(|module| module.bounds().intersects(&visible));
            for module in visible_modules {
                for (index, view) in module.panel.widgets.iter().enumerate() {
                    if !view.is_control() {
                        continue;
                    }
                    let Some(rect) = module.widget_bounds(index) else {
                        continue;
                    };
                    let (ring, radius) = focus_ring(&view.kind, rect);
                    let (colour, width) = if learn.armed == Some((module.id, index)) {
                        (p.selection, 2.5)
                    } else {
                        (p.focus, 1.0)
                    };
                    overlay
                        .shapes
                        .push(Shape::new(ring, radius, Color::TRANSPARENT, colour, width));
                }
            }
        }
        overlay
    }

    fn overlay(&self, state: &State) -> Overlay {
        let mut overlay = Overlay::default();
        let p = &self.palette;

        for tile in self.scene.modules().iter().filter(|tile| tile.is_growable()) {
            let bounds = tile.bounds();
            let grip = Rectangle::new(
                Point::new(
                    bounds.x + bounds.width - GRID * 0.65,
                    bounds.y + bounds.height - GRID * 0.65,
                ),
                Size::new(GRID * 0.4, GRID * 0.4),
            );
            overlay
                .shapes
                .push(Shape::new(grip, 1.0, p.focus, Color::TRANSPARENT, 0.0));
        }

        // Live port markers use the overlay buffer. A state change uploads
        // only these shapes and never rebuilds tiles or cables (#63).
        if detail(state.camera.zoom) == Detail::Close {
            let mut drawn = std::collections::HashSet::new();
            for cable in self.scene.cables() {
                if !self.activity.contains(&cable.from) {
                    continue;
                }
                for port in [
                    PortRef {
                        module: cable.from.0,
                        output: true,
                        index: cable.from.1,
                    },
                    PortRef {
                        module: cable.to.0,
                        output: false,
                        index: cable.to.1,
                    },
                ] {
                    if !drawn.insert(port) {
                        continue;
                    }
                    if let (Some(centre), Some(view)) = (self.scene.port_position(port), self.scene.port(port)) {
                        let r = PORT_RADIUS - 1.0;
                        let rect = Rectangle::new(Point::new(centre.x - r, centre.y - r), Size::new(2.0 * r, 2.0 * r));
                        let corner = if view.shape == crate::scene::PortShape::Round {
                            r
                        } else {
                            r * 0.3
                        };
                        overlay
                            .shapes
                            .push(Shape::new(rect, corner, view.colour, Color::TRANSPARENT, 0.0));
                    }
                }
            }
        }

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

        self.modulation_lines(state, &mut overlay);

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
        // The focus ring shows for keyboard focus only. After a click or a
        // drag the pointer already shows where the user is, and the ring
        // would only crowd the value text (#92).
        if let Some((module, widget)) = state.focus
            && state.tab_focus
            && self.selection.contains(&module)
            && let Some(m) = self.scene.module(module)
            && let (Some(rect), Some(view)) = (m.widget_bounds(widget), m.panel.widgets.get(widget))
        {
            let (ring, radius) = focus_ring(&view.kind, rect);
            overlay
                .shapes
                .push(Shape::new(ring, radius, Color::TRANSPARENT, p.focus, 1.5));
        }
        match &state.interaction {
            Interaction::Wiring { from, pulled } => {
                // A control that would take this output as an assignment is
                // highlighted, like an input port for a cable.
                if let Some(Hit::Widget { module, widget }) = state.hover
                    && !pulled
                    && self.takes_assignment(*from, module, widget)
                    && let Some(m) = self.scene.module(module)
                    && let (Some(rect), Some(view)) = (m.widget_bounds(widget), m.panel.widgets.get(widget))
                {
                    let (ring, radius) = focus_ring(&view.kind, rect);
                    overlay
                        .shapes
                        .push(Shape::new(ring, radius, Color::TRANSPARENT, p.hover, 1.5));
                }
                if !(self.window_drag_line && from.output && !pulled)
                    && let (Some(start), Some(port), Some(cursor)) =
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
            Interaction::Resizing { module, current, .. } => {
                if let Some(tile) = self.scene.module(*module) {
                    overlay.shapes.push(Shape::new(
                        Rectangle::new(tile.position, *current),
                        2.0,
                        Color::TRANSPARENT,
                        p.selection,
                        1.0 / state.camera.zoom,
                    ));
                }
            }
            _ => {}
        }
        overlay
    }

    /// Modulation lines in focus (modulation.md, Lines): thin dashed lines
    /// from a source port to its targets, for the pinned output, the output
    /// or badge under the pointer, the control under the pointer, and the
    /// modules in focus as for cables. Pointed and pinned sources get a
    /// double outline; their targets get a single one. A pinned badge gets a ring.
    fn modulation_lines(&self, state: &State, overlay: &mut Overlay) {
        let key = LinesKey {
            scene: self.scene.generation(),
            overlay: self.scene.overlay_generation(),
            pinned: self.pin(state),
            hover: state.hover,
            selection: self.selection.iter().copied().collect(),
            matrix_line: self.matrix_line.clone(),
        };
        let mut cache = state.lines.borrow_mut();
        if cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut lines = Overlay::default();
            self.build_modulation_lines(state, &mut lines);
            *cache = Some((key, lines));
        }
        if let Some((_, lines)) = cache.as_ref() {
            overlay.cables.extend_from_slice(&lines.cables);
            overlay.shapes.extend_from_slice(&lines.shapes);
        }
    }

    fn build_modulation_lines(&self, state: &State, overlay: &mut Overlay) {
        let p = &self.palette;
        if let Some(port) = self.pin(state)
            && let Some(badge) = self.scene.badge_bounds(port.module, port.index)
        {
            let ring = badge.expand(1.5);
            overlay
                .shapes
                .push(Shape::new(ring, ring.height * 0.5, Color::TRANSPARENT, p.text, 1.0));
        }
        let pointed = match state.hover {
            Some(Hit::Port(port) | Hit::Badge(port)) if port.output => Some(port),
            _ => None,
        };
        let sources: Vec<(u32, usize)> = [self.pin(state), pointed]
            .into_iter()
            .flatten()
            .map(|port| (port.module, port.index))
            .collect();
        let control = match state.hover {
            Some(Hit::Widget { module, widget }) => Some((module, widget)),
            _ => None,
        };
        let hovered = Self::hovered_module(state);
        let module_in_focus = |id: u32| self.selection.contains(&id) || hovered == Some(id);
        let mut targets: Vec<u32> = Vec::new();
        let mut sources_outlined: Vec<u32> = Vec::new();
        for m in self.scene.modulations() {
            let by_source = sources.contains(&m.from);
            let by_control = control.is_some_and(|(module, widget)| m.to == module && m.widget == Some(widget));
            if !(self.matrix_line.as_ref() == Some(m)
                || by_source
                || by_control
                || module_in_focus(m.from.0)
                || module_in_focus(m.to))
            {
                continue;
            }
            let from = PortRef {
                module: m.from.0,
                output: true,
                index: m.from.1,
            };
            let (Some(start), Some(end)) = (self.scene.port_position(from), self.scene.modulation_end(m)) else {
                continue;
            };
            let colour = if m.enabled {
                p.modulation
            } else {
                Color {
                    a: p.modulation.a * 0.4,
                    ..p.modulation
                }
            };
            let mut line = Cable::new(start, end, colour, colour, true).thin().focused();
            if !m.enabled {
                line = line.half_width();
            }
            overlay.cables.push(line);
            if by_source || by_control {
                if !targets.contains(&m.to) {
                    targets.push(m.to);
                }
                if !sources_outlined.contains(&m.from.0) {
                    sources_outlined.push(m.from.0);
                }
            }
        }
        for id in targets.into_iter().filter(|id| !sources_outlined.contains(id)) {
            if let Some(module) = self.scene.module(id) {
                overlay.shapes.push(Shape::new(
                    module.bounds().expand(3.0),
                    7.0,
                    Color::TRANSPARENT,
                    p.modulation,
                    1.2,
                ));
            }
        }
        for id in sources_outlined {
            if let Some(module) = self.scene.module(id) {
                for distance in [3.0, 6.0] {
                    overlay.shapes.push(Shape::new(
                        module.bounds().expand(distance),
                        7.0 + distance - 3.0,
                        Color::TRANSPARENT,
                        p.modulation,
                        1.2,
                    ));
                }
            }
        }
    }

    fn draw_text<Renderer>(&self, state: &State, values: &PanelValues, renderer: &mut Renderer, bounds: Rectangle)
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
        // The control whose value shows instead of its label.
        let active = match state.interaction {
            Interaction::Turning { module, widget, .. } => Some((module, widget)),
            _ => state.focus,
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
            tile_text::for_each(module, |item| {
                let (size, colour) = match item.kind {
                    TextKind::Title => (title_px, self.palette.text),
                    _ if !show_labels => return,
                    // The chosen segment is filled with the value colour,
                    // so its name is dark (visual-system.md, contrast).
                    TextKind::Option(widget, option)
                        if matches!(
                            module.panel.widgets.get(widget).map(|w| &w.kind),
                            Some(WidgetViewKind::Segmented { .. })
                        ) && self.position(module.id, widget).round() as usize == option =>
                    {
                        (label_px, self.palette.background)
                    }
                    TextKind::Option(..) => (label_px, self.palette.text),
                    // A button's label is on its face: light on the off
                    // face, dark on the orange face while on.
                    TextKind::Caption(widget)
                        if matches!(
                            module.panel.widgets.get(widget).map(|w| &w.kind),
                            Some(WidgetViewKind::Button)
                        ) =>
                    {
                        let on = self.position(module.id, widget) > 0.5;
                        (label_px, if on { self.palette.background } else { self.palette.text })
                    }
                    _ => (label_px, self.palette.text_dim),
                };
                // A focused or turning knob shows its value instead of its
                // caption.
                // In MIDI learn mode, a mapped control shows its mapping.
                let learned = match item.kind {
                    TextKind::Caption(widget) => self
                        .learn
                        .and_then(|learn| learn.labels.get(&(module.id, widget)))
                        .map(String::as_str),
                    _ => None,
                };
                let value = match item.kind {
                    _ if learned.is_some() => learned,
                    TextKind::Caption(widget) if active == Some((module.id, widget)) => values
                        .get(module.id, widget)
                        .filter(|_| matches!(module.panel.widgets[widget].kind, WidgetViewKind::Knob { .. }))
                        .map(|v| v.text.as_str()),
                    _ => None,
                };
                let align = match item.align {
                    Align::Left => text::Alignment::Left,
                    Align::Center => text::Alignment::Center,
                    Align::Right => text::Alignment::Right,
                };
                renderer.fill_text(
                    text(value.unwrap_or(&item.content), size, item.width * zoom, align),
                    to_screen(item.anchor),
                    if value.is_some() { self.palette.text } else { colour },
                    clip,
                );
            });
            if show_labels {
                for (index, widget) in module.panel.widgets.iter().enumerate() {
                    let WidgetViewKind::Select { options } = &widget.kind else {
                        continue;
                    };
                    let Some(rect) = module.widget_bounds(index) else {
                        continue;
                    };
                    let box_rect = select_box(rect);
                    let chosen = values.position(module.id, index).round().max(0.0) as usize;
                    // Cut to the box with "…", never wrapped; the open list
                    // shows the whole name. The text starts 7 units in, and
                    // the arrow takes the last 11.
                    let room = box_rect.width - 18.0;
                    let choice = options.get(chosen).map_or("", String::as_str);
                    let choice = crate::scene::fit_label(choice, room, crate::tile_text::LABEL_SIZE);
                    renderer.fill_text(
                        text(&choice, label_px, room * zoom, text::Alignment::Left),
                        to_screen(Point::new(box_rect.x + 7.0, box_rect.center_y())),
                        self.palette.text,
                        clip,
                    );
                }
            }
        }

        // Badge numbers usually sit outside their tile, so they are checked
        // against their own bounds, not the tile's, and clip to the canvas.
        // They show exactly when port labels do (#249): with its own
        // threshold, a badge could show as an empty pill beside labels.
        // Zoomed out further, the pill alone shows.
        let badge_px = quantise(BADGE_TEXT_SIZE * zoom);
        if !show_labels {
            return;
        }
        for module in self.scene.modules() {
            // A badge reaches past its tile's right edge by at most this
            // much: the gap after the port and a pill with several digits.
            const BADGE_REACH: f32 = 40.0;
            if !module.bounds().expand(BADGE_REACH).intersects(&visible) {
                continue;
            }
            for (index, port) in module.outputs.iter().enumerate() {
                if let Some(rect) = self
                    .scene
                    .badge_bounds(module.id, index)
                    .filter(|r| r.intersects(&visible))
                {
                    let label = port.badge.to_string();
                    renderer.fill_text(
                        text(&label, badge_px, rect.width * zoom, text::Alignment::Center),
                        to_screen(rect.center()),
                        self.palette.background,
                        bounds,
                    );
                }
            }
        }
    }

    /// Draws the open choice list above all tile text, at a fixed screen size.
    fn draw_select_popup<Renderer>(&self, state: &State, renderer: &mut Renderer, bounds: Rectangle)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        let Some(popup) = state.select_popup else {
            return;
        };
        let Some(rect) = self.select_popup_bounds(state, Rectangle::new(Point::ORIGIN, bounds.size())) else {
            return;
        };
        let Some(WidgetViewKind::Select { options }) = self.widget_kind(popup.module, popup.widget) else {
            return;
        };
        let at = Point::new(bounds.x + rect.x, bounds.y + rect.y);
        let area = Rectangle::new(at, rect.size());
        let font = renderer.default_font();
        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: area,
                    border: iced::Border {
                        color: self.palette.tile_border,
                        width: 1.0,
                        radius: 5.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                self.palette.control_body,
            );
            for (index, option) in options.iter().enumerate() {
                if index == popup.highlight {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(at.x + 2.0, at.y + SELECT_ROW_PX * index as f32 + 1.0),
                                Size::new(rect.width - 4.0, SELECT_ROW_PX - 2.0),
                            ),
                            border: iced::Border {
                                radius: 3.0.into(),
                                ..iced::Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        self.palette.control_track,
                    );
                }
                let chosen = self.position(popup.module, popup.widget).round().max(0.0) as usize == index;
                let label = if chosen {
                    format!("✓  {option}")
                } else {
                    format!("    {option}")
                };
                renderer.fill_text(
                    Text {
                        content: label,
                        bounds: Size::new(rect.width - 12.0, 17.0),
                        size: Pixels(12.0),
                        line_height: text::LineHeight::Relative(1.2),
                        font,
                        align_x: text::Alignment::Left,
                        align_y: alignment::Vertical::Center,
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(at.x + 7.0, at.y + SELECT_ROW_PX * (index as f32 + 0.5)),
                    self.palette.text,
                    bounds,
                );
            }
        });
    }

    /// The label, hover text, and world bounds of what hover text is about.
    /// The label, hover text, value line, and anchor of a hover target. Only
    /// lights and meters have a value line: their current value, which the
    /// app writes as the widget's text (port-and-parameter-text.md, Display
    /// values). A control's value shows on the control itself.
    fn hover_content(&self, target: HoverTarget) -> Option<(&str, &HoverText, Option<&str>, Rectangle)> {
        match target {
            HoverTarget::Port(port) => {
                let view = self.scene.port(port)?;
                let at = self.scene.port_position(port)?;
                let marker = Rectangle::new(
                    Point::new(at.x - PORT_RADIUS, at.y - PORT_RADIUS),
                    Size::new(2.0 * PORT_RADIUS, 2.0 * PORT_RADIUS),
                );
                // An output's hover text goes above its badge too, so it
                // does not cover the count (#249).
                let anchor = match port
                    .output
                    .then(|| self.scene.badge_bounds(port.module, port.index))
                    .flatten()
                {
                    Some(badge) => marker.union(&badge),
                    None => marker,
                };
                Some((&view.label, &view.hover, None, anchor))
            }
            HoverTarget::Control(id, widget) => {
                let module = self.scene.module(id)?;
                let view = module.panel.widgets.get(widget)?;
                let value = (!view.is_control())
                    .then(|| self.values.and_then(|v| v.get(id, widget)))
                    .flatten()
                    .map(|v| v.text.as_str())
                    .filter(|text| !text.is_empty());
                Some((&view.label, &view.hover, value, module.widget_bounds(widget)?))
            }
            HoverTarget::Title(id) => {
                let module = self.scene.module(id)?;
                let tile = module.bounds();
                let header = Rectangle::new(tile.position(), Size::new(tile.width, HEADER));
                Some((&module.title, &module.hover, None, header))
            }
        }
    }

    /// Hover text (panels.md, Labels and hover text): the label, the
    /// description, and the name when it differs, at a fixed screen size in
    /// a layer above the tiles. Drawn at any zoom, because hover text is
    /// what the user needs where tile text is too small to read.
    fn draw_hover_text<Renderer>(&self, state: &State, renderer: &mut Renderer, bounds: Rectangle)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        if state.select_popup.is_some() {
            return;
        }
        let Some(target) = state.hover_text.shown() else {
            return;
        };
        let Some((label, hover, value, world)) = self.hover_content(target) else {
            return;
        };
        // A port with a clock mark says which transport signal it follows
        // (panels.md, Clock mark on transport inputs).
        let follows = match target {
            HoverTarget::Port(port) => self.scene.port(port).and_then(|p| p.follows_transport),
            HoverTarget::Control(..) | HoverTarget::Title(_) => None,
        };
        let font = renderer.default_font();
        let inner = crate::hover::text_width(bounds.width);
        // Each line is measured as a paragraph, then drawn as text in that
        // size, wrapped at word boundaries.
        let line = |content: &str, size: f32, colour: Color, font: Font| {
            let text = Text {
                content: content.to_string(),
                bounds: Size::new(inner, f32::INFINITY),
                size: Pixels(size),
                line_height: text::LineHeight::Relative(1.3),
                font,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::Word,
            };
            let measured = Renderer::Paragraph::with_text(text.as_ref()).min_bounds();
            (
                Text {
                    bounds: Size::new(measured.width.ceil() + 1.0, measured.height.ceil()),
                    ..text
                },
                colour,
            )
        };
        // A light's or meter's label is bold, so it stands apart from its
        // value line (port-and-parameter-text.md, Display values).
        let bold = Font {
            weight: iced::font::Weight::Bold,
            ..font
        };
        let title = if value.is_some() { bold } else { font };
        let mut lines = vec![line(label, HOVER_TEXT_PX, self.palette.text, title)];
        if !hover.description.is_empty() {
            lines.push(line(&hover.description, HOVER_TEXT_PX, self.palette.text_dim, font));
        }
        if let Some(signal) = follows {
            lines.push(line(signal.hover_line(), HOVER_TEXT_PX, self.palette.text, font));
        }
        if let Some(value) = value {
            lines.push(line(value, HOVER_TEXT_PX, self.palette.text, font));
        }
        if let Some(name) = &hover.name {
            lines.push(line(name, HOVER_TEXT_PX - 1.0, self.palette.text_dim, font));
        }
        const LINE_GAP: f32 = 3.0;
        let width = lines.iter().map(|(t, _)| t.bounds.width).fold(0.0, f32::max);
        let height = lines.iter().map(|(t, _)| t.bounds.height).sum::<f32>() + LINE_GAP * (lines.len() - 1) as f32;
        let size = Size::new(width + 2.0 * HOVER_PADDING, height + 2.0 * HOVER_PADDING);

        let zoom = state.camera.zoom;
        let corner = state.camera.to_screen(world.position());
        let anchor = Rectangle::new(
            Point::new(bounds.x + corner.x, bounds.y + corner.y),
            world.size() * zoom,
        );
        let at = crate::hover::place(size, anchor, bounds);
        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(at, size),
                    border: iced::Border {
                        color: self.palette.tile_border,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                self.palette.tile,
            );
            let mut y = at.y + HOVER_PADDING;
            for (text, colour) in lines {
                let height = text.bounds.height;
                renderer.fill_text(text, Point::new(at.x + HOVER_PADDING, y), colour, bounds);
                y += height + LINE_GAP;
            }
        });
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

/// The control after (or with `back`, before) `current` in focus order,
/// wrapping around. Lights are skipped. `None` if the panel has no
/// controls.
fn next_focus(panel: &crate::scene::PanelView, current: Option<usize>, back: bool) -> Option<usize> {
    let controls = focusable(panel);
    if controls.is_empty() {
        return None;
    }
    let at = current.and_then(|c| controls.iter().position(|&i| i == c));
    let next = match (at, back) {
        (None, false) => 0,
        (None, true) => controls.len() - 1,
        (Some(i), false) => (i + 1) % controls.len(),
        (Some(i), true) => (i + controls.len() - 1) % controls.len(),
    };
    Some(controls[next])
}

/// The panel's widgets that keyboard focus can land on, in focus order:
/// its controls, without lights and without controls the mode hides
/// (ADR-0030). Tab and Enter both walk this list.
fn focusable(panel: &crate::scene::PanelView) -> Vec<usize> {
    (0..panel.widgets.len())
        .filter(|&i| panel.widgets[i].is_control() && !panel.widgets[i].hidden)
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
