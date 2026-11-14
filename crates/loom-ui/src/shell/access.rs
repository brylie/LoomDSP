//! The screen reader's side of the window: `accesskit_winit`'s adapter and
//! the tree it is handed (ui-layer.md, section 7, rules 7 and 11).
//!
//! The app builds the tree and hands it over as an effect
//! (`Effect::Accessibility`), with the whole tree and what changed. The
//! adapter asks for a tree when a screen reader starts, on a thread of the
//! platform's choosing (the main thread on macOS, others on Windows and
//! Linux), and wants it at once. So the shell keeps the last whole tree in a
//! [`SharedTree`], and the activation handler answers from it.
//!
//! When there is no tree yet, the handler answers nothing, and the platform
//! adapter shows a placeholder until its first update, which must then be a
//! whole tree (`accesskit_macos` 0.27.2, `Adapter::update_if_active`). The
//! shared tree remembers that a whole tree is owed, so the next update
//! hands over the whole tree rather than only its changes.

use crate::event::AccessEvent;
use accesskit::{ActivationHandler, TreeUpdate};
use std::sync::{Arc, Mutex};
use winit::event_loop::EventLoopProxy;

/// What the shell and the activation handler share: the last whole tree,
/// and whether the adapter is owed a whole one. Locked on the main thread
/// and on the platform adapter's own threads, never on the audio thread,
/// and only for a moment.
#[derive(Debug, Default)]
struct Shared {
    /// The last whole tree the app gave.
    latest: Option<TreeUpdate>,
    /// A screen reader started when there was no tree to hand it, so the
    /// next update must be a whole tree.
    owed: bool,
}

/// The last whole tree, shared with the activation handler.
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedTree(Arc<Mutex<Shared>>);

impl SharedTree {
    /// A screen reader started: the tree to hand it at once, or `None`
    /// when the app has given none yet, and then a whole tree is owed.
    pub(crate) fn activate(&self) -> Option<TreeUpdate> {
        let mut shared = self.lock();
        let tree = shared.latest.clone();
        shared.owed = tree.is_none();
        tree
    }

    /// The app gave `whole` and its `changes`: keeps `whole`, and gives
    /// what the adapter should apply. That is the whole tree when one is
    /// owed, else the changes, if any.
    pub(crate) fn hand_over(&self, whole: TreeUpdate, changes: Option<TreeUpdate>) -> Option<TreeUpdate> {
        let mut shared = self.lock();
        let owed = std::mem::take(&mut shared.owed);
        shared.latest = Some(whole.clone());
        if owed { Some(whole) } else { changes }
    }

    /// The shared state. A panic while it was held leaves it whole, so a
    /// poisoned lock is used as it is.
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Answers a screen reader that starts: the last whole tree, at once, and
/// `Activated` to the app through the event loop, so it starts sending.
pub(crate) struct Activation<E: 'static> {
    /// The last whole tree.
    pub(crate) tree: SharedTree,
    /// Tells the app.
    pub(crate) proxy: EventLoopProxy<E>,
}

impl<E: From<AccessEvent> + 'static> ActivationHandler for Activation<E> {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let tree = self.tree.activate();
        // Fails only once the loop has ended, when nothing listens.
        let _ = self.proxy.send_event(AccessEvent::Activated.into());
        tree
    }
}

/// A screen reader's request or departure, as the runtime's event.
pub(crate) fn event(event: accesskit_winit::WindowEvent) -> AccessEvent {
    match event {
        accesskit_winit::WindowEvent::InitialTreeRequested => AccessEvent::Activated,
        accesskit_winit::WindowEvent::ActionRequested(request) => AccessEvent::Action(request),
        accesskit_winit::WindowEvent::AccessibilityDeactivated => AccessEvent::Deactivated,
    }
}

#[cfg(test)]
mod tests;
