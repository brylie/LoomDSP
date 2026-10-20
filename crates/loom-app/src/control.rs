//! The control channel: commands by name into the running app, and replies
//! with what it shows (`docs/design/control-channel.md`).
//!
//! A listener thread reads one JSON command per line, from a Unix socket
//! (`--control`) or a file (`--control-script`), and hands each to the app
//! as a [`Message::Control`]. The app runs it through the same messages the
//! UI sends and replies once it is applied. The thread then writes the
//! reply line and reads the next command. While no command comes, the
//! thread sends nothing, so the window does not redraw (rule 7).

use crate::{App, Message};
use iced::Task;
use iced::advanced::widget::operation::Outcome;
use iced::advanced::widget::{Id, Operation};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream};
use iced::{Point, Rectangle, keyboard};
use loom_canvas::{CanvasEvent, CanvasView, PortRef};
use loom_core::{MidiEvent, ModuleId};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::any::Any;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Command-line options for the control channel.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(skip)]
pub struct Options {
    /// Listen for commands on this Unix socket (development only; see
    /// docs/design/control-channel.md). One JSON command per line.
    #[arg(long, value_name = "SOCKET")]
    pub control: Option<PathBuf>,
    /// Run the commands in this file, one JSON command per line, and print
    /// each reply to stdout.
    #[arg(long, value_name = "FILE")]
    pub control_script: Option<PathBuf>,
}

impl Options {
    /// The listeners to run.
    pub fn listeners(&self) -> Vec<Listener> {
        let socket = self.control.clone().map(Listener::Socket);
        let script = self.control_script.clone().map(Listener::Script);
        socket.into_iter().chain(script).collect()
    }
}

/// A command, as sent by a client.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Snapshot,
    Select {
        modules: Vec<u32>,
    },
    Set {
        module: u32,
        param: String,
        value: Option<f32>,
        position: Option<f32>,
    },
    Add {
        kind: String,
        at: Option<[f32; 2]>,
    },
    Remove {
        modules: Vec<u32>,
    },
    Connect {
        from: String,
        to: String,
    },
    Disconnect {
        to: String,
    },
    Undo,
    Redo,
    Open {
        path: PathBuf,
    },
    Save {
        path: PathBuf,
    },
    Note {
        note: u8,
        velocity: Option<u8>,
        on: bool,
    },
    Cc {
        controller: u8,
        value: u8,
        channel: Option<u8>,
    },
    Key {
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    Locate {
        target: String,
    },
    Screenshot {
        path: PathBuf,
    },
    Wait {
        ms: u64,
    },
    Quit,
}

/// What a command returns: fields to add to the reply, or an error.
pub type Answer = Result<Map<String, Value>, String>;

/// A command on its way to the app, with the way back for its answer.
#[derive(Clone)]
pub struct Call {
    pub command: Command,
    reply: Reply,
}

impl std::fmt::Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Call({:?})", self.command)
    }
}

impl Call {
    /// Sends the answer back to the listener. Only the first one counts.
    pub fn answer(&self, answer: Answer) {
        self.reply.send(answer);
    }
}

#[derive(Clone)]
struct Reply(Arc<Mutex<Option<std::sync::mpsc::Sender<Answer>>>>);

impl Reply {
    fn send(&self, answer: Answer) {
        let sender = self.0.lock().ok().and_then(|mut s| s.take());
        if let Some(sender) = sender {
            let _ = sender.send(answer);
        }
    }
}

/// Where commands come from.
#[derive(Debug, Clone)]
pub enum Listener {
    Socket(PathBuf),
    Script(PathBuf),
}

impl Hash for Listener {
    // One listener per source for the whole run: opening another patch
    // keeps it going.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-app control".hash(state);
        match self {
            Self::Socket(path) => ("socket", path).hash(state),
            Self::Script(path) => ("script", path).hash(state),
        }
    }
}

