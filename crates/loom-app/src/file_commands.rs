//! File commands: the patch name and unsaved mark, Save, Save As, New,
//! Open, and replacing the patch with another (`docs/design/files.md`).

use crate::dialogs::Waiting;
use crate::overlays::Overlay;
use crate::question::{AfterDiscard, Answer, Kind, Message as QuestionMessage, Question};
use crate::session::{PatchSession, Started};
use crate::{App, Args, FileAction, Loaded, Source, alerts, bench, control, files, settings};
use loom_core::EngineConfig;
use loom_patch::Patch;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The status bar text when Save to library is asked with no sub-patch to
/// save: at the top level, or on a tile that is not one.
pub(crate) const SAVE_TO_LIBRARY_INSIDE: &str = "Save to library works on a sub-patch: open one, or use its tile menu";

/// Why the patch was not replaced. Nothing changed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NotReplaced {
    /// Audio plays now, and the new patch would have no device to play on.
    NoDevice(String),
    /// The patch did not load.
    NotLoaded(String),
}

impl App {
    /// The name of the patch as the title shows it, without the mark.
    pub(crate) fn patch_name(&self) -> String {
        self.session.document.title_name()
    }

    /// True when the patch differs from the file (`docs/design/files.md`).
    /// An edit in patch info counts too (patch-metadata.md).
    pub(crate) fn unsaved(&self) -> bool {
        !self.session.edits.history.is_saved() || self.session.document.info_unsaved()
    }

