//! The app, as the runtime sees it.

use crate::effect::Effects;
use crate::event::{AccessEvent, WindowEvent, WindowState};
use crate::focus::AccessNodes;
use crate::view::{Layer, View};
use crate::waker::Waker;
use crate::window::Window;
use loom_ui_core::{IdTable, Size, UiId};
use std::time::Instant;

/// The program built on the runtime: one message type and one `update`.
///
/// `update` is the only place app state changes (ui-layer.md, section 2,
/// rule 1). The other methods only read it.
pub trait App {
    /// The app's message: the only way its state changes. It is `Send`
    /// because watcher threads and background work send messages.
    type Message: Send + 'static;

    /// Applies one message and returns what should happen next.
    fn update(&mut self, message: Self::Message, cx: &Context<'_, Self::Message>) -> Effects<Self::Message>;

    /// Describes the window, one panel at a time.
    fn view(&self, view: &mut View);

    /// The window's title and limits. Read after each turn that ran an
    /// update, and passed to the driver only when it changed.
    fn window(&self) -> WindowSpec;

    /// The next moment the app needs to wake with no event, and the message
    /// it gets then. Built from state after each turn that ran an update.
    /// Handling that message must move the deadline on, or it is due again
    /// at once.
    fn next_deadline(&self) -> Option<Deadline<Self::Message>> {
        None
    }

    /// The message for a window event, if the app cares about it: a close
    /// request, a dropped file. It reads state and changes none.
    fn window_event(&self, event: &WindowEvent) -> Option<Self::Message> {
        let _ = event;
        None
    }

    /// The message for something a screen reader did, if the app wants
    /// one. An app with no accessibility tree wants none.
    fn accessibility_event(&self, event: &AccessEvent) -> Option<Self::Message> {
        let _ = event;
        None
    }

    /// Which layers take input now, and whether the canvas has the
    /// keyboard (ui-layer.md, section 5, rule 2). Read before each pointer
    /// event and each Tab. The default lets every layer take input, with
    /// the canvas not focused.
    fn input(&self) -> InputState {
        InputState::default()
    }

    /// The message for a click on the runtime's element with UI ID `id`,
    /// if a click there does something. A click is a left press and a left
    /// release on the same element. The app turns it into a message by the
    /// UI ID, as it does a screen reader's Click action on the same node
    /// (section 7, rule 6). It reads state and changes none.
    fn clicked(&self, id: &UiId) -> Option<Self::Message> {
        let _ = id;
        None
    }

    /// How hover text looks (ui-layer.md, section 5, rule 11), from the
    /// app's style tokens. The default is plain and readable.
    fn hint_style(&self) -> crate::HintStyle {
        crate::HintStyle::default()
    }

    /// The message for a move of the runtime's keyboard focus, if the app
    /// wants to know: to rebuild its accessibility tree, which reports the
    /// focus (section 7, rule 7). `focus` is the runtime's element that has
    /// it now, or `None`. The runtime reports each change of the focused
    /// element once, whatever moved it (Tab, a press, a layout that settled
    /// it, or a focus effect), on the turn after the change; a ring shown
    /// or hidden on the same element is no change. It reads state and
    /// changes none.
    fn focus_moved(&self, focus: Option<&UiId>) -> Option<Self::Message> {
        let _ = focus;
        None
    }

    /// The message for something a text field of the runtime's did, if
    /// the app wants one: `id` is the field's UI ID. The field's text is
    /// the app's (ui-layer.md, section 8, rule 2): an app that wants an
    /// edit to stay takes [`FieldEvent::Edited`] into its model, so the next
    /// view shows it; one that does not, by giving `None`, sees the field go
    /// back to its old text at the next layout. It reads state and changes
    /// none.
    ///
    /// [`FieldEvent::Edited`]: crate::FieldEvent::Edited
    fn field_event(&self, id: &UiId, event: &crate::FieldEvent) -> Option<Self::Message> {
        let _ = (id, event);
        None
    }

    /// The UI ID of the node `node` names, from the app's node table, so
    /// the runtime can carry out a screen reader's text actions on its own
    /// fields (ui-layer.md, section 7, rule 10). An app with no table, or
    /// none of the runtime's fields, gives `None`, and every action goes
    /// to [`App::accessibility_event`].
    fn access_target(&self, node: accesskit::NodeId) -> Option<UiId> {
        let _ = node;
        None
    }

    /// The message for "what a field reports to a screen reader changed
    /// with no message of its own": its caret, its selection, or its
    /// scroll (section 7, rule 7). The runtime asks at most once per turn,
    /// after that turn's updates have run, field messages included, so the
    /// answer is built from a model that holds them, and runs it as one
    /// more update; the app can send a new tree there. An app with no screen reader listening gives `None`, which
    /// costs nothing.
    fn access_changed(&self) -> Option<Self::Message> {
        None
    }

    /// True while something on screen moves and needs a frame each display
    /// refresh. A frame draws and runs no update.
    fn animating(&self) -> bool {
        false
    }
}

/// Who takes input now, as the app says ([`App::input`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputState {
    /// The lowest layer that takes input. A layer below it takes no pointer
    /// event, shows no hover, and is not reached by Tab: an open question
    /// says [`Layer::Question`], an open overlay [`Layer::Overlays`]. The
    /// default, [`Layer::Canvas`], lets every layer take input.
    pub layer: Layer,
    /// True while the canvas has the keyboard: it keeps its own Tab, so
    /// Tab does not enter the runtime's stops (section 6, rule 1). The
    /// default is false.
    pub canvas_focused: bool,
    /// True while the target's own Tab stops read before the runtime's
    /// (ui-layer.md, section 6, rule 2, between the toolkits): Tab with
    /// nothing focused is then the target's, and enters the runtime's
    /// stops only when the target hands it over. Shift+Tab from nothing
    /// still enters the runtime's last stop, the last in reading order.
    /// The default is false: the runtime's stops come first.
    pub target_first: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            layer: Layer::Canvas,
            canvas_focused: false,
            target_first: false,
        }
    }
}

