//! The open choice list: its place on screen, and the keys and presses it
//! takes. It comes before every other canvas input, so a key or a press
//! never reaches the control behind the list. Spec:
//! `docs/design/panel-widgets.md`, Select list.

use super::*;

impl<Message> PatchCanvas<'_, Message> {
    /// Screen rectangle of the open list, kept readable at any canvas zoom.
    pub(super) fn select_popup_bounds(&self, state: &State, bounds: Rectangle) -> Option<Rectangle> {
        let popup = state.select_popup?;
        self.select_popup_bounds_for(state, bounds, popup)
    }

    /// Screen rectangle for a specified select popup.
    pub(super) fn select_popup_bounds_for(
        &self,
        state: &State,
        bounds: Rectangle,
        popup: SelectPopup,
    ) -> Option<Rectangle> {
        let module = self.scene.module(popup.module)?;
        let rect = module.widget_bounds(popup.widget)?;
        let WidgetViewKind::Select { options } = &module.panel.widgets.get(popup.widget)?.kind else {
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
            (anchor.y - height - 3.0).max(0.0)
        };
        Some(Rectangle::new(Point::new(x, y), Size::new(width, height)))
    }

    /// Opens a select list at its current value.
    pub(super) fn open_select(&self, state: &mut State, module: u32, widget: usize) {
        let count = self.select_options(module, widget).map_or(0, <[String]>::len);
        let mut popup = SelectPopup {
            module,
            widget,
            highlight: self.position(module, widget).round().max(0.0) as usize,
            scroll: 0,
            wheel: 0.0,
        };
        popup.reveal_highlight(select_rows(state.size.height, count));
        state.select_popup = Some(popup);
    }

    /// The choices of a select list, by module and widget.
    fn select_options(&self, module: u32, widget: usize) -> Option<&[String]> {
        match self.widget_kind(module, widget)? {
            WidgetViewKind::Select { options } => Some(options),
            _ => None,
        }
    }

    /// Closes a list whose module or widget has gone, or that a mode now
    /// hides. The event that found it goes on to normal handling, so a stale
    /// list never swallows it.
    pub(super) fn close_stale_select(&self, state: &mut State) {
        let live = state.select_popup.is_some_and(|popup| {
            self.scene
                .module(popup.module)
                .and_then(|module| module.panel.widgets.get(popup.widget))
                .is_some_and(|widget| matches!(widget.kind, WidgetViewKind::Select { .. }) && !widget.hidden)
        });
        if !live {
            state.select_popup = None;
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
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<&str>,
        repeat: bool,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        use keyboard::key::Named;
        let Some(mut popup) = state.select_popup else {
            return false;
        };
        let Some(options) = self.select_options(popup.module, popup.widget) else {
            return false;
        };
        let rows = select_rows(state.size.height, options.len());
        let used = match key.as_ref() {
            keyboard::Key::Named(Named::Escape) => {
                state.select_popup = None;
                state.last_click = None;
                true
            }
            keyboard::Key::Named(named @ (Named::ArrowUp | Named::ArrowDown)) => {
                let delta = if named == Named::ArrowUp { -1 } else { 1 };
                popup.highlight =
                    (popup.highlight as isize + delta).clamp(0, options.len().saturating_sub(1) as isize) as usize;
                popup.reveal_highlight(rows);
                state.select_popup = Some(popup);
                true
            }
            keyboard::Key::Named(Named::Enter | Named::Space) => {
                if !repeat {
                    self.set_control(shell, popup.module, popup.widget, popup.highlight as f32);
                    state.select_popup = None;
                }
                true
            }
            _ if modifiers.command() || modifiers.control() || modifiers.alt() => false,
            _ => {
                if let Some(next) = text.and_then(|text| type_ahead(options, popup.highlight, text)) {
                    popup.highlight = next;
                    popup.reveal_highlight(rows);
                    state.select_popup = Some(popup);
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
        bounds: Rectangle,
        over: bool,
        screen: Option<Point>,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let Some(mut popup) = state.select_popup else {
            return false;
        };
        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if over && screen.is_some() => {
                let list = self.select_popup_bounds_for(state, bounds, popup);
                if let (Some(list), Some(at)) = (list, screen)
                    && list.contains(at)
                {
                    // Stays open even when it is too short to scroll.
                    let count = self
                        .select_options(popup.module, popup.widget)
                        .map_or(0, <[String]>::len);
                    popup.scroll_by(*delta, count, select_rows(bounds.height, count));
                    state.select_popup = Some(popup);
                    shell.request_redraw();
                    shell.capture_event();
                    return true;
                }
                state.select_popup = None;
                state.last_click = None;
                shell.request_redraw();
                shell.capture_event();
                true
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) if !over => {
                state.select_popup = None;
                state.last_click = None;
                shell.request_redraw();
                true
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let Some(screen) = screen else {
                    return true;
                };
                state.select_popup = None;
                let last_click = state.last_click.take();
                let left = *button == mouse::Button::Left;
                // A press on a row is always a pick, even right after the
                // click that opened the list.
                if let Some(rect) = self.select_popup_bounds_for(state, bounds, popup)
                    && rect.contains(screen)
                {
                    if left {
                        let row = ((screen.y - rect.y) / SELECT_ROW_PX).floor() as usize;
                        self.set_control(shell, popup.module, popup.widget, (popup.scroll + row) as f32);
                    }
                } else if left
                    && self.scene.hit(state.camera.to_world(screen), state.hit_radius())
                        == (Hit::Widget {
                            module: popup.module,
                            widget: popup.widget,
                        })
                    && last_click.is_some_and(|(at, module, widget)| {
                        module == popup.module && widget == popup.widget && Instant::now() - at < DOUBLE_CLICK
                    })
                {
                    self.emit(
                        shell,
                        CanvasEvent::ResetControl {
                            module: popup.module,
                            widget: popup.widget,
                        },
                    );
                }
                shell.request_redraw();
                shell.capture_event();
                true
            }
            Event::Window(window::Event::Unfocused) => {
                state.select_popup = None;
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
    fn reveal_highlight(&mut self, rows: usize) {
        if self.highlight < self.scroll {
            self.scroll = self.highlight;
        } else if self.highlight >= self.scroll + rows {
            self.scroll = self.highlight + 1 - rows;
        }
    }

    /// Scrolls for one wheel event. A mouse wheel moves whole rows, one per
    /// line. A trackpad sends small pixel steps, so they add up in `wheel`
    /// until they make a row.
    fn scroll_by(&mut self, delta: mouse::ScrollDelta, count: usize, rows: usize) {
        let rows_moved = match delta {
            mouse::ScrollDelta::Lines { y, .. } => y,
            mouse::ScrollDelta::Pixels { y, .. } => y / SELECT_ROW_PX,
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
