//! Building the app: from the command line at start, and a new engine
//! and loaded patch for each patch it opens (`docs/design/files.md`,
//! `docs/design/autosave.md`).

use crate::session::{self, Started};
use crate::{
    App, Args, Loaded, Source, alerts, bench, devices, document, files, layout, matrix, pointer, recovery, registry,
    settings, stream,
};
use iced::{Color, Theme};
use loom_core::{Controller, EngineConfig, EngineParts, TRANSPORT_MODULE, create_engine};
use loom_host_desktop::AudioOutput;
use loom_patch::{FlatIds, Hierarchy, Metadata, ModulePath, Patch};
use std::collections::BTreeMap;
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
        let (recent, recent_note) = crate::recent::RecentList::open(store.path());
        notes.extend(recent_note);
        let mut rig = settings::Rig::new(host, store, args.devices.clone(), args.output_safety.is_some());
        rig.recent = recent;
        let mut session_folder = None;
        let mut autosave_off = None;
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
                Err(e) => autosave_off = Some(format!("Autosave is off: {e}.")),
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
        let loaded = Loaded::read(config, args.test_modules, source).unwrap_or_else(|e| {
            // A copy that reads but does not load is a failed restore too:
            // the orphan is kept, not adopted.
            match recovered.take() {
                Some((_, orphan, _)) => restore_error(&e, orphan, &mut notes),
                None => status = e,
            }
            Loaded::read(config, args.test_modules, Source::Empty).expect("an empty patch always loads")
        });
        let mut app = Self::assemble(output, sample_rate, loaded, 1, &args, rig, true);
        // Tests have no bundled folder unless they set one: the checkout's
        // examples must open as plain files there.
        #[cfg(test)]
        app.run_files.set_examples(files::fake::examples());
        #[cfg(not(test))]
        app.run_files.set_examples(files::bundled_examples());
        app.run_files.copy_if_example(&mut app.session.document);
        // The compile report gives way to anything that needs reading.
        let mut keep = !status.is_empty();
        if keep {
            app.alerts.error(alerts::Cause::File, status);
        }
        app.run_files.start_autosave(session_folder);
        if let Some((state, orphan, _)) = recovered {
            app.restored(state, orphan, &args);
            keep = true;
        }
        if !notes.is_empty() {
            let notes = notes.join(" ");
            if keep {
                app.alerts.append_warning(&format!(" {notes}"));
            } else {
                app.alerts.warn(notes);
            }
        }
        // It stays for the whole run, beside whatever the line says.
        if let Some(off) = autosave_off {
            app.alerts.keep_warning(alerts::Cause::Autosave, off);
        }
        app
    }

    /// Sets up a patch restored from a recovery copy (autosave.md, After a
    /// restore): marked unsaved, with its file only if that is unchanged.
    pub(crate) fn restored(&mut self, state: recovery::FileState, orphan: recovery::Orphan, args: &Args) {
        self.session.document.restore(state);
        // Nothing on disk holds this patch.
        self.session.edits.history.clear();
        let when = recovery::local_time(orphan.written());
        self.alerts.say(match args.source() {
            Source::File(path) => format!(
                "Restored unsaved changes from {when}. {} was not opened.",
                files::display_name(Some(path))
            ),
            _ => format!("Restored unsaved changes from {when}."),
        });
        for note in self.session.document.load_notes() {
            if matches!(note, loom_patch::LoadWarning::UnknownModules { .. }) {
                self.alerts.append_warning(&format!(" {note}"));
            }
        }
        self.run_files.adopt(orphan);
        // This session's own copy, at once: the orphan goes when it is on disk.
        self.autosave(Instant::now());
    }

    /// Builds the app around a loaded patch. A rig that has MIDI inputs open
    /// already keeps them, pointed at the new engine. Otherwise its inputs are
    /// opened, unless `connect_midi` is false.
    pub(crate) fn assemble(
        output: Result<AudioOutput, String>,
        sample_rate: f32,
        loaded: Loaded,
        generation: u64,
        args: &Args,
        mut rig: settings::Rig,
        connect_midi: bool,
    ) -> Self {
        let status = loaded.status();
        let Started {
            session,
            first_midi,
            recording_stopped,
            fresh,
        } = session::PatchSession::start(
            loaded,
            output,
            sample_rate,
            generation,
            stream::Playback::from_flags(args.no_audio, args.fake_audio),
            args.output_safety.unwrap_or(rig.store.saved().output_safety),
            &mut rig,
        );
        let mut app = Self {
            session,
            run_files: document::RunFiles::default(),
            right_tab: matrix::Tab::default(),
            text_focus: crate::editing::TextFocus::default(),
            generation,
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
            rig,
            alerts: alerts::Alerts::with_notes(status),
            // The app's tests load the test modules but never list them.
            modules: if args.test_modules {
                registry(true).iter().collect()
            } else {
                loom_modules::registry().iter().collect()
            },
            overlays: Default::default(),
            drops: Vec::new(),
            patches_open: 0,
            browser_key: 0,
            frame_key: 0,
            meter: bench::FrameMeter::default(),
            reporter: args.bench.reporter(),
            control: args.control.listeners(),
            control_pending: Vec::new(),
            pointer: pointer::Pointer::new(!args.control.listeners().is_empty()),
            measuring: args.bench.redraws(),
            animating: args.bench.bench,
            dev_tools: args.dev || args.bench.reporter().is_some(),
            still: None,
            history_key: None,
        };
        app.settle_session(first_midi, connect_midi, recording_stopped, fresh);
        app
    }

    /// What a new session still needs from the app around it, once it is in
    /// place: the engine idle if its stream failed, audio input, MIDI inputs,
    /// the new patch's defaults, and its saved state.
    pub(crate) fn settle_session(
        &mut self,
        first_midi: bool,
        connect_midi: bool,
        recording_stopped: Option<String>,
        fresh: bool,
    ) {
        if let Some(stopped) = recording_stopped {
            self.alerts.error(alerts::Cause::Other, stopped);
        }
        // A stream that failed to start took the engine with it.
        self.keep_engine_idle();
        self.sync_audio_input();
        self.send_panic_trigger();
        if let Some(error) = self.connect_midi(first_midi, connect_midi) {
            self.alerts.warn(error);
        }
        // A new patch credits the user and takes their default licence
        // (patch-metadata.md, Defaults).
        if fresh {
            let saved = self.rig.store.saved();
            let (author, licence) = (saved.author.clone(), saved.licence.clone());
            self.session.document.start_new(&author, licence.as_deref());
        }
        self.session.patch_info = crate::patch_info::State::new(self.session.document.info());
        // A patch just loaded is the one on disk.
        self.session.edits.history.mark_saved();
        let patch = self.current_patch();
        self.session.document.mark_loaded(patch);
        if self.alerts.status().is_empty() {
            self.report();
        }
    }
}

