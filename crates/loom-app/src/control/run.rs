//! Running each command in the app, and the screenshots and searches some of them wait for.

use super::*;

/// Searches the live window for a target, then does `then` with what it
/// found.
pub(super) fn search(call: Call, target: String, then: AfterSearch) -> Task<Message> {
    let operation = Find {
        text: target.clone(),
        found: Found::default(),
    };
    iced_runtime::task::widget(operation).map(move |found| Message::Located {
        call: call.clone(),
        target: target.clone(),
        found,
        then: then.clone(),
    })
}

/// Captures the window as it was last drawn, and saves all of it, or the
/// part inside `area` (logical window pixels).
pub(super) fn capture(call: Call, path: PathBuf, area: Option<Rectangle>) -> Task<Message> {
    iced::window::latest()
        .then(|id| match id {
            Some(id) => iced::window::screenshot(id).map(Some),
            None => Task::done(None),
        })
        .map(move |shot| call.answer(save_screenshot(shot, &path, area)))
        .discard()
}

/// The part of a capture inside `area`, in physical pixels, cut to the
/// image. `None` when nothing of it is inside.
pub(super) fn crop_region(size: iced::Size<u32>, scale: f32, area: Rectangle) -> Option<Rectangle<u32>> {
    let x0 = (area.x * scale).floor().max(0.0);
    let y0 = (area.y * scale).floor().max(0.0);
    let x1 = ((area.x + area.width) * scale).ceil().min(size.width as f32);
    let y1 = ((area.y + area.height) * scale).ceil().min(size.height as f32);
    (x1 > x0 && y1 > y0).then_some(Rectangle {
        x: x0 as u32,
        y: y0 as u32,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    })
}

/// Saves a window capture, or the part of it inside `area`, as a PNG file.
/// The reply has the image size in pixels, the scale factor, and, for an
/// area, the area saved in logical window pixels.
pub(super) fn save_screenshot(
    shot: Option<iced::window::Screenshot>,
    path: &std::path::Path,
    area: Option<Rectangle>,
) -> Answer {
    let mut shot = shot.ok_or("no window to capture")?;
    let mut saved = None;
    if let Some(area) = area {
        let region = crop_region(shot.size, shot.scale_factor, area).ok_or("the area is outside the window")?;
        shot = shot
            .crop(region)
            .map_err(|e| format!("cannot cut the area out: {e:?}"))?;
        let scale = shot.scale_factor;
        saved = Some(json!({
            "x": region.x as f32 / scale,
            "y": region.y as f32 / scale,
            "width": region.width as f32 / scale,
            "height": region.height as f32 / scale,
        }));
    }
    crate::screenshot::write_png(path, shot.size, &shot.rgba)?;
    let mut reply = fields(json!({
        "width": shot.size.width,
        "height": shot.size.height,
        "scale": shot.scale_factor,
    }));
    if let Some(saved) = saved {
        reply.insert("area".to_string(), saved);
    }
    Ok(reply)
}

impl App {
    /// Runs a command from the control channel and answers it, now or once
    /// the task it returns has run.
    pub(crate) fn control(&mut self, call: Call) -> Task<Message> {
        let (then, task) = self.run_command(&call);
        match then {
            // An error, or a command that changes nothing on screen.
            Then::Now(answer) => call.answer(answer),
            Then::AfterDraw(step) => self.control_pending.push(Pending {
                frames: FRAMES_BEFORE_ANSWER,
                call,
                step,
            }),
        }
        task
    }

