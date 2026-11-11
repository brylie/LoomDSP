//! Pointer and keyboard events: what each press, drag, and key does.

use super::*;

impl CanvasModel<'_> {
    /// The first state for a still image: as if the user had framed the
    /// view, focused a control with Tab, moved the pointer, pinned a badge,
    /// and started a drag, in that order.
    pub(super) fn still_state(&self, still: Still, now: Instant) -> CanvasInput {
        let mut state = CanvasInput {
            camera: still.camera,
            // Already framed, so the first redraw keeps this camera.
            framed_key: Some(self.frame_key),
            level_key: Some(self.level_key),
            // Focus as after a Tab press, so the ring and the value show.
            focus: still.focus.map_or(focus::FocusState::None, |(module, widget)| {
                focus::FocusState::at(module, widget, true, focus::FocusMode::Browse)
            }),
            pinned: still.pinned,
            ..CanvasInput::default()
        };
        if let Some(world) = still.pointer {
            state.cursor = Some(world);
            state.hover = Some(self.scene.hit(world, state.hit_radius()));
        }
        state.interaction = match still.drag {
            Some(StillDrag::Wire { from, pulled }) => Interaction::Wiring { from, pulled },
            Some(StillDrag::Depth(module, widget)) => {
                let drag = self.value_drag(module, widget, state.camera.zoom);
                Interaction::Depth {
                    module,
                    widget,
                    part: ControlPart::Main,
                    drag,
                    last: still.pointer.map_or(0.0, |p| drag.along(state.camera.to_screen(p))),
                }
            }
            None => Interaction::Idle,
        };
        if still.hover_text {
            // Start the wait in the past, so the text is due now and shows
            // in the first frame. One frame cannot wait for the delay.
            state.hover_text.rest(self.hover_target(&state), now);
            state.hover_text.tick(now + crate::hover::HOVER_DELAY);
        }
        state
    }

    /// Handles one event: everything but hover text.
    pub(super) fn handle(
        &self,
        state: &mut CanvasInput,
        event: Option<&Event>,
        pointer: Option<Point>,
        bounds: Rect,
        now: Instant,
        out: &mut Reaction,
    ) {
        let over = pointer.is_some_and(|at| bounds.contains(at));
        let screen = relative(pointer, bounds);
        state.size = bounds.size();

        if self.focus_policy.matrix_escape
            && matches!(event, Some(Event::Key(KeyEvent::Pressed { key: Key::Escape, .. })))
            && self.pin(state).is_some()
        {
            state.pinned = None;
            out.take();
            out.redraw();
            return;
        }

        let keys_elsewhere = match event {
            Some(Event::Key(KeyEvent::Pressed { key, modifiers, .. })) => {
                (self.focus_policy.matrix_keyboard && !modifiers.shortcut())
                    || (self.focus_policy.text_field_keys
                        && (self.focus_policy.text_field_keeps_tab || !matches!(key, Key::Tab)))
            }
            _ => false,
        };
        if keys_elsewhere {
            self.leave_keys(state, out);
            return;
        }

        if !self.focus_policy.interactive {
            // Modifiers still count, so Shift held while the pop-up closes
            // is known on the next click.
            if let Some(Event::Key(KeyEvent::ModifiersChanged(modifiers))) = event {
                state.modifiers = *modifiers;
            }
            // A pop-up has the keys. Nothing may stay held or half dragged
            // behind it. This runs on any event, redraws too, because the
            // pop-up takes the pointer and key events before the canvas
            // sees them. Each step does nothing once nothing is held.
            self.release_pointer_hold(state, out);
            self.release_every_key_hold(state, out);
            self.end_gestures(state, out);
            self.end_edit_group(out, false);
            state.interaction = Interaction::Idle;
            state.focus.close_typing();
            // Only what the window itself reports goes on from here. `None`
            // is most often a window event with no meaning to the canvas,
            // so it goes on as far as the focus check below.
            if !matches!(event, Some(Event::Redraw | Event::WindowUnfocused) | None) {
                return;
            }
        }

        // A list, field, or grab whose control has gone is closed first, so
        // the event still does its normal work.
        self.reconcile_focus(state);
        // Something that means nothing to the canvas ends here.
        let Some(event) = event else {
            return;
        };
        // An open list comes before every other use of the pointer.
        if self.select_popup_event(state, event, pointer, bounds, now, out) {
            return;
        }
        if self.typed_field_event(state, event, bounds, screen, out) {
            return;
        }

        match event {
            Event::Key(KeyEvent::Pressed { key: Key::Escape, .. })
                if matches!(state.interaction, Interaction::Wiring { .. }) =>
            {
                self.end_gestures(state, out);
                self.end_edit_group(out, false);
                out.redraw();
                out.take();
            }
            Event::Redraw => {
                let new_frame = state.framed_key != Some(self.frame_key);
                if let EditGroupView::Open { due } = self.edit_group
                    && now >= due
                {
                    self.emit(out, CanvasEvent::EditGroupDue { at: now });
                }
                if state
                    .browser_key
                    .replace(self.browser_key)
                    .is_some_and(|key| key != self.browser_key)
                {
                    let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
                    self.open_browser(state.camera.to_world(centre), centre, bounds, out);
                }
                // The selection can change from outside the canvas: a
                // screen reader's Click on another tile, say. Focus lives
                // only on the one selected tile, so focus left on another
                // is dropped now, and what the canvas reports as focused
                // stays true. An open list stays with its control, as on a
                // key press.
                let same = state
                    .selection_seen
                    .as_ref()
                    .is_some_and(|seen| seen.iter().eq(self.selection.iter()));
                if !same {
                    let first = state.selection_seen.is_none();
                    state.selection_seen = Some(self.selection.iter().copied().collect());
                    let elsewhere = state
                        .focus
                        .control()
                        .is_some_and(|(module, _)| self.selection.len() != 1 || !self.selection.contains(&module));
                    if !first && elsewhere && state.focus.popup().is_none() {
                        state.focus.clear();
                        out.redraw();
                    }
                }
                // A focus request waits while something else has the
                // input: the app would drop the events it makes, and the
                // request would be used up. It is applied on the first
                // redraw after the canvas has the input again.
                let (focus_key, focus_control) = self.focus_request;
                if self.focus_policy.interactive
                    && state.focus_key.replace(focus_key).is_some_and(|key| key != focus_key)
                    && let Some((module, widget)) = focus_control
                {
                    let can_focus = self
                        .scene
                        .module(module)
                        .is_some_and(|tile| super::panel_input::focusable(&tile.panel).contains(&widget));
                    if can_focus {
                        // As a press on the control does: its tile is the
                        // one selected, so the keys reach it and the
                        // inspector follows.
                        self.select_for_edit(module, out);
                        state.focus.set(module, widget, true);
                        out.redraw();
                    }
                }
                let new_level = state.level_key != Some(self.level_key);
                if (new_frame || new_level) && bounds.width > 0.0 {
                    // A new patch frames as always. A new level takes the
                    // camera it had, and frames on its first visit.
                    match self.level_camera.filter(|_| !new_frame) {
                        Some(camera) => state.camera = camera,
                        None => self.frame(state, bounds.size()),
                    }
                    state.framed_key = Some(self.frame_key);
                    state.level_key = Some(self.level_key);
                    state.focus.clear();
                    // A new frame key means a new patch: its lights have to
                    // be reported again, even if the list looks the same.
                    state.reported_lights = None;
                    state.animation = None;
                    // Module IDs repeat between patches, so an old pin could
                    // land on an unrelated output.
                    state.pinned = None;
                    out.redraw();
                    self.emit(out, CanvasEvent::Camera(state.camera));
                }
                if self.animate {
                    let (start, base) = *state.animation.get_or_insert((now, state.camera));
                    let t = (now - start).as_secs_f32();
                    state.camera = animated(base, bounds.size(), t);
                    out.redraw();
                } else if let Some((_, base)) = state.animation.take() {
                    state.camera = base;
                    out.redraw();
                }
                let lights = visible_lights(self.scene, &state.camera, bounds.size());
                if state.reported_lights.as_ref() != Some(&lights) {
                    state.reported_lights = Some(lights.clone());
                    self.emit(out, CanvasEvent::VisibleDisplays(lights));
                }
                let full = detail(state.camera.zoom) == Detail::Close;
                if state.reported_full_traces.replace(full) != Some(full) {
                    self.emit(out, CanvasEvent::TraceDetail(full));
                }
            }
            Event::Key(KeyEvent::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Key(KeyEvent::Pressed {
                key,
                modifiers,
                repeat,
                text,
                ..
            }) if self.focus_keys(
                state,
                super::panel_input::KeyPress {
                    key,
                    modifiers: *modifiers,
                    text: text.as_deref(),
                    repeat: *repeat,
                },
                now,
                out,
            ) => {}
            Event::Key(KeyEvent::Released { key }) => {
                self.release_key(state, key, now, out);
            }
            // Paste works with the pointer anywhere in the window. Only the
            // canvas knows where the pointer is in the patch, and where the
            // view's centre is.
            Event::Key(KeyEvent::Pressed {
                key: Key::Character(c),
                modifiers,
                ..
            }) if modifiers.shortcut() && c.eq_ignore_ascii_case(&'v') => {
                let at = screen
                    .filter(|_| over)
                    .unwrap_or(Point::new(bounds.width / 2.0, bounds.height / 2.0));
                self.emit(
                    out,
                    CanvasEvent::Paste {
                        world: state.camera.to_world(at),
                    },
                );
                out.take();
            }
            Event::Key(KeyEvent::Pressed { key, modifiers, .. }) if over => {
                match *key {
                    Key::Delete | Key::Backspace if !self.selection.is_empty() => {
                        self.emit(out, CanvasEvent::Delete(self.selection.iter().copied().collect()));
                        out.take();
                    }
                    Key::Home | Key::Character('f') => {
                        if let Some(world) = self.scene.bounds() {
                            state.camera.frame_readable(world, bounds.size(), READABLE_ZOOM);
                            out.redraw();
                            self.emit(out, CanvasEvent::Camera(state.camera));
                        }
                        out.take();
                    }
                    // Cmd/Ctrl+0 resets the zoom to 100%, about the pointer
                    // like the wheel, so what the user points at stays put.
                    Key::Character('0') if modifiers.shortcut() => {
                        let at = screen.unwrap_or(Point::new(bounds.width / 2.0, bounds.height / 2.0));
                        state.camera.zoom_to(at, 1.0);
                        out.redraw();
                        self.emit(out, CanvasEvent::Camera(state.camera));
                        out.take();
                    }
                    Key::Character('a') if modifiers.shortcut() => {
                        let all = self.scene.modules().iter().map(|m| m.id).collect();
                        self.emit(out, CanvasEvent::Select(all));
                        out.take();
                    }
                    Key::Character('a') if modifiers.none() => {
                        if let Some(screen) = screen {
                            self.open_browser(state.camera.to_world(screen), screen, bounds, out);
                        }
                        out.take();
                    }
                    // The menu key, or Shift+F10, opens the tile menu, as a
                    // right-click on the tile does (ui-layer.md, section 6,
                    // rule 8).
                    Key::ContextMenu if modifiers.none() => {
                        self.open_tile_menu_by_key(state, bounds, out);
                        out.take();
                    }
                    Key::F10
                        if *modifiers
                            == (Modifiers {
                                shift: true,
                                ..Modifiers::NONE
                            }) =>
                    {
                        self.open_tile_menu_by_key(state, bounds, out);
                        out.take();
                    }
                    _ => {}
                }
            }
            Event::Pointer(PointerEvent::Scrolled(delta)) if over => {
                let Some(screen) = screen else { return };
                match *delta {
                    // A mouse wheel zooms.
                    ScrollDelta::Lines { y, .. } => {
                        state.camera.zoom_at(screen, 1.15_f32.powf(y));
                    }
                    // A trackpad pans, or zooms with Ctrl or Cmd held.
                    ScrollDelta::Pixels { x, y } => {
                        if state.modifiers.shortcut() || state.modifiers.control {
                            state.camera.zoom_at(screen, (y * 0.01).exp());
                        } else {
                            state.camera.pan(Vector::new(x, y));
                        }
                    }
                }
                state.cursor = Some(state.camera.to_world(screen));
                // The view moved under the pointer: hover text hides, and
                // shows again only after the pointer rests anew.
                state.hover_text.restart(now);
                state.hover = Some(self.scene.hit(state.camera.to_world(screen), state.hit_radius()));
                // Each wheel event is one step of a zoom or a trackpad pan,
                // and nothing marks its end, so each one reports the view.
                self.emit(out, CanvasEvent::Camera(state.camera));
                out.redraw();
                out.take();
            }
            // A press elsewhere in the window, such as an inspector field,
            // takes the keys from the canvas, so a digit never opens a field
            // on a control the player has left.
            Event::Pointer(PointerEvent::Pressed(_)) if !over => {
                state.focus.clear();
            }
            Event::Pointer(PointerEvent::Pressed(button)) if over => {
                let Some(screen) = screen else { return };
                if self.focus_policy.matrix_keyboard {
                    self.emit(out, CanvasEvent::PointerDown);
                }
                // A second button must end a hold. Replacing the interaction
                // without a release would leave a momentary gate high.
                self.release_pointer_hold(state, out);
                self.end_gestures(state, out);
                self.end_edit_group(out, false);
                let world = state.camera.to_world(screen);
                match button {
                    PointerButton::Middle => {
                        state.interaction = Interaction::Panning {
                            last: screen,
                            click: None,
                            start: state.camera,
                        };
                    }
                    PointerButton::Right => {
                        state.interaction = Interaction::Panning {
                            last: screen,
                            click: Some(screen),
                            start: state.camera,
                        };
                    }
                    PointerButton::Left => {
                        self.press(state, world, screen, bounds, now, out);
                    }
                    _ => {}
                }
                out.redraw();
                out.take();
            }
            Event::Pointer(PointerEvent::Moved) => {
                state.focus_first = false;
                let Some(screen) = screen else { return };
                let world = state.camera.to_world(screen);
                state.cursor = Some(world);
                let hover = over.then(|| self.scene.hit(world, state.hit_radius()));
                let mut redraw = false;
                match &mut state.interaction {
                    Interaction::Panning { last, click, .. } => {
                        let delta = screen - *last;
                        *last = screen;
                        if click.is_some_and(|start| start.distance(screen) > CLICK_SLOP) {
                            *click = None;
                        }
                        state.camera.pan(delta);
                        redraw = true;
                    }
                    Interaction::Moving {
                        ids, start, applied, ..
                    } => {
                        let total = world - *start;
                        let snapped = Vector::new((total.x / GRID).round() * GRID, (total.y / GRID).round() * GRID);
                        if snapped != *applied {
                            let delta = snapped - *applied;
                            *applied = snapped;
                            let ids = ids.clone();
                            self.emit(out, CanvasEvent::Move { ids, delta });
                        }
                    }
                    Interaction::Resizing {
                        start,
                        original,
                        current,
                        minimum,
                        ..
                    } => {
                        let delta = world - *start;
                        let snap = |value: f32, min: f32| {
                            (value / GRID).round().clamp(min / GRID, (64.0_f32).max(min / GRID)) * GRID
                        };
                        let next = Size::new(
                            snap(original.width + delta.x, minimum.width),
                            snap(original.height + delta.y, minimum.height),
                        );
                        if *current != next {
                            *current = next;
                            redraw = true;
                        }
                    }
                    Interaction::Turning {
                        module,
                        widget,
                        drag,
                        last,
                        position,
                    } => {
                        let along = drag.along(screen);
                        let next = drag.moved(*position, along - *last, state.modifiers.shift);
                        *last = along;
                        if next != *position {
                            *position = next;
                            let event = CanvasEvent::SetControl {
                                module: *module,
                                widget: *widget,
                                part: ControlPart::Main,
                                position: next,
                            };
                            self.emit(out, event);
                        }
                    }
                    Interaction::Pad {
                        module,
                        widget,
                        throw,
                        last,
                        position,
                    } => {
                        let scale = if state.modifiers.shift { 0.1 } else { 1.0 } / *throw;
                        let moved = screen - *last;
                        *last = screen;
                        let next = (
                            (position.0 + moved.x * scale).clamp(0.0, 1.0),
                            (position.1 - moved.y * scale).clamp(0.0, 1.0),
                        );
                        let (module, widget, before) = (*module, *widget, *position);
                        *position = next;
                        self.set_pad(out, module, widget, before, next);
                    }
                    Interaction::Range {
                        module,
                        widget,
                        grab,
                        drag,
                        last,
                        position,
                    } => {
                        let along = drag.along(screen);
                        let delta = drag.delta(along - *last, state.modifiers.shift);
                        *last = along;
                        let (next_grab, low, high) = grab.moved(position.0, position.1, delta);
                        *grab = next_grab;
                        let (module, widget, before) = (*module, *widget, *position);
                        *position = (low, high);
                        self.set_pad(out, module, widget, before, (low, high));
                    }
                    Interaction::Depth {
                        module,
                        widget,
                        part,
                        drag,
                        last,
                    } => {
                        let along = drag.along(screen);
                        let moved = along - *last;
                        *last = along;
                        // The control's own drag scale: the way that raises
                        // the value raises the depth.
                        let delta = drag.delta(moved, state.modifiers.shift);
                        if delta != 0.0 {
                            let event = CanvasEvent::DragDepth {
                                module: *module,
                                widget: *widget,
                                part: *part,
                                pinned: self.pin(state),
                                delta,
                            };
                            self.emit(out, event);
                        }
                    }
                    Interaction::Holding { .. } | Interaction::Idle => {}
                    Interaction::Wiring { from, pulled } => {
                        if from.output && !*pulled && !state.outside_window {
                            let at = match hover {
                                Some(Hit::Port(target)) if target.output != from.output => self
                                    .scene
                                    .port_position(target)
                                    .map(|world| state.camera.to_screen(world))
                                    .map(|point| Point::new(bounds.x + point.x, bounds.y + point.y)),
                                _ => None,
                            }
                            .unwrap_or(Point::new(bounds.x + screen.x, bounds.y + screen.y));
                            self.assign_drag(*from, Some(at), state.camera, bounds, out);
                        }
                        redraw = true;
                    }
                    Interaction::Selecting { current, .. } => {
                        *current = world;
                        redraw = true;
                    }
                }
                if hover != state.hover {
                    state.hover = hover;
                    redraw = true;
                }
                if redraw {
                    out.redraw();
                }
            }
            Event::Pointer(PointerEvent::Released(button)) => {
                // The left button is what started the hold. Releasing another
                // button, one that was already down, must not end it.
                if *button != PointerButton::Left && matches!(state.interaction, Interaction::Holding { .. }) {
                    return;
                }
                // Only the right button's own release ends a right click. A
                // left button let go meanwhile must not open the menu.
                if *button != PointerButton::Right
                    && matches!(state.interaction, Interaction::Panning { click: Some(_), .. })
                {
                    return;
                }
                let interaction = std::mem::take(&mut state.interaction);
                let gesture = matches!(
                    interaction,
                    Interaction::Moving { .. }
                        | Interaction::Resizing { .. }
                        | Interaction::Turning { .. }
                        | Interaction::Pad { .. }
                        | Interaction::Range { .. }
                        | Interaction::Wiring { .. }
                        | Interaction::Depth { .. }
                );
                let world = screen.map(|s| state.camera.to_world(s));
                match interaction {
                    Interaction::Resizing {
                        module,
                        original,
                        current,
                        ..
                    } => {
                        if current != original {
                            self.emit(
                                out,
                                CanvasEvent::Resize {
                                    module,
                                    size: ((current.width / GRID) as u8, (current.height / GRID) as u8),
                                },
                            );
                        }
                    }
                    Interaction::Wiring { from, pulled } => {
                        let mut handled = false;
                        match world.filter(|_| over).map(|w| self.scene.hit(w, state.hit_radius())) {
                            Some(Hit::Port(to)) => {
                                let pair = if from.output { (from, to) } else { (to, from) };
                                if pair.0.output && !pair.1.output {
                                    self.emit(
                                        out,
                                        CanvasEvent::Connect {
                                            from: pair.0,
                                            to: pair.1,
                                        },
                                    );
                                    handled = true;
                                }
                            }
                            Some(Hit::Widget { module, widget })
                                if !pulled && self.takes_assignment(from, module, widget) =>
                            {
                                self.emit(
                                    out,
                                    CanvasEvent::Assign {
                                        from,
                                        module,
                                        widget,
                                        part: world.map_or(ControlPart::Main, |w| self.drop_part(module, widget, w)),
                                    },
                                );
                                handled = true;
                            }
                            _ => {}
                        }
                        if from.output && !pulled {
                            if handled
                                || screen.is_some_and(|p| bounds.contains(Point::new(bounds.x + p.x, bounds.y + p.y)))
                            {
                                self.emit(out, CanvasEvent::AssignCancel);
                            } else {
                                self.emit(
                                    out,
                                    CanvasEvent::AssignDrop {
                                        from,
                                        at: if state.outside_window { None } else { pointer },
                                    },
                                );
                            }
                        }
                    }
                    Interaction::Selecting { start, current, base } => {
                        let area = Rect::new(
                            Point::new(start.x.min(current.x), start.y.min(current.y)),
                            Size::new((start.x - current.x).abs(), (start.y - current.y).abs()),
                        );
                        let mut selection = base;
                        selection.extend(self.scene.modules_in(area));
                        self.emit(out, CanvasEvent::Select(selection.into_iter().collect()));
                    }
                    Interaction::Moving {
                        applied,
                        click: Some(action),
                        ..
                    } if applied == Vector::ZERO => match action {
                        ChoiceClick::Set(module, widget, position) => self.set_control(out, module, widget, position),
                        ChoiceClick::Open(module, widget) => self.open_select(state, module, widget),
                        ChoiceClick::ClearClip(module, widget) => {
                            self.emit(out, CanvasEvent::ClearClip { module, widget });
                        }
                    },
                    Interaction::Holding { module, widget } => {
                        if !Self::key_holds_button(state, module, widget) {
                            self.emit(out, CanvasEvent::ButtonReleased { module, widget });
                        }
                    }
                    Interaction::Panning { click: Some(_), .. } => {
                        if let (Some(world), Some(screen)) = (world, screen) {
                            self.open_probe_menu(state, world, screen, bounds, out);
                        }
                    }
                    // The pan is over: the app learns the view once, not on
                    // every move, and not at all if the view is where it was.
                    Interaction::Panning { click: None, start, .. } => {
                        if state.camera != start {
                            self.emit(out, CanvasEvent::Camera(state.camera));
                        }
                    }
                    Interaction::Idle
                    | Interaction::Moving { .. }
                    | Interaction::Turning { .. }
                    | Interaction::Pad { .. }
                    | Interaction::Range { .. }
                    | Interaction::Depth { .. } => {}
                }
                if gesture {
                    self.emit(out, CanvasEvent::GestureEnd);
                }
                out.redraw();
            }
            Event::Pointer(PointerEvent::LeftWindow) => {
                state.outside_window = true;
                if let Interaction::Wiring { from, pulled: false } = state.interaction
                    && from.output
                {
                    self.assign_drag(from, None, state.camera, bounds, out);
                }
                self.release_pointer_hold(state, out);
                if state.hover.take().is_some() {
                    out.redraw();
                }
            }
            Event::Pointer(PointerEvent::EnteredWindow) => {
                state.outside_window = false;
                if let Interaction::Wiring { from, pulled: false } = state.interaction
                    && from.output
                {
                    self.assign_drag(from, pointer, state.camera, bounds, out);
                }
            }
            Event::WindowUnfocused => {
                // A key release never arrives if the window loses focus
                // while Space or Enter is held. End every hold here.
                self.release_pointer_hold(state, out);
                self.release_every_key_hold(state, out);
                self.end_gestures(state, out);
                // No key-up follows, so a group this view does not show yet
                // must end too.
                self.end_edit_group(out, true);
                out.redraw();
            }
            _ => {}
        }
    }

    pub(super) fn press(
        &self,
        state: &mut CanvasInput,
        world: Point,
        screen: Point,
        bounds: Rect,
        now: Instant,
        out: &mut Reaction,
    ) {
        if let Some(tile) = self.resize_at(world) {
            self.select_for_edit(tile.id, out);
            state.focus.clear();
            self.begin(
                state,
                Interaction::Resizing {
                    module: tile.id,
                    start: world,
                    original: tile.bounds().size(),
                    current: tile.bounds().size(),
                    minimum: tile.minimum_size(),
                },
                out,
            );
            return;
        }
        let shift = state.modifiers.shift;
        let hit = self.scene.hit(world, state.hit_radius());
        // A press ends the keyboard's inside of a composite widget and any
        // grab (panel-widgets.md, rule 4).
        state.focus.pressed();
        state.focus_first = false;
        // The ring of a modulated knob reaches a little past the knob's
        // cell, so it is found before the hit test's answer is used.
        if let Some((module, widget, part)) = self.depth_drag_at(&hit, world) {
            // The selection stays as it is: a selected source module is one
            // way the drag picks its assignment (modulation.md, Arc drag).
            let drag = self.depth_scale(module, widget, part, state.camera.zoom);
            self.begin(
                state,
                Interaction::Depth {
                    module,
                    widget,
                    part,
                    drag,
                    last: drag.along(screen),
                },
                out,
            );
            return;
        }
        if !matches!(hit, Hit::Widget { .. }) {
            state.focus.clear();
        }
        match hit {
            Hit::Widget { module, widget } if self.learn.is_some() && self.is_control(module, widget) => {
                self.select_for_edit(module, out);
                // The clicked control takes the keyboard focus, so Space or
                // Enter acts on it next, not on a control focused earlier.
                state.focus.set(module, widget, false);
                self.emit(out, CanvasEvent::Learn { module, widget });
            }
            Hit::Badge(port) => {
                // A click pins this output's lines, or unpins them. Only one
                // output is pinned at a time.
                state.pinned = if state.pinned == Some(port) { None } else { Some(port) };
            }
            Hit::Widget { module, widget } => {
                self.select_for_edit(module, out);
                state.focus.set(module, widget, false);
                // Where a double-click resets, and so what it must land on
                // twice: `low` then `high` is not a double-click.
                let reset = self.reset_part(module, widget, world);
                let double = reset.is_some_and(|part| {
                    state
                        .last_click
                        .is_some_and(|(at, m, w, p)| m == module && w == widget && p == part && now - at < DOUBLE_CLICK)
                });
                // A button's second press is another press. Other controls
                // reset to the default (panels.md, Interaction).
                let button = matches!(self.widget_kind(module, widget), Some(WidgetViewKind::Button));
                if double && !button {
                    state.last_click = None;
                    self.emit(
                        out,
                        CanvasEvent::ResetControl {
                            module,
                            widget,
                            part: reset.unwrap_or_default(),
                        },
                    );
                    return;
                }
                state.last_click = reset.map(|part| (now, module, widget, part));
                if button {
                    self.press_button(state, module, widget, out);
                    return;
                }
                let position = self.position(module, widget);
                let click = match self.widget_kind(module, widget) {
                    Some(WidgetViewKind::Range { vertical, .. }) => {
                        let vertical = *vertical;
                        let bounds = self.scene.module(module).and_then(|m| m.widget_bounds(widget));
                        let high = self.part_position(module, widget, ControlPart::Second);
                        let grab = bounds.and_then(|rect| RangeGrab::at(rect, vertical, position, high, world));
                        if let (Some(grab), Some(rect)) = (grab, bounds) {
                            let drag = ValueDrag::range(rect, vertical, state.camera.zoom);
                            self.begin(
                                state,
                                Interaction::Range {
                                    module,
                                    widget,
                                    grab,
                                    drag,
                                    last: drag.along(screen),
                                    position: (position, high),
                                },
                                out,
                            );
                            return;
                        }
                        // A press on the track outside the band moves the tile,
                        // and never jumps a handle (panel-widgets.md, rule 2).
                        None
                    }
                    Some(WidgetViewKind::XyPad { .. }) => {
                        let Some(area) = self
                            .scene
                            .module(module)
                            .and_then(|m| m.widget_bounds(widget))
                            .map(xy_area)
                        else {
                            return;
                        };
                        let before = (position, self.part_position(module, widget, ControlPart::Second));
                        // Only with Alt held does a press move the point
                        // (panel-widgets.md, rule 2).
                        let jump = state.modifiers.alt.then(|| xy_position(area, world));
                        self.begin(
                            state,
                            Interaction::Pad {
                                module,
                                widget,
                                throw: xy_throw(area, state.camera.zoom),
                                last: screen,
                                position: jump.unwrap_or(before),
                            },
                            out,
                        );
                        if let Some(jump) = jump {
                            self.set_pad(out, module, widget, before, jump);
                        }
                        return;
                    }
                    Some(kind) if kind.is_one_number() => {
                        let drag = self.value_drag(module, widget, state.camera.zoom);
                        self.begin(
                            state,
                            Interaction::Turning {
                                module,
                                widget,
                                drag,
                                last: drag.along(screen),
                                position,
                            },
                            out,
                        );
                        return;
                    }
                    Some(WidgetViewKind::Toggle) => {
                        Some(ChoiceClick::Set(module, widget, if position > 0.5 { 0.0 } else { 1.0 }))
                    }
                    Some(WidgetViewKind::Radio { options }) => self
                        .scene
                        .module(module)
                        .and_then(|m| m.widget_bounds(widget))
                        .and_then(|rect| radio_option(rect, options.len(), world))
                        .map(|option| ChoiceClick::Set(module, widget, option as f32)),
                    Some(WidgetViewKind::Segmented { options, .. }) => self
                        .scene
                        .module(module)
                        .and_then(|m| m.widget_bounds(widget))
                        .and_then(|rect| crate::controls::segment_at(rect, options.len(), world))
                        .map(|option| ChoiceClick::Set(module, widget, option as f32)),
                    Some(WidgetViewKind::Select { .. }) => Some(ChoiceClick::Open(module, widget)),
                    _ => None,
                };
                // Choice controls act on release, so a drag that starts on
                // them still moves the tile.
                self.begin(
                    state,
                    Interaction::Moving {
                        ids: vec![module],
                        start: world,
                        applied: Vector::ZERO,
                        click,
                    },
                    out,
                );
            }
            Hit::Port(port) if !port.output => {
                // Grabbing a connected input picks up its cable, like pulling
                // a patch cable out of a jack.
                let source = self
                    .scene
                    .cables()
                    .iter()
                    .find(|c| c.to == (port.module, port.index))
                    .map(|c| PortRef {
                        module: c.from.0,
                        output: true,
                        index: c.from.1,
                    });
                if let Some(source) = source {
                    // Pulling a cable out and dropping it elsewhere is one
                    // step, so the gesture starts before the disconnect.
                    self.emit(out, CanvasEvent::GestureStart);
                    self.emit(out, CanvasEvent::Disconnect(port));
                    state.interaction = Interaction::Wiring {
                        from: source,
                        pulled: true,
                    };
                } else {
                    self.begin(
                        state,
                        Interaction::Wiring {
                            from: port,
                            pulled: false,
                        },
                        out,
                    );
                }
            }
            Hit::Port(port) => {
                if port.output {
                    state.outside_window = false;
                    self.assign_drag(
                        port,
                        Some(Point::new(bounds.x + screen.x, bounds.y + screen.y)),
                        state.camera,
                        bounds,
                        out,
                    );
                }
                self.begin(
                    state,
                    Interaction::Wiring {
                        from: port,
                        pulled: false,
                    },
                    out,
                );
            }
            Hit::Module(id) => {
                let mut selection = self.selection.clone();
                if shift {
                    if !selection.remove(&id) {
                        selection.insert(id);
                    }
                } else if !selection.contains(&id) {
                    selection = BTreeSet::from([id]);
                }
                if &selection != self.selection {
                    self.emit(out, CanvasEvent::Select(selection.iter().copied().collect()));
                }
                // A second press on the header opens the tile. No move
                // begins, so nothing is left dragging after the release
                // (sub-patches.md, Opening a sub-patch).
                let on_header = self
                    .scene
                    .module(id)
                    .is_some_and(|m| world.y >= m.position.y && world.y < m.position.y + HEADER);
                let last = state.last_header_click.take();
                if on_header {
                    let again =
                        last.is_some_and(|(at, m, point)| m == id && double_click(Some((at, point)), screen, now));
                    if again {
                        self.emit(out, CanvasEvent::OpenTile(id));
                        return;
                    }
                    state.last_header_click = Some((now, id, screen));
                }
                // A meter's clip light clears on release, so a drag from it
                // moves the tile and leaves it lit. A second press on the
                // meter resets it. Far out neither is drawn, so a press only
                // selects (panels.md, Meter widget).
                let module = self
                    .scene
                    .module(id)
                    .filter(|_| detail(state.camera.zoom) != Detail::Far);
                let meter = module.and_then(|m| m.clip_meter_at(world));
                let last = state.last_meter_click.take();
                let again = meter.filter(|&widget| {
                    last.is_some_and(|(at, m, w, point)| {
                        m == id && w == widget && double_click(Some((at, point)), screen, now)
                    })
                });
                let click = if let Some(widget) = again {
                    self.emit(out, CanvasEvent::ResetMeter { module: id, widget });
                    None
                } else {
                    state.last_meter_click = meter.map(|widget| (now, id, widget, screen));
                    module
                        .and_then(|m| m.clip_light_at(world))
                        .map(|widget| ChoiceClick::ClearClip(id, widget))
                };
                if selection.contains(&id) {
                    self.begin(
                        state,
                        Interaction::Moving {
                            ids: selection.into_iter().collect(),
                            start: world,
                            applied: Vector::ZERO,
                            click,
                        },
                        out,
                    );
                }
            }
            Hit::Nothing if double_click(state.last_empty_click.take(), screen, now) => {
                self.open_browser(world, screen, bounds, out);
            }
            Hit::Cable(_) | Hit::Nothing => {
                if matches!(hit, Hit::Nothing) {
                    state.last_empty_click = Some((now, screen));
                }
                let base = if shift { self.selection.clone() } else { BTreeSet::new() };
                state.interaction = Interaction::Selecting {
                    start: world,
                    current: world,
                    base,
                };
            }
        }
    }

    /// Frames the whole scene at a readable zoom. A scene with no modules
    /// keeps the camera it has.
    fn frame(&self, state: &mut CanvasInput, size: Size) {
        if let Some(world) = self.scene.bounds() {
            state.camera.frame_readable(world, size, READABLE_ZOOM);
        }
    }

    /// Opens the menu of the tile the keys are on, at the tile's top-left
    /// corner: the tile of the focused control, or else the one selected
    /// tile. With no such tile it does nothing.
    pub(super) fn open_tile_menu_by_key(&self, state: &CanvasInput, bounds: Rect, out: &mut Reaction) {
        let module = state.focus.control().map(|(module, _)| module).or_else(|| {
            (self.selection.len() == 1)
                .then(|| self.selection.iter().next().copied())
                .flatten()
        });
        let Some(tile) = module.and_then(|module| self.scene.module(module)) else {
            return;
        };
        let corner = state.camera.to_screen(tile.position);
        self.emit(
            out,
            CanvasEvent::TileMenu {
                module: tile.id,
                at: Point::new(bounds.x + corner.x, bounds.y + corner.y),
                area: bounds,
            },
        );
    }

    /// Sends [`CanvasEvent::OpenBrowser`] for a point given in world and
    /// canvas-relative screen coordinates.
    pub(super) fn open_browser(&self, world: Point, screen: Point, bounds: Rect, out: &mut Reaction) {
        self.emit(
            out,
            CanvasEvent::OpenBrowser {
                world,
                at: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                area: bounds,
            },
        );
    }

    /// Sends [`CanvasEvent::ProbeMenu`] if an output or a cable is at this
    /// point. Anything else opens no menu.
    pub(super) fn open_probe_menu(
        &self,
        state: &CanvasInput,
        world: Point,
        screen: Point,
        bounds: Rect,
        out: &mut Reaction,
    ) {
        let (output, anchor) = match self.scene.hit(world, state.hit_radius()) {
            Hit::Port(port) if port.output => match self.scene.port_position(port) {
                Some(position) => (port, position),
                None => return,
            },
            Hit::Cable(i) => match self.scene.cables().get(i) {
                Some(cable) => (
                    PortRef {
                        module: cable.from.0,
                        output: true,
                        index: cable.from.1,
                    },
                    world,
                ),
                None => return,
            },
            Hit::Module(module) => {
                self.emit(
                    out,
                    CanvasEvent::TileMenu {
                        module,
                        at: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                        area: bounds,
                    },
                );
                return;
            }
            _ => return,
        };
        self.emit(
            out,
            CanvasEvent::ProbeMenu {
                output,
                world: anchor,
                at: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                area: bounds,
            },
        );
    }
}
