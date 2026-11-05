//! The open patch as a document: its name and file, what the title and the
//! save dialog show, whether the work is saved, and its recovery copy.
//! Specs: `docs/design/files.md`, `docs/design/autosave.md`.
//!
//! The app owns one [`Document`] per patch, and one [`RunFiles`] for the run,
//! and reaches them through their methods. Building the patch from the graph, the undo history, dialogs,
//! and the status bar stay with the app.

use crate::{files, recovery};
use loom_patch::{Metadata, Patch};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// What belongs to the run and not to one patch: the folders that apply to
/// every patch it opens.
#[derive(Default)]
pub struct RunFiles {
    /// This run's recovery folder. `None` when autosave is off.
    recovery: Option<recovery::Session>,
    /// The bundled examples folder, found once at start. A patch opened
    /// from it is an untitled copy (files.md, Defaults for the dialogs).
    examples: Option<PathBuf>,
}

impl RunFiles {
    /// Sets the bundled examples folder.
    pub fn set_examples(&mut self, examples: Option<PathBuf>) {
        self.examples = examples;
    }

    /// Applies the examples rule to a patch.
    pub fn copy_if_example(&self, document: &mut Document) {
        document.copy_if_example(self.examples.as_deref());
    }

    /// Turns autosave on with this run's recovery folder, or leaves it off.
    pub fn start_autosave(&mut self, session: Option<recovery::Session>) {
        self.recovery = session;
    }

    /// True while autosave is on.
    pub fn autosave_on(&self) -> bool {
        self.recovery.is_some()
    }

    /// Keeps the orphan a restore came from until this session has a copy.
    pub fn adopt(&mut self, orphan: recovery::Orphan) {
        if let Some(session) = &mut self.recovery {
            session.adopt(orphan);
        }
    }

    /// Writes or deletes the recovery copy of `document` when one is due,
    /// given the patch as it would be saved now. Returns what the player
    /// should be told.
    pub fn autosave(&mut self, document: &mut Document, now: Instant, current: Patch) -> Vec<recovery::Report> {
        let target = (document.saved_content.as_ref() != Some(&current)).then_some(current);
        let (file, stamp) = (document.file.clone(), document.stamp);
        let Some(session) = self.recovery.as_mut() else {
            return Vec::new();
        };
        let reports = session.tick(now, target, || {
            recovery::Info::new(file.as_deref(), stamp, std::time::SystemTime::now())
        });
        document.settled = session.settled();
        reports
    }

    /// True while a recovery copy of `document` may be due, so the autosave
    /// timer runs.
    pub fn autosave_due(&self, document: &Document) -> bool {
        self.recovery.is_some() && !document.settled
    }

    /// A clean exit: the recovery folder goes (autosave.md, Clean exit).
    pub fn finish(&mut self) {
        if let Some(session) = self.recovery.take() {
            session.finish();
        }
    }

    /// This run's recovery folder, for tests.
    #[cfg(test)]
    pub fn recovery(&self) -> Option<&recovery::Session> {
        self.recovery.as_ref()
    }
}

/// The open patch's file and saved state.
pub struct Document {
    /// The name, description, authors, tags, and licence saved inside the
    /// patch (`docs/design/patch-metadata.md`).
    info: Metadata,
    /// The metadata as it was last opened or saved, so an edit in patch
    /// info marks the patch unsaved. Undo does not cover these edits.
    saved_info: Metadata,
    /// True once the first Save has offered to add the user's name, so it
    /// is offered once per opened patch (patch-metadata.md, rule 3).
    credit_offered: bool,
    /// The file the patch was opened from or saved to.
    file: Option<PathBuf>,
    /// The file's time and size when it was last opened or saved.
    stamp: Option<recovery::Stamp>,
    /// The title's name for a patch with no file, when it is not
    /// "Untitled": a restored copy whose file changed, or an example.
    shown_name: Option<String>,
    /// Where Save As starts when there is no current file: next to a
    /// restored copy's changed or missing file, or the file name of the
    /// example a copy came from.
    save_hint: Option<PathBuf>,
    /// The patch as it was last opened or saved, to tell unsaved work
    /// apart. `None` when nothing on disk holds this patch (a restored copy).
    saved_content: Option<Patch>,
    /// True when the recovery copy matches the patch and no job is under
    /// way, so the autosave timer can stop.
    settled: bool,
    /// What loaded differently, from the last load.
    load_notes: Vec<String>,
}