    /// Writes the patch to `path`. On success the path becomes the current
    /// file and the unsaved mark clears.
    pub(crate) fn write_to(&mut self, path: PathBuf) -> bool {
        let patch = self.current_patch();
        match crate::patch_file::save(&patch, &path) {
            Ok(()) => {
                self.alerts.clear(alerts::Cause::File);
                self.alerts.say(format!("saved {}", path.display()));
                self.session.document.saved(path, patch);
                self.session.edits.history.mark_saved();
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
                self.alerts.error(alerts::Cause::File, message);
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
                    .warn(format!("Could not make the library folder \"{}\": {e}.", dir.display()));
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
    pub(crate) fn used_folder_of(&mut self, path: &Path) {
        if let Err(e) = self.rig.recent.used_folder_of(path) {
            self.alerts.warn(format!("Could not keep the last folder: {e}."));
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
        let Some(file) = self.session.document.file() else {
            return;
        };
        // Absolute, so a relative path from the command line still opens
        // from another folder, and the same file is listed once.
        let path = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
        if let Err(e) = self.rig.recent.remember(&path) {
            self.alerts.warn(format!("Could not keep the recent patches: {e}."));
        }
    }

    /// Offers once per patch to add the user's name to its authors, before
    /// the first Save (patch-metadata.md, rule 3). Never with the control
    /// channel on, where nobody could answer a dialog. Returns true when it
    /// asked: the write to `path`, and then `next`, wait for the answer.
    fn offer_credit(&mut self, path: &Path, next: Option<AfterDiscard>) -> bool {
        let name = self.rig.store.saved().author.trim().to_string();
        if !self.control.is_empty() || !self.session.document.should_offer_credit(&name) {
            return false;
        }
        match files::author_question(&name, &self.session.document.info().authors) {
            Some(question) => {
                let waiting = Waiting::Credit {
                    name,
                    path: path.to_path_buf(),
                    next,
                };
                self.ask_question(waiting, question)
            }
            None => {
                self.session.document.credit_not_asked();
                false
            }
        }
    }

    /// The answer to the credit question: adds the name or not, then writes.
    pub(crate) fn credit_answered(&mut self, name: String, added: bool, path: PathBuf, next: Option<AfterDiscard>) {
        if added {
            self.session.document.edit_info(|info| info.authors.push(name));
            // The authors box shows the new name, so its next edit
            // keeps it.
            // Only the authors box: a tag being typed stays.
            self.session.patch_info.authors = crate::patch_info::State::new(self.session.document.info()).authors;
        }
        self.write_now_then(path, next);
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
        match self.session.document.file().map(Path::to_path_buf) {
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
    pub(crate) fn save_as_then(&mut self, next: Option<AfterDiscard>) -> bool {
        self.finish_edits();
        if !self.control.is_empty() {
            self.alerts
                .warn("Save As opens a file dialog; use the `save` command instead.".to_string());
            return false;
        }
        // A patch with no file yet starts in the last folder used, or else
        // the library folder, made when it is missing, under a free name
        // taken from the patch name (files.md, The last folder).
        // The current file wins, then a hint with a folder (a recovered
        // copy's). A bare hint, as an example copy suggests, goes in the
        // library like a new patch, keeping its name.
        let start = match (
            self.session.document.file().map(Path::to_path_buf),
            self.session.document.save_start().map(Path::to_path_buf),
        ) {
            // The current file always wins, even one opened by a bare name.
            (Some(file), _) => Some(file),
            (None, Some(path)) if path.parent().is_some_and(|dir| !dir.as_os_str().is_empty()) => Some(path),
            (None, hint) => {
                let name = hint.as_deref().and_then(Path::file_stem).map_or_else(
                    || self.session.document.name().to_string(),
                    |stem| stem.to_string_lossy().into_owned(),
                );
                self.dialog_folder().map(|dir| files::free_path(&dir, &name)).or(hint)
            }
        };
        // The answer comes on a later turn (`dialogs.rs`).
        self.ask_file(Waiting::SaveAs { next }, files::save_request(start.as_deref()));
        false
    }

    /// The Save As dialog's answer: saves there, then runs `next`, or asks
    /// first about a file the dialog did not ask about.
    pub(crate) fn save_as_chosen(&mut self, saving: files::Saving, next: Option<AfterDiscard>) -> bool {
        match saving {
            files::Saving::Cancelled => false,
            files::Saving::Chosen(path) => {
                self.used_folder_of(&path);
                self.write_then(path, next)
            }
            files::Saving::Confirm(path) => {
                self.used_folder_of(&path);
                self.overlays.question = Some(Question::replace(path, next));
                false
            }
        }
    }

    /// Writes the patch to `path`. If that worked, runs `next`. The first
    /// save of a patch by others may ask to credit the user first; the
    /// write then waits for the answer (`dialogs.rs`).
    pub(crate) fn write_then(&mut self, path: PathBuf, next: Option<AfterDiscard>) -> bool {
        if self.offer_credit(&path, next.clone()) {
            return false;
        }
        self.write_now_then(path, next)
    }

    /// Writes the patch to `path` with no question, then runs `next` if it
    /// worked.
    pub(crate) fn write_now_then(&mut self, path: PathBuf, next: Option<AfterDiscard>) -> bool {
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
        if self.session.edits.gesture_open() {
            self.record();
        }
    }

    /// Runs `next`, which would drop the patch, asking first if it has
    /// unsaved changes (files.md, Asking before losing work).
    pub(crate) fn after_discard(&mut self, next: AfterDiscard) {
        self.finish_edits();
        if self.unsaved() {
            self.overlays.question = Some(Question::discard(self.patch_name(), next));
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
                let current = self.session.document.file().map(Path::to_path_buf);
                // Only if the current file has no folder to start in: looking
                // up the library folder makes it.
                let has_folder = current.as_deref().and_then(Path::parent).is_some_and(Path::is_dir);
                let fallback = if has_folder { None } else { self.dialog_folder() };
                self.ask_file(
                    Waiting::Open,
                    files::open_request(current.as_deref(), fallback.as_deref()),
                );
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
    /// that what it starts can ask another. Returns false when a Save or
    /// Replace answer wrote nothing: the status bar says why, and the control
    /// channel's reply says so too (control-channel.md, rule 9).
    pub(crate) fn question_message(&mut self, message: QuestionMessage) -> bool {
        if let QuestionMessage::Move(by) = message {
            if let Some(question) = self.overlays.question.as_mut() {
                question.step(by);
            }
            return true;
        }
        let Some(question) = self.overlays.question.take() else {
            return true;
        };
        let answer = match message {
            QuestionMessage::Answer(answer) => answer,
            QuestionMessage::Activate => question.highlighted(),
            QuestionMessage::Escape | QuestionMessage::Move(_) => question.safe_answer(),
        };
        let kind = question.into_kind();
        match (kind, answer) {
            (Kind::Discard { next, .. }, Answer::Save) => self.save_then(Some(next)),
            (Kind::Discard { next, .. }, Answer::DontSave) => {
                self.proceed(next);
                true
            }
            (Kind::Replace { path, next }, Answer::Replace) => self.write_then(path, next),
            (Kind::ReplaceLibrary { path, file, name }, Answer::Replace) => {
                self.write_library_file(&file, &path, &name)
            }
            _ => true,
        }
    }

    /// Replaces the patch (and its engine) with `source`. If the source does
    /// not load, the running patch is unchanged.
    pub(crate) fn replace(&mut self, source: Source) -> bool {
        let message = match self.try_replace(source) {
            Ok(()) => return true,
            Err(NotReplaced::NoDevice(e)) => format!(
                "Could not open the patch: the audio device is not available ({e}). The current patch is unchanged."
            ),
            Err(NotReplaced::NotLoaded(e)) => format!("Could not open the patch: {e}. The current patch is unchanged."),
        };
        self.show_error(&message);
        self.alerts.error(alerts::Cause::File, message);
        false
    }

    /// Replaces the patch as [`App::replace`] does, but says nothing when
    /// it cannot: the caller words why, as a restore does (`dialogs.rs`).
    pub(crate) fn try_replace(&mut self, source: Source) -> Result<(), NotReplaced> {
        // Choosing the device does not start a stream, so the old patch keeps
        // playing. The engine is built at the rate of the device that will
        // play it.
        // A recording goes on into the same file (rig.recorder).
        let playback = self.session.engine.stream.playback();
        // A lost named device is being waited for, so the new patch stays idle
        // too, rather than coming out of other speakers (device-changes.md).
        let (output, _) = match self.rig.lost_audio.as_ref().filter(|lost| !lost.was_system_default) {
            Some(lost) => (Err(format!("waiting for \"{}\"", lost.device)), Vec::new()),
            None => settings::open_output(&self.rig, playback.is_off(), playback.is_fake()),
        };
        // Playing now and no device to play on afterwards: keep what plays.
        if let (Err(e), true) = (&output, self.session.engine.stream.is_playing()) {
            return Err(NotReplaced::NoDevice(e.clone()));
        }
        let sample_rate = output
            .as_ref()
            .map(|o| o.sample_rate() as f32)
            .unwrap_or(self.session.engine.stream.sample_rate());
        let config = EngineConfig {
            output_safety: self.session.engine.stream.output_safety(),
            ..EngineConfig::with_sample_rate(sample_rate)
        };
        let loaded = match Loaded::read(config, self.test_modules, source, &self.rig.library()) {
            Ok(loaded) => loaded,
            Err(e) => return Err(NotReplaced::NotLoaded(e)),
        };
        // Nothing can fail from here: fade the old engine out and free the
        // audio device before the new engine takes it. MIDI inputs stay
        // open and move to the new engine.
        self.stop_audio();
        self.session.engine.midi = None;
        self.generation += 1;
        self.alerts.begin_patch(loaded.status());
        let output_safety = self.session.engine.stream.output_safety();
        let Started {
            session,
            first_midi,
            recording_stopped,
            fresh,
        } = PatchSession::start(
            loaded,
            output,
            sample_rate,
            self.generation,
            playback,
            output_safety,
            &mut self.rig,
        );
        // Goes last, as the threads of the old engine's watchers end with it.
        let old = std::mem::replace(&mut self.session, session);
        self.a11y.patch_replaced();
        // The canvas keeps its state across the swap and compares this key
        // with the one it saw. A new frame key resets pan and zoom to show the
        // whole patch.
        self.frame_key += 1;
        self.meter = bench::FrameMeter::default();
        self.text_focus = crate::editing::TextFocus::default();
        self.drops.clear();
        self.overlays.close_for_new_patch();
        // Commands waiting to be drawn still get their answer, except a
        // `sample`, which would measure a module of the new patch.
        self.control_pending = control::end_samples(std::mem::take(&mut self.control_pending));
        self.settle_session(first_midi, true, recording_stopped, fresh);
        self.run_files.copy_if_example(&mut self.session.document);
        // The file that could not be read or written no longer matters.
        self.alerts.clear(alerts::Cause::File);
        // A kept problem with the devices stays only if it is still true.
        if self.session.engine.stream.is_playing() {
            self.audio_back();
        }
        if self.rig.input.open().is_some() || !self.input_wanted() {
            self.alerts.clear(alerts::Cause::AudioInput);
        }
        drop(old);
        // The new patch has no unsaved changes: the copy goes.
        self.autosave(Instant::now());
        Ok(())
    }

    /// Tells the user that something failed, in the window, unless the
    /// control channel is on: the reply says so, and a question left open
    /// would block every later command (`docs/design/control-channel.md`,
    /// rule 9). A question already open stays: the status bar has this too.
    pub(crate) fn show_error(&mut self, message: &str) {
        if self.control.is_empty() && self.overlays.question.is_none() {
            self.overlays.question = Some(Question::error(message.to_string()));
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
            FileAction::SaveToLibrary => {
                if let Some(instance) = self.shown_instance() {
                    self.save_to_library(instance);
                } else {
                    self.alerts.warn(SAVE_TO_LIBRARY_INSIDE.to_string());
                }
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
            self.say_opened(path);
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
        self.overlays.close_preferences();
        if several {
            let notes = join_notes(self.session.document.load_notes());
            let name = files::display_name(Some(path));
            let notes = if notes.is_empty() {
                notes
            } else {
                format!(" Note: {notes}.")
            };
            self.alerts
                .warn(format!("Opened \"{name}\". Drop one patch at a time.{notes}"));
        }
    }

    /// Opens the recent list, if it has anything to show. Whether a file is
    /// missing is looked up here, not on every frame. The matrix gives up the
    /// keys, or Delete would remove an assignment behind the list.
    pub(crate) fn open_recent_list(&mut self) {
        if self.rig.recent.files().is_empty() {
            self.overlays.close_recent_list();
        } else {
            let rows = crate::recent::rows(self.rig.recent.files());
            self.overlays.open(Overlay::RecentList(rows));
            self.session.matrix.focused = false;
        }
    }

    /// Handles a message for the open list.
    pub(crate) fn recent_message(&mut self, message: crate::recent::Message) {
        let rows = self.overlays.take_recent_rows();
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
                self.alerts.warn(format!("Could not keep the recent patches: {e}."));
                return;
            }
            let name = files::display_name(Some(&path));
            self.alerts
                .warn(format!("\"{name}\" is no longer there, so it is off the list."));
            return;
        }
        self.after_discard(AfterDiscard::OpenPath(path));
    }

    /// Adds a file to the drop being collected. A drop while a question is
    /// open is ignored, because the question must be answered first.
    pub(crate) fn file_dropped(&mut self, path: PathBuf) {
        if self.overlays.question.is_some() {
            self.alerts
                .warn("The dropped file was not opened: answer the question first.".to_string());
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
        if self.overlays.question.is_some() {
            self.alerts
                .warn("The dropped file was not opened: answer the question first.".to_string());
            return;
        }
        match crate::recent::dropped(&drops) {
            crate::recent::Dropped::Open { path, several } => {
                self.after_discard(AfterDiscard::OpenDropped { path, several });
            }
            crate::recent::Dropped::NoPatch => {
                self.alerts
                    .warn("Only LoomDSP patches (.ron) can be opened here.".to_string());
            }
        }
    }
}

impl App {
    /// The sub-patch tile whose inside the canvas shows. `None` at the top.
    pub(crate) fn shown_instance(&self) -> Option<loom_core::ModuleId> {
        let level = &self.session.level;
        (!level.is_root())
            .then(|| self.session.hierarchy.ids().get(level))
            .flatten()
    }

    /// Save to library: writes the sub-patch tile `instance` as a patch
    /// file in the library folder (`docs/design/sub-patches.md`, Library).
    /// A linked tile writes to the file it follows, with no dialog; a copy
    /// asks where, starting in the library folder under the sub-patch's
    /// name. Not an edit: the open patch, and every other tile in it, stays
    /// as it is (rule 3). Refused when the file would contain itself
    /// through a linked sub-patch inside. Returns true when the file was
    /// written.
    pub(crate) fn save_to_library(&mut self, instance: loom_core::ModuleId) -> bool {
        self.finish_edits();
        // A patch file has no place for a mode yet (sub-patches.md, How modes
        // meet the rest).
        if let Some(mode) = self
            .session
            .hierarchy
            .modes_of(instance)
            .map(|m| m.mode)
            .filter(|m| !m.is_single())
        {
            self.alerts.warn(loom_patch::modes::no_library(mode));
            return false;
        }
        let Some(file) =
            self.session
                .hierarchy
                .definition_file(self.session.controller.graph(), self.layout_fn(), instance)
        else {
            self.alerts.warn(SAVE_TO_LIBRARY_INSIDE.to_string());
            return false;
        };
        let name = file.metadata.name.clone();
        let folder = self
            .library_folder()
            .map(|root| crate::patch_file::FolderLibrary { root });
        let target = match self.session.hierarchy.library_of(instance).map(str::to_string) {
            Some(path) => {
                let Some(target) = folder.as_ref().and_then(|folder| folder.file(&path)) else {
                    self.alerts.warn(format!(
                        "Cannot save {name} to the library: no library folder holds \"{path}\"."
                    ));
                    return false;
                };
                target
            }
            None => {
                if !self.control.is_empty() {
                    self.alerts
                        .warn("Save to library opens a file dialog, which a script cannot answer.".to_string());
                    return false;
                }
                let start = folder.as_ref().map(|folder| files::free_path(&folder.root, &name));
                // The answer comes on a later turn (`dialogs.rs`).
                let waiting = Waiting::SaveToLibrary {
                    file: Box::new(file),
                    name,
                };
                self.ask_file(waiting, files::save_request(start.as_deref()));
                return false;
            }
        };
        self.library_target_chosen(file, name, files::Saving::Chosen(target))
    }

    /// Where a sub-patch goes in the library, from its link or its dialog:
    /// writes it there, or asks first about a file the dialog did not ask
    /// about.
    pub(crate) fn library_target_chosen(&mut self, file: Patch, name: String, saving: files::Saving) -> bool {
        match saving {
            files::Saving::Cancelled => false,
            files::Saving::Chosen(target) => {
                if !self.library_file_is_sound(&file, &target, &name) {
                    return false;
                }
                self.write_library_file(&file, &target, &name)
            }
            // A name typed without its extension that another file has:
            // ask, as Save As does.
            files::Saving::Confirm(path) => {
                if self.library_file_is_sound(&file, &path, &name) {
                    self.overlays.question = Some(Question::replace_library(path, file, name));
                }
                false
            }
        }
    }

    /// Refuses, with the reason in the status bar, a library file that
    /// would contain itself: a linked sub-patch inside it follows the very
    /// file being written (sub-patches.md, Patch file).
    fn library_file_is_sound(&mut self, file: &Patch, target: &Path, name: &str) -> bool {
        let folder = self
            .rig
            .store
            .saved()
            .library_folder()
            .map(|root| crate::patch_file::FolderLibrary { root });
        let relative = folder.as_ref().and_then(|folder| folder.path_of(target));
        if let Some(relative) = &relative
            && let Some(nested) = file
                .subpatches
                .values()
                .find(|d| d.library.as_deref() == Some(relative.as_str()))
        {
            let error = loom_patch::PatchError::SubPatchLoop {
                path: vec![name.to_string(), nested.metadata.name.clone()],
            };
            self.alerts.warn(format!("Cannot save {name} to the library: {error}"));
            return false;
        }
        true
    }

    /// Writes the sub-patch `name` as the patch file `target`, making the
    /// folders on the way, and says so. Returns true when it was written.
    fn write_library_file(&mut self, file: &Patch, target: &Path, name: &str) -> bool {
        let written = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .map_err(|e| e.to_string())
            .and_then(|()| crate::patch_file::save(file, target).map_err(|e| e.to_string()));
        match written {
            Ok(()) => {
                self.alerts.clear(alerts::Cause::File);
                self.alerts.say(format!("saved {name} to {}", target.display()));
                // A new or changed item shows in the slots' Swap lists.
                self.refresh_library_items();
                true
            }
            Err(e) => {
                let message = format!("Could not save {name} to \"{}\": {e}.", target.display());
                self.show_error(&message);
                self.alerts.error(alerts::Cause::File, message);
                false
            }
        }
    }

    /// Says a patch opened: info, or a warning when it loaded differently
    /// from how it was saved. Shared with the control channel's `open`.
    pub(crate) fn say_opened(&mut self, path: &std::path::Path) {
        let status = opened_status(path, self.session.document.load_notes());
        if self.session.document.load_notes().is_empty() {
            self.alerts.say(status);
        } else {
            self.alerts.warn(status);
        }
    }
}

/// The load notes as the status line shows them: each one's text, joined
/// by semicolons.
pub(crate) fn join_notes(notes: &[loom_patch::LoadWarning]) -> String {
    notes.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
}

/// `opened <path>`, followed by the load notes when there are any, so a note
/// about what loaded differently stays in view.
pub(crate) fn opened_status(path: &std::path::Path, notes: &[loom_patch::LoadWarning]) -> String {
    if notes.is_empty() {
        format!("opened {}", path.display())
    } else {
        format!("opened {}: {}", path.display(), join_notes(notes))
    }
}

#[cfg(test)]
mod tests;
