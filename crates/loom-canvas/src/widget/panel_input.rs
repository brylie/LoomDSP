//! Panel controls under the pointer and keyboard: drags, buttons, choice pop-ups, and focus.

use super::*;

/// The control after (or with `back`, before) `current` in focus order,
/// wrapping around. Lights are skipped. `None` if the panel has no
/// controls.
pub(super) fn next_focus(panel: &crate::scene::PanelView, current: Option<usize>, back: bool) -> Option<usize> {
    let controls = focusable(panel);
    if controls.is_empty() {
        return None;
    }
    let at = current.and_then(|c| controls.iter().position(|&i| i == c));
    let next = match (at, back) {
        (None, false) => 0,
        (None, true) => controls.len() - 1,
        (Some(i), false) => (i + 1) % controls.len(),
        (Some(i), true) => (i + controls.len() - 1) % controls.len(),
    };
    Some(controls[next])
}

/// The panel's widgets that keyboard focus can land on, in focus order:
/// its controls, without lights and without controls the mode hides
/// (ADR-0030). Tab and Enter both walk this list.
pub(super) fn focusable(panel: &crate::scene::PanelView) -> Vec<usize> {
    (0..panel.widgets.len())
        .filter(|&i| panel.widgets[i].is_control() && !panel.widgets[i].hidden)
        .collect()
}

