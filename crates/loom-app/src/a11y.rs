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

use accesskit::{ActionRequest, TreeUpdate};
use iced::advanced::widget::operation::Outcome;
use iced::advanced::widget::{Id, Operation};
use iced::{Rectangle, Subscription, Task};
use loom_canvas::CanvasView;
use loom_core::ModuleId;
use loom_ui_core::IdTable;
use std::any::Any;
use std::time::Duration;

pub(crate) mod actions;
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
    /// Where the canvas is in the window, for a rebuild.
    Located(Option<CanvasView>),
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
    }

    /// True while a screen reader listens.
    pub(crate) fn is_active(&self) -> bool {
        self.active
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

/// Asks the window where the canvas is.
fn locate() -> Task<Event> {
    iced_runtime::task::widget(FindCanvas(None)).map(Event::Located)
}

/// The window's new scale factor, from a window event.
fn rescaled(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Event> {
    match event {
        iced::Event::Window(iced::window::Event::Rescaled(scale)) => Some(Event::ScaleFactor(scale)),
        _ => None,
    }
}

/// A widget operation that finds the patch canvas and its camera.
struct FindCanvas(Option<CanvasView>);

impl Operation<Option<CanvasView>> for FindCanvas {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<CanvasView>>)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(view) = state.downcast_ref::<CanvasView>() {
            self.0 = Some(*view);
        }
    }

    fn finish(&self) -> Outcome<Option<CanvasView>> {
        Outcome::Some(self.0)
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
    pub(crate) fn a11y_changes(&mut self, view: Option<&CanvasView>) -> (TreeUpdate, Option<TreeUpdate>) {
        let snapshot = self.ui_snapshot();
        let ids = self.a11y_widget_ids(&snapshot);
        let whole = tree::tree(&snapshot, &ids, view, self.a11y.scale, &mut self.a11y.ids);
        let flush = std::mem::take(&mut self.a11y.flush_live);
        if flush {
            self.a11y.live_dirty = false;
        }
        let table = &self.a11y.ids;
        let hold = |node: accesskit::NodeId| !flush && table.id(node.0).is_some_and(|id| ids.is_live(id));
        let (changes, held) = self.a11y.sent.changes(&whole, hold);
        if held {
            self.a11y.live_dirty = true;
        }
        (whole, changes)
    }

    /// The UI IDs of every tile's panel widgets, from the names that find
    /// them again (`targets.rs`).
    pub(crate) fn a11y_widget_ids(&self, snapshot: &crate::snapshot::UiSnapshot) -> tree::WidgetIds {
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        let mut ids = tree::WidgetIds::default();
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
            } else {
                let key = hierarchy.key_at(&hierarchy.path_of(ModuleId(part))).unwrap_or_default();
                tree::part_id(tile.id, tree::SlotPart::SubPatch(key))
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
                    },
                );
            }
        }
        ids
    }

    /// The tree as it is now, with the canvas placed by `view`.
    #[cfg(test)]
    pub(crate) fn a11y_tree(&mut self, view: Option<&CanvasView>) -> TreeUpdate {
        let snapshot = self.ui_snapshot();
        let ids = self.a11y_widget_ids(&snapshot);
        tree::tree(&snapshot, &ids, view, self.a11y.scale, &mut self.a11y.ids)
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
        if !self.no_overlay() {
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
        let messages = actions::messages(&snapshot, &ids, &self.a11y.ids, request);
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
            Event::Located(view) => {
                self.a11y.locating = false;
                if self.a11y.active {
                    let (whole, changes) = self.a11y_changes(view.as_ref());
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
