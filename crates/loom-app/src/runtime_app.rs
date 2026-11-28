//! The app as `loom_ui::Runtime` runs it: the `loom_ui::App` trait,
//! implemented by the app itself (ui-layer.md, section 2, the end of the
//! bridge; plan for leaving Iced, step P4).
//!
//! Each method reads the app and gives the runtime a value or a message;
//! only `update` changes it. The canvas is a custom element
//! (`canvas.rs`) and the window's home element, so it hears the keys while
//! nothing else has the focus (section 9, rule 2). Panic is the key that
//! is always heard, and the app's shortcuts take what no element took
//! (section 6, rule 6, steps 1 and 6).

use crate::canvas::{CANVAS, CanvasElement, canvas_id};
use crate::{App, Message};
use loom_ui::{Context, CustomKind, Deadline, Effects, WindowSpec};
use loom_ui_core::{Event, KeyEvent, UiId};

impl loom_ui::App for App {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        #[cfg(any(test, feature = "test-ui"))]
        if let Some(log) = &mut self.message_log {
            log.push(message.clone());
        }
        let extra = self.test_focus(&message);
        // The app keeps a copy of what has the keyboard, for what reads it
        // outside `update`; the runtime's own word is the context's.
        let message = match message {
            Message::FocusMoved(_) => Message::FocusMoved(cx.focused().cloned()),
            message => message,
        };
        App::update(self, message, cx).and(extra)
    }

    fn view(&self, view: &mut loom_ui::View) {
        self.chrome_panels(view);
        self.test_view(view);
    }

    /// What a click on one of the runtime's elements means.
    fn clicked(&self, id: &UiId) -> Option<Message> {
        self.toolbar_click(id)
            .or_else(|| self.breadcrumb_click(id))
            .or_else(|| self.bar_click(id))
            .or_else(|| self.question_click(id))
            .or_else(|| self.help_click(id))
            .or_else(|| self.chooser_click(id))
            .or_else(|| self.preferences_click(id))
            .or_else(|| self.menu_click(id))
            .or_else(|| self.browser_click(id))
            .or_else(|| self.patches_click(id))
            .or_else(|| self.inspector_click(id))
            .or_else(|| self.tabs_click(id))
            .or_else(|| self.outline_click(id))
            .or_else(|| self.matrix_click(id))
            .or_else(|| self.test_clicked(id))
    }

    /// The pointer came onto one of the runtime's elements that reports its
    /// hover: a browser's row, which it highlights.
    fn hovered(&self, id: &UiId) -> Option<Message> {
        self.browser_hovered(id)
            .or_else(|| self.patches_hovered(id))
            .or_else(|| self.inspector_hovered(id, true))
    }

    /// The pointer leaving an element it was told it came onto: an
    /// inspector row that takes a dragged output goes dark (section 5,
    /// rule 11).
    fn unhovered(&self, id: &UiId) -> Option<Message> {
        self.inspector_hovered(id, false)
    }

    /// The runtime's slider moving: the inspector's parameter and depth
    /// sliders (section 5, rule 16).
    fn slid(&self, id: &UiId, position: f32, phase: loom_ui::SlidePhase) -> Option<Message> {
        self.inspector_slid(id, position, phase)
            .or_else(|| self.matrix_slid(id, position, phase))
    }

    /// A right press or a screen reader's ShowContextMenu on an element with
    /// a context menu: a browser's library row (section 5, rule 19).
    fn context_menu(&self, id: &UiId, by_key: bool) -> Option<Message> {
        self.browser_context_menu(id, by_key)
    }

    /// A root placed under an element that stopped showing: a browser row's
    /// menu, whose row scrolled out of view, closes (section 5, rule 13).
    fn anchor_hidden(&self, root: &UiId) -> Option<Message> {
        self.browser_anchor_hidden(root)
    }

    /// A key an element takes before the focused one inside it (section 6,
    /// rule 6, step 2b).
    fn panel_key(&self, id: &UiId, key: &KeyEvent) -> Option<Message> {
        self.browser_panel_key(id, key)
            .or_else(|| self.patches_panel_key(id, key))
            .or_else(|| self.menu_panel_key(id, key))
            .or_else(|| self.inspector_panel_key(id, key))
            .or_else(|| self.tabs_key(id, key))
            .or_else(|| self.outline_panel_key(id, key))
            .or_else(|| self.matrix_panel_key(id, key))
    }

    /// What a choice in one of the runtime's selects means.
    fn chose(&self, id: &UiId, index: usize) -> Option<Message> {
        self.preferences_chose(id, index)
            .or_else(|| self.inspector_chose(id, index))
            .or_else(|| self.matrix_chose(id, index))
    }

    /// Where Tab goes from one of the runtime's stops (`chrome.rs`).
    fn tab_from(&self, from: &UiId, next: Option<&UiId>, back: bool) -> loom_ui::TabStep<Message> {
        App::tab_from(self, from, next, back)
    }

    /// Hover text: the raised fill (`style::RAISED`), a 2 px radius, 6 px
    /// padding, at most 480 wide.
    fn hint_style(&self) -> loom_ui::HintStyle {
        loom_ui::HintStyle {
            text: crate::style::BODY_TEXT,
            colour: crate::style::TEXT,
            background: crate::style::RAISED,
            border: crate::style::RAISED,
            border_width: 0.0,
            radius: 2.0,
            padding: 6.0,
            max_width: 480.0,
            gap: 0.0,
        }
    }

    /// An edit or an Enter on one of the runtime's text fields, as that
    /// field's own message (`runtime_fields.rs`), or a test panel's.
    fn field_event(&self, id: &UiId, event: &loom_ui::FieldEvent) -> Option<Message> {
        self.field_message(id, event)
            .or_else(|| self.test_field_event(id, event))
    }

    /// The UI ID the node `node` names, from the tree's own table.
    fn access_target(&self, node: accesskit::NodeId) -> Option<UiId> {
        self.a11y_ui_id(node)
    }

    /// A SetValue on a field of the inspector's rows is the app's, which
    /// commits it as one undo step.
    fn commits_set_value(&self, field: &UiId) -> bool {
        self.inspector_commits_set_value(field)
    }

    /// What a field reports to a screen reader changed: the tree is built
    /// again, only while a screen reader listens.
    fn access_changed(&self) -> Option<Message> {
        self.a11y
            .is_active()
            .then_some(Message::A11y(crate::a11y::Event::FieldChanged))
    }

    /// A key press under an open question: the question's keys
    /// (`question::key_of`).
    fn question_key(&self, key: &KeyEvent) -> Option<Message> {
        crate::question::key_of(key)
    }

    /// Panic, heard whoever has the input (section 6, rule 6, step 1;
    /// panic.md).
    fn always_key(&self, key: &KeyEvent) -> Option<Message> {
        crate::panic::key(&Event::Key(key.clone()), false).or_else(|| {
            // Help opens from anywhere but under a question, even from a
            // field that takes every key, as the canvas's typed value field
            // does: F1, or Cmd or Ctrl with /, as the help row heard every
            // key before (help-pages.md; ui-layer.md, section 6, rule 13).
            match key {
                KeyEvent::Pressed {
                    key, modifiers, text, ..
                } if self.overlays.question.is_none()
                    && crate::subscriptions::opens_help(*key, *modifiers, text.as_deref()) =>
                {
                    Some(Message::ShowHelp)
                }
                _ => None,
            }
        })
    }

    /// A key nothing took, as the app's listeners heard it (section 6,
    /// rule 6, step 6): the app's shortcuts (`Message::Key`), a shortcut
    /// for the canvas while the focus is elsewhere, then Escape closing
    /// help, then Escape ending a value draft, each its own update, in that
    /// order. Help's opening keys come first, in `always_key`.
    fn unclaimed_key(&self, key: &KeyEvent) -> Vec<Message> {
        if self.overlays.question.is_some() {
            return Vec::new();
        }
        let event = Event::Key(key.clone());
        let mut messages = vec![Message::Key(key.clone())];
        // A shortcut the focused part left reaches the canvas too, as it
        // did on Iced, where the canvas heard every key: paste at the
        // pointer, the zoom reset, select all. With nothing focused the
        // canvas, the window's home, has had it already.
        if let KeyEvent::Pressed { modifiers, .. } = key
            && modifiers.shortcut()
            && self
                .focus
                .as_ref()
                .is_some_and(|focus| !focus.is_under(&crate::canvas::canvas_id()))
        {
            messages.push(Message::CanvasKey(key.clone()));
        }
        messages.extend(crate::subscriptions::help_key(&event, false));
        if self.session.value_draft.is_some() {
            messages.extend(crate::controls::value_draft_event(&event, false));
        }
        messages
    }

    /// A press outside the probe menu or Recent closes it (row 3b).
    fn pressed_outside(&self, root: &UiId) -> Option<Message> {
        self.menu_pressed_outside(root)
            .or_else(|| self.browser_pressed_outside(root))
    }

    /// The runtime's focus moved: the app keeps one copy of what has the
    /// keyboard (`FocusMoved`).
    fn focus_moved(&self, focus: Option<&UiId>) -> Option<Message> {
        Some(Message::FocusMoved(focus.cloned()))
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: self.title(),
            min_size: None,
        }
    }

    /// The close request, a dropped file, a new scale, and the window
    /// losing the keys, which the canvas hears of on its next sync.
    fn window_event(&self, event: &loom_ui::WindowEvent) -> Option<Message> {
        match event {
            loom_ui::WindowEvent::Focused(false) => Some(Message::WindowUnfocused),
            event => crate::subscriptions::window_message(event),
        }
    }

    fn next_deadline(&self) -> Option<Deadline<Message>> {
        App::next_deadline(self)
    }

    /// The input owner as a layer: an open question holds the window, and
    /// every other panel or menu over the canvas is an overlay (section 5,
    /// rule 2). The canvas is the window's home element ([`Self::home`]).
    fn input(&self) -> loom_ui::InputState {
        use crate::input_owner::InputOwner;
        loom_ui::InputState {
            layer: match self.input_owner() {
                InputOwner::Question => loom_ui::Layer::Question,
                InputOwner::Canvas => loom_ui::Layer::Canvas,
                _ => loom_ui::Layer::Overlays,
            },
            canvas_focused: self.canvas_has_keys(),
            // The canvas is the window's home, and Tab from nothing
            // reaches it first (section 6, rule 2).
            home_first: true,
        }
    }

    /// A screen reader's start, request, or departure, as the accessibility
    /// layer's event (`a11y/actions.rs`, section 7, rule 6).
    fn accessibility_event(&self, event: &loom_ui::AccessEvent) -> Option<Message> {
        Some(Message::A11y(match event {
            loom_ui::AccessEvent::Activated => crate::a11y::Event::Activated,
            loom_ui::AccessEvent::Action(request) => crate::a11y::Event::Action(request.clone()),
            loom_ui::AccessEvent::Deactivated => crate::a11y::Event::Deactivated,
        }))
    }

    /// The patch canvas hears the keys while nothing has the focus.
    fn home(&self) -> Option<UiId> {
        Some(canvas_id())
    }

    /// The canvas's state, the first time a layout finds it.
    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn loom_ui::Custom<Self>>> {
        (kind == CANVAS).then(|| Box::new(CanvasElement::new(self)) as Box<dyn loom_ui::Custom<Self>>)
    }
}