/// Removes the control sockets, as the app exits. The listener threads
/// would remove them too, but the process may end first.
pub fn close(listeners: &[Listener]) {
    for listener in listeners {
        if let Listener::Socket(path) = listener {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// How long a listener waits for the app to answer one command.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs a listener on its own thread and yields each command for the app.
pub fn stream(listener: &Listener) -> impl Stream<Item = Call> + use<> {
    let (tx, rx) = mpsc::channel(1);
    let listener = listener.clone();
    std::thread::spawn(move || match listener {
        Listener::Script(path) => match std::fs::File::open(&path) {
            Ok(file) => {
                let mut tx = tx;
                serve(BufReader::new(file), std::io::stdout(), &mut tx);
            }
            Err(e) => eprintln!("loom-app: control script {}: {e}", path.display()),
        },
        Listener::Socket(path) => listen(&path, tx),
    });
    rx
}

#[cfg(unix)]
fn listen(path: &std::path::Path, mut tx: mpsc::Sender<Call>) {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixListener;
    // A socket left by an earlier run is replaced; any other file is not.
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_socket() {
            let _ = std::fs::remove_file(path);
        } else {
            eprintln!("loom-app: control: {} exists and is not a socket", path.display());
            return;
        }
    }
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("loom-app: control socket {}: {e}", path.display());
            return;
        }
    };
    eprintln!("loom-app: control channel on {}", path.display());
    // Poll, so the thread ends when the app closes.
    let _ = listener.set_nonblocking(true);
    while !tx.is_closed() {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                if let Ok(reader) = stream.try_clone() {
                    serve(BufReader::new(reader), stream, &mut tx);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                eprintln!("loom-app: control socket: {e}");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(not(unix))]
fn listen(_path: &std::path::Path, _tx: mpsc::Sender<Call>) {
    eprintln!("loom-app: --control needs Unix domain sockets, which this platform lacks; use --control-script");
}

/// Reads commands line by line, has the app run each one, and writes each
/// reply as a line. Returns when the input ends or the app closes.
fn serve(input: impl BufRead, mut output: impl Write, tx: &mut mpsc::Sender<Call>) {
    for line in input.lines() {
        let Ok(line) = line else { return };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (id, answer) = run_line(line, tx);
        let mut reply = Map::new();
        if let Some(id) = id {
            reply.insert("id".to_string(), id);
        }
        match answer {
            Ok(fields) => {
                reply.insert("ok".to_string(), Value::Bool(true));
                reply.extend(fields);
            }
            Err(error) => {
                reply.insert("ok".to_string(), Value::Bool(false));
                reply.insert("error".to_string(), Value::String(error));
            }
        }
        if writeln!(output, "{}", Value::Object(reply)).is_err() || output.flush().is_err() {
            return;
        }
        if tx.is_closed() {
            return;
        }
    }
}

/// Parses one line and gets its answer.
fn run_line(line: &str, tx: &mut mpsc::Sender<Call>) -> (Option<Value>, Answer) {
    let mut value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(e) => return (None, Err(format!("not JSON: {e}"))),
    };
    let id = value.as_object_mut().and_then(|v| v.remove("id"));
    let command: Command = match serde_json::from_value(value) {
        Ok(command) => command,
        Err(e) => return (id, Err(e.to_string())),
    };
    if let Command::Wait { ms } = command {
        std::thread::sleep(Duration::from_millis(ms));
        return (id, Ok(Map::new()));
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command,
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    if iced::futures::executor::block_on(tx.send(call)).is_err() {
        return (id, Err("the app closed".to_string()));
    }
    let answer = receiver
        .recv_timeout(ANSWER_TIMEOUT)
        .unwrap_or_else(|_| Err("the app did not answer".to_string()));
    (id, answer)
}

/// What a widget operation found in the live window.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub canvas: Option<CanvasView>,
    pub text: Option<Rectangle>,
}

/// Finds the canvas, and the first text widget that reads `text`.
struct Find {
    text: String,
    found: Found,
}

impl Operation<Found> for Find {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Found>)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(view) = state.downcast_ref::<CanvasView>() {
            self.found.canvas = Some(*view);
        }
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, text: &str) {
        if self.found.text.is_none() && text == self.text {
            self.found.text = Some(bounds);
        }
    }

    fn finish(&self) -> Outcome<Found> {
        Outcome::Some(self.found.clone())
    }
}

/// A module and a name, from `"4:cutoff"`; or a module alone, from `"4"`.
fn split_target(target: &str) -> Option<(u32, Option<&str>)> {
    match target.split_once(':') {
        Some((module, name)) => Some((module.parse().ok()?, Some(name))),
        None => Some((target.parse().ok()?, None)),
    }
}

