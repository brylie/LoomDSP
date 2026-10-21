//! Autosave and crash recovery (`docs/design/autosave.md`).
//!
//! While the open patch has unsaved changes, a copy of it is kept in a
//! folder of this session's own, under the recovery folder. A writer thread
//! does the file work, so the UI thread never waits on a disk. At start,
//! folders left by sessions that did not exit cleanly are found, and the
//! newest copy is offered back.
//!
//! Each session holds an OS file lock on its folder's `lock` file for its
//! whole run. The OS releases it when the process ends, however it ends, so
//! a folder whose lock is free belongs to a session that is gone.

use crate::files;
use loom_patch::Patch;
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

/// Quiet time after the last change before a copy is written.
pub const QUIET: Duration = Duration::from_secs(2);
/// Longest wait for a copy while changes keep coming.
pub const LONGEST: Duration = Duration::from_secs(10);
/// Wait before trying again after a failed write.
pub const RETRY: Duration = Duration::from_secs(10);
/// Longest wait at exit for a write in progress.
pub const EXIT_WAIT: Duration = Duration::from_secs(2);
/// How often the app checks whether a copy is due.
pub const TICK: Duration = Duration::from_millis(500);

const PATCH: &str = "patch.ron";
const INFO: &str = "info.ron";
const LOCK: &str = "lock";
const SCAN_LOCK: &str = "scan.lock";

/// The per-user recovery folder (autosave.md, Where the recovery copy
/// lives).
pub fn default_root() -> Result<PathBuf, String> {
    use etcetera::BaseStrategy;
    let home = |_| "no home folder".to_string();
    #[cfg(target_os = "macos")]
    let root = etcetera::base_strategy::Apple::new()
        .map_err(home)?
        .data_dir()
        .join("LoomDSP");
    #[cfg(windows)]
    let root = etcetera::base_strategy::Windows::new()
        .map_err(home)?
        .cache_dir()
        .join("LoomDSP");
    #[cfg(not(any(target_os = "macos", windows)))]
    let root = etcetera::base_strategy::Xdg::new()
        .map_err(home)?
        .state_dir()
        .ok_or("no state folder")?
        .join("loomdsp");
    Ok(root.join("recovery"))
}

/// A file's modification time and size, to tell whether it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub modified: SystemTime,
    pub size: u64,
}

impl Stamp {
    /// The stamp of the file at `path` now, or `None` if it cannot be read.
    pub fn of(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        Some(Self {
            modified: meta.modified().ok()?,
            size: meta.len(),
        })
    }
}

/// `info.ron`: facts about the copy next to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    pub version: u32,
    /// The current file when the copy was taken.
    pub file: Option<PathBuf>,
    /// The file's modification time when the session last opened or saved it.
    pub file_modified: Option<String>,
    /// The file's size at that moment.
    pub file_size: Option<u64>,
    /// When the copy was written.
    pub written: String,
}

impl Info {
    pub fn new(file: Option<&Path>, stamp: Option<Stamp>, written: SystemTime) -> Self {
        let stamp = file.and(stamp);
        Self {
            version: 1,
            // Absolute, so a later start from another folder finds it.
            file: file.map(|f| std::path::absolute(f).unwrap_or_else(|_| f.to_path_buf())),
            file_modified: stamp.and_then(|s| timestamp(s.modified)),
            file_size: stamp.map(|s| s.size),
            written: timestamp(written).unwrap_or_default(),
        }
    }

    /// The stamp the file had, as recorded.
    fn stamp(&self) -> Option<Stamp> {
        Some(Stamp {
            modified: parse_time(self.file_modified.as_deref()?)?,
            size: self.file_size?,
        })
    }
}

/// A time as RFC 3339 text in UTC, the form `info.ron` keeps.
fn timestamp(time: SystemTime) -> Option<String> {
    jiff::Timestamp::try_from(time).ok().map(|t| t.to_string())
}

fn parse_time(text: &str) -> Option<SystemTime> {
    text.parse::<jiff::Timestamp>().ok().map(SystemTime::from)
}

/// A time as the offer shows it, in local time: "20 October, 14:31".
pub fn local_time(time: SystemTime) -> String {
    match jiff::Timestamp::try_from(time) {
        Ok(t) => t
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%-d %B, %H:%M")
            .to_string(),
        Err(_) => "an unknown time".to_string(),
    }
}

/// What became of the file the copy was taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    /// The patch was never saved, or the copy says nothing about its file.
    NeverSaved,
    /// The file has the time and size it had when the copy's session last
    /// opened or saved it.
    Unchanged(PathBuf),
    /// The file was changed since.
    Changed(PathBuf),
    /// The file is gone.
    Missing(PathBuf),
}