    /// Counts a frame for the commands waiting to be drawn, and finishes
    /// those that are (rule 5).
    pub(crate) fn control_frame(&mut self, now: Instant) -> Task<Message> {
        let mut tasks = Vec::new();
        let mut waiting = Vec::new();
        for mut pending in std::mem::take(&mut self.control_pending) {
            // Pointer events go in one per frame. The answer waits for the
            // last one to be drawn.
            if let Step::Pointer(serial, _) = pending.step
                && self.pointer.busy(serial)
            {
                pending.frames = FRAMES_BEFORE_ANSWER;
                waiting.push(pending);
                continue;
            }
            pending.frames = pending.frames.saturating_sub(1);
            if pending.frames > 0 {
                waiting.push(pending);
                continue;
            }
            let Pending { call, step, .. } = pending;
            match step {
                Step::Answer(answer) => call.answer(answer),
                Step::Report(report) => call.answer(Ok(self.preferences_reply(report))),
                Step::ControlChange {
                    before,
                    control,
                    value,
                    frames_left,
                } => {
                    let seen = self.controller.last_control_change();
                    if seen.is_some_and(|seen| {
                        Some(seen.serial) != before && seen.control == control && seen.value == value
                    }) {
                        // As the MIDI watch would. Applying it twice is
                        // harmless: an armed parameter learns only once.
                        self.midi_arrived();
                        call.answer(Ok(Map::new()));
                    } else if frames_left == 0 {
                        call.answer(Err(
                            "the engine did not take the control change, or other MIDI came right after it".to_string(),
                        ));
                    } else {
                        waiting.push(Pending {
                            frames: 1,
                            call,
                            step: Step::ControlChange {
                                before,
                                control,
                                value,
                                frames_left: frames_left - 1,
                            },
                        });
                    }
                }
                Step::Sample(mut sampling) => {
                    // The live values this frame draws, even when nothing
                    // else asks for frames.
                    crate::scene::update_live_values(&mut self.values, &self.controller);
                    match sampling.record(self.controller.graph(), &self.values, now) {
                        Err(error) => call.answer(Err(error)),
                        Ok(true) => call.answer(Ok(fields(json!({ "frames": sampling.frames })))),
                        Ok(false) => waiting.push(Pending {
                            frames: 1,
                            call,
                            step: Step::Sample(sampling),
                        }),
                    }
                }
                Step::Pointer(serial, position) => {
                    let answer = match self.pointer.take_error(serial) {
                        Some(error) => Err(error),
                        None if position => Ok(self.pointer_reply()),
                        None => Ok(Map::new()),
                    };
                    call.answer(answer);
                }
                Step::Locate(target) => tasks.push(search(call, target, AfterSearch::Locate)),
                Step::Screenshot(path, Area::Target { target, margin }) => {
                    tasks.push(search(call, target, AfterSearch::Screenshot { path, margin }));
                }
                Step::Screenshot(path, Area::Rect(area)) => tasks.push(capture(call, path, Some(area))),
                Step::Screenshot(path, Area::Window) => tasks.push(capture(call, path, None)),
            }
        }
        self.control_pending = waiting;
        Task::batch(tasks)
    }

    /// True while a command waits for the window to draw.
    pub(crate) fn control_waiting(&self) -> bool {
        !self.control_pending.is_empty()
    }

