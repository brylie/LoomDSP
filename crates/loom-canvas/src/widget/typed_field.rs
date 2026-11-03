//! The field for typing an exact value on a one-number control. The canvas
//! draws it and takes every key while it is open, as it does for the select
//! list, so Backspace never deletes the module behind it. Spec:
//! `docs/design/panel-widgets.md`, Typing a value.

use super::*;

/// Width of the field, in screen pixels.
const FIELD_WIDTH_PX: f32 = 110.0;
/// Height of the field, in screen pixels.
const FIELD_HEIGHT_PX: f32 = 23.0;
/// Text inside the field starts this far in, in screen pixels.
const FIELD_PADDING_PX: f32 = 6.0;
/// Size of the field's text, in screen pixels.
const FIELD_TEXT_PX: f32 = 12.0;

/// The open field.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TypedField {
    pub(super) module: u32,
    pub(super) widget: usize,
    pub(super) text: String,
    /// Where the caret is, as a byte index on a character boundary.
    caret: usize,
    /// True while the whole text is selected, so a character replaces it.
    selected: bool,
    /// The value text it opened with. Enter with this text changes nothing,
    /// because the text shown is rounded (panel-widgets.md, Typing a value).
    unchanged: Option<String>,
}

impl TypedField {
    /// Inserts typed text at the caret, or in place of the selected text.
    fn insert(&mut self, typed: &str) {
        if std::mem::take(&mut self.selected) {
            self.text.clear();
            self.caret = 0;
        }
        self.text.insert_str(self.caret, typed);
        self.caret += typed.len();
    }

