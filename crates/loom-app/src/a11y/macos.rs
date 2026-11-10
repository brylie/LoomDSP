//! Attaches AccessKit to the app's window on macOS, beside Iced.
//!
//! Temporary: phase 2 of the move off Iced replaces this file with
//! `accesskit_winit` in our own shell, on every platform
//! (`docs/dev/leaving-iced-plan.md`, step 2.2). Until then Iced owns the
//! window, so AccessKit's subclassing adapter attaches to the window's
//! `NSView` after Iced made it.
//!
//! - The adapter is not `Send`, so it lives in a thread local on the main
//!   thread, where Iced runs `update`.
//! - The adapter's callbacks run on the main thread too, outside `update`.
//!   They send [`Event`]s through a channel that a subscription reads, so
//!   every change still goes through `App::update`.

use super::Event;
use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_macos::SubclassingAdapter;
use iced::futures::channel::mpsc;
use iced::{Subscription, Task};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};

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
fn receiver() -> mpsc::UnboundedReceiver<Event> {
    CHANNEL
        .1
        .lock()
        .ok()
        .and_then(|mut receiver| receiver.take())
        // Iced runs this once per subscription; a second run gets an empty stream.
        .unwrap_or_else(|| mpsc::unbounded().1)
}

/// The events the adapter's callbacks send.
pub(super) fn events() -> Subscription<Event> {
    Subscription::run(receiver)
}

/// The adapter and what the callbacks share with it, on the main thread.
struct Attached {
    adapter: SubclassingAdapter,
    /// The last tree sent, so the adapter can ask for it again.
    latest: Rc<RefCell<Option<TreeUpdate>>>,
}

thread_local! {
    static ATTACHED: RefCell<Option<Attached>> = const { RefCell::new(None) };
}

/// Answers the adapter when a screen reader starts listening.
struct Activation {
    latest: Rc<RefCell<Option<TreeUpdate>>>,
}

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        send(Event::Activated);
        // None the first time: the first build then sends the full tree.
        self.latest.borrow().clone()
    }
}

/// Passes screen reader actions on to the app.
struct Actions;

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        send(Event::Action(request));
    }
}

/// The task that attaches the adapter to the app's window at start, and
/// reads the window's scale factor.
pub(super) fn attach() -> Task<Event> {
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

/// Sends a new tree to the adapter. Does nothing before the adapter is
/// attached, and the adapter passes it on only while a screen reader
/// listens.
pub(super) fn push(update: TreeUpdate) {
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
