//! The control channel: commands by name into the running app, and replies
//! with what it shows (`docs/design/control-channel.md`).
//!
//! A listener thread reads one JSON command per line, from a Unix socket
//! (`--control`) or a file (`--control-script`), and hands each to the app
//! as a [`Message::Control`]. The app runs it through the same messages the
//! UI sends and replies once it is applied. The thread then writes the
//! reply line and reads the next command. While no command comes, the
//! thread sends nothing, so the window does not redraw (rule 7).

use crate::pointer::{Aim, Step as PointerStep};
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

mod resolve;
mod run;
mod transport;

use resolve::*;
pub use resolve::{Found, locate};
pub use transport::{Listener, close, exit_status, stream};

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
    /// Runs panic (`docs/design/panic.md`).
    Panic,
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
    /// Clicks a toolbar button by its fixed command name.
    Toolbar {
        button: String,
    },
    /// A mouse button down (Pointer commands).
    Press {
        button: Option<String>,
        target: Option<String>,
        x: Option<f32>,
        y: Option<f32>,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    /// A mouse button up, at the pointer or at the target.
    Release {
        button: Option<String>,
        target: Option<String>,
        x: Option<f32>,
        y: Option<f32>,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    /// Moves the pointer.
    Hover {
        target: Option<String>,
        x: Option<f32>,
        y: Option<f32>,
    },
    /// Press at `from`, move in `steps` to `to`, release. Each end is a
    /// target or `[x, y]`.
    Drag {
        from: Value,
        to: Value,
        steps: Option<u32>,
        button: Option<String>,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    /// A wheel step, in lines. Negative `dy` is down.
    Scroll {
        target: Option<String>,
        x: Option<f32>,
        y: Option<f32>,
        dy: f32,
        dx: Option<f32>,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    Arm {
        target: String,
    },
    Locate {
        target: String,
    },
    /// The window, or the area around a target (`margin` in logical
    /// pixels), or a rectangle `[x, y, width, height]`.
    Screenshot {
        path: PathBuf,
        target: Option<String>,
        margin: Option<f32>,
        rect: Option<[f32; 4]>,
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
    /// A window capture, of the whole window or of an area.
    Screenshot(PathBuf, Area),
    /// Pointer or key events on their way to the view, by the command's
    /// serial: answered once they have all gone in and been drawn. True
    /// when the reply gives the pointer position.
    Pointer(u64, bool),
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

/// Ends the running `sample`s with an error and keeps the other waiting
/// commands, for when another patch is opened. A sample would otherwise go
/// on with whatever module has the same ID and widget name in the new
/// patch (rule 14).
pub(crate) fn end_samples(pending: Vec<Pending>) -> Vec<Pending> {
    let mut kept = Vec::new();
    for waiting in pending {
        if matches!(waiting.step, Step::Sample(_)) {
            waiting
                .call
                .answer(Err("another patch was opened while sampling".to_string()));
        } else {
            kept.push(waiting);
        }
    }
    kept
}

/// What part of the window a `screenshot` saves.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Area {
    Window,
    /// Logical window pixels.
    Rect(Rectangle),
    /// The area around a target, found once the window is drawn.
    Target {
        target: String,
        margin: f32,
    },
}

/// What to do once the window has been searched for a target.
#[derive(Debug, Clone)]
pub enum AfterSearch {
    /// Answer a `locate`.
    Locate,
    /// Save the area around it, `margin` logical pixels wider on each side.
    Screenshot { path: PathBuf, margin: f32 },
}

#[cfg(test)]
mod tests;
