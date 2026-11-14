//! Attaches AccessKit to the app's window on macOS.
//!
//! Temporary: plan step 2.2d replaces this file with `accesskit_winit` in
//! our own shell, on every platform (`docs/dev/leaving-iced-plan.md`).
//! Until then AccessKit's subclassing adapter attaches to the window's
//! `NSView`, which the window's layers hand it when the window opens,
//! before it is shown (ui-layer.md, section 7, rule 11).
//!
//! - The adapter is not `Send`, so it lives in a thread local on the main
//!   thread, where the shell runs `update`.
//! - The adapter's callbacks run on the main thread too, outside `update`.
//!   They send [`Event`]s through the runtime's waker, so every change
//!   still goes through `App::update`.

use super::Event;
use crate::Message;
use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_macos::SubclassingAdapter;
use loom_ui::Waker;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

/// Where the adapter's callbacks send their events: the runtime's waker,
/// once the window has opened. The adapter is attached once.
static WAKER: Mutex<Option<Waker<Message>>> = Mutex::new(None);

/// Sends an event to the app. Before the window opens, nothing listens.
fn send(event: Event) {
    if let Ok(waker) = WAKER.lock()
        && let Some(waker) = waker.as_ref()
    {
        waker.send(Message::A11y(event));
    }
}

/// The adapter's events, in tests that build the app's subscriptions:
/// they go through the waker now, so the stream is empty.
#[cfg(test)]
pub(super) fn events() -> iced::Subscription<Event> {
    iced::Subscription::none()
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

/// Attaches the adapter to `window`, before it is shown, and sends its
/// events through `waker` from now on. The app hears whether it attached,
/// and the window's scale factor.
pub(crate) fn attach(window: &winit::window::Window, waker: Waker<Message>) {
    if let Ok(mut slot) = WAKER.lock() {
        *slot = Some(waker);
    }
    send(Event::Attached(attach_to(window)));
    send(Event::ScaleFactor(window.scale_factor() as f32));
}

/// Attaches the adapter to the window's content view.
fn attach_to(window: &winit::window::Window) -> Result<(), String> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    let latest = Rc::new(RefCell::new(None));
    // SAFETY: the shell calls this on the main thread with the window
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

/// Sends the nodes that changed to the adapter, and keeps the whole tree
/// for a screen reader that starts to listen later. Does nothing before
/// the adapter is attached, and the adapter passes changes on only while a
/// screen reader listens.
pub(super) fn push(whole: TreeUpdate, changes: Option<TreeUpdate>) {
    ATTACHED.with(|attached| {
        let mut attached = attached.borrow_mut();
        let Some(attached) = attached.as_mut() else {
            return;
        };
        *attached.latest.borrow_mut() = Some(whole);
        let Some(changes) = changes else {
            return;
        };
        if let Some(events) = attached.adapter.update_if_active(|| changes) {
            events.raise();
        }
    });
}
