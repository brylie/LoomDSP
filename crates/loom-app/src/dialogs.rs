//! Native dialogs in two turns (ui-layer.md, section 2, "Dialogs take two
//! turns").
//!
//! `update` never waits for a dialog. It returns the dialog as an effect,
//! and keeps here what waits on the answer: what the Open or Save dialog is
//! for, the save that waits for the credit question, and the crash copy
//! that the recovery question offers. The answer comes back as
//! [`Message::Dialog`] on a later turn, and is
//! applied to what waits.
//!
//! One dialog at a time: while one waits, a second is not asked, and the
//! request is dropped. Nothing can ask one meanwhile today, since the bridge
//! still shows each dialog to its end, but the `winit` shell keeps the
//! window running while one is open (plan step 2.2).

use crate::question::AfterDiscard;
use crate::{App, Args, Message, Source, files, recovery};
use loom_patch::Patch;
use loom_ui::Effects;
use loom_ui::effect::{Choice, FileRequest, Question};
use std::path::PathBuf;
use std::time::Instant;

/// What waits for a native dialog's answer.
#[derive(Debug)]
pub(crate) enum Waiting {
    /// The Open dialog: the patch picked opens.
    Open,
    /// The Save As dialog: the patch is written there, then `next` runs.
    SaveAs {
        /// What was asked for after the save, such as Close.
        next: Option<AfterDiscard>,
    },
    /// The Save dialog for a sub-patch going into the library.
    SaveToLibrary {
        /// The sub-patch as a patch file.
        file: Box<Patch>,
        /// Its name, for the status line.
        name: String,
    },
    /// The question to credit the user: the write to `path` waits for it.
    Credit {
        /// The user's name.
        name: String,
        /// Where the patch is being saved.
        path: PathBuf,
        /// What runs after the save.
        next: Option<AfterDiscard>,
    },
    /// The crash recovery question.
    Recovery(Box<RecoveryOffer>),
}

/// What waits, and the engine generation it was asked under: an answer
/// that arrives after New or Open replaced the patch belongs to the patch
/// it was asked about, which is gone.
#[derive(Debug)]
pub(crate) struct Pending {
    /// What waits.
    pub waiting: Waiting,
    /// `App::generation` when it was asked.
    pub generation: u64,
}

/// A dialog's answer.
#[derive(Debug, Clone)]
pub(crate) enum DialogAnswer {
    /// The path a file dialog picked, or `None` when it was cancelled.
    File(Option<PathBuf>),
    /// The button a question's dialog was closed with.
    Choice(Choice),
}

/// The patch the command line asked for, kept while the crash recovery
/// question waits, since the answer decides whether it opens.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StartPatch {
    /// An empty patch.
    Empty,
    /// A generated patch of this many modules (`--generate`).
    Generated(usize),
    /// A patch file.
    File(PathBuf),
}

impl StartPatch {
    /// The patch `args` asks for.
    fn of(args: &Args) -> Self {
        match args.source() {
            Source::Generated(n) => Self::Generated(n),
            Source::File(path) => Self::File(path.to_path_buf()),
            Source::Empty | Source::Recovered(_) => Self::Empty,
        }
    }

    /// Where it comes from, for loading.
    fn source(&self) -> Source<'_> {
        match self {
            Self::Empty => Source::Empty,
            Self::Generated(n) => Source::Generated(*n),
            Self::File(path) => Source::File(path),
        }
    }
}

/// A crash copy found at start, to offer once the window is open.
#[derive(Debug)]
pub(crate) struct RecoveryOffer {
    /// The copy.
    pub orphan: recovery::Orphan,
    /// The patch the command line asked for.
    start: StartPatch,
    /// When it was found: the question is due from then on.
    found: Instant,
}

impl RecoveryOffer {
    /// The offer of `orphan`, made at start with `args`.
    pub(crate) fn new(orphan: recovery::Orphan, args: &Args) -> Self {
        Self {
            orphan,
            start: StartPatch::of(args),
            found: Instant::now(),
        }
    }

    /// When the question is due.
    pub(crate) fn due(&self) -> Instant {
        self.found
    }
}

impl App {
    /// Adds `effects` to what this update returns. Code deep in a call
    /// chain that returns nothing of its own uses this.
    pub(crate) fn effect(&mut self, effects: Effects<Message>) {
        let mut all = std::mem::take(&mut self.effects);
        all.extend(effects);
        self.effects = all;
    }

    /// Shows a file dialog for `request`, keeping `waiting` until it
    /// answers. Returns false when another dialog waits, and this one is
    /// dropped.
    pub(crate) fn ask_file(&mut self, waiting: Waiting, request: FileRequest) -> bool {
        if self.waiting.is_some() {
            return false;
        }
        self.waiting = Some(Pending {
            waiting,
            generation: self.generation,
        });
        self.effect(Effects::file_dialog(request, |path| {
            Message::Dialog(DialogAnswer::File(path))
        }));
        true
    }