    /// What to answer, now or after drawing, and the tasks the UI messages
    /// returned.
    pub(super) fn run_command(&mut self, call: &Call) -> (Then, Task<Message>) {
        let mut tasks = Vec::new();
        let mut send = |app: &mut App, message: Message| tasks.push(app.update(message));
        let answer = match call.command.clone() {
            Command::Select { modules, .. } => self.check_modules(&modules).map(|()| {
                send(self, Message::Canvas(CanvasEvent::Select(modules)));
                Map::new()
            }),
            Command::Set {
                module,
                param,
                value,
                position,
                ..
            } => self.set_command(module, &param, value, position, &mut send),
            Command::Add { kind, at, .. } => self.add_command(&kind, at),
            Command::Remove { modules, .. } => self.check_modules(&modules).map(|()| {
                send(self, Message::Canvas(CanvasEvent::Delete(modules)));
                Map::new()
            }),
            Command::Connect { from, to, .. } => port(self, &from, true)
                .and_then(|from| Ok((from, port(self, &to, false)?)))
                .and_then(|(from, to)| {
                    if [from.module, to.module].iter().any(|&id| {
                        self.controller
                            .graph()
                            .node(ModuleId(id))
                            .is_some_and(|node| node.unknown.is_some())
                    }) {
                        return Err("Cannot connect to an unknown module".into());
                    }
                    send(self, Message::Canvas(CanvasEvent::Connect { from, to }));
                    Ok(Map::new())
                }),
            Command::Probe { from, kind, .. } => self.probe_command(&from, &kind, &mut send),
            Command::Disconnect { to, .. } => port(self, &to, false).map(|to| {
                send(self, Message::Canvas(CanvasEvent::Disconnect(to)));
                Map::new()
            }),
            Command::Panic => {
                send(self, Message::Panic);
                Ok(Map::new())
            }
            Command::Undo | Command::Redo => {
                let undo = matches!(call.command, Command::Undo);
                send(self, if undo { Message::Undo } else { Message::Redo });
                let snapshot = self.ui_snapshot();
                Ok(fields(json!({ "undo": snapshot.undo, "redo": snapshot.redo })))
            }
            Command::Open { path, .. } => {
                if self.replace(crate::Source::File(&path)) {
                    self.alerts.say(format!("opened {}", path.display()));
                    Ok(Map::new())
                } else {
                    Err(self.alerts.status().to_string())
                }
            }
            Command::Save { path, .. } => {
                self.finish_edits();
                if self.write_to(path) {
                    Ok(Map::new())
                } else {
                    Err(self.alerts.status().to_string())
                }
            }
            Command::Note { note, velocity, on, .. } => {
                let event = if on {
                    MidiEvent::note_on(0, 0, note.min(127), velocity.unwrap_or(100).clamp(1, 127))
                } else {
                    MidiEvent::note_off(0, 0, note.min(127))
                };
                self.send_midi(event)
            }
            Command::Cc {
                controller,
                value,
                channel,
                ..
            } => {
                let control = MidiControl {
                    channel: channel.unwrap_or(1).clamp(1, 16) - 1,
                    controller: controller.min(127),
                };
                let value = value.min(127);
                let step = Step::ControlChange {
                    before: self.controller.last_control_change().map(|seen| seen.serial),
                    control,
                    value,
                    frames_left: FRAMES_FOR_ENGINE,
                };
                return match self.send_midi(MidiEvent::control_change(0, control.channel, control.controller, value)) {
                    Ok(_) => (Then::AfterDraw(step), Task::batch(tasks)),
                    Err(error) => (Then::Now(Err(error)), Task::batch(tasks)),
                };
            }
            Command::Key {
                key: name,
                modifiers: names,
                ..
            } => {
                // Through the view, as a real key: a widget such as the
                // canvas can take it (Escape unpins a badge), and what none
                // takes goes to the app as the keyboard listener sends it.
                let then = match key(&name)
                    .and_then(|k| Ok((k, modifiers(&names)?)))
                    .and_then(|(k, m)| refuse_dialog_keys(&k, m).map(|()| (k, m)))
                {
                    Ok((k, m)) => {
                        let steps = key_events(k, m).into_iter().map(PointerStep::Key).collect();
                        Then::AfterDraw(Step::Pointer(self.pointer.push(steps), false))
                    }
                    Err(error) => Then::Now(Err(error)),
                };
                return (then, Task::batch(tasks));
            }
            Command::Toolbar { button } => press_message(self, &button).map(|message| {
                send(self, message);
                Map::new()
            }),
            Command::Press {
                button,
                target,
                x,
                y,
                modifiers,
            }
            | Command::Release {
                button,
                target,
                x,
                y,
                modifiers,
            } => {
                let down = matches!(call.command, Command::Press { .. });
                let steps = aim(target, x, y).and_then(|aim| {
                    let button = crate::pointer::button(button.as_deref())?;
                    let modifiers = self::modifiers(&modifiers)?;
                    let click = if down {
                        PointerStep::Press(button)
                    } else {
                        PointerStep::Release(button)
                    };
                    let steps = aim.map(PointerStep::MoveTo).into_iter().chain([click]).collect();
                    Ok(with_modifiers(modifiers, steps))
                });
                return (self.pointer_command(steps), Task::none());
            }
            Command::Hover { target, x, y } => {
                let steps = aim(target, x, y).and_then(|aim| {
                    aim.map(|aim| vec![PointerStep::MoveTo(aim)])
                        .ok_or_else(|| "give `target`, or `x` and `y`".to_string())
                });
                return (self.pointer_command(steps), Task::none());
            }
            Command::Drag {
                from,
                to,
                steps,
                button,
                modifiers,
            } => {
                let steps = (|| {
                    let from = drag_end(&from, "from")?;
                    let to = drag_end(&to, "to")?;
                    let moves = steps.unwrap_or(10);
                    if !(1..=MAX_DRAG_STEPS).contains(&moves) {
                        return Err(format!("`steps` must be from 1 to {MAX_DRAG_STEPS}"));
                    }
                    let button = crate::pointer::button(button.as_deref())?;
                    let modifiers = self::modifiers(&modifiers)?;
                    let mut list = vec![PointerStep::MoveTo(from), PointerStep::Press(button)];
                    list.extend((1..=moves).map(|i| PointerStep::Toward {
                        to: to.clone(),
                        t: i as f32 / moves as f32,
                    }));
                    list.push(PointerStep::Release(button));
                    Ok(with_modifiers(modifiers, list))
                })();
                return (self.pointer_command(steps), Task::none());
            }
            Command::Scroll {
                target,
                x,
                y,
                dy,
                dx,
                modifiers,
            } => {
                let steps = aim(target, x, y).and_then(|aim| {
                    let modifiers = self::modifiers(&modifiers)?;
                    let wheel = PointerStep::Wheel {
                        dx: dx.unwrap_or(0.0),
                        dy,
                    };
                    let steps = aim.map(PointerStep::MoveTo).into_iter().chain([wheel]).collect();
                    Ok(with_modifiers(modifiers, steps))
                });
                return (self.pointer_command(steps), Task::none());
            }
            Command::Arm { target } => self.arm_command(&target).map(|message| {
                send(self, message);
                Map::new()
            }),
            Command::Locate { target } => return (Then::AfterDraw(Step::Locate(target)), Task::batch(tasks)),
            Command::Screenshot {
                path,
                target,
                margin,
                rect,
            } => {
                let area = match (target, rect) {
                    (None, None) if margin.is_none() => Ok(Area::Window),
                    (Some(target), None) => match margin.unwrap_or(0.0) {
                        m if m >= 0.0 && m.is_finite() => Ok(Area::Target { target, margin: m }),
                        _ => Err("`margin` is a number of pixels, 0 or more".to_string()),
                    },
                    (None, Some([x, y, w, h])) if margin.is_none() => {
                        if w > 0.0 && h > 0.0 {
                            Ok(Area::Rect(Rectangle::new(Point::new(x, y), iced::Size::new(w, h))))
                        } else {
                            Err("`rect` is [x, y, width, height], with a width and height above 0".to_string())
                        }
                    }
                    (None, _) => Err("`margin` goes with `target`".to_string()),
                    (Some(_), Some(_)) => Err("give `target` or `rect`, not both".to_string()),
                };
                let then = match area {
                    Ok(area) => Then::AfterDraw(Step::Screenshot(path, area)),
                    Err(error) => Then::Now(Err(error)),
                };
                return (then, Task::batch(tasks));
            }
            Command::Sample { target, ms } => {
                let then = match self.sample_command(&target, ms) {
                    Ok(sampling) => Then::AfterDraw(Step::Sample(sampling)),
                    Err(error) => Then::Now(Err(error)),
                };
                return (then, Task::batch(tasks));
            }
            Command::Wait { .. } => Ok(Map::new()),
            Command::Quit => {
                self.quit = true;
                return (Then::Now(Ok(Map::new())), Task::batch(tasks));
            }
            Command::Snapshot => {
                let answer = serde_json::to_value(self.ui_snapshot())
                    .map(|snapshot| {
                        let audio = self.audio_health(true);
                        Map::from_iter([("snapshot".to_string(), snapshot), ("audio".to_string(), audio)])
                    })
                    .map_err(|e| e.to_string());
                return (Then::Now(answer), Task::none());
            }
            Command::OutputSafety { mode } => {
                let mode = match mode.as_str() {
                    "low-latency" => OutputSafety::LowLatency,
                    "clean" => OutputSafety::Clean,
                    "off" => OutputSafety::Off,
                    _ => {
                        let error = format!("unknown mode `{mode}`; modes: low-latency, clean, off");
                        return (Then::Now(Err(error)), Task::none());
                    }
                };
                send(self, Message::Preferences(settings::Message::Safety(mode)));
                return (Then::AfterDraw(Step::Report(Report::Safety)), Task::batch(tasks));
            }
            Command::Audio { device, rate, buffer } => {
                let choice = |value: Option<Value>, what: &str| match value {
                    None => Ok(None),
                    Some(Value::String(s)) if s == "default" => Ok(Some(None)),
                    Some(value) => value
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .map(|n| Some(Some(n)))
                        .ok_or_else(|| format!("`{what}` is a whole number or \"default\", not {value}")),
                };
                let read = self.read_device_lists();
                let change = choice(rate, "rate").and_then(|rate| {
                    let buffer = choice(buffer, "buffer")?;
                    self.control_audio_change(device.as_deref(), rate, buffer)
                });
                if let Ok(change) = &change {
                    send(self, Message::Preferences(settings::Message::Audio(change.clone())));
                }
                if read {
                    self.forget_device_lists();
                }
                let then = match change {
                    Ok(_) => Then::AfterDraw(Step::Report(Report::Audio)),
                    Err(error) => Then::Now(Err(error)),
                };
                return (then, Task::batch(tasks));
            }
            Command::Midi { input, on } => {
                let then = match self.control_midi_input(&input) {
                    Ok(name) => {
                        send(self, Message::Preferences(settings::Message::Midi(name, on)));
                        Then::AfterDraw(Step::Report(Report::Midi))
                    }
                    Err(error) => Then::Now(Err(error)),
                };
                return (then, Task::batch(tasks));
            }
            Command::New => {
                if self.replace(crate::Source::Empty) {
                    self.alerts.say("new patch".to_string());
                    Ok(Map::new())
                } else {
                    Err(self.alerts.status().to_string())
                }
            }
        };
        let then = match answer {
            Ok(answer) => Then::AfterDraw(Step::Answer(Ok(answer))),
            Err(error) => Then::Now(Err(error)),
        };
        (then, Task::batch(tasks))
    }

