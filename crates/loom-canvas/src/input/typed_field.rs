//! The field for typing an exact value on a one-number control. The canvas
//! draws it and takes every key while it is open, as it does for the select
//! list, so Backspace never deletes the module behind it. Spec:
//! `docs/design/panel-widgets.md`, Typing a value.

use super::*;

/// Width of the field, in screen pixels.
pub(crate) const FIELD_WIDTH_PX: f32 = 110.0;
/// Height of the field, in screen pixels.
pub(crate) const FIELD_HEIGHT_PX: f32 = 23.0;
/// Text inside the field starts this far in, in screen pixels.
pub(crate) const FIELD_PADDING_PX: f32 = 6.0;
/// Size of the field's text, in screen pixels.
pub(crate) const FIELD_TEXT_PX: f32 = 12.0;

/// The open field, on the focused control.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TypedField {
    /// The text and why it is not a value yet, as the inspector keeps it too.
    pub(crate) draft: TypedDraft,
    /// Where the caret is, as a byte index on a character boundary.
    pub(crate) caret: usize,
    /// True while the whole text is selected, so a character replaces it.
    pub(crate) selected: bool,
    /// The value text it opened with. Enter with this text changes nothing,
    /// because the text shown is rounded (panel-widgets.md, Typing a value).
    unchanged: Option<String>,
}

impl TypedField {
    /// A field opened by typing `text`: the caret after it, nothing
    /// selected. `check` says why a text is not a value.
    pub(super) fn typing(text: &str, check: impl FnOnce(&str) -> Result<(), String>) -> Self {
        Self {
            draft: TypedDraft::new(text.to_string(), check),
            caret: text.len(),
            selected: false,
            unchanged: None,
        }
    }

    /// A field opened by Enter on a control showing `shown`: all of it
    /// selected, so a typed number replaces it, and Enter on it unchanged
    /// changes nothing. The text is checked at once, so a shown value that
    /// does not parse shows its reason as soon as the field opens.
    pub(super) fn opened(shown: String, check: impl FnOnce(&str) -> Result<(), String>) -> Self {
        Self {
            caret: shown.len(),
            selected: true,
            unchanged: Some(shown.clone()),
            draft: TypedDraft::new(shown, check),
        }
    }

    /// Inserts typed text at the caret, or in place of the selected text.
    fn insert(&mut self, typed: &str) {
        if std::mem::take(&mut self.selected) {
            self.draft.text.clear();
            self.caret = 0;
        }
        self.draft.text.insert_str(self.caret, typed);
        self.caret += typed.len();
    }

