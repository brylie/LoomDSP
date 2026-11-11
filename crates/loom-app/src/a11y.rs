//! The app's accessibility tree, for screen readers, through AccessKit
//! (ADR-0041, `docs/design/ui-layer.md`, section 7).
//!
//! The tree comes from the same plain-data models the window draws from
//! (`App::ui_snapshot`, ADR-0021), so it cannot drift from the screen. A
//! screen reader's actions go through the same messages as the pointer.
//!
//! The parts:
//!
//! - [`tree`] builds an AccessKit `TreeUpdate` from the view models. No
//!   platform code.
//! - [`actions`] turns an AccessKit `ActionRequest` into messages. No
//!   platform code.
//! - `macos` attaches AccessKit's adapter to the window Iced made. It is the
//!   only platform so far; the other platforms get an adapter when our own
//!   shell arrives (`docs/dev/leaving-iced-plan.md`, phase 2). Until then
//!   they build and test the tree, and send it nowhere.
//! - This file holds what joins them to the app: the [`Event`]s, the state
//!   kept between events ([`Access`]), and the Iced glue that finds the
//!   canvas in the window.
//!
//! The tree is built only while a screen reader listens, after a message
//! that may have changed what it reports, and only the nodes that changed
//! are sent. Nodes that show a live value are sent at most every 250 ms.
//! Nothing runs on a timer at rest (ui-layer.md, section 7, rule 7).

use crate::places::Places;
use accesskit::{ActionRequest, TreeUpdate};
use iced::{Subscription, Task};
use loom_core::ModuleId;
use loom_ui_core::IdTable;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub(crate) mod actions;
pub(crate) mod items;
#[cfg(target_os = "macos")]
mod macos;
pub(crate) mod tree;

#[cfg(test)]
mod tests;

#[cfg(target_os = "macos")]
use macos as platform;

/// Where no adapter exists yet: nothing attaches, nothing listens, and a
/// tree goes nowhere.
#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Event;
    use accesskit::TreeUpdate;
    use iced::{Subscription, Task};

    /// No adapter to attach.
    pub(super) fn attach() -> Task<Event> {
        Task::none()
    }

    /// No adapter, so no events.
    pub(super) fn events() -> Subscription<Event> {
        Subscription::none()
    }

    /// No adapter to send the tree to.
    pub(super) fn push(_whole: TreeUpdate, _changes: Option<TreeUpdate>) {}
}

/// The least time between two updates of a node that shows a live value:
/// a meter, a light, a readout, a scope.
pub(crate) const LIVE_INTERVAL: Duration = Duration::from_millis(250);

/// What a message means for the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Touch {
    /// The accessibility layer's own message: it rebuilds for itself.
    Own,
    /// Only live values moved: meters, lights, traces.
    Live,
    /// Anything else: a view model may have changed.
    Changed,
    /// A timer's poll, with the key from before it ran, or `None` while no
    /// screen reader listens. Most polls change nothing, so the tree is
    /// built only when the key differs afterwards.
    Poll(Option<PollKey>),
}

/// What the tree reports that a timer's poll can change, as a key that is
/// cheap to make and to compare. A poll (the audio device check, the MIDI
/// port check, autosave, the pickup check, the audio watchers' readings of
/// load and dropouts) runs several times a second for the whole session,
/// and building the tree for each would be idle work (NFR-PERF-04).
///
/// Every field stands for something the tree reads. When the tree starts
/// to report something new that a poll can change, add it here, or the
/// tree goes stale until the next ordinary message.
///
/// The inspector's items need no field of their own. They are built from
/// the selection, the graph, the inspector's drafts, and the panel values.
/// No poll changes the first three: selecting, editing, and typing are
/// ordinary messages, classed as a change. A poll changes the panel values
/// only through their generation, which `values` holds. So before adding
/// a poll that edits the graph or the selection, or a panel whose items
/// read something a poll changes, add that here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PollKey {
    /// The scene's generation: the tiles, their names and places, and the
    /// widgets on them. A device change that rebuilds tiles changes it.
    scene: u64,
    /// The panel values' generation: every control's value and text, and a
    /// meter's reading. It also changes for things the tree does not say,
    /// such as a pickup mark, which costs one build that sends nothing.
    values: u64,
    /// The toolbar, the right panel's open tab, and the bar at the bottom
    /// without its live readings, hashed. A poll that finds a device gone
    /// says so in the status line, turns Hold note off, and changes the
    /// audio label; a MIDI port that comes or goes changes the MIDI count.
    frame: u64,
    /// True while a panel owns the input, which hides the tiles.
    covered: bool,
    // The audio load and the dropout count are not here. They are live
    // values (ui-layer.md, section 3, rule 7): the message that brings
    // them is classed as live, and the live timer sends them.
    /// The control the tree reports focus on.
    focus: Option<(u32, usize)>,
}