#[cfg(any(test, feature = "test-ui"))]
impl App {
    /// The test's panels, if it added any.
    fn test_view(&self, view: &mut loom_ui::View) {
        if let Some(panels) = &self.test_panels {
            (panels.view)(view);
        }
    }

    /// The message for a click on a test panel's element.
    fn test_clicked(&self, id: &UiId) -> Option<Message> {
        self.test_panels.as_ref().and_then(|panels| (panels.clicked)(id))
    }

    /// The test's focus effect for `message`, after the app's own.
    fn test_focus(&self, message: &Message) -> Effects<Message> {
        self.test_panels
            .as_ref()
            .and_then(|panels| (panels.focuses)(message))
            .map_or_else(Effects::none, Effects::focus)
    }

    /// The message for an event on a test panel's text field.
    fn test_field_event(&self, id: &UiId, event: &loom_ui::FieldEvent) -> Option<Message> {
        self.test_panels.as_ref().and_then(|panels| (panels.fields)(id, event))
    }
}

#[cfg(not(any(test, feature = "test-ui")))]
impl App {
    /// No panels outside tests.
    fn test_view(&self, _view: &mut loom_ui::View) {}

    /// Nothing to click outside tests.
    fn test_clicked(&self, _id: &UiId) -> Option<Message> {
        None
    }

    /// No effects of its own outside tests.
    fn test_focus(&self, _message: &Message) -> Effects<Message> {
        Effects::none()
    }

    /// No fields of its own outside tests.
    fn test_field_event(&self, _id: &UiId, _event: &loom_ui::FieldEvent) -> Option<Message> {
        None
    }
}

#[cfg(test)]
mod tests;