    /// Queues a pointer command's events, or refuses it.
    pub(super) fn pointer_command(&mut self, steps: Result<Vec<PointerStep>, String>) -> Then {
        match steps {
            Ok(steps) => Then::AfterDraw(Step::Pointer(self.pointer.push(steps), true)),
            Err(error) => Then::Now(Err(error)),
        }
    }

    /// The reply to a pointer command: where the pointer is.
    pub(super) fn pointer_reply(&self) -> Map<String, Value> {
        match self.pointer.position() {
            Some(p) => fields(json!({ "x": p.x, "y": p.y })),
            None => Map::new(),
        }
    }

    /// The reply to a Preferences command, read as it is drawn.
    pub(super) fn preferences_reply(&mut self, report: Report) -> Map<String, Value> {
        let mut reply = Map::new();
        if !matches!(report, Report::Midi) {
            reply.insert("playing".to_string(), Value::String(self.playing_line()));
        }
        reply.insert("status".to_string(), Value::String(self.alerts.status().to_string()));
        match report {
            Report::Safety => {}
            Report::Audio => {
                reply.insert("audio".to_string(), self.audio_health(false));
            }
            Report::Midi => {
                reply.insert("inputs".to_string(), json!(self.open_midi_inputs()));
            }
        }
        reply
    }