    /// Asks `question` in a native dialog, keeping `waiting` until it
    /// answers. Returns false when another dialog waits, and this one is
    /// dropped.
    pub(crate) fn ask_question(&mut self, waiting: Waiting, question: Question) -> bool {
        if self.waiting.is_some() {
            return false;
        }
        self.waiting = Some(Pending {
            waiting,
            generation: self.generation,
        });
        self.effect(Effects::ask(question, |choice| {
            Message::Dialog(DialogAnswer::Choice(choice))
        }));
        true
    }

    /// What waits for a dialog, if anything.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> Option<&Waiting> {
        self.waiting.as_ref().map(|pending| &pending.waiting)
    }

    /// Applies a dialog's answer to what waited for it. An answer with
    /// nothing waiting, or of the wrong kind, changes nothing. Nor does one
    /// for a patch that New or Open has replaced since: it would write the
    /// new patch where the old one was to go, or credit the wrong patch. The
    /// recovery question is the exception, as its answer is what replaces
    /// the patch.
    pub(crate) fn dialog_answered(&mut self, answer: DialogAnswer) {
        let Some(Pending { waiting, generation }) = self.waiting.take() else {
            return;
        };
        if generation != self.generation && !matches!(waiting, Waiting::Recovery(_)) {
            self.alerts
                .warn("The patch changed while the dialog was open, so its answer was not used.".to_string());
            return;
        }
        match (waiting, answer) {
            (Waiting::Open, DialogAnswer::File(path)) => {
                if let Some(path) = path {
                    self.used_folder_of(&path);
                    self.open_path(&path);
                }
            }
            (Waiting::SaveAs { next }, DialogAnswer::File(path)) => {
                self.save_as_chosen(files::saving(path), next);
            }
            (Waiting::SaveToLibrary { file, name }, DialogAnswer::File(path)) => {
                self.library_target_chosen(*file, name, files::saving(path));
            }
            (Waiting::Credit { name, path, next }, DialogAnswer::Choice(choice)) => {
                self.credit_answered(name, files::author_added(choice), path, next);
            }
            (Waiting::Recovery(offer), DialogAnswer::Choice(choice)) => {
                self.recovery_answered(*offer, recovery::choice(choice));
            }
            (waiting, answer) => {
                debug_assert!(false, "{answer:?} does not answer {waiting:?}");
            }
        }
    }

    /// The deadline that asks the crash recovery question: due at once,
    /// while a copy waits to be offered. None while another dialog waits:
    /// the question could not be asked yet, so a due deadline would only
    /// wake the loop again and again until that dialog answers.
    pub(crate) fn next_deadline(&self) -> Option<loom_ui::Deadline<Message>> {
        if self.waiting.is_some() {
            return None;
        }
        self.recovery_offer.as_ref().map(|offer| loom_ui::Deadline {
            at: offer.due(),
            message: Message::AskRecovery,
        })
    }

    /// Asks the crash recovery question, once the window is open.
    pub(crate) fn ask_recovery(&mut self) {
        if self.waiting.is_some() {
            return;
        }
        if let Some(offer) = self.recovery_offer.take() {
            let question = recovery::question(&offer.orphan);
            self.ask_question(Waiting::Recovery(Box::new(offer)), question);
        }
    }

    /// The answer to the crash recovery question: the copy opens, or the
    /// patch the command line asked for does (autosave.md, Crash recovery).
    fn recovery_answered(&mut self, offer: RecoveryOffer, choice: recovery::Choice) {
        let RecoveryOffer { orphan, start, .. } = offer;
        match choice {
            recovery::Choice::Restore => match orphan.load() {
                Ok(patch) => {
                    let state = orphan.file_state();
                    // The file is checked after the answer: it can change
                    // while the question is open.
                    match self.try_replace(Source::Recovered(Box::new(patch))) {
                        Ok(()) => self.restored(state, orphan, &start.source()),
                        // A copy that reads but does not load is a failed
                        // restore too: the orphan is kept, not adopted.
                        Err(
                            crate::file_commands::NotReplaced::NotLoaded(e)
                            | crate::file_commands::NotReplaced::NoDevice(e),
                        ) => {
                            self.restore_failed(&e, &orphan);
                        }
                    }
                }
                Err(e) => self.restore_failed(&e, &orphan),
            },
            recovery::Choice::Discard => {
                if let Err(e) = orphan.discard() {
                    self.alerts.warn(format!("Could not delete the recovery copy: {e}."));
                }
                self.open_start(&start);
            }
            recovery::Choice::NotNow => self.open_start(&start),
        }
    }

    /// Says that a restore failed, in the window and in the status line.
    /// Dropping the orphan keeps its folder for a later start.
    fn restore_failed(&mut self, error: &str, orphan: &recovery::Orphan) {
        let message = format!(
            "Could not restore the unsaved changes: {error}. The copy is kept at {}.",
            orphan.patch_path().display()
        );
        self.show_error(&message);
        self.alerts.warn(message);
    }

    /// Opens the patch the command line asked for, after the recovery
    /// question.
    fn open_start(&mut self, start: &StartPatch) {
        match start {
            StartPatch::Empty => {}
            StartPatch::Generated(_) => {
                self.replace(start.source());
            }
            StartPatch::File(path) => {
                self.open_path(path);
            }
        }
    }
}

/// The test driver for direct calls (`App::send`), and the answers it
/// gives.
#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests;