/// When the app next needs to wake, and the message it gets then.
#[derive(Debug, Clone, PartialEq)]
pub struct Deadline<M> {
    /// When.
    pub at: Instant,
    /// The message queued when that time has come.
    pub message: M,
}

/// The window's title and limits, as the app describes them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowSpec {
    /// The title.
    pub title: String,
    /// The smallest inside size the window may have, in logical pixels.
    pub min_size: Option<Size>,
}

/// What `update` may read besides its message. Everything here is read
/// only: app state still changes only in `update`.
pub struct Context<'a, M> {
    pub(crate) now: Instant,
    pub(crate) waker: &'a Waker<M>,
    pub(crate) focused: Option<UiId>,
    pub(crate) window: WindowState,
    /// The runtime's panels, as last laid out, for their nodes. None
    /// outside a runtime.
    pub(crate) panels: Option<&'a Window>,
}

impl<'a, M> Context<'a, M> {
    /// A context for a driver or a test that calls `update` itself, outside
    /// a [`Runtime`](crate::Runtime): the bridge to a toolkit that owns the
    /// event loop, or a test that steps one app with no window. A
    /// [`Runtime`](crate::Runtime) makes its own for every update.
    pub fn new(now: Instant, waker: &'a Waker<M>, focused: Option<UiId>, window: WindowState) -> Self {
        Self {
            now,
            waker,
            focused,
            window,
            panels: None,
        }
    }

    /// True when the runtime's panels are laid out as the view now stands,
    /// so [`Context::bounds`], [`Context::text_bounds`], and
    /// [`Context::access_nodes`] are current. A view rebuilt in an earlier
    /// turn is laid out only before the next draw: an app that needs those
    /// answers then waits for it ([`Effects::after_draw`](crate::Effects::after_draw)).
    /// True outside a runtime.
    pub fn laid_out(&self) -> bool {
        self.panels.is_none_or(|panels| panels.is_settled(self.window.size))
    }

    /// True when one of the runtime's panels is where the UI ID `id` would
    /// be: its key, or one inside it, whether or not the element is laid
    /// out yet. Tells a part of the runtime's own from a toolkit's.
    pub fn holds(&self, id: &UiId) -> bool {
        self.panels.is_some_and(|panels| panels.panel_of(id).is_some())
    }

    /// Where the runtime's element with UI ID `id` is in the window, in
    /// logical pixels, as last laid out. `None` outside a runtime, and for
    /// an ID no panel of the runtime's has.
    pub fn bounds(&self, id: &UiId) -> Option<loom_ui_core::Rect> {
        self.panels?.bounds(id)
    }

    /// Where the first of the runtime's text elements that reads `text` is
    /// in the window, as last laid out.
    pub fn text_bounds(&self, text: &str) -> Option<loom_ui_core::Rect> {
        self.panels?.text_bounds(text)
    }

    /// The moment this turn runs, from the driver.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// A waker for a watcher thread to send messages with. The app keeps
    /// the watcher's handle in its state; dropping the handle stops it.
    pub fn waker(&self) -> Waker<M> {
        self.waker.clone()
    }

    /// The element that has keyboard focus, if any.
    pub fn focused(&self) -> Option<&UiId> {
        self.focused.as_ref()
    }

    /// The window's size, scale, and focus.
    pub fn window(&self) -> WindowState {
        self.window
    }

    /// The nodes of the runtime's own elements, as last laid out, for the
    /// app to put in the tree it sends (ui-layer.md, section 7). Node IDs
    /// come from `ids`, the app's one table for every node in the window,
    /// and bounds are in physical pixels at the window's scale. The focus is
    /// the runtime's focused element, when it has a node. It reads the last
    /// layout, and layout runs after the turn, so in the turn that opens a
    /// panel its nodes are not there yet: an app whose panels changed builds
    /// the tree after the draw ([`Effects::after_draw`](crate::Effects::after_draw)).
    ///
    /// A screen reader's action on one of these nodes reaches the app as
    /// any other does, through [`App::accessibility_event`]. The app turns
    /// it into a message by the UI ID its table gives (section 7, rule 6),
    /// and a Focus action into [`Effects::focus`](crate::Effects::focus).
    /// The one exception is a text field's: when [`App::access_target`]
    /// names a field or its run, the runtime carries out SetValue,
    /// ReplaceSelectedText, and SetTextSelection itself, and an edit reaches
    /// the app through [`App::field_event`] (rule 10). Focus on a field
    /// still comes through `accessibility_event`. Empty outside a runtime.
    pub fn access_nodes(&self, ids: &mut IdTable) -> AccessNodes {
        self.panels.map_or_else(AccessNodes::default, |panels| {
            panels.access_nodes(ids, self.window.scale, self.focused.as_ref())
        })
    }
}

#[cfg(test)]
mod tests;