impl<Message> PatchCanvas<'_, Message> {
    pub(super) fn is_control(&self, module: u32, widget: usize) -> bool {
        self.scene
            .module(module)
            .and_then(|m| m.panel.widgets.get(widget))
            .is_some_and(|w| w.is_control())
    }

    pub(super) fn widget_kind(&self, module: u32, widget: usize) -> Option<&WidgetViewKind> {
        Some(&self.scene.module(module)?.panel.widgets.get(widget)?.kind)
    }

    /// Finds the topmost growable tile whose bottom-right grip is under the pointer.
    pub(super) fn resize_at(&self, point: Point) -> Option<&crate::scene::ModuleView> {
        self.scene
            .modules()
            .iter()
            .rev()
            .find(|tile| tile.bounds().contains(point))
            .filter(|tile| {
                let bounds = tile.bounds();
                tile.is_growable()
                    && Rectangle::new(
                        Point::new(bounds.x + bounds.width - GRID, bounds.y + bounds.height - GRID),
                        Size::new(GRID, GRID),
                    )
                    .contains(point)
            })
    }

    /// The modulated knob whose ring a press at `world` would grab to drag
    /// depth, given what the hit test found there. Not in MIDI learn mode.
    /// `press` and the hover cursor both ask this one method, so the cursor
    /// always tells the truth about the press (#198 was the two disagreeing).
    pub(super) fn depth_drag_at(&self, hit: &Hit, world: Point) -> Option<(u32, usize)> {
        if self.learn.is_some() || !matches!(hit, Hit::Widget { .. } | Hit::Module(_)) {
            return None;
        }
        self.range_ring_at(world)
    }

    /// The modulated knob whose range ring is under `world`, if any.
    pub(super) fn range_ring_at(&self, world: Point) -> Option<(u32, usize)> {
        let module = self.scene.modules().iter().rev().find(|m| m.bounds().contains(world))?;
        (0..module.panel.widgets.len())
            .find(|&w| self.on_range_ring(module.id, w, world))
            .map(|w| (module.id, w))
    }

    /// True if `world` is on the ring around a knob where its range arc is
    /// drawn, and the knob has an enabled assignment: there a drag changes
    /// depth, not the value. This holds when the arc has zero length and
    /// the knob shows the dot, so depth 0 can be dragged back out of.
    pub(super) fn on_range_ring(&self, module: u32, widget: usize, world: Point) -> bool {
        let Some(m) = self.scene.module(module) else {
            return false;
        };
        let (Some(rect), Some(WidgetViewKind::Knob { .. })) =
            (m.widget_bounds(widget), self.widget_kind(module, widget))
        else {
            return false;
        };
        let modulated = self.values.and_then(|v| v.get(module, widget)).is_some_and(|v| {
            matches!(
                v.modulation,
                crate::controls::ModulationMark::Range { .. } | crate::controls::ModulationMark::Dot
            )
        });
        let d = world - crate::controls::knob_centre(rect);
        let distance = (d.x * d.x + d.y * d.y).sqrt();
        modulated && (crate::controls::RANGE_RING_INNER..=crate::controls::RANGE_RING_OUTER).contains(&distance)
    }

    pub(super) fn position(&self, module: u32, widget: usize) -> f32 {
        self.values.map_or(0.0, |v| v.position(module, widget))
    }

    /// True when Space or Enter is still holding this button.
    pub(super) fn key_holds_button(state: &State, module: u32, widget: usize) -> bool {
        state
            .key_holds
            .iter()
            .flatten()
            .any(|hold| hold.module == module && hold.widget == widget)
    }

    /// True when the pointer is still down on this button.
    pub(super) fn pointer_holds_button(state: &State, module: u32, widget: usize) -> bool {
        matches!(
            state.interaction,
            Interaction::Holding {
                module: held,
                widget: held_widget,
            } if held == module && held_widget == widget
        )
    }

    /// Ends a pointer hold, if there is one. The release is reported only
    /// when no key is still holding that button.
    pub(super) fn release_pointer_hold(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        if let Interaction::Holding { module, widget } = state.interaction {
            state.interaction = Interaction::Idle;
            if !Self::key_holds_button(state, module, widget) {
                self.emit(shell, CanvasEvent::ButtonReleased { module, widget });
            }
        }
    }

    /// Space or Enter, the keys that hold a button.
    pub(super) fn button_key(key: &keyboard::Key) -> Option<keyboard::key::Named> {
        use keyboard::key::Named;
        match key.as_ref() {
            keyboard::Key::Named(named @ (Named::Space | Named::Enter)) => Some(named),
            _ => None,
        }
    }

    /// Starts a hold for one key. If that key was holding another button, and
    /// nothing else is, that button is released first.
    pub(super) fn press_button_key(
        &self,
        state: &mut State,
        key: keyboard::key::Named,
        module: u32,
        widget: usize,
        shell: &mut Shell<'_, Message>,
    ) {
        let same = state
            .key_holds
            .iter()
            .flatten()
            .any(|hold| hold.key == key && hold.module == module && hold.widget == widget);
        if same {
            return;
        }
        self.release_button_key(state, key, shell);
        self.emit(shell, CanvasEvent::ButtonPressed { module, widget });
        if let Some(slot) = state.key_holds.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(KeyHold { key, module, widget });
        }
    }

    /// Ends one key's hold. The button stays down while another key holds it.
    pub(super) fn release_button_key(
        &self,
        state: &mut State,
        key: keyboard::key::Named,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(index) = state
            .key_holds
            .iter()
            .position(|slot| slot.is_some_and(|hold| hold.key == key))
        else {
            return;
        };
        let Some(hold) = state.key_holds[index].take() else {
            return;
        };
        let still_held = Self::key_holds_button(state, hold.module, hold.widget)
            || Self::pointer_holds_button(state, hold.module, hold.widget);
        if !still_held {
            self.emit(
                shell,
                CanvasEvent::ButtonReleased {
                    module: hold.module,
                    widget: hold.widget,
                },
            );
            shell.request_redraw();
            shell.capture_event();
        }
    }

    /// Ends every keyboard hold. Used when the window loses focus, because
    /// the key releases never arrive.
    pub(super) fn release_every_key_hold(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        let holds = std::mem::take(&mut state.key_holds);
        let mut reported = [(0_u32, 0_usize); 2];
        let mut count = 0;
        for hold in holds.into_iter().flatten() {
            if reported[..count].contains(&(hold.module, hold.widget))
                || Self::pointer_holds_button(state, hold.module, hold.widget)
            {
                continue;
            }
            reported[count] = (hold.module, hold.widget);
            count += 1;
            self.emit(
                shell,
                CanvasEvent::ButtonReleased {
                    module: hold.module,
                    widget: hold.widget,
                },
            );
        }
        if count > 0 {
            shell.request_redraw();
            shell.capture_event();
        }
    }

    /// Ends the hold for the key that came up.
    pub(super) fn release_key(&self, state: &mut State, key: &keyboard::Key, shell: &mut Shell<'_, Message>) {
        if let Some(named) = Self::button_key(key) {
            self.release_button_key(state, named, shell);
        }
        if state.arrow_gesture
            && matches!(
                key.as_ref(),
                keyboard::Key::Named(
                    keyboard::key::Named::ArrowUp
                        | keyboard::key::Named::ArrowRight
                        | keyboard::key::Named::ArrowDown
                        | keyboard::key::Named::ArrowLeft
                )
            )
        {
            let due = Instant::now() + ARROW_GROUP;
            state.arrow_deadline = Some(due);
            self.emit(shell, CanvasEvent::ArrowGroupPending);
            shell.request_redraw_at(due);
        }
    }

    /// Selects the module of a control being edited, so the inspector
    /// follows.
    pub(super) fn select_for_edit(&self, module: u32, shell: &mut Shell<'_, Message>) {
        if self.selection.len() != 1 || !self.selection.contains(&module) {
            self.emit(shell, CanvasEvent::Select(vec![module]));
        }
    }

    pub(super) fn set_control(&self, shell: &mut Shell<'_, Message>, module: u32, widget: usize, position: f32) {
        self.emit(
            shell,
            CanvasEvent::SetControl {
                module,
                widget,
                position,
            },
        );
    }

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

    /// Pointer down on a button. The tile stays put. Pointer up reports a
    /// release, whichever mode the app has stored.
    pub(super) fn press_button(&self, state: &mut State, module: u32, widget: usize, shell: &mut Shell<'_, Message>) {
        self.emit(shell, CanvasEvent::ButtonPressed { module, widget });
        state.interaction = Interaction::Holding { module, widget };
    }

    /// The keyboard path to panel controls (NFR-UX-01). Returns true if the
    /// key was used.
    pub(super) fn focus_keys(
        &self,
        state: &mut State,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        repeat: bool,
        _over: bool,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        use keyboard::key::Named;
        if let Some(due) = state.arrow_deadline.take() {
            let arrow = matches!(
                key.as_ref(),
                keyboard::Key::Named(Named::ArrowUp | Named::ArrowRight | Named::ArrowDown | Named::ArrowLeft)
            );
            if !arrow || Instant::now() >= due {
                state.arrow_gesture = false;
                self.emit(shell, CanvasEvent::ArrowGroupEnd);
            } else {
                self.emit(shell, CanvasEvent::ArrowGroupResumed);
            }
        }
        // Focus is only valid on a control of the one selected module.
        let selected = (self.selection.len() == 1).then(|| *self.selection.iter().next().unwrap_or(&0));
        let hidden = |m: u32, w: usize| {
            self.scene
                .module(m)
                .and_then(|module| module.panel.widgets.get(w))
                .is_some_and(|widget| widget.hidden)
        };
        // A mode change (from MIDI, say) can hide the focused control. The
        // key meant for it is used up, so it never moves the tile instead.
        let lost_to_mode = state.focus.is_some_and(|(m, w)| Some(m) == selected && hidden(m, w));
        if state.focus.is_some_and(|(m, w)| {
            Some(m) != selected
                || !self
                    .scene
                    .module(m)
                    .and_then(|module| module.panel.widgets.get(w))
                    .is_some_and(|widget| widget.is_control() && !widget.hidden)
        }) {
            state.focus = None;
        }
        // A hidden select is not drawn, so its list closes too.
        if state
            .select_popup
            .is_some_and(|popup| hidden(popup.module, popup.widget))
        {
            state.select_popup = None;
        }
        if lost_to_mode
            && matches!(
                key.as_ref(),
                keyboard::Key::Named(
                    Named::ArrowUp
                        | Named::ArrowRight
                        | Named::ArrowDown
                        | Named::ArrowLeft
                        | Named::Space
                        | Named::Enter
                )
            )
        {
            shell.capture_event();
            return true;
        }
        if let Some(mut popup) = state.select_popup
            && let Some(WidgetViewKind::Select { options }) = self.widget_kind(popup.module, popup.widget)
        {
            let used = match key.as_ref() {
                keyboard::Key::Named(Named::Escape) => {
                    state.select_popup = None;
                    state.last_click = None;
                    true
                }
                keyboard::Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                    let delta = if matches!(key.as_ref(), keyboard::Key::Named(Named::ArrowUp)) {
                        -1
                    } else {
                        1
                    };
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
                return true;
            }
        } else {
            state.select_popup = None;
        }
        let used = match key.as_ref() {
            // Escape hides pinned lines before it does anything else
            // (modulation.md, Lines).
            keyboard::Key::Named(Named::Escape) if self.pin(state).is_some() => {
                state.pinned = None;
                true
            }
            keyboard::Key::Named(Named::Tab) => {
                let mut order: Vec<_> = self.scene.modules().iter().collect();
                order.sort_by(|a, b| {
                    a.position
                        .y
                        .total_cmp(&b.position.y)
                        .then_with(|| a.position.x.total_cmp(&b.position.x))
                        .then_with(|| a.id.cmp(&b.id))
                });
                if order.is_empty() {
                    return false;
                }
                let back = modifiers.shift();
                if let Some((module, widget)) = state.focus {
                    let controls = self
                        .scene
                        .module(module)
                        .map(|m| focusable(&m.panel))
                        .unwrap_or_default();
                    let next = controls.iter().position(|&i| i == widget).and_then(|at| {
                        if back {
                            at.checked_sub(1)
                        } else {
                            (at + 1 < controls.len()).then_some(at + 1)
                        }
                    });
                    if let Some(next) = next {
                        state.focus = Some((module, controls[next]));
                        state.tab_focus = true;
                        state.focus_first = true;
                        shell.request_redraw();
                        shell.capture_event();
                        return true;
                    }
                }
                let at = selected.and_then(|id| order.iter().position(|m| m.id == id));
                let next = match at {
                    Some(at) if back => at.checked_sub(1).unwrap_or(order.len() - 1),
                    Some(at) => (at + 1) % order.len(),
                    None if back => order.len() - 1,
                    None => 0,
                };
                state.focus = None;
                self.emit(shell, CanvasEvent::Select(vec![order[next].id]));
                true
            }
            keyboard::Key::Named(Named::Escape) if state.focus.is_some() => {
                state.focus = None;
                // In MIDI learn mode, Escape also ends the mode, so the app
                // must see it too.
                self.learn.is_none()
            }
            keyboard::Key::Named(Named::Escape) if !self.selection.is_empty() && self.learn.is_none() => {
                self.emit(shell, CanvasEvent::Select(Vec::new()));
                true
            }
            keyboard::Key::Named(Named::Enter) if state.focus.is_none() => {
                let Some(module) = selected.and_then(|id| self.scene.module(id)) else {
                    return false;
                };
                let Some(widget) = next_focus(&module.panel, None, false) else {
                    return false;
                };
                state.focus = Some((module.id, widget));
                state.tab_focus = true;
                true
            }
            // In MIDI learn mode the keyboard arms the focused control, like
            // a click, and never changes a value (midi-learn.md).
            // With nothing focused, Space still toggles Run below: that is
            // the transport, not a parameter.
            keyboard::Key::Named(Named::Space | Named::Enter) if self.learn.is_some() && state.focus.is_some() => {
                if !repeat && let Some((module, widget)) = state.focus {
                    self.emit(shell, CanvasEvent::Learn { module, widget });
                }
                true
            }
            keyboard::Key::Named(Named::ArrowUp | Named::ArrowRight | Named::ArrowDown | Named::ArrowLeft)
                if self.learn.is_some() =>
            {
                false
            }
            keyboard::Key::Named(
                named @ (Named::ArrowUp | Named::ArrowRight | Named::ArrowDown | Named::ArrowLeft),
            ) => {
                if modifiers.alt() && !modifiers.shift() && self.selection.len() == 1 {
                    let id = *self.selection.iter().next().expect("one selected module");
                    if let Some(tile) = self.scene.module(id).filter(|tile| tile.is_growable()) {
                        let width = (tile.width() / GRID) as u8;
                        let height = (tile.height() / GRID) as u8;
                        let size = match named {
                            Named::ArrowUp => (width, height.saturating_sub(1)),
                            Named::ArrowDown => (width, height.saturating_add(1)),
                            Named::ArrowLeft => (width.saturating_sub(1), height),
                            Named::ArrowRight => (width.saturating_add(1), height),
                            _ => unreachable!(),
                        };
                        if !state.arrow_gesture {
                            state.arrow_gesture = true;
                            self.emit(shell, CanvasEvent::GestureStart);
                        }
                        self.emit(shell, CanvasEvent::Resize { module: id, size });
                        shell.request_redraw();
                        shell.capture_event();
                        return true;
                    }
                    return false;
                }
                let Some((module, widget)) = state.focus else {
                    if self.selection.is_empty() {
                        return false;
                    }
                    let step = if modifiers.shift() { 10.0 } else { 1.0 } * GRID;
                    let delta = match named {
                        Named::ArrowUp => Vector::new(0.0, -step),
                        Named::ArrowRight => Vector::new(step, 0.0),
                        Named::ArrowDown => Vector::new(0.0, step),
                        Named::ArrowLeft => Vector::new(-step, 0.0),
                        _ => unreachable!(),
                    };
                    if !state.arrow_gesture {
                        state.arrow_gesture = true;
                        self.emit(shell, CanvasEvent::GestureStart);
                    }
                    self.emit(
                        shell,
                        CanvasEvent::Move {
                            ids: self.selection.iter().copied().collect(),
                            delta,
                        },
                    );
                    shell.request_redraw();
                    shell.capture_event();
                    return true;
                };
                let up = matches!(named, Named::ArrowUp | Named::ArrowRight);
                if matches!(self.widget_kind(module, widget), Some(WidgetViewKind::Button)) {
                    self.emit(
                        shell,
                        CanvasEvent::ButtonArrow {
                            module,
                            widget,
                            high: up,
                        },
                    );
                } else {
                    let position = self.position(module, widget);
                    let next = match self.widget_kind(module, widget) {
                        Some(WidgetViewKind::Knob { steps }) => knob_step(position, up, modifiers.shift(), *steps),
                        Some(WidgetViewKind::Toggle) => {
                            if up {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        Some(
                            WidgetViewKind::Radio { options }
                            | WidgetViewKind::Select { options }
                            | WidgetViewKind::Segmented { options, .. },
                        ) => {
                            let last = options.len().saturating_sub(1) as f32;
                            (position.round() + if up { 1.0 } else { -1.0 }).clamp(0.0, last)
                        }
                        _ => return false,
                    };
                    if !state.arrow_gesture {
                        state.arrow_gesture = true;
                        self.emit(shell, CanvasEvent::GestureStart);
                    }
                    if next != position {
                        self.set_control(shell, module, widget, next);
                    }
                }
                true
            }
            keyboard::Key::Named(named @ (Named::Space | Named::Enter)) => {
                if let Some((module, widget)) = state.focus {
                    let position = self.position(module, widget);
                    match self.widget_kind(module, widget) {
                        Some(WidgetViewKind::Toggle) => {
                            self.set_control(shell, module, widget, if position > 0.5 { 0.0 } else { 1.0 })
                        }
                        // A repeated key must not press again. The first press is
                        // still held, so key up can release it.
                        Some(WidgetViewKind::Button) if repeat => {}
                        Some(WidgetViewKind::Button) => {
                            self.press_button_key(state, named, module, widget, shell);
                        }
                        Some(WidgetViewKind::Radio { options } | WidgetViewKind::Segmented { options, .. }) => {
                            let next = (position.round() as usize + 1) % options.len().max(1);
                            self.set_control(shell, module, widget, next as f32);
                        }
                        Some(WidgetViewKind::Select { .. }) if !repeat => self.open_select(state, module, widget),
                        Some(WidgetViewKind::Knob { .. }) if matches!(named, Named::Space) && !repeat => {
                            self.emit(shell, CanvasEvent::ToggleRun);
                        }
                        _ => {}
                    }
                    !matches!(self.widget_kind(module, widget), Some(WidgetViewKind::Knob { .. }) if matches!(named, Named::Enter))
                } else if matches!(named, Named::Space) {
                    // Space with nothing focused is the Run shortcut. A
                    // repeat is swallowed so holding the key does not toggle
                    // again. Enter is not that shortcut.
                    if !repeat {
                        self.emit(shell, CanvasEvent::ToggleRun);
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if used {
            shell.request_redraw();
            shell.capture_event();
        }
        used
    }
}
