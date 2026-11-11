//! The patch canvas's input, as plain code with no UI toolkit in it.
//!
//! [`CanvasInput`] is the canvas's widget state, the part that lasts: the
//! camera, what the pointer is on, the drag in progress, and keyboard
//! focus. [`CanvasModel`] is its view model, what the app shows this
//! frame: the scene, the selection, and the values. A driver hands
//! `CanvasInput::handle` one event from `loom-ui-core` at a time, with the
//! [`Moment`] it arrived in, and gets back a
//! [`Reaction`](crate::Reaction): the messages to report as
//! [`CanvasEvent`]s, whether the input was used, and whether to redraw.
//! `CanvasInput::next_deadline` says when the canvas next needs to wake
//! with no input.
//!
//! The driver today is the Iced widget in `crate::widget`. It is the only
//! code that turns Iced's events into ours.

use crate::camera::Camera;
use crate::controls::{
    PanelValues, RangeGrab, ValueDrag, arrow_step, focus_ring, radio_option, range_bar_at, range_handle, range_nearest,
    range_track, select_box, xy_area, xy_bar_at, xy_edge_ring, xy_nearest_edge, xy_position, xy_throw,
};
use crate::draft::{DraftSubmit, TypedDraft};
use crate::gpu::{Cable, Overlay, Shape};
use crate::hover::{HoverTarget, HoverTimer};
use crate::reaction::{CursorShape, Reaction};
use crate::scene::{
    GRID, HEADER, Hit, HoverText, PORT_HIT_RADIUS, PORT_RADIUS, PortRef, Scene, TILE_WIDTH, WidgetView, WidgetViewKind,
};
use crate::theme::Palette;
use crate::tile_text::{self, LABEL_SIZE, TITLE_SIZE, TextKind};
use crate::trace::ScopeTraces;
use loom_ui_core::{
    Color, Event, Key, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, Rect, ScrollDelta, Size, Vector,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

mod event;
mod focus;
mod frame;
mod handle;
mod hover_text;
mod overlay;
mod paint;
mod panel_input;
mod select_popup;
mod typed_field;

pub use event::{CanvasEvent, ControlPart};
pub use frame::FrameCache;
// What the widget's text drawing needs from the files below.
pub(crate) use overlay::learned_label;
pub(crate) use typed_field::{FIELD_HEIGHT_PX, FIELD_PADDING_PX, FIELD_TEXT_PX, FIELD_WIDTH_PX, typed_reason_bounds};

/// Two presses on the same control within this time reset it.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// How far in screen pixels the pointer may move between the two clicks of
/// a double-click.
const DOUBLE_CLICK_SLOP: f32 = 4.0;
/// How far a right press may move, in screen pixels, and still count as a
/// click that opens the probe menu rather than a pan.
const CLICK_SLOP: f32 = 4.0;
/// Height of a row in the choice popup, in screen pixels.
pub(crate) const SELECT_ROW_PX: f32 = 23.0;
/// Minimum popup width in screen pixels, so names stay legible at middle zoom.
const SELECT_WIDTH_PX: f32 = 124.0;
/// Titles are drawn when they would be at least this tall on screen.
pub(crate) const TITLE_MIN_PX: f32 = 6.5;
/// Port labels are drawn when they would be at least this tall on screen.
pub(crate) const LABEL_MIN_PX: f32 = 7.5;
/// The smallest zoom where port labels and captions are drawn. Opening a
/// patch and framing it (F, Home) never zoom out past it (#455).
const READABLE_ZOOM: f32 = LABEL_MIN_PX / LABEL_SIZE;

/// Opacity of cables that are not in focus. It never changes with hover or
/// selection, so moving the pointer only changes the cables it points at,
/// never the whole patch.
pub(crate) const CABLE_OPACITY: f32 = 0.4;

/// What MIDI learn mode shows on the canvas. While the canvas has one, a
/// click on a control arms it instead of changing it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LearnView {
    /// The armed control: module ID and widget index.
    pub armed: Option<(u32, usize)>,
    /// Mapping labels, such as `CC 74`, by module ID and widget index. They
    /// show in place of the control's caption.
    pub labels: BTreeMap<(u32, usize), String>,
    /// Mapping labels of the values of a widget that binds two, by module ID,
    /// widget index, and part. They show in that part's hover text
    /// (midi-learn.md, Making a mapping).
    pub part_labels: BTreeMap<(u32, usize, ControlPart), String>,
}

