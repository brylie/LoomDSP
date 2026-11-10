//! The open choice list: its place on screen, and the keys and presses it
//! takes. It comes before every other canvas input, so a key or a press
//! never reaches the control behind the list. Spec:
//! `docs/design/panel-widgets.md`, Select list.

use super::*;

impl<Message> PatchCanvas<'_, Message> {
    /// Screen rectangle of the open list, kept readable at any canvas zoom.
    pub(super) fn select_popup_bounds(&self, state: &State, bounds: Rect) -> Option<Rect> {
        state.focus.popup()?;
        let (module, widget) = state.focus.control()?;
        self.select_popup_bounds_for(state, bounds, module, widget)
    }

    /// Screen rectangle for the list of the select control `widget` on
    /// `module`.
    pub(super) fn select_popup_bounds_for(
        &self,
        state: &State,
        bounds: Rect,
        module: u32,
        widget: usize,
    ) -> Option<Rect> {
        let tile = self.scene.module(module)?;
        let rect = tile.widget_bounds(widget)?;
        let WidgetViewKind::Select { options } = &tile.panel.widgets.get(widget)?.kind else {
            return None;
        };
        let box_rect = select_box(rect);
        let anchor = state.camera.to_screen(box_rect.position());
        let width = (box_rect.width * state.camera.zoom)
            .max(SELECT_WIDTH_PX)
            .min(bounds.width);
        let height = SELECT_ROW_PX * select_rows(bounds.height, options.len()) as f32;
        let x = anchor.x.clamp(0.0, (bounds.width - width).max(0.0));
        let below = anchor.y + box_rect.height * state.camera.zoom + 3.0;
        let y = if below + height <= bounds.height {
            below
        } else {
            anchor.y - height - 3.0
        };
        // The box can be panned off either edge. The list stays in the canvas.
        let y = y.clamp(0.0, (bounds.height - height).max(0.0));
        Some(Rect::new(Point::new(x, y), Size::new(width, height)))
    }

    /// Opens a select list at its current value.
    pub(super) fn open_select(&self, state: &mut State, module: u32, widget: usize) {
        let count = self.select_options(module, widget).map_or(0, <[String]>::len);
        let mut popup = SelectPopup {
            highlight: self.position(module, widget).round().max(0.0) as usize,
            scroll: 0,
            wheel: 0.0,
        };
        popup.reveal_highlight(select_rows(state.size.height, count));
        state.focus.open_popup(module, widget, popup);
    }

    /// The choices of a select list, by module and widget.
    pub(super) fn select_options(&self, module: u32, widget: usize) -> Option<&[String]> {
        match self.widget_kind(module, widget)? {
            WidgetViewKind::Select { options } => Some(options),
            _ => None,
        }
    }

    /// A key while the list is open. Returns true if the list took it.
    ///
    /// Up and Down move only the highlight. Enter or Space commits one
    /// choice. Escape closes it with no edit. A typed character jumps to the
    /// next choice that starts with it. Every other key without Command, Ctrl,
    /// or Alt is used up, so `a` or Backspace cannot act behind the list.
    pub(super) fn select_popup_key(
        &self,
        state: &mut State,
        key: &Key,
        modifiers: Modifiers,
        text: Option<&str>,
        repeat: bool,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let (Some(mut popup), Some((module, widget))) = (state.focus.popup().copied(), state.focus.control()) else {
            return false;
        };
        let Some(options) = self.select_options(module, widget) else {
            return false;
        };
        let rows = select_rows(state.size.height, options.len());
        let used = match *key {
            Key::Escape => {
                state.focus.close_popup();
                state.last_click = None;
                true
            }
            named @ (Key::Up | Key::Down) => {
                let delta = if named == Key::Up { -1 } else { 1 };
                popup.highlight =
                    (popup.highlight as isize + delta).clamp(0, options.len().saturating_sub(1) as isize) as usize;
                popup.reveal_highlight(rows);
                if let Some(open) = state.focus.popup_mut() {
                    *open = popup;
                }
                true
            }
            Key::Enter | Key::Space => {
                if !repeat {
                    self.set_control(shell, module, widget, popup.highlight as f32);
                    state.focus.close_popup();
                }
                true
            }
            _ if modifiers.shortcut() || modifiers.control || modifiers.alt => false,
            _ => {
                if let Some(next) = text.and_then(|text| type_ahead(options, popup.highlight, text)) {
                    popup.highlight = next;
                    popup.reveal_highlight(rows);
                    if let Some(open) = state.focus.popup_mut() {
                        *open = popup;
                    }
                }
                true
            }
        };
        if used {
            shell.request_redraw();
            shell.capture_event();
        }
        used
    }

    /// The pointer and window events the open list takes. Returns true if
    /// the list used the event.
    ///
    /// A press outside closes it with no edit. The wheel scrolls it while
    /// the pointer is over it, and closes it elsewhere. A press on a row picks
    /// that row. A second press on the closed box is a double-click, which
    /// resets the control.
    pub(super) fn select_popup_event(
        &self,
        state: &mut State,
        event: &Event,
        pointer: Option<Point>,
        bounds: Rect,
        now: Instant,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let over = pointer.is_some_and(|at| bounds.contains(at));
        let screen = relative(pointer, bounds);
        let (Some(mut popup), Some((module, widget))) = (state.focus.popup().copied(), state.focus.control()) else {
            return false;
        };
        match event {
            Event::Pointer(PointerEvent::Scrolled(delta)) if over && screen.is_some() => {
                let list = self.select_popup_bounds_for(state, bounds, module, widget);
                if let (Some(list), Some(at)) = (list, screen)
                    && list.contains(at)
                {
                    // Stays open even when it is too short to scroll.
                    let count = self.select_options(module, widget).map_or(0, <[String]>::len);
                    popup.scroll_by(*delta, count, select_rows(bounds.height, count));
                    if let Some(open) = state.focus.popup_mut() {
                        *open = popup;
                    }
                    shell.request_redraw();
                    shell.capture_event();
                    return true;
                }
                state.focus.close_popup();
                state.last_click = None;
                shell.request_redraw();
                shell.capture_event();
                true
            }
            Event::Pointer(PointerEvent::Pressed(_)) if !over => {
                state.last_click = None;
                // The press took the keys from the canvas, as it does with no
                // list open.
                state.focus.clear();
                shell.request_redraw();
                true
            }
            Event::Pointer(PointerEvent::Pressed(button)) => {
                let Some(screen) = screen else {
                    return true;
                };
                state.focus.close_popup();
                let last_click = state.last_click.take();
                let left = *button == PointerButton::Left;
                // A press on a row is always a pick, even right after the
                // click that opened the list.
                if let Some(rect) = self.select_popup_bounds_for(state, bounds, module, widget)
                    && rect.contains(screen)
                {
                    if left {
                        let row = ((screen.y - rect.y) / SELECT_ROW_PX).floor() as usize;
                        self.set_control(shell, module, widget, (popup.scroll + row) as f32);
                    }
                } else if left
                    && self.scene.hit(state.camera.to_world(screen), state.hit_radius())
                        == (Hit::Widget { module, widget })
                    && last_click.is_some_and(|(at, clicked, clicked_widget, _)| {
                        clicked == module && clicked_widget == widget && now - at < DOUBLE_CLICK
                    })
                {
                    self.emit(
                        shell,
                        CanvasEvent::ResetControl {
                            module,
                            widget,
                            part: ControlPart::Main,
                        },
                    );
                }
                shell.request_redraw();
                shell.capture_event();
                true
            }
            Event::WindowUnfocused => {
                state.focus.close_popup();
                false
            }
            _ => false,
        }
    }
}