    /// The `audio` field: the stream playing now and its callback counters
    /// (control-channel.md, The `audio` field). `remember` makes these
    /// counts the ones the next `advanced` compares with, as a `snapshot`
    /// does.
    pub(super) fn audio_health(&mut self, remember: bool) -> Value {
        let Some(stats) = self.stream.stats().cloned() else {
            return json!({
                "device": null, "rate": null, "buffer": null, "callbacks": 0,
                "advanced": null, "stream_errors": 0, "start_overloads": 0, "overruns": 0,
            });
        };
        let now = stats.snapshot();
        // A new stream counts from its start.
        let before = match &self.control_seen {
            Some((seen, callbacks)) if Arc::ptr_eq(seen, &stats) => *callbacks,
            _ => 0,
        };
        if remember {
            self.control_seen = Some((stats.clone(), now.callbacks));
        }
        let playing = self.rig.playing.as_ref();
        json!({
            "device": playing.map(|p| p.device.clone()),
            "rate": playing.map(|p| p.rate),
            "buffer": (now.largest_buffer > 0).then_some(now.largest_buffer),
            "callbacks": now.callbacks,
            "advanced": now.callbacks > before,
            "stream_errors": now.stream_errors,
            "start_overloads": now.start_overloads,
            "overruns": now.overruns,
        })
    }

