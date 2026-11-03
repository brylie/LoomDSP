//! File commands: the patch name and unsaved mark, Save, Save As, New,
//! Open, and replacing the patch with another (`docs/design/files.md`).

use crate::question::{AfterDiscard, Answer, Kind, Message as QuestionMessage, Question};
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
    /// An edit in patch info counts too (patch-metadata.md).
    pub(crate) fn unsaved(&self) -> bool {
        !self.edits.history.is_saved() || self.document.info_unsaved()
    }

    /// Writes the patch to `path`. On success the path becomes the current
    /// file and the unsaved mark clears.
    pub(crate) fn write_to(&mut self, path: PathBuf) -> bool {
        self.offer_credit();
        let patch = self.current_patch();
        match patch.save(&path) {
            Ok(()) => {
                self.alerts.say(format!("saved {}", path.display()));
                self.document.saved(path, patch);
                self.edits.history.mark_saved();
                self.remember_current();
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

    /// The library folder, made if it is missing. `None`, with a status
    /// line, when there is none or it cannot be made: Save As then starts
    /// where the system chooses.
    pub(crate) fn library_folder(&mut self) -> Option<PathBuf> {
        let dir = self.rig.store.saved().library_folder()?;
        match std::fs::create_dir_all(&dir) {
            Ok(()) => Some(dir),
            Err(e) => {
                self.alerts
                    .say(format!("Could not make the library folder \"{}\": {e}.", dir.display()));
                None
            }
        }
    }

    /// Where a dialog starts with no current file: the last folder used in
    /// one, or else the library folder (files.md, The last folder).
    fn dialog_folder(&mut self) -> Option<PathBuf> {
        match self.rig.recent.last_folder() {
            Some(dir) => Some(dir.to_path_buf()),
            None => self.library_folder(),
        }
    }

    /// Notes the folder of a file picked in a dialog. A failed write costs
    /// only the convenience, so it is only said.
    fn used_folder_of(&mut self, path: &Path) {
        if let Err(e) = self.rig.recent.used_folder_of(path) {
            self.alerts.say(format!("Could not keep the last folder: {e}."));
        }
    }

    /// Puts the current file at the top of the recent list. It reads the
    /// document, so a copy of an example and a failed open never get there.
    /// Not with the control channel on: a script must not change the
    /// player's list (files.md, What is listed).
    pub(crate) fn remember_current(&mut self) {
        if !self.control.is_empty() {
            return;
        }
        let Some(file) = self.document.file() else {
            return;
        };
        // Absolute, so a relative path from the command line still opens
        // from another folder, and the same file is listed once.
        let path = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
        if let Err(e) = self.rig.recent.remember(&path) {
            self.alerts.say(format!("Could not keep the recent patches: {e}."));
        }
    }

    /// Offers once per patch to add the user's name to its authors, before
    /// the first Save (patch-metadata.md, rule 3). Never with the control
    /// channel on, where nobody could answer a dialog.
    fn offer_credit(&mut self) {
        let name = self.rig.store.saved().author.trim().to_string();
        if !self.control.is_empty() || !self.document.should_offer_credit(&name) {
            return;
        }
        match files::confirm_add_author(&name, &self.document.info().authors) {
            Some(true) => {
                self.document.edit_info(|info| info.authors.push(name));
                // The authors box shows the new name, so its next edit
                // keeps it.
                // Only the authors box: a tag being typed stays.
                self.patch_info.authors = crate::patch_info::State::new(self.document.info()).authors;
            }
            Some(false) => {}
            None => self.document.credit_not_asked(),
        }
    }

    /// Lists a patch given on the command line, in a normal windowed run.
    /// Not in a run that only measures or is driven by a script.
    pub(crate) fn remember_opened_at_start(&mut self, args: &Args) {
        let measuring = args.bench.bench || args.bench.measure || args.bench.measure_idle;
        if !measuring && args.patch.is_some() {
            self.remember_current();
        }
    }

    /// Save: to the current file, or Save As when there is none. Returns
    /// true if the patch was written by the time this returns.
    pub(crate) fn save(&mut self) -> bool {
        self.save_then(None)
    }

    /// Saves, then runs `next`. If the save fails or its dialog is cancelled,
    /// `next` never runs, so nothing is lost. A Replace question waits for
    /// its answer and carries `next` with it (files.md, Asking before losing
    /// work).
    pub(crate) fn save_then(&mut self, next: Option<AfterDiscard>) -> bool {
        self.finish_edits();
        match self.document.file().map(Path::to_path_buf) {
            Some(path) => self.write_then(path, next),
            None => self.save_as_then(next),
        }
    }

    pub(crate) fn save_as(&mut self) -> bool {
        self.save_as_then(None)
    }

    /// Asks where to save, saves there, then runs `next`. With the control
    /// channel on it stops, because a native picker would stop the app with
    /// nobody to close it (control-channel.md, rule 9).
    fn save_as_then(&mut self, next: Option<AfterDiscard>) -> bool {
        self.finish_edits();
        if !self.control.is_empty() {
            self.alerts
                .say("Save As opens a file dialog; use the `save` command instead.".to_string());
            return false;
        }
        // A patch with no file yet starts in the last folder used, or else
        // the library folder, made when it is missing, under a free name
        // taken from the patch name (files.md, The last folder).
        // The current file wins, then a hint with a folder (a recovered
        // copy's). A bare hint, as an example copy suggests, goes in the
        // library like a new patch, keeping its name.
        let start = match (
            self.document.file().map(Path::to_path_buf),
            self.document.save_start().map(Path::to_path_buf),
        ) {
            // The current file always wins, even one opened by a bare name.
            (Some(file), _) => Some(file),
            (None, Some(path)) if path.parent().is_some_and(|dir| !dir.as_os_str().is_empty()) => Some(path),
            (None, hint) => {
                let name = hint.as_deref().and_then(Path::file_stem).map_or_else(
                    || self.document.name().to_string(),
                    |stem| stem.to_string_lossy().into_owned(),
                );
                self.dialog_folder().map(|dir| files::free_path(&dir, &name)).or(hint)
            }
        };
        match files::pick_save(start.as_deref()) {
            files::Saving::Cancelled => false,
            files::Saving::Chosen(path) => {
                self.used_folder_of(&path);
                self.write_then(path, next)
            }
            files::Saving::Confirm(path) => {
                self.used_folder_of(&path);
                self.question = Some(Question::replace(path, next));
                false
            }
        }
    }

    /// Writes the patch to `path`. If that worked, runs `next`.
    fn write_then(&mut self, path: PathBuf, next: Option<AfterDiscard>) -> bool {
        let written = self.write_to(path);
        if written && let Some(next) = next {
            self.proceed(next);
        }
        written
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

    /// Runs `next`, which would drop the patch, asking first if it has
    /// unsaved changes (files.md, Asking before losing work).
    pub(crate) fn after_discard(&mut self, next: AfterDiscard) {
        self.finish_edits();
        if self.unsaved() {
            self.question = Some(Question::discard(self.patch_name(), next));
        } else {
            self.proceed(next);
        }
    }

    /// Does what was asked for, now that unsaved changes are dealt with.
    fn proceed(&mut self, next: AfterDiscard) {
        match next {
            AfterDiscard::New => {
                if self.replace(Source::Empty) {
                    self.alerts.say("new patch".to_string());
                }
            }
            AfterDiscard::OpenDialog => {
                let current = self.document.file().map(Path::to_path_buf);
                let fallback = self.dialog_folder();
                if let Some(path) = files::pick_open(current.as_deref(), fallback.as_deref()) {
                    self.used_folder_of(&path);
                    self.open_path(&path);
                }
            }
            AfterDiscard::OpenPath(path) => {
                self.open_path(&path);
            }
            AfterDiscard::OpenDropped { path, several } => self.open_dropped(&path, several),
            AfterDiscard::Close => self.quit = true,
            AfterDiscard::Example(patch) => {
                self.replace(Source::Recovered(patch));
            }
        }
    }

    /// Handles a question message. An answer closes the question first, so
    /// that what it starts can ask another.
    pub(crate) fn question_message(&mut self, message: QuestionMessage) {
        let Some(question) = self.question.as_mut() else {
            return;
        };
        let answer = match message {
            QuestionMessage::Answer(answer) => answer,
            QuestionMessage::Move(by) => {
                question.step(by);
                return;
            }
            QuestionMessage::Activate => question.highlighted(),
            QuestionMessage::Escape => question.safe_answer(),
        };
        let kind = question.clone().into_kind();
        self.question = None;
        match (kind, answer) {
            (Kind::Discard { next, .. }, Answer::Save) => {
                self.save_then(Some(next));
            }
            (Kind::Discard { next, .. }, Answer::DontSave) => self.proceed(next),
            (Kind::Replace { path, next }, Answer::Replace) => {
                self.write_then(path, next);
            }
            _ => {}
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
        fresh.patches_open = self.patches_open;
        fresh.frame_key = self.frame_key + 1;
        *self = fresh;
        // The new patch has no unsaved changes: the copy goes.
        self.autosave(Instant::now());
        true
    }

    /// Tells the user that something failed, in the window, unless the
    /// control channel is on: the reply says so, and a question left open
    /// would block every later command (`docs/design/control-channel.md`,
    /// rule 9). A question already open stays: the status bar has this too.
    pub(crate) fn show_error(&mut self, message: &str) {
        if self.control.is_empty() && self.question.is_none() {
            self.question = Some(Question::error(message.to_string()));
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
            // Opening the browser starts a task, so `App::update` handles
            // it before it reaches here.
            FileAction::Browse => {}
            FileAction::Close => self.after_discard(AfterDiscard::Close),
        }
    }

    pub(crate) fn new_patch(&mut self) {
        self.after_discard(AfterDiscard::New);
    }

    pub(crate) fn open(&mut self) {
        self.after_discard(AfterDiscard::OpenDialog);
    }

    /// Opens the patch at `path` in place of this one, and says so in the
    /// status bar, with any notes about what loaded differently.
    pub(crate) fn open_path(&mut self, path: &std::path::Path) -> bool {
        let opened = self.replace(Source::File(path));
        if opened {
            self.alerts.say(opened_status(path, self.document.load_notes()));
            self.remember_current();
        }
        opened
    }

    /// Opens a dropped patch. Overlays close only once it has opened, so
    /// Cancel changes nothing. When others came with it, the status says so
    /// and keeps any notes about what loaded differently.
    fn open_dropped(&mut self, path: &Path, several: bool) {
        if !self.open_path(path) {
            return;
        }
        self.rig.window = None;
        if several {
            let notes = self.document.load_notes().join("; ");
            let name = files::display_name(Some(path));
            let notes = if notes.is_empty() {
                notes
            } else {
                format!(" Note: {notes}.")
            };
            self.alerts
                .say(format!("Opened \"{name}\". Drop one patch at a time.{notes}"));
        }
    }

    /// Opens the recent list, if it has anything to show. Whether a file is missing is looked
    /// up here, not on every frame.
    pub(crate) fn open_recent_list(&mut self) {
        self.recent_rows = (!self.rig.recent.files().is_empty()).then(|| crate::recent::rows(self.rig.recent.files()));
    }

    /// Handles a message for the open list.
    pub(crate) fn recent_message(&mut self, message: crate::recent::Message) {
        let rows = self.recent_rows.take();
        let crate::recent::Message::Pick(index) = message else {
            return;
        };
        let Some(path) = rows.and_then(|rows| rows.into_iter().nth(index)).map(|row| row.path) else {
            return;
        };
        // Looked up again: it may have gone since the list opened. It goes
        // before any question, so there is nothing to answer for a file that
        // cannot open.
        if !path.is_file() {
            if let Err(e) = self.rig.recent.forget(&path) {
                self.alerts.say(format!("Could not keep the recent patches: {e}."));
                return;
            }
            let name = files::display_name(Some(&path));
            self.alerts
                .say(format!("\"{name}\" is no longer there, so it is off the list."));
            return;
        }
        self.after_discard(AfterDiscard::OpenPath(path));
    }

    /// Adds a file to the drop being collected. A drop while a question is
    /// open is ignored, because the question must be answered first.
    pub(crate) fn file_dropped(&mut self, path: PathBuf) {
        if self.question.is_some() {
            self.alerts
                .say("The dropped file was not opened: answer the question first.".to_string());
        } else {
            self.drops.push(path);
        }
    }

    /// The moment for collecting a drop is over: acts on what came.
    pub(crate) fn drops_done(&mut self) {
        let drops = std::mem::take(&mut self.drops);
        if drops.is_empty() {
            return;
        }
        if self.question.is_some() {
            self.alerts
                .say("The dropped file was not opened: answer the question first.".to_string());
            return;
        }
        match crate::recent::dropped(&drops) {
            crate::recent::Dropped::Open { path, several } => {
                self.after_discard(AfterDiscard::OpenDropped { path, several });
            }
            crate::recent::Dropped::NoPatch => {
                self.alerts
                    .say("Only LoomDSP patches (.ron) can be opened here.".to_string());
            }
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

#[cfg(test)]
mod tests;
