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
mod boot;
mod browser;
mod canvas_events;
mod clipboard;
mod control;
mod controls;
mod devices;
mod document;
mod editing;
mod edits;
#[cfg(test)]
mod example_tests;
mod file_commands;
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
mod subscriptions;
mod targets;
#[cfg(test)]
mod ui_test;
mod undo;
mod view;

use clap::Parser;
use iced::{Color, Point, Size, Task, Theme};
use live::Live;
use loom_canvas::{CanvasEvent, PanelValues, Scene};
use loom_core::{
    AssignmentId, Controller, InputRef, MidiEvent, ModuleDescriptor, ModuleId, OutputRef, OutputSafety, ProbeId,
};
use loom_host_desktop::{CallbackStats, SharedMidi};
use loom_patch::Patch;
use matrix::{Event as MatrixEvent, Tab as RightTab};
use modulation::PolarityChoice;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

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

    fn title(&self) -> String {
        files::titled(&self.patch_name(), self.unsaved())
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
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
}

/// Quiet text. The same as `style::QUIET`, kept as a short name here.
const DIM: Color = style::QUIET;

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

#[cfg(test)]
mod tests;
