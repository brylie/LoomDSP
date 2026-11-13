//! Running each command in the app, and the screenshots and searches some of them wait for.

use super::*;

/// True for a command that works through what an open question blocks. It is
/// refused with an error, because its reply would otherwise say it worked
/// when nothing happened (rule 9).
///
/// No wildcard, on purpose: a new command must choose a side. `new`, `open`,
/// and `save` act directly, and a pointer goes through the view, where the
/// question's scrim takes it as it would a real pointer.
fn blocked_by_question(command: &Command) -> bool {
    match command {
        Command::Select { .. }
        | Command::Set { .. }
        | Command::Add { .. }
        | Command::Remove { .. }
        | Command::Connect { .. }
        | Command::Disconnect { .. }
        | Command::Probe { .. }
        | Command::Undo
        | Command::Redo
        | Command::Arm { .. }
        | Command::MidiOptions { .. }
        | Command::Type { .. }
        | Command::OutputSafety { .. }
        | Command::Audio { .. }
        | Command::Midi { .. } => true,
        Command::Snapshot
        | Command::Panic
        | Command::Open { .. }
        | Command::Save { .. }
        | Command::Note { .. }
        | Command::Cc { .. }
        | Command::Key { .. }
        | Command::Toolbar { .. }
        | Command::Press { .. }
        | Command::Release { .. }
        | Command::Hover { .. }
        | Command::Drag { .. }
        | Command::Scroll { .. }
        | Command::Locate { .. }
        | Command::Screenshot { .. }
        | Command::Sample { .. }
        | Command::Wait { .. }
        | Command::Quit
        | Command::New => false,
    }
}

/// Searches the live window for a target, then does `then` with what it
/// found.
pub(super) fn search(call: Call, target: String, then: AfterSearch) -> Task<Message> {
    let operation = Find {
        text: target.clone(),
        found: Found::default(),
        parts: crate::places::Collect::default(),
    };
    iced_runtime::task::widget(operation).map(move |found| Message::Located {
        call: call.clone(),
        target: target.clone(),
        found,
        then: then.clone(),
    })
}

/// Asks for the window as it was last drawn, to save all of it, or the
/// part inside `area` (logical window pixels), once the capture comes.
pub(super) fn capture(call: Call, path: PathBuf, area: Option<Rect>) -> loom_ui::Effects<Message> {
    loom_ui::Effects::capture(move |image| Message::Captured {
        call,
        path,
        area,
        image,
    })
}

/// A part of a capture, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Region {
    /// Its left edge.
    pub x: u32,
    /// Its top edge.
    pub y: u32,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
}

/// The part of a `width` by `height` capture inside `area`, in physical
/// pixels at `scale`, cut to the image. `None` when nothing of it is
/// inside.
pub(super) fn crop_region(width: u32, height: u32, scale: f32, area: Rect) -> Option<Region> {
    let x0 = (area.x * scale).floor().max(0.0);
    let y0 = (area.y * scale).floor().max(0.0);
    let x1 = ((area.x + area.width) * scale).ceil().min(width as f32);
    let y1 = ((area.y + area.height) * scale).ceil().min(height as f32);
    (x1 > x0 && y1 > y0).then_some(Region {
        x: x0 as u32,
        y: y0 as u32,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    })
}

/// The pixels of `image` inside `region`, row by row from the top.
pub(super) fn crop(image: &loom_ui::effect::Image, region: Region) -> Vec<u8> {
    let row = image.width() as usize * 4;
    let (left, wide) = (region.x as usize * 4, region.width as usize * 4);
    (region.y..region.y + region.height)
        .flat_map(|y| {
            let start = y as usize * row + left;
            image.rgba()[start..start + wide].iter().copied()
        })
        .collect()
}

