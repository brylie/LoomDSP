//! Spike, not for release: an accessibility tree for screen readers on
//! macOS, built beside Iced (which has no AccessKit support yet, ADR-0028).
//!
//! The question it answers: can the tree come from the same plain-data
//! models the window draws from (`App::ui_snapshot`, ADR-0021), so it cannot
//! drift from the screen, and can screen reader actions go through the same
//! messages as the pointer? Results are in
//! `docs/research/2026-11-a11y-beside-iced-spike.md`.
//!
//! How it fits together:
//!
//! - At start, a window task hands us the window's `NSView`, and AccessKit's
//!   subclassing adapter attaches to it. The adapter is not `Send`, so it
//!   lives in a thread local on the main thread, where Iced runs `update`.
//! - The adapter's callbacks run on the main thread too, outside `update`.
//!   They send [`Event`]s through a channel that a subscription reads, so
//!   every change still goes through `App::update`.
//! - While a screen reader is listening, a timer asks the window where the
//!   canvas is, and the tree is rebuilt from the snapshot.

use crate::snapshot::{Rect, UiSnapshot, Widget};
use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Role, Toggled, TreeId, TreeInfo,
    TreeUpdate,
};
use accesskit_macos::SubclassingAdapter;
use iced::advanced::widget::operation::Outcome;
use iced::advanced::widget::{Id, Operation};
use iced::futures::channel::mpsc;
use iced::{Rectangle, Subscription, Task};
use loom_canvas::{CanvasEvent, CanvasView, ControlPart};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

#[cfg(test)]
mod tests;

/// How often the tree is rebuilt while a screen reader listens.
const REFRESH: Duration = Duration::from_millis(500);

/// How far one Increment or Decrement moves a continuous control.
const STEP: f32 = 0.05;

/// What the accessibility layer tells the app.
#[derive(Debug, Clone)]
pub enum Event {
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

/// The channel from the adapter's callbacks to the subscription.
type Channel = (
    mpsc::UnboundedSender<Event>,
    Mutex<Option<mpsc::UnboundedReceiver<Event>>>,
);

/// One channel for the life of the process: the adapter is attached once.
static CHANNEL: LazyLock<Channel> = LazyLock::new(|| {
    let (sender, receiver) = mpsc::unbounded();
    (sender, Mutex::new(Some(receiver)))
});

/// Sends an event to the app. A closed channel means the app is quitting.
fn send(event: Event) {
    let _ = CHANNEL.0.unbounded_send(event);
}

/// The events from the adapter's callbacks, for `Subscription::run`.
fn events() -> mpsc::UnboundedReceiver<Event> {
    CHANNEL
        .1
        .lock()
        .ok()
        .and_then(|mut receiver| receiver.take())
        // Iced runs this once per subscription; a second run gets an empty stream.
        .unwrap_or_else(|| mpsc::unbounded().1)
}

/// The adapter and what the callbacks share with it, on the main thread.
struct Attached {
    adapter: SubclassingAdapter,
    /// The last tree sent, so the adapter can ask for it again.
    latest: Rc<RefCell<Option<TreeUpdate>>>,
}

thread_local! {
    static ATTACHED: RefCell<Option<Attached>> = const { RefCell::new(None) };
    /// True once a screen reader asked for the tree.
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Answers the adapter when a screen reader starts listening.
struct Activation {
    latest: Rc<RefCell<Option<TreeUpdate>>>,
}

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        ACTIVE.with(|active| active.set(true));
        send(Event::Activated);
        // None the first time: the first rebuild then sends the full tree.
        self.latest.borrow().clone()
    }
}

/// Passes screen reader actions on to the app.
struct Actions;

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        // Spike: log what the screen reader asks for, to see how it adjusts a knob.
        eprintln!(
            "a11y: {:?} on node {:#x}, data {:?}",
            request.action, request.target_node.0, request.data
        );
        send(Event::Action(request));
    }
}

/// The task that attaches the adapter to the app's window at start.
pub fn attach() -> Task<Event> {
    iced::window::latest().then(|id| match id {
        Some(id) => Task::batch([
            iced::window::run(id, attach_to).map(Event::Attached),
            iced::window::scale_factor(id).map(Event::ScaleFactor),
        ]),
        None => Task::done(Event::Attached(Err("the app has no window".into()))),
    })
}

/// Attaches the adapter to the window's content view.
fn attach_to(window: &dyn iced::window::Window) -> Result<(), String> {
    use iced_runtime::window::raw_window_handle::RawWindowHandle;
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    let latest = Rc::new(RefCell::new(None));
    // SAFETY: Iced runs this callback on the main thread with the window
    // alive, and `ns_view` is the window's content view, which lives as long
    // as the window. The adapter retains the view, so the pointer stays valid
    // while the adapter exists.
    let adapter = unsafe {
        SubclassingAdapter::new(
            appkit.ns_view.as_ptr(),
            Activation {
                latest: Rc::clone(&latest),
            },
            Actions,
        )
    };
    ATTACHED.with(|attached| *attached.borrow_mut() = Some(Attached { adapter, latest }));
    Ok(())
}