/// What the accessibility layer tells the app.
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "only an adapter makes these, and only macOS has one until phase 2"
    )
)]
#[derive(Debug, Clone)]
pub(crate) enum Event {
    /// The adapter attached to the window, or why it did not.
    Attached(Result<(), String>),
    /// The window's scale factor, to turn logical pixels into physical ones.
    ScaleFactor(f32),
    /// A screen reader started listening.
    Activated,
    /// The screen reader left. No adapter on macOS says so
    /// (`accesskit_macos` 0.27.2); the adapters of phase 2 do.
    #[cfg_attr(
        target_os = "macos",
        allow(dead_code, reason = "accesskit_macos 0.27.2 never says a screen reader left")
    )]
    Deactivated,
    /// Live values changed a while ago: time to send them.
    LiveTick,
    /// Where the canvas and the window's named parts are, for a rebuild.
    Located(Places),
    /// A screen reader asked for an action.
    Action(ActionRequest),
}

/// What the app keeps for the accessibility tree between events. It
/// outlives the open patch: the window stays when a patch is replaced.
#[derive(Debug)]
pub(crate) struct Access {
    /// The node ID of every element in the tree.
    ids: IdTable,
    /// True once a screen reader asked for the tree.
    active: bool,
    /// The window's scale factor: AccessKit wants physical pixels.
    scale: f32,
    /// What the screen reader was last sent.
    sent: tree::Sent,
    /// True while a rebuild waits for the window to say where the canvas
    /// is. Messages that arrive meanwhile are in that rebuild.
    locating: bool,
    /// True while live values have changed and were not sent yet. The
    /// timer that sends them runs only then.
    live_dirty: bool,
    /// True when the next rebuild sends live values too.
    flush_live: bool,
    /// The control with keyboard focus on the canvas, as the canvas last
    /// reported it: module ID and panel index.
    focus: Option<(u32, usize)>,
    /// A screen reader's request to focus a control: a key that changes
    /// with each request, and the control. The canvas acts when the key
    /// changes.
    focus_request: (u64, Option<(u32, usize)>),
    /// True with `--measure-tree`: each build is timed.
    timed: bool,
}

/// How many times this process built the tree while `--measure-tree` timed
/// it, and how long that took. Process-wide, as the canvas counters are,
/// because the report runs on its own thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BuildTimes {
    /// The builds so far.
    pub count: u64,
    /// The time of all of them.
    pub total: Duration,
    /// The longest one.
    pub worst: Duration,
}

static BUILD_COUNT: AtomicU64 = AtomicU64::new(0);
static BUILD_NANOS: AtomicU64 = AtomicU64::new(0);
static BUILD_WORST_NANOS: AtomicU64 = AtomicU64::new(0);

impl BuildTimes {
    /// The counts of this process so far.
    pub(crate) fn process() -> Self {
        Self {
            count: BUILD_COUNT.load(Ordering::Relaxed),
            total: Duration::from_nanos(BUILD_NANOS.load(Ordering::Relaxed)),
            worst: Duration::from_nanos(BUILD_WORST_NANOS.load(Ordering::Relaxed)),
        }
    }

    /// The time of one build, on average. Zero before the first.
    pub(crate) fn average(self) -> Duration {
        match u32::try_from(self.count) {
            Ok(count) if count > 0 => self.total / count,
            _ => Duration::ZERO,
        }
    }

    /// Adds one build that took `time`.
    fn record(time: Duration) {
        let nanos = u64::try_from(time.as_nanos()).unwrap_or(u64::MAX);
        BUILD_COUNT.fetch_add(1, Ordering::Relaxed);
        BUILD_NANOS.fetch_add(nanos, Ordering::Relaxed);
        BUILD_WORST_NANOS.fetch_max(nanos, Ordering::Relaxed);
    }
}

