//! File commands: the patch name and unsaved mark, Save, Save As, New,
//! Open, and replacing the patch with another (`docs/design/files.md`).

use crate::{App, Args, FileAction, Source, bench, control, devices, files, screenshot, settings};
use loom_core::EngineConfig;
use std::path::{Path, PathBuf};
use std::time::Instant;

impl App {
    /// The name of the patch as the title shows it, without the mark.
    pub(crate) fn patch_name(&self) -> String {
        self.document.title_name()
    }

    /// True when the patch differs from the file (`docs/design/files.md`).
    pub(crate) fn unsaved(&self) -> bool {
        !self.edits.history.is_saved()
    }

    /// Writes the patch to `path`. On success the path becomes the current
    /// file and the unsaved mark clears.
    pub(crate) fn write_to(&mut self, path: PathBuf) -> bool {
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
    pub(crate) fn save(&mut self) -> bool {
        self.finish_edits();
        match self.document.file().map(Path::to_path_buf) {
            Some(path) => self.write_to(path),
            None => self.save_as(),
        }
    }

    pub(crate) fn save_as(&mut self) -> bool {
        self.finish_edits();
        match files::pick_save(self.document.save_start()) {
            Some(path) => self.write_to(path),
            None => false,
        }
    }

    /// Ends any edit in progress, so the history and the saved mark see it.
    /// A canvas drag stays open: the rest of it becomes a step of its own
    /// when it ends.
    pub(crate) fn finish_edits(&mut self) {
        self.close_slider_gesture();
        if self.edits.gesture_open() {
            self.record();
        }
    }

    /// Asks what to do with unsaved changes. Returns true if the caller may
    /// go on and drop them.
    pub(crate) fn can_discard(&mut self) -> bool {
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
    pub(crate) fn replace(&mut self, source: Source) -> bool {
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
    pub(crate) fn show_error(&self, message: &str) {
        if self.control.is_empty() {
            files::show_error(message);
        }
    }

    /// Runs a file command from the toolbar, a shortcut, or the window.
    pub(crate) fn file_action(&mut self, action: FileAction) {
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

    pub(crate) fn new_patch(&mut self) {
        if self.can_discard() && self.replace(Source::Empty) {
            self.alerts.say("new patch".to_string());
        }
    }

    pub(crate) fn open(&mut self) {
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
    pub(crate) fn open_path(&mut self, path: &std::path::Path) {
        if self.replace(Source::File(path)) {
            self.alerts.say(opened_status(path, self.document.load_notes()));
        }
    }
}

/// `opened <path>`, followed by the load notes when there are any, so a note
/// about what loaded differently stays in view.
pub(crate) fn opened_status(path: &std::path::Path, notes: &[String]) -> String {
    if notes.is_empty() {
        format!("opened {}", path.display())
    } else {
        format!("opened {}: {}", path.display(), notes.join("; "))
    }
}