/// A port by module and name, as in patch files: `"3:out"`.
fn port(app: &App, text: &str, output: bool) -> Result<PortRef, String> {
    let (module, Some(name)) = split_target(text).ok_or_else(|| format!("`{text}` is not MODULE:PORT"))? else {
        return Err(format!("`{text}` is not MODULE:PORT"));
    };
    let node = app
        .controller
        .graph()
        .node(ModuleId(module))
        .ok_or_else(|| format!("no module {module}"))?;
    let ports = if output {
        node.descriptor.outputs
    } else {
        node.descriptor.inputs
    };
    let index = ports.iter().position(|p| p.name == name).ok_or_else(|| {
        let kind = if output { "output" } else { "input" };
        format!("module {module} ({}) has no {kind} `{name}`", node.descriptor.name)
    })?;
    Ok(PortRef { module, output, index })
}

/// A world rectangle in window coordinates.
fn to_window(view: &CanvasView, world: Rectangle) -> Rectangle {
    let a = view.to_window(Point::new(world.x, world.y));
    let b = view.to_window(Point::new(world.x + world.width, world.y + world.height));
    Rectangle::new(a, iced::Size::new(b.x - a.x, b.y - a.y))
}

/// Where a target is in the window: a module, a control or a port by name
/// (rule 10), or any text the window shows.
pub fn locate(app: &App, target: &str, found: &Found) -> Result<Rectangle, String> {
    let Some((module, name)) = split_target(target) else {
        return found
            .text
            .ok_or_else(|| format!("no module, port, control, or text `{target}` in the window"));
    };
    let view = found.canvas.as_ref().ok_or("the window has no patch canvas")?;
    let tile = app
        .scene
        .module(module)
        .ok_or_else(|| format!("no module {module} on the canvas"))?;
    let Some(name) = name else {
        return Ok(to_window(view, tile.bounds()));
    };
    let node = app.controller.graph().node(ModuleId(module)).ok_or("no module")?;
    if let Some(widget) = node.descriptor.panel.widgets.iter().position(|w| w.bind == name) {
        let world = tile.widget_bounds(widget).ok_or("the control is not drawn")?;
        return Ok(to_window(view, world));
    }
    let port = port(app, target, true)
        .or_else(|_| port(app, target, false))
        .map_err(|_| {
            format!(
                "module {module} ({}) has no control or port `{name}`",
                node.descriptor.name
            )
        })?;
    let centre = app.scene.port_position(port).ok_or("the port is not drawn")?;
    let r = loom_canvas::PORT_HIT_RADIUS;
    Ok(to_window(
        view,
        Rectangle::new(
            Point::new(centre.x - r, centre.y - r),
            iced::Size::new(2.0 * r, 2.0 * r),
        ),
    ))
}

fn rect_fields(r: Rectangle) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("x".to_string(), json!(r.center_x()));
    map.insert("y".to_string(), json!(r.center_y()));
    map.insert("width".to_string(), json!(r.width));
    map.insert("height".to_string(), json!(r.height));
    map
}

fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// A key by name: one character, or a named key.
fn key(name: &str) -> Result<keyboard::Key, String> {
    use keyboard::key::Named;
    let named = match name.to_ascii_lowercase().as_str() {
        "escape" => Named::Escape,
        "enter" => Named::Enter,
        "tab" => Named::Tab,
        "space" => Named::Space,
        "delete" => Named::Delete,
        "backspace" => Named::Backspace,
        "up" => Named::ArrowUp,
        "down" => Named::ArrowDown,
        "left" => Named::ArrowLeft,
        "right" => Named::ArrowRight,
        "home" => Named::Home,
        "end" => Named::End,
        "pageup" => Named::PageUp,
        "pagedown" => Named::PageDown,
        _ if name.chars().count() == 1 => return Ok(keyboard::Key::Character(name.to_lowercase().into())),
        _ => return Err(format!("unknown key `{name}`")),
    };
    Ok(keyboard::Key::Named(named))
}

fn modifiers(names: &[String]) -> Result<keyboard::Modifiers, String> {
    names.iter().try_fold(keyboard::Modifiers::default(), |m, name| {
        let add = match name.to_ascii_lowercase().as_str() {
            "cmd" | "command" => keyboard::Modifiers::COMMAND,
            "ctrl" | "control" => keyboard::Modifiers::CTRL,
            "shift" => keyboard::Modifiers::SHIFT,
            "alt" | "option" => keyboard::Modifiers::ALT,
            _ => return Err(format!("unknown modifier `{name}`")),
        };
        Ok(m | add)
    })
}