    pub(super) fn check_modules(&self, modules: &[u32]) -> Result<(), String> {
        match modules
            .iter()
            .find(|&&m| self.controller.graph().node(ModuleId(m)).is_none())
        {
            Some(m) => Err(format!("no module {m}")),
            None => Ok(()),
        }
    }

    pub(super) fn set_command(
        &mut self,
        module: u32,
        param: &str,
        value: Option<f32>,
        position: Option<f32>,
        send: &mut impl FnMut(&mut App, Message),
    ) -> Answer {
        let node = self
            .controller
            .graph()
            .node(ModuleId(module))
            .ok_or_else(|| format!("no module {module}"))?;
        let descriptor = node.descriptor;
        let index = descriptor
            .param_index(param)
            .ok_or_else(|| format!("module {module} ({}) has no parameter `{param}`", descriptor.name))?;
        // As a knob turn: one change, then let go, so it is one undo step. A
        // `value` goes in as it is, so it is not bent by a position round trip.
        let message = match (value, position) {
            (Some(value), None) => Message::ParamValue { module, index, value },
            (None, Some(position)) => Message::Param {
                module,
                index,
                position,
            },
            _ => return Err("give `value` or `position`, not both".to_string()),
        };
        send(self, message);
        send(self, Message::SliderReleased);
        let stored = self
            .controller
            .graph()
            .node(ModuleId(module))
            .map(|n| n.params[index])
            .unwrap_or_default();
        Ok(fields(json!({ "value": stored })))
    }

    /// The message an inspector row sends in learn mode: it arms the
    /// parameter, or disarms it if it is armed.
    pub(super) fn arm_command(&self, target: &str) -> Result<Message, String> {
        if !self.learn.is_on() {
            return Err("MIDI learn is off; press `learn` first".to_string());
        }
        let Some((module, Some(param))) = split_target(target) else {
            return Err(format!(
                "`{target}` is not a parameter; use `module:param`, like `4:cutoff`"
            ));
        };
        let node = self
            .controller
            .graph()
            .node(ModuleId(module))
            .ok_or_else(|| format!("no module {module}"))?;
        let descriptor = node.descriptor;
        let index = descriptor
            .param_index(param)
            .ok_or_else(|| format!("module {module} ({}) has no parameter `{param}`", descriptor.name))?;
        Ok(Message::Learn(crate::learn::Message::Arm((module, index))))
    }