/// A session folder left by a session that did not exit cleanly, with a
/// copy in it. Its lock is held while this value lives, so no other
/// instance offers or deletes it; dropping it releases the lock and leaves
/// the folder for a later start.
#[derive(Debug)]
pub struct Orphan {
    dir: PathBuf,
    lock: Option<File>,
    info: Option<Info>,
    written: SystemTime,
}

impl Orphan {
    /// The copy.
    pub fn patch_path(&self) -> PathBuf {
        self.dir.join(PATCH)
    }

    /// The name the offer shows: the file name, or "Untitled".
    pub fn name(&self) -> String {
        files::display_name(self.info.as_ref().and_then(|i| i.file.as_deref()))
    }

    /// When the copy was written.
    pub fn written(&self) -> SystemTime {
        self.written
    }

    /// Reads the copy's file now. A copy without a readable `info.ron` is
    /// treated as a patch never saved, so it cannot be paired with the
    /// wrong file.
    pub fn file_state(&self) -> FileState {
        let Some(info) = &self.info else {
            return FileState::NeverSaved;
        };
        let Some(file) = &info.file else {
            return FileState::NeverSaved;
        };
        match Stamp::of(file) {
            None if !file.exists() => FileState::Missing(file.clone()),
            Some(now) if Some(now) == info.stamp() => FileState::Unchanged(file.clone()),
            _ => FileState::Changed(file.clone()),
        }
    }

    /// The offer's title and text (autosave.md, The offer at start).
    pub fn question(&self) -> (String, String) {
        let name = self.name();
        let title = format!("Restore unsaved changes to \"{name}\"?");
        let mut text = format!(
            "LoomDSP did not close properly. A copy of your unsaved changes from {} was kept.",
            local_time(self.written)
        );
        match self.file_state() {
            FileState::Changed(_) => {
                text.push_str(&format!(
                    "\n\n\"{name}\" has changed since this copy was taken. Restoring will not overwrite it."
                ));
            }
            FileState::Missing(_) => text.push_str(&format!("\n\n\"{name}\" no longer exists.")),
            FileState::NeverSaved | FileState::Unchanged(_) => {}
        }
        (title, text)
    }

    /// Reads the copy.
    pub fn load(&self) -> Result<Patch, String> {
        Patch::load(self.patch_path()).map_err(|e| e.to_string())
    }

    /// Deletes the folder: the user chose Discard, or the copy now lives on
    /// in a new session.
    pub fn discard(mut self) -> io::Result<()> {
        remove_copy(&self.dir)?;
        // Windows cannot delete a file that is still open.
        self.lock = None;
        fs::remove_dir_all(&self.dir)
    }
}

/// What the user chose when offered a copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Restore,
    Discard,
    NotNow,
    /// No dialog could be shown (Linux without `zenity` or `kdialog`).
    Unavailable,
}

/// How the app talks to the user at start. Tests answer without dialogs.
pub trait Prompt {
    fn ask(&mut self, orphan: &Orphan) -> Choice;
    fn error(&mut self, message: &str);
}

/// The native dialogs.
pub struct Dialogs;

impl Prompt for Dialogs {
    fn ask(&mut self, orphan: &Orphan) -> Choice {
        if !files::dialogs_available() {
            return Choice::Unavailable;
        }
        let (title, text) = orphan.question();
        let result = MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title(title)
            .set_description(text)
            .set_buttons(MessageButtons::YesNoCancelCustom(
                "Restore".to_string(),
                "Discard".to_string(),
                "Not Now".to_string(),
            ))
            .show();
        match result {
            MessageDialogResult::Custom(label) if label == "Restore" => Choice::Restore,
            MessageDialogResult::Custom(label) if label == "Discard" => Choice::Discard,
            MessageDialogResult::Yes => Choice::Restore,
            MessageDialogResult::No => Choice::Discard,
            // A closed dialog keeps the copy.
            _ => Choice::NotNow,
        }
    }

    fn error(&mut self, message: &str) {
        files::show_error(message);
    }
}

/// What [`start`] found.
pub struct Startup {
    pub session: Session,
    /// The newest orphan with a copy, to offer. Older ones are left for
    /// later starts.
    pub orphan: Option<Orphan>,
}

/// Opens this run's session folder under `root` and looks for orphans
/// (autosave.md, How the app knows the last run crashed).
pub fn start(root: &Path) -> Result<Startup, String> {
    start_with(root, Box::new(perform))
}

