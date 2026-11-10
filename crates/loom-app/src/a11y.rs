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
//! This is plan step 1.4, first slice: the spike's code as a module, with
//! the spike's coverage (status line, tiles, tile controls). While a screen
//! reader listens the whole tree is still rebuilt on a timer. The next slice
//! rebuilds it when it changes and sends only the changed nodes.

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
    pub(super) fn push(_update: TreeUpdate) {}
}

/// How often the tree is rebuilt while a screen reader listens.
const REFRESH: Duration = Duration::from_millis(500);

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
    /// Time to rebuild the tree.
    Refresh,
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
}

impl Default for Access {
    fn default() -> Self {
        Self {
            ids: IdTable::new(),
            active: false,
            scale: 1.0,
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
    /// Events from the adapter, and a refresh timer while a screen reader
    /// listens.
    pub(crate) fn a11y_subscription(&self) -> Subscription<Event> {
        let timer = if self.a11y.active {
            iced::time::every(REFRESH).map(|_| Event::Refresh)
        } else {
            Subscription::none()
        };
        Subscription::batch([platform::events(), timer])
    }

    /// The UI IDs of every tile's panel widgets, from the names that find
    /// them again (`targets.rs`).
    pub(crate) fn a11y_widget_ids(&self, snapshot: &crate::snapshot::UiSnapshot) -> tree::WidgetIds {
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        let mut ids = tree::WidgetIds::default();
        for tile in &snapshot.tiles {
            let part = crate::surface::part_tile(hierarchy, tile.id);
            let on_module = graph.node(ModuleId(part)).is_some();
            let widgets = tile.widgets.iter().enumerate().map(|(index, widget)| {
                (
                    widget.kind,
                    crate::surface::widget_name(graph, hierarchy, tile.id, index),
                )
            });
            ids.add_tile(tile.id, on_module, widgets);
        }
        ids
    }

    /// The tree as it is now, with the canvas placed by `view`.
    pub(crate) fn a11y_tree(&mut self, view: Option<&CanvasView>) -> TreeUpdate {
        let snapshot = self.ui_snapshot();
        let ids = self.a11y_widget_ids(&snapshot);
        tree::tree(&snapshot, &ids, view, self.a11y.scale, &mut self.a11y.ids)
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
                self.a11y.scale = scale;
                Task::none()
            }
            Event::Activated => {
                self.a11y.active = true;
                locate().map(Message::A11y)
            }
            Event::Refresh => locate().map(Message::A11y),
            Event::Located(view) => {
                let update = self.a11y_tree(view.as_ref());
                platform::push(update);
                Task::none()
            }
            Event::Action(request) => {
                // An open panel owns input, as it does for the pointer: the
                // canvas behind it does not change.
                if !self.no_overlay() {
                    return Task::none();
                }
                let snapshot = self.ui_snapshot();
                let ids = self.a11y_widget_ids(&snapshot);
                let messages = actions::messages(&snapshot, &ids, &self.a11y.ids, &request);
                let mut tasks: Vec<_> = messages.into_iter().map(|message| self.update(message)).collect();
                // Show the new value at once, not at the next refresh.
                tasks.push(locate().map(Message::A11y));
                Task::batch(tasks)
            }
        }
    }
}