/// Events from the adapter, and a refresh timer while a screen reader listens.
pub fn subscription() -> Subscription<Event> {
    let timer = if ACTIVE.with(Cell::get) {
        iced::time::every(REFRESH).map(|_| Event::Refresh)
    } else {
        Subscription::none()
    };
    Subscription::batch([Subscription::run(events), timer])
}

/// Asks the window where the canvas is.
pub fn locate() -> Task<Event> {
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

/// Sends a new tree to the adapter, if a screen reader listens.
pub fn push(update: TreeUpdate) {
    ATTACHED.with(|attached| {
        let mut attached = attached.borrow_mut();
        let Some(attached) = attached.as_mut() else {
            return;
        };
        *attached.latest.borrow_mut() = Some(update.clone());
        if let Some(events) = attached.adapter.update_if_active(|| update) {
            events.raise();
        }
    });
}

/// The root node.
const ROOT: NodeId = NodeId(1);
/// The status line.
const STATUS: NodeId = NodeId(2);

/// A tile's node: the module ID in the high bits, so it stays the same
/// while the module exists.
fn tile_node(module: u32) -> NodeId {
    NodeId((u64::from(module) + 1) << 16)
}

/// A panel widget's node, under its tile.
fn widget_node(module: u32, widget: usize) -> NodeId {
    NodeId(tile_node(module).0 | (widget as u64 + 1))
}

/// The module and widget a node stands for, if it is a widget node.
fn widget_of(node: NodeId) -> Option<(u32, usize)> {
    let widget = (node.0 & 0xffff) as usize;
    let module = (node.0 >> 16).checked_sub(1)?;
    (widget > 0).then(|| (module as u32, widget - 1))
}

/// Builds the whole tree from the snapshot. `view` places the canvas in
/// the window; `scale` turns logical pixels into the physical pixels
/// AccessKit expects.
pub fn tree(snapshot: &UiSnapshot, view: Option<&CanvasView>, scale: f32) -> TreeUpdate {
    let mut nodes = Vec::new();
    let place = |node: &mut Node, world: Rect| {
        if let Some(view) = view {
            let a = view.to_window(iced::Point::new(world.x, world.y));
            let b = view.to_window(iced::Point::new(world.x + world.width, world.y + world.height));
            let s = f64::from(scale);
            node.set_bounds(accesskit::Rect {
                x0: f64::from(a.x) * s,
                y0: f64::from(a.y) * s,
                x1: f64::from(b.x) * s,
                y1: f64::from(b.y) * s,
            });
        }
    };

    let mut status = Node::new(Role::Label);
    status.set_value(snapshot.status.clone());
    nodes.push((STATUS, status));

    let mut children = vec![STATUS];
    for tile in &snapshot.tiles {
        let id = tile_node(tile.id);
        let mut node = Node::new(Role::Group);
        node.set_label(tile.title.clone());
        place(&mut node, tile.bounds);
        node.add_action(Action::Focus);
        let mut kids = Vec::new();
        for (index, widget) in tile.widgets.iter().enumerate() {
            if widget.hidden {
                continue;
            }
            let child = widget_node(tile.id, index);
            let mut w = widget_node_for(widget);
            if let Some(bounds) = widget.bounds {
                place(&mut w, bounds);
            }
            kids.push(child);
            nodes.push((child, w));
        }
        node.set_children(kids);
        children.push(id);
        nodes.push((id, node));
    }

    let mut root = Node::new(Role::Window);
    root.set_label(snapshot.title.clone());
    root.set_children(children);
    nodes.push((ROOT, root));

    let mut info = TreeInfo::new(ROOT);
    info.toolkit_name = Some("LoomDSP".into());
    TreeUpdate {
        nodes,
        tree: Some(info),
        tree_id: TreeId::ROOT,
        focus: ROOT,
    }
}

/// The node for one panel widget: its role, name, value, and actions.
fn widget_node_for(widget: &Widget) -> Node {
    let continuous = |role| {
        let mut node = Node::new(role);
        node.set_numeric_value(f64::from(widget.position));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(1.0);
        node.set_numeric_value_step(f64::from(STEP));
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
        node.add_action(Action::SetValue);
        node
    };
    let mut node = match widget.kind {
        "knob" | "fader" | "number" => continuous(Role::Slider),
        "toggle" => {
            let mut node = Node::new(Role::Switch);
            node.set_toggled(if widget.position >= 0.5 {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node
        }
        "button" => {
            let mut node = Node::new(Role::Button);
            node.add_action(Action::Click);
            node
        }
        "meter" => {
            let mut node = Node::new(Role::Meter);
            node.set_numeric_value(f64::from(widget.position));
            node
        }
        // Radio, segmented, select, light, readout, scope: read only in
        // the spike.
        _ => Node::new(Role::Label),
    };
    node.set_label(widget.label.clone());
    // The value as the screen shows it, with its unit ("800 Hz"). Experiment:
    // LOOM_A11Y_VALUE=number leaves it out, so macOS reports the numeric value
    // (AccessKit prefers text when both are set) and VoiceOver can step it.
    if !widget.text.is_empty() && !*NUMBER_VALUE {
        node.set_value(widget.text.clone());
    }
    // Experiment: AccessKit's macOS adapter exposes the name only as the
    // accessibility title, and VoiceOver did not read it on a knob. Try
    // other places for the name, chosen at launch with LOOM_A11Y_NAME.
    match *NAME_MODE {
        NameMode::Title => {}
        NameMode::Help => node.set_description(widget.label.clone()),
        NameMode::Value => node.set_value(if widget.text.is_empty() {
            widget.label.clone()
        } else {
            format!("{}, {}", widget.label, widget.text)
        }),
    }
    node
}

/// Where a control's name goes, for the VoiceOver experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameMode {
    /// Only AccessKit's label, which macOS gets as the title (the default).
    Title,
    /// Also the description, which macOS gets as help, read after a pause.
    Help,
    /// Also at the start of the value: "Tune, 0.000 st".
    Value,
}

/// True when `LOOM_A11Y_VALUE=number`: report controls by number only.
static NUMBER_VALUE: LazyLock<bool> = LazyLock::new(|| std::env::var("LOOM_A11Y_VALUE").as_deref() == Ok("number"));

/// The name mode from `LOOM_A11Y_NAME` (`title`, `help`, or `value`).
static NAME_MODE: LazyLock<NameMode> = LazyLock::new(|| match std::env::var("LOOM_A11Y_NAME").as_deref() {
    Ok("help") => NameMode::Help,
    Ok("value") => NameMode::Value,
    _ => NameMode::Title,
});

/// What the app should do for a screen reader's request: canvas events,
/// in order. Empty when the request is not one the spike handles.
pub fn action(snapshot: &UiSnapshot, request: &ActionRequest) -> Vec<CanvasEvent> {
    let target = request.target_node;
    if request.action == Action::Focus {
        // Only a tile that is still on screen: the tree may be a step behind.
        let module = (target.0 >> 16).checked_sub(1).map(|m| m as u32);
        return module
            .filter(|m| snapshot.tiles.iter().any(|t| t.id == *m))
            .map(|m| vec![CanvasEvent::Select(vec![m])])
            .unwrap_or_default();
    }
    let Some((module, index)) = widget_of(target) else {
        return Vec::new();
    };
    let Some(widget) = snapshot
        .tiles
        .iter()
        .find(|t| t.id == module)
        .and_then(|t| t.widgets.get(index))
        .filter(|w| !w.hidden)
    else {
        return Vec::new();
    };
    let set = |position: f32| {
        vec![CanvasEvent::SetControl {
            module,
            widget: index,
            position: position.clamp(0.0, 1.0),
            part: ControlPart::Main,
        }]
    };
    match (request.action, widget.kind) {
        (Action::Increment, _) => set(widget.position + STEP),
        (Action::Decrement, _) => set(widget.position - STEP),
        (Action::SetValue, _) => match request.data {
            Some(ActionData::NumericValue(v)) => set(v as f32),
            _ => Vec::new(),
        },
        (Action::Click, "toggle") => set(if widget.position >= 0.5 { 0.0 } else { 1.0 }),
        (Action::Click, "button") => vec![CanvasEvent::ButtonPressed { module, widget: index }],
        _ => Vec::new(),
    }
}

thread_local! {
    /// The window's scale factor: AccessKit wants physical pixels.
    static SCALE: Cell<f32> = const { Cell::new(1.0) };
}

impl crate::App {
    /// Handles the accessibility layer's events. Actions go through the same
    /// canvas events as the pointer, so undo, MIDI feedback, and autosave see
    /// them as ordinary edits.
    pub(crate) fn a11y_event(&mut self, event: Event) -> Task<crate::Message> {
        use crate::Message;
        match event {
            Event::Attached(Ok(())) => Task::none(),
            Event::Attached(Err(e)) => {
                eprintln!("loom-app: accessibility spike did not attach: {e}");
                Task::none()
            }
            Event::ScaleFactor(scale) => {
                SCALE.with(|s| s.set(scale));
                Task::none()
            }
            Event::Activated | Event::Refresh => locate().map(Message::A11y),
            Event::Located(view) => {
                push(tree(&self.ui_snapshot(), view.as_ref(), SCALE.with(Cell::get)));
                Task::none()
            }
            Event::Action(request) => {
                // An open panel owns input, as it does for the pointer: the
                // canvas behind it does not change.
                if !self.no_overlay() {
                    eprintln!("a11y: -> ignored, a panel owns input");
                    return Task::none();
                }
                let events = action(&self.ui_snapshot(), &request);
                eprintln!("a11y: -> {events:?}");
                // A set is one turn of a knob: one change, then let go, so it
                // is one undo step (as the control channel's `set`).
                let turned = events.iter().any(|e| matches!(e, CanvasEvent::SetControl { .. }));
                let mut tasks: Vec<_> = events.into_iter().map(|e| self.update(Message::Canvas(e))).collect();
                if turned {
                    tasks.push(self.update(Message::SliderReleased));
                }
                // Show the new value at once, not at the next refresh.
                tasks.push(locate().map(Message::A11y));
                Task::batch(tasks)
            }
        }
    }
}
