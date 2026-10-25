//! The control channel: commands by name into the running app, and replies
//! with what it shows (`docs/design/control-channel.md`).
//!
//! A listener thread reads one JSON command per line, from a Unix socket
//! (`--control`) or a file (`--control-script`), and hands each to the app
//! as a [`Message::Control`]. The app runs it through the same messages the
//! UI sends and replies once it is applied. The thread then writes the
//! reply line and reads the next command. While no command comes, the
//! thread sends nothing, so the window does not redraw (rule 7).

use crate::{App, Message, settings};
use iced::Task;
use iced::advanced::widget::operation::Outcome;
use iced::advanced::widget::{Id, Operation};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream};
use iced::{Point, Rectangle, keyboard};
use loom_canvas::PanelValues;
use loom_canvas::{CanvasEvent, CanvasView, PortRef};
use loom_core::{Graph, MidiControl, MidiEvent, ModuleId, OutputSafety, WidgetKind};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::any::Any;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    Probe {
        from: String,
        kind: String,
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
    Press {
        button: String,
    },
    Arm {
        target: String,
    },
    Locate {
        target: String,
    },
    Screenshot {
        path: PathBuf,
    },
    Sample {
        target: String,
        ms: u64,
    },
    Wait {
        ms: u64,
    },
    Quit,
    /// The output safety mode, as its radio button in Preferences.
    OutputSafety {
        mode: String,
    },
    /// One pick in one of the Preferences audio lists. `rate` and `buffer`
    /// are a number or `"default"`.
    Audio {
        device: Option<String>,
        rate: Option<Value>,
        buffer: Option<Value>,
    },
    /// A MIDI input's check box in Preferences.
    Midi {
        input: String,
        on: bool,
    },
    /// The toolbar New button, with no unsaved-changes dialog.
    New,
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
            remove_socket(path);
        }
    }
}

/// Removes a socket file, and never any other kind of file: a mistaken
/// `--control` path must not delete a user's file.
fn remove_socket(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket()) {
            let _ = std::fs::remove_file(path);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
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
                // Only a script reads the environment (V1).
                let env = |name: &str| std::env::var(name).ok();
                serve(BufReader::new(file), std::io::stdout(), &mut tx, Some(&env));
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
            remove_socket(path);
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
                    serve(BufReader::new(reader), stream, &mut tx, None);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                eprintln!("loom-app: control socket: {e}");
                break;
            }
        }
    }
    remove_socket(path);
}

#[cfg(not(unix))]
fn listen(_path: &std::path::Path, _tx: mpsc::Sender<Call>) {
    eprintln!("loom-app: --control needs Unix domain sockets, which this platform lacks; use --control-script");
}

/// Reads a variable for a script (`None` on the socket, rule V1).
type Env<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

/// The exit status the app ends with, set when a script stops early
/// (Script variables, V4). 0 until then.
static EXIT_STATUS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Exit status for a script that could not run, as for `loom analyze`.
const COULD_NOT_RUN: i32 = 2;

/// The exit status a script asked for, or 0.
pub fn exit_status() -> i32 {
    EXIT_STATUS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Reads commands line by line, has the app run each one, and writes each
/// reply as a line. Returns when the input ends or the app closes. With
/// `env`, `${NAME}` in string values is replaced first, and an unset
/// variable ends the script and quits the app with status 2.
fn serve(input: impl BufRead, mut output: impl Write, tx: &mut mpsc::Sender<Call>, env: Env) {
    for line in input.lines() {
        let Ok(line) = line else { return };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (id, answer, vars, stop) = run_line(line, tx, env);
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
        if !vars.is_empty() {
            reply.insert("vars".to_string(), Value::Object(vars));
        }
        let write_failed = writeln!(output, "{}", Value::Object(reply)).is_err() || output.flush().is_err();
        // A script that must stop still quits with its status, even when
        // its reply could not be written (stdout closed).
        if stop {
            EXIT_STATUS.store(COULD_NOT_RUN, std::sync::atomic::Ordering::Relaxed);
            let _ = send(Command::Quit, tx);
            return;
        }
        if write_failed {
            return;
        }
        if tx.is_closed() {
            return;
        }
    }
}

/// Parses one line and gets its answer, with the variables it used, and
/// whether the script must stop because one was unset.
fn run_line(line: &str, tx: &mut mpsc::Sender<Call>, env: Env) -> (Option<Value>, Answer, Map<String, Value>, bool) {
    let mut value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(e) => return (None, Err(format!("not JSON: {e}")), Map::new(), false),
    };
    let id = value.as_object_mut().and_then(|v| v.remove("id"));
    let vars = match env.map(|env| crate::script_vars::substitute(&mut value, &env)) {
        Some(Err(unset)) => return (id, Err(format!("{unset}; the script stops here")), Map::new(), true),
        Some(Ok(vars)) => vars,
        None => Map::new(),
    };
    let command: Command = match serde_json::from_value(value) {
        Ok(command) => command,
        Err(e) => return (id, Err(e.to_string()), vars, false),
    };
    (id, send(command, tx), vars, false)
}