/// The key press and release, as the keyboard listener sends them.
fn key_events(key: keyboard::Key, modifiers: keyboard::Modifiers) -> [keyboard::Event; 2] {
    let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
    [
        keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        },
        keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
        },
    ]
}

impl App {
    /// Runs a command from the control channel and answers it, now or once
    /// the task it returns has run.
    pub(crate) fn control(&mut self, call: Call) -> Task<Message> {
        let (answer, task) = self.run_command(&call);
        match answer {
            Some(answer) => {
                call.answer(answer);
                task
            }
            // The task answers.
            None => task,
        }
    }

    /// The answer, unless a task gives it later, and the tasks the UI
    /// messages returned.
    fn run_command(&mut self, call: &Call) -> (Option<Answer>, Task<Message>) {
        let mut tasks = Vec::new();
        let mut send = |app: &mut App, message: Message| tasks.push(app.update(message));
        let answer = match call.command.clone() {
            Command::Snapshot => serde_json::to_value(self.ui_snapshot())
                .map(|snapshot| Map::from_iter([("snapshot".to_string(), snapshot)]))
                .map_err(|e| e.to_string()),
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
                .map(|(from, to)| {
                    send(self, Message::Canvas(CanvasEvent::Connect { from, to }));
                    Map::new()
                }),
            Command::Disconnect { to, .. } => port(self, &to, false).map(|to| {
                send(self, Message::Canvas(CanvasEvent::Disconnect(to)));
                Map::new()
            }),
            Command::Undo | Command::Redo => {
                let undo = matches!(call.command, Command::Undo);
                send(self, if undo { Message::Undo } else { Message::Redo });
                let snapshot = self.ui_snapshot();
                Ok(fields(json!({ "undo": snapshot.undo, "redo": snapshot.redo })))
            }
            Command::Open { path, .. } => {
                if self.replace(crate::Source::File(&path)) {
                    self.status = format!("opened {}", path.display());
                    Ok(Map::new())
                } else {
                    Err(self.status.clone())
                }
            }
            Command::Save { path, .. } => {
                self.finish_edits();
                if self.write_to(path) {
                    Ok(Map::new())
                } else {
                    Err(self.status.clone())
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
                let channel = channel.unwrap_or(1).clamp(1, 16) - 1;
                self.send_midi(MidiEvent::control_change(
                    0,
                    channel,
                    controller.min(127),
                    value.min(127),
                ))
            }
            Command::Key {
                key: name,
                modifiers: names,
                ..
            } => key(&name).and_then(|k| Ok((k, modifiers(&names)?))).map(|(k, m)| {
                for event in key_events(k, m) {
                    send(self, Message::Key(event));
                }
                Map::new()
            }),
            Command::Locate { target, .. } => {
                let operation = Find {
                    text: target.clone(),
                    found: Found::default(),
                };
                let call = call.clone();
                let task = iced_runtime::task::widget(operation).map(move |found| Message::Located {
                    call: call.clone(),
                    target: target.clone(),
                    found,
                });
                return (None, Task::batch(tasks.into_iter().chain([task])));
            }
            Command::Screenshot { path, .. } => {
                let call = call.clone();
                let task = iced::window::latest()
                    .then(|id| match id {
                        Some(id) => iced::window::screenshot(id).map(Some),
                        None => Task::done(None),
                    })
                    .map(move |shot| call.answer(save_screenshot(shot, &path)))
                    .discard();
                return (None, Task::batch(tasks.into_iter().chain([task])));
            }
            Command::Wait { .. } => Ok(Map::new()),
            Command::Quit => {
                self.quit = true;
                Ok(Map::new())
            }
        };
        (Some(answer), Task::batch(tasks))
    }

    fn check_modules(&self, modules: &[u32]) -> Result<(), String> {
        match modules
            .iter()
            .find(|&&m| self.controller.graph().node(ModuleId(m)).is_none())
        {
            Some(m) => Err(format!("no module {m}")),
            None => Ok(()),
        }
    }

    fn set_command(
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
        let spec = &descriptor.params[index];
        let position = match (value, position) {
            (Some(value), None) => spec.normalize(value),
            (None, Some(position)) => position,
            _ => return Err("give `value` or `position`, not both".to_string()),
        };
        // As a knob turn: one change, then let go, so it is one undo step.
        send(
            self,
            Message::Param {
                module,
                index,
                position,
            },
        );
        send(self, Message::SliderReleased);
        let stored = self
            .controller
            .graph()
            .node(ModuleId(module))
            .map(|n| n.params[index])
            .unwrap_or_default();
        Ok(fields(json!({ "value": stored })))
    }

    fn add_command(&mut self, kind: &str, at: Option<[f32; 2]>) -> Answer {
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
            None => Err(self.status.clone()),
        }
    }