/// Saves a window capture, or the part of it inside `area`, as a PNG file.
/// The reply has the image size in pixels, the scale factor, and, for an
/// area, the area saved in logical window pixels.
pub(super) fn save_screenshot(
    shot: Result<loom_ui::effect::Image, loom_ui::effect::NoGpu>,
    path: &std::path::Path,
    area: Option<Rect>,
) -> Answer {
    let shot = shot.map_err(|_| "no window to capture")?;
    let scale = shot.scale();
    let (mut width, mut height) = (shot.width(), shot.height());
    let mut saved = None;
    let rgba = if let Some(area) = area {
        let region = crop_region(width, height, scale, area).ok_or("the area is outside the window")?;
        (width, height) = (region.width, region.height);
        saved = Some(json!({
            "x": region.x as f32 / scale,
            "y": region.y as f32 / scale,
            "width": region.width as f32 / scale,
            "height": region.height as f32 / scale,
        }));
        std::borrow::Cow::Owned(crop(&shot, region))
    } else {
        // The whole window is saved as it came: no second copy of a frame
        // that can be tens of megabytes.
        std::borrow::Cow::Borrowed(shot.rgba())
    };
    crate::screenshot::write_png(path, iced::Size::new(width, height), &rgba)?;
    let mut reply = fields(json!({
        "width": width,
        "height": height,
        "scale": scale,
    }));
    if let Some(saved) = saved {
        reply.insert("area".to_string(), saved);
    }
    Ok(reply)
}