    /// Checks a `sample` target: a panel widget by the name it is bound
    /// to, a parameter for a control or a display value for a light.
    pub(super) fn sample_command(&self, target: &str, ms: u64) -> Result<Sampling, String> {
        if !(1..=MAX_SAMPLE_MS).contains(&ms) {
            return Err(format!("`ms` must be from 1 to {MAX_SAMPLE_MS}"));
        }
        let Some((module, Some(name))) = split_target(target) else {
            return Err(format!(
                "`{target}` is not a control, light, or meter; use `module:name`, like `4:cutoff`"
            ));
        };
        let node = self
            .controller
            .graph()
            .node(ModuleId(module))
            .ok_or_else(|| format!("no module {module}"))?;
        let widgets = node.descriptor.panel.widgets;
        let widget = widgets.iter().position(|w| w.bind == name).ok_or_else(|| {
            format!(
                "module {module} ({}) has no control, light, or meter `{name}`",
                node.descriptor.name
            )
        })?;
        Ok(Sampling {
            module,
            widget,
            name: name.to_string(),
            meter: widgets[widget].kind == WidgetKind::Meter,
            ms,
            start: None,
            frames: Vec::new(),
        })
    }

    pub(super) fn add_command(&mut self, kind: &str, at: Option<[f32; 2]>) -> Answer {
        // Patch files name a kind with its version (`loom.filter.svf@1`).
        let id = kind.split_once('@').map_or(kind, |(id, _)| id);
        let descriptor = self
            .modules
            .iter()
            .copied()
            .find(|d| d.id == id)
            .ok_or_else(|| format!("no module kind `{kind}`"))?;
        // Below the patch, unless a place is given.
        let world = match at {
            Some([x, y]) => Point::new(x, y),
            None => self.scene.bounds().map_or(Point::ORIGIN, |b| {
                Point::new(b.x, b.y + b.height + 2.0 * loom_canvas::GRID)
            }),
        };
        let before: Vec<u32> = self.controller.graph().nodes().map(|(id, _)| id.0).collect();
        self.add_module(descriptor, world);
        let added = self
            .controller
            .graph()
            .nodes()
            .map(|(id, _)| id.0)
            .find(|id| !before.contains(id));
        match added {
            Some(module) => Ok(fields(json!({ "module": module }))),
            None => Err(self.alerts.status().to_string()),
        }
    }

    /// Adds a scope or a meter fed by an output, as the probe menu does.
    pub(super) fn probe_command(&mut self, from: &str, kind: &str, send: &mut impl FnMut(&mut App, Message)) -> Answer {
        let output = port(self, from, true)?;
        let probe = match kind {
            "scope" => crate::probe_menu::Probe::Scope,
            "meter" => crate::probe_menu::Probe::Meter,
            _ => return Err(format!("`{kind}` is not scope or meter")),
        };
        let world = self
            .scene
            .port_position(output)
            .ok_or_else(|| format!("`{from}` is not on the canvas"))?;
        let before: Vec<u32> = self.controller.graph().nodes().map(|(id, _)| id.0).collect();
        send(self, Message::AddProbe { probe, output, world });
        let added = self
            .controller
            .graph()
            .nodes()
            .map(|(id, _)| id.0)
            .find(|id| !before.contains(id));
        match added {
            Some(module) => Ok(fields(json!({ "module": module }))),
            None => Err(self.alerts.status().to_string()),
        }
    }

    pub(super) fn send_midi(&mut self, event: MidiEvent) -> Answer {
        let midi = self
            .midi
            .as_ref()
            .ok_or("no audio is playing the engine; start the app with --fake-audio or a device")?;
        if loom_host_desktop::send_midi(midi, event) {
            Ok(Map::new())
        } else {
            Err("the MIDI queue is full".to_string())
        }
    }

    /// Answers a `locate`, or takes a `screenshot` of the area around the
    /// target, once the window has been searched.
    pub(crate) fn located(&mut self, call: Call, target: &str, found: &Found, then: AfterSearch) -> Task<Message> {
        let rect = locate(self, target, found);
        match then {
            AfterSearch::Locate => {
                call.answer(rect.map(rect_fields));
                Task::none()
            }
            AfterSearch::Screenshot { path, margin } => match rect {
                Ok(rect) => capture(call, path, Some(rect.expand(margin))),
                Err(error) => {
                    call.answer(Err(error));
                    Task::none()
                }
            },
        }
    }
}
