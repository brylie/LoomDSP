//! A custom element's keys and focus (ui-layer.md, section 9, rules 2
//! and 6; section 6, rule 6, steps 3 and 5).
//!
//! While the runtime's focus is on a custom element or a part inside it,
//! the element is the focused element of step 3: it hears each key after
//! the keys an element around it takes first (step 2b) and before Tab, so
//! Tab moves inside it until it lets Tab go. While nothing has focus, the
//! element the app names as the window's home hears the key at step 5. A
//! key the element used goes no further.

use super::{CustomEvent, Used};
use crate::app::App;
use crate::draw::Panels;
use loom_ui_core::{Event, KeyEvent, UiId};

impl<A: App> Panels<'_, A> {
    /// The custom element the runtime's focus is on, if it is one.
    pub(crate) fn focused_custom(&self) -> Option<UiId> {
        self.focus.id().filter(|id| self.customs.is_custom(id)).cloned()
    }

    /// The UI ID of what has the runtime's focus, as the app hears it: the
    /// stop itself, or, on a custom element, the part inside it that the
    /// element says has the focus.
    pub(crate) fn focused_part(&self) -> Option<UiId> {
        let id = self.focus.id()?;
        if self.customs.is_custom(id) {
            return Some(self.customs.focused_part(self.app, id));
        }
        Some(id.clone())
    }

    /// Gives a key press or release to the custom element `id`, when it is
    /// in a layer that takes input. Returns true when it used the key.
    fn key_to(&mut self, id: &UiId, event: &Event) -> bool {
        let Event::Key(key @ (KeyEvent::Pressed { .. } | KeyEvent::Released { .. })) = event else {
            return false;
        };
        if !crate::runtime::in_input_layer(self.window, self.app, id) {
            return false;
        }
        let Some(moment) = self.custom_moment(id, self.pointer.at) else {
            return false;
        };
        let event = CustomEvent::Key(key.clone());
        self.customs.tell(self.app, id, &event, &moment) != Used::No
    }

    /// Step 3: gives the key to the focused custom element. Returns true
    /// when it used the key.
    pub(crate) fn key_to_focused(&mut self, event: &Event) -> bool {
        self.focused_custom().is_some_and(|id| self.key_to(&id, event))
    }

    /// Step 5: gives the key to the window's home element ([`App::home`])
    /// while nothing has focus in either toolkit: `target_focus` says
    /// whether the target's own element has it. Returns true when it used
    /// the key.
    pub(crate) fn key_to_home(&mut self, event: &Event, target_focus: bool) -> bool {
        if self.focus.id().is_some() || target_focus {
            return false;
        }
        let Some(home) = self.app.home().filter(|id| self.customs.is_custom(id)) else {
            return false;
        };
        self.key_to(&home, event)
    }

    /// Tells the custom element the focus is on now that Tab (`back`
    /// false) or Shift+Tab (`back` true) moved it there.
    pub(crate) fn tell_entered(&mut self, back: bool) {
        let Some(id) = self.focused_custom() else {
            return;
        };
        if let Some(moment) = self.custom_moment(&id, self.pointer.at) {
            let _ = self
                .customs
                .tell(self.app, &id, &CustomEvent::Entered { back }, &moment);
        }
    }

    /// Carries out a focus effect on `id`, when it names a part inside a
    /// custom element: the element focuses that part, and the runtime's
    /// focus goes to the element. `None` when `id` names no such part, so
    /// the effect goes on as any other. `Some(false)` when the element has
    /// no such part, has no state, or is no focus stop yet: the effect is
    /// dropped, the focus stays, and nothing waits for a later layout or
    /// goes to the target (section 9, rule 6).
    pub(crate) fn focus_inner(&mut self, id: &UiId) -> Option<bool> {
        let owner = self.customs.owner_of(id)?.clone();
        if !self.window.is_stop(&owner) {
            return Some(false);
        }
        let rest = &id.keys()[owner.keys().len()..];
        Some(self.customs.focus_inner(self.app, &owner, rest))
    }
}

#[cfg(test)]
mod tests;