/// Says why typed text is not a value of a control: module, widget, text.
type TypedCheck<'a> = Box<dyn Fn(u32, usize, &str) -> Result<(), String> + 'a>;

/// The canvas's view model: what the canvas shows this frame, as the app
/// gives it. It holds the scene, the selection, the live values, and the
/// settings that change how input is read. It borrows from the app and
/// lasts one frame. The state that lasts is [`CanvasInput`], which is the
/// canvas's widget state.
pub struct CanvasModel<'a> {
    pub(crate) scene: &'a Scene,
    selection: &'a BTreeSet<u32>,
    pub(crate) values: Option<&'a PanelValues>,
    pub(crate) traces: Option<&'a ScopeTraces>,
    /// Outputs that currently carry a signal; their cable inputs follow.
    activity: Vec<(u32, usize)>,
    pub(crate) palette: Palette,
    frame_key: u64,
    browser_key: u64,
    /// A request to move keyboard focus: a key that changes with each
    /// request, and the control to focus.
    focus_request: (u64, Option<(u32, usize)>),
    /// The level shown, and its camera when the app has one. See
    /// [`CanvasModel::level`].
    level_key: u64,
    level_camera: Option<Camera>,
    animate: bool,
    pub(crate) learn: Option<&'a LearnView>,
    /// The row selected in the modulation matrix, without changing the pin.
    matrix_line: Option<crate::scene::ModulationView>,
    /// Who else in the window has the keys. See [`CanvasFocusPolicy`].
    focus_policy: CanvasFocusPolicy,
    /// The app's open edit group, as of this view. See
    /// [`CanvasModel::edit_group`].
    edit_group: EditGroupView,
    /// The check for typed values. See [`CanvasModel::typed_check`].
    typed_check: Option<TypedCheck<'a>>,
    pub(crate) still: Option<Still>,
    window_drag_line: bool,
}

/// How a still image of the canvas starts: its camera, the control with
/// keyboard focus, the pointer, the pinned badge, and a drag in progress.
/// See [`CanvasModel::still`].
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
    pub bounds: Rect,
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

/// The text field the canvas draws for a typed value, while one is open. The
/// canvas reports it to widget operations as custom state, as it does
/// [`CanvasPointer`], so a script can see it. The app works out the reason for
/// bad text itself, so it cannot differ from what the field shows.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasTyped {
    /// The module the field is over.
    pub module: u32,
    /// The widget the field is over, as an index into the panel.
    pub widget: usize,
    /// The text in the field.
    pub text: String,
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
    /// A knob or a fader is dragged: module ID and widget index.
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
    /// A knob's range arc or a fader's bar is dragged: module ID and widget
    /// index.
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
            Interaction::Turning { module, widget, .. }
            | Interaction::Pad { module, widget, .. }
            | Interaction::Range { module, widget, .. } => Self::Knob(*module, *widget),
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

    /// A world rectangle in window coordinates, in logical pixels: where
    /// something on the canvas is in the window, for a screen reader's
    /// cursor or a script's pointer.
    pub fn rect_to_window(&self, world: Rect) -> Rect {
        let a = self.to_window(Point::new(world.x, world.y));
        let b = self.to_window(Point::new(world.x + world.width, world.y + world.height));
        Rect::new(a, Size::new(b.x - a.x, b.y - a.y))
    }
}

