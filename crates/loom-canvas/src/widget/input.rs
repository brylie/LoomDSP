//! Pointer and keyboard events: what each press, drag, and key does.

use super::*;

impl<Message> PatchCanvas<'_, Message> {
    /// The first state for a still image: as if the user had framed the
    /// view, focused a control with Tab, moved the pointer, pinned a badge,
    /// and started a drag, in that order.
    pub(super) fn still_state(&self, still: Still) -> State {
        let mut state = State {
            camera: still.camera,
            // Already framed, so the first redraw keeps this camera.
            framed_key: Some(self.frame_key),
            level_key: Some(self.level_key),
            // Focus as after a Tab press, so the ring and the value show.
            focus: still.focus.map_or(focus::FocusState::None, |(module, widget)| {
                focus::FocusState::at(module, widget, true, focus::FocusMode::Browse)
            }),
            pinned: still.pinned,
            ..State::default()
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
            let now = Instant::now();
            state.hover_text.rest(self.hover_target(&state), now);
            state.hover_text.tick(now + crate::hover::HOVER_DELAY);
        }
        state
    }

    /// Handles one event: everything but hover text.
    pub(super) fn handle(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
    ) {
        let over = cursor.is_over(bounds);
        let screen = relative(cursor, bounds);
        state.size = bounds.size();

        if self.focus_policy.matrix_escape
            && matches!(
                event,
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                })
            )
            && self.pin(state).is_some()
        {
            state.pinned = None;
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        let keys_elsewhere = match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                (self.focus_policy.matrix_keyboard && !modifiers.command())
                    || (self.focus_policy.text_field_keys
                        && (self.focus_policy.text_field_keeps_tab
                            || !matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab))))
            }
            _ => false,
        };
        if keys_elsewhere {
            self.leave_keys(state, shell);
            return;
        }

        if !self.focus_policy.interactive {
            // Modifiers still count, so Shift held while the pop-up closes
            // is known on the next click.
            if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
                state.modifiers = *modifiers;
            }
            // A pop-up has the keys. Nothing may stay held or half dragged
            // behind it. This runs on any event, redraws too, because the
            // pop-up takes the pointer and key events before the canvas
            // sees them. Each step does nothing once nothing is held.
            self.release_pointer_hold(state, shell);
            self.release_every_key_hold(state, shell);
            self.end_gestures(state, shell);
            self.end_arrow_group(shell, false);
            state.interaction = Interaction::Idle;
            state.focus.close_typing();
            if !matches!(event, Event::Window(_)) {
                return;
            }
        }

        // A list, field, or grab whose control has gone is closed first, so
        // the event still does its normal work.
        self.reconcile_focus(state);
        // An open list comes before every other use of the pointer.
        if self.select_popup_event(state, event, bounds, over, screen, shell) {
            return;
        }
        if self.typed_field_event(state, event, bounds, screen, shell) {
            return;
        }

        match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) if matches!(state.interaction, Interaction::Wiring { .. }) => {
                self.end_gestures(state, shell);
                self.end_arrow_group(shell, false);
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                let new_frame = state.framed_key != Some(self.frame_key);
                if let ArrowGroupView::Released { due } = self.arrow_group
                    && *now >= due
                {
                    self.emit(shell, CanvasEvent::ArrowGroupDue { at: *now });
                }
                if state
                    .browser_key
                    .replace(self.browser_key)
                    .is_some_and(|key| key != self.browser_key)
                {
                    let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
                    self.open_browser(state.camera.to_world(centre), centre, bounds, shell);
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
                    shell.request_redraw();
                    self.emit(shell, CanvasEvent::Camera(state.camera));
                }
                if self.animate {
                    let (start, base) = *state.animation.get_or_insert((*now, state.camera));
                    let t = (*now - start).as_secs_f32();
                    state.camera = animated(base, bounds.size(), t);
                    shell.request_redraw();
                } else if let Some((_, base)) = state.animation.take() {
                    state.camera = base;
                    shell.request_redraw();
                }
                let lights = visible_lights(self.scene, &state.camera, bounds.size());
                if state.reported_lights.as_ref() != Some(&lights) {
                    state.reported_lights = Some(lights.clone());
                    self.emit(shell, CanvasEvent::VisibleDisplays(lights));
                }
                let full = detail(state.camera.zoom) == Detail::Close;
                if state.reported_full_traces.replace(full) != Some(full) {
                    self.emit(shell, CanvasEvent::TraceDetail(full));
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat,
                text,
                ..
            }) if self.focus_keys(state, key, *modifiers, text.as_deref(), *repeat, shell) => {}
            Event::Keyboard(keyboard::Event::KeyReleased { key, .. }) => {
                self.release_key(state, key, shell);
            }
            // Paste works with the pointer anywhere in the window. Only the
            // canvas knows where the pointer is in the patch, and where the
            // view's centre is.
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(c),
                modifiers,
                ..
            }) if modifiers.command() && c.eq_ignore_ascii_case("v") => {
                let at = screen
                    .filter(|_| over)
                    .unwrap_or(Point::new(bounds.width / 2.0, bounds.height / 2.0));
                self.emit(
                    shell,
                    CanvasEvent::Paste {
                        world: state.camera.to_world(at),
                    },
                );
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) if over => {
                use keyboard::key::Named;
                match key.as_ref() {
                    keyboard::Key::Named(Named::Delete | Named::Backspace) if !self.selection.is_empty() => {
                        self.emit(shell, CanvasEvent::Delete(self.selection.iter().copied().collect()));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Home) | keyboard::Key::Character("f") => {
                        if let Some(world) = self.scene.bounds() {
                            state.camera.frame_readable(world, bounds.size(), READABLE_ZOOM);
                            shell.request_redraw();
                            self.emit(shell, CanvasEvent::Camera(state.camera));
                        }
                        shell.capture_event();
                    }
                    // Cmd/Ctrl+0 resets the zoom to 100%, about the pointer
                    // like the wheel, so what the user points at stays put.
                    keyboard::Key::Character("0") if modifiers.command() => {
                        let at = screen.unwrap_or(Point::new(bounds.width / 2.0, bounds.height / 2.0));
                        state.camera.zoom_to(at, 1.0);
                        shell.request_redraw();
                        self.emit(shell, CanvasEvent::Camera(state.camera));
                        shell.capture_event();
                    }
                    keyboard::Key::Character("a") if modifiers.command() => {
                        let all = self.scene.modules().iter().map(|m| m.id).collect();
                        self.emit(shell, CanvasEvent::Select(all));
                        shell.capture_event();
                    }
                    keyboard::Key::Character("a") if modifiers.is_empty() => {
                        if let Some(screen) = screen {
                            self.open_browser(state.camera.to_world(screen), screen, bounds, shell);
                        }
                        shell.capture_event();
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if over => {
                let Some(screen) = screen else { return };
                match *delta {
                    // A mouse wheel zooms.
                    mouse::ScrollDelta::Lines { y, .. } => {
                        state.camera.zoom_at(screen, 1.15_f32.powf(y));
                    }
                    // A trackpad pans, or zooms with Ctrl or Cmd held.
                    mouse::ScrollDelta::Pixels { x, y } => {
                        if state.modifiers.command() || state.modifiers.control() {
                            state.camera.zoom_at(screen, (y * 0.01).exp());
                        } else {
                            state.camera.pan(Vector::new(x, y));
                        }
                    }
                }
                state.cursor = Some(state.camera.to_world(screen));
                // The view moved under the pointer: hover text hides, and
                // shows again only after the pointer rests anew.
                state.hover_text.restart(Instant::now());
                state.hover = Some(self.scene.hit(state.camera.to_world(screen), state.hit_radius()));
                // Each wheel event is one step of a zoom or a trackpad pan,
                // and nothing marks its end, so each one reports the view.
                self.emit(shell, CanvasEvent::Camera(state.camera));
                shell.request_redraw();
                shell.capture_event();
            }
            // A press elsewhere in the window, such as an inspector field,
            // takes the keys from the canvas, so a digit never opens a field
            // on a control the player has left.
            Event::Mouse(mouse::Event::ButtonPressed(_)) if !over => {
                state.focus.clear();
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) if over => {
                let Some(screen) = screen else { return };
                if self.focus_policy.matrix_keyboard {
                    self.emit(shell, CanvasEvent::PointerDown);
                }
                // A second button must end a hold. Replacing the interaction
                // without a release would leave a momentary gate high.
                self.release_pointer_hold(state, shell);
                self.end_gestures(state, shell);
                self.end_arrow_group(shell, false);
                let world = state.camera.to_world(screen);
                match button {
                    mouse::Button::Middle => {
                        state.interaction = Interaction::Panning {
                            last: screen,
                            click: None,
                            start: state.camera,
                        };
                    }
                    mouse::Button::Right => {
                        state.interaction = Interaction::Panning {
                            last: screen,
                            click: Some(screen),
                            start: state.camera,
                        };
                    }
                    mouse::Button::Left => {
                        self.press(state, world, screen, bounds, shell);
                    }
                    _ => {}
                }
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
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
                            self.emit(shell, CanvasEvent::Move { ids, delta });
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
                        let next = drag.moved(*position, along - *last, state.modifiers.shift());
                        *last = along;
                        if next != *position {
                            *position = next;
                            let event = CanvasEvent::SetControl {
                                module: *module,
                                widget: *widget,
                                part: ControlPart::Main,
                                position: next,
                            };
                            self.emit(shell, event);
                        }
                    }
                    Interaction::Pad {
                        module,
                        widget,
                        throw,
                        last,
                        position,
                    } => {
                        let scale = if state.modifiers.shift() { 0.1 } else { 1.0 } / *throw;
                        let moved = screen - *last;
                        *last = screen;
                        let next = (
                            (position.0 + moved.x * scale).clamp(0.0, 1.0),
                            (position.1 - moved.y * scale).clamp(0.0, 1.0),
                        );
                        let (module, widget, before) = (*module, *widget, *position);
                        *position = next;
                        self.set_pad(shell, module, widget, before, next);
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
                        let delta = drag.delta(along - *last, state.modifiers.shift());
                        *last = along;
                        let (next_grab, low, high) = grab.moved(position.0, position.1, delta);
                        *grab = next_grab;
                        let (module, widget, before) = (*module, *widget, *position);
                        *position = (low, high);
                        self.set_pad(shell, module, widget, before, (low, high));
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
                        let delta = drag.delta(moved, state.modifiers.shift());
                        if delta != 0.0 {
                            let event = CanvasEvent::DragDepth {
                                module: *module,
                                widget: *widget,
                                part: *part,
                                pinned: self.pin(state),
                                delta,
                            };
                            self.emit(shell, event);
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
                            self.assign_drag(*from, Some(at), state.camera, bounds, shell);
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
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                // The left button is what started the hold. Releasing another
                // button, one that was already down, must not end it.
                if *button != mouse::Button::Left && matches!(state.interaction, Interaction::Holding { .. }) {
                    return;
                }
                // Only the right button's own release ends a right click. A
                // left button let go meanwhile must not open the menu.
                if *button != mouse::Button::Right
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
                                shell,
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
                                        shell,
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
                                    shell,
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
                                self.emit(shell, CanvasEvent::AssignCancel);
                            } else {
                                self.emit(
                                    shell,
                                    CanvasEvent::AssignDrop {
                                        from,
                                        at: if state.outside_window { None } else { cursor.position() },
                                    },
                                );
                            }
                        }
                    }
                    Interaction::Selecting { start, current, base } => {
                        let area = Rectangle::new(
                            Point::new(start.x.min(current.x), start.y.min(current.y)),
                            Size::new((start.x - current.x).abs(), (start.y - current.y).abs()),
                        );
                        let mut selection = base;
                        selection.extend(self.scene.modules_in(area));
                        self.emit(shell, CanvasEvent::Select(selection.into_iter().collect()));
                    }
                    Interaction::Moving {
                        applied,
                        click: Some(action),
                        ..
                    } if applied == Vector::ZERO => match action {
                        ChoiceClick::Set(module, widget, position) => self.set_control(shell, module, widget, position),
                        ChoiceClick::Open(module, widget) => self.open_select(state, module, widget),
                        ChoiceClick::ClearClip(module, widget) => {
                            self.emit(shell, CanvasEvent::ClearClip { module, widget });
                        }
                    },
                    Interaction::Holding { module, widget } => {
                        if !Self::key_holds_button(state, module, widget) {
                            self.emit(shell, CanvasEvent::ButtonReleased { module, widget });
                        }
                    }
                    Interaction::Panning { click: Some(_), .. } => {
                        if let (Some(world), Some(screen)) = (world, screen) {
                            self.open_probe_menu(state, world, screen, bounds, shell);
                        }
                    }
                    // The pan is over: the app learns the view once, not on
                    // every move, and not at all if the view is where it was.
                    Interaction::Panning { click: None, start, .. } => {
                        if state.camera != start {
                            self.emit(shell, CanvasEvent::Camera(state.camera));
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
                    self.emit(shell, CanvasEvent::GestureEnd);
                }
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.outside_window = true;
                if let Interaction::Wiring { from, pulled: false } = state.interaction
                    && from.output
                {
                    self.assign_drag(from, None, state.camera, bounds, shell);
                }
                self.release_pointer_hold(state, shell);
                if state.hover.take().is_some() {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorEntered) => {
                state.outside_window = false;
                if let Interaction::Wiring { from, pulled: false } = state.interaction
                    && from.output
                {
                    self.assign_drag(from, cursor.position(), state.camera, bounds, shell);
                }
            }
            Event::Window(window::Event::Unfocused) => {
                // A key release never arrives if the window loses focus
                // while Space or Enter is held. End every hold here.
                self.release_pointer_hold(state, shell);
                self.release_every_key_hold(state, shell);
                self.end_gestures(state, shell);
                // No key-up follows, so a group this view does not show yet
                // must end too.
                self.end_arrow_group(shell, true);
                shell.request_redraw();
            }
            _ => {}
        }
    }

    pub(super) fn press(
        &self,
        state: &mut State,
        world: Point,
        screen: Point,
        bounds: Rectangle,
        shell: &mut Shell<'_, Message>,
    ) {
        if let Some(tile) = self.resize_at(world) {
            self.select_for_edit(tile.id, shell);
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
                shell,
            );
            return;
        }
        let shift = state.modifiers.shift();
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
                shell,
            );
            return;
        }
        if !matches!(hit, Hit::Widget { .. }) {
            state.focus.clear();
        }
        match hit {
            Hit::Widget { module, widget } if self.learn.is_some() && self.is_control(module, widget) => {
                self.select_for_edit(module, shell);
                // The clicked control takes the keyboard focus, so Space or
                // Enter acts on it next, not on a control focused earlier.
                state.focus.set(module, widget, false);
                self.emit(shell, CanvasEvent::Learn { module, widget });
            }
            Hit::Badge(port) => {
                // A click pins this output's lines, or unpins them. Only one
                // output is pinned at a time.
                state.pinned = if state.pinned == Some(port) { None } else { Some(port) };
            }
            Hit::Widget { module, widget } => {
                self.select_for_edit(module, shell);
                state.focus.set(module, widget, false);
                let now = Instant::now();
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
                        shell,
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
                    self.press_button(state, module, widget, shell);
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
                                shell,
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
                        let jump = state.modifiers.alt().then(|| xy_position(area, world));
                        self.begin(
                            state,
                            Interaction::Pad {
                                module,
                                widget,
                                throw: xy_throw(area, state.camera.zoom),
                                last: screen,
                                position: jump.unwrap_or(before),
                            },
                            shell,
                        );
                        if let Some(jump) = jump {
                            self.set_pad(shell, module, widget, before, jump);
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
                            shell,
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
                    shell,
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
                    self.emit(shell, CanvasEvent::GestureStart);
                    self.emit(shell, CanvasEvent::Disconnect(port));
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
                        shell,
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
                        shell,
                    );
                }
                self.begin(
                    state,
                    Interaction::Wiring {
                        from: port,
                        pulled: false,
                    },
                    shell,
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
                    self.emit(shell, CanvasEvent::Select(selection.iter().copied().collect()));
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
                    let again = last.is_some_and(|(at, m, point)| m == id && double_click(Some((at, point)), screen));
                    if again {
                        self.emit(shell, CanvasEvent::OpenTile(id));
                        return;
                    }
                    state.last_header_click = Some((Instant::now(), id, screen));
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
                        m == id && w == widget && double_click(Some((at, point)), screen)
                    })
                });
                let click = if let Some(widget) = again {
                    self.emit(shell, CanvasEvent::ResetMeter { module: id, widget });
                    None
                } else {
                    state.last_meter_click = meter.map(|widget| (Instant::now(), id, widget, screen));
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
                        shell,
                    );
                }
            }
            Hit::Nothing if double_click(state.last_empty_click.take(), screen) => {
                self.open_browser(world, screen, bounds, shell);
            }
            Hit::Cable(_) | Hit::Nothing => {
                if matches!(hit, Hit::Nothing) {
                    state.last_empty_click = Some((Instant::now(), screen));
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
    fn frame(&self, state: &mut State, size: Size) {
        if let Some(world) = self.scene.bounds() {
            state.camera.frame_readable(world, size, READABLE_ZOOM);
        }
    }

    /// Sends [`CanvasEvent::OpenBrowser`] for a point given in world and
    /// canvas-relative screen coordinates.
    pub(super) fn open_browser(&self, world: Point, screen: Point, bounds: Rectangle, shell: &mut Shell<'_, Message>) {
        self.emit(
            shell,
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
        state: &State,
        world: Point,
        screen: Point,
        bounds: Rectangle,
        shell: &mut Shell<'_, Message>,
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
                    shell,
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
            shell,
            CanvasEvent::ProbeMenu {
                output,
                world: anchor,
                at: Point::new(bounds.x + screen.x, bounds.y + screen.y),
                area: bounds,
            },
        );
    }
}