    fn send_midi(&mut self, event: MidiEvent) -> Answer {
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

    /// Answers a `locate` once the window has been searched.
    pub(crate) fn located(&mut self, call: &Call, target: &str, found: &Found) {
        call.answer(locate(self, target, found).map(rect_fields));
    }
}

/// Saves a window capture as a PNG file.
fn save_screenshot(shot: Option<iced::window::Screenshot>, path: &std::path::Path) -> Answer {
    let shot = shot.ok_or("no window to capture")?;
    crate::screenshot::write_png(path, shot.size, &shot.rgba)?;
    Ok(fields(json!({
        "width": shot.size.width,
        "height": shot.size.height,
        "scale": shot.scale_factor,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(app: &mut App, line: &str) -> Answer {
        let command: Command = serde_json::from_str(line).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let call = Call {
            command,
            reply: Reply(Arc::new(Mutex::new(Some(sender)))),
        };
        let _ = app.control(call);
        receiver.try_recv().expect("answered at once")
    }

    #[test]
    fn parses_commands_and_rejects_unknown_ones() {
        let parse = |s: &str| serde_json::from_str::<Command>(s);
        assert!(matches!(
            parse(r#"{"cmd": "set", "module": 4, "param": "cutoff", "value": 800}"#),
            Ok(Command::Set { module: 4, .. })
        ));
        assert!(parse(r#"{"cmd": "fly"}"#).is_err());
        assert!(parse(r#"{"cmd": "select", "modules": [1], "extra": 1}"#).is_err());
    }

    #[test]
    fn targets_split_into_module_and_name() {
        assert_eq!(split_target("4:cutoff"), Some((4, Some("cutoff"))));
        assert_eq!(split_target("4"), Some((4, None)));
        assert_eq!(split_target("Cutoff"), None);
    }

    #[test]
    fn keys_and_modifiers_by_name() {
        assert_eq!(key("z"), Ok(keyboard::Key::Character("z".into())));
        assert_eq!(key("Escape"), Ok(keyboard::Key::Named(keyboard::key::Named::Escape)));
        assert!(key("hyper").is_err());
        assert_eq!(
            modifiers(&["cmd".to_string(), "shift".to_string()]),
            Ok(keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT)
        );
    }

    /// Serves a script through a fake app thread that answers every call.
    #[test]
    fn serve_writes_one_reply_per_command_with_its_id() {
        let (mut tx, mut rx) = mpsc::channel::<Call>(1);
        let app = std::thread::spawn(move || {
            while let Some(call) = iced::futures::executor::block_on(iced::futures::StreamExt::next(&mut rx)) {
                call.answer(Ok(fields(json!({ "seen": matches!(call.command, Command::Snapshot) }))));
            }
        });
        let input =
            "{\"id\": 1, \"cmd\": \"snapshot\"}\n\n{\"cmd\": \"fly\"}\n{\"id\": \"w\", \"cmd\": \"wait\", \"ms\": 1}\n";
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output, &mut tx);
        drop(tx);
        app.join().unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], json!({ "id": 1, "ok": true, "seen": true }));
        assert_eq!(lines[1]["ok"], json!(false));
        assert_eq!(lines[2], json!({ "id": "w", "ok": true }));
    }

    #[test]
    fn commands_change_the_app_as_the_ui_would() {
        let mut app = App::new(crate::Args {
            patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
            fake_audio: true,
            no_audio: false,
            ..crate::tests::app_args()
        });

        assert_eq!(run(&mut app, r#"{"cmd": "select", "modules": [4]}"#), Ok(Map::new()));
        assert_eq!(app.selection, [4].into());
        assert!(run(&mut app, r#"{"cmd": "select", "modules": [99]}"#).is_err());

        let set = run(
            &mut app,
            r#"{"cmd": "set", "module": 4, "param": "cutoff", "value": 800}"#,
        )
        .unwrap();
        let stored = set["value"].as_f64().unwrap();
        assert!((stored - 800.0).abs() < 0.5, "{stored}");
        let set = run(
            &mut app,
            r#"{"cmd": "set", "module": 4, "param": "cutoff", "position": 0}"#,
        )
        .unwrap();
        assert_eq!(set["value"], json!(20.0));
        let error = run(
            &mut app,
            r#"{"cmd": "set", "module": 4, "param": "nonsense", "value": 1}"#,
        )
        .unwrap_err();
        assert!(error.contains("no parameter `nonsense`"), "{error}");

        // Each set is one undo step, as a knob turn.
        let undone = run(&mut app, r#"{"cmd": "undo"}"#).unwrap();
        assert!(undone["redo"].is_string(), "{undone:?}");
        let cutoff = app.controller.graph().node(ModuleId(4)).unwrap().params[0];
        assert!((cutoff - 800.0).abs() < 0.5, "{cutoff}");

        let snapshot = run(&mut app, r#"{"cmd": "snapshot"}"#).unwrap();
        assert_eq!(snapshot["snapshot"]["selection"], json!([4]));

        assert_eq!(
            run(&mut app, r#"{"cmd": "note", "note": 60, "on": true}"#),
            Ok(Map::new())
        );
        assert_eq!(
            run(&mut app, r#"{"cmd": "cc", "controller": 1, "value": 64}"#),
            Ok(Map::new())
        );

        let added = run(&mut app, r#"{"cmd": "add", "kind": "loom.filter.svf@1"}"#).unwrap();
        let module = added["module"].as_u64().unwrap();
        let from = run(
            &mut app,
            &format!(r#"{{"cmd": "connect", "from": "3:out", "to": "{module}:in"}}"#),
        );
        assert_eq!(from, Ok(Map::new()));
        assert!(
            run(&mut app, r#"{"cmd": "connect", "from": "3:nope", "to": "4:in"}"#)
                .unwrap_err()
                .contains("no output `nope`")
        );
        assert_eq!(
            run(&mut app, &format!(r#"{{"cmd": "remove", "modules": [{module}]}}"#)),
            Ok(Map::new())
        );
        assert!(app.controller.graph().node(ModuleId(module as u32)).is_none());

        assert_eq!(
            run(&mut app, r#"{"cmd": "key", "key": "z", "modifiers": ["cmd"]}"#),
            Ok(Map::new())
        );
        assert!(
            app.controller.graph().node(ModuleId(module as u32)).is_some(),
            "Cmd+Z undid the removal"
        );

        assert_eq!(run(&mut app, r#"{"cmd": "quit"}"#), Ok(Map::new()));
        assert!(app.quit);
    }

    /// `locate` finds a control, a port, and a text where the headless UI
    /// harness finds them.
    #[test]
    fn locate_agrees_with_the_ui_harness() {
        let mut ui = crate::ui_test::Ui::open("subtractive.ron");
        let found = Found {
            canvas: Some(ui.canvas()),
            text: None,
        };
        let knob = locate(&ui.app, "4:cutoff", &found).unwrap();
        assert_eq!(knob.center(), ui.control(4, "cutoff"));
        let out = locate(&ui.app, "8:out", &found).unwrap();
        assert_eq!(out.center(), ui.output(8, "out"));
        let input = locate(&ui.app, "9:right", &found).unwrap();
        assert_eq!(input.center(), ui.input(9, "right"));
        let tile = locate(&ui.app, "4", &found).unwrap();
        assert!(tile.contains(knob.center()));
        assert!(locate(&ui.app, "4:nonsense", &found).is_err());
        let text = Found {
            text: Some(Rectangle::new(Point::new(1.0, 2.0), iced::Size::new(3.0, 4.0))),
            ..found
        };
        assert_eq!(locate(&ui.app, "Cutoff", &text).unwrap().x, 1.0);
        assert!(locate(&ui.app, "Cutoff", &Found::default()).is_err());
    }
}