impl App {
    /// Saves a window capture for a `screenshot` command, and answers it.
    pub(crate) fn captured(
        &self,
        call: &Call,
        path: &std::path::Path,
        area: Option<Rect>,
        image: Result<loom_ui::effect::Image, loom_ui::effect::NoGpu>,
    ) {
        call.answer(save_screenshot(image, path, area));
    }

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
                    let seen = self.session.controller.last_control_change();
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
                    crate::scene::update_live_values(
                        &mut self.session.values,
                        &self.session.controller,
                        &self.session.hierarchy,
                    );
                    match sampling.record(
                        self.session.controller.graph(),
                        &self.session.hierarchy,
                        &self.session.values,
                        now,
                    ) {
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
                Step::Type(text) => {
                    tasks.push(
                        crate::editing::text_field_focused().map(move |focused| Message::TypeFocus {
                            call: call.clone(),
                            text: text.clone(),
                            focused,
                        }),
                    )
                }
                Step::Screenshot(path, Area::Target { target, margin }) => {
                    tasks.push(search(call, target, AfterSearch::Screenshot { path, margin }));
                }
                Step::Screenshot(path, Area::Rect(area)) => self.effect(capture(call, path, Some(area))),
                Step::Screenshot(path, Area::Window) => self.effect(capture(call, path, None)),
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
        if let Some(question) = &self.overlays.question
            && blocked_by_question(&call.command)
        {
            return (Then::Now(Err(question_open_error(question))), Task::none());
        }
        let mut tasks = Vec::new();
        let mut send = |app: &mut App, message: Message| tasks.push(app.update_iced(message));
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
                        self.session
                            .controller
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
                    self.say_opened(&path);
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
                    before: self.session.controller.last_control_change().map(|seen| seen.serial),
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
                // takes goes to the app as the input listener sends it.
                let then = match key(&name)
                    .and_then(|k| Ok((k, modifiers(&names)?)))
                    .and_then(|(k, m)| refuse_dialog_keys(&k, m).map(|()| (k, m)))
                    .and_then(|(k, m)| refuse_while_asking(self.overlays.question.as_ref(), &k, m).map(|()| (k, m)))
                {
                    Ok((k, m)) => {
                        let steps = key_events(k, m).into_iter().map(PointerStep::Key).collect();
                        Then::AfterDraw(Step::Pointer(self.pointer.push(steps), false))
                    }
                    Err(error) => Then::Now(Err(error)),
                };
                return (then, Task::batch(tasks));
            }
            Command::Type { text } => {
                let then = match type_events(&text) {
                    Ok(_) => Then::AfterDraw(Step::Type(text)),
                    Err(error) => Then::Now(Err(error)),
                };
                return (then, Task::batch(tasks));
            }
            Command::Toolbar { button } => press_message(self, &button).and_then(|message| {
                // An answer that wrote nothing is an error, because the step
                // after it was dropped too (rule 9). `update` cannot say so,
                // and a field would not survive the patch being replaced.
                if let Message::Question(answer) = message {
                    self.session.document.may_have_changed();
                    if !self.question_message(answer) {
                        return Err(self.alerts.status().to_string());
                    }
                } else {
                    send(self, message);
                }
                Ok(Map::new())
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
            Command::MidiOptions {
                target,
                mode,
                encoding,
                low,
                high,
                invert,
            } => self
                .midi_options_command(&target, mode.as_deref(), encoding.as_deref(), (low, high), invert)
                .map(|message| {
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
                            Ok(Area::Rect(Rect::new(Point::new(x, y), loom_ui_core::Size::new(w, h))))
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
        let wanted = self.input_wanted();
        let denied = wanted && self.rig.host.audio_input_permission_denied();
        let input = self.rig.input.stream_report(wanted, denied);
        let Some(stats) = self.session.engine.stream.stats().cloned() else {
            return json!({
                "device": null, "rate": null, "buffer": null, "callbacks": 0,
                "advanced": null, "stream_errors": 0, "start_overloads": 0, "overruns": 0,
                "input": input,
            });
        };
        let now = stats.snapshot();
        // A new stream counts from its start.
        let before = match &self.session.engine.control_seen {
            Some((seen, callbacks)) if Arc::ptr_eq(seen, &stats) => *callbacks,
            _ => 0,
        };
        if remember {
            self.session.engine.control_seen = Some((stats.clone(), now.callbacks));
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
            "input": input,
        })
    }

    pub(super) fn check_modules(&self, modules: &[u32]) -> Result<(), String> {
        match modules
            .iter()
            .find(|&&m| self.session.controller.graph().node(ModuleId(m)).is_none())
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
            .session
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
            (Some(value), None) => Message::Inspector(crate::inspector::Message::ParamValue { module, index, value }),
            (None, Some(position)) => Message::Inspector(crate::inspector::Message::Param {
                module,
                index,
                position,
            }),
            _ => return Err("give `value` or `position`, not both".to_string()),
        };
        send(self, message);
        send(self, Message::SliderReleased);
        let stored = self
            .session
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
        if !self.session.learn.is_on() {
            return Err("MIDI learn is off; press `learn` first".to_string());
        }
        let Some((module, Some(param))) = split_target(target) else {
            return Err(format!(
                "`{target}` is not a parameter; use `module:param`, like `4:cutoff`"
            ));
        };
        let node = self
            .session
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

    /// Checks a `midi_options` command and builds what it asks for. Fields
    /// left out keep their value, and options that do not pass are refused
    /// with the reason.
    pub(super) fn midi_options_command(
        &self,
        target: &str,
        mode: Option<&str>,
        encoding: Option<&str>,
        (low, high): (Option<f32>, Option<f32>),
        invert: Option<bool>,
    ) -> Result<Message, String> {
        use loom_core::{MidiEncoding, MidiMode};
        let Some((module, Some(param))) = split_target(target) else {
            return Err(format!(
                "`{target}` is not a parameter; use `module:param`, like `4:cutoff`"
            ));
        };
        let node = self
            .session
            .controller
            .graph()
            .node(ModuleId(module))
            .ok_or_else(|| format!("no module {module}"))?;
        let index = node
            .descriptor
            .param_index(param)
            .ok_or_else(|| format!("module {module} ({}) has no parameter `{param}`", node.descriptor.name))?;
        let (_, mapping) = self
            .session
            .controller
            .graph()
            .midi_mapping_of(ModuleId(module), index)
            .ok_or_else(|| format!("`{target}` has no MIDI mapping"))?;
        let mut options = mapping.options;
        let encoding = match encoding {
            None => None,
            Some("offset64") => Some(MidiEncoding::Offset64),
            Some("twos_complement") => Some(MidiEncoding::TwosComplement),
            Some("sign_bit") => Some(MidiEncoding::SignBit),
            Some(other) => {
                return Err(format!(
                    "no encoding `{other}`; use offset64, twos_complement, or sign_bit"
                ));
            }
        };
        let relative = |current: MidiMode| match current {
            MidiMode::Relative(kept) => kept,
            _ => MidiEncoding::default(),
        };
        options = match (mode, encoding) {
            (None, None) => options,
            (None, Some(encoding)) => {
                if !matches!(options.mode, MidiMode::Relative(_)) {
                    return Err("an encoding is only for a Relative mapping".to_string());
                }
                options.with_mode(MidiMode::Relative(encoding))
            }
            (Some("relative"), encoding) => {
                options.with_mode(MidiMode::Relative(encoding.unwrap_or(relative(options.mode))))
            }
            (Some(_), Some(_)) => return Err("an encoding is only for a Relative mapping".to_string()),
            (Some("absolute"), None) => options.with_mode(MidiMode::Absolute),
            (Some("pickup"), None) => options.with_mode(MidiMode::Pickup),
            (Some(other), None) => return Err(format!("no mode `{other}`; use absolute, pickup, or relative")),
        };
        options.low = low.unwrap_or(options.low);
        options.high = high.unwrap_or(options.high);
        options.invert = invert.unwrap_or(options.invert);
        options.check()?;
        Ok(Message::Learn(crate::learn::Message::MidiOptions {
            module,
            index,
            options,
        }))
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
        let graph = self.session.controller.graph();
        let (widget, part, meter) = match graph.node(ModuleId(module)) {
            Some(node) => {
                let widgets = node.descriptor.panel.widgets;
                let widget = widgets.iter().position(|w| w.binds(name)).ok_or_else(|| {
                    format!(
                        "module {module} ({}) has no control, light, or meter `{name}`",
                        node.descriptor.name
                    )
                })?;
                let part = if widgets[widget].second() == Some(name) {
                    ControlPart::Second
                } else {
                    ControlPart::Main
                };
                (widget, part, widgets[widget].kind == WidgetKind::Meter)
            }
            // A sub-patch tile's surface: its exposed parameters, by name.
            None => {
                let widget = crate::surface::find_control(graph, &self.session.hierarchy, module, name).ok_or_else(
                    || match self.session.hierarchy.tile_name(ModuleId(module)) {
                        Some(tile) => format!("module {module} ({tile}) has no control `{name}`"),
                        None => format!("no module {module}"),
                    },
                )?;
                (widget, ControlPart::Main, false)
            }
        };
        Ok(Sampling {
            module,
            widget,
            name: name.to_string(),
            part,
            meter,
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
            None => self.session.scene.bounds().map_or(Point::ORIGIN, |b| {
                Point::new(b.x, b.y + b.height + 2.0 * loom_canvas::GRID)
            }),
        };
        let before: Vec<u32> = self.session.controller.graph().nodes().map(|(id, _)| id.0).collect();
        self.add_module(descriptor, world);
        let added = self
            .session
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
            .session
            .scene
            .port_position(output)
            .ok_or_else(|| format!("`{from}` is not on the canvas"))?;
        let before: Vec<u32> = self.session.controller.graph().nodes().map(|(id, _)| id.0).collect();
        send(self, Message::AddProbe { probe, output, world });
        let added = self
            .session
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
            .session
            .engine
            .midi
            .as_ref()
            .ok_or("no audio is playing the engine; start the app with --fake-audio or a device")?;
        if loom_host_desktop::send_midi(midi, event) {
            Ok(Map::new())
        } else {
            Err("the MIDI queue is full".to_string())
        }
    }

    /// Types the text of a `type` command, now that the window has said
    /// whether a text field has focus. With none, no key is sent: a letter
    /// would reach the app's shortcuts.
    pub(crate) fn type_after_focus_check(&mut self, call: Call, text: &str, focused: bool) {
        if !focused {
            call.answer(Err(
                "no text field has focus, so nothing was typed; open or click one first".to_string(),
            ));
            return;
        }
        match type_events(text) {
            Ok(events) => {
                let steps = events.into_iter().map(PointerStep::Key).collect();
                self.control_pending.push(Pending {
                    frames: FRAMES_BEFORE_ANSWER,
                    call,
                    step: Step::Pointer(self.pointer.push(steps), false),
                });
            }
            Err(error) => call.answer(Err(error)),
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
            AfterSearch::Screenshot { path, margin } => {
                match rect {
                    Ok(rect) => self.effect(capture(call, path, Some(rect.expand(margin)))),
                    Err(error) => call.answer(Err(error)),
                }
                Task::none()
            }
        }
    }
}