impl<'a> CanvasModel<'a> {
    /// What to show for a scene and the current selection, with every other
    /// setting at its default.
    pub fn new(scene: &'a Scene, selection: &'a BTreeSet<u32>) -> Self {
        Self {
            scene,
            selection,
            values: None,
            traces: None,
            activity: Vec::new(),
            palette: Palette::default(),
            frame_key: 0,
            browser_key: 0,
            focus_request: (0, None),
            level_key: 0,
            level_camera: None,
            animate: false,
            learn: None,
            matrix_line: None,
            focus_policy: CanvasFocusPolicy::default(),
            edit_group: EditGroupView::Closed,
            typed_check: None,
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

    /// Says who else in the window has the keyboard, so the canvas leaves
    /// them alone (panels.md, Keyboard without a mouse). The app builds it
    /// from who has the input.
    pub fn focus_policy(mut self, policy: CanvasFocusPolicy) -> Self {
        self.focus_policy = policy;
        self
    }

    /// Says where the app's open edit group is: the run of edits that make
    /// one undo step (`docs/design/undo.md`). Arrow keys open one, and so
    /// can the app for edits that come from outside the canvas. The app
    /// owns the group; the canvas only reads it, to know when an arrow
    /// press or another key matters, and to wake for its deadline. It is
    /// not told who opened the group.
    pub fn edit_group(mut self, group: EditGroupView) -> Self {
        self.edit_group = group;
        self
    }

    /// Sets the check for typed values: given a module, a widget, and the
    /// text, why it is not a value. The field shows the reason and Enter does
    /// nothing (panel-widgets.md, Typing a value). Only the app knows the
    /// parameter, so the canvas asks.
    pub fn typed_check(mut self, check: impl Fn(u32, usize, &str) -> Result<(), String> + 'a) -> Self {
        self.typed_check = Some(Box::new(check));
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

    /// Says which level of a nested patch the scene shows, and the camera
    /// that level had when it was last shown, if any. When the key changes,
    /// the canvas adopts that camera, or frames the scene when there is
    /// none (a first visit), and reports [`CanvasEvent::Camera`]. A
    /// [`frame_key`](Self::frame_key) change still frames the scene, and
    /// wins over a camera given here. The default key 0 never changes, so
    /// a caller that shows one level needs nothing.
    pub fn level(mut self, key: u64, camera: Option<Camera>) -> Self {
        self.level_key = key;
        self.level_camera = camera;
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

    /// Moves keyboard focus to a control whenever this key changes, but
    /// not when the canvas first appears: `control` is the module ID and
    /// the widget's index in its panel. The focus shows as after a Tab
    /// press. Nothing happens when the widget is gone, hidden, or not a
    /// control. The control's tile becomes the one selected, as a press on
    /// the control makes it, so the keys reach the control.
    ///
    /// This is how something outside the canvas asks for focus on one of
    /// its parts: a screen reader's Focus action now, the UI layer's focus
    /// effect later (ui-layer.md, section 7 and section 9).
    pub fn focus_request(mut self, key: u64, control: Option<(u32, usize)>) -> Self {
        self.focus_request = (key, control);
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
    fn assign_drag(&self, from: PortRef, at: Option<Point>, camera: Camera, bounds: Rect, out: &mut Reaction) {
        if let Some(world) = self.scene.port_position(from) {
            let screen = camera.to_screen(world);
            self.emit(
                out,
                CanvasEvent::AssignDrag {
                    from,
                    start: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                    at,
                    zoom: camera.zoom,
                },
            );
        }
    }

    /// Everything one event does: what it means, and the hover text. When
    /// the canvas next needs to wake is not part of it: see
    /// [`CanvasInput::next_deadline`].
    ///
    /// `event` is `None` for something the window reported that means
    /// nothing to the canvas. The steps that run on every event still run.
    /// `pointer` is the pointer in window pixels, or `None` when the canvas
    /// may not see it. `now` is the time of the event.
    fn on_event(
        &self,
        state: &mut CanvasInput,
        event: Option<&Event>,
        pointer: Option<Point>,
        bounds: Rect,
        now: Instant,
        out: &mut Reaction,
    ) {
        self.handle(state, event, pointer, bounds, now, out);
        self.sync_hover_text(state, event, now, out);
    }

    fn emit(&self, out: &mut Reaction, event: CanvasEvent) {
        out.emit(event);
    }

    /// Starts a gesture and sets the interaction that lasts through it.
    fn begin(&self, state: &mut CanvasInput, interaction: Interaction, out: &mut Reaction) {
        self.emit(out, CanvasEvent::GestureStart);
        state.interaction = interaction;
    }

    /// Ends the gesture of the current interaction, if it has one. Used when
    /// the interaction is replaced or cut off, so a start always has an end.
    /// The edit group ends separately, with [`CanvasModel::end_edit_group`].
    fn end_gestures(&self, state: &mut CanvasInput, out: &mut Reaction) {
        if matches!(
            state.interaction,
            Interaction::Wiring {
                from: PortRef { output: true, .. },
                pulled: false
            }
        ) {
            self.emit(out, CanvasEvent::AssignCancel);
        }
        if matches!(
            state.interaction,
            Interaction::Moving { .. }
                | Interaction::Resizing { .. }
                | Interaction::Turning { .. }
                | Interaction::Pad { .. }
                | Interaction::Range { .. }
                | Interaction::Wiring { .. }
                | Interaction::Depth { .. }
        ) {
            state.interaction = Interaction::Idle;
            self.emit(out, CanvasEvent::GestureEnd);
        }
    }

    /// Ends the edit group. With `always` false, only when this view shows
    /// one open, so the paths that run on every key or redraw stay quiet.
    /// The app ignores an end with no group open, so sending one too many is
    /// safe; with `always` true it is sent where no key-up or timer would
    /// follow to close a group the view does not show yet.
    fn end_edit_group(&self, out: &mut Reaction, always: bool) {
        if always || self.edit_group != EditGroupView::Closed {
            self.emit(out, CanvasEvent::EditGroupEnd);
        }
    }

    /// Reports an arrow key about to edit.
    fn arrow_step(&self, now: Instant, out: &mut Reaction) {
        self.emit(out, CanvasEvent::ArrowStep { at: now });
    }
}

/// The app's open edit group, as the canvas sees it in one view
/// (`docs/design/undo.md`): edits that make one undo step. The app owns the
/// group and its history step. The canvas does not know who opened it.
/// Arrow keys do, and the app may show one of its own that waits in the
/// same way, with a deadline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditGroupView {
    /// No group is open.
    #[default]
    Closed,
    /// An arrow key is down, and its edits are one step.
    Held,
    /// The group waits: no key is held. An arrow press before `due` is
    /// reported, so the app can join it to the step or end the group, and
    /// any other key or a pointer press ends it.
    Open {
        /// When the group ends if nothing comes.
        due: Instant,
    },
}

/// Who else in the window has the keyboard, so the canvas knows which
/// keys and presses are its own. The app builds it from who has the input,
/// once per view.
///
/// It is a set of flags rather than one enum because they combine: the
/// matrix's depth field is a text field inside the matrix, so
/// `matrix_keyboard` and `text_field_keys` are both true while it has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasFocusPolicy {
    /// False while a pop-up such as the module browser has the input. The
    /// canvas then ignores the keyboard and the pointer, and lets go of any
    /// hold or drag.
    pub interactive: bool,
    /// The focused matrix has the keys: unmodified keys are left to it, and
    /// the pointer still works.
    pub matrix_keyboard: bool,
    /// The matrix has the keys and no cell editor is open, so Escape can
    /// still hide a pinned badge's lines.
    pub matrix_escape: bool,
    /// An app text field has focus. The canvas then leaves every key to it
    /// except Tab, which moves focus as usual. The pointer still works
    /// (panel-widgets.md, Typing a value).
    pub text_field_keys: bool,
    /// The focused text field moves to the next field on Tab, so the canvas
    /// leaves Tab alone too (panels.md, Keyboard without a mouse).
    pub text_field_keeps_tab: bool,
}

impl Default for CanvasFocusPolicy {
    /// The canvas has the keys and the pointer.
    fn default() -> Self {
        Self {
            interactive: true,
            matrix_keyboard: false,
            matrix_escape: false,
            text_field_keys: false,
            text_field_keeps_tab: false,
        }
    }
}

/// The modulation lines and outlines last built, and what they were built
/// from. They are rebuilt only when that changes, not every frame
/// (ADR-0010). The driver keeps one beside its [`CanvasInput`].
#[derive(Default)]
pub(crate) struct LinesCache(pub(crate) RefCell<Option<(LinesKey, Overlay)>>);

/// What the modulation lines depend on: the scene (assignments and module
/// positions), the pin, what the pointer is on, and the selection.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinesKey {
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
        /// The view at the press. The release reports the view only when
        /// it differs, so a click or a drag back to the start says nothing.
        start: Camera,
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
        /// How pointer movement changes the value.
        drag: ValueDrag,
        /// Where the pointer was along the drag at the last move.
        last: f32,
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
    /// Dragging the point of an XY pad.
    Pad {
        module: u32,
        widget: usize,
        /// Screen pixels for the full range.
        throw: f32,
        /// Where the pointer was at the last move.
        last: Point,
        /// The positions last sent.
        position: (f32, f32),
    },
    /// Dragging a range's handle or band.
    Range {
        module: u32,
        widget: usize,
        /// What the press took.
        grab: RangeGrab,
        /// How pointer movement changes the position.
        drag: ValueDrag,
        /// Where the pointer was along the drag at the last move.
        last: f32,
        /// The positions last sent.
        position: (f32, f32),
    },
    /// Dragging a knob's range arc or a fader's bar: its depth, not its value.
    Depth {
        module: u32,
        widget: usize,
        /// Which of the control's values the depth belongs to.
        part: ControlPart,
        /// How pointer movement changes the depth.
        drag: ValueDrag,
        /// Where the pointer was along the drag at the last move.
        last: f32,
    },
    Selecting {
        start: Point,
        current: Point,
        base: BTreeSet<u32>,
    },
}

/// The cursor that says which way a value drag goes.
fn drag_cursor(drag: &ValueDrag) -> CursorShape {
    if drag.is_horizontal() {
        CursorShape::ResizeHorizontal
    } else {
        CursorShape::ResizeVertical
    }
}

impl Interaction {
    /// The cursor during this gesture, or `None` with no gesture. A value drag
    /// shows its own direction, so a horizontal fader keeps the cursor it had
    /// on hover.
    fn cursor(&self) -> Option<CursorShape> {
        Some(match self {
            Self::Panning { .. } | Self::Moving { .. } => CursorShape::Grabbing,
            Self::Resizing { .. } => CursorShape::ResizeDiagonal,
            Self::Turning { drag, .. } | Self::Depth { drag, .. } | Self::Range { drag, .. } => drag_cursor(drag),
            Self::Holding { .. } => CursorShape::Pointer,
            Self::Pad { .. } | Self::Wiring { .. } | Self::Selecting { .. } => CursorShape::Crosshair,
            Self::Idle => return None,
        })
    }
}

/// What a stationary click on a choice control does.
#[derive(Debug, Clone)]
enum ChoiceClick {
    Set(u32, usize, f32),
    Open(u32, usize),
    /// A press on a meter's clip light, which clears it on release.
    ClearClip(u32, usize),
}

/// The choice list that is open above the canvas, for the focused control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SelectPopup {
    pub(crate) highlight: usize,
    /// The first row that shows, for a list longer than it can show.
    pub(crate) scroll: usize,
    /// Wheel movement that has not yet made a whole row.
    wheel: f32,
}

/// The canvas's lasting input state: the camera, what the pointer is on,
/// the drag in progress, keyboard focus, and the timing of clicks and hover
/// text. It lives as long as the canvas is on screen. It holds nothing that
/// is only for drawing.
#[derive(Default)]
pub struct CanvasInput {
    camera: Camera,
    /// The canvas size at the last event, for key handling that needs it.
    size: Size,
    framed_key: Option<u64>,
    /// The level key last shown, so a change is seen on the next redraw.
    level_key: Option<u64>,
    interaction: Interaction,
    /// A release after CursorLeft cannot target the inspector until the
    /// window reports CursorEntered again.
    outside_window: bool,
    hover: Option<Hit>,
    /// Cursor in world coordinates, while over the canvas or dragging.
    cursor: Option<Point>,
    modifiers: Modifiers,
    /// Whether scopes were last reported as drawing full traces.
    reported_full_traces: Option<bool>,
    animation: Option<(Instant, Camera)>,
    /// The control with keyboard focus, and what the keys do on it.
    focus: focus::FocusState,
    /// True from a Tab press until the pointer moves: the focused control's
    /// hover text wins over the port or control under a resting pointer. It
    /// is not part of [`focus::FocusState`], because it outlives a control:
    /// Tab off a module and then Enter keep it.
    focus_first: bool,
    /// The output whose modulation lines stay on screen: its badge was
    /// clicked (`docs/design/modulation.md`, Lines).
    pinned: Option<PortRef>,
    /// When hover text shows (panels.md, Labels and hover text).
    hover_text: HoverTimer,
    /// The last press on a control, for double-click.
    last_click: Option<(Instant, u32, usize, ControlPart)>,
    /// The last press on empty canvas, in screen coordinates, for
    /// double-click.
    last_empty_click: Option<(Instant, Point)>,
    /// The last press on a tile's header, for double-click: when, the
    /// module, and where on screen.
    last_header_click: Option<(Instant, u32, Point)>,
    /// The last press on a meter with a clip light, for double-click: when,
    /// the module, the widget, and where on screen.
    last_meter_click: Option<(Instant, u32, usize, Point)>,
    /// The browser key last seen.
    browser_key: Option<u64>,
    /// The focus request key last seen.
    focus_key: Option<u64>,
    /// Buttons held from the keyboard, one slot for Space and one for Enter.
    ///
    /// Each key is its own hold. Releasing one leaves a button high while the
    /// other key is still down on it. Replacing a key's hold releases the
    /// button that key was holding, when nothing else is holding it.
    key_holds: [Option<KeyHold>; 2],
    /// Modules with lights on screen, as last reported.
    reported_lights: Option<Vec<u32>>,
}

/// The moment an event arrives in: where the pointer is, where the canvas
/// is, and the time. The driver knows these and the canvas does not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moment {
    /// The pointer in window logical pixels, or `None` when the canvas may
    /// not see it: it is outside the window, or under something drawn above
    /// the canvas.
    pub pointer: Option<Point>,
    /// The canvas's place and size in the window, in logical pixels.
    pub bounds: Rect,
    /// The time of the event. No handler reads the clock itself, so timing
    /// such as a double-click can be tested without waiting.
    pub now: Instant,
}