    /// The caret position one character before the caret.
    fn before(&self) -> usize {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    /// The caret position one character after the caret.
    fn after(&self) -> usize {
        self.text[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    /// Clears the selected text, if any. Returns true if it did.
    fn clear_selected(&mut self) -> bool {
        let selected = std::mem::take(&mut self.selected);
        if selected {
            self.text.clear();
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
pub(super) fn typed_reason_bounds(field: Rectangle, width: f32, bounds: Rectangle) -> Rectangle {
    let width = width.min(bounds.width);
    let below = field.y + FIELD_HEIGHT_PX + 2.0;
    let y = if below + FIELD_HEIGHT_PX <= bounds.y + bounds.height {
        below
    } else {
        field.y - FIELD_HEIGHT_PX - 2.0
    };
    let x = field.x.clamp(bounds.x, (bounds.x + bounds.width - width).max(bounds.x));
    Rectangle::new(Point::new(x, y), Size::new(width, FIELD_HEIGHT_PX))
}

impl<Message> PatchCanvas<'_, Message> {
    /// Why the text cannot be committed, or `None` if it can.
    pub(super) fn typed_error(&self, field: &TypedField) -> Option<String> {
        self.typed_check
            .as_ref()
            .and_then(|check| check(field.module, field.widget, &field.text).err())
    }

    /// Screen rectangle of the field: at a fixed size, centred on its widget,
    /// and inside the canvas.
    pub(super) fn typed_field_bounds(&self, state: &State, bounds: Rectangle, field: &TypedField) -> Option<Rectangle> {
        let rect = self.scene.module(field.module)?.widget_bounds(field.widget)?;
        let centre = state.camera.to_screen(rect.center());
        let x = (centre.x - FIELD_WIDTH_PX * 0.5).clamp(0.0, (bounds.width - FIELD_WIDTH_PX).max(0.0));
        let y = (centre.y - FIELD_HEIGHT_PX * 0.5).clamp(0.0, (bounds.height - FIELD_HEIGHT_PX).max(0.0));
        Some(Rectangle::new(
            Point::new(x, y),
            Size::new(FIELD_WIDTH_PX, FIELD_HEIGHT_PX),
        ))
    }

    /// Closes a field whose control has gone, been hidden, or lost its
    /// selection, or when MIDI learn starts. No value changes.
    pub(super) fn close_stale_typed(&self, state: &mut State) {
        let live = state.typed.as_ref().is_some_and(|field| {
            self.learn.is_none()
                && self.selection.len() == 1
                && self.selection.contains(&field.module)
                && self
                    .scene
                    .module(field.module)
                    .and_then(|module| module.panel.widgets.get(field.widget))
                    .is_some_and(|widget| widget.number_steps().is_some() && !widget.hidden)
        });
        if !live {
            state.typed = None;
        }
    }

    /// Opens the field if this key starts one on the focused control: Enter,
    /// holding the value text, or the first character of a number. Not in MIDI
    /// learn mode, where digits do nothing and Enter arms, not with Command,
    /// Ctrl, or Alt, so those shortcuts keep their meaning, and not during a
    /// drag, whose next move would overwrite the typed value. Returns true if
    /// the key was used.
    pub(super) fn open_typed_field(
        &self,
        state: &mut State,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<&str>,
        repeat: bool,
    ) -> bool {
        use keyboard::key::Named;
        if self.learn.is_some()
            || modifiers.command()
            || modifiers.control()
            || modifiers.alt()
            || !matches!(state.interaction, Interaction::Idle)
        {
            return false;
        }
        let Some((module, widget)) = state.focus else {
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
        if matches!(key.as_ref(), keyboard::Key::Named(Named::Enter)) {
            // A held Enter must not open the field again after a commit.
            if !repeat {
                let shown = self
                    .values
                    .and_then(|values| values.get(module, widget))
                    .map_or_else(String::new, |value| value.text.clone());
                state.typed = Some(TypedField {
                    module,
                    widget,
                    caret: shown.len(),
                    selected: true,
                    unchanged: Some(shown.clone()),
                    text: shown,
                });
            }
            return true;
        }
        match text.filter(|text| starts_a_number(text)) {
            Some(text) => {
                state.typed = Some(TypedField {
                    module,
                    widget,
                    text: text.to_string(),
                    caret: text.len(),
                    selected: false,
                    unchanged: None,
                });
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
        state: &mut State,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<&str>,
        repeat: bool,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        use keyboard::key::Named;
        let Some(mut field) = state.typed.take() else {
            return false;
        };
        match key.as_ref() {
            keyboard::Key::Named(Named::Tab) => return false,
            keyboard::Key::Named(Named::Escape) => {}
            keyboard::Key::Named(Named::Enter) if repeat => state.typed = Some(field),
            keyboard::Key::Named(Named::Enter) => {
                if field.unchanged.as_ref() == Some(&field.text) {
                    // Nothing was typed, so nothing changes.
                } else if self.typed_error(&field).is_some() {
                    state.typed = Some(field);
                } else {
                    self.emit(
                        shell,
                        CanvasEvent::TypedValue {
                            module: field.module,
                            widget: field.widget,
                            text: field.text,
                        },
                    );
                }
            }
            named_key => {
                match named_key {
                    keyboard::Key::Named(Named::Backspace) => {
                        if !field.clear_selected() && field.caret > 0 {
                            let at = field.before();
                            field.text.remove(at);
                            field.caret = at;
                        }
                    }
                    keyboard::Key::Named(Named::Delete) => {
                        if !field.clear_selected() && field.caret < field.text.len() {
                            field.text.remove(field.caret);
                        }
                    }
                    keyboard::Key::Named(Named::ArrowLeft) => {
                        field.caret = if std::mem::take(&mut field.selected) {
                            0
                        } else {
                            field.before()
                        };
                    }
                    keyboard::Key::Named(Named::ArrowRight) => {
                        field.caret = if std::mem::take(&mut field.selected) {
                            field.text.len()
                        } else {
                            field.after()
                        };
                    }
                    keyboard::Key::Named(Named::Home) => {
                        field.selected = false;
                        field.caret = 0;
                    }
                    keyboard::Key::Named(Named::End) => {
                        field.selected = false;
                        field.caret = field.text.len();
                    }
                    _ if modifiers.command() || modifiers.control() || modifiers.alt() => {}
                    _ => {
                        if let Some(typed) = text.filter(|typed| !typed.chars().any(char::is_control)) {
                            field.insert(typed);
                        }
                    }
                }
                state.typed = Some(field);
            }
        }
        shell.request_redraw();
        shell.capture_event();
        true
    }

    /// The pointer and window events that close the field. A press outside it,
    /// the wheel, and the window losing focus each close it with no change,
    /// and still do their normal work. A press on it is used up. Returns true
    /// if the field used the event.
    pub(super) fn typed_field_event(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        screen: Option<Point>,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let Some(field) = state.typed.as_ref() else {
            return false;
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                let inside = self
                    .typed_field_bounds(state, bounds, field)
                    .zip(screen)
                    .is_some_and(|(rect, at)| rect.contains(at));
                if inside {
                    shell.capture_event();
                    return true;
                }
                state.typed = None;
                shell.request_redraw();
                false
            }
            Event::Mouse(mouse::Event::WheelScrolled { .. }) | Event::Window(window::Event::Unfocused) => {
                state.typed = None;
                shell.request_redraw();
                false
            }
            _ => false,
        }
    }

    /// Draws the open field above all tile text. Bad text gets a warning
    /// outline, and its reason on a line below (rule 8: not colour alone).
    pub(super) fn draw_typed_field<Renderer>(&self, state: &State, renderer: &mut Renderer, bounds: Rectangle)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        let Some(field) = state.typed.as_ref() else {
            return;
        };
        let Some(rect) = self.typed_field_bounds(state, Rectangle::new(Point::ORIGIN, bounds.size()), field) else {
            return;
        };
        let area = Rectangle::new(Point::new(bounds.x + rect.x, bounds.y + rect.y), rect.size());
        let font = renderer.default_font();
        let error = self.typed_error(field);
        let line = |content: String, width: f32| Text {
            content,
            bounds: Size::new(width, FIELD_HEIGHT_PX),
            size: Pixels(FIELD_TEXT_PX),
            line_height: text::LineHeight::Relative(1.2),
            font,
            align_x: text::Alignment::Left,
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        };
        let inner = FIELD_WIDTH_PX - 2.0 * FIELD_PADDING_PX;
        let width_of = |content: &str| {
            Renderer::Paragraph::with_text(Text {
                content,
                bounds: Size::new(f32::INFINITY, FIELD_HEIGHT_PX),
                size: Pixels(FIELD_TEXT_PX),
                line_height: text::LineHeight::Relative(1.2),
                font,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Center,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            })
            .min_bounds()
            .width
        };
        let text_at = Point::new(area.x + FIELD_PADDING_PX, area.center_y());
        renderer.with_layer(bounds, |renderer| {
            let outline = if error.is_some() {
                (self.palette.warning, 2.0)
            } else {
                (self.palette.control_value, 1.0)
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: area,
                    border: iced::Border {
                        color: outline.0,
                        width: outline.1,
                        radius: 4.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                self.palette.control_body,
            );
            if field.selected && !field.text.is_empty() {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(text_at.x - 1.0, area.y + 3.0),
                            Size::new(width_of(&field.text).min(inner) + 2.0, FIELD_HEIGHT_PX - 6.0),
                        ),
                        ..renderer::Quad::default()
                    },
                    self.palette.control_track,
                );
            }
            renderer.fill_text(line(field.text.clone(), inner), text_at, self.palette.text, area);
            if !field.selected {
                let x = text_at.x + width_of(&field.text[..field.caret]).min(inner);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(Point::new(x, area.y + 4.0), Size::new(1.0, FIELD_HEIGHT_PX - 8.0)),
                        ..renderer::Quad::default()
                    },
                    self.palette.text,
                );
            }
            if let Some(reason) = error {
                let place = typed_reason_bounds(area, width_of(&reason) + 2.0 * FIELD_PADDING_PX, bounds);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: place,
                        border: iced::Border {
                            color: self.palette.tile_border,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..renderer::Quad::default()
                    },
                    self.palette.control_body,
                );
                renderer.fill_text(
                    line(reason, place.width),
                    Point::new(place.x + FIELD_PADDING_PX, place.center_y()),
                    self.palette.text,
                    bounds,
                );
            }
        });
    }
}
