//! `loom-app`: the LoomDSP desktop app.
//!
//! Phase 1 prototype: the patch canvas (ADR-0010), an inspector for the
//! selected module, live audio and MIDI, and a stats bar for measuring the
//! canvas against the performance gates in NFR-PERF-04.
//!
//! A library, so that tests under `tests/` can import `ui_test::Ui` with the
//! `test-ui` feature. The program in `src/bin/loom-app.rs` calls [`run`].

mod a11y;
mod alerts;
mod audio_watch;
#[cfg(test)]
mod autosave_tests;
mod bars;
mod bench;
mod boot;
mod breadcrumb;
mod browser;
mod builtin;
mod cable_picker;
mod canvas_events;
mod chrome;
mod clip;
mod clipboard;
mod clock;
mod control;
mod controls;
mod devices;
mod dialogs;
mod document;
mod documents;
mod editing;
mod edits;
#[cfg(test)]
mod example_tests;
mod file_commands;
mod files;
mod gestures;
mod help;
mod history;
mod iced_bridge;
mod input_owner;
mod input_session;
mod input_watch;
mod inspector;
mod layout;
mod learn;
mod levels;
#[cfg(test)]
mod library_tests;
mod licence_chooser;
mod lights;
mod live;
mod matrix;
mod menus;
mod modes;
mod modulation;
mod monitor;
mod outline;
mod overlays;
mod panic;
mod param;
mod patch_file;
mod patch_info;
mod patch_map;
mod patches;
mod places;
mod pointer;
#[cfg(test)]
mod polysynth_tests;
mod preferences;
mod probe_menu;
#[cfg(test)]
mod qa_tests;
mod question;
mod recent;
mod recovery;
mod runtime_fields;
mod scene;
mod screenshot;
mod script_vars;
mod session;
mod settings;
mod shortcut;
mod slots;
mod snapshot;
mod spectrum;
mod stats_bar;
mod stream;
mod style;
mod subscriptions;
mod surface;
mod targets;
mod timers;
mod toolbar;
#[cfg(any(test, feature = "test-ui"))]
#[allow(dead_code, reason = "the crate's unit tests use the rest of this harness")]
pub mod ui_test;
mod undo;
mod view;
mod watcher;
mod watching;
mod window_question;
mod xy;

use clap::Parser;
use iced::{Color, Size, Theme};
use loom_canvas::CanvasEvent;
use loom_core::{Controller, InputRef, MidiEvent, ModuleDescriptor, ModuleId, OutputRef, OutputSafety, ProbeId};
use loom_patch::Patch;
use loom_ui_core::Point;
use matrix::{Event as MatrixEvent, Tab as RightTab};
use overlays::Overlay;
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
    /// the recording. Standard WAV is limited to 4 GiB (about 3 hours at 48 kHz).
    #[arg(long, value_name = "WAV", conflicts_with = "no_audio")]
    record: Option<PathBuf>,
    /// Benchmark options.
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
    #[arg(long, conflicts_with = "screenshot", groups = ["inspect", "focusable_output"])]
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