impl CanvasInput {
    /// Handles one event and says what it led to.
    pub fn handle(&mut self, model: &CanvasModel<'_>, event: &Event, moment: Moment) -> Reaction {
        self.run(model, Some(event), moment)
    }

    /// Handles something the driver received that means nothing to the
    /// canvas, such as the window moving. The steps that run on every event
    /// still run: the canvas notes its size, lets go of what a pop-up took
    /// over, and checks its hover text.
    pub fn pass(&mut self, model: &CanvasModel<'_>, moment: Moment) -> Reaction {
        self.run(model, None, moment)
    }

    /// The shared path of [`CanvasInput::handle`] and [`CanvasInput::pass`].
    fn run(&mut self, model: &CanvasModel<'_>, event: Option<&Event>, moment: Moment) -> Reaction {
        let mut out = Reaction::default();
        let before = self.focus.control();
        model.on_event(self, event, moment.pointer, moment.bounds, moment.now, &mut out);
        // Whatever moved the focus, the app hears of it once, here.
        let after = self.focus.control();
        if after != before {
            out.emit(CanvasEvent::FocusChanged(after));
        }
        out
    }

    /// The control with keyboard focus: its module ID and the widget's
    /// index in the module's panel.
    pub fn focused_control(&self) -> Option<(u32, usize)> {
        self.focus.control()
    }