impl Default for Access {
    fn default() -> Self {
        Self {
            ids: IdTable::new(),
            active: false,
            scale: 1.0,
            sent: tree::Sent::default(),
            locating: false,
            live_dirty: false,
            flush_live: false,
            focus: None,
            focus_request: (0, None),
            timed: false,
        }
    }
}

impl Access {
    /// The open patch was replaced. Its tiles are new elements, even where a
    /// module ID repeats, so they must get new node IDs: a screen reader
    /// resting on the old module 4 must not read the new one in its place
    /// (ui-layer.md, section 4, rule 4).
    pub(crate) fn patch_replaced(&mut self) {
        self.ids.forget(&tree::canvas_id());
        // The old patch's module IDs mean nothing in the new one.
        self.focus = None;
        self.focus_request.1 = None;
    }

    /// Notes which control has keyboard focus on the canvas.
    pub(crate) fn set_focus(&mut self, control: Option<(u32, usize)>) {
        self.focus = control;
    }

    /// The focus request the canvas is shown: its key and its control.
    pub(crate) fn focus_request(&self) -> (u64, Option<(u32, usize)>) {
        self.focus_request
    }

    /// True while a screen reader listens.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// The layer as `--measure-tree` runs it: building the tree as if a
    /// screen reader were listening from the start, and timing each build.
    /// For measuring only. A normal run starts with [`Access::default`].
    pub(crate) fn measured() -> Self {
        Self {
            active: true,
            timed: true,
            ..Self::default()
        }
    }

    /// True while live values wait to be sent.
    pub(crate) fn live_waits(&self) -> bool {
        self.active && self.live_dirty
    }
}

/// The task that attaches the accessibility layer to the app's window at
/// start. Nothing where no adapter exists yet.
pub(crate) fn attach() -> Task<Event> {
    platform::attach()
}

/// Asks the window where the canvas and its named parts are.
fn locate() -> Task<Event> {
    iced_runtime::task::widget(crate::places::Collect::default()).map(Event::Located)
}