fn start_with(root: &Path, perform: Perform) -> Result<Startup, String> {
    let failed = |what: &str, path: &Path, e: io::Error| format!("cannot {what} {}: {e}", path.display());
    fs::create_dir_all(root).map_err(|e| failed("create", root, e))?;
    let scan_path = root.join(SCAN_LOCK);
    let scan = lock_file(&scan_path).map_err(|e| failed("open", &scan_path, e))?;
    // Waits for an instance starting at the same moment to finish its scan.
    scan.lock().map_err(|e| failed("lock", &scan_path, e))?;

    let dir = new_session_dir(root).map_err(|e| failed("create a folder in", root, e))?;
    let lock_path = dir.join(LOCK);
    let lock = lock_file(&lock_path).map_err(|e| failed("create", &lock_path, e))?;
    lock.try_lock().map_err(|e| failed("lock", &lock_path, e.into()))?;

    let mut orphans = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| failed("read", root, e))?.flatten() {
        let other = entry.path();
        if other == dir || !other.is_dir() {
            continue;
        }
        // A folder without a lock file was left before its session took
        // its lock: it is an orphan too.
        let Ok(other_lock) = lock_file(&other.join(LOCK)) else {
            continue;
        };
        if other_lock.try_lock().is_err() {
            // Another instance is running, or the lock cannot be taken:
            // leave the folder alone.
            continue;
        }
        if other.join(PATCH).is_file() {
            orphans.push(Orphan::found(other, other_lock));
        } else {
            // No unsaved work was left there.
            let _ = remove_copy(&other);
            drop(other_lock);
            let _ = fs::remove_dir_all(&other);
        }
    }
    drop(scan);

    orphans.sort_by_key(|o| std::cmp::Reverse(o.written));
    // The rest are released, and offered at later starts.
    let orphan = (!orphans.is_empty()).then(|| orphans.remove(0));
    Ok(Startup {
        session: Session::new(dir, lock, perform),
        orphan,
    })
}

impl Orphan {
    fn found(dir: PathBuf, lock: File) -> Self {
        let info = fs::read_to_string(dir.join(INFO))
            .ok()
            .and_then(|text| ron::from_str::<Info>(&text).ok());
        let written = info
            .as_ref()
            .and_then(|i| parse_time(&i.written))
            .or_else(|| fs::metadata(dir.join(PATCH)).and_then(|m| m.modified()).ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        Self {
            dir,
            lock: Some(lock),
            info,
            written,
        }
    }
}

/// Opens (or creates) a file to lock.
fn lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).truncate(false).write(true).open(path)
}