    /// When the canvas next needs to wake with no input, if it does: the
    /// earlier of an edit group closing and hover text appearing.
    ///
    /// This is a fact of the state, so it can be asked at any time and
    /// gives the same answer until an event changes it. The driver wakes
    /// the canvas with an [`Event::Redraw`] at or after that time.
    pub fn next_deadline(&self, model: &CanvasModel<'_>) -> Option<Instant> {
        let edit_group = match model.edit_group {
            EditGroupView::Open { due } => Some(due),
            _ => None,
        };
        match (edit_group, self.hover_text.due()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (one, other) => one.or(other),
        }
    }

    /// The pointer shape to show: the one of the gesture in progress, or
    /// else, with the pointer over the canvas, the one for what is under it.
    pub fn cursor(&self, model: &CanvasModel<'_>, pointer_over: bool) -> CursorShape {
        match self.interaction.cursor() {
            Some(gesture) => gesture,
            None if pointer_over => model.hover_interaction(self),
            None => CursorShape::default(),
        }
    }

    /// The state for one still image: the camera, focus, pointer, pin, and
    /// drag that `still` describes, as if input had led there.
    pub fn still(model: &CanvasModel<'_>, still: Still, now: Instant) -> Self {
        model.still_state(still, now)
    }

    /// Where the canvas is in the window and how it looks at the patch, for
    /// the control channel.
    pub fn view(&self, bounds: Rect) -> CanvasView {
        CanvasView {
            bounds,
            camera: self.camera,
        }
    }

