//! The app, as the runtime sees it.

use crate::custom::CustomLookup;
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
///
/// It is `'static`: it holds no borrowed data. The runtime keeps each
/// custom element's state as a trait object over the app and reads it back
/// by its type ([`Context::custom`]), and only a `'static` type can be read
/// back so. The bound is on the type, not on how long the app lives: a
/// plugin's app lives as long as its plugin instance, and holds what the
/// host gives it as owned handles (ui-layer.md, section 9, the model path).
pub trait App: 'static {
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

    /// The message for a key press that nothing of the runtime's took while
    /// the Question layer takes input (section 6, rule 6, step 2): the
    /// question's keys, or a key that is always heard, such as Panic. The
    /// runtime keeps every such press from the target, whatever this
    /// answers, so no key reaches a field, the canvas, or a shortcut under
    /// an open question, whichever toolkit drew it. Its release goes on as
    /// usual. It reads state and changes none.
    fn question_key(&self, key: &loom_ui_core::KeyEvent) -> Option<Self::Message> {
        let _ = key;
        None
    }

    /// The message for a press outside the panel `root`, whose root asks
    /// for it ([`Element::closes_on_press_outside`](crate::Element::closes_on_press_outside)),
    /// while its layer takes input: usually, close it (section 5, rule
    /// 13). The runtime takes the press, so nothing under the panel sees it.
    /// Any mouse button counts. The message runs on the next turn, as a
    /// click's does. It reads state and changes none.
    fn pressed_outside(&self, root: &UiId) -> Option<Self::Message> {
        let _ = root;
        None
    }

    /// The message for the pointer moving onto the element with UI ID `id`,
    /// one that asked for it with
    /// [`Element::reports_hover`](crate::Element::reports_hover)
    /// (ui-layer.md, section 3, rule 4). Heard once each time the pointer
    /// comes onto it, not for each move over it. It arrives on the next
    /// turn. It reads state and changes none.
    fn hovered(&self, id: &UiId) -> Option<Self::Message> {
        let _ = id;
        None
    }

    /// The message for the pointer leaving the element with UI ID `id`,
    /// one the app heard [`App::hovered`] for: heard once for each time it
    /// heard that, when the pointer moves onto another element or off
    /// every one, leaves the window, or is no longer taken by the layer the
    /// element is in, and when the element leaves the layout (ui-layer.md,
    /// section 5, rule 11). It arrives on the next turn. It reads state and
    /// changes none.
    fn unhovered(&self, id: &UiId) -> Option<Self::Message> {
        let _ = id;
        None
    }

    /// The message for opening the context menu of the element with UI ID
    /// `id`, one that has one
    /// ([`Element::context_menu`](crate::Element::context_menu)): a right
    /// press on it (`by_key` false), or a screen reader's ShowContextMenu on
    /// its node (`by_key` true, so the menu opens on its first item, as one
    /// opened from the keyboard does; section 5, rule 19). It arrives on the
    /// next turn. It reads state and changes none.
    fn context_menu(&self, id: &UiId, by_key: bool) -> Option<Self::Message> {
        let _ = (id, by_key);
        None
    }

    /// The message for the root panel `root`, placed under another element
    /// ([`Placement::Under`](crate::Placement::Under)), whose element stopped
    /// showing: scrolled out of view, cut away, or gone (ui-layer.md,
    /// section 5, rule 13). Told once, on the next turn, after a settled
    /// layout, and again only after the element shows and hides once more.
    /// A menu under a list's row closes on it, as a select's list closes
    /// when its select scrolls away. It reads state and changes none.
    fn anchor_hidden(&self, root: &UiId) -> Option<Self::Message> {
        let _ = root;
        None
    }

    /// The message for the slider with UI ID `id` moving to `position`,
    /// from 0 to 1 along its travel, in `phase` of its gesture (ui-layer.md,
    /// section 5, rule 16): a drag, a double-click's reset, a group of keys,
    /// or a screen reader's Increment or Decrement. Each phase carries the
    /// position to show now, so the app sets it at every one, and makes one
    /// undo step from Start to End. It arrives on the next turn. It reads
    /// state and changes none.
    fn slid(&self, id: &UiId, position: f32, phase: crate::SlidePhase) -> Option<Self::Message> {
        let _ = (id, position, phase);
        None
    }

    /// The message for the option at `index` chosen in the select with UI
    /// ID `id` (ui-layer.md, section 5, rule 18): from its open list, by a
    /// key, a press, or a screen reader, or a screen reader's Increment or
    /// Decrement, which steps one option. Only a choice that differs from
    /// the chosen option is reported, except while the select has a step's
    /// start ([`Select::steps_from`](crate::Select::steps_from)): then a
    /// choice of the chosen option is reported too, as going back to it. It
    /// arrives on the next turn. It reads state and changes none.
    fn chose(&self, id: &UiId, index: usize) -> Option<Self::Message> {
        let _ = (id, index);
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

    /// Where Tab (or Shift+Tab, when `back`) goes from the runtime's focus
    /// stop `from` (ui-layer.md, section 6, rule 3). `next` is the stop
    /// after it in the runtime's reading order, among the layers that take
    /// input now, or `None` past either end. The app answers where its
    /// stops are not one run: when the target's own stops sit between two
    /// of the runtime's, or the order wraps. It reads state and changes
    /// none. The default goes to `next`, and past either end leaves the
    /// runtime's stops for the target, as with one run.
    fn tab_from(&self, from: &UiId, next: Option<&UiId>, back: bool) -> TabStep<Self::Message> {
        let _ = (from, back);
        next.map_or(TabStep::Leave, |next| TabStep::To(next.clone()))
    }

    /// The message for `key`, pressed while the focus is on or inside the
    /// element `id` that takes it first ([`Element::takes_first`](crate::Element::takes_first)),
    /// before the focused element hears it (ui-layer.md, section 6,
    /// rule 6, step 2b). `Some` takes the key: the message runs this turn,
    /// and neither the focused element nor the target hears the press.
    /// `None` passes it on, as if no element took it first. It reads state
    /// and changes none.
    fn panel_key(&self, id: &UiId, key: &loom_ui_core::KeyEvent) -> Option<Self::Message> {
        let _ = (id, key);
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

    /// Whether a screen reader's SetValue on the field `field` is the
    /// app's to carry out, through [`App::accessibility_event`], rather
    /// than the runtime's: a field whose value the app commits whole, as
    /// one step, where the runtime would only make a draft of it. The
    /// field's ReplaceSelectedText and SetTextSelection stay the
    /// runtime's (section 7, rule 10). False by default.
    fn commits_set_value(&self, field: &UiId) -> bool {
        let _ = field;
        false
    }

    /// The message for "what a field reports to a screen reader changed":
    /// its text, its caret, its selection, or its scroll (section 7, rule
    /// 7), or a scroll container's offset (rule 28). An edit gives both: [`FieldEvent::Edited`](crate::FieldEvent::Edited)
    /// through [`App::field_event`], then this notice in the same turn. The runtime asks at most once per turn,
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

    /// The window's home element: the custom element that hears the keys
    /// while nothing has focus, at section 6, rule 6, step 5, after the
    /// open overlay's keys (section 9, rule 2). The patch canvas is
    /// LoomDSP's. `None`, the default, when the window has none.
    fn home(&self) -> Option<UiId> {
        None
    }

    /// Makes the state of the custom element with UI ID `id`, of `kind`,
    /// the first time a layout finds it (ui-layer.md, section 9, rule 1).
    /// The runtime keeps it by `id` until a layout no longer finds that UI
    /// ID. `None` for a kind the app does not know, which panics in a debug
    /// build and, in a release build, leaves an element that hears, draws,
    /// and sends nothing.
    ///
    /// It runs at layout, which a driver calls outside a turn, so it must
    /// not expect one: with only `&self` it changes no app state, and it
    /// can send no message.
    fn make_custom(&self, id: &UiId, kind: crate::CustomKind) -> Option<Box<dyn crate::Custom<Self>>> {
        let _ = (id, kind);
        None
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

/// Where a Tab from one of the runtime's stops goes ([`App::tab_from`]).
#[derive(Debug, Clone, PartialEq)]
pub enum TabStep<M> {
    /// To this focus stop of the runtime's.
    To(UiId),
    /// Out of the runtime's stops: its focus clears and the key goes on to
    /// the target, which takes it as from nothing.
    Leave,
    /// Out of the runtime's stops, with this message for the next turn,
    /// which moves the focus into the target (a focus request to the
    /// canvas, or a focus effect on one of the target's fields). The key
    /// goes no further, so the target does not step a second time.
    Send(M),
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
    /// The custom elements' state, to read back by type.
    pub(crate) customs: Option<&'a dyn CustomLookup>,
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
            customs: None,
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

    /// True when the element with focus is one of the runtime's text
    /// fields. A key the field left, such as an app shortcut, must not act
    /// on what is behind the field while it does (section 6, rule 6, step
    /// 6). False outside a runtime, where a toolkit's focus is asked of its
    /// window instead.
    pub fn field_has_focus(&self) -> bool {
        self.panels
            .zip(self.focused.as_ref())
            .is_some_and(|(panels, id)| panels.fields.has(id))
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

    /// The state of the custom element with UI ID `id`, as its own type
    /// `S`, as the last event, tick, or layout left it: in the same turn as
    /// the event that changed it (ui-layer.md, section 9, rule 1). `None`
    /// when no element has that UI ID, or its state is not an `S`.
    pub fn custom<S: 'static>(&self, id: &UiId) -> Option<&S> {
        self.customs?.state(id)?.downcast_ref()
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