/// Runs the program with the command line it was started with.
pub fn run() -> Result<(), loom_ui::shell::Error> {
    let mut args = Args::parse();
    if args.bench.bench_autosave_copy {
        if args.patch.is_none() && args.generate.is_none() {
            args.generate = Some(300);
        }
        match snapshot::headless(&args) {
            Ok(app) => bench::autosave_copy(&app),
            Err(e) => {
                eprintln!("loom-app: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
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
    let mut app = App::new(args.clone());
    app.remember_opened_at_start(&args);
    let result = loom_ui::shell::run(
        iced_bridge::Hosted(app, None, iced_bridge::Extra::default()),
        iced_bridge::IcedTarget::default(),
        iced_bridge::WindowLayers::new(),
        iced_bridge::AppServices,
        loom_ui::shell::Options {
            size: loom_ui_core::Size::new(WINDOW.width, WINDOW.height),
            // The window is open: the watchers start on the first turn.
            first: vec![Message::StartWatching],
        },
    );
    // A control script that stopped early asks for its own status.
    match control::exit_status() {
        0 => result,
        status => std::process::exit(status),
    }
}

/// The window size the app opens with.
const WINDOW: Size = Size::new(1440.0, 900.0);

/// The renderer the app's views are built for. Every view, widget, and
/// element names this alias rather than Iced's own `Renderer`. It is Iced's
/// fallback renderer over two: the window always draws with the first,
/// wgpu; the headless harness lays out on the second, which measures text
/// as wgpu does and draws nothing, so event tests need no GPU (step 2.3,
/// items 2 and 3 of the plan for leaving Iced).
pub(crate) type Renderer = iced_renderer::fallback::Renderer<iced_wgpu::Renderer, iced_bridge::Measure>;

/// An element of the app's views, built for [`Renderer`].
pub(crate) type Element<'a, M = Message> = iced::Element<'a, M, Theme, Renderer>;

#[derive(Debug, Clone)]
enum Message {
    /// The player pressed × on a kept error.
    DismissError(alerts::Cause),
    Canvas(CanvasEvent),
    /// The runtime has updated the window's scale.
    WindowRescaled,
    /// A window event for the app's one input listener, and whether a widget
    /// used it. `update` turns it into the messages it routes to
    /// (`Listening::route`).
    Input {
        event: iced::Event,
        status: iced::event::Status,
        /// Who was listening when the event was heard.
        listening: input_owner::Listening,
    },
    /// The toolbar asks for the module browser at the centre of the canvas.
    OpenBrowser,
    /// Picks a port in the cable menu, or disconnects its chosen input.
    CablePort {
        start: Option<loom_canvas::PortRef>,
        port: Option<loom_canvas::PortRef>,
    },
    /// Open help for the browser highlight or selected tile.
    ShowHelp,
    /// Open the keyboard shortcuts page in the help overlay.
    ShowShortcuts,
    /// Open help for a specific tile.
    ShowModuleHelp(u32),
    /// Reset every clip light and peak hold on a module, from the inspector.
    ResetClip(u32),
    /// Close module help.
    CloseHelp,
    /// Open the current help example as a patch.
    HelpExample,
    /// A link in bundled help was selected.
    HelpLink(String),
    /// Close the tile menu.
    CloseTileMenu,
    /// Show a level of the patch: the top, or the inside of a sub-patch
    /// (the breadcrumb bar).
    OpenLevel(loom_patch::ModulePath),
    /// Open the sub-patch tile with this id.
    OpenTile(u32),
    /// Make a sub-patch of the selection and this tile (the tile menu).
    MakeSubPatch(u32),
    /// Dissolve this sub-patch tile (the tile menu).
    DissolveSubPatch(u32),
    /// Save this sub-patch tile to the library (the tile menu).
    SaveToLibrary(u32),
    /// Make this linked sub-patch tile a plain copy (the tile menu).
    Unlink(u32),
    /// Make this module or sub-patch tile the part of a new slot for the
    /// role with this id (the tile menu; sub-patches.md, Slots).
    MakeSlot {
        /// The tile.
        module: u32,
        /// The role's id, such as `loom.role.filter`.
        role: String,
    },
    /// Swap what this slot tile holds (the tile menu, the inspector).
    SwapSlot {
        /// The slot tile.
        slot: u32,
        /// The part to take.
        to: slots::SwapTarget,
    },
    /// Make this slot tile's part a plain module again (the tile menu).
    DissolveSlot(u32),
    /// A gesture on a sub-patch tile's mode rows in the inspector
    /// (sub-patches.md, The editor for modes).
    Mode(modes::Message),
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
    /// The window is open: start the watcher threads (`watching.rs`) and
    /// the timers (`timers.rs`). The bridge sends it once, as it starts.
    StartWatching,
    /// A watched engine drained the queue after staging a graph edit.
    RetryCommit(u64),
    /// Follows a control command's UI messages in the queue: they have run,
    /// so its answer can be read (`control/run.rs`).
    ControlReply,
    /// A frame shown while control commands wait to be drawn, or sample
    /// it. An after-draw message, so it carries the time of the turn that
    /// asked for it, a frame before, not the `cx.now()` of the turn it
    /// arrives in: each step between them is still a frame, as `sample`
    /// counts, and a test sends them with its own clock (#328), which
    /// `cx.now()` would replace with the wall clock.
    ControlFrame(Instant),
    /// The window was searched for a `locate` command.
    Located {
        call: control::Call,
        target: String,
        found: control::Found,
        then: control::AfterSearch,
    },
    /// The window was captured for a `screenshot` command: save it, all
    /// of it or the part inside `area`, and answer.
    Captured {
        call: control::Call,
        path: std::path::PathBuf,
        area: Option<loom_ui_core::Rect>,
        image: Result<loom_ui::effect::Image, loom_ui::effect::NoGpu>,
    },
    /// Whether a text field has focus, for a `type` command.
    TypeFocus {
        call: control::Call,
        text: String,
        focused: bool,
    },
    Animate(bool),
    /// A frame while `--measure` runs, a deadline at display rate
    /// (`timers.rs`). It carries the time it was due, not the turn's
    /// `cx.now()`, so a test can step the frame meter by its own clock.
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
    /// MIDI learn, and the rows of a parameter's MIDI mapping.
    Learn(learn::Message),
    /// A control in the inspector, or a command that sets a parameter.
    Inspector(inspector::Message),
    /// A change to a modulation assignment.
    Modulation(modulation::Message),
    /// Time to check whether a recovery copy is due.
    Autosave,
    /// Time to check whether a ghost mark has run out.
    MidiGhosts,
    /// A MIDI control change arrived.
    Midi,
    /// A key: the undo and redo shortcuts, and Escape in learn mode. Ignored
    /// while the module browser is open.
    Key(iced::keyboard::Event),
    /// Take back the last step (`docs/design/undo.md`).
    Undo,
    /// A shortcut that acts on the patch (undo, redo, or one on the
    /// selection), after checking whether a text field has focus. While
    /// one has, the shortcut does nothing: the field is being edited, and
    /// the shortcut would change the patch behind it (`shortcut.rs`).
    Shortcut {
        shortcut: shortcut::Shortcut,
        typing: bool,
    },
    /// Replay the step undo took back.
    Redo,
    /// Something may have moved text focus, such as a press. The bridge asks
    /// the widgets after it.
    TextFocusCheck,
    /// The part with focus, or none, as the window says (`iced_bridge.rs`).
    /// Every answer is sent, changed or not, as the window's word on focus
    /// after a press: a press elsewhere ends a draft whose field never had
    /// focus, which a change alone would miss.
    FocusMoved(Option<loom_ui_core::UiId>),
    /// A runtime text field's paste could not read the clipboard: said in
    /// the status line, never shown as an empty clipboard (ui-layer.md,
    /// section 8, rule 2).
    FieldPasteRefused,
    /// Tab was pressed. It closes a text field on the main view.
    TextFieldTab {
        backwards: bool,
    },
    /// Moves the runtime's focus to a toolbar button, or with `None` out of
    /// the toolbar and back to the canvas: the toolbar's arrow keys and
    /// Escape (ui-layer.md, section 6, rule 7). Posted for a turn of its
    /// own, after every other message the same key sent.
    ToolbarFocus(Option<loom_ui_core::UiId>),
    /// Tab or Shift+Tab on an item of the probe menu or Recent: the menu
    /// closes, and the focus moves on from what opened it (the WAI-ARIA
    /// menu pattern; row 3b).
    MenuTab {
        back: bool,
    },
    /// Tab or Shift+Tab from a script inside help or the licence chooser:
    /// the focus moves to that overlay's next stop, or the one before,
    /// wrapping at both ends, as the runtime's own Tab does (the WAI-ARIA
    /// modal dialog pattern; rows 6 and 3c). Only until scripted keys take
    /// the runtime's path (lane C's 1b).
    OverlayTab {
        back: bool,
    },
    /// An arrow key from a script on an answer of the licence chooser: the
    /// next answer, or the one before, takes the focus and is chosen, as
    /// the runtime's radio keys do for a real key (row 3c). Only until
    /// scripted keys take the runtime's path (lane C's 1b).
    ChooserArrow {
        back: bool,
    },
    /// One step of the runtime's focus, after a menu closed and gave back
    /// the focus it opened over (row 3b).
    FocusStep {
        back: bool,
    },
    /// Tab between a view's fields, from the part the window says has
    /// focus (`window_question.rs`).
    TabFrom {
        from: Option<loom_ui_core::UiId>,
        backwards: bool,
    },
    /// Tab reached patch info's fields from one of the runtime's stops: the
    /// last field takes focus, or the first when not `backwards`
    /// (ui-layer.md, section 6, rule 3, from row 2).
    EnterFields {
        backwards: bool,
    },
    /// An inspector slider was let go.
    SliderReleased,
    /// A change in the session-only modulation matrix.
    Matrix(MatrixEvent),
    /// A key handled by the focused matrix even if a widget used it.
    MatrixKey(iced::keyboard::Event),
    /// A message for the Preferences window (`docs/design/preferences.md`).
    Preferences(settings::Message),
    /// Focuses a row of the patch outline.
    OutlineRow(loom_ui_core::UiId),
    /// Takes one of the offered cable jumps.
    JumpChoice(usize),
    /// A pointer press outside the cable jump list.
    DismissJump,
    /// An edit in patch info (`docs/design/patch-metadata.md`).
    PatchInfo(patch_info::Message),
    /// A message for the open licence chooser.
    Licence(licence_chooser::Message),
    /// A message for the open patches browser.
    Patches(patches::Message),
    /// An answer or a key for the open question (`docs/design/files.md`).
    Question(question::Message),
    /// The toolbar's **Recent** button.
    OpenRecent,
    /// A message for the open recent list.
    Recent(recent::Message),
    /// A file was dropped on the window. The window sends one per file.
    FileDropped(PathBuf),
    /// The moment for collecting a drop is over.
    DropsDone,
    /// The clipboard was read for a paste at a world point, not yet snapped
    /// (`docs/design/clipboard.md`).
    /// `generation` is the engine's when the read began: a result that
    /// arrives after another patch opened is dropped.
    Pasted {
        world: Point,
        text: loom_ui::ClipboardText,
        generation: u64,
    },
    /// The accessibility tree for screen readers (`a11y.rs`).
    A11y(a11y::Event),
    /// A native dialog's answer, for what waits on it (`dialogs.rs`).
    Dialog(dialogs::DialogAnswer),
    /// Ask the crash recovery question, once the window is open.
    AskRecovery,
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
                | Self::Inspector(inspector::Message::UnknownLabelDraft { .. })
                | Self::TextFocusCheck
                | Self::FocusMoved(_)
                | Self::AskRecovery
                | Self::ControlReply
                | Self::StartWatching
                | Self::RetryCommit(_)
                | Self::WindowRescaled
                // Only says a refused paste in the status line.
                | Self::FieldPasteRefused
        )
    }

    /// The view whose text field sent this message, for a message only a
    /// focused text field sends (its typing, or Enter in it): the main
    /// view's inspector and patch info (`InputOwner::Canvas`), Preferences,
    /// Your patches, the module browser, or the licence chooser. `None` for
    /// every other message.
    ///
    /// A text field keeps its focus when an overlay opens over it without a
    /// left press (a right-click that opens the probe menu, a screen
    /// reader's Click on Choose), and typing then reached it: Enter even
    /// committed a value into the patch under the probe menu (found
    /// 2026-11-20). [`App::apply_at`] drops such a message unless its view
    /// owns the input, so a covered field changes nothing. Row 3 of the
    /// plan for leaving Iced moves the focus into the overlay, and keeps
    /// this as the second guard.
    ///
    /// Each view's own messages are matched in full, so a field message
    /// added there must choose a side.
    fn field_view(&self) -> Option<input_owner::InputOwner> {
        use input_owner::InputOwner;
        let from = |field: bool, view: InputOwner| field.then_some(view);
        match self {
            Self::Inspector(message) => {
                use inspector::Message as M;
                let field = match message {
                    M::ValueText { .. }
                    | M::ValueSubmit { .. }
                    | M::UnknownLabelDraft { .. }
                    | M::UnknownLabelApply { .. }
                    | M::SubPatchNameDraft { .. }
                    | M::SubPatchNameApply { .. } => true,
                    M::Param { .. }
                    | M::ParamValue { .. }
                    | M::ValueDraftClear
                    | M::Choice { .. }
                    | M::Range { .. }
                    | M::SelectModule(_)
                    | M::DropRow(_)
                    | M::Expose { .. }
                    | M::Unexpose { .. }
                    | M::Scrolled => false,
                };
                from(field, InputOwner::Canvas)
            }
            Self::PatchInfo(message) => {
                use patch_info::Message as M;
                let field = match message {
                    M::Authors(_) | M::PartAuthors(..) | M::TagDraft(_) | M::TagSubmit => true,
                    M::RemoveTag(_) | M::ChooseLicence => false,
                };
                from(field, InputOwner::Canvas)
            }
            Self::Preferences(message) => {
                use settings::Message as M;
                let field = match message {
                    M::Author(_) | M::SaveAuthor => true,
                    M::ClockSource(_)
                    | M::SendClock(..)
                    | M::Open
                    | M::Close
                    | M::Refresh
                    | M::Safety(_)
                    | M::Activity(_)
                    | M::Map(_)
                    | M::Audio(_)
                    | M::Input(_)
                    | M::Midi(..)
                    | M::PanicLearn
                    | M::PanicClear
                    | M::ChooseLicence => false,
                };
                from(field, InputOwner::Preferences)
            }
            Self::Patches(message) => {
                use patches::Message as M;
                let field = match message {
                    M::Search(_) | M::Author(_) | M::Tag(_) | M::Submit => true,
                    M::Found { .. } | M::Licence(..) | M::Hover(_) | M::Pick(_) | M::Step(_) | M::Close => false,
                };
                from(field, InputOwner::Patches)
            }
            Self::Browser(message) => {
                use browser::Message as M;
                let field = match message {
                    M::Query(_) | M::Submit => true,
                    M::Hover(_)
                    | M::Pick(_)
                    | M::Menu { .. }
                    | M::InsertLinked(_)
                    | M::Library(_)
                    | M::Scrolled(_)
                    | M::Key(_)
                    | M::Close => false,
                };
                from(field, InputOwner::Browser)
            }
            Self::Licence(message) => {
                use licence_chooser::Message as M;
                let field = match message {
                    M::OtherText(_) | M::Use => true,
                    M::Answer(_) | M::Commercial(_) | M::Clear | M::Cancel => false,
                };
                from(field, InputOwner::Chooser)
            }
            // No other message comes from a view's text field.
            _ => None,
        }
    }

    /// True for what still runs while a question is open: the question
    /// itself, and everything that keeps the music going and the screen
    /// honest. A hardware knob keeps working, and a stray click or key does
    /// not reach the patch (files.md, Asking before losing work).
    ///
    /// No wildcard, on purpose: a new message must choose a side.
    fn allowed_during_question(&self) -> bool {
        match self {
            // A screen reader action becomes ordinary messages, each checked on
            // its own, so a Click on one of the question's answers is heard and
            // one on anything below it waits, as a press would.
            Self::A11y(_) | Self::Question(_)
            // Never asked: `update` routes it before this check, and each
            // message it becomes is checked on its own.
            | Self::Input { .. }
            | Self::Panic
            | Self::MidiPoll
            | Self::AudioPoll
            | Self::InputAudio(_)
            | Self::BenchDone
            | Self::Control(_)
            | Self::ControlReply
            | Self::StartWatching
            | Self::RetryCommit(_)
            | Self::WindowRescaled
            // It only says a refused paste in the status line.
            | Self::FieldPasteRefused
            | Self::ControlFrame(_)
            | Self::Located { .. }
            | Self::Captured { .. }
            | Self::TypeFocus { .. }
            | Self::Frame(_)
            | Self::Tick
            | Self::Audio(_)
            | Self::LiveFrame
            | Self::Lights(_)
            | Self::Meters(_)
            | Self::Telemetry(_)
            | Self::Autosave
            | Self::Midi
            // Where focus went: the window has already moved it.
            | Self::FocusMoved(_)
            // A native dialog's answer: what it was asked for is already
            // decided, as a question's answer is.
            | Self::Dialog(_)
            | Self::AskRecovery
            // These say they were ignored, so they must arrive.
            | Self::FileDropped(_)
            | Self::DropsDone => true,
            // The canvas ends what it was doing when it loses input. Those
            // must land, or a button stays held behind the question.
            Self::Canvas(event) => matches!(
                event,
                CanvasEvent::ButtonReleased { .. }
                    | CanvasEvent::GestureEnd
                    | CanvasEvent::EditGroupEnd
                    | CanvasEvent::AssignCancel
            ),
            // A close asks over the browser, and Cancel returns to it: rows
            // read meanwhile must not go missing.
            Self::Patches(message) => matches!(message, patches::Message::Found { .. }),
            Self::Key(_)
            | Self::MidiGhosts
            | Self::OpenBrowser
            | Self::CablePort { .. }
            | Self::ShowHelp
            | Self::ShowShortcuts
            | Self::ShowModuleHelp(_)
            | Self::ResetClip(_)
            | Self::CloseHelp
            | Self::HelpExample
            | Self::HelpLink(_)
            | Self::CloseTileMenu
            | Self::OpenLevel(_)
            | Self::OpenTile(_)
            | Self::MakeSubPatch(_)
            | Self::DissolveSubPatch(_)
            | Self::SaveToLibrary(_)
            | Self::Unlink(_)
            | Self::MakeSlot { .. }
            | Self::SwapSlot { .. }
            | Self::DissolveSlot(_)
            | Self::Mode(_)
            | Self::Browser(_)
            | Self::ProbeMenu(_)
            | Self::AddProbe { .. }
            | Self::HoldNote
            | Self::File(_)
            | Self::Measure(_)
            | Self::Animate(_)
            | Self::Learn(_)
            | Self::Inspector(_)
            | Self::Modulation(_)
            | Self::Undo
            | Self::Shortcut { .. }
            | Self::Redo
            | Self::TextFocusCheck
            | Self::TextFieldTab { .. }
            | Self::TabFrom { .. }
            | Self::EnterFields { .. }
            | Self::ToolbarFocus(_)
            | Self::MenuTab { .. }
            | Self::OverlayTab { .. }
            | Self::ChooserArrow { .. }
            | Self::FocusStep { .. }
            | Self::SliderReleased
            | Self::Matrix(_)
            | Self::MatrixKey(_)
            | Self::Preferences(_)
            | Self::OutlineRow(_)
            | Self::JumpChoice(_)
            | Self::DismissJump
            | Self::PatchInfo(_)
            | Self::Licence(_)
            | Self::OpenRecent
            | Self::Recent(_)
            | Self::DismissError(_) => false,
            // A refused read says so even under a question, since a read
            // that began before the question opened can end during it, and
            // a refusal must never pass silently; it changes no patch. A read
            // with text waits out the question, as other edits do.
            Self::Pasted { text, .. } => *text == loom_ui::ClipboardText::Refused,
        }
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
    /// Open your patches browser (`docs/design/patch-metadata.md`).
    Browse,
    /// Save the sub-patch whose inside the canvas shows to the library
    /// (`docs/design/sub-patches.md`, Library).
    SaveToLibrary,
    /// The window asked to close: quit, unless the player keeps the patch.
    Close,
}

struct App {
    /// The open patch. New and Open replace it whole.
    session: session::PatchSession,
    /// The recovery folder and the examples folder, for the whole run.
    run_files: document::RunFiles,
    /// Which tab the right panel shows. It stays when the patch changes.
    right_tab: RightTab,
    /// The part with keyboard focus, as `FocusMoved` last said. While a text
    /// field has it, the canvas leaves the keys to it. A copy for what reads
    /// focus outside `update` (the view, the input rows, the accessibility
    /// tree); `update` checks it equals `cx.focused()`.
    focus: Option<loom_ui_core::UiId>,
    /// Set when a message may have moved focus: the bridge then asks the
    /// window where it is, after the turn (`App::take_focus_question`).
    focus_question: std::cell::Cell<bool>,
    /// Where the canvas widget leaves the frame it builds while the view is
    /// drawn, for the window's canvas pass (`iced_bridge/paint.rs`). Each
    /// draw empties it first and takes the frame after, so it holds nothing
    /// between frames.
    pub(crate) canvas_drawn: loom_canvas::CanvasSlot,
    /// Counts the engines built. Background watchers restart when it changes.
    generation: u64,
    /// The `test.` modules are loaded (`--test-modules`).
    test_modules: bool,
    /// Set when the app should exit after this update.
    quit: bool,
    theme: Theme,
    /// What belongs to the machine: preferences, devices, MIDI inputs.
    rig: settings::Rig,
    /// The status line, the stats bar's warnings, and problems reported once.
    alerts: alerts::Alerts,
    /// Every registered module, for the browser.
    modules: Vec<&'static ModuleDescriptor>,
    /// The library folder's items, read once when a session starts, when
    /// the browser reads the folder, after Save to library, and when the
    /// folder preference changes, so a slot's Swap list never reads the
    /// disk while the screen draws (sub-patches.md, Slots).
    library: Vec<browser::LibraryItem>,
    /// What is open over the canvas.
    overlays: overlays::Overlays,
    /// Files of a drop being collected, for a moment after the first.
    drops: Vec<PathBuf>,
    /// Counts openings of the patches browser, so files read for an
    /// earlier one are dropped.
    patches_open: u64,
    /// Changed by the toolbar button; the canvas then opens the browser at
    /// its centre.
    browser_key: u64,
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
    measuring: bool,
    animating: bool,
    /// The status bar shows the developer tools: `--dev`, or a measuring
    /// flag (visual-system.md, Status bar).
    dev_tools: bool,
    /// How the canvas starts, for a still image of the window
    /// (`--screenshot-window`). `None` in the app.
    still: Option<loom_canvas::Still>,
    /// A shortcut on the patch waiting for the check that no text field
    /// has focus (`shortcut.rs`).
    pending_shortcut: Option<shortcut::Shortcut>,
    /// The accessibility tree's node IDs and whether a screen reader
    /// listens. It belongs to the window, so it stays when the patch changes.
    a11y: a11y::Access,
    /// The effects this update returns, from code deep in a call chain
    /// (`App::effect`). Empty between updates.
    effects: loom_ui::Effects<Message>,
    /// Messages for the next turn, each its own update, in order
    /// (`App::post`). Returned at the end of `update` as one next-turn
    /// effect (`Effects::next_turn`).
    posted: Vec<Message>,
    /// Where the app reads the time: the system clock, or the test
    /// harness's stepped clock (`clock.rs`, #868).
    clock: clock::Clock,
    /// What only the window's widgets can answer, for the bridge to ask
    /// after the turn (`window_question.rs`). Interior, so a call that only
    /// reads the app can ask.
    window_questions: std::cell::RefCell<Vec<window_question::WindowQuestion>>,
    /// Control commands whose UI messages are on their way (`control/run.rs`).
    control_replies: control::Replies,
    /// True once the window is open and watchers and timers may run
    /// (`watching.rs`, `timers.rs`).
    watching: bool,
    /// When each timer is next due (`timers.rs`).
    timers: timers::Timers,
    /// True while a `ControlFrame` waits for a frame to be shown, so a
    /// waiting command asks for one at a time.
    control_frame_asked: bool,
    /// The window's scale factor as last read from an update's context, for
    /// drawn display sizes and accessibility bounds in physical pixels.
    window_scale: f32,
    /// The watcher threads that outlive a patch (`watching.rs`).
    watchers: watching::AppWatchers,
    /// What waits for a native dialog's answer (`dialogs.rs`). It belongs
    /// to the window: a dialog's answer can open another patch.
    waiting: Option<dialogs::Pending>,
    /// A crash copy found at start, until the window is open to offer it.
    recovery_offer: Option<dialogs::RecoveryOffer>,
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
struct Loaded {
    controller: Controller,
    /// The sub-patch hierarchy read with the patch (ADR-0039).
    hierarchy: loom_patch::Hierarchy,
    engine: loom_core::Engine,
    midi: loom_core::MidiSender,
    telemetry: loom_core::Telemetry,
    positions: BTreeMap<u32, (f32, f32)>,
    /// Original and displayed placeholder positions for lossless saving.
    unknown_positions: session::UnknownPositions,
    /// Tile dimensions carried with the patch between sessions.
    sizes: BTreeMap<u32, (u8, u8)>,
    /// The patch's name and metadata (patch-metadata.md).
    info: loom_patch::Metadata,
    /// True for a new, empty patch, which takes the user's defaults.
    fresh: bool,
    file: Option<PathBuf>,
    stamp: Option<recovery::Stamp>,
    /// What loaded differently from the file, such as a saved control
    /// range that was not kept (control-ranges.md, rule 7).
    notes: Vec<loom_patch::LoadWarning>,
}

impl App {
    /// Each tile's layout as the file writes it, by flat id.
    pub(crate) fn layout_fn(&self) -> impl Fn(u32) -> loom_patch::Layout + '_ {
        let session = &self.session;
        move |id: u32| {
            let shown = session.positions.get(&id);
            let position = match session.unknown_positions.get(&id) {
                // A placeholder nobody moved keeps the position from its
                // file, exactly (unknown-modules.md, rule 1).
                Some((original, initial)) if shown == Some(initial) => *original,
                _ => shown.map(|&(x, y)| (x.round(), y.round())),
            };
            loom_patch::Layout {
                position,
                size: session.sizes.get(&id).copied(),
            }
        }
    }

    /// The patch as it would be saved now.
    fn current_patch(&self) -> Patch {
        let session = &self.session;
        // Through the hierarchy, so the sub-patches are written as the file
        // holds them (ADR-0039, invariant 5).
        let mut patch = session
            .hierarchy
            .unflatten(session.controller.graph(), self.layout_fn());
        patch.metadata = session.document.info().clone();
        patch
    }

    /// Writes or deletes the recovery copy when one is due
    /// (`docs/design/autosave.md`). Runs on a timer and after save, open,
    /// and new.
    fn autosave(&mut self, now: Instant) {
        if !self.run_files.autosave_on() {
            return;
        }
        let current = self.current_patch();
        for report in self.run_files.autosave(&mut self.session.document, now, current) {
            self.autosave_report(report);
        }
    }

    /// Tells the player what autosave reported. A failure stays on screen as
    /// a warning until autosave works again: the safety net is gone, but
    /// nothing else is wrong, so it never interrupts playing.
    pub(crate) fn autosave_report(&mut self, report: recovery::Report) {
        match report {
            recovery::Report::Failed(line) => self.alerts.warn_kept(alerts::Cause::Autosave, line),
            recovery::Report::Works => {
                self.alerts.clear(alerts::Cause::Autosave);
                self.alerts.say(recovery::AUTOSAVE_WORKS);
            }
            recovery::Report::Note(line) => self.alerts.warn(line),
        }
    }

    /// True once after a message that may have moved focus: the bridge then
    /// asks the window where focus is.
    pub(crate) fn take_focus_question(&self) -> bool {
        self.focus_question.replace(false)
    }

    /// True while the window waits to be asked where focus is, for a harness
    /// that runs a turn so the bridge asks.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn focus_question_waits(&self) -> bool {
        self.focus_question.get()
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
        if let Some(engine) = self.session.engine.stream.idle_mut() {
            engine.apply_pending();
        }
        // What the engine handed back is freed here, after any message,
        // rather than on a timer that would wake an idle app (#90).
        self.session.controller.collect_garbage();
    }

    /// Runs the engine for `frames` frames on silent buffers, as an audio
    /// device would. For tests and `--frames`: the window never runs the
    /// idle engine. Runs in chunks, so a long run needs no long buffers;
    /// the engine gives the same output at any block size.
    fn process(&mut self, frames: usize) {
        const CHUNK: usize = 4096;
        let engine = self
            .session
            .engine
            .stream
            .idle_mut()
            .expect("an audio device plays the engine");
        let (mut left, mut right) = (vec![0.0; CHUNK], vec![0.0; CHUNK]);
        let mut remaining = frames;
        while remaining > 0 {
            let n = remaining.min(CHUNK);
            engine.process(&mut left[..n], &mut right[..n]);
            remaining -= n;
        }
        // What a tick would do after the engine ran.
        self.finish_due_fades();
    }

    /// Rebuilds the scene after the graph changed.
    fn refresh(&mut self) {
        self.refresh_scene();
        self.refresh_patch_map();
    }

    /// Rebuilds the scene and live values without rebuilding the navigation structure.
    fn refresh_scene(&mut self) {
        // A module added straight to the controller is a top-level module
        // the hierarchy must know (ADR-0039, invariant 1).
        self.session.hierarchy.adopt(self.session.controller.graph());
        layout::auto_layout(&self.session.controller, &mut self.session.positions);
        self.session.scene = scene::build_with_clock(
            &self.session.controller,
            &self.session.hierarchy,
            &self.session.level,
            &self.session.positions,
            &self.session.sizes,
            self.rig.midi.following_clock.as_deref(),
        );
        self.session.mode_key = scene::mode_key(&self.session.controller);
        self.refresh_values();
        self.refresh_learn();
        self.session.matrix.reconcile(self.session.controller.graph());
    }

    /// Rebuilds the control, light, and meter values. Only changed values
    /// change what the canvas uploads.
    fn refresh_values(&mut self) {
        // A mode change shows or hides controls, which changes the panel
        // (ADR-0030). Rare: a mode click, never a knob turn.
        let mode_key = scene::mode_key(&self.session.controller);
        if mode_key != self.session.mode_key {
            self.session.scene = scene::build_with_clock(
                &self.session.controller,
                &self.session.hierarchy,
                &self.session.level,
                &self.session.positions,
                &self.session.sizes,
                self.rig.midi.following_clock.as_deref(),
            );
            self.session.mode_key = mode_key;
        }
        let mut shown: scene::ShownValues = self
            .session
            .lights
            .iter()
            .map(|(&light, &level)| (light, (level, 0.0, None)))
            .collect();
        let modules: Vec<u32> = self.session.controller.graph().nodes().map(|(id, _)| id.0).collect();
        for (widget, output) in scene::meter_outputs(&self.session.controller, &modules) {
            let level = self
                .session
                .engine
                .live
                .probe(output)
                .and_then(|probe| self.session.meter_levels.get(&probe))
                .copied()
                .unwrap_or_default();
            shown.insert(widget, (level.rms, level.peak, level.right));
        }
        let dragging = self.session.gestures.depth_target();
        scene::update_panel_values_dragging(
            &mut self.session.values,
            &self.session.controller,
            &self.session.hierarchy,
            &shown,
            &self.session.clip_holds,
            dragging,
        );
        scene::set_ghosts(
            &mut self.session.values,
            &self.session.controller,
            &self.session.hierarchy,
            self.session.learn.ghosts().shown(),
        );
        // A scope's window or trigger may have changed.
        self.publish_watch();
    }

    /// Takes the latest dropout count for the stats bar warning. Runs on
    /// every tick and, in Measure mode, on every frame.
    fn update_dropouts(&mut self, now: Instant) {
        if let Some(stats) = self.session.engine.stream.stats() {
            let s = stats.snapshot();
            self.alerts.note_dropouts(s.overruns + s.stream_errors, now);
        }
        // A recording that stopped early (a full disk) is reported once.
        if let Some(error) = self
            .rig
            .output
            .recorder
            .as_ref()
            .and_then(loom_host_desktop::Recorder::error)
            && self.alerts.recording_stopped()
        {
            self.alerts
                .error(alerts::Cause::Other, format!("The recording stopped: {error}"));
            eprintln!("loom-app: {}", self.alerts.status());
        }
    }

    /// The outputs of the meters and level histories on screen: each needs a
    /// meter probe.
    fn metered_outputs(&self) -> BTreeSet<OutputRef> {
        scene::meter_outputs(&self.session.controller, &self.session.visible_displays)
            .into_iter()
            .map(|(_, output)| output)
            .chain(
                scene::history_outputs(&self.session.controller, &self.session.visible_displays)
                    .into_iter()
                    .map(|history| history.output),
            )
            .collect()
    }

    /// Brings the meter and scope probes in line with the graph and the
    /// displays on screen, and tells the telemetry thread what to read.
    /// Call it before a commit.
    fn sync_live(&mut self) {
        let graph = self.session.controller.graph();
        self.session
            .clip_holds
            .retain(|module, _| graph.node(ModuleId(module)).is_some());
        let wanted = self.metered_outputs();
        self.session.engine.live.sync(&mut self.session.controller, &wanted);
        let scopes: BTreeSet<OutputRef> =
            scene::scope_outputs(&self.session.controller, &self.session.visible_displays)
                .into_iter()
                .map(|scope| scope.output)
                .chain(
                    scene::spectrum_outputs(&self.session.controller, &self.session.visible_displays)
                        .into_iter()
                        .map(|spectrum| spectrum.output),
                )
                .collect();
        self.session
            .engine
            .live
            .sync_scopes(&mut self.session.controller, &scopes);
        let pairs: Vec<_> = scene::xy_outputs(&self.session.controller, &self.session.visible_displays)
            .into_iter()
            .map(|scope| (scope.x, scope.y, scope.y_input))
            .collect();
        self.session
            .engine
            .live
            .sync_pairs(&mut self.session.controller, &pairs);
        self.publish_watch();
    }

    /// Tells the telemetry thread which meters and scopes are drawn, how
    /// each scope triggers, and the size it is drawn at. Runs after probes
    /// or scope parameters change, after a camera change, a new window
    /// scale, and a scene change such as a tile resize (through
    /// `refresh_values`). An equal list wakes nothing.
    fn publish_watch(&mut self) {
        let meters = self
            .metered_outputs()
            .into_iter()
            .filter_map(|output| self.session.engine.live.probe(output))
            .collect();
        let rate = self.session.engine.monitor.sample_rate();
        let mut scopes = std::collections::HashMap::<ProbeId, monitor::ScopeWant>::new();
        for scope in scene::scope_outputs(&self.session.controller, &self.session.visible_displays) {
            let Some(probe) = self.session.engine.live.scope_probe(scope.output) else {
                continue;
            };
            let drawn = self.scope_drawn_size(scope.widget);
            scopes
                .entry(probe)
                // Two scopes on one probe share a want: each side the
                // larger, so neither lags by more than half a pixel.
                .and_modify(|want| want.drawn = want.drawn.zip(drawn).map(|(a, b)| a.max(b)))
                .or_insert(monitor::ScopeWant {
                    window: (scope.window * rate).round().max(1.0) as usize,
                    trigger: scope.trigger,
                    slope: scope.slope,
                    drawn,
                });
        }
        let histories = self.trace_wants(
            scene::history_outputs(&self.session.controller, &self.session.visible_displays)
                .into_iter()
                .filter_map(|history| {
                    Some((
                        self.session.engine.live.probe(history.output)?,
                        history.widget,
                        history.window,
                    ))
                }),
        );
        let spectra = self.trace_wants(
            scene::spectrum_outputs(&self.session.controller, &self.session.visible_displays)
                .into_iter()
                .filter_map(|spectrum| {
                    let settings = monitor::SpectrumWant {
                        tilt: spectrum.tilt,
                        hold: spectrum.hold,
                    };
                    Some((
                        self.session.engine.live.scope_probe(spectrum.output)?,
                        spectrum.widget,
                        settings,
                    ))
                }),
        );
        let xy = self.trace_wants(
            scene::xy_outputs(&self.session.controller, &self.session.visible_displays)
                .into_iter()
                .filter_map(|scope| {
                    let settings = monitor::XyWant {
                        frames: (scope.time * rate).round().max(1.0) as usize,
                        range: scope.range,
                        rotate: scope.rotate,
                    };
                    Some((
                        self.session.engine.live.pair_probe(scope.x, scope.y)?,
                        scope.widget,
                        settings,
                    ))
                }),
        );
        let activity = self.session.engine.live.activity_probes().collect();
        self.session
            .carrying
            .retain(|output| self.session.engine.live.probe(*output).is_some());
        let list = monitor::WatchList {
            meters,
            activity,
            scopes,
            histories,
            spectra,
            xy,
            peak_resets: self.peak_resets(),
            full_traces: self.session.full_traces,
        };
        // An equal list changes nothing, so the thread is not woken.
        self.session.engine.monitor.watch(list);
        // A display that left the screen, or lost its probe, keeps no trace.
        let shown: BTreeSet<(u32, usize)> =
            scene::scope_outputs(&self.session.controller, &self.session.visible_displays)
                .into_iter()
                .filter(|scope| self.session.engine.live.scope_probe(scope.output).is_some())
                .map(|scope| scope.widget)
                .chain(
                    scene::history_outputs(&self.session.controller, &self.session.visible_displays)
                        .into_iter()
                        .filter(|history| self.session.engine.live.probe(history.output).is_some())
                        .map(|history| history.widget),
                )
                .chain(
                    scene::spectrum_outputs(&self.session.controller, &self.session.visible_displays)
                        .into_iter()
                        .filter(|spectrum| self.session.engine.live.scope_probe(spectrum.output).is_some())
                        .map(|spectrum| spectrum.widget),
                )
                .chain(
                    scene::xy_outputs(&self.session.controller, &self.session.visible_displays)
                        .into_iter()
                        .filter(|scope| self.session.engine.live.pair_probe(scope.x, scope.y).is_some())
                        .map(|scope| scope.widget),
                )
                .collect();
        self.session
            .traces
            .retain(|module, widget| shown.contains(&(module, widget)));
    }

    /// A scope widget's size as drawn, in physical pixels: its bounds on the
    /// tile, times the camera's zoom, times the window's scale
    /// (`docs/design/panels.md`, Drawing the scope trace, The drawn size).
    /// `None` before the canvas has reported a camera for this level, or for
    /// a widget not drawn.
    fn scope_drawn_size(&self, (module, widget): (u32, usize)) -> Option<monitor::DrawnSize> {
        let zoom = self.session.cameras.get(&self.session.level)?.zoom;
        let bounds = self.session.scene.module(module)?.widget_bounds(widget)?;
        let scale = zoom * self.window_scale;
        Some(monitor::DrawnSize::new(bounds.width * scale, bounds.height * scale))
    }

    /// Builds wants for displays whose wake rule uses the data area inside
    /// the frame. A shared probe uses the largest size on each axis.
    fn trace_wants<T>(
        &self,
        displays: impl Iterator<Item = (ProbeId, (u32, usize), T)>,
    ) -> std::collections::HashMap<ProbeId, monitor::TraceWant<T>> {
        let mut wants = std::collections::HashMap::<_, monitor::TraceWant<T>>::new();
        let zoom = self.session.cameras.get(&self.session.level).map(|camera| camera.zoom);
        for (probe, (module, index), settings) in displays {
            let drawn = zoom.and_then(|zoom| {
                let tile = self.session.scene.module(module)?;
                let bounds = tile.widget_bounds(index)?;
                let area = loom_canvas::trace_draw_area(bounds, &tile.panel.widgets.get(index)?.kind);
                let scale = zoom * self.window_scale;
                Some(monitor::DrawnSize::new(area.width * scale, area.height * scale))
            });
            let drawn = wants.get(&probe).map_or(drawn, |previous| {
                previous.drawn.zip(drawn).map(|(previous, drawn)| previous.max(drawn))
            });
            wants.insert(probe, monitor::TraceWant { settings, drawn });
        }
        wants
    }

    /// Gives each new trace from the telemetry thread to the widgets that show
    /// its probe. `shown` is each widget with its probe, if it has one.
    fn give_traces(&mut self, traces: Vec<(ProbeId, loom_canvas::Trace)>, shown: &[((u32, usize), Option<ProbeId>)]) {
        for (probe, trace) in traces {
            for &((module, widget), showing) in shown {
                if showing == Some(probe) {
                    self.session.traces.set(module, widget, trace.clone());
                }
            }
        }
    }

    /// Takes new scope traces from the telemetry thread.
    fn take_traces(&mut self, traces: Vec<(ProbeId, loom_canvas::Trace)>) {
        if traces.is_empty() {
            return;
        }
        let shown: Vec<_> = scene::scope_outputs(&self.session.controller, &self.session.visible_displays)
            .iter()
            .map(|scope| (scope.widget, self.session.engine.live.scope_probe(scope.output)))
            .collect();
        self.give_traces(traces, &shown);
    }

    /// Each meter on screen with the probe on its output and its own reset
    /// count. Two meters on one output share the probe and keep their own
    /// counts, so a reset of one clears only its news
    /// (`docs/design/panels.md`, Meter widget, Two meters on one output).
    fn peak_resets(&self) -> std::collections::HashMap<clip::MeterWidget, (ProbeId, u64)> {
        scene::meter_outputs(&self.session.controller, &self.session.visible_displays)
            .into_iter()
            .filter_map(|(meter, output)| {
                let probe = self.session.engine.live.probe(output)?;
                Some((meter, (probe, self.session.clip_holds.resets(meter))))
            })
            .collect()
    }

    /// Takes raw peaks from the telemetry thread into the clip lights and peak
    /// holds of the meters they are for. News from before its own meter's
    /// latest reset is dropped, so a reset is not undone by a message that
    /// was already on its way. So is news from a probe the meter no longer
    /// has, and news for a meter no longer on screen. Another meter's news
    /// still counts, even on the same output.
    fn take_peaks(&mut self, peaks: Vec<(clip::MeterWidget, clip::PeakNews)>) {
        if peaks.is_empty() {
            return;
        }
        let resets = self.peak_resets();
        let mut changed = false;
        for (meter, news) in peaks {
            if resets
                .get(&meter)
                .is_some_and(|&(probe, count)| news.probe == Some(probe) && count == news.after)
            {
                changed |= self.session.clip_holds.take(meter, news);
            }
        }
        if changed {
            self.refresh_values();
        }
    }

    /// Clears a meter's clip light, or also its peak hold, and tells the
    /// telemetry thread to report peaks again from here.
    pub(crate) fn reset_clip(&mut self, meter: (u32, usize), hold_too: bool) {
        if hold_too {
            self.session.clip_holds.reset(meter);
        } else {
            self.session.clip_holds.clear_light(meter);
        }
        self.publish_watch();
        self.refresh_values();
    }

    /// Takes new level histories from the telemetry thread.
    fn take_histories(&mut self, histories: Vec<(ProbeId, loom_canvas::Trace)>) {
        if histories.is_empty() {
            return;
        }
        let shown: Vec<_> = scene::history_outputs(&self.session.controller, &self.session.visible_displays)
            .iter()
            .map(|history| (history.widget, self.session.engine.live.probe(history.output)))
            .collect();
        self.give_traces(histories, &shown);
    }

    /// Takes new XY traces from the telemetry thread.
    fn take_xy(&mut self, traces: Vec<(ProbeId, loom_canvas::Trace)>) {
        if traces.is_empty() {
            return;
        }
        let shown: Vec<_> = scene::xy_outputs(&self.session.controller, &self.session.visible_displays)
            .iter()
            .map(|scope| (scope.widget, self.session.engine.live.pair_probe(scope.x, scope.y)))
            .collect();
        self.give_traces(traces, &shown);
    }

    /// Takes new spectra from the telemetry thread. A scope on the same probe
    /// keeps its trace.
    fn take_spectra(&mut self, spectra: Vec<(ProbeId, loom_canvas::Trace)>) {
        if spectra.is_empty() {
            return;
        }
        let shown: Vec<_> = scene::spectrum_outputs(&self.session.controller, &self.session.visible_displays)
            .iter()
            .map(|spectrum| (spectrum.widget, self.session.engine.live.scope_probe(spectrum.output)))
            .collect();
        self.give_traces(spectra, &shown);
    }

    /// Adds probes for meters that came on screen and removes those that
    /// left. Only probes change, so the scene is not rebuilt.
    fn sync_probes(&mut self) {
        self.sync_live();
        // Probe IDs are never reused, so a level of a removed probe is dead.
        let live: std::collections::HashSet<ProbeId> = self.session.engine.live.probes().collect();
        self.session.meter_levels.retain(|probe, _| live.contains(probe));
        self.commit_maintenance();
    }

    fn report(&mut self) {
        let report = self.session.controller.last_report();
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
            self.alerts.append_warning(&format!(
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
    /// A window event from the input listener never comes here: the bridge
    /// routes it first (`Listening::route`), and each message it becomes is
    /// its own update.
    ///
    /// One message, from the runtime (`iced_bridge.rs`), and its effects
    /// (ui-layer.md, section 2). Messages the app posts for the next turn
    /// (`App::post`) are returned as one next-turn effect, in order, once it
    /// has run; the runtime queues them itself.
    pub(crate) fn update(&mut self, message: Message, cx: &loom_ui::Context<'_, Message>) -> loom_ui::Effects<Message> {
        // The copy of focus follows the runtime's; `FocusMoved` is how it
        // catches up, so it alone may arrive with the two apart.
        if !matches!(message, Message::FocusMoved(_)) {
            debug_assert_eq!(self.focus.as_ref(), cx.focused(), "the app's copy of focus drifted");
        }
        // A shortcut waits for Iced's answer to which part has focus; the
        // runtime's own fields say so through the context (section 6, rule
        // 6, step 6).
        let message = match message {
            Message::Shortcut { shortcut, typing } => Message::Shortcut {
                shortcut,
                typing: typing || cx.field_has_focus(),
            },
            other => other,
        };
        // What reads the runtime's layout (where a part is for `locate`, the
        // nodes for the tree) waits for the draw that lays out a view a
        // live value just rebuilt, so it never reads a layout half made.
        // A `locate` that Iced or the canvas answers needs nothing of ours,
        // so it is answered at once, from a fresh search; only one whose
        // answer would come from the runtime's layout waits.
        let waits = match &message {
            Message::Located { target, found, .. } => crate::control::waits_for_ours(self, target, found),
            Message::A11y(crate::a11y::Event::Located(_) | crate::a11y::Event::LocateAfterLayout) => true,
            _ => false,
        };
        if waits && !cx.laid_out() {
            let message = if matches!(message, Message::A11y(crate::a11y::Event::Located(_))) {
                Message::A11y(crate::a11y::Event::LocateAfterLayout)
            } else {
                message
            };
            return loom_ui::Effects::after_draw(message);
        }
        // The runtime's nodes as last laid out, for the tree a rebuild
        // sends (`a11y/tree.rs`).
        if matches!(message, Message::A11y(crate::a11y::Event::Located(_))) {
            self.a11y_take_ours(|ids| cx.access_nodes(ids));
        }
        let mut message = message;
        // A pointer command aimed at one of the runtime's own elements is
        // aimed at its place, which only the context knows; it waits for the
        // draw that lays a rebuilt view out (section 10, rule 6).
        // It waits by the rule `locate` waits by, before asking for a place,
        // since text a rebuilt view shows has no place yet.
        if let Message::Control(call) = &mut message {
            let stale = !cx.laid_out();
            if stale
                && call
                    .command
                    .pointer_targets()
                    .iter()
                    .any(|target| crate::control::may_be_ours(target))
            {
                return loom_ui::Effects::after_draw(message);
            }
            call.command
                .place_targets(|target| crate::control::ours_for_pointer(target, cx));
        }
        // A target `locate` looks for may be one of the runtime's own
        // elements, which only the context can place (section 10, rule 6).
        if let Message::Located { target, found, .. } = &mut message {
            found.find_ours(target, cx);
        }
        debug_assert!(
            !matches!(message, Message::Input { .. }),
            "input is routed before update (iced_bridge.rs)"
        );
        let now = cx.now();
        self.timers.set_frame(cx.window().frame);
        // Display sizes and accessibility bounds share the runtime's scale.
        if cx.window().scale != self.window_scale {
            self.window_scale = cx.window().scale;
            self.publish_watch();
            self.a11y_rebuild();
        }
        if let Some(timer) = timers::Timer::of(&message) {
            self.timers.taken(timer, now);
        }
        if matches!(message, Message::ControlFrame(_)) {
            self.control_frame_asked = false;
        }
        let asked = self.overlays.question.is_some();
        self.apply_at(message, now);
        // A question that opened takes the focus to its safe answer, with a
        // ring (ui-layer.md, section 6, rule 4; plan 3.2, row 3b).
        if !asked && let Some(question) = &self.overlays.question {
            let answer = question.highlighted_id();
            self.effect(loom_ui::Effects::focus(answer));
        }
        self.sync_part_authors();
        // The timers, the watchers, and the frames a command waits for
        // follow what this update changed.
        self.sync_timers(now);
        self.sync_watchers(&cx.waker());
        if self.control_waiting() && !self.control_frame_asked {
            self.control_frame_asked = true;
            self.effect(loom_ui::Effects::after_draw(Message::ControlFrame(now)));
        }
        // As one batch for the next turn, which the runtime queues itself,
        // so no other thread's message lands between a command's steps and
        // its reply, or a step and its release.
        let posted = std::mem::take(&mut self.posted);
        self.effect(loom_ui::Effects::next_turn(posted));
        std::mem::take(&mut self.effects)
    }

    /// Queues `message` for the next turn, as its own update, after the
    /// messages posted before it (ui-layer.md, section 2, rule 5), through
    /// the next-turn effect `update` returns. For what
    /// the player does through another part: an open menu's Enter, a
    /// screen reader's action, a control command.
    pub(crate) fn post(&mut self, message: Message) {
        self.posted.push(message);
    }

    /// Applies one message. Its effects collect in `self.effects`, and the
    /// window questions it asks in `self.window_questions`.
    fn apply(&mut self, message: Message) {
        self.apply_at(message, self.clock.now());
    }

    /// Applies one message at the time given by the runtime.
    fn apply_at(&mut self, message: Message, now: Instant) {
        use loom_canvas::stats::FrameCause;
        // First, before anything can return: a watcher's message is taken.
        self.watched_message_taken(&message);
        let cause = match &message {
            Message::Tick => FrameCause::Tick,
            Message::LiveFrame => FrameCause::Live,
            Message::Meters(_) | Message::Telemetry(_) => FrameCause::Meters,
            Message::Lights(_) => FrameCause::Lights,
            Message::Control(_)
            | Message::ControlReply
            | Message::ControlFrame(_)
            | Message::Located { .. }
            | Message::Captured { .. }
            | Message::TypeFocus { .. } => FrameCause::Control,
            Message::Canvas(_) | Message::Key(_) | Message::File(FileAction::Close) | Message::WindowRescaled => {
                FrameCause::Input
            }
            Message::Audio(_) | Message::InputAudio(_) | Message::AudioPoll => FrameCause::Audio,
            Message::StartWatching | Message::BenchDone => FrameCause::Lifecycle,
            Message::Frame(_) => FrameCause::Forced,
            _ => FrameCause::Other,
        };
        loom_canvas::stats::request_frame(cause);
        if self.overlays.question.is_some() && !message.allowed_during_question() {
            return;
        }
        // A covered text field changes nothing (`Message::field_view`).
        if message.field_view().is_some_and(|view| view != self.overlays.owner()) {
            return;
        }
        // What this message means for a screen reader's tree.
        let touch = match (&message, cause) {
            (Message::A11y(_) | Message::WindowRescaled, _) => a11y::Touch::Own,
            // The audio watchers' readings: the load, the dropouts, and
            // the bar's warnings. The load and the dropouts are live
            // values, so these wait for the live timer, which then sends a
            // new warning too, at most 250 ms late.
            (Message::Audio(_) | Message::InputAudio(_), _) => a11y::Touch::Live,
            // The timers that only look: they rebuild the tree only when
            // they changed something it reports.
            (Message::AudioPoll | Message::MidiPoll | Message::Autosave | Message::MidiGhosts, _) => {
                a11y::Touch::Poll(self.a11y.is_active().then(|| self.a11y_poll_key()))
            }
            (_, cause) if cause.is_live() => a11y::Touch::Live,
            _ => a11y::Touch::Changed,
        };
        // A field can go with the view, so any input while one has focus asks
        // again. Live readings and frames do not change what the view holds.
        // The accessibility layer's own messages change no view either. If
        // they asked, the answer would ask for a rebuild, and the rebuild's
        // answer would ask again, for as long as a field has focus.
        let ask_focus = matches!(message, Message::TextFocusCheck)
            || (self.focus.is_some()
                && !matches!(message, Message::FocusMoved(_) | Message::A11y(_))
                && !cause.is_live());
        // A mode change the engine's full queue refused goes again now.
        self.send_pending_safety();
        if !message.keeps_patch() {
            // It may have changed the patch: autosave looks again.
            self.session.document.may_have_changed();
        }
        let was_open = self.overlays.browser().is_some();
        let was_help_open = self.overlays.help.is_some();
        let was_chooser_open = self.overlays.chooser.is_some();
        let before_help = self.input_owner();
        match message {
            Message::Browser(message) => self.browser_message(message),
            Message::Patches(message) => self.patches_message(message),
            Message::Question(message) => {
                self.question_message(message);
            }
            Message::File(FileAction::Browse) => self.open_patches(),
            Message::Key(event) if self.navigation_key(&event, now) => {}
            // An open menu has its own keys (`menus.rs`): Enter sends what its item sends.
            Message::Key(event)
                if menus::is_menu_key(&event)
                    && matches!(
                        self.input_owner(),
                        input_owner::InputOwner::RecentList
                            | input_owner::InputOwner::ProbeMenu
                            | input_owner::InputOwner::TileMenu
                    ) =>
            {
                self.menu_key(&event)
            }
            Message::Key(iced::keyboard::Event::KeyPressed { key, modifiers, .. })
                if modifiers.command()
                    && modifiers.shift()
                    && matches!(key.as_ref(), iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("o"))
                    && self.no_overlay() =>
            {
                self.open_patches()
            }
            Message::Key(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                modifiers,
                ..
            }) if !modifiers.command() && !modifiers.control() && !modifiers.alt() && !self.tab_fields().is_empty() => {
                // Only the window's widgets know in time where focus is: a
                // field can take it with no message (window_question.rs).
                let backwards = modifiers.shift();
                self.ask_window(window_question::WindowQuestion::focus(move |from| Message::TabFrom {
                    from,
                    backwards,
                }));
            }
            Message::TabFrom { from, backwards } => {
                self.tab_between_fields(from.as_ref(), backwards);
            }
            Message::EnterFields { backwards } => self.enter_fields(backwards),
            Message::MenuTab { back } => {
                if let Some(menu) = self.runtime_menu() {
                    self.close_menu(menu.kind);
                    // After the frame that shows the menu closed: by then
                    // the runtime has given back the focus the menu opened
                    // over, and told the app (`FocusMoved`), so the step
                    // goes on from there.
                    self.effect(loom_ui::Effects::after_draw(Message::FocusStep { back }));
                }
            }
            Message::FocusStep { back } => self.effect(loom_ui::Effects::focus_step(back)),
            Message::OverlayTab { back } => {
                let next = if self.overlays.help.is_some() {
                    self.help_next_stop(self.focus.as_ref(), back)
                } else {
                    self.chooser_next_stop(self.focus.as_ref(), back)
                };
                if let Some(next) = next {
                    self.effect(loom_ui::Effects::focus(next));
                }
            }
            Message::ChooserArrow { back } => {
                if let Some((next, choose)) = self.chooser_arrow(back) {
                    self.effect(loom_ui::Effects::focus(next));
                    self.post(choose);
                }
            }
            Message::ToolbarFocus(stop) => self.toolbar_focus(stop),
            Message::Canvas(CanvasEvent::Paste { world }) => {
                let generation = self.generation;
                self.effect(loom_ui::Effects::clipboard_read(move |text| Message::Pasted {
                    world,
                    text,
                    generation,
                }));
            }
            // The library folder is read for the browser's list as it opens.
            Message::Canvas(event @ CanvasEvent::OpenBrowser { .. }) => {
                self.canvas_event(event);
                self.read_library_for_browser();
            }
            Message::Control(call) => self.control(call),
            Message::ControlReply => self.control_reply(),
            Message::StartWatching => self.watching = true,
            Message::RetryCommit(generation) => {
                if generation == self.generation {
                    self.retry_staged_commit();
                }
            }
            Message::Matrix(event) => self.matrix_message(event),
            Message::MatrixKey(key) => self.matrix_key_message(key),
            Message::ControlFrame(now) => self.control_frame(now),
            Message::FieldPasteRefused => {
                self.alerts.warn(crate::clipboard::COULD_NOT_READ.to_string());
            }
            Message::TextFieldTab { backwards } => {
                self.text_field_tab(self.focus.clone().as_ref(), backwards);
            }
            Message::FocusMoved(focus) => {
                self.focus_moved(focus);
            }
            Message::Inspector(message) => self.inspector_message(message),
            Message::Located {
                call,
                target,
                found,
                then,
            } => self.located(call, &target, &found, then),
            Message::Captured {
                call,
                path,
                area,
                image,
            } => {
                self.captured(&call, &path, area, image);
            }
            Message::TypeFocus { call, text, focused } => {
                self.type_after_focus_check(call, &text, focused);
            }
            Message::A11y(event) => self.a11y_event(event),
            Message::Dialog(answer) => {
                self.dialog_answered(answer);
            }
            Message::AskRecovery => {
                self.ask_recovery();
            }
            message if self.clipboard_key(&message).is_some() => {
                self.pending_shortcut = self.clipboard_key(&message).and_then(shortcut::Shortcut::clipboard);
            }
            message => {
                self.handle(message);
            }
        }
        // A range editor can disappear before the window's focus answer.
        if self.session.learn.has_range_draft() && !cause.is_live() && self.midi_range_fields().is_none() {
            self.session.learn.set_range_draft(None);
        }
        // The bridge asks the window where focus is after this turn.
        if ask_focus {
            self.focus_question.set(true);
        }
        // A shortcut on the patch asks the window whether a text field has
        // focus first, so Cmd+Z or Cmd+D right after a click into a field
        // leaves the patch alone (window_question.rs, shortcut.rs).
        if let Some(shortcut) = self.pending_shortcut.take() {
            self.ask_window(window_question::WindowQuestion::focus(move |part| Message::Shortcut {
                shortcut,
                typing: part.is_some(),
            }));
        }
        // A screen reader's tree follows what this message changed.
        if self.a11y.is_active() {
            self.a11y_after(touch);
        }
        self.drain_idle_engine();
        // An idle engine drains commands here, so no watcher is needed.
        while self.session.engine.stream.idle_mut().is_some()
            && self
                .session
                .retry_commit
                .is_some_and(|retry| self.command_drains() != retry.before)
        {
            self.retry_staged_commit();
            self.drain_idle_engine();
        }
        if self.quit {
            // Stop audio now, with a fade so it does not click, so a
            // recording is closed before the process ends, and leave no
            // control socket behind.
            self.stop_audio();
            control::close(&self.control);
            // A clean exit: the recovery folder goes (autosave.md, Clean exit).
            self.run_files.finish();
            self.effect(loom_ui::Effects::quit());
        } else if !was_help_open && self.overlays.help.is_some() {
            if matches!(
                before_help,
                input_owner::InputOwner::Browser | input_owner::InputOwner::Patches | input_owner::InputOwner::Chooser
            ) {
                self.overlays.help_focus = self.focus.clone();
                if self.overlays.help_focus.is_some() {
                    self.effect(loom_ui::Effects::unfocus());
                }
            }
            // Then the focus goes to Close, with a ring only when help was
            // opened from the keyboard (help-pages.md; section 6, rule 4).
            // After the unfocus, which would drop it.
            self.effect(loom_ui::Effects::focus_as_input(help::Help::first_stop()));
        } else if was_help_open && self.overlays.help.is_none() {
            let owner = self.input_owner();
            let field = self
                .overlays
                .help_focus
                .take()
                .filter(|field| match owner {
                    input_owner::InputOwner::Browser => *field == browser::search_id(),
                    input_owner::InputOwner::Patches => patches::text_fields().contains(field),
                    input_owner::InputOwner::Chooser => self.chooser_stops().contains(field),
                    _ => false,
                })
                .or_else(|| (owner == input_owner::InputOwner::Browser).then(browser::search_id))
                .or_else(|| self.overlays.chooser.as_ref().map(licence_chooser::Chooser::first_stop));
            if let Some(field) = field {
                self.effect(loom_ui::Effects::focus(field));
            } else if self
                .focus
                .as_ref()
                .is_some_and(|part| part.is_under(&overlays::access::help_id()))
            {
                self.effect(loom_ui::Effects::unfocus());
            }
        } else if !was_chooser_open && self.overlays.chooser.is_some() {
            self.chooser_opened();
        } else if was_chooser_open && self.overlays.chooser.is_none() {
            self.chooser_closed();
        } else if !was_open && self.overlays.browser().is_some() {
            self.effect(loom_ui::Effects::focus(browser::search_id()));
        }
    }

    /// Opens your patches browser over the library folder, which is read
    /// on a thread (patch-metadata.md, Your patches browser).
    fn open_patches(&mut self) {
        // The folder is made on first use, so a new player's library
        // starts empty rather than missing.
        let Some(folder) = self.library_folder() else {
            if self.rig.store.saved().library_folder().is_none() {
                self.alerts
                    .warn("No library folder: set `library` in the preferences file.".to_string());
            }
            return;
        };
        self.patches_open += 1;
        let (browser, read) = patches::PatchBrowser::open(self.patches_open, folder);
        self.overlays.open(Overlay::Patches(browser));
        self.effect(read);
        let [search, _] = patches::text_fields();
        self.effect(loom_ui::Effects::focus(search));
    }

    /// Handles a patches browser message: keep it open, close it, or open
    /// a patch, asking about unsaved changes first.
    fn patches_message(&mut self, message: patches::Message) {
        let Some(open) = self.overlays.patches_mut() else {
            return;
        };
        let (outcome, effects) = open.update(message);
        self.effect(effects);
        match outcome {
            patches::Outcome::Stay => {}
            patches::Outcome::Close => self.overlays.close_patches(),
            patches::Outcome::Open(path) => {
                self.overlays.close_patches();
                self.after_discard(question::AfterDiscard::OpenPath(path));
            }
        }
    }

    /// Handles a browser message: keep it open, close it, or add a module.
    fn browser_message(&mut self, message: browser::Message) {
        // The folder's items serve the slots' Swap lists too.
        if let browser::Message::Library(items) = &message {
            self.library = items.clone();
        }
        let Some(open) = self.overlays.browser_mut() else {
            return;
        };
        let (outcome, effects) = open.update(message);
        self.effect(effects);
        match outcome {
            browser::Outcome::Stay => {}
            browser::Outcome::Close => self.overlays.close_browser(),
            browser::Outcome::Add(descriptor, placement) => {
                self.overlays.close_browser();
                self.add_module(descriptor, placement);
            }
            browser::Outcome::Insert {
                item,
                linked,
                placement,
            } => {
                self.overlays.close_browser();
                self.insert_library_item(&item, linked, placement);
            }
            browser::Outcome::AddSlot(role, placement) => {
                self.overlays.close_browser();
                self.add_empty_slot(role, placement);
            }
            browser::Outcome::InsertBuiltin(builtin, placement) => {
                self.overlays.close_browser();
                self.insert_builtin(builtin, placement);
            }
        }
    }

    /// Starts reading the library folder for the open browser's list of
    /// sub-patches (sub-patches.md, Library). With no folder, the list
    /// holds modules only.
    fn read_library_for_browser(&mut self) {
        if self.overlays.browser().is_none() {
            return;
        }
        if let Some(folder) = self.rig.store.saved().library_folder() {
            self.effect(browser::read_library(folder));
        }
    }

    /// Adds a module with its top-left corner at a world point and selects
    /// it, as one undo step.
    fn add_module(
        &mut self,
        descriptor: &'static ModuleDescriptor,
        placement: impl Into<loom_canvas::BrowserPlacement>,
    ) {
        let placement = placement.into();
        self.close_slider_gesture();
        if let Some(why) = self.refuses_kind(descriptor.id) {
            self.alerts.warn(why.to_string());
            return;
        }
        // Inside a sub-patch, a Patch input or output is a port of it.
        if self.add_port_tile(&descriptor.kind(), placement) {
            return;
        }
        self.edit(|app| match app.session.new_module(descriptor.id) {
            Ok(id) => {
                app.commit_added_tile(id, placement);
            }
            Err(e) => app.alerts.warn(format!("Could not add {}: {e}", descriptor.name)),
        });
    }

    /// Adds a scope or a meter fed by `output`, beside `world`, and selects
    /// it. The module and its cable are one undo step.
    fn add_probe(&mut self, probe: probe_menu::Probe, output: loom_canvas::PortRef, world: Point) {
        let Some(descriptor) = self.modules.iter().copied().find(|d| d.id == probe.id()) else {
            self.alerts
                .warn(format!("Cannot {}: no module `{}`", probe.verb(), probe.id()));
            return;
        };
        let tiles: Vec<loom_ui_core::Rect> = self.session.scene.modules().iter().map(|m| m.bounds()).collect();
        let corner = probe_menu::place(descriptor, world, &tiles);
        let source = OutputRef {
            module: ModuleId(output.module),
            port: output.index,
        };
        // The output may be gone: the control channel can remove its module
        // while the menu is open.
        let exists = self
            .session
            .controller
            .graph()
            .node(source.module)
            .is_some_and(|node| source.port < node.descriptor.outputs.len());
        if !exists {
            self.alerts.warn(format!("Cannot {}: the output is gone", probe.verb()));
            return;
        }
        self.close_slider_gesture();
        self.session.step_name = Some(probe.verb().to_string());
        self.edit(|app| {
            let id = match app.session.new_module(descriptor.id) {
                Ok(id) => id,
                Err(e) => {
                    app.alerts.warn(format!("Cannot {}: {e}", probe.verb()));
                    return;
                }
            };
            if let Err(e) = app.session.controller.connect(source, InputRef { module: id, port: 0 }) {
                // A probe that is not connected shows nothing: take it back,
                // so the edit makes no step.
                let _ = app.session.controller.remove_module(id);
                app.commit();
                app.alerts.warn(format!("Cannot {}: {e}", probe.verb()));
                return;
            }
            app.session.positions.insert(id.0, (corner.x, corner.y));
            app.session.selection = BTreeSet::from([id.0]);
            app.commit();
        });
        // `record` clears it, but an edit inside a gesture does not record.
        self.session.step_name = None;
    }

    /// The messages with no handler of their own. Called from `apply`,
    /// which has done the shared work first.
    fn handle(&mut self, message: Message) {
        match message {
            // `apply` gives these to their own handlers.
            Message::Dialog(_)
            | Message::AskRecovery
            | Message::Browser(_)
            | Message::Input { .. }
            | Message::Control(_)
            | Message::ControlReply
            | Message::StartWatching
            | Message::RetryCommit(_)
            | Message::ControlFrame(_)
            | Message::FieldPasteRefused
            | Message::TextFieldTab { .. }
            | Message::TabFrom { .. }
            | Message::EnterFields { .. }
            | Message::ToolbarFocus(_)
            | Message::MenuTab { .. }
            | Message::OverlayTab { .. }
            | Message::ChooserArrow { .. }
            | Message::FocusStep { .. }
            | Message::FocusMoved(_)
            | Message::Inspector(_)
            | Message::Located { .. }
            | Message::Captured { .. }
            | Message::TypeFocus { .. }
            | Message::Matrix(_)
            | Message::MatrixKey(_) => {}
            Message::A11y(_) => {}
            Message::ShowHelp => {
                if self.overlays.help.is_none() {
                    let page = if let Some(browser) = self.overlays.browser() {
                        browser.highlighted().map(help::Help::new)
                    } else if self.session.selection.len() == 1 {
                        self.session.selection.first().and_then(|&id| self.tile_help(id))
                    } else {
                        None
                    };
                    // With nothing to describe, F1 shows the shortcuts.
                    self.overlays.help = Some(page.unwrap_or_else(help::Help::shortcuts));
                }
            }
            Message::ShowShortcuts => {
                if self.overlays.preferences().is_none() {
                    self.overlays.help = Some(help::Help::shortcuts());
                }
            }
            Message::ResetClip(id) => {
                let meters = scene::meter_outputs(&self.session.controller, &[id]);
                for (meter, _) in meters {
                    self.reset_clip(meter, true);
                }
            }
            Message::OpenLevel(level) => self.show_level(&level),
            Message::OpenTile(id) => {
                self.overlays.close_tile_menu();
                self.open_tile(id);
            }
            Message::MakeSubPatch(id) => {
                self.overlays.close_tile_menu();
                self.session.selection.insert(id);
                self.make_subpatch();
            }
            Message::DissolveSubPatch(id) => {
                self.overlays.close_tile_menu();
                self.session.selection = BTreeSet::from([id]);
                self.dissolve_subpatch();
            }
            Message::SaveToLibrary(id) => {
                self.overlays.close_tile_menu();
                self.save_to_library(ModuleId(id));
            }
            Message::Unlink(id) => {
                self.overlays.close_tile_menu();
                self.unlink(id);
            }
            Message::MakeSlot { module, role } => {
                self.overlays.close_tile_menu();
                self.make_slot(module, &role);
            }
            Message::SwapSlot { slot, to } => {
                self.overlays.close_tile_menu();
                self.swap_slot(slot, to);
            }
            Message::DissolveSlot(id) => {
                self.overlays.close_tile_menu();
                self.dissolve_slot(id);
            }
            Message::Mode(message) => self.mode_message(message),
            Message::ShowModuleHelp(id) => {
                self.overlays.close_tile_menu();
                self.overlays.help = self.tile_help(id);
            }
            Message::CloseHelp => self.overlays.help = None,
            Message::HelpExample => {
                let example = self.overlays.help.as_ref().and_then(|help| help.model.example.clone());
                if let Some(example) = example {
                    match Patch::from_ron(&example) {
                        Ok(patch) => self.after_discard(question::AfterDiscard::Example(Box::new(patch))),
                        Err(error) => self.alerts.warn(format!("Could not open example: {error}")),
                    }
                }
            }
            Message::HelpLink(link) => {
                let descriptor = link
                    .strip_suffix(".md")
                    .and_then(|id| self.modules.iter().find(|d| d.id == id))
                    .copied();
                if let Some(descriptor) = descriptor {
                    self.overlays.help = Some(help::Help::new(descriptor));
                    self.help_to_top();
                } else if let Some(role) = link
                    .strip_suffix(".md")
                    .and_then(|id| self.session.controller.registry().role(id))
                {
                    self.overlays.help = Some(help::Help::role(role, self.session.controller.registry()));
                    self.help_to_top();
                } else {
                    self.alerts.warn(format!("Linked document is not bundled: {link}"));
                }
            }
            Message::CloseTileMenu => self.overlays.close_tile_menu(),
            Message::CablePort { start, port } => self.pick_cable_port(start, port),
            Message::Key(event) if self.input_owner() != input_owner::InputOwner::Canvas => self.overlay_key(&event),
            Message::Preferences(message) => self.preferences(message),
            Message::OutlineRow(id) => self.outline_select(&id),
            Message::JumpChoice(index) => self.choose_jump(index),
            Message::DismissJump => self.session.outline.close_jump(),
            Message::PatchInfo(message) => self.patch_info_message(message),
            Message::Shortcut { shortcut, typing } => {
                if !typing {
                    self.run_shortcut(shortcut);
                }
            }
            Message::Licence(message) => self.licence_message(message),
            Message::Patches(_) | Message::Question(_) => {}
            Message::OpenRecent => self.open_recent_list(),
            Message::Recent(message) => self.recent_message(message),
            Message::FileDropped(path) => self.file_dropped(path),
            Message::DropsDone => self.drops_done(),
            Message::Key(iced::keyboard::Event::KeyPressed { modifiers, .. })
                if self.matrix_has_keys() && !modifiers.command() => {}
            Message::Key(iced::keyboard::Event::KeyReleased { .. }) if self.matrix_has_keys() => {}
            Message::ProbeMenu(message) => {
                if let (Some(menu), probe_menu::Message::Pick(probe)) = (self.overlays.take_probe_menu(), message) {
                    self.add_probe(probe, menu.output, menu.world);
                }
            }
            Message::AddProbe { probe, output, world } => self.add_probe(probe, output, world),
            Message::Canvas(event) => self.canvas_event(event),
            Message::TextFocusCheck => {}
            Message::SliderReleased => self.close_slider_gesture(),
            Message::Undo => self.undo(),
            Message::Redo => self.redo(),
            Message::OpenBrowser => self.browser_key += 1,
            Message::Panic => self.panic(),
            Message::WindowRescaled => {}
            Message::DismissError(cause) => self.alerts.dismiss(cause),
            Message::MidiPoll => self.poll_midi(),
            Message::HoldNote => {
                if let Some(midi) = &self.session.engine.midi {
                    self.session.engine.holding.toggle();
                    loom_host_desktop::send_midi(
                        midi,
                        if self.session.engine.holding {
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
                scene::update_live_values(
                    &mut self.session.values,
                    &self.session.controller,
                    &self.session.hierarchy,
                );
                self.session.controller.collect_garbage();
                self.update_dropouts(now);
            }
            Message::LiveFrame => {
                scene::update_live_values(
                    &mut self.session.values,
                    &self.session.controller,
                    &self.session.hierarchy,
                );
                if !self.rig.input.feedback_warned() && self.rig.input.port().is_some() {
                    self.sync_audio_input();
                }
            }
            Message::Autosave => self.autosave(Instant::now()),
            Message::Tick => {
                self.session.controller.collect_garbage();
                self.update_dropouts(Instant::now());
                // A swap's crossfade is finished once every fade reports
                // done (sub-patches.md, How a swap crossfades).
                self.finish_due_fades();
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
                silenced,
                clipped,
            }) => {
                self.session.engine.stream.set_load(peak, max);
                self.alerts.note_dropouts(dropouts, at);
                self.alerts.set_output_warnings(silenced, clipped);
            }
            Message::Audio(audio_watch::Event::Stalled { callbacks, advanced }) => {
                // Time the stall from when the watcher last saw a callback,
                // as a poll on this thread would have.
                self.session.engine.stream.watcher_saw(callbacks, advanced);
                self.poll_audio();
            }
            Message::Meters(levels) => {
                // A reading may arrive after its probe was removed.
                let live: std::collections::HashSet<ProbeId> = self.session.engine.live.probes().collect();
                self.session
                    .meter_levels
                    .extend(levels.into_iter().filter(|(probe, _)| live.contains(probe)));
                self.refresh_values();
            }
            Message::Telemetry(update) => {
                self.take_traces(update.traces);
                self.take_histories(update.histories);
                self.take_spectra(update.spectra);
                self.take_xy(update.xy);
                self.take_peaks(update.peaks);
                for (probe, carrying) in update.activity {
                    if let Some(output) = self.session.engine.live.output_of(probe) {
                        if carrying {
                            self.session.carrying.insert(output);
                        } else {
                            self.session.carrying.remove(&output);
                        }
                    }
                }
                if !update.levels.is_empty() {
                    self.handle(Message::Meters(update.levels));
                }
            }
            Message::Lights(readings) => {
                self.take_lights(readings);
                self.sync_audio_input();
                self.refresh_values();
            }
            Message::Learn(message) => self.learn_message(message),
            Message::Modulation(message) => self.modulation_message(message),
            Message::MidiGhosts => {
                if self.refresh_ghosts() {
                    self.refresh_values();
                }
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
            }) if self.session.learn.is_on() => self.stop_learning(),
            // Escape with nothing else using it goes up one level
            // (sub-patches.md, Make, open, and dissolve): the canvas took it
            // first for a drag, pinned lines, control focus, or a selection.
            Message::Key(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) if self.no_overlay() && !self.session.level.is_root() => self.go_up(),
            Message::Key(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) if modifiers.command() => {
                // Ctrl+Z or Cmd+Z undoes; adding Shift redoes. Ctrl+Y also
                // redoes, except on macOS.
                match key.as_ref() {
                    // Undo, redo, and the shortcuts on the selection wait for
                    // a focus check: a text field being edited keeps them
                    // (undo.md, Gestures; shortcut.rs).
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") && modifiers.shift() => {
                        self.pending_shortcut = Some(shortcut::Shortcut::Redo)
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") => {
                        self.pending_shortcut = Some(shortcut::Shortcut::Undo)
                    }
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
                        self.pending_shortcut = Some(shortcut::Shortcut::Redo)
                    }
                    iced::keyboard::Key::Character(",") => self.toggle_preferences(),
                    // Ctrl+G makes a sub-patch; with Shift it dissolves one.
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("g") && modifiers.shift() => {
                        self.pending_shortcut = Some(shortcut::Shortcut::DissolveSubpatch)
                    }
                    iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("g") => {
                        self.pending_shortcut = Some(shortcut::Shortcut::MakeSubpatch)
                    }
                    // Ctrl+Enter or Cmd+Enter opens the selected sub-patch.
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) => {
                        self.pending_shortcut = Some(shortcut::Shortcut::OpenSelected)
                    }
                    _ => {}
                }
            }
            Message::Key(iced::keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat: false,
                ..
            }) if modifiers.is_empty()
                && self.focus.is_none()
                && matches!(key.as_ref(), iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("m")) =>
            {
                self.choose_map(None);
            }
            Message::Key(_) => {}
        }
    }

    /// Space: flips the Transport's Run toggle, as a click on it does
    /// (clock.md, Controls live on the Transport module). While a cable
    /// drives `run`, the input decides, so Space changes nothing. Without a
    /// Transport, it starts or stops the engine's transport directly.
    fn toggle_run(&mut self) {
        if self.rig.midi.following_clock.is_some() {
            return;
        }
        let Some((transport, index)) = transport_run(&self.session.controller) else {
            let running = !self.session.controller.transport().running;
            if let Err(e) = self.session.controller.set_running(running) {
                self.alerts.warn(e.to_string());
            }
            return;
        };
        if run_is_driven(&self.session.controller) {
            self.alerts
                .warn("Run follows the cable on the Transport's run input".to_string());
            return;
        }
        let on = self
            .session
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
        let driven = run_is_driven(&self.session.controller);
        if self.rig.midi.following_clock.is_none()
            && self.session.run_driven
            && !driven
            && let Some((transport, index)) = transport_run(&self.session.controller)
            && let Some(running) = transport_running(&self.session.controller)
        {
            let value = if running { 1.0 } else { 0.0 };
            if self
                .session
                .controller
                .graph()
                .node(transport)
                .is_some_and(|n| n.params[index] != value)
            {
                self.set_param(transport.0, index, value);
            }
        }
        self.session.run_driven = driven;
    }
}

/// Quiet text. The same as `style::QUIET`, kept as a short name here.
const DIM: Color = style::QUIET;

/// Inspector help text, shown with nothing selected. It lists every canvas
/// gesture and shortcut (#16), so keep it in step with `PatchCanvas::handle`
/// and the key handling in `loom-canvas/src/widget/input.rs` and `panel_input.rs`.
pub(crate) const HELP: &str = "Double-click empty canvas to open the module browser there.\n\
Drag from a port to patch a cable. Drag a connected input to move or remove its cable.\n\
Drag tiles to move them. Shift-click or drag a box to select several. Cmd/Ctrl+A selects all.\n\
Delete or Backspace removes the selection.\n\
Drag a knob up or down; Shift for fine steps; double-click resets it.\n\
Tab and Shift-Tab select modules in reading order. Enter focuses the selected module's first control.\n\
With a control focused, Tab moves between controls and arrow keys change values. Enter or a digit starts typing a value on a knob, fader, or number. Space or Enter acts on a toggle, radio, or button.\n\
Escape clears control focus, then the selection. With a module selected and no control focused, arrow keys move it.\n\
Right-drag or two-finger scroll to pan. Wheel or Cmd/Ctrl-scroll to zoom. Cmd/Ctrl+0 zooms to 100%.\n\
F or Home frames the patch.\n\
M cycles the patch map. O opens the Outline. The bracket keys follow a cable, and Cmd/Ctrl+[ goes back.\n\
Right-click an output or a cable to add a scope or a meter fed by it.\n\
Click a modulation badge to pin its lines; click again or press Escape to hide them.\n\
Cmd/Ctrl+C copies, X cuts, V pastes at the pointer, and D duplicates the selection.\n\
Cmd/Ctrl+Z undoes, and Cmd/Ctrl+Shift+Z redoes.\n\
Cmd/Ctrl+G makes a sub-patch of the selection; Cmd/Ctrl+Shift+G dissolves the selected one.\n\
Double-click a sub-patch's header, or press Cmd/Ctrl+Enter with it selected, to open it. Escape with nothing selected goes back up; so does a name in the breadcrumb bar.\n\
MIDI learn: click a control, then move a knob on your controller. Escape ends it.";

#[cfg(test)]
mod tests;
