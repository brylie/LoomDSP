//! Building the app: from the command line at start, and a new engine
//! and session for each patch it opens (`docs/design/files.md`,
//! `docs/design/autosave.md`).

use crate::live::Live;
use crate::{
    App, Args, Session, Source, alerts, bench, devices, document, edits, files, gestures, layout, learn, matrix,
    monitor, pointer, recovery, registry, run_is_driven, scene, settings, stream,
};
use iced::{Color, Point, Theme};
use loom_canvas::PanelValues;
use loom_core::{Controller, EngineConfig, EngineParts, ModuleId, TRANSPORT_MODULE, create_engine};
use loom_host_desktop::AudioOutput;
use loom_patch::Patch;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Instant;

impl App {
    pub(crate) fn new(args: Args) -> Self {
        Self::boot(args, &mut recovery::Dialogs)
    }

    /// Starts the app on the real devices. Tests get the real audio but no
    /// MIDI inputs, so connected hardware cannot change their results.
    pub(crate) fn boot(args: Args, prompt: &mut dyn recovery::Prompt) -> Self {
        #[cfg(test)]
        let host: Rc<dyn devices::Host> = Rc::new(devices::fake::NoMidi);
        #[cfg(not(test))]
        let host: Rc<dyn devices::Host> = Rc::new(devices::System);
        Self::boot_with(args, prompt, host)
    }

    /// Starts the app: reads the preferences, opens this run's recovery
    /// folder, offers a copy left by a crash, then opens the patch and the
    /// devices (`docs/design/autosave.md`, `docs/design/preferences.md`).
    pub(crate) fn boot_with(args: Args, prompt: &mut dyn recovery::Prompt, host: Rc<dyn devices::Host>) -> Self {
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
    pub(crate) fn restored(&mut self, state: recovery::FileState, orphan: recovery::Orphan, args: &Args) {
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

    /// Builds an engine and loads a patch into it. On failure nothing else
    /// is touched, so a failed open leaves the running patch alone.
    pub(crate) fn session(config: EngineConfig, test_modules: bool, source: Source) -> Result<Session, String> {
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
    pub(crate) fn assemble(
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
}