/// Most rows an open list shows at once.
const SELECT_MAX_ROWS: usize = 12;

/// How many rows of a list with `count` choices show: up to 12, and no more
/// than fit in a canvas `height` pixels tall, so the list stays inside it.
pub(super) fn select_rows(height: f32, count: usize) -> usize {
    let room = (height / SELECT_ROW_PX).floor().max(1.0) as usize;
    count.min(SELECT_MAX_ROWS).min(room)
}

/// The next choice after `highlight` that starts with `text`, ignoring case,
/// wrapping round. It starts after the highlight, so pressing the same letter
/// again walks through the matches.
fn type_ahead(options: &[String], highlight: usize, text: &str) -> Option<usize> {
    if text.chars().any(char::is_control) {
        return None;
    }
    let wanted = text.to_lowercase();
    (1..=options.len())
        .map(|step| (highlight + step) % options.len())
        .find(|&index| options[index].to_lowercase().starts_with(&wanted))
}

impl SelectPopup {
    /// Moves the scroll by the least that shows the highlighted row.
    pub(super) fn reveal_highlight(&mut self, rows: usize) {
        if self.highlight < self.scroll {
            self.scroll = self.highlight;
        } else if self.highlight >= self.scroll + rows {
            self.scroll = self.highlight + 1 - rows;
        }
    }

    /// Scrolls for one wheel event. A mouse wheel moves whole rows, one per
    /// line. A trackpad sends small pixel steps, so they add up in `wheel`
    /// until they make a row.
    fn scroll_by(&mut self, delta: ScrollDelta, count: usize, rows: usize) {
        let rows_moved = match delta {
            ScrollDelta::Lines { y, .. } => y,
            ScrollDelta::Pixels { y, .. } => y / SELECT_ROW_PX,
        };
        self.wheel += rows_moved;
        let whole = self.wheel.trunc();
        self.wheel -= whole;
        let last = count.saturating_sub(rows) as isize;
        // Up on the wheel shows earlier rows.
        let wanted = self.scroll as isize - whole as isize;
        self.scroll = wanted.clamp(0, last) as usize;
        // Movement past an end is lost, so it cannot delay the way back.
        if wanted != self.scroll as isize {
            self.wheel = 0.0;
        }
    }
}