impl Loaded {
    /// What the status line says about this load, if anything.
    pub(crate) fn status(&self) -> String {
        crate::file_commands::join_notes(&self.notes)
    }

    /// Builds an engine and loads a patch into it. On failure nothing else
    /// is touched, so a failed open leaves the running patch alone.
    pub(crate) fn read(config: EngineConfig, test_modules: bool, source: Source) -> Result<Self, String> {
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
        let mut hierarchy = Hierarchy::flat();
        let mut load = |patch: &Patch, controller: &mut Controller| -> Result<Metadata, String> {
            // The flat graph plays; the hierarchy keeps the sub-patches and
            // gives every tile its id (ADR-0039).
            let mut ids = FlatIds::new();
            notes = patch.load_with_ids(controller, &mut ids).map_err(|e| e.to_string())?;
            hierarchy = Hierarchy::from_patch(patch, controller.graph(), ids).map_err(|e| e.to_string())?;
            for (id, layout) in hierarchy.layouts(patch) {
                if controller.graph().node(id).is_some_and(|node| node.unknown.is_some()) {
                    original_unknown_positions.insert(id.0, layout.position);
                }
                if let Some((x, y)) = layout.position {
                    positions.insert(id.0, (x, y));
                }
                if let Some(size) = layout.size {
                    sizes.insert(id.0, size);
                }
            }
            Ok(patch.metadata.clone())
        };
        let named = |name: String| Metadata {
            name,
            ..Metadata::default()
        };
        let fresh = matches!(source, Source::Empty);
        let (info, file, stamp) = match source {
            Source::Generated(n) => {
                layout::generate(&mut controller, n).map_err(|e| format!("generate failed: {e}"))?;
                layout::generated_layout(&controller, &mut positions);
                (named(format!("Generated ({n} modules)")), None, None)
            }
            Source::File(path) => {
                // Stamped before reading: a change after this shows as a change.
                let stamp = recovery::Stamp::of(path);
                let patch = crate::patch_file::load(path).map_err(|e| e.to_string())?;
                let info = load(&patch, &mut controller)?;
                (info, Some(path.to_path_buf()), stamp)
            }
            Source::Recovered(patch) => (load(&patch, &mut controller)?, None, None),
            Source::Empty => (named("Untitled".to_string()), None, None),
        };
        // Every patch has one Transport (clock.md). A loaded file gets one
        // in `load_into`; a new or generated patch gets it here.
        hierarchy.adopt(controller.graph());
        if controller.transport_module().is_none() && controller.registry().resolve(TRANSPORT_MODULE).is_ok() {
            let (_, id) = hierarchy.new_module(&ModulePath::root()).map_err(|e| e.to_string())?;
            controller
                .add_module_with_id(id, TRANSPORT_MODULE)
                .map_err(|e| e.to_string())?;
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
        Ok(Self {
            controller,
            hierarchy,
            engine,
            midi,
            telemetry,
            positions,
            unknown_positions,
            sizes,
            info,
            fresh,
            file,
            stamp,
            notes,
        })
    }
}