/// Has the app run one command and waits for its answer.
fn send(command: Command, tx: &mut mpsc::Sender<Call>) -> Answer {
    if let Command::Wait { ms } = command {
        std::thread::sleep(Duration::from_millis(ms));
        return Ok(Map::new());
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command,
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    if iced::futures::executor::block_on(tx.send(call)).is_err() {
        return Err("the app closed".to_string());
    }
    receiver
        .recv_timeout(ANSWER_TIMEOUT)
        .unwrap_or_else(|_| Err("the app did not answer".to_string()))
}

/// Frames to wait before answering, so the change is drawn: the frame
/// that was already on its way, then the one that draws the change.
const FRAMES_BEFORE_ANSWER: u8 = 2;

/// Frames a `cc` waits for the engine to take the control change, after
/// the frames that draw it: about two seconds at 60 frames per second.
const FRAMES_FOR_ENGINE: u16 = 120;

/// When a command is answered.
enum Then {
    Now(Answer),
    AfterDraw(Step),
}

/// What to do once the window has drawn a command's change.
pub(crate) enum Step {
    Answer(Answer),
    /// A control change sent to the engine: answer once the engine has
    /// taken it and the app has applied it (a mapped control moved, or the
    /// armed parameter learned it). `before` is the serial of the last
    /// control change seen before it was sent. The engine keeps only the
    /// last change, so the command's own is the one that is newer and has
    /// its control and value: a MIDI port's change that arrives first is
    /// not taken for it.
    ControlChange {
        before: Option<u32>,
        control: MidiControl,
        value: u8,
        frames_left: u16,
    },
    Locate(String),
    Screenshot(PathBuf),
    /// A widget's value, recorded every frame until the time is up.
    Sample(Sampling),
    /// A Preferences change: the reply is read once it is drawn, so the
    /// new stream has had a moment to call back.
    Report(Report),
}

/// What a Preferences command replies with.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Report {
    /// `playing` and `status`.
    Safety,
    /// `playing`, `status`, and `audio`.
    Audio,
    /// `status` and the open `inputs`.
    Midi,
}

/// The longest `sample`, well inside the time a listener waits for an
/// answer.
const MAX_SAMPLE_MS: u64 = 30_000;

/// A `sample` in progress: one entry per frame, with the values the canvas
/// draws for one widget.
pub(crate) struct Sampling {
    module: u32,
    widget: usize,
    /// The name the widget is bound to, to tell that it is still there.
    name: String,
    meter: bool,
    ms: u64,
    start: Option<Instant>,
    frames: Vec<Value>,
}

impl Sampling {
    /// Records the frame at `now`. True once the time is up: that frame is
    /// not recorded, so every entry is inside the asked time.
    fn record(&mut self, graph: &Graph, values: &PanelValues, now: Instant) -> Result<bool, String> {
        let start = *self.start.get_or_insert(now);
        let elapsed = now.duration_since(start);
        if elapsed >= Duration::from_millis(self.ms) {
            return Ok(true);
        }
        // The value table keeps a removed module's last values, so ask the
        // graph whether the widget is still drawn.
        let drawn = graph
            .node(ModuleId(self.module))
            .and_then(|node| node.descriptor.panel.widgets.get(self.widget))
            .is_some_and(|w| w.bind == self.name);
        let value = values
            .get(self.module, self.widget)
            .filter(|_| drawn)
            .ok_or_else(|| format!("module {} was removed while sampling", self.module))?;
        // Milliseconds, to the microsecond.
        let mut entry = fields(json!({
            "t": elapsed.as_micros() as f64 / 1000.0,
            "position": value.position,
        }));
        if let Some(live) = value.live {
            entry.insert("live".to_string(), json!(live));
        }
        if self.meter {
            entry.insert("peak".to_string(), json!(value.peak));
        }
        self.frames.push(Value::Object(entry));
        Ok(false)
    }
}

