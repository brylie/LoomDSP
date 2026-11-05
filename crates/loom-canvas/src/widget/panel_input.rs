//! Panel controls under the pointer and keyboard: drags, buttons, choice pop-ups, and focus.

use super::focus::RangePart;
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

/// True for the four arrow keys.
pub(super) fn is_arrow(key: &keyboard::Key) -> bool {
    use keyboard::key::Named;
    matches!(
        key.as_ref(),
        keyboard::Key::Named(Named::ArrowUp | Named::ArrowRight | Named::ArrowDown | Named::ArrowLeft)
    )
}

/// True for a modifier key pressed on its own, which the window reports as
/// a key press too.
fn is_modifier(key: &keyboard::Key) -> bool {
    use keyboard::key::Named;
    matches!(
        key.as_ref(),
        keyboard::Key::Named(Named::Shift | Named::Control | Named::Alt | Named::AltGraph | Named::Meta | Named::Super)
    )
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
    pub(super) fn depth_drag_at(&self, hit: &Hit, world: Point) -> Option<(u32, usize, ControlPart)> {
        if self.learn.is_some() || !matches!(hit, Hit::Widget { .. } | Hit::Module(_)) {
            return None;
        }
        self.range_ring_at(world)
    }

    /// The modulated knob whose range ring is under `world`, if any.
    pub(super) fn range_ring_at(&self, world: Point) -> Option<(u32, usize, ControlPart)> {
        let module = self.scene.modules().iter().rev().find(|m| m.bounds().contains(world))?;
        (0..module.panel.widgets.len()).find_map(|w| Some((module.id, w, self.range_ring_part(module.id, w, world)?)))
    }

    /// The scale of a depth drag: an XY pad's bar moves along its own axis.
    pub(super) fn depth_scale(&self, module: u32, widget: usize, part: ControlPart, zoom: f32) -> ValueDrag {
        let rect = self.scene.module(module).and_then(|m| m.widget_bounds(widget));
        match (self.widget_kind(module, widget), rect) {
            (Some(WidgetViewKind::XyPad { .. }), Some(rect)) => ValueDrag::xy_axis(xy_area(rect), part, zoom),
            _ => self.value_drag(module, widget, zoom),
        }
    }

    /// The value of a control that takes a cable dropped at `world`: the
    /// nearest edge of an XY pad (panel-widgets.md, XY pad).
    pub(super) fn drop_part(&self, module: u32, widget: usize, world: Point) -> ControlPart {
        let rect = self.scene.module(module).and_then(|m| m.widget_bounds(widget));
        match (self.widget_kind(module, widget), rect) {
            (Some(WidgetViewKind::XyPad { .. }), Some(rect)) => xy_nearest_edge(xy_area(rect), world),
            (Some(WidgetViewKind::Range { vertical, .. }), Some(rect)) => {
                let (low, high) = (
                    self.part_position(module, widget, ControlPart::Main),
                    self.part_position(module, widget, ControlPart::Second),
                );
                range_nearest(rect, *vertical, low, high, world)
            }
            _ => ControlPart::Main,
        }
    }

    /// What a double-click at `world` resets: both values of an XY pad, one or
    /// both of a range, or the control's one value. `None` on a range's track
    /// outside the band, where it only selects the module.
    pub(super) fn reset_part(&self, module: u32, widget: usize, world: Point) -> Option<ControlPart> {
        match self.widget_kind(module, widget) {
            Some(WidgetViewKind::XyPad { .. }) => Some(ControlPart::Both),
            Some(WidgetViewKind::Range { vertical, .. }) => {
                let rect = self.scene.module(module)?.widget_bounds(widget)?;
                let low = self.part_position(module, widget, ControlPart::Main);
                let high = self.part_position(module, widget, ControlPart::Second);
                RangeGrab::at(rect, *vertical, low, high, world).map(RangeGrab::reset)
            }
            _ => Some(ControlPart::Main),
        }
    }

    /// The keyboard on a range: going inside, picking a part, grabbing it, and
    /// moving it (panel-widgets.md, rule 4). Returns true if the key was used.
    /// Learn mode has its own keys, and Tab always leaves.
    fn range_keys(
        &self,
        state: &mut State,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        repeat: bool,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        use keyboard::key::Named;
        let Some((module, widget)) = state.focus.control() else {
            return false;
        };
        let Some(WidgetViewKind::Range { vertical, .. }) = self.widget_kind(module, widget) else {
            state.focus.set_range_part(None);
            return false;
        };
        let vertical = *vertical;
        if self.learn.is_some() {
            state.focus.set_range_part(None);
            return false;
        }
        let keyboard::Key::Named(named) = key.as_ref() else {
            return false;
        };
        let low = self.part_position(module, widget, ControlPart::Main);
        let high = self.part_position(module, widget, ControlPart::Second);
        match (named, state.focus.range_part()) {
            (Named::Tab, _) => {
                state.focus.set_range_part(None);
                false
            }
            (Named::Enter, None) => {
                if !repeat {
                    state.focus.set_range_part(Some(RangePart { part: 0, grab: None }));
                }
                true
            }
            // Space on the widget itself does what it does on a knob.
            (Named::Space | Named::Escape, None) => false,
            (Named::ArrowUp | Named::ArrowDown | Named::ArrowLeft | Named::ArrowRight, None) => true,
            (Named::Escape, Some(inside)) if inside.grab.is_some() => {
                state.focus.set_range_part(Some(RangePart { grab: None, ..inside }));
                true
            }
            // Pinned lines hide first; the next Escape leaves (modulation.md,
            // Lines).
            (Named::Escape, Some(_)) if self.pin(state).is_some() => false,
            (Named::Escape, Some(_)) => {
                state.focus.set_range_part(None);
                true
            }
            (Named::Enter, Some(inside)) if inside.grab.is_some() => {
                if !repeat {
                    state.focus.set_range_part(Some(RangePart { grab: None, ..inside }));
                }
                true
            }
            (Named::Space, Some(inside)) if inside.grab.is_some() => true,
            (Named::Space | Named::Enter, Some(inside)) => {
                if !repeat {
                    let grab = match inside.part {
                        0 => RangeGrab::handle(ControlPart::Main, low, high),
                        2 => RangeGrab::handle(ControlPart::Second, low, high),
                        _ => RangeGrab::Band,
                    };
                    state.focus.set_range_part(Some(RangePart {
                        grab: Some(grab),
                        ..inside
                    }));
                }
                true
            }
            (Named::ArrowUp | Named::ArrowDown | Named::ArrowLeft | Named::ArrowRight, Some(inside)) => {
                let along = if vertical {
                    matches!(named, Named::ArrowUp | Named::ArrowDown)
                } else {
                    matches!(named, Named::ArrowLeft | Named::ArrowRight)
                };
                if !along {
                    return true;
                }
                let up = matches!(named, Named::ArrowUp | Named::ArrowRight);
                let Some(grab) = inside.grab else {
                    let part = if up {
                        inside.part.saturating_add(1).min(2)
                    } else {
                        inside.part.saturating_sub(1)
                    };
                    state.focus.set_range_part(Some(RangePart { part, ..inside }));
                    return true;
                };
                let step = if modifiers.shift() { 0.001 } else { 0.01 };
                let (_, next_low, next_high) = grab.moved(low, high, if up { step } else { -step });
                self.arrow_step(shell);
                self.set_pad(shell, module, widget, (low, high), (next_low, next_high));
                true
            }
            _ => false,
        }
    }

    /// Sends the values of an XY pad that changed.
    pub(super) fn set_pad(
        &self,
        shell: &mut Shell<'_, Message>,
        module: u32,
        widget: usize,
        before: (f32, f32),
        next: (f32, f32),
    ) {
        if next.0 != before.0 {
            self.set_part(shell, module, widget, ControlPart::Main, next.0);
        }
        if next.1 != before.1 {
            self.set_part(shell, module, widget, ControlPart::Second, next.1);
        }
    }

    /// The position of one value of a control.
    pub(super) fn part_position(&self, module: u32, widget: usize, part: ControlPart) -> f32 {
        self.values.map_or(0.0, |v| v.part_position(module, widget, part))
    }

    /// How a drag on this control turns pointer movement into a value.
    pub(super) fn value_drag(&self, module: u32, widget: usize, zoom: f32) -> ValueDrag {
        let rect = self.scene.module(module).and_then(|m| m.widget_bounds(widget));
        match (self.widget_kind(module, widget), rect) {
            (Some(WidgetViewKind::Fader { vertical, .. }), Some(rect)) => ValueDrag::fader(rect, *vertical, zoom),
            (Some(WidgetViewKind::Range { vertical, .. }), Some(rect)) => ValueDrag::range(rect, *vertical, zoom),
            (Some(WidgetViewKind::Number { steps, .. }), _) => ValueDrag::number(*steps),
            _ => ValueDrag::KNOB,
        }
    }

    /// The value whose depth a press at `world` would drag: when `world` is on
    /// the ring around a knob where its range arc is drawn, or on the place of
    /// a bar beside a fader or an XY pad, and that value has an enabled
    /// assignment. This holds when the range has zero length and the control
    /// shows the dot, so depth 0 can be dragged back out of.
    pub(super) fn range_ring_part(&self, module: u32, widget: usize, world: Point) -> Option<ControlPart> {
        let rect = self.scene.module(module)?.widget_bounds(widget)?;
        let part = match self.widget_kind(module, widget)? {
            WidgetViewKind::Knob { .. } => {
                let d = world - crate::controls::knob_centre(rect);
                let distance = (d.x * d.x + d.y * d.y).sqrt();
                (crate::controls::RANGE_RING_INNER..=crate::controls::RANGE_RING_OUTER)
                    .contains(&distance)
                    .then_some(ControlPart::Main)
            }
            WidgetViewKind::Fader { vertical, .. } => {
                crate::controls::on_fader_bar(rect, *vertical, world).then_some(ControlPart::Main)
            }
            WidgetViewKind::XyPad { .. } => xy_bar_at(xy_area(rect), world),
            WidgetViewKind::Range { vertical, .. } => range_bar_at(rect, *vertical, world),
            _ => None,
        }?;
        let mark = self.values?.part_mark(module, widget, part);
        matches!(
            mark,
            crate::controls::ModulationMark::Range { .. } | crate::controls::ModulationMark::Dot
        )
        .then_some(part)
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

    /// Ends what keys are doing on the canvas, because something else now
    /// takes them: no button stays held, and an arrow-key edit closes.
    pub(super) fn leave_keys(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        self.release_every_key_hold(state, shell);
        self.end_arrow_group(shell, false);
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
        if is_arrow(key) {
            self.emit(shell, CanvasEvent::ArrowReleased { at: Instant::now() });
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
        self.set_part(shell, module, widget, ControlPart::Main, position);
    }

    /// Sets one value of a control.
    pub(super) fn set_part(
        &self,
        shell: &mut Shell<'_, Message>,
        module: u32,
        widget: usize,
        part: ControlPart,
        position: f32,
    ) {
        self.emit(
            shell,
            CanvasEvent::SetControl {
                module,
                widget,
                part,
                position,
            },
        );
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
        text: Option<&str>,
        repeat: bool,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        use keyboard::key::Named;
        // Within the release window, an arrow keeps the group open and any
        // other key ends it. A modifier alone does not: Shift pressed before
        // the next arrow is part of that arrow.
        if let ArrowGroupView::Released { .. } = self.arrow_group {
            if is_arrow(key) {
                self.emit(shell, CanvasEvent::ArrowPressed { at: Instant::now() });
            } else if !is_modifier(key) {
                self.end_arrow_group(shell, false);
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
        let lost_to_mode = state
            .focus
            .control()
            .is_some_and(|(m, w)| Some(m) == selected && hidden(m, w));
        // A hidden or removed select is not drawn, so its list closes too, and
        // so does a field or grab whose control has gone.
        self.reconcile_focus(state);
        // An open list stays with its control whatever is selected, as the
        // pointer can open one on any tile. Once it closes, the next key
        // finds the focus stale.
        if state.focus.popup().is_none()
            && state.focus.control().is_some_and(|(m, w)| {
                Some(m) != selected
                    || !self
                        .scene
                        .module(m)
                        .and_then(|module| module.panel.widgets.get(w))
                        .is_some_and(|widget| widget.is_control() && !widget.hidden)
            })
        {
            state.focus.clear();
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
        // An open list or field comes before every other use of a key.
        if self.select_popup_key(state, key, modifiers, text, repeat, shell) {
            return true;
        }
        if self.typed_field_key(state, key, modifiers, text, repeat, shell) {
            return true;
        }
        if self.open_typed_field(state, key, modifiers, text, repeat) {
            shell.request_redraw();
            shell.capture_event();
            return true;
        }
        if self.range_keys(state, key, modifiers, repeat, shell) {
            shell.request_redraw();
            shell.capture_event();
            return true;
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
                if let Some((module, widget)) = state.focus.control() {
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
                        state.focus.set(module, controls[next], true);
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
                state.focus.clear();
                self.emit(shell, CanvasEvent::Select(vec![order[next].id]));
                true
            }
            keyboard::Key::Named(Named::Escape) if state.focus.control().is_some() => {
                state.focus.clear();
                // In MIDI learn mode, Escape also ends the mode, so the app
                // must see it too.
                self.learn.is_none()
            }
            keyboard::Key::Named(Named::Escape) if !self.selection.is_empty() && self.learn.is_none() => {
                self.emit(shell, CanvasEvent::Select(Vec::new()));
                true
            }
            keyboard::Key::Named(Named::Enter) if state.focus.control().is_none() => {
                let Some(module) = selected.and_then(|id| self.scene.module(id)) else {
                    return false;
                };
                let Some(widget) = next_focus(&module.panel, None, false) else {
                    return false;
                };
                state.focus.set(module.id, widget, true);
                true
            }
            // In MIDI learn mode the keyboard arms the focused control, like
            // a click, and never changes a value (midi-learn.md).
            // With nothing focused, Space still toggles Run below: that is
            // the transport, not a parameter.
            keyboard::Key::Named(Named::Space | Named::Enter)
                if self.learn.is_some() && state.focus.control().is_some() =>
            {
                if !repeat && let Some((module, widget)) = state.focus.control() {
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
                        self.arrow_step(shell);
                        self.emit(shell, CanvasEvent::Resize { module: id, size });
                        shell.request_redraw();
                        shell.capture_event();
                        return true;
                    }
                    return false;
                }
                let Some((module, widget)) = state.focus.control() else {
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
                    self.arrow_step(shell);
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
                if matches!(self.widget_kind(module, widget), Some(WidgetViewKind::XyPad { .. })) {
                    // Left and Right change `x`, Up and Down change `y`
                    // (panel-widgets.md, XY pad).
                    let part = if matches!(named, Named::ArrowLeft | Named::ArrowRight) {
                        ControlPart::Main
                    } else {
                        ControlPart::Second
                    };
                    let position = self.part_position(module, widget, part);
                    let next = knob_step(position, up, modifiers.shift(), 0);
                    self.arrow_step(shell);
                    if next != position {
                        self.set_part(shell, module, widget, part, next);
                    }
                } else if matches!(self.widget_kind(module, widget), Some(WidgetViewKind::Button)) {
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
                        // A parameter can declare its own step (panel-widgets.md,
                        // Number).
                        Some(WidgetViewKind::Number { key_step, .. }) if *key_step > 0.0 => {
                            let step = key_step * if modifiers.shift() { 0.1 } else { 1.0 };
                            (position + if up { step } else { -step }).clamp(0.0, 1.0)
                        }
                        Some(
                            WidgetViewKind::Knob { steps }
                            | WidgetViewKind::Fader { steps, .. }
                            | WidgetViewKind::Number { steps, .. },
                        ) => knob_step(position, up, modifiers.shift(), *steps),
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
                    self.arrow_step(shell);
                    if next != position {
                        self.set_control(shell, module, widget, next);
                    }
                }
                true
            }
            keyboard::Key::Named(named @ (Named::Space | Named::Enter)) => {
                if let Some((module, widget)) = state.focus.control() {
                    let position = self.position(module, widget);
                    let one_number = self
                        .widget_kind(module, widget)
                        .is_some_and(WidgetViewKind::is_one_number);
                    let two_numbers = self
                        .widget_kind(module, widget)
                        .is_some_and(WidgetViewKind::is_two_numbers);
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
                        _ if (one_number || two_numbers) && matches!(named, Named::Space) && !repeat => {
                            self.emit(shell, CanvasEvent::ToggleRun);
                        }
                        _ => {}
                    }
                    !(one_number && matches!(named, Named::Enter))
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