impl Document {
    /// A patch just loaded from `file`, or with no file.
    pub fn new(info: Metadata, file: Option<PathBuf>, stamp: Option<recovery::Stamp>, load_notes: Vec<String>) -> Self {
        Self {
            saved_info: info.clone(),
            info,
            credit_offered: false,
            file,
            stamp,
            shown_name: None,
            save_hint: None,
            saved_content: None,
            settled: false,
            load_notes,
        }
    }

    /// The name saved inside the patch.
    pub fn name(&self) -> &str {
        &self.info.name
    }

    /// The patch's metadata.
    pub fn info(&self) -> &Metadata {
        &self.info
    }

    /// Changes the patch's metadata. The change shows as unsaved work.
    pub fn edit_info(&mut self, edit: impl FnOnce(&mut Metadata)) {
        edit(&mut self.info);
        self.settled = false;
    }

    /// True when the metadata differs from the file.
    pub fn info_unsaved(&self) -> bool {
        self.info != self.saved_info
    }

    /// Starts a new patch with the user's defaults (patch-metadata.md,
    /// Defaults): their name as the only author and their default licence,
    /// when set. A new patch is not unsaved work, so this is its saved
    /// state too.
    pub fn start_new(&mut self, author: &str, licence: Option<&str>) {
        self.info.authors = loom_patch::metadata::tidy_authors([author]);
        self.info.license = licence.map(str::to_string);
        self.saved_info = self.info.clone();
    }

    /// True when Save should offer to add `name` to the authors: it is
    /// set, not in the list, and not offered yet for this patch (rule 3).
    /// Asking marks it offered.
    pub fn should_offer_credit(&mut self, name: &str) -> bool {
        if self.credit_offered || name.trim().is_empty() || loom_patch::metadata::has_author(&self.info.authors, name) {
            return false;
        }
        self.credit_offered = true;
        true
    }

    /// Forgets that credit was offered, when no dialog could ask: the next
    /// Save asks again.
    pub fn credit_not_asked(&mut self) {
        self.credit_offered = false;
    }

    /// The current file, if the patch has one.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// Where the save dialog starts: the current file, or the hint.
    pub fn save_start(&self) -> Option<&Path> {
        self.file.as_deref().or(self.save_hint.as_deref())
    }

    /// What loaded differently, from the last load.
    pub fn load_notes(&self) -> &[String] {
        &self.load_notes
    }

    /// The name of the patch as the title shows it, without the mark.
    pub fn title_name(&self) -> String {
        match (&self.file, &self.shown_name) {
            (None, Some(name)) => name.clone(),
            _ => files::display_name(self.file.as_deref()),
        }
    }

    /// Makes a patch opened from the bundled examples folder an untitled
    /// copy (files.md, Defaults for the dialogs): no current file,
    /// `<name> (example)` in the title, and a Save that asks for a place,
    /// suggesting the example's file name.
    fn copy_if_example(&mut self, examples: Option<&Path>) {
        let Some(path) = self.file.as_deref() else {
            return;
        };
        if !files::is_example(path, examples) {
            return;
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        let stem = stem.as_deref().unwrap_or("Untitled");
        // A bare file name: the dialog starts in the Documents folder.
        self.save_hint = Some(PathBuf::from(format!("{stem}.{}", files::EXTENSION)));
        self.shown_name = Some(format!("{stem} (example)"));
        self.file = None;
        self.stamp = None;
    }

    /// Notes that `patch` is what the file holds now, as after a load.
    pub fn mark_loaded(&mut self, patch: Patch) {
        self.saved_info = patch.metadata();
        self.saved_content = Some(patch);
    }

    /// Notes that `patch` was saved to `path`, which becomes the current
    /// file. The title and Save As hints from a copy no longer apply.
    pub fn saved(&mut self, path: PathBuf, patch: Patch) {
        self.stamp = recovery::Stamp::of(&path);
        self.file = Some(path);
        self.shown_name = None;
        self.save_hint = None;
        self.saved_info = patch.metadata();
        self.saved_content = Some(patch);
    }

    /// Takes the file from a recovery copy (autosave.md, After a restore):
    /// the file only if it is unchanged, otherwise a "(recovered)" name and
    /// a Save As next to it. Nothing on disk holds the restored patch.
    pub fn restore(&mut self, state: recovery::FileState) {
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
        self.saved_content = None;
    }

    /// Tells autosave to look again: something may have changed the patch.
    pub fn may_have_changed(&mut self) {
        self.settled = false;
    }

    /// Where Save As starts with no current file, for tests.
    #[cfg(test)]
    pub fn save_hint(&self) -> Option<&Path> {
        self.save_hint.as_deref()
    }

    /// Drops the current file, for tests.
    #[cfg(test)]
    pub fn forget_file(&mut self) {
        self.file = None;
    }
}

#[cfg(test)]
mod tests;