/// A command waiting for the window to draw.
pub(crate) struct Pending {
    frames: u8,
    call: Call,
    step: Step,
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

/// Refuses the file shortcuts (New, Open, Save, Save As): they open a file
/// dialog or ask to save, which would stop the app with nobody to answer
/// (rule 9). The `open` and `save` commands do the same without a dialog.
fn refuse_dialog_keys(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Result<(), String> {
    let keyboard::Key::Character(c) = key else {
        return Ok(());
    };
    if modifiers.command() && matches!(c.as_str(), "n" | "o" | "s") {
        Err(format!(
            "the {c} shortcut opens a dialog; use the `open` or `save` command instead"
        ))
    } else {
        Ok(())
    }
}

/// The toolbar buttons `press` clicks, by stable name. Labels are not
/// used: they change while the app runs ("Hold note" becomes "Release
/// note") and may be reworded.
const BUTTONS: [&str; 4] = ["learn", "hold", "undo", "redo"];

/// The message a toolbar button sends, or why it cannot be pressed: it
/// opens a dialog (rule 9), it is disabled, or there is no such button.
/// Add module is left out: it opens the browser at the pointer, which a
/// script does not have, and the `add` command adds a module directly.
fn press_message(app: &App, name: &str) -> Result<Message, String> {
    let (message, disabled) = match name {
        "learn" => (Message::ToggleLearn, None),
        "hold" => (
            Message::HoldNote,
            app.midi
                .is_none()
                .then_some("Hold note is disabled: no audio is playing the engine"),
        ),
        "undo" => (
            Message::Undo,
            app.history
                .undo_step()
                .is_none()
                .then_some("Undo is disabled: nothing to undo"),
        ),
        "redo" => (
            Message::Redo,
            app.history
                .redo_step()
                .is_none()
                .then_some("Redo is disabled: nothing to redo"),
        ),
        "new" | "open" | "save" | "save-as" => {
            return Err(format!(
                "the {name} button opens a dialog; use the `new`, `open`, or `save` command instead"
            ));
        }
        "add" => return Err("use the `add` command to add a module".to_string()),
        _ => return Err(format!("no toolbar button `{name}`; buttons: {}", BUTTONS.join(", "))),
    };
    match disabled {
        Some(reason) => Err(reason.to_string()),
        None => Ok(message),
    }
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
                Step::Locate(target) => {
                    let operation = Find {
                        text: target.clone(),
                        found: Found::default(),
                    };
                    tasks.push(
                        iced_runtime::task::widget(operation).map(move |found| Message::Located {
                            call: call.clone(),
                            target: target.clone(),
                            found,
                        }),
                    );
                }
                Step::Screenshot(path) => tasks.push(
                    iced::window::latest()
                        .then(|id| match id {
                            Some(id) => iced::window::screenshot(id).map(Some),
                            None => Task::done(None),
                        })
                        .map(move |shot| call.answer(save_screenshot(shot, &path)))
                        .discard(),
                ),
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
    fn run_command(&mut self, call: &Call) -> (Then, Task<Message>) {
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
                .map(|(from, to)| {
                    send(self, Message::Canvas(CanvasEvent::Connect { from, to }));
                    Map::new()
                }),
            Command::Probe { from, kind, .. } => self.probe_command(&from, &kind, &mut send),
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
            } => key(&name)
                .and_then(|k| Ok((k, modifiers(&names)?)))
                .and_then(|(k, m)| refuse_dialog_keys(&k, m).map(|()| (k, m)))
                .map(|(k, m)| {
                    for event in key_events(k, m) {
                        send(self, Message::Key(event));
                    }
                    Map::new()
                }),
            Command::Press { button } => press_message(self, &button).map(|message| {
                send(self, message);
                Map::new()
            }),
            Command::Arm { target } => self.arm_command(&target).map(|message| {
                send(self, message);
                Map::new()
            }),
            Command::Locate { target } => return (Then::AfterDraw(Step::Locate(target)), Task::batch(tasks)),
            Command::Screenshot { path } => return (Then::AfterDraw(Step::Screenshot(path)), Task::batch(tasks)),
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
                    self.status = "new patch".to_string();
                    Ok(Map::new())
                } else {
                    Err(self.status.clone())
                }
            }
        };
        let then = match answer {
            Ok(answer) => Then::AfterDraw(Step::Answer(Ok(answer))),
            Err(error) => Then::Now(Err(error)),
        };
        (then, Task::batch(tasks))
    }

    /// The reply to a Preferences command, read as it is drawn.
    fn preferences_reply(&mut self, report: Report) -> Map<String, Value> {
        let mut reply = Map::new();
        if !matches!(report, Report::Midi) {
            reply.insert("playing".to_string(), Value::String(self.playing_line()));
        }
        reply.insert("status".to_string(), Value::String(self.status.clone()));
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
    fn audio_health(&mut self, remember: bool) -> Value {
        let Some(stats) = self.audio_stats.clone() else {
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
    fn arm_command(&self, target: &str) -> Result<Message, String> {
        if !self.learning {
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
        Ok(Message::LearnParam { module, index })
    }

    /// Checks a `sample` target: a panel widget by the name it is bound
    /// to, a parameter for a control or a display value for a light.
    fn sample_command(&self, target: &str, ms: u64) -> Result<Sampling, String> {
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

    /// Adds a scope or a meter fed by an output, as the probe menu does.
    fn probe_command(&mut self, from: &str, kind: &str, send: &mut impl FnMut(&mut App, Message)) -> Answer {
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
        // As the window would: answers wait for the change to be drawn, and
        // a `cc` for the engine to take it. Frames come about 60 a second.
        for _ in 0..u32::from(FRAMES_BEFORE_ANSWER) + u32::from(FRAMES_FOR_ENGINE) {
            if let Ok(answer) = receiver.try_recv() {
                return answer;
            }
            let _ = app.update(Message::ControlFrame(Instant::now()));
            if app.control_waiting() {
                std::thread::sleep(Duration::from_millis(16));
            }
        }
        receiver.try_recv().expect("answered after drawing")
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

    /// File shortcuts would open a dialog nobody can close (rule 9).
    #[test]
    fn file_shortcuts_are_refused() {
        let cmd = keyboard::Modifiers::COMMAND;
        for c in ["n", "o", "s"] {
            let key = keyboard::Key::Character(c.into());
            assert!(refuse_dialog_keys(&key, cmd).is_err(), "{c}");
            assert!(
                refuse_dialog_keys(&key, cmd | keyboard::Modifiers::SHIFT).is_err(),
                "{c}"
            );
            assert!(refuse_dialog_keys(&key, keyboard::Modifiers::default()).is_ok(), "{c}");
        }
        assert!(refuse_dialog_keys(&keyboard::Key::Character("z".into()), cmd).is_ok());
    }

    /// A mistaken `--control` path that names a regular file is never
    /// deleted, when the app closes or otherwise.
    #[test]
    fn closing_leaves_other_files_alone() {
        let path = std::env::temp_dir().join(format!("loom-control-not-a-socket-{}", std::process::id()));
        std::fs::write(&path, "keep me").unwrap();
        close(&[Listener::Socket(path.clone())]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me");
        std::fs::remove_file(&path).unwrap();
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
        serve(input.as_bytes(), &mut output, &mut tx, None);
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

    /// Serves `script` with the variables in `vars` to a real app, as
    /// `--control-script` does, and returns the replies.
    fn serve_script(app: &mut App, script: &'static str, vars: &'static [(&'static str, &'static str)]) -> Vec<Value> {
        let (mut tx, mut rx) = mpsc::channel::<Call>(1);
        let client = std::thread::spawn(move || {
            let env = |name: &str| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string());
            let mut output = Vec::new();
            serve(script.as_bytes(), &mut output, &mut tx, Some(&env));
            output
        });
        // The app's side: take each call and draw frames until it is answered.
        while !client.is_finished() {
            if let Ok(call) = rx.try_recv() {
                let _ = app.control(call);
            }
            let _ = app.update(Message::ControlFrame(Instant::now()));
            std::thread::sleep(Duration::from_millis(1));
        }
        String::from_utf8(client.join().unwrap())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// A script's `${NAME}` takes its value from the environment, as a
    /// number when the whole string is one variable, and the reply names
    /// the variables used (Script variables, V3 and V6).
    #[test]
    fn a_script_takes_values_from_the_environment() {
        let mut app = App::new(crate::Args {
            patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
            ..crate::tests::app_args()
        });
        let script = r#"{"cmd": "set", "module": 4, "param": "cutoff", "value": "${LOOM_TEST_VALUE}"}"#;
        let replies = serve_script(&mut app, script, &[("LOOM_TEST_VALUE", "800")]);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["ok"], json!(true), "{}", replies[0]);
        assert_eq!(replies[0]["vars"], json!({"LOOM_TEST_VALUE": "800"}));
        let cutoff = app.controller.graph().node(ModuleId(4)).unwrap().params[0];
        assert!((cutoff - 800.0).abs() < 0.01, "cutoff is {cutoff}");
    }

    /// An unset variable ends the script with an error naming it, asks the
    /// app to quit, and sets exit status 2; later lines do not run (V4).
    #[test]
    fn an_unset_variable_stops_the_script() {
        let mut app = App::new(crate::tests::app_args());
        let script = "{\"cmd\": \"audio\", \"device\": \"${LOOM_TEST_UNSET}\"}\n{\"cmd\": \"snapshot\"}\n";
        let replies = serve_script(&mut app, script, &[]);
        assert_eq!(replies.len(), 1, "the snapshot never ran");
        assert_eq!(replies[0]["ok"], json!(false));
        assert!(
            replies[0]["error"].as_str().unwrap().contains("LOOM_TEST_UNSET"),
            "{}",
            replies[0]
        );
        assert!(app.quit, "the app was asked to quit");
        assert_eq!(exit_status(), COULD_NOT_RUN);
    }

    /// Output that refuses every write, like a closed stdout.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    /// An unset variable still quits the app with status 2 when its reply
    /// cannot be written (V4).
    #[test]
    fn an_unset_variable_quits_even_when_stdout_is_closed() {
        let (mut tx, mut rx) = mpsc::channel::<Call>(1);
        let app = std::thread::spawn(move || {
            let mut quit = false;
            while let Some(call) = iced::futures::executor::block_on(iced::futures::StreamExt::next(&mut rx)) {
                quit |= matches!(call.command, Command::Quit);
                call.answer(Ok(Map::new()));
            }
            quit
        });
        let env = |_: &str| None;
        serve(
            "{\"cmd\": \"audio\", \"device\": \"${LOOM_TEST_UNSET}\"}\n".as_bytes(),
            Closed,
            &mut tx,
            Some(&env),
        );
        drop(tx);
        assert!(app.join().unwrap(), "the app was asked to quit");
        assert_eq!(exit_status(), COULD_NOT_RUN);
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
        assert_eq!(set["value"], json!(800.0));
        assert_eq!(app.controller.graph().node(ModuleId(4)).unwrap().params[0], 800.0);
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
        assert_eq!(cutoff, 800.0);

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
        // Another control change queued just before, as from a MIDI port:
        // `cc` still waits for its own.
        assert_eq!(app.send_midi(MidiEvent::control_change(0, 0, 2, 10)), Ok(Map::new()));
        assert_eq!(
            run(&mut app, r#"{"cmd": "cc", "controller": 3, "value": 99}"#),
            Ok(Map::new())
        );
        let seen = app.controller.last_control_change().unwrap();
        assert_eq!((seen.control.controller, seen.value), (3, 99));

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

        // `probe` adds a scope fed by the output, as the probe menu does.
        let scope = run(&mut app, r#"{"cmd": "probe", "from": "3:out", "kind": "scope"}"#).unwrap();
        let scope = ModuleId(scope["module"].as_u64().unwrap() as u32);
        assert_eq!(
            app.controller.graph().node(scope).unwrap().descriptor.id,
            "loom.display.scope"
        );
        assert!(
            app.controller
                .graph()
                .cables()
                .any(|(from, to)| from.module == ModuleId(3) && to.module == scope)
        );
        assert!(
            run(&mut app, r#"{"cmd": "probe", "from": "3:out", "kind": "spectrum"}"#)
                .unwrap_err()
                .contains("not scope or meter")
        );
        assert_eq!(run(&mut app, r#"{"cmd": "undo"}"#).map(|_| ()), Ok(()));
        assert!(app.controller.graph().node(scope).is_none());

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

    /// The MIDI learn check from #256: turn learn mode on, arm a control,
    /// send a control change, turn learn mode off, and read the mapping.
    #[test]
    fn a_script_maps_a_control_in_learn_mode() {
        let mut app = App::new(crate::Args {
            patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
            fake_audio: true,
            no_audio: false,
            ..crate::tests::app_args()
        });
        let ok = Ok(Map::new());
        assert!(
            run(&mut app, r#"{"cmd": "arm", "target": "4:cutoff"}"#)
                .unwrap_err()
                .contains("press `learn` first")
        );
        assert_eq!(run(&mut app, r#"{"cmd": "press", "button": "learn"}"#), ok);
        assert!(app.learning);
        assert_eq!(run(&mut app, r#"{"cmd": "select", "modules": [4]}"#), ok);
        assert_eq!(run(&mut app, r#"{"cmd": "arm", "target": "4:cutoff"}"#), ok);
        let row = &run(&mut app, r#"{"cmd": "snapshot"}"#).unwrap()["snapshot"]["inspector"]["module"]["params"][0];
        assert_eq!(row["mode"]["learn"]["armed"], json!(true), "{row}");

        // `cc` answers once the engine has taken the change and the armed
        // parameter has learned it, so turning learn mode off next cannot
        // lose the mapping.
        assert_eq!(run(&mut app, r#"{"cmd": "cc", "controller": 21, "value": 64}"#), ok);
        assert!(app.armed.is_none(), "the armed parameter learned the control");
        assert_eq!(run(&mut app, r#"{"cmd": "press", "button": "learn"}"#), ok);
        assert!(!app.learning);
        assert_eq!(run(&mut app, r#"{"cmd": "press", "button": "hold"}"#), ok);
        assert!(app.holding);

        let row = &run(&mut app, r#"{"cmd": "snapshot"}"#).unwrap()["snapshot"]["inspector"]["module"]["params"][0];
        let mapping = row["mapping"].as_str().unwrap_or_default();
        assert!(mapping.contains("21"), "{row}");
    }

    /// `sample` records a knob's live value once per frame (#255): with the
    /// filter envelope modulating Resonance and a note held, the live dot
    /// moves. Then the app asks for no more frames (rule 7).
    #[test]
    fn sample_follows_a_live_value_frame_by_frame() {
        let mut app = App::new(crate::Args {
            patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
            fake_audio: true,
            no_audio: false,
            ..crate::tests::app_args()
        });
        let resonance = app
            .controller
            .graph()
            .node(ModuleId(4))
            .unwrap()
            .descriptor
            .param_index("resonance")
            .unwrap();
        let _ = app.update(Message::Assign {
            source: loom_core::OutputRef {
                module: ModuleId(5),
                port: 0,
            },
            module: 4,
            index: resonance,
        });
        assert_eq!(
            run(&mut app, r#"{"cmd": "note", "note": 60, "on": true}"#),
            Ok(Map::new())
        );

        let sample = run(&mut app, r#"{"cmd": "sample", "target": "4:resonance", "ms": 300}"#).unwrap();
        assert!(!app.control_waiting());
        let frames = sample["frames"].as_array().unwrap();
        // A busy CI machine draws few frames in 300 ms; two are enough to
        // see the live value move.
        assert!(frames.len() >= 2, "{sample:?}");
        let times: Vec<f64> = frames.iter().map(|f| f["t"].as_f64().unwrap()).collect();
        assert_eq!(times[0], 0.0);
        assert!(times.windows(2).all(|t| t[0] < t[1]), "{times:?}");
        assert!(times.iter().all(|&t| t < 300.0), "{times:?}");
        let live: Vec<f64> = frames.iter().map(|f| f["live"].as_f64().unwrap()).collect();
        assert!(
            live.windows(2).any(|l| l[0] != l[1]),
            "the live value did not move: {live:?}"
        );
        assert!(
            frames.iter().all(|f| f.get("peak").is_none()),
            "only meters have a peak"
        );

        let error = run(&mut app, r#"{"cmd": "sample", "target": "4:nonsense", "ms": 100}"#).unwrap_err();
        assert!(error.contains("no control, light, or meter `nonsense`"), "{error}");
        let error = run(&mut app, r#"{"cmd": "sample", "target": "4:resonance", "ms": 0}"#).unwrap_err();
        assert!(error.contains("from 1 to"), "{error}");
        assert!(!app.control_waiting());
    }

    /// Removing the sampled module ends the sample with an error, not with
    /// the last values it drew, which the value table still holds.
    #[test]
    fn removing_the_sampled_module_ends_the_sample() {
        let mut app = App::new(crate::Args {
            patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
            ..crate::tests::app_args()
        });
        let (sender, receiver) = std::sync::mpsc::channel();
        let call = Call {
            command: Command::Sample {
                target: "4:resonance".to_string(),
                ms: 10_000,
            },
            reply: Reply(Arc::new(Mutex::new(Some(sender)))),
        };
        let _ = app.control(call);
        for _ in 0..=FRAMES_BEFORE_ANSWER {
            let _ = app.update(Message::ControlFrame(Instant::now()));
        }
        assert!(receiver.try_recv().is_err(), "still sampling");
        let _ = app.update(Message::Canvas(CanvasEvent::Delete(vec![4])));
        let _ = app.update(Message::ControlFrame(Instant::now()));
        let error = receiver.try_recv().unwrap().unwrap_err();
        assert!(error.contains("removed while sampling"), "{error}");
        assert!(!app.control_waiting());
    }

    /// `press` refuses what a click could not do: dialogs (rule 9),
    /// disabled buttons, and unknown names.
    #[test]
    fn press_refuses_dialogs_disabled_buttons_and_unknown_names() {
        // No audio: Hold note is disabled. Nothing done yet: nor are Undo
        // and Redo.
        let mut app = App::new(crate::tests::app_args());
        for name in ["new", "open", "save", "save-as"] {
            let error = run(&mut app, &format!(r#"{{"cmd": "press", "button": "{name}"}}"#)).unwrap_err();
            assert!(error.contains("opens a dialog"), "{name}: {error}");
        }
        for (name, reason) in [
            ("hold", "Hold note is disabled"),
            ("undo", "nothing to undo"),
            ("redo", "nothing to redo"),
        ] {
            let error = run(&mut app, &format!(r#"{{"cmd": "press", "button": "{name}"}}"#)).unwrap_err();
            assert!(error.contains(reason), "{name}: {error}");
        }
        let error = run(&mut app, r#"{"cmd": "press", "button": "MIDI learn"}"#).unwrap_err();
        assert!(error.contains("buttons: learn, hold, undo, redo"), "{error}");
        let error = run(&mut app, r#"{"cmd": "press", "button": "add"}"#).unwrap_err();
        assert!(error.contains("`add` command"), "{error}");
    }

    /// A command waiting to be drawn keeps its answer when another patch
    /// is opened before the frame comes.
    #[test]
    fn opening_a_patch_keeps_waiting_answers() {
        let mut app = App::new(crate::tests::app_args());
        let (sender, receiver) = std::sync::mpsc::channel();
        let call = Call {
            command: Command::Select { modules: Vec::new() },
            reply: Reply(Arc::new(Mutex::new(Some(sender)))),
        };
        let _ = app.control(call);
        assert!(app.control_waiting());
        let patch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/sine.ron");
        assert!(app.replace(crate::Source::File(&patch)));
        for _ in 0..FRAMES_BEFORE_ANSWER {
            let _ = app.update(Message::ControlFrame(Instant::now()));
        }
        assert_eq!(receiver.try_recv(), Ok(Ok(Map::new())));
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

    /// An app on the fake host, playing the subtractive patch, with its
    /// preferences file in a folder of its own.
    fn on_fake_devices(name: &str, fake: &std::rc::Rc<crate::devices::fake::Fake>) -> App {
        let dir = std::env::temp_dir().join(format!("loom-control-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let args = crate::Args {
            patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
            no_audio: false,
            no_recovery: true,
            devices: crate::devices::Options {
                preferences: Some(dir.join(crate::preferences::FILE_NAME)),
                ..crate::devices::Options::default()
            },
            ..crate::tests::app_args()
        };
        App::boot_with(
            args,
            &mut crate::recovery::Dialogs,
            std::rc::Rc::new(crate::devices::fake::Shared(fake.clone())),
        )
    }

    /// The Preferences commands go through the window's messages: the
    /// device, rate, buffer, output safety, and MIDI inputs change as a
    /// click would change them, and each reply says what plays now
    /// (control-channel.md, Devices and stream health).
    #[test]
    fn preferences_commands_change_what_the_window_shows() {
        use crate::devices::fake::{self, Fake};
        use crate::preferences::{BufferChoice, DeviceChoice, RateChoice};
        let fake = std::rc::Rc::new(Fake::new(
            &[fake::device("Fake A"), fake::device("Fake B")],
            &["Keys", "Pad"],
        ));
        let mut app = on_fake_devices("preferences", &fake);

        let reply = run(&mut app, r#"{"cmd": "audio", "device": "fake b"}"#).unwrap();
        assert_eq!(app.rig.shown_audio().device, DeviceChoice::Named("Fake B".into()));
        assert_eq!(fake.opened(), ["Fake A", "Fake B"]);
        assert!(
            reply["playing"]
                .as_str()
                .unwrap()
                .starts_with("Playing on \"Fake B\" at 48000 Hz"),
            "{reply:?}"
        );
        assert!(reply["status"].is_string());
        assert_eq!(reply["audio"]["device"], json!("Fake B"));

        let reply = run(&mut app, r#"{"cmd": "audio", "rate": 44100}"#).unwrap();
        assert_eq!(app.rig.shown_audio().sample_rate, RateChoice::Hz(44_100));
        assert!(reply["playing"].as_str().unwrap().contains("44100 Hz"), "{reply:?}");
        let reply = run(&mut app, r#"{"cmd": "audio", "buffer": 64}"#).unwrap();
        assert_eq!(app.rig.shown_audio().buffer, BufferChoice::Frames(64));
        assert!(reply["playing"].as_str().unwrap().contains("64 frames"), "{reply:?}");

        // What the window does not offer is refused, with what it offers.
        let error = run(&mut app, r#"{"cmd": "audio", "rate": 96000}"#).unwrap_err();
        assert!(error.contains("44100, 48000"), "{error}");
        let error = run(&mut app, r#"{"cmd": "audio", "buffer": 100}"#).unwrap_err();
        assert!(error.contains("32, 64"), "{error}");
        let error = run(&mut app, r#"{"cmd": "audio", "device": "Fake"}"#).unwrap_err();
        assert!(error.contains("several"), "{error}");
        let error = run(&mut app, r#"{"cmd": "audio", "device": "Nowhere"}"#).unwrap_err();
        assert!(error.contains("\"Fake A\", \"Fake B\""), "{error}");
        assert!(run(&mut app, r#"{"cmd": "audio", "rate": 48000, "buffer": 64}"#).is_err());
        assert!(run(&mut app, r#"{"cmd": "audio", "rate": "fast"}"#).is_err());
        assert_eq!(fake.opened().len(), 4, "a refused change opens nothing");

        run(&mut app, r#"{"cmd": "audio", "buffer": "default"}"#).unwrap();
        assert_eq!(app.rig.shown_audio().buffer, BufferChoice::DeviceDefault);
        run(&mut app, r#"{"cmd": "audio", "rate": "default"}"#).unwrap();
        assert_eq!(app.rig.shown_audio().sample_rate, RateChoice::DeviceDefault);
        run(&mut app, r#"{"cmd": "audio", "device": "default"}"#).unwrap();
        assert_eq!(app.rig.shown_audio().device, DeviceChoice::SystemDefault);
        assert_eq!(fake.opened().last().map(String::as_str), Some("Fake A"));
        assert!(app.rig.window.is_none(), "the commands leave the window closed");

        let reply = run(&mut app, r#"{"cmd": "output_safety", "mode": "clean"}"#).unwrap();
        assert_eq!(app.output_safety, OutputSafety::Clean);
        assert!(
            reply["playing"].as_str().unwrap().starts_with("Playing on"),
            "{reply:?}"
        );
        assert!(run(&mut app, r#"{"cmd": "output_safety", "mode": "loud"}"#).is_err());

        let reply = run(&mut app, r#"{"cmd": "midi", "input": "pad", "on": false}"#).unwrap();
        assert_eq!(reply["inputs"], json!(["Keys"]));
        assert!(!fake.is_open("Pad"));
        let reply = run(&mut app, r#"{"cmd": "midi", "input": "Pad", "on": true}"#).unwrap();
        assert_eq!(reply["inputs"], json!(["Keys", "Pad"]));
        assert!(run(&mut app, r#"{"cmd": "midi", "input": "Drums", "on": true}"#).is_err());

        assert!(!app.controller.graph().is_empty());
        assert_eq!(run(&mut app, r#"{"cmd": "new"}"#), Ok(Map::new()));
        assert!(app.controller.graph().is_empty());
        assert!(fake.is_open("Keys"), "New keeps the MIDI inputs");
    }

    /// With `--fake-audio` there are no device lists, so `audio` is refused
    /// (rule S6). Output safety still changes.
    #[test]
    fn audio_is_refused_with_no_device() {
        let mut app = App::new(crate::Args {
            fake_audio: true,
            no_audio: false,
            ..crate::tests::app_args()
        });
        let error = run(&mut app, r#"{"cmd": "audio", "rate": 48000}"#).unwrap_err();
        assert!(error.contains("--fake-audio"), "{error}");
        assert!(run(&mut app, r#"{"cmd": "output_safety", "mode": "off"}"#).is_ok());
        assert_eq!(app.output_safety, OutputSafety::Off);
    }

    /// A device can start, call back a few times, and then stop with no
    /// error, while the app still says it plays. The snapshot's `audio`
    /// field shows it: `advanced` turns false and `callbacks` stops.
    #[test]
    fn the_snapshot_sees_a_device_stop_calling_back() {
        use crate::devices::fake::{self, Fake, STALL_AFTER};
        let mut fake = Fake::new(&[fake::device("Fake A"), fake::device("Stuck")], &[]);
        fake.stalls = vec!["Stuck".to_string()];
        let fake = std::rc::Rc::new(fake);
        let mut app = on_fake_devices("stall", &fake);

        let audio = |answer: Answer| answer.unwrap()["audio"].clone();
        let first = audio(run(&mut app, r#"{"cmd": "snapshot"}"#));
        assert_eq!(first["device"], json!("Fake A"));
        assert_eq!(first["rate"], json!(48_000));

        run(&mut app, r#"{"cmd": "audio", "device": "Stuck"}"#).unwrap();
        let started = audio(run(&mut app, r#"{"cmd": "snapshot"}"#));
        assert_eq!(started["device"], json!("Stuck"));
        assert_eq!(started["callbacks"], json!(STALL_AFTER));
        assert_eq!(started["advanced"], json!(true), "a new stream counts from its start");
        assert_eq!(started["buffer"], json!(256));
        assert_eq!(started["stream_errors"], json!(0));
        assert_eq!(started["start_overloads"], json!(0));

        let stalled = audio(run(&mut app, r#"{"cmd": "snapshot"}"#));
        assert_eq!(stalled["callbacks"], json!(STALL_AFTER));
        assert_eq!(stalled["advanced"], json!(false));
        // The line still says it plays: only the counters show the stall.
        assert!(
            app.playing_line().starts_with("Playing on \"Stuck\""),
            "{}",
            app.playing_line()
        );
    }
}
