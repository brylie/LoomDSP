//! `loom-app`: the LoomDSP desktop app.
//!
//! Phase 1 prototype: the patch canvas (ADR-0010), an inspector for the
//! selected module, live audio and MIDI, and a stats bar for measuring the
//! canvas against the performance gates in NFR-PERF-04.

mod alerts;
mod audio_watch;
#[cfg(test)]
mod autosave_tests;
mod bench;
mod browser;
mod clipboard;
mod control;
mod devices;
mod document;
mod edits;
#[cfg(test)]
mod example_tests;
mod files;
mod gestures;
mod help;
mod input_watch;
mod inspector;
mod layout;
mod learn;
mod lights;
mod live;
mod matrix;
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
mod stream;
mod style;
mod targets;
#[cfg(test)]
mod ui_test;
mod undo;

use crate::style::button;
use clap::Parser;
use iced::widget::{column, container, mouse_area, row, rule, space, stack, text, toggler, tooltip};
use iced::{Color, Element, Length, Point, Size, Subscription, Task, Theme};
use live::Live;
use loom_canvas::{CanvasEvent, DragLine, PanelValues, PatchCanvas, Scene};
use loom_core::{
    AssignmentId, Controller, EngineConfig, EngineParts, InputRef, LoomError, MidiEvent, ModuleDescriptor, ModuleId,
    OutputRef, OutputSafety, ProbeId, TRANSPORT_MODULE, create_engine,
};
use loom_host_desktop::{AudioOutput, CallbackStats, SharedMidi};
use loom_patch::Patch;
use matrix::{Event as MatrixEvent, Tab as RightTab};
use modulation::PolarityChoice;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The modules a patch can use: the built-ins, and with `test_modules`
/// also the `test.` modules from `loom_modules::test_modules`, which have widgets
/// no built-in module has yet. The app's own tests always get them.
fn registry(test_modules: bool) -> Arc<loom_core::Registry> {
    if !(test_modules || cfg!(test)) {
        return loom_modules::registry();
    }
    let mut registry = loom_core::Registry::new();
    loom_modules::register_all(&mut registry).expect("built-in modules are valid and unique");
    loom_modules::test_modules::register(&mut registry).expect("test modules are valid and unique");
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
    /// With --dump-ui or --screenshot-window, open help for the selected tile.
    #[arg(long, requires_all = ["inspect", "select"])]
    show_help: bool,
    /// With --dump-ui or --screenshot-window: first run the engine for N
    /// frames at 48 kHz on silent input, so live values, lights, and meters
    /// show where the engine left them.
    #[arg(long, value_name = "N", requires = "inspect")]
    frames: Option<usize>,
    /// With --frames: hold a note (A3, as the Hold note button plays it)
    /// while the engine runs, so a voice patch sounds and its scopes show a
    /// wave.
    #[arg(long, requires = "frames")]
    hold_note: bool,
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
    /// For developers: show the frame meter's Measure and Move camera
    /// switches in the status bar. They also show with --measure,
    /// --measure-idle, or --bench (docs/design/visual-system.md).
    #[arg(long)]
    dev: bool,
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
    /// The eligible inspector parameter currently under an output drag.
    DropRow(Option<(u32, usize)>),
    Param {
        module: u32,
        index: usize,
        position: f32,
    },
    /// The toolbar asks for the module browser at the centre of the canvas.
    OpenBrowser,
    /// Open help for the browser highlight or selected tile.
    ShowHelp,
    /// Open the keyboard shortcuts page in the help overlay.
    ShowShortcuts,
    /// Open help for a specific tile.
    ShowModuleHelp(u32),
    /// Close module help.
    CloseHelp,
    /// Open the current help example as a patch.
    HelpExample,
    /// A link in bundled help was selected.
    HelpLink(String),
    /// Close the tile menu.
    CloseTileMenu,
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
    /// Audio input counters or stream health changed.
    InputAudio(input_watch::Event),
    /// A file command, or the window asking to close (`docs/design/files.md`).
    File(FileAction),
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
    /// A slow tick while a recording runs, to notice a write error.
    Tick,
    /// The audio watcher saw a change to show, or stalled callbacks.
    Audio(audio_watch::Event),
    /// A frame while live modulated values are on screen.
    LiveFrame,
    /// New brightness for lights on screen.
    Lights(Vec<(lights::Light, f32)>),
    /// New levels for meter probes.
    Meters(Vec<(ProbeId, live::Level)>),
    /// What the telemetry thread found that changed on screen: meter levels
    /// and scope traces.
    Telemetry(monitor::Update),
    /// MIDI learn: turn it on or off, or arm a parameter.
    Learn(learn::Message),
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
    /// Edit a placeholder label without changing the patch yet.
    UnknownLabelDraft {
        module: u32,
        value: String,
    },
    /// Apply the placeholder label as one undo step.
    UnknownLabelApply {
        module: u32,
    },
    /// A change in the session-only modulation matrix.
    Matrix(MatrixEvent),
    /// A key handled by the focused matrix even if a widget used it.
    MatrixKey(iced::keyboard::Event),
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

impl Message {
    /// True for the frequent messages that never change the patch: live
    /// readings, timers, and frames. Every other message may, so autosave
    /// looks again after it. Telling them apart lets the autosave timer
    /// stop in an idle app (#90).
    fn keeps_patch(&self) -> bool {
        matches!(
            self,
            Self::Audio(audio_watch::Event::Stats { .. })
                | Self::InputAudio(_)
                | Self::Tick
                | Self::Meters(_)
                | Self::Telemetry(_)
                | Self::Lights(_)
                | Self::Autosave
                | Self::Frame(_)
                | Self::LiveFrame
                | Self::MidiPoll
                | Self::BenchDone
                | Self::UnknownLabelDraft { .. }
        )
    }
}

/// What the player asks of the patch file. Each can ask about unsaved
/// changes first (`docs/design/files.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileAction {
    /// Start an empty patch.
    New,
    /// Pick a patch file and open it.
    Open,
    /// Save to the current file, or ask for one.
    Save,
    /// Ask for a file and save to it.
    SaveAs,
    /// The window asked to close: quit, unless the player keeps the patch.
    Close,
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
    /// A full command queue left fresh modules staged for a later commit.
    retry_commit: bool,
    positions: BTreeMap<u32, Point>,
    /// Original placeholder positions and their initial displayed positions.
    unknown_positions: BTreeMap<u32, (Option<(f32, f32)>, Point)>,
    /// Saved tile dimensions in grid units, keyed by module ID.
    sizes: BTreeMap<u32, (u8, u8)>,
    scene: Scene,
    /// The mode values the scene was built with (ADR-0030). A change
    /// rebuilds the scene, so hidden controls follow the mode.
    mode_key: Vec<(u32, usize, i64)>,
    /// Modulation gestures in progress: the Add modulation list, an output
    /// dragged onto the inspector, an arc drag, a placeholder label.
    gestures: gestures::Gestures,
    /// Live values of panel controls and lights, apart from the scene so a
    /// knob turn does not rebuild tiles or cables.
    values: PanelValues,
    /// Last brightness of each light, by module and widget.
    lights: BTreeMap<lights::Light, f32>,
    /// True when the last commit had a cable on the Transport's `run`
    /// input. When the cable goes, the Run toggle takes the state the input
    /// left (clock.md, Starting and stopping).
    run_driven: bool,
    /// Latest level of each meter probe.
    meter_levels: std::collections::HashMap<ProbeId, live::Level>,
    /// The traces of the scopes on screen (`docs/design/panels.md`, Drawing
    /// the scope trace).
    traces: loom_canvas::ScopeTraces,
    /// Outputs carrying a signal (`docs/design/panels.md`, Signal activity
    /// on ports).
    carrying: BTreeSet<OutputRef>,
    /// True while the canvas draws full scope traces (close zoom), false
    /// when it draws only their level.
    full_traces: bool,
    /// Modules whose lights or meters are on screen, as the canvas reports.
    visible_displays: Vec<u32>,
    selection: BTreeSet<u32>,
    /// The patch's file, title, saved state, and recovery copy.
    document: document::Document,
    /// Counts the engines built. Background watchers restart when it changes.
    generation: u64,
    /// The `test.` modules are loaded (`--test-modules`).
    test_modules: bool,
    /// Set when the app should exit after this update.
    quit: bool,
    theme: Theme,
    /// The output stream or idle engine, its counters, and how this run
    /// plays.
    stream: stream::Stream,
    /// Meter and scope data from the engine, drained on the telemetry
    /// thread so the queues never fill.
    monitor: monitor::Monitor,
    live: Live,
    /// What belongs to the machine: preferences, devices, MIDI inputs.
    rig: settings::Rig,
    /// The status line, the stats bar's warnings, and problems reported once.
    alerts: alerts::Alerts,
    /// Note input shared by every MIDI port and the "Hold note" button.
    midi: Option<SharedMidi>,
    holding: bool,
    /// Every registered module, for the browser.
    modules: Vec<&'static ModuleDescriptor>,
    /// The module browser, while it is open.
    browser: Option<browser::Browser>,
    /// Open module help.
    help: Option<help::Help>,
    /// Right-clicked tile and its menu position.
    tile_menu: Option<(u32, Point)>,
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
    measuring: bool,
    animating: bool,
    /// The status bar shows the developer tools: `--dev`, or a measuring
    /// flag (visual-system.md, Status bar).
    dev_tools: bool,
    /// MIDI learn mode, the armed parameter, and what the canvas shows.
    learn: learn::Learn,
    /// Undo and redo (`docs/design/undo.md`): the history, and the edit
    /// or gesture being recorded.
    edits: edits::Edits,
    /// An arrow key was released and its undo group may still grow.
    arrow_group_pending: bool,
    /// A toolbar or inspector action closed that group before its canvas timer.
    arrow_group_closed: bool,
    /// Session-only matrix tab, filters, selection, and pending edit.
    matrix: matrix::State,
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
    /// Original and displayed placeholder positions for lossless saving.
    unknown_positions: BTreeMap<u32, (Option<(f32, f32)>, Point)>,
    /// Tile dimensions carried with the patch between sessions.
    sizes: BTreeMap<u32, (u8, u8)>,
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
            // A copy that reads but does not load is a failed restore too:
            // the orphan is kept, not adopted.
            match recovered.take() {
                Some((_, orphan, _)) => restore_error(&e, orphan, &mut notes),
                None => status = e,
            }
            Self::session(config, args.test_modules, Source::Empty).expect("an empty patch always loads")
        });
        let mut app = Self::assemble(output, sample_rate, session, 1, &args, rig, true);
        // Tests have no bundled folder unless they set one: the checkout's
        // examples must open as plain files there.
        #[cfg(test)]
        app.document.set_examples(files::fake::examples());
        #[cfg(not(test))]
        app.document.set_examples(files::bundled_examples());
        app.document.copy_if_example();
        // The compile report gives way to anything that needs reading.
        let mut keep = !status.is_empty();
        if keep {
            app.alerts.say(status);
        }
        app.document.start_autosave(session_folder);
        if let Some((state, orphan, _)) = recovered {
            app.restored(state, orphan, &args);
            keep = true;
        }
        if !notes.is_empty() {
            if keep {
                notes.insert(0, app.alerts.take_status());
            }
            app.alerts.say(notes.join(" "));
        }
        app
    }

    /// Sets up a patch restored from a recovery copy (autosave.md, After a
    /// restore): marked unsaved, with its file only if that is unchanged.
    fn restored(&mut self, state: recovery::FileState, orphan: recovery::Orphan, args: &Args) {
        self.document.restore(state);
        // Nothing on disk holds this patch.
        self.edits.history.clear();
        let when = recovery::local_time(orphan.written());
        self.alerts.say(match args.source() {
            Source::File(path) => format!(
                "Restored unsaved changes from {when}. {} was not opened.",
                files::display_name(Some(path))
            ),
            _ => format!("Restored unsaved changes from {when}."),
        });
        for note in self.document.load_notes() {
            if note.contains("not in this version:") {
                self.alerts.append(" ");
                self.alerts.append(note);
            }
        }
        self.document.adopt(orphan);
        // This session's own copy, at once: the orphan goes when it is on disk.
        self.autosave(Instant::now());
    }

    /// The patch as it would be saved now.
    fn current_patch(&self) -> Patch {
        let mut patch = Patch::from_graph(self.document.name(), self.controller.graph());
        for m in &mut patch.modules {
            m.position = self.positions.get(&m.id).map(|p| (p.x.round(), p.y.round()));
            if let Some((original, initial)) = self.unknown_positions.get(&m.id)
                && self.positions.get(&m.id) == Some(initial)
            {
                m.position = *original;
            }
            m.size = self.sizes.get(&m.id).copied();
        }
        patch
    }

    /// Writes or deletes the recovery copy when one is due
    /// (`docs/design/autosave.md`). Runs on a timer and after save, open,
    /// and new.
    fn autosave(&mut self, now: Instant) {
        if !self.document.autosave_on() {
            return;
        }
        let current = self.current_patch();
        if let Some(line) = self.document.autosave(now, current) {
            self.alerts.say(line);
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
        let mut sizes = BTreeMap::new();
        let mut original_unknown_positions = BTreeMap::new();
        let mut notes = Vec::new();
        let mut load = |patch: &Patch, controller: &mut Controller| -> Result<String, String> {
            notes = patch.load_into(controller).map_err(|e| e.to_string())?;
            for m in &patch.modules {
                if controller
                    .graph()
                    .node(ModuleId(m.id))
                    .is_some_and(|node| node.unknown.is_some())
                {
                    original_unknown_positions.insert(m.id, m.position);
                }
                if let Some((x, y)) = m.position {
                    positions.insert(m.id, Point::new(x, y));
                }
                if let Some(size) = m.size {
                    sizes.insert(m.id, size);
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
        let unknown_positions = original_unknown_positions
            .into_iter()
            .filter_map(|(id, original)| positions.get(&id).copied().map(|initial| (id, (original, initial))))
            .collect();
        for note in &notes {
            eprintln!("loom-app: {note}");
        }
        Ok(Session {
            controller,
            engine,
            midi,
            telemetry,
            positions,
            unknown_positions,
            sizes,
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
            unknown_positions,
            sizes,
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
        let midi = rig.midi_in.clone().expect("Rig::attach always sets midi_in");

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

        let scene = scene::build(&controller, &positions, &sizes);
        let mode_key = scene::mode_key(&controller);
        let mut values = PanelValues::new();
        scene::update_panel_values(&mut values, &controller, &BTreeMap::new());
        // The loaded patch may already have a cable on run, before any
        // commit of the app's own (#413 review).
        let run_driven = run_is_driven(&controller);
        let mut app = Self {
            controller,
            retry_commit: false,
            positions,
            unknown_positions,
            sizes,
            mode_key,
            scene,
            gestures: gestures::Gestures::default(),
            values,
            lights: BTreeMap::new(),
            run_driven,
            meter_levels: std::collections::HashMap::new(),
            traces: loom_canvas::ScopeTraces::new(),
            carrying: BTreeSet::new(),
            full_traces: true,
            visible_displays: Vec::new(),
            selection: BTreeSet::new(),
            document: document::Document::new(name, file, stamp, load_notes),
            generation,
            stream: stream::Stream::new(
                audio,
                idle_engine,
                audio_name,
                sample_rate,
                stream::Playback::from_flags(args.no_audio, args.fake_audio),
                args.output_safety.unwrap_or(rig.store.saved().output_safety),
            ),
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
            monitor: monitor::Monitor::new(telemetry, sample_rate, generation),
            live: {
                let mut live = Live::showing_activity();
                live.set_activity(rig.store.saved().show_activity);
                live
            },
            rig,
            midi,
            holding: false,
            alerts: alerts::Alerts::new(status),
            // The app's tests load the test modules but never list them.
            modules: if args.test_modules {
                registry(true).iter().collect()
            } else {
                loom_modules::registry().iter().collect()
            },
            browser: None,
            help: None,
            tile_menu: None,
            browser_key: 0,
            probe_menu: None,
            frame_key: 0,
            meter: bench::FrameMeter::default(),
            reporter: args.bench.reporter(),
            control: args.control.listeners(),
            control_pending: Vec::new(),
            pointer: pointer::Pointer::new(!args.control.listeners().is_empty()),
            control_seen: None,
            measuring: args.bench.redraws(),
            animating: args.bench.bench,
            dev_tools: args.dev || args.bench.reporter().is_some(),
            learn: learn::Learn::default(),
            edits: edits::Edits::default(),
            arrow_group_pending: false,
            arrow_group_closed: false,
            matrix: matrix::State::default(),
            still: None,
            step_name: None,
        };
        // A stream that failed to start took the engine with it.
        app.keep_engine_idle();
        app.sync_audio_input();
        app.send_panic_trigger();
        if let Some(error) = app.connect_midi(first_midi, connect_midi) {
            app.alerts.say(error);
        }
        // A patch just loaded is the one on disk.
        app.edits.history.mark_saved();
        app.document.mark_loaded(app.current_patch());
        if app.alerts.status().is_empty() {
            app.report();
        }
        app
    }

    /// The name of the patch as the title shows it, without the mark.
    fn patch_name(&self) -> String {
        self.document.title_name()
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
                self.alerts.say(format!("saved {}", path.display()));
                self.document.saved(path, patch);
                self.edits.history.mark_saved();
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
                self.alerts.say(message);
                false
            }
        }
    }

    /// Save: to the current file, or Save As when there is none. Returns
    /// false if nothing was saved.
    fn save(&mut self) -> bool {
        self.finish_edits();
        match self.document.file().map(Path::to_path_buf) {
            Some(path) => self.write_to(path),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        self.finish_edits();
        match files::pick_save(self.document.save_start()) {
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
        match files::confirm_discard(&self.patch_name()) {
            files::Choice::Save => self.save(),
            files::Choice::DontSave => true,
            files::Choice::Cancel => false,
            files::Choice::Unavailable => {
                self.alerts.say(
                    "Unsaved changes. No dialog program (zenity or kdialog) is installed: save with Ctrl+S first."
                        .to_string(),
                );
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
        let playback = self.stream.playback();
        let (output, _) = settings::open_output(&self.rig, playback.is_off(), playback.is_fake());
        // Playing now and no device to play on afterwards: keep what plays.
        if let (Err(e), true) = (&output, self.stream.is_playing()) {
            let message = format!(
                "Could not open the patch: the audio device is not available ({e}). The current patch is unchanged."
            );
            self.show_error(&message);
            self.alerts.say(message);
            return false;
        }
        let sample_rate = output
            .as_ref()
            .map(|o| o.sample_rate() as f32)
            .unwrap_or(self.stream.sample_rate());
        let config = EngineConfig {
            output_safety: self.stream.output_safety(),
            ..EngineConfig::with_sample_rate(sample_rate)
        };
        let session = match Self::session(config, self.test_modules, source) {
            Ok(session) => session,
            Err(e) => {
                let message = format!("Could not open the patch: {e}. The current patch is unchanged.");
                self.show_error(&message);
                self.alerts.say(message);
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
            no_audio: playback.is_off(),
            fake_audio: playback.is_fake(),
            record: None,
            bench: bench::Options {
                measure: self.measuring,
                ..Default::default()
            },
            control: control::Options::default(),
            output_safety: Some(self.stream.output_safety()),
            devices: devices::Options::default(),
            view: screenshot::Options::default(),
            dump_ui: false,
            select: Vec::new(),
            show_help: false,
            frames: None,
            hold_note: false,
            recovery_dir: None,
            no_recovery: true,
            test_modules: self.test_modules,
            dev: self.dev_tools,
        };
        let rig = std::mem::replace(&mut self.rig, settings::Rig::empty());
        let mut fresh = Self::assemble(output, sample_rate, session, self.generation + 1, &args, rig, true);
        fresh.animating = self.animating;
        fresh.matrix.tab = self.matrix.tab;
        fresh.reporter = self.reporter.take();
        fresh.control = std::mem::take(&mut self.control);
        // The script's pointer stays where it was, over the new patch.
        fresh.pointer = self.pointer.clone();
        // The session goes on; only its patch changed.
        fresh.document.carry_on_from(&mut self.document);
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

    /// Runs a file command from the toolbar, a shortcut, or the window.
    fn file_action(&mut self, action: FileAction) {
        match action {
            FileAction::New => self.new_patch(),
            FileAction::Open => self.open(),
            FileAction::Save => {
                self.save();
            }
            FileAction::SaveAs => {
                self.save_as();
            }
            FileAction::Close => {
                if self.can_discard() {
                    self.quit = true;
                }
            }
        }
    }

    fn new_patch(&mut self) {
        if self.can_discard() && self.replace(Source::Empty) {
            self.alerts.say("new patch".to_string());
        }
    }

    fn open(&mut self) {
        if !self.can_discard() {
            return;
        }
        let Some(path) = files::pick_open(self.document.file()) else {
            return;
        };
        self.open_path(&path);
    }

    /// Opens the patch at `path` in place of this one, and says so in the
    /// status bar, with any notes about what loaded differently.
    fn open_path(&mut self, path: &std::path::Path) {
        if self.replace(Source::File(path)) {
            self.alerts.say(opened_status(path, self.document.load_notes()));
        }
    }

    fn title(&self) -> String {
        files::titled(&self.patch_name(), self.unsaved())
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        let main = if self.measuring {
            // Redraw every frame, so the counter measures real render cost.
            iced::window::frames().map(Message::Frame)
        } else if self.rig.recorder.is_some() {
            // Otherwise the window redraws only when something changes
            // (#90). A recording, a QA tool, ticks to notice a write error.
            iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        };
        // The audio numbers and stalled callbacks are read on a thread,
        // which wakes the UI only when the stats bar changes (#90).
        let audio_watch = match self.stream.stats() {
            Some(stats) => Subscription::run_with(
                audio_watch::Watch {
                    stats: stats.clone(),
                    engine: self.controller.stats().clone(),
                },
                audio_watch::stream,
            )
            .map(Message::Audio),
            None => Subscription::none(),
        };
        // An open input always has a drift tracker: both are set and cleared
        // together.
        let input_watch = self
            .rig
            .input
            .as_ref()
            .zip(self.rig.input_tracker.as_ref())
            .map(|(input, tracker)| {
                let stats = self.controller.stats().clone();
                let tracker = tracker.clone();
                Subscription::run_with(
                    input_watch::Watch {
                        status: input.status(),
                        stats,
                        tracker,
                        device: input.name().to_string(),
                        system_default: self.rig.flags.input.is_none()
                            && matches!(
                                self.rig.store.saved().audio.input,
                                preferences::InputChoice::SystemDefault
                            ),
                        list: self.rig.host.audio_input_lister(),
                        permission_denied: self.rig.host.audio_input_permission_checker(),
                        active: self
                            .controller
                            .graph()
                            .nodes()
                            .filter_map(|(id, node)| {
                                (node.descriptor.id == "loom.io.audio-in")
                                    .then(|| self.controller.display_source(id, 0))
                                    .flatten()
                            })
                            .collect(),
                        displays: self.controller.display_reader(),
                    },
                    input_watch::stream,
                )
                .map(Message::InputAudio)
            });
        // Live modulated values move every block, so a patch with an
        // assignment that is on follows them every frame.
        // Without audio the engine does not run, so nothing moves.
        let live = if !self.measuring
            && self.stream.is_playing()
            && self.controller.graph().assignments().any(|(_, a)| a.enabled)
        {
            iced::window::frames().map(|_| Message::LiveFrame)
        } else {
            Subscription::none()
        };
        let telemetry = Subscription::run_with(self.monitor.clone(), monitor::stream).map(Message::Telemetry);
        let close = iced::window::close_requests().map(|_| Message::File(FileAction::Close));
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
        // The autosave timer runs only while a copy may be due: once the
        // copy matches the patch, an idle app ticks no more (#90).
        let autosave = if self.document.autosave_due() {
            iced::time::every(recovery::TICK).map(|_| Message::Autosave)
        } else {
            Subscription::none()
        };
        // MIDI inputs plugged in and out are found by polling four times a
        // second: the systems offer no portable callback (device-changes.md).
        // With a real host the list is read on a thread, which wakes the
        // UI only when it changes. A test host is polled from the UI.
        let midi_poll = if self.rig.midi_in.is_some() && !self.rig.flags.no_midi {
            match self.rig.host.midi_lister() {
                Some(list) => Subscription::run_with(
                    devices::MidiWatch {
                        list,
                        seen: self.rig.seen_midi.clone(),
                        generation: self.generation,
                    },
                    devices::midi_stream,
                )
                .map(|()| Message::MidiPoll),
                None => iced::time::every(devices::MIDI_POLL).map(|_| Message::MidiPoll),
            }
        } else {
            Subscription::none()
        };
        // While audio plays, the audio watcher reports stalled callbacks.
        // Once a device is lost, reconnection is tried by polling four
        // times a second (device-changes.md, Audio device loss).
        let waiting_for_input = self.rig.input.is_none()
            && self.stream.is_playing()
            && !self.rig.flags.no_input
            && (self.rig.flags.input.is_some()
                || !matches!(self.rig.store.saved().audio.input, preferences::InputChoice::Off))
            && self
                .controller
                .graph()
                .nodes()
                .any(|(_, node)| node.descriptor.id == "loom.io.audio-in");
        let audio_poll = if !self.stream.without_device() && (!self.stream.is_playing() || waiting_for_input) {
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
                audio_watch,
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
            .chain(control)
            .chain(input_watch),
        )
    }

    /// Wakes the UI when control changes arrive, while there is something to
    /// update: mapped parameters to follow, or learn mode waiting for MIDI.
    /// In learn mode, it also listens for Escape.
    fn midi_watch(&self) -> Subscription<Message> {
        let mapped = self.controller.graph().midi_mappings().next().is_some();
        let watch = (mapped || self.learn.is_on() || self.rig.panic_learn.is_some()).then(|| {
            let watch = learn::Watch {
                reader: self.controller.midi_reader(),
                generation: self.generation,
            };
            Subscription::run_with(watch, learn::stream).map(|_| Message::Midi)
        });
        let keys = Some(iced::keyboard::listen().map(Message::Key));
        // The matrix owns arrows and Escape even when a child widget uses
        // the key. Its handler ignores them outside table focus.
        let matrix_keys = (self.matrix.tab == RightTab::Matrix && self.matrix.focused)
            .then(|| iced::event::listen_with(matrix::key_listener));
        // The search field takes Escape for itself, so the browser listens
        // to keys whether or not a widget used them.
        let browser = (self.browser.is_some() && self.help.is_none()).then(|| iced::event::listen_with(browser_key));
        let help_keys = Some(iced::event::listen_with(help_key));
        // The panic key works whatever has focus (panic.md).
        let panic = Some(iced::event::listen_with(panic::key));
        Subscription::batch(
            watch
                .into_iter()
                .chain(keys)
                .chain(matrix_keys)
                .chain(browser)
                .chain(help_keys)
                .chain(panic),
        )
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
        if let Some(engine) = self.stream.idle_mut() {
            engine.apply_pending();
        }
        // What the engine handed back is freed here, after any message,
        // rather than on a timer that would wake an idle app (#90).
        self.controller.collect_garbage();
    }

    /// Runs the engine for `frames` frames on silent buffers, as an audio
    /// device would. For tests and `--frames`: the window never runs the
    /// idle engine. Runs in chunks, so a long run needs no long buffers;
    /// the engine gives the same output at any block size.
    fn process(&mut self, frames: usize) {
        const CHUNK: usize = 4096;
        let engine = self.stream.idle_mut().expect("an audio device plays the engine");
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
        self.scene = scene::build(&self.controller, &self.positions, &self.sizes);
        self.mode_key = scene::mode_key(&self.controller);
        self.refresh_values();
        self.refresh_learn();
        self.matrix.reconcile(self.controller.graph());
    }

    /// Rebuilds what learn mode shows, after mappings or the armed
    /// parameter changed. A removed module disarms its parameter.
    fn refresh_learn(&mut self) {
        self.learn.refresh(&self.controller);
    }

    /// Turns learn mode on or off, or arms a parameter.
    fn learn_message(&mut self, message: learn::Message) {
        match message {
            learn::Message::Toggle if self.learn.is_on() => self.stop_learning(),
            learn::Message::Toggle => {
                self.learn.start();
                self.alerts.say("MIDI learn: click a control to map it".to_string());
                self.refresh_learn();
            }
            learn::Message::Arm(param) => {
                if self.learn.is_on() {
                    self.arm(param);
                }
            }
        }
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
            self.alerts.say(format!(
                "MIDI learn: {} is a live control and cannot be mapped",
                learn::param_name(&self.controller, param)
            ));
            return;
        }
        if self.learn.is_armed(param) {
            self.learn.disarm();
            self.alerts.say("MIDI learn: click a control to map it".to_string());
        } else {
            let serial = self.controller.last_control_change().map(|seen| seen.serial);
            self.learn.arm(param, serial);
            self.alerts.say(format!(
                "Move a MIDI control to map it to {}",
                learn::param_name(&self.controller, param)
            ));
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
        let Some(armed) = self.learn.armed() else { return };
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
                self.learn.disarm();
                self.commit();
                if let Some(mapping) = mapping {
                    self.alerts
                        .say(learn::mapped_status(&self.controller, mapping, &removed));
                }
            }
            Err(e) => {
                self.learn.disarm();
                self.alerts.say(format!("error: {e}"));
                self.refresh_learn();
            }
        }
        if !self.edits.gesture_open() {
            self.record();
        }
    }

    fn stop_learning(&mut self) {
        self.learn.stop();
        self.alerts.say("MIDI learn off".to_string());
        self.refresh_learn();
    }

    /// Rebuilds the control, light, and meter values. Only changed values
    /// change what the canvas uploads.
    fn refresh_values(&mut self) {
        // A mode change shows or hides controls, which changes the panel
        // (ADR-0030). Rare: a mode click, never a knob turn.
        let mode_key = scene::mode_key(&self.controller);
        if mode_key != self.mode_key {
            self.scene = scene::build(&self.controller, &self.positions, &self.sizes);
            self.mode_key = mode_key;
        }
        let mut shown: scene::ShownValues = self
            .lights
            .iter()
            .map(|(&light, &level)| (light, (level, 0.0, None)))
            .collect();
        let modules: Vec<u32> = self.controller.graph().nodes().map(|(id, _)| id.0).collect();
        for (widget, output) in scene::meter_outputs(&self.controller, &modules) {
            let level = self
                .live
                .probe(output)
                .and_then(|probe| self.meter_levels.get(&probe))
                .copied()
                .unwrap_or_default();
            shown.insert(widget, (level.rms, level.peak, level.right));
        }
        let dragging = self.gestures.depth_target();
        scene::update_panel_values_dragging(&mut self.values, &self.controller, &shown, dragging);
        // A scope's window or trigger may have changed.
        self.publish_watch();
    }

    /// Takes the latest dropout count for the stats bar warning. Runs on
    /// every tick and, in Measure mode, on every frame.
    fn update_dropouts(&mut self, now: Instant) {
        if let Some(stats) = self.stream.stats() {
            let s = stats.snapshot();
            self.alerts.note_dropouts(s.overruns + s.stream_errors, now);
        }
        // A recording that stopped early (a full disk) is reported once.
        if let Some(error) = self.rig.recorder.as_ref().and_then(loom_host_desktop::Recorder::error)
            && self.alerts.recording_stopped()
        {
            self.alerts.say(format!("The recording stopped: {error}"));
            eprintln!("loom-app: {}", self.alerts.status());
        }
    }

    /// The outputs of the meters on screen: each needs a probe.
    fn metered_outputs(&self) -> BTreeSet<OutputRef> {
        scene::meter_outputs(&self.controller, &self.visible_displays)
            .into_iter()
            .map(|(_, output)| output)
            .collect()
    }

    /// Brings the meter and scope probes in line with the graph and the
    /// displays on screen, and tells the telemetry thread what to read.
    /// Call it before a commit.
    fn sync_live(&mut self) {
        let wanted = self.metered_outputs();
        self.live.sync(&mut self.controller, &wanted);
        let scopes: BTreeSet<OutputRef> = scene::scope_outputs(&self.controller, &self.visible_displays)
            .into_iter()
            .map(|scope| scope.output)
            .collect();
        self.live.sync_scopes(&mut self.controller, &scopes);
        self.publish_watch();
    }

    /// Tells the telemetry thread which meters and scopes are drawn, and
    /// how each scope triggers. Runs after probes or scope parameters
    /// change.
    fn publish_watch(&mut self) {
        let meters = self
            .metered_outputs()
            .into_iter()
            .filter_map(|output| self.live.probe(output))
            .collect();
        let rate = self.monitor.sample_rate();
        let scopes = scene::scope_outputs(&self.controller, &self.visible_displays)
            .into_iter()
            .filter_map(|scope| {
                let probe = self.live.scope_probe(scope.output)?;
                let want = monitor::ScopeWant {
                    window: (scope.window * rate).round().max(1.0) as usize,
                    trigger: scope.trigger,
                };
                Some((probe, want))
            })
            .collect();
        let activity = self.live.activity_probes().collect();
        self.carrying.retain(|output| self.live.probe(*output).is_some());
        let list = monitor::WatchList {
            meters,
            activity,
            scopes,
            full_traces: self.full_traces,
        };
        if self.monitor.watched() != list {
            self.monitor.watch(list);
        }
        // A scope that left the screen, or lost its probe, keeps no trace.
        let shown: BTreeSet<(u32, usize)> = scene::scope_outputs(&self.controller, &self.visible_displays)
            .into_iter()
            .filter(|scope| self.live.scope_probe(scope.output).is_some())
            .map(|scope| scope.widget)
            .collect();
        self.traces.retain(|module, widget| shown.contains(&(module, widget)));
    }

    /// Takes new scope traces from the telemetry thread and gives each to
    /// the scopes that show its probe.
    fn take_traces(&mut self, traces: Vec<(ProbeId, loom_canvas::Trace)>) {
        if traces.is_empty() {
            return;
        }
        let scopes = scene::scope_outputs(&self.controller, &self.visible_displays);
        for (probe, trace) in traces {
            for scope in &scopes {
                if self.live.scope_probe(scope.output) == Some(probe) {
                    self.traces.set(scope.widget.0, scope.widget.1, trace.clone());
                }
            }
        }
    }

    /// Adds probes for meters that came on screen and removes those that
    /// left. Only probes change, so the scene is not rebuilt.
    fn sync_probes(&mut self) {
        self.sync_live();
        // Probe IDs are never reused, so a level of a removed probe is dead.
        let live: std::collections::HashSet<ProbeId> = self.live.probes().collect();
        self.meter_levels.retain(|probe, _| live.contains(probe));
        if let Err(e) = self.controller.commit() {
            self.alerts.say(format!("error: {e}"));
        }
    }

    /// Sets a parameter as a knob turn does. A slider sends no start, so its
    /// first change opens the gesture and letting go closes it.
    fn turn_param(&mut self, module: u32, index: usize, value: f32) {
        self.close_pending_arrow_group();
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
                app.alerts.say(format!("error: {e}"));
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
            self.alerts.say(format!("error: {e}"));
        }
        self.edits.touch(module, index);
        self.sync_audio_input();
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
            self.alerts.say(format!("{from} already modulates {to}"));
            return;
        }
        let depth = modulation::default_depth(&spec);
        match self.controller.add_assignment(source, target, index, depth) {
            Ok(_) => {
                if self.commit() {
                    self.alerts
                        .say(format!("{from} modulates {to}, {}", modulation::depth_text(depth)));
                }
            }
            Err(e) => self.alerts.say(format!("error: {e}")),
        }
    }

    /// One step of an arc drag. The first picks the assignment, by the
    /// spec's order; the rest of the drag keeps it.
    fn drag_depth(&mut self, module: u32, widget: usize, pinned: Option<loom_canvas::PortRef>, delta: f32) {
        let Some((param, _, _)) = scene::control_param(&self.controller, module, widget) else {
            return;
        };
        let graph = self.controller.graph();
        let picked = self.gestures.pick_depth(|| {
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
                    self.alerts.say(why.to_string());
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
                self.alerts.say(format!(
                    "{}: depth {}",
                    modulation::source_name(self.controller.graph(), a.source),
                    modulation::depth_text(depth)
                ));
            }
            Err(e) => self.alerts.say(format!("error: {e}")),
        }
        self.refresh_values();
    }

    fn report(&mut self) {
        if let Some(lost) = &self.rig.lost_audio {
            self.alerts.say(lost.message.clone());
            return;
        }
        let report = self.controller.last_report();
        let inserted = report.inserted_delays().count();
        self.alerts.say(match report.loops.len() {
            0 => "no feedback loops".to_string(),
            n => format!(
                "{n} feedback {}, {inserted} automatic 1-sample {} (dashed)",
                if n == 1 { "loop" } else { "loops" },
                if inserted == 1 { "delay" } else { "delays" },
            ),
        });
        // Warn only on entry into the overflow state, including after a
        // patch has returned below the table limit.
        if self.alerts.live_values_full(report.without_live_value > 0) {
            self.alerts.append(&format!(
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
        let (controller, positions, sizes) = (&self.controller, &self.positions, &self.sizes);
        self.edits.begin(controller.has_uncommitted_changes(), || {
            undo::Snapshot::take_with_sizes(controller, positions, sizes)
        });
    }

    /// Ends the recording and adds a step if the patch changed.
    fn record(&mut self) {
        // A command's name is for this step only, also when no step is made.
        let step_name = self.step_name.take();
        let (controller, positions, sizes) = (&self.controller, &self.positions, &self.sizes);
        self.edits.record(step_name, || {
            undo::Snapshot::take_with_sizes(controller, positions, sizes)
        });
    }

    /// Closes the open gesture, of either kind, and records it.
    fn end_gesture(&mut self) {
        let step_name = self.step_name.take();
        let (controller, positions, sizes) = (&self.controller, &self.positions, &self.sizes);
        self.edits
            .end_gesture(controller.has_uncommitted_changes(), step_name, || {
                undo::Snapshot::take_with_sizes(controller, positions, sizes)
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

    /// Closes an inspector slider or a released arrow group before another action.
    fn close_slider_gesture(&mut self) {
        if self.edits.slider_gesture_open() {
            self.end_gesture();
        }
        self.close_pending_arrow_group();
    }

    /// Records a released arrow edit before an action outside the canvas.
    fn close_pending_arrow_group(&mut self) {
        if self.arrow_group_pending {
            self.arrow_group_pending = false;
            self.arrow_group_closed = true;
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
        self.gestures.clear_label();
        let step = if forward {
            self.edits.history.redo_step()
        } else {
            self.edits.history.undo_step()
        };
        let Some(step) = step.cloned() else { return };
        let verb = if forward { "Redo" } else { "Undo" };
        if let Err(why) = undo::check(&step, forward, &self.controller) {
            self.alerts
                .say(format!("Cannot {} {}: {why}", verb.to_lowercase(), step.name()));
            return;
        }
        // An armed parameter is a mode, not an edit: undo disarms it first.
        self.learn.disarm();
        let applied = undo::apply_with_sizes(
            &step,
            forward,
            &mut self.controller,
            &mut self.positions,
            &mut self.sizes,
        );
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
                    self.alerts.say(format!("{verb}: {}", step.name()));
                } else {
                    // The graph has the change but the engine does not. The
                    // history is already cleared by `commit`.
                    self.alerts.say(format!(
                        "{verb} could not reach the engine ({}); undo history cleared",
                        self.alerts.status()
                    ));
                }
            }
            Err(e) => {
                self.edits.history.clear();
                self.commit();
                self.alerts.say(format!("{verb} failed ({e}); undo history cleared"));
            }
        }
    }

    /// Sends the graph to the engine. Returns false if that failed; the
    /// status bar then holds the error.
    fn commit(&mut self) -> bool {
        self.keep_run_state();
        // Probes follow the graph: a removed module loses its probes.
        self.sync_live();
        let sent = match self.controller.commit() {
            Ok(_) => {
                self.retry_commit = false;
                self.report();
                self.edits.committed(true);
                true
            }
            Err(e) => {
                self.retry_commit = matches!(&e, LoomError::QueueFull);
                self.alerts.say(format!("error: {e}"));
                // The controller stays dirty, so the next commit retries the
                // change; until then the history cannot be trusted.
                self.edits.committed(false);
                false
            }
        };
        self.sync_audio_input();
        self.refresh();
        sent
    }

    /// The one entry point for every message. It owns the work that crosses
    /// workflows, in this order:
    ///
    /// 1. Counts the frame and its cause, for `--measure-idle`.
    /// 2. Sends a safety mode change the engine's full queue refused.
    /// 3. Tells autosave to look again, unless the message keeps the patch.
    /// 4. Hands the message to its workflow: the browser, the control
    ///    channel, the matrix (`matrix_message`, `matrix_key_message`), a
    ///    clipboard command, or [`handle`](Self::handle) for the rest,
    ///    which sends file commands to `file_action`.
    /// 5. Runs the idle engine, and retries a commit that waited for room.
    /// 6. Quits cleanly when asked, or focuses the browser when it opens.
    ///
    /// A workflow returns the `Task` it needs; this joins them.
    fn update(&mut self, message: Message) -> Task<Message> {
        use loom_canvas::stats::FrameCause;
        let cause = match &message {
            Message::Tick => FrameCause::Tick,
            Message::LiveFrame => FrameCause::Live,
            Message::Meters(_) | Message::Telemetry(_) | Message::Lights(_) => FrameCause::Meters,
            Message::Control(_) | Message::ControlFrame(_) | Message::Located { .. } => FrameCause::Control,
            Message::Canvas(_) | Message::Key(_) | Message::File(FileAction::Close) => FrameCause::Input,
            Message::Frame(_) => FrameCause::Forced,
            _ => FrameCause::Other,
        };
        loom_canvas::stats::request_frame(cause);
        // A mode change the engine's full queue refused goes again now.
        self.send_pending_safety();
        if !message.keeps_patch() {
            // It may have changed the patch: autosave looks again.
            self.document.may_have_changed();
        }
        let was_open = self.browser.is_some();
        let was_help_open = self.help.is_some();
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
            Message::Matrix(event) => self.matrix_message(event),
            Message::MatrixKey(key) => self.matrix_key_message(key),
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
        // The audio callback, or the idle engine above, makes room for the
        // staged modules. Tick messages keep this retry moving even without
        // another edit from the player.
        if self.retry_commit {
            self.commit();
            self.drain_idle_engine();
        }
        if self.quit {
            // Stop audio now, with a fade so it does not click, so a
            // recording is closed before the process ends, and leave no
            // control socket behind.
            self.stop_audio();
            control::close(&self.control);
            // A clean exit: the recovery folder goes (autosave.md, Clean exit).
            self.document.finish();
            Task::batch([task, iced::exit()])
        } else if self.browser.is_some() && (!was_open || (was_help_open && self.help.is_none())) {
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
            Err(e) => app.alerts.say(format!("Could not add {}: {e}", descriptor.name)),
        });
    }

    /// Adds a scope or a meter fed by `output`, beside `world`, and selects
    /// it. The module and its cable are one undo step.
    fn add_probe(&mut self, probe: probe_menu::Probe, output: loom_canvas::PortRef, world: Point) {
        let Some(descriptor) = self.modules.iter().copied().find(|d| d.id == probe.id()) else {
            self.alerts
                .say(format!("Cannot {}: no module `{}`", probe.verb(), probe.id()));
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
            self.alerts.say(format!("Cannot {}: the output is gone", probe.verb()));
            return;
        }
        self.close_slider_gesture();
        self.step_name = Some(probe.verb().to_string());
        self.edit(|app| {
            let id = match app.controller.add_module(descriptor.id) {
                Ok(id) => id,
                Err(e) => {
                    app.alerts.say(format!("Cannot {}: {e}", probe.verb()));
                    return;
                }
            };
            if let Err(e) = app.controller.connect(source, InputRef { module: id, port: 0 }) {
                // A probe that is not connected shows nothing: take it back,
                // so the edit makes no step.
                let _ = app.controller.remove_module(id);
                app.commit();
                app.alerts.say(format!("Cannot {}: {e}", probe.verb()));
                return;
            }
            app.positions.insert(id.0, corner);
            app.selection = BTreeSet::from([id.0]);
            app.commit();
        });
        // `record` clears it, but an edit inside a gesture does not record.
        self.step_name = None;
    }

    /// The messages that return no task. Called from [`update`](Self::update),
    /// which has done the shared work first.
    fn handle(&mut self, message: Message) {
        match message {
            // `update` runs these: they return tasks.
            Message::Browser(_)
            | Message::Control(_)
            | Message::ControlFrame(_)
            | Message::Located { .. }
            | Message::Matrix(_)
            | Message::MatrixKey(_) => {}
            Message::ShowHelp => {
                if self.help.is_none() && self.rig.window.is_none() {
                    let descriptor = self
                        .browser
                        .as_ref()
                        .and_then(browser::Browser::highlighted)
                        .or_else(|| {
                            (self.browser.is_none() && self.selection.len() == 1)
                                .then(|| {
                                    self.selection
                                        .iter()
                                        .next()
                                        .and_then(|id| self.controller.graph().node(ModuleId(*id)))
                                        .map(|node| node.descriptor)
                                })
                                .flatten()
                        });
                    // With nothing to describe, F1 shows the shortcuts.
                    self.help = Some(descriptor.map_or_else(help::Help::shortcuts, help::Help::new));
                }
            }
            Message::ShowShortcuts => {
                if self.rig.window.is_none() {
                    self.help = Some(help::Help::shortcuts());
                }
            }
            Message::ShowModuleHelp(id) => {
                self.tile_menu = None;
                self.help = self
                    .controller
                    .graph()
                    .node(ModuleId(id))
                    .map(|node| help::Help::new(node.descriptor));
            }
            Message::CloseHelp => self.help = None,
            Message::HelpExample => {
                let example = self.help.as_ref().and_then(|help| help.model.example.clone());
                if let Some(example) = example {
                    match Patch::from_ron(&example) {
                        Ok(patch) if self.can_discard() => {
                            self.replace(Source::Recovered(Box::new(patch)));
                        }
                        Ok(_) => {}
                        Err(error) => self.alerts.say(format!("Could not open example: {error}")),
                    }
                }
            }
            Message::HelpLink(link) => {
                let descriptor = link
                    .strip_suffix(".md")
                    .and_then(|id| self.modules.iter().find(|d| d.id == id))
                    .copied();
                if let Some(descriptor) = descriptor {
                    self.help = Some(help::Help::new(descriptor));
                } else {
                    self.alerts.say(format!("Linked document is not bundled: {link}"));
                }
            }
            Message::CloseTileMenu => self.tile_menu = None,
            Message::Key(_) if self.help.is_some() => {}
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
            Message::Key(iced::keyboard::Event::KeyPressed { modifiers, .. })
                if self.matrix.tab == RightTab::Matrix && self.matrix.focused && !modifiers.command() => {}
            Message::Key(iced::keyboard::Event::KeyReleased { .. })
                if self.matrix.tab == RightTab::Matrix && self.matrix.focused => {}
            Message::Key(iced::keyboard::Event::KeyPressed { key, .. }) if self.probe_menu.is_some() => {
                // The menu takes Escape and keeps other keys from the canvas.
                if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                    self.probe_menu = None;
                }
            }
            Message::Key(_) if self.probe_menu.is_some() => {}
            Message::Key(iced::keyboard::Event::KeyPressed { key, .. }) if self.tile_menu.is_some() => {
                if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                    self.tile_menu = None;
                }
            }
            Message::Key(_) if self.tile_menu.is_some() => {}
            Message::ProbeMenu(message) => {
                if let (Some(menu), probe_menu::Message::Pick(probe)) = (self.probe_menu.take(), message) {
                    self.add_probe(probe, menu.output, menu.world);
                }
            }
            Message::AddProbe { probe, output, world } => self.add_probe(probe, output, world),
            Message::Canvas(event) => self.canvas_event(event),
            Message::UnknownLabelDraft { module, value } => self.gestures.draft_label(module, value),
            Message::UnknownLabelApply { module } => {
                if let Some(value) = self.gestures.take_label(module) {
                    let label = (!value.is_empty()).then_some(value);
                    self.edit(|app| {
                        if app.controller.set_unknown_label(ModuleId(module), label) {
                            app.refresh();
                        }
                    });
                }
            }
            Message::DropRow(row) => {
                let allowed = !self.learn.is_on();
                self.gestures.hover_row(row, allowed);
            }
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
                self.gestures.toggle_adding((module, index));
            }
            Message::Assign { source, module, index } => {
                self.gestures.close_adding();
                self.close_slider_gesture();
                self.edit(|app| app.assign(source, ModuleId(module), index));
            }
            Message::Depth { id, depth } => {
                // Like a parameter slider: the first change opens the
                // gesture, and letting go closes it, so one drag is one step.
                self.close_pending_arrow_group();
                self.edits.start_slider_gesture();
                self.begin_edit();
                if let Err(e) = self.controller.set_assignment_depth(id, depth) {
                    self.alerts.say(format!("error: {e}"));
                }
                self.refresh_values();
            }
            Message::Polarity { id, choice } => {
                self.close_slider_gesture();
                self.edit(|app| {
                    if let Err(e) = app.controller.set_assignment_polarity(id, choice.polarity()) {
                        app.alerts.say(format!("error: {e}"));
                    }
                    app.refresh_values();
                });
            }
            Message::Enabled { id, enabled } => {
                self.close_slider_gesture();
                self.edit(|app| {
                    if let Err(e) = app.controller.set_assignment_enabled(id, enabled) {
                        app.alerts.say(format!("error: {e}"));
                    } else {
                        app.scene.set_modulation_enabled(id.0, enabled);
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
                        app.alerts.say(format!(
                            "Removed modulation from {} to {}",
                            modulation::source_name(graph, removed.source),
                            modulation::target_name(graph, removed.target, removed.param)
                        ));
                    }
                    Err(e) => app.alerts.say(format!("error: {e}")),
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
            Message::File(action) => self.file_action(action),
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
            Message::LiveFrame => {
                scene::update_live_values(&mut self.values, &self.controller);
                if !self.rig.input_warned && self.rig.input.is_some() {
                    self.sync_audio_input();
                }
            }
            Message::Autosave => self.autosave(Instant::now()),
            Message::Tick => {
                self.controller.collect_garbage();
                self.update_dropouts(Instant::now());
            }
            Message::AudioPoll => self.poll_audio(),
            Message::InputAudio(event) => {
                self.alerts
                    .set_input_drift(event.show.then_some((event.underruns, event.overruns)));
                if event.failed || event.ready {
                    self.sync_audio_input();
                }
            }
            Message::Audio(audio_watch::Event::Stats {
                dropouts,
                at,
                peak,
                max,
            }) => {
                self.stream.set_load(peak, max);
                self.alerts.note_dropouts(dropouts, at);
            }
            Message::Audio(audio_watch::Event::Stalled { callbacks, advanced }) => {
                // Time the stall from when the watcher last saw a callback,
                // as a poll on this thread would have.
                self.stream.watcher_saw(callbacks, advanced);
                self.poll_audio();
            }
            Message::Meters(levels) => {
                // A reading may arrive after its probe was removed.
                let live: std::collections::HashSet<ProbeId> = self.live.probes().collect();
                self.meter_levels
                    .extend(levels.into_iter().filter(|(probe, _)| live.contains(probe)));
                self.refresh_values();
            }
            Message::Telemetry(update) => {
                self.take_traces(update.traces);
                for (probe, carrying) in update.activity {
                    if let Some(output) = self.live.output_of(probe) {
                        if carrying {
                            self.carrying.insert(output);
                        } else {
                            self.carrying.remove(&output);
                        }
                    }
                }
                if !update.levels.is_empty() {
                    self.handle(Message::Meters(update.levels));
                }
            }
            Message::Lights(readings) => {
                self.lights.extend(readings);
                self.sync_audio_input();
                self.refresh_values();
            }
            Message::Learn(message) => self.learn_message(message),
            Message::Unmap { module, index } => {
                self.close_slider_gesture();
                self.edit(|app| match app.controller.unmap_midi(ModuleId(module), index) {
                    Ok(Some(mapping)) => {
                        app.commit();
                        app.alerts.say(format!(
                            "Removed {} from {}",
                            mapping.control,
                            learn::param_name(&app.controller, (module, index))
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => app.alerts.say(format!("error: {e}")),
                });
            }
            Message::Midi => {
                self.midi_arrived();
                self.sync_audio_input();
            }
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
            }) if self.learn.is_on() => self.stop_learning(),
            Message::Key(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) if modifiers.command() => {
                // Ctrl+Z or Cmd+Z undoes; adding Shift redoes. Ctrl+Y also
                // redoes, except on macOS.
                match key.as_ref() {
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") && modifiers.shift() => {
                        self.redo()
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") => self.undo(),
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("n") => {
                        self.file_action(FileAction::New)
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("o") => {
                        self.file_action(FileAction::Open)
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("s") && modifiers.shift() => {
                        self.file_action(FileAction::SaveAs);
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("s") => {
                        self.file_action(FileAction::Save);
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
        if event == CanvasEvent::PointerDown {
            self.matrix.focused = false;
            self.matrix.edit = None;
            self.matrix.add = None;
            return;
        }
        let is_unknown = |id: u32| {
            self.controller
                .graph()
                .node(ModuleId(id))
                .is_some_and(|node| node.unknown.is_some())
        };
        let forbidden = match &event {
            CanvasEvent::Connect { from, to } => is_unknown(from.module) || is_unknown(to.module),
            CanvasEvent::Assign { from, module, .. } => is_unknown(from.module) || is_unknown(*module),
            CanvasEvent::AssignDrop { from, .. } => is_unknown(from.module),
            _ => false,
        };
        if forbidden {
            self.alerts.say("Cannot connect to an unknown module");
            self.gestures.cancel_drag();
            return;
        }
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
            CanvasEvent::PointerDown => {}
            CanvasEvent::AssignDrag { from, start, at, zoom } => {
                self.gestures.drag(gestures::AssignDrag { from, start, at, zoom });
            }
            CanvasEvent::AssignDrop { from, at } => {
                let landed = at.is_some() && !self.learn.is_on();
                if let Some((module, index)) = self.gestures.finish_drag(from, landed) {
                    let source = OutputRef {
                        module: ModuleId(from.module),
                        port: from.index,
                    };
                    self.assign(source, ModuleId(module), index);
                }
            }
            CanvasEvent::AssignCancel => self.gestures.cancel_drag(),
            CanvasEvent::GestureStart => {
                self.close_slider_gesture();
                self.edits.start_gesture();
                self.begin_edit();
            }
            CanvasEvent::GestureEnd => {
                if self.gestures.end_depth() {
                    self.refresh_values();
                }
                self.end_gesture();
            }
            CanvasEvent::ArrowGroupPending => self.arrow_group_pending = true,
            CanvasEvent::ArrowGroupResumed => {
                self.arrow_group_pending = false;
                if self.arrow_group_closed {
                    self.close_slider_gesture();
                    self.edits.start_gesture();
                    self.begin_edit();
                    self.arrow_group_closed = false;
                }
            }
            CanvasEvent::ArrowGroupEnd => {
                self.arrow_group_pending = false;
                if !self.arrow_group_closed {
                    self.end_gesture();
                }
                self.arrow_group_closed = false;
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
            CanvasEvent::Resize { module, size } => {
                if let Some(tile) = self.scene.module(module).filter(|tile| tile.is_growable()) {
                    let min = tile.minimum_size();
                    let min_w = (min.width / loom_canvas::GRID).ceil() as u8;
                    let min_h = (min.height / loom_canvas::GRID).ceil() as u8;
                    let size = (size.0.clamp(min_w, min_w.max(64)), size.1.clamp(min_h, min_h.max(64)));
                    let current = (
                        (tile.width() / loom_canvas::GRID) as u8,
                        (tile.height() / loom_canvas::GRID) as u8,
                    );
                    if size != current {
                        if size == (min_w, min_h) {
                            self.sizes.remove(&module);
                        } else {
                            self.sizes.insert(module, size);
                        }
                        self.refresh();
                    }
                }
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
                    Err(e) => self.alerts.say(format!("error: {e}")),
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
            CanvasEvent::TileMenu { module, at, area } => {
                self.tile_menu = Some((
                    module,
                    Point::new(
                        at.x.min(area.x + area.width - 140.0).max(area.x),
                        at.y.min(area.y + area.height - 36.0).max(area.y),
                    ),
                ));
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
            CanvasEvent::TraceDetail(full) => {
                self.full_traces = full;
                self.publish_watch();
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
                self.alerts.say(format!("error: {e}"));
            }
            return;
        };
        if run_is_driven(&self.controller) {
            self.alerts
                .say("Run follows the cable on the Transport's run input".to_string());
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
            clipboard::Group::from_selection(self.controller.graph(), &self.positions, &self.selection, incoming)
                .with_sizes(&self.sizes);
        if group.is_empty() {
            self.alerts.say(format!("Nothing selected to {verb}"));
            return None;
        }
        Some(group)
    }

    /// Copy: returns the patch text for the clipboard. Not an edit.
    fn copy(&mut self) -> Option<String> {
        let group = self.selected_group("copy", false)?;
        self.alerts.say(format!("Copied {}", group.describe()));
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
                app.alerts.say(format!("Cut {}", group.describe()));
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
            self.alerts.say(clipboard::NOTHING_TO_PASTE.to_string());
            return;
        };
        match clipboard::Group::from_text(&text, self.controller.registry()) {
            Ok(group) => self.add_group(&group, browser::snap(world), "paste", "Pasted"),
            Err(why) => self.alerts.say(why),
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
            self.alerts.say(format!("Cannot {verb}: {why}"));
            return;
        }
        self.step_name = Some(format!("{verb} {}", group.describe()));
        self.edit(|app| match group.add(&mut app.controller, &mut app.positions, origin) {
            Ok(ids) => {
                group.apply_sizes(&ids, &mut app.sizes);
                app.selection = ids.into_iter().collect();
                if app.commit() {
                    app.alerts.say(format!("{done} {}", group.describe()));
                }
            }
            Err(e) => {
                // The rollback freed the new modules' slots. Only a commit
                // hands them back, so the next paste has room.
                app.commit();
                app.alerts.say(format!("Cannot {verb}: {e}"));
            }
        });
        // `record` clears it, but an edit inside a gesture does not record.
        self.step_name = None;
    }

    fn view(&self) -> Element<'_, Message> {
        let mut canvas = PatchCanvas::new(&self.scene, &self.selection, Message::Canvas)
            .values(&self.values)
            .window_drag_line(self.gestures.assign_drag().is_some())
            .traces(&self.traces)
            .activity(self.carrying.iter().map(|output| (output.module.0, output.port)))
            .frame_key(self.frame_key)
            .browser_key(self.browser_key)
            .interactive(
                self.browser.is_none()
                    && self.probe_menu.is_none()
                    && self.tile_menu.is_none()
                    && self.help.is_none()
                    && self.rig.window.is_none(),
            )
            .matrix_line(self.matrix_line())
            .matrix_keyboard(self.matrix.tab == RightTab::Matrix && self.matrix.focused)
            .matrix_escape(
                self.matrix.tab == RightTab::Matrix
                    && self.matrix.focused
                    && self.matrix.edit.is_none()
                    && self.matrix.add.is_none(),
            )
            .animate(self.animating)
            .learn(self.learn.shown());
        if let Some(still) = self.still {
            canvas = canvas.still(still);
        }

        let body = row![
            container(canvas).width(Length::Fill).height(Length::Fill),
            rule::vertical(1),
            self.right_panel(),
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
        if let Some(drag) = self.gestures.assign_drag()
            && let Some(at) = drag.at
            && let Some(port) = self.scene.port(drag.from)
        {
            layers = layers.push(DragLine::new(drag.start, at, port.colour, drag.zoom));
        }
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
        if let Some((module, at)) = self.tile_menu {
            let outside = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::CloseTileMenu)
                .on_right_press(Message::CloseTileMenu);
            let menu = container(button("Help").on_press(Message::ShowModuleHelp(module)).width(140))
                .padding(4)
                .style(container::rounded_box);
            layers = layers
                .push(outside)
                .push(iced::widget::pin(iced::widget::opaque(menu)).position(at));
        }
        if let Some(help) = &self.help {
            layers = layers.push(iced::widget::opaque(help.view(self.theme())));
        }
        if let Some(window) = self.preferences_view() {
            layers = layers.push(window);
        }
        // The control channel's pointer events go in here, as a real
        // pointer's would (control-channel.md, Pointer commands).
        let browser_open = self.browser.is_some() && self.help.is_none();
        let matrix_keys = self.matrix.tab == RightTab::Matrix && self.matrix.focused;
        pointer::Injector::new(layers, self, move |key, status| {
            // As the subscriptions do: `keyboard::listen` passes on what no
            // widget took, and the open browser hears every key.
            let mut messages = Vec::new();
            if status == iced::event::Status::Ignored {
                messages.push(Message::Key(key.clone()));
            }
            if matrix_keys && let Some(message) = matrix::key_message(key.clone(), status) {
                messages.push(message);
            }
            if let Some(message) = panic::key(iced::Event::Keyboard(key.clone()), status, iced::window::Id::unique()) {
                messages.push(message);
            }
            if let Some(message) = help_key(iced::Event::Keyboard(key.clone()), status, iced::window::Id::unique()) {
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

    /// The toolbar: the patch name, then buttons in groups (file, add,
    /// play, edit, setup), with wider space between groups than inside them
    /// (visual-system.md, Toolbar).
    fn toolbar(&self) -> Element<'_, Message> {
        let tip = |button: iced::widget::Button<'static, Message>, hint: &'static str| -> Element<'static, Message> {
            tooltip(
                button,
                container(text(hint).size(style::BODY))
                    .padding(6)
                    .style(container::rounded_box),
                tooltip::Position::Bottom,
            )
            .into()
        };
        let label = |s: &'static str| text(s).size(style::CONTROL);
        let file = row![
            button(label("New")).on_press(Message::File(FileAction::New)),
            button(label("Open")).on_press(Message::File(FileAction::Open)),
            button(label("Save")).on_press(Message::File(FileAction::Save)),
            button(label("Save As")).on_press(Message::File(FileAction::SaveAs)),
        ];
        let add = tip(
            button(label("Add module")).on_press(Message::OpenBrowser),
            "Or press A over the canvas, or double-click empty canvas",
        );
        let play = row![
            button(label(if self.holding { "Release note" } else { "Hold note" }))
                .on_press_maybe(self.midi.as_ref().map(|_| Message::HoldNote)),
            tip(
                button(label("Panic")).on_press(Message::Panic).style(style::danger),
                if cfg!(target_os = "macos") {
                    "Silence everything: Cmd+."
                } else {
                    "Silence everything: Ctrl+."
                },
            ),
        ];
        let edit = row![
            self.history_button("Undo", "Undo", self.edits.history.undo_step(), Message::Undo),
            self.history_button("Redo", "Redo", self.edits.history.redo_step(), Message::Redo),
        ];
        let setup = row![
            // Primary only while learning: a mode that is on.
            button(label("MIDI learn"))
                .on_press(Message::Learn(learn::Message::Toggle))
                .style(if self.learn.is_on() {
                    style::primary
                } else {
                    style::secondary
                }),
            tip(
                button(label("Preferences")).on_press(Message::Preferences(settings::Message::Open)),
                if cfg!(target_os = "macos") { "Cmd+," } else { "Ctrl+," },
            ),
        ];
        let group = |items: iced::widget::Row<'static, Message>| items.spacing(6).align_y(iced::Alignment::Center);
        row![
            text(format!(
                "{}{}",
                if self.unsaved() { "* " } else { "" },
                self.patch_name()
            ))
            .size(style::HEADING),
            group(file),
            space::horizontal(),
            add,
            group(play),
            group(edit),
            group(setup),
        ]
        .spacing(20)
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
            button(text(label).size(style::CONTROL)).on_press_maybe(step.map(|_| message)),
            container(text(hint).size(style::BODY))
                .padding(6)
                .style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .into()
    }

    fn inspector(&self) -> Element<'_, Message> {
        inspector::view(
            self.inspector_model(),
            self.gestures.drop_row(),
            self.gestures.assign_drag().is_some() && !self.learn.is_on(),
        )
    }

    /// Inspector and matrix share the same width in the right-hand column.
    fn right_panel(&self) -> Element<'_, Message> {
        // The open tab has a fill and a border (visual-system.md, Tabs).
        let tab = |label: &'static str, which: RightTab| {
            button(text(label).size(style::CONTROL))
                .padding([4, 12])
                .style(style::tab(self.matrix.tab == which))
                .on_press(Message::Matrix(MatrixEvent::Tab(which)))
        };
        let tabs = row![tab("Inspector", RightTab::Inspector), tab("Matrix", RightTab::Matrix)]
            .spacing(4)
            .padding([6, 8]);
        let panel = match self.matrix.tab {
            RightTab::Inspector => self.inspector(),
            RightTab::Matrix => matrix::view(
                self.matrix.model(self.controller.graph(), &self.selection),
                &self.matrix,
                self.controller.graph(),
            ),
        };
        column![tabs, panel].width(280).height(Length::Fill).into()
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
            let load = bar_text(stats.load_text()).color(severity_colour(alerts::load_severity(self.stream.load().0)));
            let dropouts = bar_text(format!("dropouts {}", stats.dropouts))
                .color(severity_colour(self.alerts.dropout_severity(Instant::now())));
            let mut items = row![bar_text(stats.label).color(DIM), load, dropouts].spacing(10);
            if let Some((underruns, overruns)) = stats.input_alert {
                items = items.push(
                    bar_text(format!("input underruns {underruns}, overruns {overruns}"))
                        .color(severity_colour(alerts::Severity::Warning)),
                );
            }
            if stats.telemetry_dropped > 0 {
                items = items.push(bar_text(format!("telemetry dropped {}", stats.telemetry_dropped)).color(DIM));
            }
            items.into()
        } else {
            bar_text(stats.label).color(DIM).into()
        };
        // A count, with the names in hover text, so a long list never
        // pushes the other items off the window (visual-system.md, rule 7).
        let midi: Element<'_, Message> = match self.rig.ports.as_slice() {
            [] => bar_text("MIDI: no inputs").color(DIM).into(),
            ports => tooltip(
                bar_text(match ports.len() {
                    1 => "MIDI: 1 input".to_string(),
                    n => format!("MIDI: {n} inputs"),
                })
                .color(DIM),
                container(text(ports.iter().map(|port| port.name()).collect::<Vec<_>>().join("\n")).size(style::BODY))
                    .padding(6)
                    .style(container::rounded_box),
                tooltip::Position::Top,
            )
            .into(),
        };
        let mut bar = row![].spacing(16).align_y(iced::Alignment::Center);
        if self.dev_tools {
            bar = bar
                .push(
                    toggler(self.measuring)
                        .label("Measure")
                        .on_toggle(Message::Measure)
                        .size(style::SWITCH)
                        .text_size(style::BODY),
                )
                .push(
                    toggler(self.animating)
                        .label("Move camera")
                        .on_toggle(Message::Animate)
                        .size(style::SWITCH)
                        .text_size(style::BODY),
                )
                .push(bar_text(frames));
        }
        bar.push(bar_text(format!(
            "{} modules, {} cables",
            graph.len(),
            graph.cables().count()
        )))
        .push(audio)
        .push(midi)
        // The status takes the remaining width and is cut off rather
        // than wrapped, so the bar stays one line high. Hover shows all
        // of it.
        .push(tooltip(
            container(bar_text(self.alerts.status()).color(DIM))
                .width(Length::Fill)
                .align_x(iced::Alignment::End)
                .clip(true),
            container(text(self.alerts.status()).size(style::BODY))
                .padding(6)
                .max_width(480)
                .style(container::rounded_box),
            tooltip::Position::Top,
        ))
        .padding([4, 10])
        .into()
    }
}

/// The inspector's remove button for an item in a list, such as a MIDI
/// mapping or a modulation assignment: a small × at the end of its row,
/// with hover text that names what it removes (panels.md, The inspector).
fn remove_button<'a>(what: &'a str, on_press: Message) -> Element<'a, Message> {
    tooltip(
        button(text("×").size(style::CONTROL))
            .padding([0, 6])
            .style(style::plain)
            .on_press(on_press),
        container(text(what).size(style::BODY))
            .padding(6)
            .style(container::rounded_box),
        tooltip::Position::Top,
    )
    .into()
}

/// Stats bar text: one line, never wrapped.
fn bar_text<'a>(content: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(content).size(style::BODY).wrapping(text::Wrapping::None)
}

/// Quiet text. The same as `style::QUIET`, kept as a short name here.
const DIM: Color = style::QUIET;

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
/// and the key handling in `loom-canvas/src/widget/input.rs` and `panel_input.rs`.
pub(crate) const HELP: &str = "A or a double-click on empty canvas opens the module browser at the pointer; type, then Enter adds.\n\
Drag from a port to patch a cable. Drag a connected input to move or remove its cable.\n\
Drag tiles to move them. Shift-click or drag a box to select several. Cmd/Ctrl+A selects all.\n\
Delete or Backspace removes the selection.\n\
Drag a knob up or down; Shift for fine steps; double-click resets it.\n\
Tab and Shift-Tab select modules in reading order. Enter focuses the selected module's first control.\n\
With a control focused, Tab moves between controls and arrow keys change values. Space or Enter acts on a toggle, radio, or button.\n\
Escape clears control focus, then the selection. With a module selected and no control focused, arrow keys move it.\n\
Space toggles Run when no control uses it, including a focused knob.\n\
Right-drag or two-finger scroll to pan. Wheel or Cmd/Ctrl-scroll to zoom. Cmd/Ctrl+0 zooms to 100%.\n\
F or Home frames the patch.\n\
Right-click an output or a cable to add a scope or a meter fed by it.\n\
Click a modulation badge to pin its lines; click again or press Escape to hide them.\n\
Cmd/Ctrl+C copies, X cuts, V pastes at the pointer, and D duplicates the selection.\n\
Cmd/Ctrl+Z undoes, and Cmd/Ctrl+Shift+Z redoes.\n\
MIDI learn: click a control, then move a knob on your controller. Escape ends it.";

/// F1 and Escape reach help even while a text field has focus.
fn help_key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    match event {
        iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Named(Named::F1),
            ..
        }) => Some(Message::ShowHelp),
        iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        }) => Some(Message::CloseHelp),
        _ => None,
    }
}

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
mod tests;
