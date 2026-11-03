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
        let height = SELECT_ROW_PX * options.len() as f32;
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
        state.select_popup = Some(SelectPopup {
            module,
            widget,
            highlight: self.position(module, widget).round().max(0.0) as usize,
        });
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
    /// choice. Escape closes it with no edit. Left, Right, and Tab do
    /// nothing, so they cannot reach a control or move focus behind the list.
    pub(super) fn select_popup_key(
        &self,
        state: &mut State,
        key: &keyboard::Key,
        repeat: bool,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        use keyboard::key::Named;
        let Some(mut popup) = state.select_popup else {
            return false;
        };
        let Some(WidgetViewKind::Select { options }) = self.widget_kind(popup.module, popup.widget) else {
            return false;
        };
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
            keyboard::Key::Named(Named::ArrowLeft | Named::ArrowRight | Named::Tab) => true,
            _ => false,
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
    /// A press outside closes it with no edit, as does the wheel. A press on
    /// a row picks that row. A second press on the closed box is a
    /// double-click, which resets the control.
    pub(super) fn select_popup_event(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        over: bool,
        screen: Option<Point>,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let Some(popup) = state.select_popup else {
            return false;
        };
        match event {
            Event::Mouse(mouse::Event::WheelScrolled { .. }) if over && screen.is_some() => {
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
                let double = *button == mouse::Button::Left
                    && self.scene.hit(state.camera.to_world(screen), state.hit_radius())
                        == Hit::Widget {
                            module: popup.module,
                            widget: popup.widget,
                        }
                    && state.last_click.is_some_and(|(at, module, widget)| {
                        module == popup.module && widget == popup.widget && Instant::now() - at < DOUBLE_CLICK
                    });
                state.last_click = None;
                if double {
                    self.emit(
                        shell,
                        CanvasEvent::ResetControl {
                            module: popup.module,
                            widget: popup.widget,
                        },
                    );
                } else if *button == mouse::Button::Left
                    && let Some(rect) = self.select_popup_bounds_for(state, bounds, popup)
                    && rect.contains(screen)
                {
                    let index = ((screen.y - rect.y) / SELECT_ROW_PX).floor() as usize;
                    self.set_control(shell, popup.module, popup.widget, index as f32);
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
