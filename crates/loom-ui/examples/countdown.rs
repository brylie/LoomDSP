//! A countdown in the terminal, driven by a driver with no window.
//!
//! The app counts down from five, one step every 200 ms, while a piece of
//! background work runs on a real thread and reports back through the
//! waker. The driver here is a plain loop: it sleeps until the runtime's
//! next deadline, or until a waker unparks it, and prints a line each time
//! the runtime asks it to draw.
//!
//! It shows that a driver needs no window toolkit and no change to the
//! runtime (docs/design/ui-layer.md, section 2, rule 9). A plugin host's
//! window would be one more driver like this one.
//!
//! Run it with `cargo run -p loom-ui --example countdown`.

use loom_ui::effect::{Choice, FileRequest, Image, Job, NoGpu, Question, Reply};
use loom_ui::{
    App, Context, Deadline, Driver, DriverEvent, Effects, Layer, NoTarget, Runtime, View, Waker, WindowSpec,
    WindowState,
};
use loom_ui_core::{Size, UiId};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// How long one step of the countdown lasts.
const STEP: Duration = Duration::from_millis(200);

/// The countdown's messages.
#[derive(Debug)]
enum Message {
    /// Begin: set the first deadline and start the background work.
    Start,
    /// One step of the countdown.
    Step,
    /// The background work finished.
    Loaded(String),
}

/// The app: what is left, when the next step is due, and what the work
/// reported.
struct Countdown {
    left: u32,
    next_step: Option<Instant>,
    status: String,
}

impl App for Countdown {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        match message {
            Message::Start => {
                self.next_step = Some(cx.now() + STEP);
                self.status = "loading".into();
                Effects::work(|| {
                    // Stands for slow work, such as reading a folder.
                    thread::sleep(Duration::from_millis(450));
                    Message::Loaded("loaded on a worker thread".into())
                })
            }
            Message::Step => {
                self.left -= 1;
                if self.left == 0 {
                    self.next_step = None;
                    return Effects::quit();
                }
                self.next_step = Some(cx.now() + STEP);
                Effects::none()
            }
            Message::Loaded(status) => {
                self.status = status;
                Effects::none()
            }
        }
    }

    fn view(&self, view: &mut View) {
        view.panel(UiId::panel("left"), Layer::Panels, &self.left);
        view.panel(UiId::panel("status"), Layer::Panels, &self.status);
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Countdown".into(),
            min_size: None,
        }
    }

    fn next_deadline(&self) -> Option<Deadline<Message>> {
        self.next_step.map(|at| Deadline {
            at,
            message: Message::Step,
        })
    }
}

/// A driver for the terminal. It notes a draw and prints after the turn,
/// and runs work on real threads.
#[derive(Default)]
struct Terminal {
    draw: bool,
}

impl Driver<Message> for Terminal {
    fn redraw(&mut self) {
        self.draw = true;
    }

    fn window(&mut self, spec: &WindowSpec) {
        println!("title: {}", spec.title);
    }

    fn clipboard_write(&mut self, _text: String) {
        unimplemented!("this example has no clipboard");
    }

    fn clipboard_read(&mut self, _reply: Reply<Option<String>, Message>, _waker: &Waker<Message>) {
        unimplemented!("this example has no clipboard");
    }

    fn file_dialog(&mut self, _request: FileRequest, _reply: Reply<Option<PathBuf>, Message>, _waker: &Waker<Message>) {
        unimplemented!("this example shows no dialogs");
    }

    fn ask(&mut self, _question: Question, _reply: Reply<Choice, Message>, _waker: &Waker<Message>) {
        unimplemented!("this example shows no dialogs");
    }

    fn capture(&mut self, _reply: Reply<Result<Image, NoGpu>, Message>, _waker: &Waker<Message>) {
        unimplemented!("this example draws nothing to capture");
    }

    fn work(&mut self, job: Job<Message>, waker: &Waker<Message>) {
        let waker = waker.clone();
        thread::spawn(move || job.run(&waker));
    }

    fn exit(&mut self) {
        println!("quit");
    }
}

fn main() {
    let start = Instant::now();
    let main_thread = thread::current();
    let app = Countdown {
        left: 5,
        next_step: None,
        status: String::new(),
    };
    let mut runtime = Runtime::new(
        app,
        NoTarget,
        WindowState::new(Size::new(80.0, 24.0)),
        Arc::new(move || main_thread.unpark()),
    );
    let mut driver = Terminal::default();
    runtime.waker().send(Message::Start);

    let mut event = DriverEvent::Woken;
    while !runtime.exited() {
        let next = runtime.handle(event, Instant::now(), &mut driver);
        if std::mem::take(&mut driver.draw) {
            let app = runtime.app();
            println!(
                "{:>5} ms  {} left, {}",
                start.elapsed().as_millis(),
                app.left,
                app.status
            );
        }
        if next.again {
            event = DriverEvent::Woken;
            continue;
        }
        match next.deadline {
            Some(at) => thread::park_timeout(at.saturating_duration_since(Instant::now())),
            None if !runtime.exited() => thread::park(),
            None => {}
        }
        event = match next.deadline {
            Some(at) if Instant::now() >= at => DriverEvent::DeadlineReached,
            _ => DriverEvent::Woken,
        };
    }
    println!("{:?}", runtime.counts());
}