/// Creates a folder named `<start time>-<process ID>`.
fn new_session_dir(root: &Path) -> io::Result<PathBuf> {
    let base = format!(
        "{}-{}",
        jiff::Timestamp::now().strftime("%Y%m%dT%H%M%S"),
        std::process::id()
    );
    let mut n = 0;
    loop {
        let name = if n == 0 { base.clone() } else { format!("{base}-{n}") };
        let dir = root.join(name);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            // Two sessions in one process in the same second (tests).
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Deletes a copy: `info.ron` first, then `patch.ron`, so a crash between
/// the two never leaves an `info.ron` that names another patch's file.
fn remove_copy(dir: &Path) -> io::Result<()> {
    for name in [INFO, PATCH] {
        match fs::remove_file(dir.join(name)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Work for the writer thread.
#[derive(Debug)]
pub enum Job {
    Write(Box<Patch>, Info),
    Delete,
}

/// Does a job in a session folder. Tests replace it to block or fail.
pub type Perform = Box<dyn FnMut(&Path, &Job) -> Result<(), String> + Send>;

fn perform(dir: &Path, job: &Job) -> Result<(), String> {
    match job {
        Job::Write(patch, info) => {
            // A copy for another file goes first, so a crash between the
            // two writes below can never pair this patch with that file.
            // A pending delete that this write replaced lands here.
            let old = fs::read_to_string(dir.join(INFO))
                .ok()
                .and_then(|text| ron::from_str::<Info>(&text).ok());
            if old.is_none_or(|old| old.file != info.file) {
                remove_copy(dir).map_err(|e| e.to_string())?;
            }
            patch.save(dir.join(PATCH)).map_err(|e| e.to_string())?;
            let text = ron::ser::to_string_pretty(info, ron::ser::PrettyConfig::new()).map_err(|e| e.to_string())?;
            write_atomic(&dir.join(INFO), &text).map_err(|e| e.to_string())
        }
        Job::Delete => remove_copy(dir).map_err(|e| e.to_string()),
    }
}

/// Writes through a temporary file and a rename, so a crash never leaves
/// half a file.
pub(crate) fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.tmp"));
    fs::write(&temp, text)?;
    fs::rename(&temp, path)
}

#[derive(Default)]
struct WriterState {
    /// The next job. A newer one replaces it if the writer has not started it.
    job: Option<(u64, Job)>,
    busy: bool,
    stop: bool,
    done: Vec<(u64, Result<(), String>)>,
    /// Set at clean exit: the session's lock, closed by the thread before
    /// it deletes the folder.
    finish: Option<File>,
    /// Set once the thread ended its clean-exit work: true if it deleted
    /// the folder. A failed delete ends the wait too.
    finished: Option<bool>,
}

struct Shared {
    state: Mutex<WriterState>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, WriterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The recovery writer thread.
struct Writer {
    shared: Arc<Shared>,
    next: u64,
}

impl Writer {
    fn spawn(dir: PathBuf, mut perform: Perform) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(WriterState::default()),
            changed: Condvar::new(),
        });
        let thread = shared.clone();
        std::thread::Builder::new()
            .name("loom-recovery".to_string())
            .spawn(move || {
                loop {
                    let (id, job) = {
                        let mut state = thread.lock();
                        loop {
                            if state.stop {
                                return;
                            }
                            if let Some(lock) = state.finish.take() {
                                drop(state);
                                // On this thread, so a slow disk cannot
                                // hold up the window closing.
                                let _ = remove_copy(&dir);
                                // Windows cannot delete a file that is still open.
                                drop(lock);
                                let deleted = fs::remove_dir_all(&dir).is_ok();
                                let mut state = thread.lock();
                                state.finished = Some(deleted);
                                thread.changed.notify_all();
                                return;
                            }
                            if let Some(job) = state.job.take() {
                                state.busy = true;
                                break job;
                            }
                            state = thread.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
                        }
                    };
                    let result = perform(&dir, &job);
                    let mut state = thread.lock();
                    state.busy = false;
                    state.done.push((id, result));
                    thread.changed.notify_all();
                }
            })
            .expect("the recovery writer thread starts");
        Self { shared, next: 0 }
    }

    /// Hands a job to the thread, in place of any it has not started.
    fn send(&mut self, job: Job) -> u64 {
        self.next += 1;
        self.shared.lock().job = Some((self.next, job));
        self.shared.changed.notify_all();
        self.next
    }

    fn take_done(&self) -> Vec<(u64, Result<(), String>)> {
        std::mem::take(&mut self.shared.lock().done)
    }

    /// Clean exit: drops any job not started, lets the thread finish the
    /// one in progress, then has it close `lock` and delete the folder.
    /// Waits up to `wait` for all of that, and returns false if the folder
    /// was not deleted in time.
    fn finish(&self, lock: File, wait: Duration) -> bool {
        let mut state = self.shared.lock();
        state.job = None;
        state.finish = Some(lock);
        self.shared.changed.notify_all();
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, wait, |s| s.finished.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        state.finished == Some(true)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.changed.notify_all();
    }
}

/// What the schedule asks the writer to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Write(Patch),
    Delete,
}

/// When to write or delete the copy (autosave.md, When autosave runs).
/// Plain state and times, so tests drive it with a fake clock.
#[derive(Debug, Default)]
pub struct Schedule {
    /// The target at the last tick: the patch to keep, or `None` when the
    /// patch has no unsaved changes.
    seen: Option<Option<Patch>>,
    last_change: Option<Instant>,
    /// The first change not yet on disk.
    first_unwritten: Option<Instant>,
    /// What the folder holds, as far as the writer reported.
    on_disk: Option<Patch>,
    /// True after a failed job: it may have changed the folder part way,
    /// so `on_disk` is not trusted until a job succeeds.
    unknown: bool,
    /// The job sent and not yet reported, with its target.
    sending: Option<(u64, Option<Patch>)>,
    /// After a failure, no attempt before this time.
    retry_at: Option<Instant>,
    /// Write at the next tick, without the quiet time.
    write_now: bool,
}

impl Schedule {
    /// Called on every tick with what the folder should hold. Returns the
    /// job to send, if one is due.
    pub fn tick(&mut self, now: Instant, target: Option<Patch>) -> Option<Action> {
        if self.seen.as_ref() != Some(&target) {
            self.last_change = Some(now);
            self.seen = Some(target.clone());
        }
        if !self.unknown && target == self.on_disk {
            self.first_unwritten = None;
            return None;
        }
        let first = *self.first_unwritten.get_or_insert(now);
        if self.sending.as_ref().is_some_and(|(_, t)| *t == target) {
            return None;
        }
        if self.retry_at.is_some_and(|at| now < at) {
            return None;
        }
        match target {
            // Only unsaved work is copied: delete at once.
            None => Some(Action::Delete),
            Some(patch) => {
                let quiet = self.last_change.is_some_and(|t| now.duration_since(t) >= QUIET);
                let overdue = now.duration_since(first) >= LONGEST;
                (quiet || overdue || self.write_now).then(|| {
                    self.write_now = false;
                    // Changes made from here on wait at most another 10 s,
                    // so a long gesture writes every 10 s, not every tick.
                    self.first_unwritten = Some(now);
                    Action::Write(patch)
                })
            }
        }
    }

    /// Notes that a job for `target` was sent.
    pub fn sent(&mut self, id: u64, target: Option<Patch>) {
        self.sending = Some((id, target));
    }

    /// Notes the writer's report on a job.
    pub fn done(&mut self, id: u64, ok: bool, now: Instant) {
        let Some((sent, target)) = self.sending.take() else {
            return;
        };
        if sent != id {
            // An older job finished; the newer one is still on its way.
            self.sending = Some((sent, target));
            return;
        }
        if ok {
            self.on_disk = target;
            self.unknown = false;
            self.retry_at = None;
        } else {
            self.unknown = true;
            self.retry_at = Some(now + RETRY);
        }
    }

    /// Writes at the next tick, without waiting for quiet.
    pub fn write_now(&mut self) {
        self.write_now = true;
    }
}

/// This run's recovery folder, its lock, and its writer.
pub struct Session {
    /// The session folder. The writer owns the work on it; tests look in.
    #[cfg(test)]
    dir: PathBuf,
    lock: Option<File>,
    writer: Writer,
    schedule: Schedule,
    /// True from a failed write until one succeeds.
    failing: bool,
    /// The orphan a restore came from. Deleted once this session's own
    /// copy is on disk.
    restored_from: Option<Orphan>,
}

impl Session {
    fn new(dir: PathBuf, lock: File, perform: Perform) -> Self {
        Self {
            writer: Writer::spawn(dir.clone(), perform),
            #[cfg(test)]
            dir,
            lock: Some(lock),
            schedule: Schedule::default(),
            failing: false,
            restored_from: None,
        }
    }

    /// The session folder.
    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Keeps the orphan a restore came from until this session has a copy,
    /// and writes that copy at the next tick.
    pub fn adopt(&mut self, orphan: Orphan) {
        self.restored_from = Some(orphan);
        self.schedule.write_now();
    }

    /// Reads the writer's reports, then sends a job if one is due. `target`
    /// is the patch to keep, or `None` when nothing needs keeping. Returns
    /// lines for the status bar.
    pub fn tick(&mut self, now: Instant, target: Option<Patch>, info: impl FnOnce() -> Info) -> Vec<String> {
        let mut status = Vec::new();
        for (id, result) in self.writer.take_done() {
            self.schedule.done(id, result.is_ok(), now);
            match result {
                Ok(()) => {
                    if self.failing {
                        self.failing = false;
                        status.push("Autosave works again.".to_string());
                    }
                    if let Some(orphan) = self.restored_from.take()
                        && let Err(e) = orphan.discard()
                    {
                        status.push(format!("Could not delete the old recovery copy: {e}"));
                    }
                }
                Err(e) => {
                    if !self.failing {
                        self.failing = true;
                        status.push(format!(
                            "Autosave failed: {e}. Your changes are not protected against a crash. Save to keep them."
                        ));
                    }
                }
            }
        }
        if let Some(action) = self.schedule.tick(now, target.clone()) {
            let job = match action {
                Action::Write(patch) => Job::Write(Box::new(patch), info()),
                Action::Delete => Job::Delete,
            };
            let id = self.writer.send(job);
            self.schedule.sent(id, target);
        }
        status
    }

    /// Clean exit: the writer finishes its job in progress and deletes the
    /// folder. If that takes longer than [`EXIT_WAIT`] (a stuck write, a
    /// slow disk), the app goes on without it: the folder is left for the
    /// next start, and this returns false.
    pub fn finish(self) -> bool {
        self.finish_within(EXIT_WAIT)
    }

    fn finish_within(mut self, wait: Duration) -> bool {
        match self.lock.take() {
            Some(lock) => self.writer.finish(lock, wait),
            None => false,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