    /// The caret position one character before the caret.
    fn before(&self) -> usize {
        self.draft.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    /// The caret position one character after the caret.
    fn after(&self) -> usize {
        self.draft.text[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    /// Clears the selected text, if any. Returns true if it did.
    fn clear_selected(&mut self) -> bool {
        let selected = std::mem::take(&mut self.selected);
        if selected {
            self.draft.text.clear();
            self.caret = 0;
        }
        selected
    }
}

/// True for the first character of a typed number: a digit, a minus sign, or
/// a decimal point, which a comma also is (panel-widgets.md, Typing a value).
fn starts_a_number(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | ','))
}

/// Where the line with the reason for bad text goes: below the field, or above
/// it when there is no room below, and inside the canvas. `width` is what the
/// reason needs. A wider reason than the canvas is cut at its edge.
pub(crate) fn typed_reason_bounds(field: Rect, width: f32, bounds: Rect) -> Rect {
    let width = width.min(bounds.width);
    let below = field.y + FIELD_HEIGHT_PX + 2.0;
    let y = if below + FIELD_HEIGHT_PX <= bounds.y + bounds.height {
        below
    } else {
        field.y - FIELD_HEIGHT_PX - 2.0
    };
    let x = field.x.clamp(bounds.x, (bounds.x + bounds.width - width).max(bounds.x));
    Rect::new(Point::new(x, y), Size::new(width, FIELD_HEIGHT_PX))
}

impl CanvasModel<'_> {
    /// The check for text typed on `widget` of `module`: why it is not a
    /// value, if it is not. Only the app knows the parameter, so it is the
    /// app's check (see [`CanvasModel::typed_check`]).
    pub(super) fn draft_check(&self, module: u32, widget: usize) -> impl Fn(&str) -> Result<(), String> + '_ {
        move |text| {
            self.typed_check
                .as_ref()
                .map_or(Ok(()), |check| check(module, widget, text))
        }
    }

    /// Screen rectangle of the open field: at a fixed size, centred on its
    /// widget, and inside the canvas.
    pub(crate) fn typed_field_bounds(&self, state: &CanvasInput, bounds: Rect) -> Option<Rect> {
        state.focus.typed()?;
        let (module, widget) = state.focus.control()?;
        let rect = self.scene.module(module)?.widget_bounds(widget)?;
        let centre = state.camera.to_screen(rect.center());
        let x = (centre.x - FIELD_WIDTH_PX * 0.5).clamp(0.0, (bounds.width - FIELD_WIDTH_PX).max(0.0));
        let y = (centre.y - FIELD_HEIGHT_PX * 0.5).clamp(0.0, (bounds.height - FIELD_HEIGHT_PX).max(0.0));
        Some(Rect::new(Point::new(x, y), Size::new(FIELD_WIDTH_PX, FIELD_HEIGHT_PX)))
    }

    /// Opens the field if this key starts one on the focused control: Enter,
    /// holding the value text, or the first character of a number. Not in MIDI
    /// learn mode, where digits do nothing and Enter arms, not with Command,
    /// Ctrl, or Alt, so those shortcuts keep their meaning, and not during a
    /// drag, whose next move would overwrite the typed value. Returns true if
    /// the key was used.
    pub(super) fn open_typed_field(
        &self,
        state: &mut CanvasInput,
        key: &Key,
        modifiers: Modifiers,
        text: Option<&str>,
        repeat: bool,
    ) -> bool {
        if self.learn.is_some()
            || modifiers.shortcut()
            || modifiers.control
            || modifiers.alt
            || !matches!(state.interaction, Interaction::Idle)
        {
            return false;
        }
        let Some((module, widget)) = state.focus.control() else {
            return false;
        };
        let is_number = self
            .scene
            .module(module)
            .and_then(|m| m.panel.widgets.get(widget))
            .is_some_and(|w| w.number_steps().is_some());
        if !is_number {
            return false;
        }
        if matches!(*key, Key::Enter) {
            // A held Enter must not open the field again after a commit.
            if !repeat {
                let shown = self
                    .values
                    .and_then(|values| values.get(module, widget))
                    .map_or_else(String::new, |value| value.text.clone());
                state
                    .focus
                    .open_typing(TypedField::opened(shown, self.draft_check(module, widget)));
            }
            return true;
        }
        match text.filter(|text| starts_a_number(text)) {
            Some(text) => {
                state
                    .focus
                    .open_typing(TypedField::typing(text, self.draft_check(module, widget)));
                true
            }
            None => false,
        }
    }

    /// A key while the field is open. It takes every key, so none acts behind
    /// it, except Tab, which closes the field and then moves focus as usual.
    /// The panic shortcut still works: it listens whatever has the keys.
    /// Returns true if the field used the key.
    pub(super) fn typed_field_key(
        &self,
        state: &mut CanvasInput,
        key: &Key,
        modifiers: Modifiers,
        text: Option<&str>,
        repeat: bool,
        out: &mut Reaction,
    ) -> bool {
        let Some((module, widget)) = state.focus.control() else {
            return false;
        };
        let Some(mut field) = state.focus.take_typing() else {
            return false;
        };
        match *key {
            Key::Tab => return false,
            Key::Escape => {}
            Key::Enter if repeat => state.focus.open_typing(field),
            Key::Enter => match field.draft.submit(field.unchanged.as_deref()) {
                // Nothing was typed, so nothing changes.
                DraftSubmit::Unchanged => {}
                DraftSubmit::Blocked => state.focus.open_typing(field),
                DraftSubmit::Commit => self.emit(
                    out,
                    CanvasEvent::TypedValue {
                        module,
                        widget,
                        text: field.draft.text,
                    },
                ),
            },
            named_key => {
                match named_key {
                    Key::Backspace => {
                        if !field.clear_selected() && field.caret > 0 {
                            let at = field.before();
                            field.draft.text.remove(at);
                            field.caret = at;
                        }
                    }
                    Key::Delete => {
                        if !field.clear_selected() && field.caret < field.draft.text.len() {
                            field.draft.text.remove(field.caret);
                        }
                    }
                    Key::Left => {
                        field.caret = if std::mem::take(&mut field.selected) {
                            0
                        } else {
                            field.before()
                        };
                    }
                    Key::Right => {
                        field.caret = if std::mem::take(&mut field.selected) {
                            field.draft.text.len()
                        } else {
                            field.after()
                        };
                    }
                    Key::Home => {
                        field.selected = false;
                        field.caret = 0;
                    }
                    Key::End => {
                        field.selected = false;
                        field.caret = field.draft.text.len();
                    }
                    _ if modifiers.shortcut() || modifiers.control || modifiers.alt => {}
                    _ => {
                        if let Some(typed) = text.filter(|typed| !typed.chars().any(char::is_control)) {
                            field.insert(typed);
                        }
                    }
                }
                field.draft.recheck(self.draft_check(module, widget));
                state.focus.open_typing(field);
            }
        }
        out.redraw();
        out.take();
        true
    }

    /// The pointer and window events that close the field. A press outside it,
    /// the wheel, and the window losing focus each close it with no change,
    /// and still do their normal work. A press on it is used up. Returns true
    /// if the field used the event.
    pub(super) fn typed_field_event(
        &self,
        state: &mut CanvasInput,
        event: &Event,
        bounds: Rect,
        screen: Option<Point>,
        out: &mut Reaction,
    ) -> bool {
        if state.focus.typed().is_none() {
            return false;
        }
        match event {
            Event::Pointer(PointerEvent::Pressed(_)) => {
                let inside = self
                    .typed_field_bounds(state, bounds)
                    .zip(screen)
                    .is_some_and(|(rect, at)| rect.contains(at));
                if inside {
                    out.take();
                    return true;
                }
                state.focus.close_typing();
                out.redraw();
                false
            }
            Event::Pointer(PointerEvent::Scrolled(_)) | Event::WindowUnfocused => {
                state.focus.close_typing();
                out.redraw();
                false
            }
            _ => false,
        }
    }
}
