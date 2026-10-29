//! `loom-app`: the LoomDSP desktop app.
//!
//! Phase 1 prototype: the patch canvas (ADR-0010), an inspector for the
//! selected module, live audio and MIDI, and a stats bar for measuring the
//! canvas against the performance gates in NFR-PERF-04.

mod alerts;
#[cfg(test)]
mod autosave_tests;
mod bench;
mod browser;
mod clipboard;
mod control;
mod devices;
mod edits;
mod files;
mod inspector;
mod layout;
mod learn;
mod lights;
mod live;
mod modulation;
mod monitor;
mod panic;
mod param;
mod pointer;
mod preferences;
mod probe_menu;
#[cfg(test)]
mod qa_tests;
mod recovery;
mod scene;
mod screenshot;
mod script_vars;
mod settings;
mod snapshot;
mod targets;
#[cfg(test)]
mod ui_test;
mod undo;

use clap::Parser;
use iced::widget::{button, column, container, mouse_area, row, rule, space, stack, text, toggler, tooltip};
use iced::{Color, Element, Length, Point, Size, Subscription, Task, Theme};
use live::Live;
use loom_canvas::{CanvasEvent, LearnView, PanelValues, PatchCanvas, Scene};
use loom_core::{
    AssignmentId, Controller, EngineConfig, EngineParts, InputRef, MidiEvent, ModuleDescriptor, ModuleId, OutputRef,
    OutputSafety, ProbeId, TRANSPORT_MODULE, create_engine,
};
use loom_host_desktop::{AudioOutput, CallbackStats, RunningAudio, SharedMidi};
use loom_patch::Patch;
use modulation::PolarityChoice;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The modules a patch can use: the built-ins, and with `test_modules`
/// also the `test.` modules from `loom_test::modules`, which have widgets
/// no built-in module has yet. The app's own tests always get them.
fn registry(test_modules: bool) -> Arc<loom_core::Registry> {
    if !(test_modules || cfg!(test)) {
        return loom_modules::registry();
    }
    let mut registry = loom_core::Registry::new();
    loom_modules::register_all(&mut registry).expect("built-in modules are valid and unique");
    loom_test::modules::register(&mut registry).expect("test modules are valid and unique");
    Arc::new(registry)
}

#[derive(Parser, Debug, Clone)]
#[command(name = "loom-app", version, about = "LoomDSP desktop app")]
struct Args {
    /// Patch file (.ron) to open.
    patch: Option<PathBuf>,
    /// Instead of a file, generate a test patch with about this many modules.
    #[arg(long, value_name = "N")]
    generate: Option<usize>,
    /// Do not open an audio device.
    #[arg(long)]
    no_audio: bool,
    /// Open no audio device, but play the engine from a timer at 48 kHz, so
    /// live values, lights, and meters move. Nothing is heard. For machines
    /// without audio and for scripts.
    #[arg(long, conflicts_with = "no_audio")]
    fake_audio: bool,
    /// Save what the engine plays to this WAV file (32-bit float, stereo),
    /// until the app quits. Opening a patch, New, and a device change keep
    /// recording into the same file; a device at another sample rate ends
    /// the recording.
    #[arg(long, value_name = "WAV", conflicts_with = "no_audio")]
    record: Option<PathBuf>,
    /// Measuring the frame rate: --bench, --measure, --bench-seconds.
    #[command(flatten)]
    bench: bench::Options,
    /// The control channel: --control, --control-script.
    #[command(flatten)]
    control: control::Options,
    /// Safety stage on the output for this run: low-latency, clean, or off
    /// (ADR-0019). Without it, the saved preference applies. Not saved.
    #[arg(long)]
    output_safety: Option<OutputSafety>,
    /// Devices and the preferences file: --preferences, --device,
    /// --sample-rate, --buffer, --midi, --no-midi.
    #[command(flatten)]
    devices: devices::Options,
    /// Saving a still image of the canvas instead of opening a window.
    #[command(flatten)]
    view: screenshot::Options,
    /// Print what the UI shows (tiles, inspector, status bar, undo steps,
    /// window title) as JSON and exit, without a window, audio, or MIDI.
    #[arg(long, conflicts_with = "screenshot", group = "inspect")]
    dump_ui: bool,
    /// With --dump-ui or --screenshot-window: select this module, so the
    /// inspector shows it. Repeat to select several.
    #[arg(long, value_name = "MODULE", requires = "inspect")]
    select: Vec<u32>,
    /// With --dump-ui or --screenshot-window: first run the engine for N
    /// frames at 48 kHz on silent input, so live values, lights, and meters
    /// show where the engine left them.
    #[arg(long, value_name = "N", requires = "inspect")]
    frames: Option<usize>,
    /// Keep autosave recovery copies in this folder instead of the
    /// per-user one (docs/design/autosave.md). For tests and scripts.
    #[arg(long, value_name = "FOLDER", conflicts_with = "no_recovery")]
    recovery_dir: Option<PathBuf>,
    /// No autosave and no crash recovery for this run.
    #[arg(long)]
    no_recovery: bool,
    /// For developers: also load the `test.` modules, which have every
    /// panel widget, and list them in the module browser. A patch that
    /// uses them, such as examples/dev/test-widgets.ron, loads only with
    /// this flag.
    #[arg(long)]
    test_modules: bool,
}

impl Args {
    /// Where the patch comes from.
    fn source(&self) -> Source<'_> {
        match (&self.patch, self.generate) {
            (_, Some(n)) => Source::Generated(n),
            (Some(path), None) => Source::File(path),
            (None, None) => Source::Empty,
        }
    }

    /// Where this run keeps recovery copies, or `None` when it keeps none:
    /// with --no-recovery, and in runs nobody watches, which must not stop
    /// at a dialog or use up a user's copy (`docs/design/autosave.md`).
    fn recovery_root(&self) -> Option<Result<PathBuf, String>> {
        let unattended = self.no_recovery || !self.control.listeners().is_empty() || self.bench.reporter().is_some();
        if unattended {
            return None;
        }
        match &self.recovery_dir {
            Some(dir) => Some(Ok(dir.clone())),
            // Tests never touch the real folder.
            None if cfg!(test) => None,
            None => Some(recovery::default_root()),
        }
    }
}