/// The window's new scale factor, from a window event.
fn rescaled(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Event> {
    match event {
        iced::Event::Window(iced::window::Event::Rescaled(scale)) => Some(Event::ScaleFactor(scale)),
        _ => None,
    }
}

impl crate::App {
    /// Events from the adapter. While a screen reader listens: the window's
    /// scale factor when it changes, and a timer only while live values
    /// wait to be sent. With no screen reader, and at rest, nothing runs.
    pub(crate) fn a11y_subscription(&self) -> Subscription<Event> {
        let live = if self.a11y.live_waits() {
            iced::time::every(LIVE_INTERVAL).map(|_| Event::LiveTick)
        } else {
            Subscription::none()
        };
        let scale = if self.a11y.is_active() {
            iced::event::listen_with(rescaled)
        } else {
            Subscription::none()
        };
        Subscription::batch([platform::events(), live, scale])
    }

    /// Notes a message the app just handled. Live values wait for the
    /// timer. Anything else asks for a rebuild now, unless one is on its
    /// way. With no screen reader this does nothing.
    pub(crate) fn a11y_after(&mut self, touch: Touch) -> Task<Event> {
        if !self.a11y.active {
            return Task::none();
        }
        match touch {
            Touch::Own => Task::none(),
            Touch::Live => {
                self.a11y.live_dirty = true;
                Task::none()
            }
            Touch::Changed => self.a11y_rebuild(),
            Touch::Poll(before) if before == Some(self.a11y_poll_key()) => Task::none(),
            Touch::Poll(_) => self.a11y_rebuild(),
        }
    }

    /// The key a poll is compared by, before and after it runs.
    pub(crate) fn a11y_poll_key(&self) -> PollKey {
        use std::hash::{Hash, Hasher};
        let mut frame = std::collections::hash_map::DefaultHasher::new();
        let bars = self.bars_model();
        for button in self.toolbar_model().buttons {
            (button.name, button.label, button.enabled, button.hover, button.on).hash(&mut frame);
        }
        (self.right_tab == crate::matrix::Tab::Matrix).hash(&mut frame);
        bars.dev.map(|dev| (dev.measuring, dev.animating)).hash(&mut frame);
        for item in bars.items.iter().filter(|item| item.kind != crate::bars::Kind::Live) {
            (item.name, &item.text, &item.hover).hash(&mut frame);
        }
        for field in bars.kept.iter().chain(std::iter::once(&bars.status)) {
            (
                &field.shown,
                &field.hover,
                field.severity as u8,
                field.dismiss.is_some(),
            )
                .hash(&mut frame);
        }
        PollKey {
            scene: self.session.scene.generation(),
            values: self.session.values.generation(),
            frame: frame.finish(),
            covered: !self.pointer_reaches_window(),
            focus: self.a11y.focus,
        }
    }

    /// Asks the window where the canvas is, to rebuild the tree when the
    /// answer comes. One request at a time.
    fn a11y_rebuild(&mut self) -> Task<Event> {
        if !self.a11y.active || self.a11y.locating {
            return Task::none();
        }
        self.a11y.locating = true;
        locate()
    }

    /// Builds the tree and gives what the screen reader must be sent: the
    /// whole tree, and the nodes that changed since the last send, if any.
    /// Live values go only when `flush_live` was set.
    pub(crate) fn a11y_changes(&mut self, places: &Places) -> (TreeUpdate, Option<TreeUpdate>) {
        let view = places.canvas.as_ref();
        let snapshot = self.ui_snapshot();
        let frames = self.a11y_frames();
        let ids = self.a11y_widget_ids(&snapshot);
        let whole = tree::tree(&snapshot, &frames, &ids, places, self.a11y.scale, &mut self.a11y.ids);
        let flush = std::mem::take(&mut self.a11y.flush_live);
        if flush {
            self.a11y.live_dirty = false;
        }
        let table = &self.a11y.ids;
        // The canvas in the window, in physical pixels, as node bounds are.
        let screen = view.map(|view| {
            let s = f64::from(self.a11y.scale);
            let b = view.bounds;
            accesskit::Rect {
                x0: f64::from(b.x) * s,
                y0: f64::from(b.y) * s,
                x1: f64::from(b.x + b.width) * s,
                y1: f64::from(b.y + b.height) * s,
            }
        });
        let canvas = tree::canvas_id();
        let hold = |node: accesskit::NodeId, built: &accesskit::Node| {
            let Some(id) = table.id(node.0) else {
                return tree::Hold::Send;
            };
            // Below a panel that owns the input, what is inside a tile
            // keeps the value last sent. The tile itself is sent, hidden.
            if ids.is_covered() && id.is_under(&canvas) && tree::tile_of(id).is_none() && *id != canvas {
                return tree::Hold::Skip;
            }
            // The bar's readings are always on screen.
            if tree::is_live_bar_item(id) {
                return if flush { tree::Hold::Send } else { tree::Hold::Wait };
            }
            if !ids.is_live(id) {
                return tree::Hold::Send;
            }
            // A live value off screen keeps the value last sent.
            let off_screen = match (screen, built.bounds()) {
                (Some(screen), Some(bounds)) => {
                    bounds.x1 <= screen.x0 || bounds.x0 >= screen.x1 || bounds.y1 <= screen.y0 || bounds.y0 >= screen.y1
                }
                _ => false,
            };
            if off_screen {
                tree::Hold::Skip
            } else if flush {
                tree::Hold::Send
            } else {
                tree::Hold::Wait
            }
        };
        let (changes, held) = self.a11y.sent.changes(&whole, hold);
        if held {
            self.a11y.live_dirty = true;
        }
        (whole, changes)
    }

    /// Where the accessibility tree says the element named `id` is, in the
    /// window, in logical pixels. `None` when the tree has no such node, or
    /// the node has no bounds. The control channel's `locate` reads it, so
    /// a script and a screen reader find one element in one place.
    ///
    /// The tree is built at scale 1 with a table of its own, so asking
    /// changes nothing a screen reader was told.
    pub(crate) fn a11y_bounds(&self, id: &loom_ui_core::UiId, places: &Places) -> Option<loom_ui_core::Rect> {
        let snapshot = self.ui_snapshot();
        let frames = self.a11y_frames();
        let ids = self.a11y_widget_ids(&snapshot);
        let mut table = IdTable::new();
        let whole = tree::tree(&snapshot, &frames, &ids, places, 1.0, &mut table);
        let node = accesskit::NodeId(table.number(id));
        let bounds = whole.nodes.iter().find(|(found, _)| *found == node)?.1.bounds()?;
        Some(loom_ui_core::Rect::new(
            loom_ui_core::Point::new(bounds.x0 as f32, bounds.y0 as f32),
            loom_ui_core::Size::new((bounds.x1 - bounds.x0) as f32, (bounds.y1 - bounds.y0) as f32),
        ))
    }

    /// The UI IDs of every tile's panel widgets, from the names that find
    /// them again (`targets.rs`).
    pub(crate) fn a11y_widget_ids(&self, snapshot: &crate::snapshot::UiSnapshot) -> tree::WidgetIds {
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        let mut ids = tree::WidgetIds::default();
        if !self.pointer_reaches_window() {
            ids.set_covered();
        }
        for tile in &snapshot.tiles {
            let part = crate::surface::part_tile(hierarchy, tile.id);
            let node = graph.node(ModuleId(part));
            let on_module = node.is_some();
            // A slot's controls are its part's: keyed under the part, so a
            // swap makes new elements.
            let under = if part == tile.id {
                tree::tile_id(tile.id)
            } else if let Some(node) = node {
                tree::part_id(tile.id, tree::SlotPart::Module(node.descriptor.id))
            } else if let Some(key) = hierarchy.key_at(&hierarchy.path_of(ModuleId(part))) {
                tree::part_id(tile.id, tree::SlotPart::SubPatch(key))
            } else {
                // A part with no key cannot be told from another: its
                // controls get no IDs, so no nodes, rather than shared ones.
                continue;
            };
            let widgets = (0..tile.widgets.len()).map(|index| {
                // From the panel's description, not from how the widget is
                // drawn, so the kind agrees with the names `locate` takes.
                let kind = if !crate::surface::is_control(graph, hierarchy, tile.id, index) {
                    tree::WidgetKey::Display
                } else if on_module {
                    tree::WidgetKey::Param
                } else {
                    tree::WidgetKey::Exposed
                };
                (kind, crate::surface::widget_name(graph, hierarchy, tile.id, index))
            });
            ids.add_tile(tile.id, &under, widgets);
            if let Some((module, widget)) = self.a11y.focus.filter(|(module, _)| *module == tile.id) {
                ids.set_focus(module, widget);
            }
            // What each widget is: its kind from the scene the canvas
            // draws, and its range from the parameter the control moves.
            let views = self
                .session
                .scene
                .module(tile.id)
                .map(|view| view.panel.widgets.as_slice());
            for (index, view) in views.unwrap_or_default().iter().enumerate() {
                let name = if view.label.is_empty() {
                    crate::surface::widget_name(graph, hierarchy, tile.id, index).unwrap_or_default()
                } else {
                    view.label.clone()
                };
                let spec =
                    |part| crate::surface::control_target(graph, hierarchy, tile.id, index, part).map(|t| t.spec);
                ids.set_detail(
                    tile.id,
                    index,
                    tree::Detail {
                        kind: view.kind.clone(),
                        name,
                        main: spec(loom_canvas::ControlPart::Main),
                        second: spec(loom_canvas::ControlPart::Second),
                        reading: self.session.values.get(tile.id, index).and_then(|value| value.reading),
                    },
                );
            }
        }
        ids
    }

    /// The tree as it is now, with the canvas placed by `view`.
    #[cfg(test)]
    pub(crate) fn a11y_tree(&mut self, places: &Places) -> TreeUpdate {
        let snapshot = self.ui_snapshot();
        let frames = self.a11y_frames();
        let ids = self.a11y_widget_ids(&snapshot);
        tree::tree(&snapshot, &frames, &ids, places, self.a11y.scale, &mut self.a11y.ids)
    }

    /// What the tree reports of the parts around the canvas: the bar, the
    /// open panel's parts, and the text field with keyboard focus. The
    /// inspector's parts come from the same model its view draws.
    pub(crate) fn a11y_frames(&self) -> tree::Frames {
        let panel = if self.right_tab == crate::matrix::Tab::Inspector {
            crate::inspector::access::items(&self.inspector_model())
        } else {
            Vec::new()
        };
        tree::Frames {
            bars: self.bars_model(),
            panel,
            focused_field: self.text_focus.field.clone(),
        }
    }

    /// A screen reader's action on a part of the open panel: the messages
    /// its item says that action sends, or a focus request for a field.
    /// `None` for a node that is not one of the panel's parts.
    ///
    /// SetValue, Increment, and Decrement on a slider join the screen
    /// reader's undo group, as on the canvas. Any other action ends the
    /// group first, also when it changes nothing (section 7, rule 6).
    fn a11y_panel_action(
        &mut self,
        request: &ActionRequest,
        at: std::time::Instant,
    ) -> Option<Vec<Task<crate::Message>>> {
        use crate::edits::ReaderEvent;
        let id = self.a11y.ids.id(request.target_node.0)?.clone();
        if !id.is_under(&loom_ui_core::UiId::panel("inspector")) {
            return None;
        }
        let frames = self.a11y_frames();
        let item = items::find(&frames.panel, &id)?.clone();
        if request.action == accesskit::Action::Focus {
            let Some(field) = item.focus else {
                return Some(Vec::new());
            };
            self.reader_event(ReaderEvent::Close);
            let task = iced::widget::operation::focus(field)
                .chain(crate::editing::text_focus().map(crate::Message::TextFocus));
            return Some(vec![task]);
        }
        let messages = items::messages(&item, request.action, request.data.as_ref());
        let joins = item.groups && actions::joins_group(request.action);
        if messages.is_empty() {
            if !joins {
                self.reader_event(ReaderEvent::Close);
            }
            return Some(Vec::new());
        }
        self.reader_event(if joins {
            ReaderEvent::Action {
                node: request.target_node.0,
                at,
            }
        } else {
            ReaderEvent::Close
        });
        let tasks = messages.into_iter().map(|message| self.update(message)).collect();
        self.reader_event(ReaderEvent::Applied);
        Some(tasks)
    }

    /// The message for a Click on a part of the window outside the canvas:
    /// a toolbar button that is enabled, one of the right panel's tabs, a
    /// × in the status line, or a developer switch. `None` for any other
    /// node or action, and for a button that is disabled now.
    pub(crate) fn a11y_frame_message(&self, request: &ActionRequest) -> Option<crate::Message> {
        use crate::Message;
        // What a pointer press could not reach, a screen reader's Click
        // does not reach either.
        if request.action != accesskit::Action::Click || !self.pointer_reaches_window() {
            return None;
        }
        let id = self.a11y.ids.id(request.target_node.0)?;
        match tree::frame_part(id)? {
            tree::FramePart::Button(name) => {
                let model = self.toolbar_model();
                let button = model.buttons.iter().find(|button| button.name == name)?;
                button.enabled.then(|| crate::toolbar::message(name)).flatten()
            }
            tree::FramePart::Tab(tab) => Some(Message::Matrix(crate::matrix::Event::Tab(tab))),
            tree::FramePart::Dismiss { kept } => {
                let bars = self.bars_model();
                let field = if kept { bars.kept? } else { bars.status };
                field.dismiss.map(Message::DismissError)
            }
            tree::FramePart::Measure => self.dev_tools.then_some(Message::Measure(!self.measuring)),
            tree::FramePart::MoveCamera => self.dev_tools.then_some(Message::Animate(!self.animating)),
        }
    }

    /// Applies a screen reader's request that arrived at `at`, and gives
    /// the tasks its messages made.
    ///
    /// A SetValue, Increment, or Decrement joins the screen reader's undo
    /// group, or opens one. Any other action that changes something ends
    /// the group first and is a step of its own.
    pub(crate) fn a11y_action(&mut self, request: &ActionRequest, at: std::time::Instant) -> Vec<Task<crate::Message>> {
        use crate::edits::ReaderEvent;
        // An open panel owns input, as it does for the pointer: the
        // canvas behind it does not change.
        if !self.pointer_reaches_window() {
            return Vec::new();
        }
        // A held arrow key owns the edit in progress, and may step again.
        // A second owner would share its undo step, so the action waits:
        // it does nothing, as Undo does nothing while a key is held.
        if self.session.edits.arrow_group() == crate::edits::ArrowGroup::Held {
            return Vec::new();
        }
        let snapshot = self.ui_snapshot();
        let ids = self.a11y_widget_ids(&snapshot);
        // A part of the open panel acts as its item says.
        if let Some(tasks) = self.a11y_panel_action(request, at) {
            return tasks;
        }
        if request.action == accesskit::Action::Focus {
            // Focus on a control moves the canvas's keyboard focus there.
            // The canvas selects the control's tile as a press on it does.
            // It changes nothing in the patch, and Focus on a tile does
            // nothing (ui-layer.md, section 7, rules 9 and 20).
            if let Some(control) = actions::focus_target(&snapshot, &ids, &self.a11y.ids, request) {
                self.a11y.focus_request = (self.a11y.focus_request.0 + 1, Some(control));
                // The keys go with the focus, by the path a press on the
                // canvas takes: the matrix lets go of them, and a text
                // field that has them closes, leaving its draft as a press
                // leaves it (dropped, unsubmitted). A press closes the
                // field itself; with no press, the app closes it, then
                // reads focus back as after a press.
                self.canvas_event(loom_canvas::CanvasEvent::PointerDown);
                if self.text_focus.any {
                    return vec![
                        crate::editing::unfocus_text_fields().chain(Task::done(crate::Message::TextFocusCheck)),
                    ];
                }
            }
            return Vec::new();
        }
        // A button, a tab, or a switch outside the canvas sends the message
        // its press sends. Anything else is on the canvas.
        let messages = match self.a11y_frame_message(request) {
            Some(message) => vec![message],
            None => actions::messages(&snapshot, &ids, &self.a11y.ids, request),
        };
        if messages.is_empty() {
            // A Click that changes nothing, such as one on the option
            // already chosen, still ends an open group, as a pointer
            // press does. A node that is gone ends nothing.
            if !actions::joins_group(request.action) && actions::names_a_part(&ids, &self.a11y.ids, request) {
                self.reader_event(ReaderEvent::Close);
            }
            return Vec::new();
        }
        self.reader_event(if actions::joins_group(request.action) {
            ReaderEvent::Action {
                node: request.target_node.0,
                at,
            }
        } else {
            ReaderEvent::Close
        });
        let tasks = messages.into_iter().map(|message| self.update(message)).collect();
        // The step's release, at once: the group now waits for the next.
        self.reader_event(ReaderEvent::Applied);
        tasks
    }

    /// Handles the accessibility layer's events. Actions go through the same
    /// messages as the pointer, so undo, MIDI feedback, and autosave see
    /// them as ordinary edits.
    pub(crate) fn a11y_event(&mut self, event: Event) -> Task<crate::Message> {
        use crate::Message;
        match event {
            Event::Attached(Ok(())) => Task::none(),
            Event::Attached(Err(e)) => {
                eprintln!("loom-app: no accessibility tree for screen readers: {e}");
                Task::none()
            }
            Event::ScaleFactor(scale) => {
                // Every node's bounds change with it, so all are sent again.
                let changed = self.a11y.scale != scale;
                self.a11y.scale = scale;
                if changed {
                    self.a11y_rebuild().map(Message::A11y)
                } else {
                    Task::none()
                }
            }
            Event::Activated => {
                self.a11y.active = true;
                // The screen reader has nothing yet: send the whole tree.
                self.a11y.sent.clear();
                self.a11y_rebuild().map(Message::A11y)
            }
            Event::Deactivated => {
                self.a11y.active = false;
                self.a11y.live_dirty = false;
                self.a11y.sent.clear();
                Task::none()
            }
            Event::LiveTick => {
                self.a11y.flush_live = true;
                self.a11y_rebuild().map(Message::A11y)
            }
            Event::Located(places) => {
                self.a11y.locating = false;
                if self.a11y.active {
                    let start = self.a11y.timed.then(std::time::Instant::now);
                    let (whole, changes) = self.a11y_changes(&places);
                    if let Some(start) = start {
                        BuildTimes::record(start.elapsed());
                    }
                    platform::push(whole, changes);
                }
                Task::none()
            }
            Event::Action(request) => {
                let mut tasks = self.a11y_action(&request, std::time::Instant::now());
                // Show the new value at once.
                tasks.push(self.a11y_rebuild().map(Message::A11y));
                Task::batch(tasks)
            }
        }
    }
}