    /// What the pointer is on, what is pinned, and the drag in progress,
    /// for the control channel.
    pub fn pointer(&self, model: &CanvasModel<'_>) -> CanvasPointer {
        CanvasPointer {
            hover: self.hover.filter(|hit| *hit != Hit::Nothing),
            pinned: model.pin(self),
            drag: PointerDrag::of(&self.interaction),
        }
    }

    /// The open typed field and its text, if one is open, for the control
    /// channel.
    pub fn typed(&self) -> Option<CanvasTyped> {
        let (module, widget) = self.focus.control()?;
        let field = self.focus.typed()?;
        Some(CanvasTyped {
            module,
            widget,
            text: field.draft.text.clone(),
        })
    }

    /// The control whose value shows in place of its label: the one being
    /// turned, or else the one with keyboard focus.
    fn value_shown(&self) -> Option<(u32, usize)> {
        match self.interaction {
            Interaction::Turning { module, widget, .. } => Some((module, widget)),
            _ => self.focus.control(),
        }
    }

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
    key: Key,
    module: u32,
    widget: usize,
}

/// True when a press at `screen` is the second click of a double-click
/// after the `last` press.
fn double_click(last: Option<(Instant, Point)>, screen: Point, now: Instant) -> bool {
    last.is_some_and(|(at, point)| {
        now.saturating_duration_since(at) < DOUBLE_CLICK && point.distance(screen) < DOUBLE_CLICK_SLOP
    })
}

/// The pointer's place measured from the top-left corner of `bounds`.
fn relative(pointer: Option<Point>, bounds: Rect) -> Option<Point> {
    pointer.map(|p| Point::new(p.x - bounds.x, p.y - bounds.y))
}

/// How much a zoom level shows (panels.md, Level of detail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Detail {
    /// Port labels and panel text.
    Close,
    /// Titles and control shapes, no other text.
    Middle,
    /// Plain tiles: no text, no controls.
    Far,
}

pub(crate) fn detail(zoom: f32) -> Detail {
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
        .filter(|m| m.panel.widgets.iter().any(|w| !w.is_control()) && m.bounds().intersects(visible))
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

#[cfg(test)]
mod tests;