fn main() -> iced::Result {
    let args = Args::parse();
    if let Some(path) = &args.view.screenshot {
        if let Err(e) = screenshot::run(&args, path) {
            eprintln!("loom-app: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(path) = &args.view.screenshot_window {
        if let Err(e) = screenshot::run_window(&args, path) {
            eprintln!("loom-app: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if args.dump_ui {
        match snapshot::run(&args) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("loom-app: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let result = iced::application(move || App::new(args.clone()), App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .exit_on_close_request(false)
        .window_size(WINDOW)
        .antialiasing(false)
        .run();
    // A control script that stopped early asks for its own status.
    match control::exit_status() {
        0 => result,
        status => std::process::exit(status),
    }
}

/// The window size the app opens with.
const WINDOW: Size = Size::new(1440.0, 900.0);

#[derive(Debug, Clone)]
enum Message {
    Canvas(CanvasEvent),
    Param {
        module: u32,
        index: usize,
        position: f32,
    },
    /// The toolbar asks for the module browser at the centre of the canvas.
    OpenBrowser,
    /// A message for the open module browser.
    Browser(browser::Message),
    /// A message for the open probe menu.
    ProbeMenu(probe_menu::Message),
    /// Add a scope or a meter fed by an output, placed beside `world`
    /// (`docs/design/panels.md`, Add scope and Add meter). The probe menu
    /// and the control channel send it.
    AddProbe {
        /// What to add.
        probe: probe_menu::Probe,
        /// The output that feeds it.
        output: loom_canvas::PortRef,
        /// Where the menu was opened, in world units.
        world: Point,
    },
    HoldNote,
    /// Silence everything and clear stuck notes (`docs/design/panic.md`).
    Panic,
    /// Look for MIDI inputs plugged in or out (`docs/design/device-changes.md`).
    MidiPoll,
    /// Look for audio device loss, silent stalls, and reconnects (`docs/design/device-changes.md`).
    AudioPoll,
    /// File commands (`docs/design/files.md`).
    New,
    Open,
    Save,
    SaveAs,
    /// The window asked to close.
    CloseRequested,
    Measure(bool),
    /// The --bench-seconds run is over.
    BenchDone,
    /// A command from the control channel.
    Control(control::Call),
    /// A frame while control commands wait to be drawn, or sample it.
    ControlFrame(Instant),
    /// The window was searched for a `locate` command.
    Located {
        call: control::Call,
        target: String,
        found: control::Found,
        then: control::AfterSearch,
    },
    Animate(bool),
    Frame(Instant),
    Tick,
    /// A frame while live modulated values are on screen.
    LiveFrame,
    /// New brightness for lights on screen.
    Lights(Vec<(lights::Light, f32)>),
    /// New levels for meter probes.
    Meters(Vec<(ProbeId, live::Level)>),
    /// Turn MIDI learn mode on or off.
    ToggleLearn,
    /// In learn mode, arm a parameter from the inspector.
    LearnParam {
        module: u32,
        index: usize,
    },
    /// Time to check whether a recovery copy is due.
    Autosave,
    /// Remove a parameter's MIDI mapping.
    Unmap {
        module: u32,
        index: usize,
    },
    /// A MIDI control change arrived.
    Midi,
    /// A key: the undo and redo shortcuts, and Escape in learn mode. Ignored
    /// while the module browser is open.
    Key(iced::keyboard::Event),
    /// Take back the last step (`docs/design/undo.md`).
    Undo,
    /// Replay the step undo took back.
    Redo,
    /// An exact parameter value, in the parameter's units, sent as a knob
    /// turn is. The control channel's `set` with a `value` uses it, so the
    /// stored number is the one sent and not a round trip through a position.
    ParamValue {
        module: u32,
        index: usize,
        value: f32,
    },
    /// A choice name was picked in the inspector: value `min + choice` of
    /// the parameter, as one undo step (panels.md, Choice names).
    Choice {
        module: u32,
        index: usize,
        choice: usize,
    },
    /// A range was picked in a parameter's **Range** selector: index
    /// `range` of the ranges it offers (`docs/design/control-ranges.md`).
    Range {
        module: u32,
        index: usize,
        range: usize,
    },
    /// An inspector slider was let go.
    SliderReleased,
    /// Open or close the **Add modulation** list under a parameter.
    AddModulation {
        module: u32,
        index: usize,
    },
    /// Assign an output to a parameter, picked from **Add modulation**.
    Assign {
        source: OutputRef,
        module: u32,
        index: usize,
    },
    /// An assignment's depth slider moved.
    Depth {
        id: AssignmentId,
        depth: f32,
    },
    /// An assignment's polarity was picked.
    Polarity {
        id: AssignmentId,
        choice: PolarityChoice,
    },
    /// An assignment was turned on or off.
    Enabled {
        id: AssignmentId,
        enabled: bool,
    },
    /// The remove button on an assignment row.
    Unassign(AssignmentId),
    /// A line under **Drives** selects its target module.
    SelectModule(u32),
    /// A message for the Preferences window (`docs/design/preferences.md`).
    Preferences(settings::Message),
    /// The clipboard was read for a paste at a world point, not yet snapped
    /// (`docs/design/clipboard.md`).
    /// `generation` is the engine's when the read began: a result that
    /// arrives after another patch opened is dropped.
    Pasted {
        world: Point,
        text: Option<String>,
        generation: u64,
    },
}

/// The Transport module and the index of its `run` parameter, if the patch
/// has one.
fn transport_run(controller: &Controller) -> Option<(ModuleId, usize)> {
    let id = controller.transport_module()?;
    let index = controller.graph().node(id)?.descriptor.param_index("run")?;
    Some((id, index))
}

/// True when a cable drives the Transport's `run` input.
fn run_is_driven(controller: &Controller) -> bool {
    controller.transport_module().is_some_and(|module| {
        controller
            .graph()
            .cables()
            .any(|(_, to)| to == InputRef { module, port: 0 })
    })
}

/// Whether the engine's transport runs, as the Transport module last said
/// (its `running` display value). `None` without a Transport.
fn transport_running(controller: &Controller) -> Option<bool> {
    let id = controller.transport_module()?;
    let index = controller.graph().node(id)?.descriptor.display_index("running")?;
    Some(controller.display(id, index)? >= 0.5)
}

/// `opened <path>`, followed by the load notes when there are any, so a note
/// about what loaded differently stays in view.
fn opened_status(path: &std::path::Path, notes: &[String]) -> String {
    if notes.is_empty() {
        format!("opened {}", path.display())
    } else {
        format!("opened {}: {}", path.display(), notes.join("; "))
    }
}

struct App {
    controller: Controller,
    positions: BTreeMap<u32, Point>,
    scene: Scene,
    /// Live values of panel controls and lights, apart from the scene so a
    /// knob turn does not rebuild tiles or cables.
    values: PanelValues,
    /// Last brightness of each light, by module and widget.
    lights: BTreeMap<lights::Light, f32>,
    /// What the last loaded patch loaded differently from its file, such
    /// as a saved control range that was not kept (control-ranges.md).
    load_notes: Vec<String>,
    /// True when the last commit had a cable on the Transport's `run`
    /// input. When the cable goes, the Run toggle takes the state the input
    /// left (clock.md, Starting and stopping).
    run_driven: bool,
    /// Latest level of each meter probe.
    meter_levels: std::collections::HashMap<ProbeId, live::Level>,
    /// Modules whose lights or meters are on screen, as the canvas reports.
    visible_displays: Vec<u32>,
    selection: BTreeSet<u32>,
    /// The name saved inside the patch.
    name: String,
    /// The file the patch was opened from or saved to (`docs/design/files.md`).
    file: Option<PathBuf>,
    /// The file's time and size when it was last opened or saved.
    stamp: Option<recovery::Stamp>,
    /// The title's name for a patch with no file, when it is not
    /// "Untitled": a restored copy whose file changed.
    shown_name: Option<String>,
    /// Where Save As starts when there is no current file: next to a
    /// restored copy's changed or missing file.
    save_hint: Option<PathBuf>,
    /// The patch as it was last opened or saved, to tell unsaved work
    /// apart. `None` when nothing on disk holds this patch (a restored copy).
    saved_content: Option<Patch>,
    /// This run's recovery folder (`docs/design/autosave.md`). `None` when
    /// autosave is off.
    recovery: Option<recovery::Session>,
    /// Counts the engines built. Background watchers restart when it changes.
    generation: u64,
    sample_rate: f32,
    no_audio: bool,
    fake_audio: bool,
    /// The safety stage's mode, for every engine this app builds.
    output_safety: OutputSafety,
    /// The `test.` modules are loaded (`--test-modules`).
    test_modules: bool,
    /// Set when the app should exit after this update.
    quit: bool,
    theme: Theme,
    _audio: Option<RunningAudio>,
    /// The engine, when no audio device plays it. The app never renders
    /// with it: each update only applies the queued edits
    /// (`App::drain_idle_engine`), so the queue never fills. Tests run it by
    /// hand with `App::process`, so they can check what depends on a
    /// running engine (live values, lights, meters). Keeping it costs the
    /// real audio path nothing: there is none.
    idle_engine: Option<loom_core::Engine>,
    /// Meter and scope data from the engine, drained on the telemetry
    /// thread so the queues never fill.
    monitor: monitor::Monitor,
    live: Live,
    /// When the dropout count last grew, for the stats bar warning.
    dropouts: alerts::Dropouts,
    /// What belongs to the machine: preferences, devices, MIDI inputs.
    rig: settings::Rig,
    audio_stats: Option<Arc<CallbackStats>>,
    audio_callbacks_seen: u64,
    audio_callbacks_advanced: Instant,
    audio_name: String,
    /// Note input shared by every MIDI port and the "Hold note" button.
    midi: Option<SharedMidi>,
    holding: bool,
    status: String,
    /// Every registered module, for the browser.
    modules: Vec<&'static ModuleDescriptor>,
    /// The module browser, while it is open.
    browser: Option<browser::Browser>,
    /// Changed by the toolbar button; the canvas then opens the browser at
    /// its centre.
    browser_key: u64,
    /// The probe menu, while it is open.
    probe_menu: Option<probe_menu::ProbeMenu>,
    frame_key: u64,
    meter: bench::FrameMeter,
    /// The stdout report, when started with --bench, --measure, or
    /// --measure-idle.
    reporter: Option<bench::Reporter>,
    /// The control channel's listeners. While there are any, errors go to
    /// the status bar and the reply, not to a dialog.
    control: Vec<control::Listener>,
    /// Commands waiting for the window to draw their change.
    control_pending: Vec<control::Pending>,
    /// The control channel's pointer and its events on their way to the
    /// view (`pointer.rs`).
    pointer: pointer::Pointer,
    /// The stream and its callback count at the last control `snapshot`,
    /// for its `advanced` field.
    control_seen: Option<(Arc<CallbackStats>, u64)>,
    /// True once a failed `--record` has been reported.
    recording_failed: bool,
    measuring: bool,
    animating: bool,
    /// MIDI learn mode (`docs/design/midi-learn.md`).
    learning: bool,
    /// The parameter the next control change maps to.
    armed: Option<learn::Armed>,
    /// What learn mode shows on the canvas.
    learn_view: LearnView,
    /// Undo and redo (`docs/design/undo.md`): the history, and the edit
    /// or gesture being recorded.
    edits: edits::Edits,
    /// The parameter whose **Add modulation** list is open.
    adding: Option<(u32, usize)>,
    /// During an arc drag: the assignment it changes, once picked, or
    /// `Some(None)` when none fits and the drag does nothing.
    depth_drag: Option<Option<AssignmentId>>,
    /// How the canvas starts, for a still image of the window
    /// (`--screenshot-window`). `None` in the app.
    still: Option<loom_canvas::Still>,
    /// The name for the step the edit in progress makes, when a command
    /// names it (cut, paste, duplicate) rather than what changed.
    step_name: Option<String>,
}

/// Where a patch comes from.
enum Source<'a> {
    Empty,
    Generated(usize),
    File(&'a Path),
    /// A recovery copy (`docs/design/autosave.md`).
    Recovered(Box<Patch>),
}

/// An engine and the patch loaded into it, before audio is attached.
struct Session {
    controller: Controller,
    engine: loom_core::Engine,
    midi: loom_core::MidiSender,
    telemetry: loom_core::Telemetry,
    positions: BTreeMap<u32, Point>,
    name: String,
    file: Option<PathBuf>,
    stamp: Option<recovery::Stamp>,
    /// What loaded differently from the file, such as a saved control
    /// range that was not kept (control-ranges.md, rule 7).
    notes: Vec<String>,
}

impl App {
    fn new(args: Args) -> Self {
        Self::boot(args, &mut recovery::Dialogs)
    }

    /// Starts the app on the real devices. Tests get the real audio but no
    /// MIDI inputs, so connected hardware cannot change their results.
    fn boot(args: Args, prompt: &mut dyn recovery::Prompt) -> Self {
        #[cfg(test)]
        let host: Rc<dyn devices::Host> = Rc::new(devices::fake::NoMidi);
        #[cfg(not(test))]
        let host: Rc<dyn devices::Host> = Rc::new(devices::System);
        Self::boot_with(args, prompt, host)
    }

    /// Starts the app: reads the preferences, opens this run's recovery
    /// folder, offers a copy left by a crash, then opens the patch and the
    /// devices (`docs/design/autosave.md`, `docs/design/preferences.md`).
    fn boot_with(args: Args, prompt: &mut dyn recovery::Prompt, host: Rc<dyn devices::Host>) -> Self {
        let (store, mut notes) = settings::open_store(&args.devices);
        let mut rig = settings::Rig::new(host, store, args.devices.clone(), args.output_safety.is_some());
        let mut session_folder = None;
        let mut restore = None;
        if let Some(root) = args.recovery_root() {
            match root.and_then(|root| recovery::start(&root)) {
                Ok(startup) => {
                    session_folder = Some(startup.session);
                    if let Some(orphan) = startup.orphan {
                        match prompt.ask(&orphan) {
                            recovery::Choice::Restore => restore = Some(orphan),
                            recovery::Choice::Discard => {
                                if let Err(e) = orphan.discard() {
                                    notes.push(format!("Could not delete the recovery copy: {e}."));
                                }
                            }
                            recovery::Choice::NotNow => {}
                            recovery::Choice::Unavailable => notes.push(format!(
                                "Unsaved changes from a crash were kept. Install zenity or kdialog to restore them, or open {}.",
                                orphan.patch_path().display()
                            )),
                        }
                    }
                }
                Err(e) => notes.push(format!("Autosave is off: {e}.")),
            }
        }
        // The file is checked after the answer: it can change while the
        // question is open.
        let mut restore_failed = false;
        let mut restore_error = |e: &str, orphan: recovery::Orphan, notes: &mut Vec<String>| {
            let message = format!(
                "Could not restore the unsaved changes: {e}. The copy is kept at {}.",
                orphan.patch_path().display()
            );
            prompt.error(&message);
            notes.push(message);
            // Dropping the orphan keeps its folder for a later start.
        };
        let mut recovered = restore.and_then(|orphan| match orphan.load() {
            Ok(patch) => Some((orphan.file_state(), orphan, patch)),
            Err(e) => {
                restore_error(&e, orphan, &mut notes);
                restore_failed = true;
                None
            }
        });

        let (output, audio_notes) = settings::open_output(&rig, args.no_audio, args.fake_audio);
        notes.extend(audio_notes);
        if let (Some(path), Ok(output)) = (&args.record, &output) {
            match loom_host_desktop::Recorder::start(path, output.sample_rate()) {
                Ok(recorder) => rig.recorder = Some(recorder),
                Err(e) => {
                    let note = format!("Not recording: {e}.");
                    eprintln!("loom-app: {note}");
                    notes.push(note);
                }
            }
        }
        let sample_rate = output.as_ref().map(|o| o.sample_rate() as f32).unwrap_or(48_000.0);
        let source = match &recovered {
            Some((_, _, patch)) => Source::Recovered(Box::new(patch.clone())),
            None if restore_failed => Source::Empty,
            None => args.source(),
        };
        let mut status = String::new();
        let config = EngineConfig {
            output_safety: args.output_safety.unwrap_or(rig.store.saved().output_safety),
            ..EngineConfig::with_sample_rate(sample_rate)
        };
        let session = Self::session(config, args.test_modules, source).unwrap_or_else(|e| {
            // A copy that reads but does not load (an unknown module) is a
            // failed restore too: the orphan is kept, not adopted.
            match recovered.take() {
                Some((_, orphan, _)) => restore_error(&e, orphan, &mut notes),
                None => status = e,
            }
            Self::session(config, args.test_modules, Source::Empty).expect("an empty patch always loads")
        });
        let mut app = Self::assemble(output, sample_rate, session, 1, &args, rig, true);
        // The compile report gives way to anything that needs reading.
        let mut keep = !status.is_empty();
        if keep {
            app.status = status;
        }
        app.recovery = session_folder;
        if let Some((state, orphan, _)) = recovered {
            app.restored(state, orphan, &args);
            keep = true;
        }
        if !notes.is_empty() {
            if keep {
                notes.insert(0, std::mem::take(&mut app.status));
            }
            app.status = notes.join(" ");
        }
        app
    }

    /// Sets up a patch restored from a recovery copy (autosave.md, After a
    /// restore): marked unsaved, with its file only if that is unchanged.
    fn restored(&mut self, state: recovery::FileState, orphan: recovery::Orphan, args: &Args) {
        match state {
            recovery::FileState::Unchanged(path) => {
                self.stamp = recovery::Stamp::of(&path);
                self.file = Some(path);
            }
            recovery::FileState::Changed(path) | recovery::FileState::Missing(path) => {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                let name = format!("{} (recovered)", stem.as_deref().unwrap_or("Untitled"));
                // Save As starts in the file's folder, under a name that
                // does not replace it.
                self.save_hint = path
                    .parent()
                    .map(|dir| dir.join(format!("{name}.{}", files::EXTENSION)));
                self.shown_name = Some(name);
            }
            recovery::FileState::NeverSaved => {}
        }
        // Nothing on disk holds this patch.
        self.edits.history.clear();
        self.saved_content = None;
        let when = recovery::local_time(orphan.written());
        self.status = match args.source() {
            Source::File(path) => format!(
                "Restored unsaved changes from {when}. {} was not opened.",
                files::display_name(Some(path))
            ),
            _ => format!("Restored unsaved changes from {when}."),
        };
        if let Some(session) = &mut self.recovery {
            session.adopt(orphan);
        }
        // This session's own copy, at once: the orphan goes when it is on disk.
        self.autosave(Instant::now());
    }

    /// The patch as it would be saved now.
    fn current_patch(&self) -> Patch {
        let mut patch = Patch::from_graph(&self.name, self.controller.graph());
        for m in &mut patch.modules {
            m.position = self.positions.get(&m.id).map(|p| (p.x.round(), p.y.round()));
        }
        patch
    }

    /// Writes or deletes the recovery copy when one is due
    /// (`docs/design/autosave.md`). Runs on a timer and after save, open,
    /// and new.
    fn autosave(&mut self, now: Instant) {
        if self.recovery.is_none() {
            return;
        }
        let current = self.current_patch();
        let target = (self.saved_content.as_ref() != Some(&current)).then_some(current);
        let (file, stamp) = (self.file.clone(), self.stamp);
        let Some(session) = &mut self.recovery else { return };
        let lines = session.tick(now, target, || {
            recovery::Info::new(file.as_deref(), stamp, std::time::SystemTime::now())
        });
        if let Some(line) = lines.into_iter().last() {
            self.status = line;
        }
    }

    /// Builds an engine and loads a patch into it. On failure nothing else
    /// is touched, so a failed open leaves the running patch alone.
    fn session(config: EngineConfig, test_modules: bool, source: Source) -> Result<Session, String> {
        let EngineParts {
            mut controller,
            engine,
            midi,
            telemetry,
        } = create_engine(registry(test_modules), config);
        let mut positions = BTreeMap::new();
        let mut notes = Vec::new();
        let mut load = |patch: &Patch, controller: &mut Controller| -> Result<String, String> {
            notes = patch.load_into(controller).map_err(|e| e.to_string())?;
            for m in &patch.modules {
                if let Some((x, y)) = m.position {
                    positions.insert(m.id, Point::new(x, y));
                }
            }
            Ok(patch.name.clone())
        };
        let (name, file, stamp) = match source {
            Source::Generated(n) => {
                layout::generate(&mut controller, n).map_err(|e| format!("generate failed: {e}"))?;
                layout::generated_layout(&controller, &mut positions);
                (format!("Generated ({n} modules)"), None, None)
            }
            Source::File(path) => {
                // Stamped before reading: a change after this shows as a change.
                let stamp = recovery::Stamp::of(path);
                let patch = Patch::load(path).map_err(|e| e.to_string())?;
                let name = load(&patch, &mut controller)?;
                (name, Some(path.to_path_buf()), stamp)
            }
            Source::Recovered(patch) => (load(&patch, &mut controller)?, None, None),
            Source::Empty => ("Untitled".to_string(), None, None),
        };
        // Every patch has one Transport (clock.md). A loaded file gets one
        // in `load_into`; a new or generated patch gets it here.
        if controller.transport_module().is_none() && controller.registry().resolve(TRANSPORT_MODULE).is_ok() {
            controller.add_module(TRANSPORT_MODULE).map_err(|e| e.to_string())?;
            controller.commit().map_err(|e| e.to_string())?;
        }
        layout::auto_layout(&controller, &mut positions);
        for note in &notes {
            eprintln!("loom-app: {note}");
        }
        Ok(Session {
            controller,
            engine,
            midi,
            telemetry,
            positions,
            name,
            file,
            stamp,
            notes,
        })
    }

    /// Attaches audio and MIDI to a session and builds the app around it.
    /// A rig that has MIDI inputs open already keeps them, pointed at the
    /// new engine. Otherwise its inputs are opened, unless `connect_midi`
    /// is false.
    fn assemble(
        output: Result<AudioOutput, String>,
        sample_rate: f32,
        session: Session,
        generation: u64,
        args: &Args,
        mut rig: settings::Rig,
        connect_midi: bool,
    ) -> Self {
        let Session {
            controller,
            engine,
            midi: sender,
            telemetry,
            positions,
            name,
            file,
            stamp,
            notes,
        } = session;
        let mut status = notes.join("; ");
        let load_notes = notes;

        // Every MIDI input plays the patch, and the "Hold note" button works
        // alongside them, so a keyboard is never hidden behind another port.
        // The inputs are opened once the app is built, on its controller.
        let first_midi = rig.attach(sender);
        let midi = rig.midi_in.clone().expect("attached");

        let (audio, midi, audio_name, idle_engine) = match output {
            Ok(mut output) => {
                let label = format!("{} at {} Hz", output.name(), output.sample_rate());
                rig.playing = (!args.fake_audio).then(|| devices::Playing::of(&output));
                let refused = rig.record_into(&mut output);
                match output.start(engine) {
                    Ok(audio) => {
                        if let Some(e) = refused {
                            status = rig.end_recording(&e);
                        }
                        (Some(audio), Some(midi), label, None)
                    }
                    Err(e) => {
                        rig.playing = None;
                        (None, None, format!("no audio ({})", e.error), Some(e.engine))
                    }
                }
            }
            Err(e) => (None, None, format!("no audio ({e})"), Some(engine)),
        };

        let scene = scene::build(&controller, &positions);
        let mut values = PanelValues::new();
        scene::update_panel_values(&mut values, &controller, &BTreeMap::new());
        // The loaded patch may already have a cable on run, before any
        // commit of the app's own (#413 review).
        let run_driven = run_is_driven(&controller);
        let mut app = Self {
            controller,
            positions,
            scene,
            values,
            lights: BTreeMap::new(),
            load_notes,
            run_driven,
            meter_levels: std::collections::HashMap::new(),
            visible_displays: Vec::new(),
            selection: BTreeSet::new(),
            name,
            file,
            stamp,
            shown_name: None,
            save_hint: None,
            saved_content: None,
            recovery: None,
            generation,
            sample_rate,
            no_audio: args.no_audio,
            fake_audio: args.fake_audio,
            output_safety: args.output_safety.unwrap_or(rig.store.saved().output_safety),
            test_modules: args.test_modules,
            quit: false,
            theme: Theme::custom(
                "Loom",
                iced::theme::Palette {
                    background: Color::from_rgb8(0x22, 0x24, 0x2A),
                    text: Color::from_rgb8(0xE6, 0xE8, 0xEE),
                    primary: Color::from_rgb8(0xF5, 0xA6, 0x23),
                    success: Color::from_rgb8(0x4C, 0xB0, 0x6E),
                    warning: Color::from_rgb8(0xE8, 0xD2, 0x4A),
                    danger: Color::from_rgb8(0xE0, 0x4F, 0x4F),
                },
            ),
            audio_stats: audio.as_ref().map(|a| a.stats().clone()),
            audio_callbacks_seen: 0,
            audio_callbacks_advanced: Instant::now(),
            _audio: audio,
            idle_engine,
            monitor: monitor::Monitor::new(telemetry, generation),
            live: Live::new(),
            dropouts: alerts::Dropouts::default(),
            rig,
            audio_name,
            midi,
            holding: false,
            status,
            // The app's tests load the test modules but never list them.
            modules: if args.test_modules {
                registry(true).iter().collect()
            } else {
                loom_modules::registry().iter().collect()
            },
            browser: None,
            browser_key: 0,
            probe_menu: None,
            frame_key: 0,
            meter: bench::FrameMeter::default(),
            reporter: args.bench.reporter(),
            control: args.control.listeners(),
            control_pending: Vec::new(),
            pointer: pointer::Pointer::new(!args.control.listeners().is_empty()),
            control_seen: None,
            recording_failed: false,
            measuring: args.bench.redraws(),
            animating: args.bench.bench,
            learning: false,
            armed: None,
            learn_view: LearnView::default(),
            edits: edits::Edits::default(),
            adding: None,
            depth_drag: None,
            still: None,
            step_name: None,
        };
        // A stream that failed to start took the engine with it.
        app.keep_engine_idle();
        app.send_panic_trigger();
        if let Some(error) = app.connect_midi(first_midi, connect_midi) {
            app.status = error;
        }
        // A patch just loaded is the one on disk.
        app.edits.history.mark_saved();
        app.saved_content = Some(app.current_patch());
        if app.status.is_empty() {
            app.report();
        }
        app
    }

    /// True when the patch differs from the file (`docs/design/files.md`).
    fn unsaved(&self) -> bool {
        !self.edits.history.is_saved()
    }

    /// Writes the patch to `path`. On success the path becomes the current
    /// file and the unsaved mark clears.
    fn write_to(&mut self, path: PathBuf) -> bool {
        let patch = self.current_patch();
        match patch.save(&path) {
            Ok(()) => {
                self.status = format!("saved {}", path.display());
                self.stamp = recovery::Stamp::of(&path);
                self.file = Some(path);
                self.shown_name = None;
                self.save_hint = None;
                self.edits.history.mark_saved();
                self.saved_content = Some(patch);
                // The file holds the work now: the copy goes.
                self.autosave(Instant::now());
                true
            }
            Err(e) => {
                let message = format!(
                    "Could not save \"{}\": {e}. Your changes are not saved.",
                    files::display_name(Some(&path))
                );
                self.show_error(&message);
                self.status = message;
                false
            }
        }
    }

    /// Save: to the current file, or Save As when there is none. Returns
    /// false if nothing was saved.
    fn save(&mut self) -> bool {
        self.finish_edits();
        match self.file.clone() {
            Some(path) => self.write_to(path),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        self.finish_edits();
        match files::pick_save(self.file.as_deref().or(self.save_hint.as_deref())) {
            Some(path) => self.write_to(path),
            None => false,
        }
    }

    /// Ends any edit in progress, so the history and the saved mark see it.
    /// A canvas drag stays open: the rest of it becomes a step of its own
    /// when it ends.
    fn finish_edits(&mut self) {
        self.close_slider_gesture();
        if self.edits.gesture_open() {
            self.record();
        }
    }

    /// Asks what to do with unsaved changes. Returns true if the caller may
    /// go on and drop them.
    fn can_discard(&mut self) -> bool {
        self.finish_edits();
        if !self.unsaved() {
            return true;
        }
        match files::confirm_discard(&files::display_name(self.file.as_deref())) {
            files::Choice::Save => self.save(),
            files::Choice::DontSave => true,
            files::Choice::Cancel => false,
            files::Choice::Unavailable => {
                self.status =
                    "Unsaved changes. No dialog program (zenity or kdialog) is installed: save with Ctrl+S first."
                        .to_string();
                false
            }
        }
    }

    /// Replaces the patch (and its engine) with `source`. If the source does
    /// not load, the running patch is unchanged.
    fn replace(&mut self, source: Source) -> bool {
        // Choosing the device does not start a stream, so the old patch keeps
        // playing. The engine is built at the rate of the device that will
        // play it.
        // A recording goes on into the same file (rig.recorder).
        let (output, _) = settings::open_output(&self.rig, self.no_audio, self.fake_audio);
        // Playing now and no device to play on afterwards: keep what plays.
        if let (Err(e), true) = (&output, self._audio.is_some()) {
            let message = format!(
                "Could not open the patch: the audio device is not available ({e}). The current patch is unchanged."
            );
            self.show_error(&message);
            self.status = message;
            return false;
        }
        let sample_rate = output
            .as_ref()
            .map(|o| o.sample_rate() as f32)
            .unwrap_or(self.sample_rate);
        let config = EngineConfig {
            output_safety: self.output_safety,
            ..EngineConfig::with_sample_rate(sample_rate)
        };
        let session = match Self::session(config, self.test_modules, source) {
            Ok(session) => session,
            Err(e) => {
                let message = format!("Could not open the patch: {e}. The current patch is unchanged.");
                self.show_error(&message);
                self.status = message;
                return false;
            }
        };
        // Nothing can fail from here: fade the old engine out and free the
        // audio device before the new engine takes it. MIDI inputs stay
        // open and move to the new engine.
        self.stop_audio();
        self.midi = None;
        let args = Args {
            patch: None,
            generate: None,
            no_audio: self.no_audio,
            fake_audio: self.fake_audio,
            record: None,
            bench: bench::Options {
                measure: self.measuring,
                ..Default::default()
            },
            control: control::Options::default(),
            output_safety: Some(self.output_safety),
            devices: devices::Options::default(),
            view: screenshot::Options::default(),
            dump_ui: false,
            select: Vec::new(),
            frames: None,
            recovery_dir: None,
            no_recovery: true,
            test_modules: self.test_modules,
        };
        let rig = std::mem::replace(&mut self.rig, settings::Rig::empty());
        let mut fresh = Self::assemble(output, sample_rate, session, self.generation + 1, &args, rig, true);
        fresh.animating = self.animating;
        fresh.reporter = self.reporter.take();
        fresh.control = std::mem::take(&mut self.control);
        // The script's pointer stays where it was, over the new patch.
        fresh.pointer = self.pointer.clone();
        // The session goes on; only its patch changed.
        fresh.recovery = self.recovery.take();
        // Commands waiting to be drawn still get their answer, except a
        // `sample`, which would measure a module of the new patch.
        fresh.control_pending = control::end_samples(std::mem::take(&mut self.control_pending));
        // The canvas keeps its state across the rebuild and compares these
        // keys with the ones it saw: they must keep counting up. A new frame
        // key also resets pan and zoom to show the whole patch.
        fresh.browser_key = self.browser_key;
        fresh.frame_key = self.frame_key + 1;
        *self = fresh;
        // The new patch has no unsaved changes: the copy goes.
        self.autosave(Instant::now());
        true
    }

    /// Tells the user that something failed, in a dialog, unless the
    /// control channel is on: a dialog would stop the app with nobody to
    /// close it (`docs/design/control-channel.md`, rule 9).
    fn show_error(&self, message: &str) {
        if self.control.is_empty() {
            files::show_error(message);
        }
    }

    fn new_patch(&mut self) {
        if self.can_discard() && self.replace(Source::Empty) {
            self.status = "new patch".to_string();
        }
    }

    fn open(&mut self) {
        if !self.can_discard() {
            return;
        }
        let Some(path) = files::pick_open(self.file.as_deref()) else {
            return;
        };
        self.open_path(&path);
    }

    /// Opens the patch at `path` in place of this one, and says so in the
    /// status bar, with any notes about what loaded differently.
    fn open_path(&mut self, path: &std::path::Path) {
        if self.replace(Source::File(path)) {
            self.status = opened_status(path, &self.load_notes);
        }
    }

    fn title(&self) -> String {
        match (&self.file, &self.shown_name) {
            (None, Some(name)) => files::titled(name, self.unsaved()),
            _ => files::window_title(self.file.as_deref(), self.unsaved()),
        }
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        let main = if self.measuring {
            // Redraw every frame, so the counter measures real render cost.
            iced::window::frames().map(Message::Frame)
        } else {
            // Otherwise the window redraws only when something changes. A
            // slow tick keeps the audio numbers fresh.
            iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick)
        };
        // Live modulated values move every block, so a patch with an
        // assignment that is on follows them every frame.
        // Without audio the engine does not run, so nothing moves.
        let live = if !self.measuring
            && self._audio.is_some()
            && self.controller.graph().assignments().any(|(_, a)| a.enabled)
        {
            iced::window::frames().map(|_| Message::LiveFrame)
        } else {
            Subscription::none()
        };
        let telemetry = Subscription::run_with(self.monitor.clone(), monitor::stream).map(Message::Meters);
        let close = iced::window::close_requests().map(|_| Message::CloseRequested);
        let control = self
            .control
            .iter()
            .map(|listener| Subscription::run_with(listener.clone(), control::stream).map(Message::Control))
            // Frames only while a command waits to be drawn, so an idle app
            // stays idle.
            .chain(
                self.control_waiting()
                    .then(|| iced::window::frames().map(Message::ControlFrame)),
            );
        let autosave = match &self.recovery {
            Some(_) => iced::time::every(recovery::TICK).map(|_| Message::Autosave),
            None => Subscription::none(),
        };
        // MIDI inputs plugged in and out are found by polling four times a
        // second: the systems offer no portable callback (device-changes.md).
        let midi_poll = if self.rig.midi_in.is_some() && !self.rig.flags.no_midi {
            iced::time::every(Duration::from_millis(250)).map(|_| Message::MidiPoll)
        } else {
            Subscription::none()
        };
        // Audio device loss, silent stalls, and device reconnection are watched
        // by polling four times a second (device-changes.md, Audio device loss).
        let audio_poll = if !self.no_audio && !self.fake_audio {
            iced::time::every(Duration::from_millis(250)).map(|_| Message::AudioPoll)
        } else {
            Subscription::none()
        };
        let report = match &self.reporter {
            Some(reporter) => Subscription::run_with(reporter.clone(), bench::stream).map(|()| Message::BenchDone),
            None => Subscription::none(),
        };
        Subscription::batch(
            [
                main,
                live,
                self.light_watch(),
                telemetry,
                self.midi_watch(),
                close,
                report,
                autosave,
                midi_poll,
                audio_poll,
            ]
            .into_iter()
            .chain(control),
        )
    }

    /// Wakes the UI when control changes arrive, while there is something to
    /// update: mapped parameters to follow, or learn mode waiting for MIDI.
    /// In learn mode, it also listens for Escape.
    fn midi_watch(&self) -> Subscription<Message> {
        let mapped = self.controller.graph().midi_mappings().next().is_some();
        let watch = (mapped || self.learning || self.rig.panic_learn.is_some()).then(|| {
            let watch = learn::Watch {
                reader: self.controller.midi_reader(),
                generation: self.generation,
            };
            Subscription::run_with(watch, learn::stream).map(|_| Message::Midi)
        });
        let keys = Some(iced::keyboard::listen().map(Message::Key));
        // The search field takes Escape for itself, so the browser listens
        // to keys whether or not a widget used them.
        let browser = self.browser.is_some().then(|| iced::event::listen_with(browser_key));
        // The panic key works whatever has focus (panic.md).
        let panic = Some(iced::event::listen_with(panic::key));
        Subscription::batch(watch.into_iter().chain(keys).chain(browser).chain(panic))
    }

    /// Watches the lights on screen, and sends a message only when one
    /// visibly changes. No lights on screen: no watcher.
    fn light_watch(&self) -> Subscription<Message> {
        match self.watched_lights() {
            Some(watch) => Subscription::run_with(watch, lights::stream).map(Message::Lights),
            None => Subscription::none(),
        }
    }

    /// The lights on screen and where their values live, or `None` when
    /// there are none.
    fn watched_lights(&self) -> Option<lights::Watch> {
        let graph = self.controller.graph();
        let watched: Vec<_> = self
            .visible_displays
            .iter()
            .filter_map(|&id| Some((id, graph.node(ModuleId(id))?.descriptor)))
            .flat_map(|(id, d)| {
                d.panel.widgets.iter().enumerate().filter_map(move |(index, w)| {
                    let display = (!w.is_control()).then(|| d.display_index(w.bind))??;
                    Some(((id, index), display))
                })
            })
            .filter_map(|(light, display)| {
                let source = self.controller.display_source(ModuleId(light.0), display)?;
                Some((light, source))
            })
            .collect();
        (!watched.is_empty()).then(|| lights::Watch {
            reader: self.controller.display_reader(),
            lights: watched,
            generation: self.generation,
        })
    }

    /// Applies the edits queued for the idle engine, and frees what it
    /// hands back, after every update. Without it, nothing would drain the
    /// command queue when no device plays, and edits would fail once it is
    /// full (#220). It renders no audio, so lights and meters stay still.
    fn drain_idle_engine(&mut self) {
        if let Some(engine) = self.idle_engine.as_mut() {
            engine.apply_pending();
            self.controller.collect_garbage();
        }
    }

    /// Runs the engine for `frames` frames on silent buffers, as an audio
    /// device would. For tests and `--frames`: the window never runs the
    /// idle engine. Runs in chunks, so a long run needs no long buffers;
    /// the engine gives the same output at any block size.
    fn process(&mut self, frames: usize) {
        const CHUNK: usize = 4096;
        let engine = self.idle_engine.as_mut().expect("an audio device plays the engine");
        let (mut left, mut right) = (vec![0.0; CHUNK], vec![0.0; CHUNK]);
        let mut remaining = frames;
        while remaining > 0 {
            let n = remaining.min(CHUNK);
            engine.process(&mut left[..n], &mut right[..n]);
            remaining -= n;
        }
    }

    /// Rebuilds the scene after the graph changed.
    fn refresh(&mut self) {
        layout::auto_layout(&self.controller, &mut self.positions);
        self.scene = scene::build(&self.controller, &self.positions);
        self.refresh_values();
        self.refresh_learn();
    }

    /// Rebuilds what learn mode shows, after mappings or the armed
    /// parameter changed. A removed module disarms its parameter.
    fn refresh_learn(&mut self) {
        if let Some(armed) = self.armed
            && self.controller.graph().node(ModuleId(armed.param.0)).is_none()
        {
            self.armed = None;
        }
        self.learn_view = learn::view(&self.controller, self.armed);
    }

    /// Arms a parameter in learn mode, or disarms it if it is armed.
    fn arm(&mut self, param: learn::Param) {
        let (module, index) = param;
        let transient = self
            .controller
            .graph()
            .node(ModuleId(module))
            .and_then(|node| node.descriptor.params.get(index))
            .is_some_and(|spec| spec.is_transient());
        if transient {
            // A live gesture cannot be mapped (transient-params.md).
            self.status = format!(
                "MIDI learn: {} is a live control and cannot be mapped",
                learn::param_name(&self.controller, param)
            );
            return;
        }
        if self.armed.is_some_and(|armed| armed.param == param) {
            self.armed = None;
            self.status = "MIDI learn: click a control to map it".to_string();
        } else {
            self.armed = Some(learn::Armed {
                param,
                serial: self.controller.last_control_change().map(|seen| seen.serial),
            });
            self.status = format!(
                "Move a MIDI control to map it to {}",
                learn::param_name(&self.controller, param)
            );
        }
        self.refresh_learn();
    }

    /// Follows control changes: mapped parameters on screen, and the armed
    /// parameter in learn mode.
    fn midi_arrived(&mut self) {
        if !self.controller.sync_midi().is_empty() {
            self.refresh_values();
        }
        if self.panic_midi_arrived() {
            return;
        }
        let Some(armed) = self.armed else { return };
        let Some(seen) = self.controller.last_control_change() else {
            return;
        };
        if Some(seen.serial) == armed.serial {
            return;
        }
        let (module, index) = armed.param;
        self.begin_edit();
        self.edits.touch(module, index);
        let learned = self.controller.learn_midi(seen, ModuleId(module), index);
        match learned {
            Ok((id, removed)) => {
                let mapping = self
                    .controller
                    .graph()
                    .midi_mappings()
                    .find(|(i, _)| *i == id)
                    .map(|(_, m)| *m);
                self.armed = None;
                self.commit();
                if let Some(mapping) = mapping {
                    self.status = learn::mapped_status(&self.controller, mapping, &removed);
                }
            }
            Err(e) => {
                self.armed = None;
                self.status = format!("error: {e}");
                self.refresh_learn();
            }
        }
        if !self.edits.gesture_open() {
            self.record();
        }
    }

    fn stop_learning(&mut self) {
        self.learning = false;
        self.armed = None;
        self.status = "MIDI learn off".to_string();
        self.refresh_learn();
    }

    /// Rebuilds the control, light, and meter values. Only changed values
    /// change what the canvas uploads.
    fn refresh_values(&mut self) {
        let mut shown: BTreeMap<lights::Light, (f32, f32)> = self
            .lights
            .iter()
            .map(|(&light, &level)| (light, (level, 0.0)))
            .collect();
        let modules: Vec<u32> = self.controller.graph().nodes().map(|(id, _)| id.0).collect();
        for (widget, output) in scene::meter_outputs(&self.controller, &modules) {
            let level = self
                .live
                .probe(output)
                .and_then(|probe| self.meter_levels.get(&probe))
                .copied()
                .unwrap_or_default();
            shown.insert(widget, (level.rms, level.peak));
        }
        let dragging = self.depth_drag.flatten();
        scene::update_panel_values_dragging(&mut self.values, &self.controller, &shown, dragging);
    }

    /// Takes the latest dropout count for the stats bar warning. Runs on
    /// every tick and, in Measure mode, on every frame.
    fn update_dropouts(&mut self, now: Instant) {
        if let Some(stats) = &self.audio_stats {
            let s = stats.snapshot();
            self.dropouts.update(s.overruns + s.stream_errors, now);
        }
        // A recording that stopped early (a full disk) is reported once.
        if !self.recording_failed
            && let Some(error) = self.rig.recorder.as_ref().and_then(loom_host_desktop::Recorder::error)
        {
            self.recording_failed = true;
            self.status = format!("The recording stopped: {error}");
            eprintln!("loom-app: {}", self.status);
        }
    }

    /// The outputs of the meters on screen: each needs a probe.
    fn metered_outputs(&self) -> BTreeSet<OutputRef> {
        scene::meter_outputs(&self.controller, &self.visible_displays)
            .into_iter()
            .map(|(_, output)| output)
            .collect()
    }

    /// Adds probes for meters that came on screen and removes those that
    /// left. Only probes change, so the scene is not rebuilt.
    fn sync_probes(&mut self) {
        let wanted = self.metered_outputs();
        self.live.sync(&mut self.controller, &wanted);
        // Probe IDs are never reused, so a level of a removed probe is dead.
        let live: std::collections::HashSet<ProbeId> = self.live.probes().collect();
        self.meter_levels.retain(|probe, _| live.contains(probe));
        if let Err(e) = self.controller.commit() {
            self.status = format!("error: {e}");
        }
    }

    /// Sets a parameter as a knob turn does. A slider sends no start, so its
    /// first change opens the gesture and letting go closes it.
    fn turn_param(&mut self, module: u32, index: usize, value: f32) {
        self.edits.start_slider_gesture();
        self.begin_edit();
        self.set_param(module, index, value);
    }

    /// Selects a parameter's control range, as one undo step. A smaller
    /// range clamps the value into it, a wider one keeps it (control-ranges.md,
    /// rule 6). The commit puts the bounds into the schedule for modulation
    /// and MIDI (rule 8).
    fn set_range(&mut self, module: u32, index: usize, range: usize) {
        let Some(range) = self
            .controller
            .graph()
            .node(ModuleId(module))
            .and_then(|n| n.descriptor.params.get(index))
            .and_then(|spec| spec.options.ranges.get(range))
            .copied()
        else {
            return;
        };
        self.close_slider_gesture();
        self.edit(|app| {
            if let Err(e) = app.controller.set_range(ModuleId(module), index, range) {
                app.status = format!("error: {e}");
                return;
            }
            app.edits.touch(module, index);
            app.commit();
        });
        self.refresh_values();
    }

    /// Sets a parameter from a control on a tile or in the inspector.
    fn set_param(&mut self, module: u32, index: usize, value: f32) {
        if let Err(e) = self.controller.set_param(ModuleId(module), index, value) {
            self.status = format!("error: {e}");
        }
        self.edits.touch(module, index);
        self.refresh_values();
    }

    /// Applies a button gesture. The canvas names the gesture. [`loom_core::ButtonMode`]
    /// names the value, from the mode parameter stored on the module. Toggle
    /// writes the latch and Momentary the transient hold.
    fn apply_button(&mut self, module: u32, widget: usize, change: impl Fn(loom_core::ButtonMode, f32) -> Option<f32>) {
        let edit = self.controller.graph().node(ModuleId(module)).and_then(|node| {
            let panel_widget = node.descriptor.panel.widgets.get(widget)?;
            let mode = node.descriptor.button_mode(panel_widget, &node.params)?;
            let target = node.descriptor.control_param(panel_widget, &node.params)?;
            let next = change(mode, node.params[target])?;
            Some((target, next))
        });
        if let Some((index, value)) = edit {
            self.set_param(module, index, value);
        }
    }

    /// Lets go of a button's transient hold, whatever the mode is now. The
    /// mode can change during a momentary press (inspector, undo, or a MIDI
    /// mapping), and the release then resolves to the latch. Without this
    /// the hold would stay high and come back with Momentary mode.
    fn release_hold(&mut self, module: u32, widget: usize) {
        let held = self.controller.graph().node(ModuleId(module)).and_then(|node| {
            let loom_core::WidgetKind::Button { hold, .. } = node.descriptor.panel.widgets.get(widget)?.kind else {
                return None;
            };
            let index = node.descriptor.param_index(hold)?;
            let default = node.descriptor.params[index].default;
            (node.params[index] != default).then_some((index, default))
        });
        if let Some((index, default)) = held {
            self.set_param(module, index, default);
        }
    }

    /// Assigns an output to a parameter with the default depth, after the
    /// others on that parameter. A drop on a tile control and **Add
    /// modulation** both come here. An output that already drives the
    /// parameter adds nothing (`docs/design/modulation.md`, Making an
    /// assignment).
    fn assign(&mut self, source: OutputRef, target: ModuleId, index: usize) {
        let graph = self.controller.graph();
        let Some(spec) = graph.node(target).and_then(|n| n.spec(index)) else {
            return;
        };
        let (from, to) = (
            modulation::source_name(graph, source),
            modulation::target_name(graph, target, index),
        );
        if modulation::exists(graph, source, target, index) {
            self.status = format!("{from} already modulates {to}");
            return;
        }
        let depth = modulation::default_depth(&spec);
        match self.controller.add_assignment(source, target, index, depth) {
            Ok(_) => {
                if self.commit() {
                    self.status = format!("{from} modulates {to}, {}", modulation::depth_text(depth));
                }
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// One step of an arc drag. The first picks the assignment, by the
    /// spec's order; the rest of the drag keeps it.
    fn drag_depth(&mut self, module: u32, widget: usize, pinned: Option<loom_canvas::PortRef>, delta: f32) {
        let Some((param, _, _)) = scene::control_param(&self.controller, module, widget) else {
            return;
        };
        let graph = self.controller.graph();
        let picked = *self.depth_drag.get_or_insert_with(|| {
            let pinned = pinned.map(|p| OutputRef {
                module: ModuleId(p.module),
                port: p.index,
            });
            let selected = (self.selection.len() == 1)
                .then(|| self.selection.iter().next().map(|&id| ModuleId(id)))
                .flatten();
            match modulation::drag_target(graph, ModuleId(module), param, pinned, selected) {
                Ok(id) => Some(id),
                Err(why) => {
                    self.status = why.to_string();
                    None
                }
            }
        });
        let Some(id) = picked else { return };
        let Some(a) = self.controller.graph().assignment(id).copied() else {
            return;
        };
        match self.controller.set_assignment_depth(id, a.depth + delta) {
            Ok(depth) => {
                self.status = format!(
                    "{}: depth {}",
                    modulation::source_name(self.controller.graph(), a.source),
                    modulation::depth_text(depth)
                );
            }
            Err(e) => self.status = format!("error: {e}"),
        }
        self.refresh_values();
    }

    fn report(&mut self) {
        if let Some(lost) = &self.rig.lost_audio {
            self.status = lost.message.clone();
            return;
        }
        let report = self.controller.last_report();
        let inserted = report.inserted_delays().count();
        self.status = match report.loops.len() {
            0 => "no feedback loops".to_string(),
            n => format!(
                "{n} feedback {}, {inserted} automatic 1-sample {} (dashed)",
                if n == 1 { "loop" } else { "loops" },
                if inserted == 1 { "delay" } else { "delays" },
            ),
        };
        // One warning per compile, not one per parameter.
        if report.without_live_value > 0 {
            self.status.push_str(&format!(
                "; {} modulated {} show no live value (table full)",
                report.without_live_value,
                if report.without_live_value == 1 {
                    "knob"
                } else {
                    "knobs"
                }
            ));
        }
    }

    /// Starts recording an edit: keeps the patch as it is now. Does nothing
    /// if one is already open, so a gesture's first call is the one that
    /// counts.
    fn begin_edit(&mut self) {
        let (controller, positions) = (&self.controller, &self.positions);
        self.edits.begin(controller.has_uncommitted_changes(), || {
            undo::Snapshot::take(controller, positions)
        });
    }

    /// Ends the recording and adds a step if the patch changed.
    fn record(&mut self) {
        // A command's name is for this step only, also when no step is made.
        let step_name = self.step_name.take();
        let (controller, positions) = (&self.controller, &self.positions);
        self.edits
            .record(step_name, || undo::Snapshot::take(controller, positions));
    }

    /// Closes the open gesture, of either kind, and records it.
    fn end_gesture(&mut self) {
        let step_name = self.step_name.take();
        let (controller, positions) = (&self.controller, &self.positions);
        self.edits
            .end_gesture(controller.has_uncommitted_changes(), step_name, || {
                undo::Snapshot::take(controller, positions)
            });
    }

    /// Runs one edit as one undo step, unless a gesture is open: then the
    /// gesture's end makes the step (undo.md, rule 1 and 2).
    fn edit(&mut self, change: impl FnOnce(&mut Self)) {
        self.begin_edit();
        change(self);
        if !self.edits.gesture_open() {
            self.record();
        }
    }

    /// Closes a gesture that began on an inspector slider.
    fn close_slider_gesture(&mut self) {
        if self.edits.slider_gesture_open() {
            self.end_gesture();
        }
    }

    fn undo(&mut self) {
        self.close_slider_gesture();
        if !self.edits.gesture_open() {
            self.step(false);
        }
    }

    fn redo(&mut self) {
        self.close_slider_gesture();
        if !self.edits.gesture_open() {
            self.step(true);
        }
    }

    /// Undoes (`forward` false) or redoes the next step.
    fn step(&mut self, forward: bool) {
        let step = if forward {
            self.edits.history.redo_step()
        } else {
            self.edits.history.undo_step()
        };
        let Some(step) = step.cloned() else { return };
        let verb = if forward { "Redo" } else { "Undo" };
        if let Err(why) = undo::check(&step, forward, &self.controller) {
            self.status = format!("Cannot {} {}: {why}", verb.to_lowercase(), step.name());
            return;
        }
        // An armed parameter is a mode, not an edit: undo disarms it first.
        self.armed = None;
        let applied = undo::apply(&step, forward, &mut self.controller, &mut self.positions);
        match applied {
            Ok(()) => {
                if forward {
                    self.edits.history.redone();
                } else {
                    self.edits.history.undone();
                }
                // What the step touched, if it still exists, is selected.
                let touched: BTreeSet<u32> = step
                    .touched_modules()
                    .filter(|&id| self.controller.graph().node(ModuleId(id)).is_some())
                    .collect();
                if !touched.is_empty() {
                    self.selection = touched;
                }
                self.selection
                    .retain(|&id| self.controller.graph().node(ModuleId(id)).is_some());
                if self.commit() {
                    self.status = format!("{verb}: {}", step.name());
                } else {
                    // The graph has the change but the engine does not. The
                    // history is already cleared by `commit`.
                    self.status = format!(
                        "{verb} could not reach the engine ({}); undo history cleared",
                        self.status
                    );
                }
            }
            Err(e) => {
                self.edits.history.clear();
                self.commit();
                self.status = format!("{verb} failed ({e}); undo history cleared");
            }
        }
    }

    /// Sends the graph to the engine. Returns false if that failed; the
    /// status bar then holds the error.
    fn commit(&mut self) -> bool {
        self.keep_run_state();
        // Probes follow the graph: a removed module loses its probes.
        let wanted = self.metered_outputs();
        self.live.sync(&mut self.controller, &wanted);
        let sent = match self.controller.commit() {
            Ok(_) => {
                self.report();
                self.edits.committed(true);
                true
            }
            Err(e) => {
                self.status = format!("error: {e}");
                // The controller stays dirty, so the next commit retries the
                // change; until then the history cannot be trusted.
                self.edits.committed(false);
                false
            }
        };
        self.refresh();
        sent
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        // A mode change the engine's full queue refused goes again now.
        self.send_pending_safety();
        let was_open = self.browser.is_some();
        let task = match message {
            Message::Browser(message) => self.browser_message(message),
            Message::Canvas(CanvasEvent::Paste { world }) => {
                let generation = self.generation;
                iced::clipboard::read().map(move |text| Message::Pasted {
                    world,
                    text,
                    generation,
                })
            }
            Message::Control(call) => self.control(call),
            Message::ControlFrame(now) => self.control_frame(now),
            Message::Located {
                call,
                target,
                found,
                then,
            } => self.located(call, &target, &found, then),
            message if self.clipboard_key(&message).is_some() => self.clipboard_command(&message),
            message => {
                self.handle(message);
                Task::none()
            }
        };
        self.drain_idle_engine();
        if self.quit {
            // Stop audio now, with a fade so it does not click, so a
            // recording is closed before the process ends, and leave no
            // control socket behind.
            self.stop_audio();
            control::close(&self.control);
            // A clean exit: the recovery folder goes (autosave.md, Clean exit).
            if let Some(session) = self.recovery.take() {
                session.finish();
            }
            Task::batch([task, iced::exit()])
        } else if !was_open && self.browser.is_some() {
            Task::batch([task, browser::Browser::focus()])
        } else {
            task
        }
    }

    /// Handles a browser message: keep it open, close it, or add a module.
    fn browser_message(&mut self, message: browser::Message) -> Task<Message> {
        let Some(open) = self.browser.as_mut() else {
            return Task::none();
        };
        let (outcome, task) = open.update(message);
        match outcome {
            browser::Outcome::Stay => {}
            browser::Outcome::Close => self.browser = None,
            browser::Outcome::Add(descriptor, world) => {
                self.browser = None;
                self.add_module(descriptor, world);
            }
        }
        task.map(Message::Browser)
    }

    /// Adds a module with its top-left corner at a world point and selects
    /// it, as one undo step.
    fn add_module(&mut self, descriptor: &'static ModuleDescriptor, world: Point) {
        self.close_slider_gesture();
        self.edit(|app| match app.controller.add_module(descriptor.id) {
            Ok(id) => {
                app.positions.insert(id.0, world);
                app.selection = BTreeSet::from([id.0]);
                app.commit();
            }
            Err(e) => app.status = format!("Could not add {}: {e}", descriptor.name),
        });
    }

    /// Adds a scope or a meter fed by `output`, beside `world`, and selects
    /// it. The module and its cable are one undo step.
    fn add_probe(&mut self, probe: probe_menu::Probe, output: loom_canvas::PortRef, world: Point) {
        let Some(descriptor) = self.modules.iter().copied().find(|d| d.id == probe.id()) else {
            self.status = format!("Cannot {}: no module `{}`", probe.verb(), probe.id());
            return;
        };
        let tiles: Vec<iced::Rectangle> = self.scene.modules().iter().map(|m| m.bounds()).collect();
        let corner = probe_menu::place(descriptor, world, &tiles);
        let source = OutputRef {
            module: ModuleId(output.module),
            port: output.index,
        };
        // The output may be gone: the control channel can remove its module
        // while the menu is open.
        let exists = self
            .controller
            .graph()
            .node(source.module)
            .is_some_and(|node| source.port < node.descriptor.outputs.len());
        if !exists {
            self.status = format!("Cannot {}: the output is gone", probe.verb());
            return;
        }
        self.close_slider_gesture();
        self.step_name = Some(probe.verb().to_string());
        self.edit(|app| {
            let id = match app.controller.add_module(descriptor.id) {
                Ok(id) => id,
                Err(e) => {
                    app.status = format!("Cannot {}: {e}", probe.verb());
                    return;
                }
            };
            if let Err(e) = app.controller.connect(source, InputRef { module: id, port: 0 }) {
                // A probe that is not connected shows nothing: take it back,
                // so the edit makes no step.
                let _ = app.controller.remove_module(id);
                app.commit();
                app.status = format!("Cannot {}: {e}", probe.verb());
                return;
            }
            app.positions.insert(id.0, corner);
            app.selection = BTreeSet::from([id.0]);
            app.commit();
        });
        // `record` clears it, but an edit inside a gesture does not record.
        self.step_name = None;
    }

    fn handle(&mut self, message: Message) {
        match message {
            // `update` runs these: they return tasks.
            Message::Browser(_) | Message::Control(_) | Message::ControlFrame(_) | Message::Located { .. } => {}
            Message::Preferences(message) => self.preferences(message),
            Message::Key(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) if self.rig.window.is_some() => {
                // The window takes Escape and its own shortcut, and keeps
                // other keys from the canvas behind it.
                match key.as_ref() {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
                        self.preferences(settings::Message::Close)
                    }
                    iced::keyboard::Key::Character(",") if modifiers.command() => self.toggle_preferences(),
                    _ => {}
                }
            }
            Message::Key(_) if self.browser.is_some() || self.rig.window.is_some() => {}
            Message::Key(iced::keyboard::Event::KeyPressed { key, .. }) if self.probe_menu.is_some() => {
                // The menu takes Escape and keeps other keys from the canvas.
                if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                    self.probe_menu = None;
                }
            }
            Message::Key(_) if self.probe_menu.is_some() => {}
            Message::ProbeMenu(message) => {
                if let (Some(menu), probe_menu::Message::Pick(probe)) = (self.probe_menu.take(), message) {
                    self.add_probe(probe, menu.output, menu.world);
                }
            }
            Message::AddProbe { probe, output, world } => self.add_probe(probe, output, world),
            Message::Canvas(event) => self.canvas_event(event),
            Message::Param {
                module,
                index,
                position,
            } => {
                let value = self
                    .controller
                    .graph()
                    .node(ModuleId(module))
                    .and_then(|n| n.spec(index))
                    .map(|spec| spec.denormalize(position));
                if let Some(value) = value {
                    self.turn_param(module, index, value);
                }
            }
            Message::ParamValue { module, index, value } => {
                let value = self
                    .controller
                    .graph()
                    .node(ModuleId(module))
                    .and_then(|n| n.spec(index))
                    .map(|spec| spec.clamp(value));
                if let Some(value) = value {
                    self.turn_param(module, index, value);
                }
            }
            Message::Choice { module, index, choice } => {
                let value = self
                    .controller
                    .graph()
                    .node(ModuleId(module))
                    .and_then(|n| n.spec(index))
                    .map(|spec| spec.clamp(spec.min + choice as f32));
                if let Some(value) = value {
                    self.turn_param(module, index, value);
                    self.close_slider_gesture();
                }
            }
            Message::Range { module, index, range } => self.set_range(module, index, range),
            Message::SliderReleased => self.close_slider_gesture(),
            Message::AddModulation { module, index } => {
                self.adding = if self.adding == Some((module, index)) {
                    None
                } else {
                    Some((module, index))
                };
            }
            Message::Assign { source, module, index } => {
                self.adding = None;
                self.close_slider_gesture();
                self.edit(|app| app.assign(source, ModuleId(module), index));
            }
            Message::Depth { id, depth } => {
                // Like a parameter slider: the first change opens the
                // gesture, and letting go closes it, so one drag is one step.
                self.edits.start_slider_gesture();
                self.begin_edit();
                if let Err(e) = self.controller.set_assignment_depth(id, depth) {
                    self.status = format!("error: {e}");
                }
                self.refresh_values();
            }
            Message::Polarity { id, choice } => {
                self.close_slider_gesture();
                self.edit(|app| {
                    if let Err(e) = app.controller.set_assignment_polarity(id, choice.polarity()) {
                        app.status = format!("error: {e}");
                    }
                    app.refresh_values();
                });
            }
            Message::Enabled { id, enabled } => {
                self.close_slider_gesture();
                self.edit(|app| {
                    if let Err(e) = app.controller.set_assignment_enabled(id, enabled) {
                        app.status = format!("error: {e}");
                    }
                    app.refresh_values();
                });
            }
            Message::Unassign(id) => {
                self.close_slider_gesture();
                self.edit(|app| match app.controller.remove_assignment(id) {
                    Ok(removed) => {
                        app.commit();
                        let graph = app.controller.graph();
                        app.status = format!(
                            "Removed modulation from {} to {}",
                            modulation::source_name(graph, removed.source),
                            modulation::target_name(graph, removed.target, removed.param)
                        );
                    }
                    Err(e) => app.status = format!("error: {e}"),
                });
            }
            Message::SelectModule(id) => {
                if self.controller.graph().node(ModuleId(id)).is_some() {
                    self.selection = BTreeSet::from([id]);
                }
            }
            Message::Undo => self.undo(),
            Message::Redo => self.redo(),
            Message::OpenBrowser => self.browser_key += 1,
            Message::Panic => self.panic(),
            Message::MidiPoll => self.poll_midi(),
            Message::HoldNote => {
                if let Some(midi) = &self.midi {
                    self.holding = !self.holding;
                    loom_host_desktop::send_midi(
                        midi,
                        if self.holding {
                            MidiEvent::note_on(0, 0, 57, 100)
                        } else {
                            MidiEvent::note_off(0, 0, 57)
                        },
                    );
                }
            }
            Message::New => self.new_patch(),
            Message::Open => self.open(),
            Message::Save => {
                self.save();
            }
            Message::SaveAs => {
                self.save_as();
            }
            Message::CloseRequested => {
                if self.can_discard() {
                    self.quit = true;
                }
            }
            Message::Measure(on) => {
                self.measuring = on;
                self.meter = bench::FrameMeter::default();
                if !on {
                    self.animating = false;
                }
            }
            Message::Animate(on) => {
                self.animating = on;
                if on {
                    self.measuring = true;
                }
                self.meter = bench::FrameMeter::default();
            }
            Message::BenchDone => self.quit = true,
            Message::Frame(now) => {
                self.meter.record(now);
                scene::update_live_values(&mut self.values, &self.controller);
                self.controller.collect_garbage();
                self.update_dropouts(now);
            }
            Message::LiveFrame => scene::update_live_values(&mut self.values, &self.controller),
            Message::Autosave => self.autosave(Instant::now()),
            Message::Tick => {
                self.controller.collect_garbage();
                self.update_dropouts(Instant::now());
            }
            Message::AudioPoll => self.poll_audio(),
            Message::Meters(levels) => {
                // A reading may arrive after its probe was removed.
                let live: std::collections::HashSet<ProbeId> = self.live.probes().collect();
                self.meter_levels
                    .extend(levels.into_iter().filter(|(probe, _)| live.contains(probe)));
                self.refresh_values();
            }
            Message::Lights(readings) => {
                self.lights.extend(readings);
                self.refresh_values();
            }
            Message::ToggleLearn => {
                if self.learning {
                    self.stop_learning();
                } else {
                    self.learning = true;
                    self.status = "MIDI learn: click a control to map it".to_string();
                    self.refresh_learn();
                }
            }
            Message::LearnParam { module, index } => {
                if self.learning {
                    self.arm((module, index));
                }
            }
            Message::Unmap { module, index } => {
                self.close_slider_gesture();
                self.edit(|app| match app.controller.unmap_midi(ModuleId(module), index) {
                    Ok(Some(mapping)) => {
                        app.commit();
                        app.status = format!(
                            "Removed {} from {}",
                            mapping.control,
                            learn::param_name(&app.controller, (module, index))
                        );
                    }
                    Ok(None) => {}
                    Err(e) => app.status = format!("error: {e}"),
                });
            }
            Message::Midi => self.midi_arrived(),
            Message::Pasted {
                world,
                text,
                generation,
            } => {
                // The paste was for a patch that is no longer open.
                if generation == self.generation {
                    self.paste(world, text);
                }
            }
            Message::Key(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) if self.learning => self.stop_learning(),
            Message::Key(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) if modifiers.command() => {
                // Ctrl+Z or Cmd+Z undoes; adding Shift redoes. Ctrl+Y also
                // redoes, except on macOS.
                match key.as_ref() {
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") && modifiers.shift() => {
                        self.redo()
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") => self.undo(),
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("n") => self.new_patch(),
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("o") => self.open(),
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("s") && modifiers.shift() => {
                        self.save_as();
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("s") => {
                        self.save();
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("y") && !cfg!(target_os = "macos") => {
                        self.redo()
                    }
                    iced::keyboard::Key::Character(",") => self.toggle_preferences(),
                    _ => {}
                }
            }
            Message::Key(_) => {}
        }
    }

    fn canvas_event(&mut self, event: CanvasEvent) {
        let edits = undo::is_edit(&self.controller, &event);
        if edits {
            self.close_slider_gesture();
            self.edit(|app| app.apply_canvas_event(event));
        } else {
            self.apply_canvas_event(event);
        }
    }

    fn apply_canvas_event(&mut self, event: CanvasEvent) {
        match event {
            CanvasEvent::GestureStart => {
                self.close_slider_gesture();
                self.edits.start_gesture();
                self.begin_edit();
            }
            CanvasEvent::GestureEnd => {
                if self.depth_drag.take().is_some() {
                    self.refresh_values();
                }
                self.end_gesture();
            }
            CanvasEvent::Select(ids) => self.selection = ids.into_iter().collect(),
            CanvasEvent::Move { ids, delta } => {
                for id in &ids {
                    if let Some(p) = self.positions.get_mut(id) {
                        *p += delta;
                    }
                }
                self.scene.move_modules(&ids, delta);
            }
            CanvasEvent::Connect { from, to } => {
                let output = OutputRef {
                    module: ModuleId(from.module),
                    port: from.index,
                };
                let input = InputRef {
                    module: ModuleId(to.module),
                    port: to.index,
                };
                match self.controller.connect(output, input) {
                    Ok(_) => {
                        self.commit();
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
            }
            CanvasEvent::Assign { from, module, widget } => {
                if let Some((index, _, _)) = scene::control_param(&self.controller, module, widget) {
                    let source = OutputRef {
                        module: ModuleId(from.module),
                        port: from.index,
                    };
                    self.assign(source, ModuleId(module), index);
                }
            }
            CanvasEvent::DragDepth {
                module,
                widget,
                pinned,
                delta,
            } => self.drag_depth(module, widget, pinned, delta),
            CanvasEvent::Disconnect(port) => {
                let input = InputRef {
                    module: ModuleId(port.module),
                    port: port.index,
                };
                if self.controller.disconnect(input).is_ok() {
                    self.commit();
                }
            }
            CanvasEvent::SetControl {
                module,
                widget,
                position,
            } => {
                if let Some((index, kind, spec)) = scene::control_param(&self.controller, module, widget) {
                    let value = scene::control_value(kind, &spec, position);
                    self.set_param(module, index, value);
                }
            }
            CanvasEvent::ButtonPressed { module, widget } => {
                self.apply_button(module, widget, loom_core::ButtonMode::press);
            }
            CanvasEvent::ButtonReleased { module, widget } => {
                self.apply_button(module, widget, |mode, _current| mode.release());
                self.release_hold(module, widget);
            }
            CanvasEvent::ButtonArrow { module, widget, high } => {
                self.apply_button(module, widget, |mode, current| mode.arrow(current, high));
            }
            CanvasEvent::ResetControl { module, widget } => {
                if let Some((index, _, spec)) = scene::control_param(&self.controller, module, widget) {
                    self.set_param(module, index, spec.default);
                }
            }
            CanvasEvent::Learn { module, widget } => {
                if let Some((index, _, _)) = scene::control_param(&self.controller, module, widget) {
                    self.arm((module, index));
                }
            }
            CanvasEvent::ProbeMenu {
                output,
                world,
                at,
                area,
            } => {
                self.close_slider_gesture();
                self.probe_menu = Some(probe_menu::ProbeMenu::open(output, world, at, area));
            }
            CanvasEvent::OpenBrowser { world, at, area } => {
                self.close_slider_gesture();
                self.browser = Some(browser::Browser::open(self.modules.clone(), world, at, area));
            }
            CanvasEvent::VisibleDisplays(ids) => {
                self.visible_displays = ids;
                self.sync_probes();
                self.refresh_values();
            }
            CanvasEvent::ToggleRun => self.toggle_run(),
            CanvasEvent::Delete(ids) => {
                self.delete_modules(&ids);
            }
            // The app reads the clipboard first, in `update`.
            CanvasEvent::Paste { .. } => {}
        }
    }

    /// Space: flips the Transport's Run toggle, as a click on it does
    /// (clock.md, Controls live on the Transport module). While a cable
    /// drives `run`, the input decides, so Space changes nothing. Without a
    /// Transport, it starts or stops the engine's transport directly.
    fn toggle_run(&mut self) {
        let Some((transport, index)) = transport_run(&self.controller) else {
            let running = !self.controller.transport().running;
            if let Err(e) = self.controller.set_running(running) {
                self.status = format!("error: {e}");
            }
            return;
        };
        if run_is_driven(&self.controller) {
            self.status = "Run follows the cable on the Transport's run input".to_string();
            return;
        }
        let on = self
            .controller
            .graph()
            .node(transport)
            .is_some_and(|n| n.params[index] >= 0.5);
        self.set_param(transport.0, index, if on { 0.0 } else { 1.0 });
    }

    /// Before a commit: when the cable on the Transport's `run` input is
    /// gone, the transport stays as the input left it, and the Run toggle
    /// takes that state, so it shows the truth and works again.
    fn keep_run_state(&mut self) {
        let driven = run_is_driven(&self.controller);
        if self.run_driven
            && !driven
            && let Some((transport, index)) = transport_run(&self.controller)
            && let Some(running) = transport_running(&self.controller)
        {
            let value = if running { 1.0 } else { 0.0 };
            if self
                .controller
                .graph()
                .node(transport)
                .is_some_and(|n| n.params[index] != value)
            {
                self.set_param(transport.0, index, value);
            }
        }
        self.run_driven = driven;
    }

    /// Removes modules and commits. Returns false if the commit failed; the
    /// status bar then holds the error.
    fn delete_modules(&mut self, ids: &[u32]) -> bool {
        for id in ids {
            if self.controller.remove_module(ModuleId(*id)).is_ok() {
                self.positions.remove(id);
                self.selection.remove(id);
            }
        }
        self.commit()
    }

    /// The clipboard command a message asks for: `'c'`, `'x'`, or `'d'`
    /// with Ctrl or Cmd. Paste comes from the canvas instead, which knows
    /// where the pointer is. Keys go to the browser while it is open.
    fn clipboard_key(&self, message: &Message) -> Option<char> {
        let Message::Key(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character(c),
            modifiers,
            ..
        }) = message
        else {
            return None;
        };
        if !modifiers.command() || self.browser.is_some() || self.probe_menu.is_some() || self.rig.window.is_some() {
            return None;
        }
        ['c', 'x', 'd']
            .into_iter()
            .find(|k| c.eq_ignore_ascii_case(&k.to_string()))
    }

    fn clipboard_command(&mut self, message: &Message) -> Task<Message> {
        let text = match self.clipboard_key(message) {
            Some('c') => self.copy(),
            Some('x') => self.cut(),
            Some('d') => {
                self.duplicate();
                None
            }
            _ => None,
        };
        text.map_or_else(Task::none, iced::clipboard::write)
    }

    /// The selected modules, or `None` and a status bar note when nothing
    /// is selected.
    fn selected_group(&mut self, verb: &str, incoming: bool) -> Option<clipboard::Group> {
        let group =
            clipboard::Group::from_selection(self.controller.graph(), &self.positions, &self.selection, incoming);
        if group.is_empty() {
            self.status = format!("Nothing selected to {verb}");
            return None;
        }
        Some(group)
    }

    /// Copy: returns the patch text for the clipboard. Not an edit.
    fn copy(&mut self) -> Option<String> {
        let group = self.selected_group("copy", false)?;
        self.status = format!("Copied {}", group.describe());
        Some(group.to_text())
    }

    /// Cut: copy, then delete, as one step named "cut ...".
    fn cut(&mut self) -> Option<String> {
        // Like undo, these commands wait for a drag to finish.
        self.close_slider_gesture();
        if self.edits.gesture_open() {
            return None;
        }
        let group = self.selected_group("cut", false)?;
        let ids: Vec<u32> = self.selection.iter().copied().collect();
        self.step_name = Some(format!("cut {}", group.describe()));
        self.edit(|app| {
            if app.delete_modules(&ids) {
                app.status = format!("Cut {}", group.describe());
            }
        });
        Some(group.to_text())
    }

    /// Pastes clipboard text with its top-left corner at a world point.
    fn paste(&mut self, world: Point, text: Option<String>) {
        self.close_slider_gesture();
        if self.edits.gesture_open() {
            return;
        }
        let Some(text) = text else {
            self.status = clipboard::NOTHING_TO_PASTE.to_string();
            return;
        };
        match clipboard::Group::from_text(&text, self.controller.registry()) {
            Ok(group) => self.add_group(&group, browser::snap(world), "paste", "Pasted"),
            Err(why) => self.status = why,
        }
    }

    /// Duplicates the selection next to it, keeping incoming cables and
    /// assignments.
    fn duplicate(&mut self) {
        self.close_slider_gesture();
        if self.edits.gesture_open() {
            return;
        }
        let Some(group) = self.selected_group("duplicate", true) else {
            return;
        };
        let at = group.top_left().unwrap_or(Point::ORIGIN);
        let offset = clipboard::DUPLICATE_OFFSET;
        self.add_group(
            &group,
            Point::new(at.x + offset, at.y + offset),
            "duplicate",
            "Duplicated",
        );
    }

    /// Adds a group as one step and selects it, or changes nothing and says
    /// why.
    fn add_group(&mut self, group: &clipboard::Group, origin: Point, verb: &str, done: &str) {
        if let Err(why) = group.check(&self.controller) {
            self.status = format!("Cannot {verb}: {why}");
            return;
        }
        self.step_name = Some(format!("{verb} {}", group.describe()));
        self.edit(|app| match group.add(&mut app.controller, &mut app.positions, origin) {
            Ok(ids) => {
                app.selection = ids.into_iter().collect();
                if app.commit() {
                    app.status = format!("{done} {}", group.describe());
                }
            }
            Err(e) => {
                // The rollback freed the new modules' slots. Only a commit
                // hands them back, so the next paste has room.
                app.commit();
                app.status = format!("Cannot {verb}: {e}");
            }
        });
        // `record` clears it, but an edit inside a gesture does not record.
        self.step_name = None;
    }

    fn view(&self) -> Element<'_, Message> {
        let mut canvas = PatchCanvas::new(&self.scene, &self.selection, Message::Canvas)
            .values(&self.values)
            .frame_key(self.frame_key)
            .browser_key(self.browser_key)
            .interactive(self.browser.is_none() && self.probe_menu.is_none() && self.rig.window.is_none())
            .animate(self.animating)
            .learn(self.learning.then_some(&self.learn_view));
        if let Some(still) = self.still {
            canvas = canvas.still(still);
        }

        let body = row![
            container(canvas).width(Length::Fill).height(Length::Fill),
            rule::vertical(1),
            self.inspector(),
        ];

        let main = column![
            self.toolbar(),
            rule::horizontal(1),
            body,
            rule::horizontal(1),
            self.stats_bar()
        ];
        // Always a stack, so opening the browser keeps the canvas's state
        // (its camera) in the same place in the widget tree. Tested by
        // `ui_test::tests::the_camera_survives_opening_the_browser`.
        let mut layers = stack![main];
        if let Some(open) = &self.browser {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::Browser(browser::Message::Close));
            layers = layers.push(outside).push(open.view().map(Message::Browser));
        }
        if let Some(menu) = &self.probe_menu {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::ProbeMenu(probe_menu::Message::Close))
                .on_right_press(Message::ProbeMenu(probe_menu::Message::Close));
            layers = layers.push(outside).push(menu.view().map(Message::ProbeMenu));
        }
        if let Some(window) = self.preferences_view() {
            layers = layers.push(window);
        }
        // The control channel's pointer events go in here, as a real
        // pointer's would (control-channel.md, Pointer commands).
        let browser_open = self.browser.is_some();
        pointer::Injector::new(layers, self, move |key, status| {
            // As the subscriptions do: `keyboard::listen` passes on what no
            // widget took, and the open browser hears every key.
            let mut messages = Vec::new();
            if status == iced::event::Status::Ignored {
                messages.push(Message::Key(key.clone()));
            }
            if let Some(message) = panic::key(iced::Event::Keyboard(key.clone()), status, iced::window::Id::unique()) {
                messages.push(message);
            }
            if browser_open
                && let Some(message) = browser_key(iced::Event::Keyboard(key), status, iced::window::Id::unique())
            {
                messages.push(message);
            }
            messages
        })
        .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let hold = button(text(if self.holding { "Release note" } else { "Hold note" }).size(13))
            .on_press_maybe(self.midi.as_ref().map(|_| Message::HoldNote));
        row![
            text(format!(
                "{}{}",
                if self.unsaved() { "* " } else { "" },
                files::display_name(self.file.as_deref())
            ))
            .size(15),
            button(text("New").size(13)).on_press(Message::New),
            button(text("Open").size(13)).on_press(Message::Open),
            button(text("Save").size(13)).on_press(Message::Save),
            button(text("Save As").size(13)).on_press(Message::SaveAs),
            space::horizontal(),
            tooltip(
                button(text("Add module").size(13)).on_press(Message::OpenBrowser),
                container(text("Or press A over the canvas, or double-click empty canvas").size(12))
                    .padding(6)
                    .style(container::rounded_box),
                tooltip::Position::Bottom,
            ),
            hold,
            tooltip(
                button(text("Panic").size(13)).on_press(Message::Panic),
                container(
                    text(if cfg!(target_os = "macos") {
                        "Silence everything: Cmd+."
                    } else {
                        "Silence everything: Ctrl+."
                    })
                    .size(12)
                )
                .padding(6)
                .style(container::rounded_box),
                tooltip::Position::Bottom,
            ),
            self.history_button("Undo", "Undo", self.edits.history.undo_step(), Message::Undo),
            self.history_button("Redo", "Redo", self.edits.history.redo_step(), Message::Redo),
            button(text("MIDI learn").size(13))
                .on_press(Message::ToggleLearn)
                .style(if self.learning {
                    button::primary
                } else {
                    button::secondary
                }),
            tooltip(
                button(text("Preferences").size(13)).on_press(Message::Preferences(settings::Message::Open)),
                container(text(if cfg!(target_os = "macos") { "Cmd+," } else { "Ctrl+," }).size(12))
                    .padding(6)
                    .style(container::rounded_box),
                tooltip::Position::Bottom,
            ),
        ]
        .spacing(8)
        .padding([6, 10])
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// An Undo or Redo button. It is disabled with nothing to do, and its
    /// hover text names the step.
    fn history_button<'a>(
        &self,
        label: &'a str,
        verb: &str,
        step: Option<&undo::Step>,
        message: Message,
    ) -> Element<'a, Message> {
        let hint = match step {
            Some(step) => format!("{verb}: {}", step.name()),
            None => format!("Nothing to {}", verb.to_lowercase()),
        };
        tooltip(
            button(text(label).size(13)).on_press_maybe(step.map(|_| message)),
            container(text(hint).size(12)).padding(6).style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .into()
    }

    fn inspector(&self) -> Element<'_, Message> {
        inspector::view(self.inspector_model())
    }

    fn stats_bar(&self) -> Element<'_, Message> {
        let graph = self.controller.graph();
        let frames = if self.measuring {
            self.meter.stats().summary()
        } else {
            "redraws on change".to_string()
        };
        // Load and dropouts are always shown, and turn amber or red before
        // a problem is audible (FR-PERF-04).
        // The snapshot reads the same model, so the two cannot disagree.
        let stats = self.stats_model();
        let audio: Element<'_, Message> = if stats.playing {
            let load = bar_text(stats.load_text()).color(severity_colour(alerts::load_severity(stats.load)));
            let dropouts = bar_text(format!("dropouts {}", stats.dropouts))
                .color(severity_colour(self.dropouts.severity(Instant::now())));
            let mut items = row![bar_text(stats.label).color(DIM), load, dropouts].spacing(10);
            if stats.telemetry_dropped > 0 {
                items = items.push(bar_text(format!("telemetry dropped {}", stats.telemetry_dropped)).color(DIM));
            }
            items.into()
        } else {
            bar_text(stats.label).color(DIM).into()
        };
        let midi = match self.rig.ports.as_slice() {
            [] => "MIDI: no inputs".to_string(),
            ports => format!(
                "MIDI: {}",
                ports.iter().map(|port| port.name()).collect::<Vec<_>>().join(", ")
            ),
        };
        row![
            toggler(self.measuring)
                .label("Measure")
                .on_toggle(Message::Measure)
                .size(14)
                .text_size(12),
            toggler(self.animating)
                .label("Move camera")
                .on_toggle(Message::Animate)
                .size(14)
                .text_size(12),
            bar_text(frames),
            bar_text(format!("{} modules, {} cables", graph.len(), graph.cables().count())),
            audio,
            bar_text(midi).color(DIM),
            // The status takes the remaining width and is cut off rather than
            // wrapped, so the bar stays one line high. Hover shows all of it.
            tooltip(
                container(bar_text(&self.status).color(DIM))
                    .width(Length::Fill)
                    .align_x(iced::Alignment::End)
                    .clip(true),
                container(text(&self.status).size(12))
                    .padding(6)
                    .max_width(480)
                    .style(container::rounded_box),
                tooltip::Position::Top,
            ),
        ]
        .spacing(16)
        .padding([4, 10])
        .align_y(iced::Alignment::Center)
        .into()
    }
}

/// The inspector's remove button for an item in a list, such as a MIDI
/// mapping or a modulation assignment: a small × at the end of its row,
/// with hover text that names what it removes (panels.md, The inspector).
fn remove_button<'a>(what: &'a str, on_press: Message) -> Element<'a, Message> {
    tooltip(
        button(text("×").size(14))
            .padding([0, 6])
            .style(button::text)
            .on_press(on_press),
        container(text(what).size(12)).padding(6).style(container::rounded_box),
        tooltip::Position::Top,
    )
    .into()
}

/// Stats bar text: one line, never wrapped.
fn bar_text<'a>(content: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(content).size(12).wrapping(text::Wrapping::None)
}

const DIM: Color = Color::from_rgb(0.62, 0.65, 0.71);

/// Stats bar colour for a warning level.
fn severity_colour(severity: alerts::Severity) -> Color {
    match severity {
        alerts::Severity::Normal => DIM,
        alerts::Severity::Warning => Color::from_rgb8(0xF5, 0xA6, 0x23),
        alerts::Severity::Alarm => Color::from_rgb8(0xEB, 0x6B, 0x6B),
    }
}

/// Inspector help text, shown with nothing selected. It lists every canvas
/// gesture and shortcut (#16), so keep it in step with `PatchCanvas::handle`
/// and the key handling in `loom-canvas/src/widget.rs`.
const HELP: &str = "A or a double-click on empty canvas opens the module browser at the pointer; type, then Enter adds.\n\
Drag from a port to patch a cable. Drag a connected input to move or remove its cable.\n\
Drag tiles to move them. Shift-click or drag a box to select several. Cmd/Ctrl+A selects all.\n\
Delete or Backspace removes the selection.\n\
Drag a knob up or down; Shift for fine steps; double-click resets it.\n\
With a module selected, Tab and Shift-Tab move between its controls, the arrow keys change them, \
and Space or Enter presses, toggles, or steps the focused control. Escape clears the focus.\n\
Space toggles Run when no control has focus.\n\
Right-drag or two-finger scroll to pan. Wheel or Cmd/Ctrl-scroll to zoom. Cmd/Ctrl+0 zooms to 100%.\n\
F or Home frames the patch.\n\
Right-click an output or a cable to add a scope or a meter fed by it.\n\
Click a modulation badge to pin its lines; click again or press Escape to hide them.\n\
Cmd/Ctrl+C copies, X cuts, V pastes at the pointer, and D duplicates the selection.\n\
Cmd/Ctrl+Z undoes, and Cmd/Ctrl+Shift+Z redoes.\n\
MIDI learn: click a control, then move a knob on your controller. Escape ends it.";

/// The keys the open browser handles, whether or not a widget used them.
fn browser_key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    let iced::Event::Keyboard(Event::KeyPressed { key, .. }) = event else {
        return None;
    };
    let nav = match key {
        Key::Named(Named::ArrowUp) => browser::Nav::Up,
        Key::Named(Named::ArrowDown) => browser::Nav::Down,
        Key::Named(Named::PageUp) => browser::Nav::PageUp,
        Key::Named(Named::PageDown) => browser::Nav::PageDown,
        Key::Named(Named::Escape) => browser::Nav::Escape,
        _ => return None,
    };
    Some(Message::Browser(browser::Message::Key(nav)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Rectangle;

    fn app() -> App {
        App::new(app_args())
    }

    /// #16: the inspector help text names every canvas shortcut, the new
    /// Cmd/Ctrl+0 zoom reset and the ones that were there before it.
    #[test]
    fn help_text_lists_every_canvas_shortcut() {
        for shortcut in [
            "A or a double-click",
            "Cmd/Ctrl+A",
            "Delete or Backspace",
            "Tab and Shift-Tab",
            "arrow keys",
            "Space or Enter",
            "Escape clears the focus",
            "Space toggles Run",
            "Cmd/Ctrl+0",
            "F or Home",
            "press Escape",
            "Cmd/Ctrl+C",
            "V pastes",
            "D duplicates",
            "Cmd/Ctrl+Z",
            "Cmd/Ctrl+Shift+Z",
        ] {
            assert!(HELP.contains(shortcut), "help text lacks {shortcut:?}");
        }
    }

    /// Text in the window chrome meets WCAG 1.4.3 (4.5:1) on the theme
    /// background: the theme text colour, the dimmed `DIM` text, and the
    /// stats bar warning colours. Issue #257.
    #[test]
    fn window_text_meets_wcag_minimum_contrast() {
        let theme = app().theme.palette();
        let pairs = [
            ("theme text on background", theme.text),
            ("DIM text on background", DIM),
            ("warning text on background", severity_colour(alerts::Severity::Warning)),
            ("alarm text on background", severity_colour(alerts::Severity::Alarm)),
        ];
        for (name, text) in pairs {
            let ratio = loom_canvas::contrast_ratio(text, theme.background);
            assert!(
                ratio >= loom_canvas::MIN_TEXT_CONTRAST,
                "{name}: {ratio:.2}:1 is below {}:1",
                loom_canvas::MIN_TEXT_CONTRAST
            );
        }
    }

    /// The test constructors never open a real MIDI port (issue 221): the
    /// app built by `App::new` has no open inputs, and the host it uses
    /// reports none, whatever is plugged into the machine.
    #[test]
    fn test_app_opens_no_midi_ports() {
        let app = app();
        assert!(app.rig.ports.is_empty());
        assert!(devices::Host::midi_inputs(&devices::fake::NoMidi).unwrap().is_empty());
    }

    pub(crate) fn app_args() -> Args {
        Args {
            patch: None,
            generate: None,
            no_audio: true,
            fake_audio: false,
            record: None,
            bench: bench::Options::default(),
            control: control::Options::default(),
            output_safety: None,
            devices: crate::devices::Options::default(),
            view: screenshot::Options::default(),
            dump_ui: false,
            select: Vec::new(),
            frames: None,
            recovery_dir: None,
            no_recovery: false,
            test_modules: false,
        }
    }

    /// `--record` saves what the engine plays. The file is complete once the
    /// app is dropped.
    #[test]
    fn record_writes_a_wav_file() {
        let dir = std::env::temp_dir().join(format!("loom-app-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.wav");
        let app = App::new(Args {
            fake_audio: true,
            no_audio: false,
            record: Some(path.clone()),
            ..app_args()
        });
        std::thread::sleep(Duration::from_millis(200));
        drop(app);
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert!(reader.duration() > 0, "some audio was recorded");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// With `--fake-audio`, a timer thread plays the engine and drains its
    /// command queue, so edits keep working with no audio device (#220).
    /// The edits come in bursts with short pauses, as fast as a person
    /// could turn a knob, many times over.
    #[test]
    fn fake_audio_keeps_taking_edits() {
        let mut app = App::new(Args {
            fake_audio: true,
            no_audio: false,
            ..app_args()
        });
        assert!(app._audio.is_some(), "the clock plays the engine");
        let filter = app.controller.add_module("loom.filter.svf").unwrap();
        app.commit();
        for burst in 0..10 {
            for i in 0..500 {
                let _ = app.update(Message::Param {
                    module: filter.0,
                    index: 0,
                    position: ((burst * 500 + i) % 100) as f32 / 100.0,
                });
                assert!(!app.status.contains("queue is full"), "{}", app.status);
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Without an audio device the app keeps taking edits (#220). Each
    /// update applies the idle engine's queued commands, so 5,000 knob
    /// moves, far more than the queue holds, never fill it. The engine
    /// renders nothing in the meantime.
    #[test]
    fn no_audio_keeps_taking_edits() {
        let mut app = app();
        assert!(app._audio.is_none() && app.idle_engine.is_some());
        let filter = app.controller.add_module("loom.filter.svf").unwrap();
        app.commit();
        let capacity = loom_core::EngineConfig::default().command_capacity;
        for i in 0..5_000 {
            let _ = app.update(Message::Param {
                module: filter.0,
                index: 0,
                position: (i % 100) as f32 / 100.0,
            });
            assert!(!app.status.contains("queue is full"), "move {i}: {}", app.status);
        }
        assert!(5_000 > capacity);
        let stats = app.controller.stats();
        assert_eq!(stats.buffers.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(stats.leaked.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    fn open_at(app: &mut App, world: Point) {
        let _ = app.update(Message::Canvas(CanvasEvent::OpenBrowser {
            world,
            at: Point::ORIGIN,
            area: Rectangle::new(Point::ORIGIN, iced::Size::new(1000.0, 800.0)),
        }));
    }

    fn type_query(app: &mut App, query: &str) {
        let _ = app.update(Message::Browser(browser::Message::Query(query.to_string())));
    }

    #[test]
    fn adding_from_the_browser_is_one_undo_step_at_the_snapped_point() {
        let mut app = app();
        let before = app.controller.graph().nodes().count();
        let undo = app.edits.history.undo_step().map(|s| s.name().to_string());

        // Opening, typing, and closing are not edits.
        open_at(&mut app, Point::new(250.0, 130.0));
        type_query(&mut app, "svf");
        let _ = app.update(Message::Browser(browser::Message::Key(browser::Nav::Escape)));
        assert!(app.browser.is_none());
        assert_eq!(app.edits.history.undo_step().map(|s| s.name().to_string()), undo);

        open_at(&mut app, Point::new(250.0, 130.0));
        type_query(&mut app, "svf");
        let _ = app.update(Message::Browser(browser::Message::Submit));
        assert!(app.browser.is_none());
        assert_eq!(app.controller.graph().nodes().count(), before + 1);
        let id = *app.selection.iter().next().expect("the new module is selected");
        assert_eq!(app.selection.len(), 1);
        assert_eq!(app.positions[&id], Point::new(252.0, 132.0));

        app.undo();
        assert_eq!(app.controller.graph().nodes().count(), before);
        assert_eq!(app.edits.history.undo_step().map(|s| s.name().to_string()), undo);
    }

    #[test]
    fn keys_go_to_the_browser_not_the_shortcuts() {
        let mut app = app();
        let before = app.controller.graph().nodes().count();
        open_at(&mut app, Point::ORIGIN);
        type_query(&mut app, "saw");
        let _ = app.update(Message::Browser(browser::Message::Submit));
        assert_eq!(app.controller.graph().nodes().count(), before + 1);

        // Cmd+Z or Ctrl+Z while the browser is open does not undo.
        open_at(&mut app, Point::ORIGIN);
        let _ = app.update(Message::Key(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character("z".into()),
            modified_key: iced::keyboard::Key::Character("z".into()),
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::COMMAND,
            text: None,
            repeat: false,
        }));
        assert_eq!(app.controller.graph().nodes().count(), before + 1);
    }

    /// An envelope and a filter, with the filter's cutoff knob widget.
    fn env_and_filter(app: &mut App) -> (ModuleId, ModuleId, usize, usize) {
        let env = app.controller.add_module("loom.env.adsr").unwrap();
        let filter = app.controller.add_module("loom.filter.svf").unwrap();
        app.commit();
        let d = app.controller.graph().node(filter).unwrap().descriptor;
        let cutoff = d.param_index("cutoff").unwrap();
        let widget = d.panel.widgets.iter().position(|w| w.bind == "cutoff").unwrap();
        app.edits.history.clear();
        (env, filter, cutoff, widget)
    }

    fn drop_env(app: &mut App, env: ModuleId, filter: ModuleId, widget: usize) {
        let from = loom_canvas::PortRef {
            module: env.0,
            output: true,
            index: 0,
        };
        let _ = app.update(Message::Canvas(CanvasEvent::Assign {
            from,
            module: filter.0,
            widget,
        }));
    }

    fn assignments(app: &App) -> Vec<loom_core::Assignment> {
        app.controller.graph().assignments().map(|(_, a)| *a).collect()
    }

    /// Dropping an output on a knob assigns it with depth 0.25, following
    /// the source's polarity. A second drop of the same output adds nothing
    /// and says why. Undo and redo take the assignment back and forth.
    #[test]
    fn a_drop_assigns_once_and_is_one_undo_step() {
        let mut app = app();
        let (env, filter, cutoff, widget) = env_and_filter(&mut app);
        drop_env(&mut app, env, filter, widget);
        let made = assignments(&app);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].source, OutputRef { module: env, port: 0 });
        assert_eq!((made[0].target, made[0].param), (filter, cutoff));
        assert_eq!(made[0].depth, 0.25);
        assert_eq!(made[0].polarity, None);
        assert!(made[0].enabled);
        assert_eq!(
            app.edits.history.undo_step().map(undo::Step::name),
            Some("add modulation")
        );

        drop_env(&mut app, env, filter, widget);
        assert_eq!(assignments(&app).len(), 1);
        assert!(app.status.contains("already modulates"), "{}", app.status);

        app.undo();
        assert!(assignments(&app).is_empty());
        app.redo();
        assert_eq!(assignments(&app), made);
    }

    /// `--test-modules` loads the `test.` modules and lists them in the
    /// module browser, so the example patch for them opens. Without it the
    /// browser lists only the built-ins, though tests can still add them.
    #[test]
    fn test_modules_are_listed_only_with_the_flag() {
        let patch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/dev/test-widgets.ron");
        let app = App::new(Args {
            patch: Some(patch),
            test_modules: true,
            ..app_args()
        });
        // The two test modules and the patch's Transport.
        assert_eq!(app.controller.graph().nodes().count(), 3, "{}", app.status);
        assert_eq!(app.controller.graph().assignments().count(), 7);
        assert!(app.modules.iter().any(|d| d.id == "test.panel"));

        let app = self::app();
        assert!(app.modules.iter().any(|d| d.id == "loom.osc.sine"));
        assert!(!app.modules.iter().any(|d| d.id.starts_with("test.")));
    }

    /// A drop on a toggle gives depth 1, and on radio buttons with three
    /// options 0.5: one step of the parameter. The assignment follows the
    /// source's polarity, unipolar or bipolar (spec, Making an assignment).
    #[test]
    fn a_drop_on_a_toggle_or_radio_moves_one_step() {
        let mut app = app();
        let source = app.controller.add_module("test.panel").unwrap();
        let target = app.controller.add_module("test.panel").unwrap();
        app.commit();
        let d = app.controller.graph().node(target).unwrap().descriptor;
        let cases = [("on", 1.0), ("choice", 0.5)];
        let outputs = [(0, loom_core::Polarity::Unipolar), (1, loom_core::Polarity::Bipolar)];
        for (port, _) in outputs {
            for (bind, _) in cases {
                let widget = d.panel.widgets.iter().position(|w| w.bind == bind).unwrap();
                let _ = app.update(Message::Canvas(CanvasEvent::Assign {
                    from: loom_canvas::PortRef {
                        module: source.0,
                        output: true,
                        index: port,
                    },
                    module: target.0,
                    widget,
                }));
            }
        }

        let graph = app.controller.graph();
        assert_eq!(graph.assignments().count(), 4);
        for (port, polarity) in outputs {
            for (bind, depth) in cases {
                let param = d.param_index(bind).unwrap();
                let made = assignments(&app)
                    .into_iter()
                    .find(|a| a.source == OutputRef { module: source, port } && a.param == param)
                    .unwrap_or_else(|| panic!("no assignment from output {port} to {bind}"));
                assert_eq!(made.target, target);
                assert_eq!(made.depth, depth, "{bind}");
                assert_eq!(made.polarity, None, "{bind} follows the source");
                let reach = modulation::reach(graph, &made).unwrap();
                assert_eq!(reach.polarity, polarity, "{bind} from output {port}");
            }
        }
    }

    /// **Add modulation** makes the same assignment as a drop, from a list
    /// of every output in module order, where outputs that already drive
    /// the parameter cannot be picked.
    #[test]
    fn add_modulation_matches_a_drop() {
        let mut app = app();
        let (env, filter, cutoff, widget) = env_and_filter(&mut app);
        let source = OutputRef { module: env, port: 0 };
        let graph = app.controller.graph();
        let sources = modulation::sources(graph);
        // The new patch's Transport comes first; the envelope is next.
        let transport = app.controller.transport_module();
        assert_eq!(sources.iter().find(|s| Some(s.module) != transport), Some(&source));
        let mut sorted = sources.clone();
        sorted.sort();
        assert_eq!(sources, sorted, "module order, then port order");

        let _ = app.update(Message::AddModulation {
            module: filter.0,
            index: cutoff,
        });
        assert_eq!(app.adding, Some((filter.0, cutoff)));
        let _ = app.update(Message::Assign {
            source,
            module: filter.0,
            index: cutoff,
        });
        assert_eq!(app.adding, None);
        let picked = assignments(&app);
        assert!(modulation::exists(app.controller.graph(), source, filter, cutoff));

        let mut dropped = self::app();
        let (env, filter, _, widget2) = env_and_filter(&mut dropped);
        assert_eq!(widget, widget2);
        drop_env(&mut dropped, env, filter, widget);
        assert_eq!(assignments(&dropped), picked);
    }

    /// The inspector row edits depth, polarity, and on/off, and removes. A
    /// depth slider drag is one undo step. None of these but removal
    /// rebuilds the scene, so the GPU keeps its tiles, ports, and cables.
    #[test]
    fn inspector_rows_edit_and_remove_assignments() {
        let mut app = app();
        let (env, filter, cutoff, widget) = env_and_filter(&mut app);
        drop_env(&mut app, env, filter, widget);
        let id = app.controller.graph().assignments().next().unwrap().0;
        let generation = app.scene.generation();
        let steps = app.edits.history.len();

        for depth in [0.3, 0.4, 0.5] {
            let _ = app.update(Message::Depth { id, depth });
        }
        let _ = app.update(Message::SliderReleased);
        assert_eq!(assignments(&app)[0].depth, 0.5);
        assert_eq!(app.edits.history.len(), steps + 1, "one drag, one step");

        let _ = app.update(Message::Polarity {
            id,
            choice: PolarityChoice::Bipolar,
        });
        let _ = app.update(Message::Enabled { id, enabled: false });
        let a = assignments(&app)[0];
        assert_eq!(a.polarity, Some(loom_core::Polarity::Bipolar));
        assert!(!a.enabled);
        assert_eq!(app.scene.generation(), generation, "no scene rebuild for edits");

        app.undo();
        app.undo();
        app.undo();
        assert_eq!(assignments(&app)[0].depth, 0.25);

        let _ = app.update(Message::Unassign(id));
        assert!(assignments(&app).is_empty());
        assert!(modulation::on_param(app.controller.graph(), filter, cutoff).is_empty());
    }

    /// The source's output shows how many assignments it drives, counting
    /// those that are off, and follows adds, removals, and deletes.
    #[test]
    fn badges_count_every_assignment_on_an_output() {
        let mut app = app();
        let (env, filter, _, widget) = env_and_filter(&mut app);
        let badge = |app: &App| app.scene.module(env.0).unwrap().outputs[0].badge;
        assert_eq!(badge(&app), 0);
        drop_env(&mut app, env, filter, widget);
        let resonance = app
            .controller
            .graph()
            .node(filter)
            .unwrap()
            .descriptor
            .param_index("resonance")
            .unwrap();
        let _ = app.update(Message::Assign {
            source: OutputRef { module: env, port: 0 },
            module: filter.0,
            index: resonance,
        });
        assert_eq!(badge(&app), 2);
        let id = app.controller.graph().assignments().next().unwrap().0;
        let _ = app.update(Message::Enabled { id, enabled: false });
        assert_eq!(badge(&app), 2, "off still counts");
        let _ = app.update(Message::Unassign(id));
        assert_eq!(badge(&app), 1);
        let _ = app.update(Message::Canvas(CanvasEvent::Delete(vec![filter.0])));
        assert_eq!(badge(&app), 0);
    }

    /// **Drives** lists what each output drives, in the order added, and a
    /// line selects its target.
    #[test]
    fn drives_lists_targets_and_selects_them() {
        let mut app = app();
        let (env, filter, cutoff, widget) = env_and_filter(&mut app);
        drop_env(&mut app, env, filter, widget);
        let drives = modulation::drives(app.controller.graph(), env);
        let targets: Vec<_> = drives[&0].iter().map(|(_, a)| (a.target, a.param)).collect();
        assert_eq!(targets, vec![(filter, cutoff)]);
        assert!(modulation::drives(app.controller.graph(), filter).is_empty());
        let _ = app.update(Message::SelectModule(filter.0));
        assert_eq!(app.selection, BTreeSet::from([filter.0]));
    }

    /// Saved and loaded, the inspector lists the same assignments in the
    /// same order.
    #[test]
    fn assignments_keep_their_order_through_a_save() {
        let mut app = app();
        let (env, filter, cutoff, widget) = env_and_filter(&mut app);
        let other = app.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
        let _ = app.update(Message::Assign {
            source: OutputRef { module: other, port: 0 },
            module: filter.0,
            index: cutoff,
        });
        drop_env(&mut app, env, filter, widget);
        let rows = |c: &Controller| -> Vec<(OutputRef, f32)> {
            modulation::on_param(c.graph(), filter, cutoff)
                .into_iter()
                .map(|(_, a)| (a.source, a.depth))
                .collect()
        };
        let before = rows(&app.controller);
        assert_eq!(before[0].0.module, other, "the sine was added first");

        let text = Patch::from_graph("Order", app.controller.graph()).to_ron().unwrap();
        let mut reloaded = create_engine(loom_modules::registry(), EngineConfig::default());
        Patch::from_ron(&text)
            .unwrap()
            .load_into(&mut reloaded.controller)
            .unwrap();
        assert_eq!(rows(&reloaded.controller), before);
    }

    /// An envelope and a sine both on the filter cutoff.
    fn two_sources(app: &mut App) -> (ModuleId, ModuleId, ModuleId, usize, usize) {
        let (env, filter, cutoff, widget) = env_and_filter(app);
        let sine = app.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
        drop_env(app, env, filter, widget);
        let _ = app.update(Message::Assign {
            source: OutputRef { module: sine, port: 0 },
            module: filter.0,
            index: cutoff,
        });
        (env, sine, filter, cutoff, widget)
    }

    fn out(module: ModuleId) -> OutputRef {
        OutputRef { module, port: 0 }
    }

    /// The arc drag picks the pinned source, then the selected module's
    /// output, then the only enabled assignment, and otherwise nothing.
    #[test]
    fn the_arc_drag_picks_its_assignment_in_order() {
        let mut app = app();
        let (env, sine, filter, cutoff, _) = two_sources(&mut app);
        let graph = app.controller.graph();
        let id_of = |source: OutputRef| {
            graph
                .assignments()
                .find(|(_, a)| a.source == source)
                .map(|(id, _)| id)
                .unwrap()
        };
        let pick = |pinned, selected| modulation::drag_target(graph, filter, cutoff, pinned, selected);
        assert_eq!(pick(Some(out(sine)), Some(env)), Ok(id_of(out(sine))));
        assert_eq!(pick(None, Some(env)), Ok(id_of(out(env))));
        // A pinned output that does not drive this knob falls through.
        assert_eq!(pick(Some(out(filter)), Some(sine)), Ok(id_of(out(sine))));
        assert!(pick(None, Some(filter)).is_err(), "two enabled: none fits");
        assert!(pick(None, None).unwrap_err().contains("pin one"));

        // With one of them off, the other is the only enabled one.
        let (sine_id, env_id) = (id_of(out(sine)), id_of(out(env)));
        app.controller.set_assignment_enabled(sine_id, false).unwrap();
        let graph = app.controller.graph();
        assert_eq!(modulation::drag_target(graph, filter, cutoff, None, None), Ok(env_id));
    }

    /// Two assignments from the same output (only possible in a hand-written
    /// patch) make the drag change nothing, even when that output is pinned.
    #[test]
    fn a_duplicate_source_is_not_a_drag_target() {
        let mut app = app();
        let (env, filter, cutoff, _) = env_and_filter(&mut app);
        for depth in [0.1, 0.2] {
            app.controller.add_assignment(out(env), filter, cutoff, depth).unwrap();
        }
        let graph = app.controller.graph();
        for (pinned, selected) in [(Some(out(env)), None), (None, Some(env))] {
            let why = modulation::drag_target(graph, filter, cutoff, pinned, selected).unwrap_err();
            assert!(why.contains("inspector"), "{why}");
        }
    }

    /// One arc drag changes one assignment's depth as one undo step. While
    /// it lasts, the knob's arc shows only that assignment.
    #[test]
    fn an_arc_drag_is_one_step_and_shows_only_its_assignment() {
        let mut app = app();
        let (_, sine, filter, _, widget) = two_sources(&mut app);
        let steps = app.edits.history.len();
        let pinned = Some(loom_canvas::PortRef {
            module: sine.0,
            output: true,
            index: 0,
        });
        let drag = |delta| {
            Message::Canvas(CanvasEvent::DragDepth {
                module: filter.0,
                widget,
                pinned,
                delta,
            })
        };
        let mark = |app: &App| app.values.get(filter.0, widget).unwrap().modulation;
        let both = mark(&app);
        let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
        let _ = app.update(drag(0.1));
        let _ = app.update(drag(0.1));
        let during = mark(&app);
        assert_ne!(during, both, "the arc shows the dragged assignment alone");
        let _ = app.update(Message::Canvas(CanvasEvent::GestureEnd));
        assert_eq!(app.depth_drag, None);
        let depth = |app: &App| {
            app.controller
                .graph()
                .assignments()
                .find(|(_, a)| a.source == out(sine))
                .unwrap()
                .1
                .depth
        };
        assert!((depth(&app) - 0.45).abs() < 1e-6, "{}", depth(&app));
        assert!(app.status.contains("+45%"), "{}", app.status);
        assert_eq!(app.edits.history.len(), steps + 1);
        app.undo();
        assert_eq!(depth(&app), 0.25);
    }

    /// A selected source picks the assignment even though the drag starts
    /// on another module's knob: the press does not change the selection.
    #[test]
    fn a_selected_source_picks_the_arc_drag() {
        let mut app = app();
        let (env, sine, filter, _, widget) = two_sources(&mut app);
        app.selection = BTreeSet::from([sine.0]);
        let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
        let _ = app.update(Message::Canvas(CanvasEvent::DragDepth {
            module: filter.0,
            widget,
            pinned: None,
            delta: 0.1,
        }));
        let _ = app.update(Message::Canvas(CanvasEvent::GestureEnd));
        let depth = |source| {
            app.controller
                .graph()
                .assignments()
                .find(|(_, a)| a.source == out(source))
                .unwrap()
                .1
                .depth
        };
        assert!((depth(sine) - 0.35).abs() < 1e-6);
        assert_eq!(depth(env), 0.25);
    }

    /// With several sources and none in focus, a drag changes nothing and
    /// the status bar says why.
    #[test]
    fn an_arc_drag_with_no_choice_changes_nothing() {
        let mut app = app();
        let (_, _, filter, _, widget) = two_sources(&mut app);
        app.selection.clear();
        let before = assignments(&app);
        let steps = app.edits.history.len();
        let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
        let _ = app.update(Message::Canvas(CanvasEvent::DragDepth {
            module: filter.0,
            widget,
            pinned: None,
            delta: 0.2,
        }));
        let _ = app.update(Message::Canvas(CanvasEvent::GestureEnd));
        assert_eq!(assignments(&app), before);
        assert_eq!(app.edits.history.len(), steps);
        assert!(app.status.contains("Several sources"), "{}", app.status);
    }

    /// The scene has one line per assignment, ending on its control.
    #[test]
    fn the_scene_has_a_line_per_assignment() {
        let mut app = app();
        let (env, sine, filter, _, widget) = two_sources(&mut app);
        let lines: Vec<_> = app
            .scene
            .modulations()
            .iter()
            .map(|m| (m.from, m.to, m.widget))
            .collect();
        assert_eq!(
            lines,
            vec![
                ((env.0, 0), filter.0, Some(widget)),
                ((sine.0, 0), filter.0, Some(widget))
            ]
        );
    }

    /// Ctrl+`c` or Cmd+`c`.
    pub(crate) fn command_key(c: &str) -> Message {
        Message::Key(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character(c.into()),
            modified_key: iced::keyboard::Key::Character(c.into()),
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::COMMAND,
            text: None,
            repeat: false,
        })
    }

    /// A sine into a multiply, both selected, at known places.
    fn two_selected(app: &mut App) -> (ModuleId, ModuleId) {
        let sine = app.controller.add_module("loom.osc.sine").unwrap();
        let vca = app.controller.add_module("loom.math.mul").unwrap();
        app.controller.connect_named(sine, "out", vca, "a").unwrap();
        app.controller.set_param_named(vca, "gain", 0.4).unwrap();
        app.positions.insert(sine.0, Point::new(0.0, 0.0));
        app.positions.insert(vca.0, Point::new(240.0, 0.0));
        app.commit();
        app.edits.history.clear();
        app.selection = BTreeSet::from([sine.0, vca.0]);
        (sine, vca)
    }

    fn step_name(app: &App) -> Option<String> {
        app.edits.history.undo_step().map(|s| s.name().to_string())
    }

    #[test]
    fn copy_with_nothing_selected_writes_nothing() {
        let mut app = app();
        app.selection.clear();
        assert!(app.copy().is_none());
        assert_eq!(app.status, "Nothing selected to copy");
        assert!(app.edits.history.undo_step().is_none(), "copy is not an edit");
    }

    #[test]
    fn a_paste_lands_at_the_snapped_point_is_selected_and_is_one_undo_step() {
        let mut app = app();
        let (sine, vca) = two_selected(&mut app);
        let text = app.copy().unwrap();
        assert_eq!(app.status, "Copied 2 modules");
        assert!(app.edits.history.undo_step().is_none(), "copy is not an edit");

        let before = app.controller.graph().nodes().count();
        app.paste(Point::new(605.0, 299.0), Some(text));
        assert_eq!(app.controller.graph().nodes().count(), before + 2);
        assert_eq!(app.status, "Pasted 2 modules");
        assert_eq!(step_name(&app).as_deref(), Some("paste 2 modules"));
        let new: Vec<u32> = app.selection.iter().copied().collect();
        assert_eq!(new.len(), 2, "only the pasted modules are selected");
        assert!(!new.contains(&sine.0) && !new.contains(&vca.0));
        assert_eq!(app.positions[&new[0]], Point::new(600.0, 300.0));
        assert_eq!(app.positions[&new[1]], Point::new(840.0, 300.0));

        app.undo();
        assert_eq!(app.controller.graph().nodes().count(), before);
        app.redo();
        let back: Vec<u32> = app
            .controller
            .graph()
            .nodes()
            .map(|(id, _)| id.0)
            .filter(|id| new.contains(id))
            .collect();
        assert_eq!(back, new, "redo brings the paste back with the same ids");
        assert_eq!(app.controller.graph().cables().count(), 2);
    }

    #[test]
    fn duplicate_from_the_keyboard_is_one_step_offset_by_two_grid_units() {
        let mut app = app();
        let (sine, _) = two_selected(&mut app);
        let before = app.controller.graph().nodes().count();
        let _ = app.update(command_key("d"));
        assert_eq!(app.controller.graph().nodes().count(), before + 2);
        assert_eq!(step_name(&app).as_deref(), Some("duplicate 2 modules"));
        let first = *app.selection.iter().next().unwrap();
        assert_ne!(first, sine.0);
        assert_eq!(app.positions[&first], Point::new(24.0, 24.0));
        app.undo();
        assert_eq!(app.controller.graph().nodes().count(), before);
    }

    #[test]
    fn cut_copies_and_deletes_as_one_step() {
        let mut app = app();
        let (sine, vca) = two_selected(&mut app);
        let before = app.controller.graph().nodes().count();
        let text = app.cut().unwrap();
        assert!(text.contains("loom.osc.sine"));
        assert_eq!(app.controller.graph().nodes().count(), before - 2);
        assert_eq!(step_name(&app).as_deref(), Some("cut 2 modules"));
        assert_eq!(app.edits.history.len(), 1);
        app.undo();
        assert!(app.controller.graph().node(sine).is_some() && app.controller.graph().node(vca).is_some());
        assert_eq!(app.controller.graph().cables().count(), 1);
    }

    #[test]
    fn a_refused_paste_adds_nothing_and_no_step() {
        let mut app = app();
        let before = app.controller.graph().nodes().count();
        app.paste(Point::ORIGIN, None);
        assert_eq!(app.status, clipboard::NOTHING_TO_PASTE);
        app.paste(Point::ORIGIN, Some("not a patch".to_string()));
        assert_eq!(app.status, clipboard::NOTHING_TO_PASTE);
        app.paste(
            Point::ORIGIN,
            Some("Patch(version: 1, modules: [(id: 1, kind: \"loom.none.such\")])".to_string()),
        );
        assert_eq!(app.status, "Cannot paste: unknown module `loom.none.such`");
        assert_eq!(app.controller.graph().nodes().count(), before);
        assert!(app.edits.history.undo_step().is_none());
    }

    #[test]
    fn clipboard_keys_go_to_the_browser_while_it_is_open() {
        let mut app = app();
        two_selected(&mut app);
        let before = app.controller.graph().nodes().count();
        open_at(&mut app, Point::ORIGIN);
        let _ = app.update(command_key("d"));
        let _ = app.update(command_key("x"));
        assert_eq!(app.controller.graph().nodes().count(), before);
    }

    /// A command's step name never labels a later edit, even when the
    /// command made no step.
    #[test]
    fn a_step_name_does_not_outlive_its_edit() {
        let mut app = app();
        let (_, vca) = two_selected(&mut app);
        app.edits.set_history_off(true);
        app.step_name = Some("cut 2 modules".to_string());
        app.edit(|_| {});
        assert!(app.step_name.is_none());
        app.edits.set_history_off(false);
        app.edit(|app| {
            app.controller.set_param_named(vca, "gain", 0.9).unwrap();
            app.commit();
        });
        assert_eq!(step_name(&app).as_deref(), Some("change Multiply Gain"));
    }

    /// A clipboard read that finishes after another patch opened is dropped.
    #[test]
    fn a_paste_for_a_closed_patch_is_dropped() {
        let mut app = app();
        two_selected(&mut app);
        let text = app.copy().unwrap();
        let before = app.controller.graph().nodes().count();
        let _ = app.update(Message::Pasted {
            world: Point::ORIGIN,
            text: Some(text.clone()),
            generation: app.generation + 1,
        });
        assert_eq!(app.controller.graph().nodes().count(), before);
        let _ = app.update(Message::Pasted {
            world: Point::ORIGIN,
            text: Some(text),
            generation: app.generation,
        });
        assert_eq!(app.controller.graph().nodes().count(), before + 2);
    }

    /// Panic is not an edit: the value, the cables, and the unsaved mark
    /// stay, and there is no new undo step (panic.md, Acceptance criteria).
    #[test]
    fn panic_is_not_an_edit() {
        let mut app = app();
        let (_, vca) = two_selected(&mut app);
        app.edit(|app| {
            app.controller.set_param_named(vca, "gain", 0.9).unwrap();
            app.commit();
        });
        assert!(app.unsaved());
        let steps = app.edits.history.len();
        let cables = app.controller.graph().cables().count();
        app.holding = true;
        let _ = app.update(Message::Panic);
        // The MIDI poll runs while the app has an engine, and finds nothing.
        let _ = app.subscription();
        let _ = app.update(Message::MidiPoll);
        assert!(app.status.starts_with("Panic"), "{}", app.status);
        assert!(!app.holding, "Hold note lets go");
        let gain = app
            .controller
            .graph()
            .node(vca)
            .unwrap()
            .descriptor
            .param_index("gain")
            .unwrap();
        assert_eq!(app.controller.graph().node(vca).unwrap().params[gain], 0.9);
        assert_eq!(app.controller.graph().cables().count(), cables);
        assert!(app.unsaved(), "the unsaved mark stays");
        assert_eq!(app.edits.history.len(), steps, "no undo step");
    }

    /// The panic key works with a text field focused, the toolbar button
    /// runs it, and so do the control channel's `panic` and `press`.
    #[test]
    fn the_panic_key_button_and_command_run_panic() {
        let mut ui = crate::ui_test::Ui::open("subtractive.ron");
        let _ = ui.app.update(Message::OpenBrowser);
        ui.redraw();
        ui.typewrite("sin");
        let messages = ui.tap(
            iced::keyboard::Key::Character(".".into()),
            iced::keyboard::Modifiers::COMMAND,
        );
        assert!(messages.iter().any(|m| matches!(m, Message::Panic)), "{messages:?}");
        assert!(ui.app.status.starts_with("Panic"), "{}", ui.app.status);

        ui.app.status.clear();
        let _ = ui.app.update(Message::Browser(browser::Message::Close));
        ui.redraw();
        let button = ui.text("Panic").expect("a Panic button on the toolbar");
        ui.move_to(button.center());
        let mut messages = ui.press();
        messages.extend(ui.release());
        assert!(messages.iter().any(|m| matches!(m, Message::Panic)), "{messages:?}");
    }

    /// A momentary press sets the transient hold: the tile shows it, but
    /// the patch is unchanged, the saved file has no `hold`, the inspector
    /// has no row for it, and a copy pastes a released button
    /// (transient-params.md, Acceptance criteria).
    #[test]
    fn a_momentary_hold_is_not_part_of_the_patch() {
        let mut app = app();
        let id = app.controller.add_module("loom.ui.button").unwrap();
        app.controller.set_param_named(id, "mode", 1.0).unwrap();
        app.positions.insert(id.0, Point::new(0.0, 0.0));
        app.commit();
        app.edits.history.clear();
        app.edits.history.mark_saved();
        let hold = app
            .controller
            .graph()
            .node(id)
            .unwrap()
            .descriptor
            .param_index("hold")
            .unwrap();
        let held = |app: &App| app.controller.graph().node(id).unwrap().params[hold];

        let _ = app.update(Message::Canvas(CanvasEvent::ButtonPressed {
            module: id.0,
            widget: 0,
        }));
        assert_eq!(held(&app), 1.0);
        assert_eq!(tile_widget(&app, id, 0).position, 1.0, "a held button looks held");
        assert!(!app.unsaved(), "a hold does not mark the patch changed");
        assert_eq!(app.edits.history.len(), 0, "a hold is no undo step");
        let text = app.current_patch().to_ron().unwrap();
        assert!(!text.contains("hold"), "{text}");
        app.arm((id.0, hold));
        assert!(app.armed.is_none(), "a hold cannot be a MIDI learn target");

        app.selection = BTreeSet::from([id.0]);
        let snapshot = app.ui_snapshot();
        let rows = serde_json::to_string(&snapshot.inspector).unwrap();
        assert!(!rows.contains("\"Hold\""), "{rows}");
        let copied = app.copy().unwrap();
        let before: BTreeSet<u32> = app.controller.graph().nodes().map(|(m, _)| m.0).collect();
        let _ = app.update(Message::Pasted {
            world: Point::new(400.0, 0.0),
            text: Some(copied),
            generation: app.generation,
        });
        let (pasted, node) = app
            .controller
            .graph()
            .nodes()
            .find(|(m, _)| !before.contains(&m.0))
            .expect("a pasted button");
        assert_ne!(pasted, id);
        assert_eq!(node.params[hold], 0.0, "a pasted button is released");
        assert_eq!(held(&app), 1.0, "the original is still held");

        let _ = app.update(Message::Canvas(CanvasEvent::ButtonReleased {
            module: id.0,
            widget: 0,
        }));
        assert_eq!(held(&app), 0.0);
        assert_eq!(tile_widget(&app, id, 0).position, 0.0);
    }
    /// The widget on module `id`'s tile at `widget`, as the snapshot shows it.
    fn tile_widget(app: &App, id: ModuleId, widget: usize) -> snapshot::Widget {
        let snapshot = app.ui_snapshot();
        let tile = snapshot.tiles.iter().find(|t| t.id == id.0).expect("a tile");
        tile.widgets[widget].clone()
    }

    /// A modulated knob shows no live value until the engine runs, then
    /// follows it through the app's own `LiveFrame` path.
    #[test]
    fn a_running_engine_moves_the_live_dot() {
        let mut app = app();
        let source = app.controller.add_module("loom.math.constant").unwrap();
        app.controller.set_param_named(source, "value", 0.5).unwrap();
        let (_, filter, cutoff, widget) = env_and_filter(&mut app);
        let _ = app.update(Message::Assign {
            source: out(source),
            module: filter.0,
            index: cutoff,
        });
        let _ = app.update(Message::LiveFrame);
        assert_eq!(tile_widget(&app, filter, widget).live, None, "the engine has not run");

        app.process(64);
        let _ = app.update(Message::LiveFrame);
        let knob = tile_widget(&app, filter, widget);
        let live = knob.live.expect("a live value");
        assert!(live > knob.position, "a positive source raises the cutoff: {knob:?}");
        let snapshot::Mark::Range { low, high } = knob.modulation else {
            panic!("an arc: {knob:?}");
        };
        assert!((low..=high).contains(&live), "the dot is on the arc: {knob:?}");
    }

    /// A light on screen follows the engine: the envelope's level light is
    /// dark at rest and lights up while its gate is held.
    #[test]
    fn a_running_engine_lights_the_envelope() {
        let mut app = app();
        let gate = app.controller.add_module("loom.math.constant").unwrap();
        app.controller.set_param_named(gate, "value", 1.0).unwrap();
        let env = app.controller.add_module("loom.env.adsr").unwrap();
        app.commit();
        let d = app.controller.graph().node(env).unwrap().descriptor;
        let light = d.panel.widgets.iter().position(|w| w.bind == "level").unwrap();
        let _ = app.update(Message::Canvas(CanvasEvent::VisibleDisplays(vec![env.0])));
        let watch = app.watched_lights().expect("the level light is on screen");

        app.process(480);
        let _ = app.update(Message::Lights(watch.read()));
        assert_eq!(tile_widget(&app, env, light).position, 0.0, "no gate, no level");

        app.controller.connect_named(gate, "out", env, "gate").unwrap();
        app.commit();
        app.process(4_800);
        // A level light draws its value as it is, with no fade.
        let mut fades = vec![lights::Fade::default(); watch.lights.len()];
        let _ = app.update(Message::Lights(watch.shown(&mut fades, Instant::now())));
        let level = tile_widget(&app, env, light).position;
        assert!(level > 0.5, "the gate is held: {level}");
    }

    /// Button's mode shows its choice names, "Toggle" and "Momentary", as
    /// radio buttons, and a parameter without names keeps its slider. No
    /// module is special-cased: the names come from the descriptor
    /// (panels.md, Choice names). A pick is one undo step.
    #[test]
    fn the_inspector_names_the_choices_of_a_whole_number_parameter() {
        let mut app = app();
        let button = app.controller.add_module("loom.ui.button").unwrap();
        let lfo = app.controller.add_module("loom.mod.lfo").unwrap();
        app.commit();
        let rows = |app: &App, module: ModuleId| {
            let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
                panic!("one module is selected");
            };
            assert_eq!(rows.id, module.0);
            rows.params
        };
        let control = |row: &inspector::ParamRow| match &row.mode {
            inspector::ParamMode::Edit { control, .. } => control.clone(),
            inspector::ParamMode::Learn { .. } => panic!("not in learn mode"),
        };
        let _ = app.update(Message::SelectModule(button.0));
        let params = rows(&app, button);
        let mode = params.iter().find(|p| p.label == "Mode").unwrap();
        assert_eq!(
            control(mode),
            inspector::ParamControl::Radio {
                choices: vec!["Toggle", "Momentary"],
                selected: 0
            }
        );
        assert_eq!(mode.value, "Toggle", "the value is the name, never the bare number");
        let on = params.iter().find(|p| p.label != "Mode").unwrap();
        assert_eq!(control(on), inspector::ParamControl::Slider);

        let undo = app.edits.history.undo_step().map(|s| s.name().to_string());
        let _ = app.update(Message::Choice {
            module: button.0,
            index: mode.index,
            choice: 1,
        });
        assert_eq!(app.controller.graph().node(button).unwrap().params[mode.index], 1.0);
        let mode = rows(&app, button).into_iter().find(|p| p.label == "Mode").unwrap();
        assert_eq!(mode.value, "Momentary");
        assert_ne!(
            app.edits.history.undo_step().map(|s| s.name().to_string()),
            undo,
            "one step"
        );
        let _ = app.update(Message::Undo);
        assert_eq!(app.controller.graph().node(button).unwrap().params[mode.index], 0.0);

        // More than four names: a pick list.
        let _ = app.update(Message::SelectModule(lfo.0));
        let shape = rows(&app, lfo).into_iter().find(|p| p.label == "Shape").unwrap();
        let inspector::ParamControl::PickList { choices, selected } = control(&shape) else {
            panic!("five shapes are a pick list: {shape:?}");
        };
        assert_eq!(choices.len(), 5);
        assert_eq!(selected, 0);
    }

    /// A saved range that does not hold its value loads with the smallest
    /// range that does, the value unchanged, and a note in the status bar
    /// (control-ranges.md, rule 7).
    #[test]
    fn a_range_note_shows_in_the_status_bar() {
        let dir = std::env::temp_dir().join(format!("loom-app-range-note-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("range.ron");
        std::fs::write(
            &path,
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 5.0}, ranges: {"gain": (-0.1, 0.1)})])"#,
        )
        .unwrap();
        let app = App::new(Args {
            patch: Some(path),
            ..app_args()
        });
        let _ = std::fs::remove_dir_all(&dir);
        let node = app.controller.graph().node(ModuleId(1)).unwrap();
        assert_eq!(
            (node.params[0], node.range(0)),
            (5.0, Some(loom_core::ParamRange::new(-5.0, 5.0)))
        );
        assert!(app.status.contains("-0.1 to 0.1"), "{}", app.status);
    }

    /// Opening a patch that loads as saved says only "opened <path>". One
    /// with a load note says the note after it.
    #[test]
    fn opening_a_patch_names_only_its_load_notes() {
        let mut app = app();
        let clean = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
        app.open_path(&clean);
        assert_eq!(app.status, format!("opened {}", clean.display()));

        let dir = std::env::temp_dir().join(format!("loom-app-open-note-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let noted = dir.join("noted.ron");
        std::fs::write(
            &noted,
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 5.0}, ranges: {"gain": (-0.1, 0.1)})])"#,
        )
        .unwrap();
        app.open_path(&noted);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            app.status.starts_with(&format!("opened {}: module 1", noted.display())),
            "{}",
            app.status
        );

        app.open_path(&clean);
        assert_eq!(
            app.status,
            format!("opened {}", clean.display()),
            "the old note is gone"
        );
    }

    /// control-ranges.md: Multiply's gain shows a **Range** selector. A
    /// smaller range clamps the value, as one undo step, and the tile knob
    /// then spans the selected range. Undo brings back range and value.
    #[test]
    fn the_inspector_selects_a_control_range() {
        let mut app = app();
        let mul = app.controller.add_module("loom.math.mul").unwrap();
        app.controller.set_param(mul, 0, 0.5).unwrap();
        app.commit();
        let _ = app.update(Message::SelectModule(mul.0));
        let gain_row = |app: &App| {
            let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
                panic!("one module is selected");
            };
            rows.params.into_iter().find(|p| p.label == "Gain").unwrap()
        };
        let range = gain_row(&app).range.expect("a Range selector");
        assert_eq!(range.choices.len(), 7);
        assert_eq!(range.choices[0], "-0.1 to 0.1");
        assert_eq!(range.choices[range.selected.unwrap()], "-1 to 1");

        let _ = app.update(Message::Range {
            module: mul.0,
            index: 0,
            range: 0,
        });
        let row = gain_row(&app);
        assert_eq!(row.range.unwrap().selected, Some(0));
        assert_eq!(app.controller.graph().node(mul).unwrap().params[0], 0.1, "clamped");
        assert_eq!(
            app.edits.history.undo_step().map(|s| s.name().to_string()).as_deref(),
            Some("change Multiply Gain range")
        );
        // The tile knob and its keyboard steps span -0.1 to 0.1.
        let widget = app
            .controller
            .graph()
            .node(mul)
            .unwrap()
            .descriptor
            .panel
            .widgets
            .iter()
            .position(|w| w.bind == "gain")
            .unwrap();
        let (_, _, spec) = scene::control_param(&app.controller, mul.0, widget).unwrap();
        assert_eq!((spec.min, spec.max), (-0.1, 0.1));
        assert!(
            (spec.denormalize(0.51) - spec.denormalize(0.5) - 0.002).abs() < 1e-6,
            "1% of the range"
        );

        let _ = app.update(Message::Undo);
        let node = app.controller.graph().node(mul).unwrap();
        assert_eq!(
            (node.params[0], node.range(0)),
            (0.5, Some(loom_core::ParamRange::new(-1.0, 1.0)))
        );
        // A parameter without ranges has no selector.
        let lfo = app.controller.add_module("loom.mod.lfo").unwrap();
        app.commit();
        let _ = app.update(Message::SelectModule(lfo.0));
        let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
            panic!("one module is selected");
        };
        assert!(rows.params.iter().all(|p| p.range.is_none()));
    }

    /// A one-sample trigger lights Clock Divide's trigger light, though the
    /// output went low again long before the light is read. The light fades
    /// out in 120 ms, and a second read without a trigger is dark
    /// (panels.md, Lights for one-sample triggers).
    #[test]
    fn a_one_sample_trigger_lights_the_divide_light() {
        let mut app = app();
        let gate = app.controller.add_module("loom.math.constant").unwrap();
        app.controller.set_param_named(gate, "value", 1.0).unwrap();
        let divide = app.controller.add_module("loom.logic.divide").unwrap();
        app.controller.set_param_named(divide, "n", 1.0).unwrap();
        app.controller.connect_named(gate, "out", divide, "trigger").unwrap();
        app.commit();
        let d = app.controller.graph().node(divide).unwrap().descriptor;
        let light = d.panel.widgets.iter().position(|w| w.bind == "fired").unwrap();
        let _ = app.update(Message::Canvas(CanvasEvent::VisibleDisplays(vec![divide.0])));
        let watch = app.watched_lights().expect("the trigger light is on screen");

        // The constant's rise is one trigger in the first frame, then 4 799
        // frames of 0.
        app.process(4_800);
        let start = Instant::now();
        let mut fades = vec![lights::Fade::default(); watch.lights.len()];
        let _ = app.update(Message::Lights(watch.shown(&mut fades, start)));
        assert_eq!(tile_widget(&app, divide, light).position, 1.0, "the trigger lit it");

        app.process(4_800);
        let later = start + lights::FADE;
        let _ = app.update(Message::Lights(watch.shown(&mut fades, later)));
        assert_eq!(
            tile_widget(&app, divide, light).position,
            0.0,
            "faded, and no new trigger"
        );
    }

    /// A meter on screen follows the engine through the app's `Meters`
    /// path: empty before the engine runs, then a sine's level, with its
    /// peak above the RMS level (#222).
    #[test]
    fn a_running_engine_fills_the_meter() {
        let mut app = app();
        let phase = app.controller.add_module("loom.phase.generator").unwrap();
        let sine = app.controller.add_module("loom.osc.sine").unwrap();
        let meter = app.controller.add_module("loom.display.meter").unwrap();
        app.controller.connect_named(phase, "phase", sine, "phase").unwrap();
        app.controller.connect_named(sine, "out", meter, "in").unwrap();
        app.commit();
        let d = app.controller.graph().node(meter).unwrap().descriptor;
        let widget = d.panel.widgets.iter().position(|w| w.bind == "out").unwrap();
        // On screen, so the meter gets a probe.
        let _ = app.update(Message::Canvas(CanvasEvent::VisibleDisplays(vec![meter.0])));
        let _ = app.update(Message::Meters(app.monitor.read()));
        let before = tile_widget(&app, meter, widget);
        assert_eq!((before.position, before.peak), (0.0, 0.0), "the engine has not run");

        app.process(4_800);
        let _ = app.update(Message::Meters(app.monitor.read()));
        let shown = tile_widget(&app, meter, widget);
        assert_eq!(shown.kind, "meter");
        assert!(shown.position > 0.0, "a level: {shown:?}");
        assert!(shown.peak > shown.position, "a sine peaks above its RMS: {shown:?}");
        assert!(shown.peak > 0.99, "a full-scale sine: {shown:?}");
    }

    /// A MIDI mapping applied in the engine shows on the knob: a control
    /// change sent through the engine's MIDI input moves the mapped cutoff
    /// to the end of its range once the app reads it with `Midi` (#222).
    #[test]
    fn a_mapped_control_change_turns_the_knob() {
        let mut app = app();
        let (_, filter, cutoff, widget) = env_and_filter(&mut app);
        let control = loom_core::MidiControl {
            channel: 0,
            controller: 74,
        };
        app.controller.map_midi(control, filter, cutoff).unwrap();
        app.commit();
        let midi = app.rig.midi_in.clone().expect("the idle engine's MIDI input");
        let start = tile_widget(&app, filter, widget).position;
        assert!(start > 0.0 && start < 1.0, "{start}");

        for (value, position) in [(127, 1.0), (0, 0.0)] {
            let change = loom_core::MidiEvent::from_bytes(0, &[0xB0, 74, value]);
            assert!(loom_host_desktop::send_midi(&midi, change));
            app.process(128);
            let _ = app.update(Message::Midi);
            let knob = tile_widget(&app, filter, widget);
            assert_eq!(knob.position, position, "CC 74 at {value}: {knob:?}");
            let param = app.controller.graph().node(filter).unwrap().params[cutoff];
            let spec = &app.controller.graph().node(filter).unwrap().descriptor.params[cutoff];
            assert_eq!(param, if value == 0 { spec.min } else { spec.max });
        }
    }

    /// The transport moves only while it runs. Its position shows through a
    /// clock.md, Starting and stopping: a cable on the Transport's `run`
    /// input decides, and the Run toggle shows what it sets. Space changes
    /// nothing then. When the cable goes, the transport stays as it was, the
    /// toggle takes that state, and Space works again. A second Transport
    /// cannot be added, and the one there cannot be deleted.
    #[test]
    fn the_run_toggle_follows_a_cable_on_run() {
        let mut app = app();
        let (transport, run) = transport_run(&app.controller).expect("a new patch has a Transport");
        let run_param = |app: &App| app.controller.graph().node(transport).unwrap().params[run];
        // A Button's gate is low until pressed: it stops the transport.
        let button = app.controller.add_module("loom.ui.button").unwrap();
        app.controller.connect_named(button, "gate", transport, "run").unwrap();
        app.commit();
        app.process(2048);
        app.refresh_values();
        assert_eq!(transport_running(&app.controller), Some(false));
        assert_eq!(
            tile_widget(&app, transport, 0).position,
            0.0,
            "the toggle shows the input"
        );
        assert_eq!(run_param(&app), 1.0, "Run itself is unchanged");
        let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
        assert_eq!(run_param(&app), 1.0, "Space leaves a driven Run alone");

        let _ = app.update(Message::Canvas(CanvasEvent::Disconnect(loom_canvas::PortRef {
            module: transport.0,
            output: false,
            index: 0,
        })));
        assert_eq!(run_param(&app), 0.0, "the toggle takes the state the input left");
        app.process(1024);
        assert_eq!(transport_running(&app.controller), Some(false), "still stopped");
        let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
        app.process(1024);
        assert_eq!(transport_running(&app.controller), Some(true), "Space works again");

        // Delete leaves the Transport, and a second one is refused.
        let _ = app.update(Message::Canvas(CanvasEvent::Delete(vec![transport.0, button.0])));
        assert_eq!(app.controller.transport_module(), Some(transport));
        assert!(app.controller.graph().node(button).is_none());
        assert!(app.controller.add_module(TRANSPORT_MODULE).is_err());
    }

    /// A patch that loads with a cable on `run`: removing the cable as the
    /// first edit still gives the Run toggle the state the input left
    /// (#413 review).
    #[test]
    fn a_loaded_run_cable_hands_its_state_to_the_toggle() {
        let dir = std::env::temp_dir().join(format!("loom-app-run-cable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.ron");
        std::fs::write(
            &path,
            r#"Patch(version: 1, modules: [
                PatchModule(id: 1, kind: "loom.clock.transport@1"),
                PatchModule(id: 2, kind: "loom.ui.button@1"),
            ], cables: [PatchCable(from: (2, "gate"), to: (1, "run"))])"#,
        )
        .unwrap();
        let mut app = App::new(Args {
            patch: Some(path),
            ..app_args()
        });
        let _ = std::fs::remove_dir_all(&dir);
        let (transport, run) = transport_run(&app.controller).expect("the patch has a Transport");
        app.process(2048);
        assert_eq!(transport_running(&app.controller), Some(false));
        let _ = app.update(Message::Canvas(CanvasEvent::Disconnect(loom_canvas::PortRef {
            module: transport.0,
            output: false,
            index: 0,
        })));
        let param = app.controller.graph().node(transport).unwrap().params[run];
        assert_eq!(param, 0.0, "the toggle takes the state the input left");
    }

    /// Phase Scale that follows the bar phase and modulates a knob at depth
    /// 1: the live dot sits the bar phase above the knob (#222). The
    /// controller reports whether the transport runs, not its position.
    #[test]
    fn the_transport_position_moves_only_while_running() {
        let mut app = app();
        let scale = app.controller.add_module("loom.phase.scale").unwrap();
        let (_, filter, cutoff, widget) = env_and_filter(&mut app);
        app.controller.add_assignment(out(scale), filter, cutoff, 1.0).unwrap();
        app.commit();
        let settings = app.controller.transport();
        let beats_per_frame = settings.tempo / 60.0 / f64::from(app.sample_rate);
        let beats_per_bar = settings.time_signature.beats_per_bar();
        // The bar phase the knob shows, from the live value of the last
        // frame the engine ran.
        let bar_phase = |app: &mut App, frames: usize| {
            app.process(frames);
            let _ = app.update(Message::LiveFrame);
            let knob = tile_widget(app, filter, widget);
            knob.live.expect("a live value") - knob.position
        };
        let expected = |frames: usize| ((frames - 1) as f64 * beats_per_frame / beats_per_bar) as f32;

        // The bar phase is read as live minus knob position, which holds
        // only while the knob plus the phase stays below 1 (no clamp). The
        // test reaches a bar phase of about 0.05.
        let base = tile_widget(&app, filter, widget).position;
        assert!(base < 0.9, "the knob at {base} leaves no room above it for the phase");
        assert!(settings.running, "a new patch starts running");
        // Space flips the Transport's Run toggle.
        let run_on = |app: &App| {
            let (id, index) = transport_run(&app.controller).expect("a new patch has a Transport");
            app.controller.graph().node(id).unwrap().params[index] >= 0.5
        };
        assert!(run_on(&app));
        let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
        assert!(!run_on(&app));
        assert!(bar_phase(&mut app, 4_800).abs() < 1e-6, "stopped at bar 1");

        let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
        assert!(run_on(&app));
        let first = bar_phase(&mut app, 4_800);
        assert!((first - expected(4_800)).abs() < 1e-4, "{first} vs {}", expected(4_800));
        let second = bar_phase(&mut app, 4_800);
        assert!(
            (second - expected(9_600)).abs() < 1e-4,
            "{second} vs {}",
            expected(9_600)
        );

        let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
        let held = bar_phase(&mut app, 4_800);
        assert!(
            (held - expected(9_600)).abs() < 1e-4,
            "stopping holds the position: {held}"
        );
    }

    /// Meter levels follow their probes: turning metering on and off many
    /// times leaves at most one level per live probe, and none once the
    /// cable is gone.
    #[test]
    fn removed_probes_lose_their_levels() {
        let mut app = app();
        let gate = app.controller.add_module("loom.math.constant").unwrap();
        let env = app.controller.add_module("loom.env.adsr").unwrap();
        app.controller.connect_named(gate, "out", env, "gate").unwrap();
        app.commit();
        for _ in 0..20 {
            app.live.set_activity(true);
            app.sync_probes();
            app.process(64);
            let probe = app.live.probes().next().expect("a probe on the cable");
            let level = live::Level { rms: 0.5, peak: 0.6 };
            let _ = app.update(Message::Meters(vec![(probe, level)]));
            assert_eq!(app.meter_levels.len(), 1);

            app.live.set_activity(false);
            app.sync_probes();
            assert!(app.meter_levels.is_empty(), "{:?}", app.meter_levels);
            // A late reading for the removed probe is ignored.
            let _ = app.update(Message::Meters(vec![(probe, level)]));
            assert!(app.meter_levels.is_empty());
        }
    }

    /// The inspector lists a parameter's assignments in order, with the one
    /// that is off shown off. **Add modulation** greys out the outputs that
    /// already drive it, and the source's **Drives** line says "(off)".
    #[test]
    fn the_snapshot_shows_the_inspector_rows() {
        let mut app = app();
        let (env, sine, filter, cutoff, _) = two_sources(&mut app);
        let off = app
            .controller
            .graph()
            .assignments()
            .find(|(_, a)| a.source == out(sine))
            .unwrap()
            .0;
        let _ = app.update(Message::Enabled {
            id: off,
            enabled: false,
        });
        let _ = app.update(Message::SelectModule(filter.0));
        let _ = app.update(Message::AddModulation {
            module: filter.0,
            index: cutoff,
        });

        let snapshot = app.ui_snapshot();
        assert_eq!(snapshot.undo.as_deref(), Some("change modulation"));
        let inspector::Inspector::Module(rows) = snapshot.inspector else {
            panic!("one module is selected: {:?}", snapshot.inspector);
        };
        assert_eq!(rows.name, "SVF");
        let row = &rows.params[cutoff];
        assert_eq!(row.label, "Cutoff");
        let inspector::ParamMode::Edit {
            assignments, adding, ..
        } = &row.mode
        else {
            panic!("not in learn mode");
        };
        let (env_out, sine_out) = (format!("ADSR #{}: Out", env.0), format!("Sine #{}: Out", sine.0));
        let shown: Vec<_> = assignments
            .iter()
            .map(|a| (a.source.as_str(), a.depth_text.as_str(), a.enabled))
            .collect();
        assert_eq!(shown, vec![(&*env_out, "+25%", true), (&*sine_out, "+25%", false)]);
        let pickable: Vec<_> = adding
            .as_ref()
            .expect("Add modulation is open")
            .iter()
            .map(|s| (s.name.as_str(), s.enabled))
            .collect();
        assert!(pickable.contains(&(&*env_out, false)), "{pickable:?}");
        assert!(pickable.contains(&(&*sine_out, false)), "{pickable:?}");
        assert!(pickable.iter().any(|&(_, enabled)| enabled), "{pickable:?}");
        assert!(rows.drives.is_empty(), "the filter drives nothing");

        let _ = app.update(Message::SelectModule(sine.0));
        let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
            panic!("the sine is selected");
        };
        let line = &rows.drives[0].lines[0];
        assert_eq!((line.target, line.enabled), (filter.0, false));
        assert!(line.text.ends_with("(off)"), "{}", line.text);
    }

    /// The snapshot's places are the ones the canvas draws and hits: a
    /// badge, a port, a widget, and a cable's ends.
    #[test]
    fn the_snapshot_places_what_the_canvas_draws() {
        use loom_canvas::{Hit, PortRef};
        let mut app = app();
        let (env, sine, filter, _, widget) = two_sources(&mut app);
        app.controller.connect_named(sine, "out", filter, "in").unwrap();
        app.commit();
        let snapshot = app.ui_snapshot();
        let tile = |id: ModuleId| snapshot.tiles.iter().find(|t| t.id == id.0).expect("a tile");
        let view = app.scene.module(env.0).unwrap();

        let output = &tile(env).outputs[0];
        let badge = output.badge_bounds.expect("the envelope drives one assignment");
        let drawn = app.scene.badge_bounds(env.0, 0).unwrap();
        assert_eq!(badge, drawn.into());
        let port = PortRef {
            module: env.0,
            output: true,
            index: 0,
        };
        assert_eq!(
            app.scene.hit(drawn.center(), snapshot.port_hit_radius),
            Hit::Badge(port)
        );
        let centre = Point::new(output.centre.x, output.centre.y);
        assert_eq!(app.scene.hit(centre, snapshot.port_hit_radius), Hit::Port(port));
        assert_eq!(tile(env).bounds, view.bounds().into());
        assert_eq!(tile(sine).outputs[0].badge_bounds.map(|b| b.width), Some(drawn.width));

        let knob = tile(filter).widgets[widget].bounds;
        let filter_view = app.scene.module(filter.0).unwrap();
        assert_eq!(knob, filter_view.widget_bounds(widget).unwrap().into());

        let cable = snapshot
            .cables
            .iter()
            .find(|c| c.to.module == filter.0)
            .expect("a cable");
        assert_eq!((cable.from.module, cable.from.index), (sine.0, 0));
        assert_eq!(cable.from.point, tile(sine).outputs[0].centre);
        assert_eq!(cable.to.point, tile(filter).inputs[cable.to.index].centre);
    }

    /// An app with one filter, its ID, and the index of its Cutoff.
    fn app_with_filter() -> (App, u32, usize) {
        let mut app = app();
        let id = app.controller.add_module("loom.filter.svf").unwrap();
        app.commit();
        let cutoff = app
            .controller
            .graph()
            .node(id)
            .unwrap()
            .descriptor
            .param_index("cutoff")
            .unwrap();
        (app, id.0, cutoff)
    }

    /// Turns the filter's Cutoff with the inspector slider.
    fn slide(app: &mut App, module: u32, index: usize, values: &[f32]) {
        for &value in values {
            let _ = app.update(Message::ParamValue { module, index, value });
        }
    }

    /// An inspector slider gesture is one step, and it closes at release,
    /// at the next edit, and at undo: a slider sends no start, and arrow
    /// keys on it send no release (undo.md, Gestures). Issue #170.
    #[test]
    fn an_inspector_slider_gesture_closes_at_release_at_the_next_edit_and_at_undo() {
        let (mut app, filter, cutoff) = app_with_filter();
        let steps = app.edits.history.len();

        // Release.
        slide(&mut app, filter, cutoff, &[500.0, 600.0, 700.0]);
        assert!(app.edits.slider_gesture_open());
        let _ = app.update(Message::SliderReleased);
        assert!(!app.edits.gesture_open());
        assert_eq!(app.edits.history.len(), steps + 1, "one drag, one step");

        // The next edit, here a canvas gesture start.
        slide(&mut app, filter, cutoff, &[800.0, 900.0]);
        let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
        assert!(!app.edits.slider_gesture_open());
        assert_eq!(app.edits.history.len(), steps + 2, "the start closed the slider's step");
        let _ = app.update(Message::Canvas(CanvasEvent::GestureEnd));
        assert_eq!(
            app.edits.history.len(),
            steps + 2,
            "a gesture with no change is no step"
        );

        // Undo, which takes back the slider's step it closes.
        slide(&mut app, filter, cutoff, &[1000.0]);
        let _ = app.update(Message::Undo);
        assert!(!app.edits.gesture_open());
        let value = app.controller.graph().node(ModuleId(filter)).unwrap().params[cutoff];
        assert_eq!(value, 900.0, "undo took back the slider's last step");
        assert_eq!(app.edits.history.len(), steps + 3);
        assert_eq!(
            app.edits.history.redo_step().map(undo::Step::name),
            Some("change SVF Cutoff")
        );
    }

    /// A mode change during a momentary press does not leave the hold
    /// stuck high: the release lets go of it in any mode (review of #402).
    #[test]
    fn a_release_clears_the_hold_after_a_mode_change() {
        let mut app = app();
        let id = app.controller.add_module("loom.ui.button").unwrap();
        app.commit();
        let node = app.controller.graph().node(id).unwrap();
        let mode = node.descriptor.param_index("mode").unwrap();
        let hold = node.descriptor.param_index("hold").unwrap();
        let (module, widget) = (id.0, 0);
        let held = |app: &App| app.controller.graph().node(id).unwrap().params[hold];

        app.controller.set_param(id, mode, 1.0).unwrap();
        let _ = app.update(Message::Canvas(CanvasEvent::ButtonPressed { module, widget }));
        assert_eq!(held(&app), 1.0, "the hold is down");
        app.controller.set_param(id, mode, 0.0).unwrap();
        let _ = app.update(Message::Canvas(CanvasEvent::ButtonReleased { module, widget }));
        assert_eq!(held(&app), 0.0, "the release let go of the hold");
        app.controller.set_param(id, mode, 1.0).unwrap();
        assert_eq!(held(&app), 0.0, "Momentary mode comes back released");
    }

    /// A momentary button hold makes no step, and a toggle press does
    /// (undo.md, What is not undoable). A button sends a press and a
    /// release, with no gesture around them.
    #[test]
    fn a_momentary_hold_makes_no_step_and_a_toggle_press_does() {
        let mut app = app();
        let id = app.controller.add_module("loom.ui.button").unwrap();
        app.commit();
        let node = app.controller.graph().node(id).unwrap();
        let mode = node.descriptor.param_index("mode").unwrap();
        let on = node
            .descriptor
            .param_index(node.descriptor.panel.widgets[0].bind)
            .unwrap();
        // Momentary mode writes the transient hold, not the latch (#161).
        let hold = node.descriptor.param_index("hold").unwrap();
        let (module, widget) = (id.0, 0);
        let press = |app: &mut App, param: usize, release: bool| {
            let _ = app.update(Message::Canvas(CanvasEvent::ButtonPressed { module, widget }));
            let held = app.controller.graph().node(id).unwrap().params[param];
            if release {
                let _ = app.update(Message::Canvas(CanvasEvent::ButtonReleased { module, widget }));
            }
            held
        };

        app.controller.set_param(id, mode, 1.0).unwrap();
        app.commit();
        let steps = app.edits.history.len();
        assert_eq!(press(&mut app, hold, false), 1.0, "the hold is down");
        assert_eq!(app.edits.history.len(), steps, "a momentary press is no step");
        let _ = app.update(Message::Canvas(CanvasEvent::ButtonReleased { module, widget }));
        assert_eq!(app.controller.graph().node(id).unwrap().params[hold], 0.0);
        assert_eq!(app.edits.history.len(), steps, "nor is its release");

        app.controller.set_param(id, mode, 0.0).unwrap();
        app.commit();
        let steps = app.edits.history.len();
        assert_eq!(press(&mut app, on, true), 1.0, "the toggle is on");
        assert_eq!(app.edits.history.len(), steps + 1, "a toggle press is a step");
        assert_eq!(app.controller.graph().node(id).unwrap().params[on], 1.0, "and stays on");
    }
}
