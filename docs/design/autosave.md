# Autosave and Crash Recovery Specification

The desktop app keeps a recovery copy of the open patch while it has unsaved changes, and after a crash it offers to bring that copy back.

- **Kind:** living specification.
- **Status:** Draft
- **Decision behind it:** none of its own. Builds on [ADR-0008](../adr/0008-patch-format-ron.md) (the patch format) and [Open, Save, and Save As](files.md), which leaves autosave and crash recovery out of its scope.
- **Use case and requirements:** NFR-REL-01 (the app autosaves and recovers after a crash), UC3 (perform live).
- **Background:** none.
- **Prior art:** most desktop editors keep a hidden copy of unsaved work and offer it after a crash. We use only that convention: a quiet background copy, and a question at the next start.
- **Issues:** [#15](https://github.com/brylie/LoomDSP/issues/15).
- **Code:** none yet. It will live in `crates/loom-app` (a new `recovery.rs`), next to `files.rs`. It reuses `Patch::from_graph` and `Patch::save` from `loom-patch`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

> I have patched for an hour without saving. The app crashes, or my laptop runs out of battery while the app is open. When I start LoomDSP again, it tells me it found my unsaved patch from a few seconds before the crash, and one click brings it back.

> On stage the app crashes in the middle of a set. I restart it, press Restore, and I am playing the same patch again within seconds.

## Terms

| Term | Meaning |
|---|---|
| **Session** | One run of the app, from start to exit. Each session has its own recovery folder. |
| **Recovery copy** | The patch as it was at the last autosave, in the session's recovery folder. |
| **Clean exit** | The app closes through its own close path (window close, quit), after any question about unsaved changes. |
| **Orphan** | A recovery folder whose session is no longer running and did not exit cleanly. |
| **Current file** and **unsaved changes** | As in [files.md](files.md#terms). |

## Behaviour

### When autosave runs

The app writes a recovery copy **2 seconds after the last change**. If changes keep coming without a 2 second pause (a long knob turn, a long drag), it writes anyway **10 seconds after the first change it has not written yet**. So at most about 10 seconds of work can be lost.

| Value | Setting |
|---|---|
| Quiet time after the last change | 2 s |
| Longest wait while changes keep coming | 10 s |

*Why these values:* 2 seconds of quiet means the player has paused, so the write does not land in the middle of a gesture. The 10 second ceiling bounds the loss when a gesture never stops, for example a slow filter sweep during a set. Writes are small (a patch file is a few kilobytes) and happen on another thread, so this rate costs nothing the player can notice.

A **change** is anything that sets the unsaved mark ([files.md](files.md#what-counts-as-an-unsaved-change)): add, delete, connect, disconnect, parameter, assignment, MIDI mapping, and module moves. Undo and redo are changes too. A gesture still in progress also counts: the copy holds the patch as it is when the copy is taken, including a knob halfway through a turn or a module halfway through a drag. *Why:* a crash in the middle of a long gesture should still keep most of it.

Not changes: selection, pan, zoom, transport start or stop, playing notes, and Hold note. They are not in the patch file, so they do not start the timer.

**Only unsaved work is copied.** When the patch has no unsaved changes (just saved, just opened, or undone back to the saved state), the app deletes the session's recovery copy instead of writing one. *Why:* the file on disk already holds that patch. A copy of it would only cause a pointless question after a crash.

### Where the recovery copy lives

All sessions share one recovery folder, in the operating system's per-user data folder:

| OS | Folder |
|---|---|
| macOS | `~/Library/Application Support/LoomDSP/recovery/` |
| Windows | `%LOCALAPPDATA%\LoomDSP\recovery\` |
| Linux and other Unix | `$XDG_STATE_HOME/loomdsp/recovery/`, or `~/.local/state/loomdsp/recovery/` when `XDG_STATE_HOME` is unset or empty |

*Why these folders:* macOS and Windows use the same base folder as [preferences](preferences.md#where-the-file-lives), so all LoomDSP files on one machine are in one place. On Linux the XDG specification separates settings (config) from data the app keeps between runs (state); a recovery copy is state.

Inside it, **one folder per session**, named `<start time>-<process ID>`, for example `20261020T143005-48211`. Each session folder holds:

| File | Content |
|---|---|
| `lock` | Empty. The session holds an exclusive OS file lock on it for its whole run. |
| `patch.ron` | The recovery copy. An ordinary patch file: Open can load it by hand. Present only while there are unsaved changes. |
| `info.ron` | Facts about the copy (below). |

`info.ron`:

```ron
(
    version: 1,
    file: Some("/Users/me/Documents/drone.ron"),
    file_modified: Some("2026-10-20T14:02:11Z"),
    written: "2026-10-20T14:31:47Z",
)
```

| Field | Meaning |
|---|---|
| `file` | The current file when the copy was taken, or `None` for a patch never saved. |
| `file_modified` | The modification time of the current file when the session last opened or saved it. `None` when there is no current file. |
| `written` | When the copy was written. |

*Why one folder per session, not one per patch or one for all:* one window holds one patch, so a session and its patch are the same thing. A folder per session lets two app instances run side by side without overwriting each other's copy, and a patch never saved has no file name to key on. *Why a plain patch file:* if the app itself cannot start, a user can still find `patch.ron` and open it.

A command-line flag `--recovery-dir <folder>` uses another folder instead. *Why:* tests must not read or change the real folder, as with `--preferences`.

### How the app knows the last run crashed

At start, before the window opens, the app:

1. creates its own session folder and takes the lock on its `lock` file;
2. looks at every other session folder. If it can take that folder's lock, the session that owned it is no longer running. If it cannot, another instance is running, and the app leaves that folder alone.

A clean exit deletes the session's own folder, as the last thing it does. So a folder whose lock is free was left by a session that did not exit cleanly: a crash, a forced kill, or a power loss.

- An orphan **with** a `patch.ron` is offered for recovery (below).
- An orphan **without** a `patch.ron` had no unsaved work. The app deletes it without asking.

*Why a lock, not only a marker written at exit:* a marker alone cannot tell a crashed session from one that is still running in another instance. The operating system releases a file lock when a process ends, however it ends, so a free lock is reliable proof that the owner is gone.

### The offer at start

When there is an orphan with a recovery copy, the app shows a question after the window opens and before it loads any patch:

> **Restore unsaved changes to "drone.ron"?**
> LoomDSP did not close properly. A copy of your unsaved changes from 20 October, 14:31 was kept.
> **[Restore]  [Discard]  [Not Now]**

For a patch never saved, the name is `Untitled`. The time is `written`, in local time.

| Choice | Result |
|---|---|
| **Restore** | Loads the recovery copy as the open patch (see After a restore). |
| **Discard** | Deletes the orphan folder. Asks no second question. The app starts as it would without the offer. |
| **Not Now** | Leaves the orphan folder as it is. The app starts as it would without the offer, and the same question comes at the next start. |

*Why Not Now:* on stage, the player may want to start a known patch fast and look at the lost one later. Discard is final; Not Now is the safe default when unsure.

If the window is closed or the dialog cannot be shown, the result is **Not Now**. On Linux without `zenity` or `kdialog` (see [files.md](files.md#asking-before-losing-work)), the status bar says: `Unsaved changes from a crash were kept. Install zenity or kdialog to restore them, or open <path to patch.ron>.` *Why:* nothing is ever deleted without the user's choice.

**Several orphans** (two instances crashed, or the user chose Not Now before): the app asks about the newest one only. The others stay for later starts. *Why:* one window holds one patch, so one restore per start is all the app can do. A list would be a larger design (see Open points).

**A patch path on the command line:** the offer still comes first. Restore replaces the command-line patch, and the status bar says so. The other two choices load the command-line patch as usual. *Why:* the player who restarts after a crash usually starts the app the same way as before, and the copy has more recent work.

**Non-interactive runs** never ask and never touch other sessions' folders: `--screenshot`, `--dump-ui`, `--bench`, `--measure`, `--measure-idle`, `--control`, `--control-script`, and `--no-recovery`. They also write no recovery copy. *Why:* scripts and tests must not stop at a dialog, and must not use up or delete a real user's recovery copy.

### When the saved file is newer than the copy

Before asking, the app reads the modification time of `file`. If it is later than `file_modified`, the file was saved after the copy's session last saved or opened it: by another instance, by hand, or by a sync tool. The question then adds a line:

> "drone.ron" was saved again after this copy was taken. Restoring will not overwrite it.

And Restore opens the copy **without a current file** (see After a restore), so Save asks where to write instead of replacing the newer file.

If `file` no longer exists, the question adds: `"drone.ron" no longer exists.` Restore again opens the copy without a current file.

*Why:* the recovery copy is not always the newest work. The app does not guess which is right. It shows both facts and keeps both versions, so the player can compare them and choose.

### After a restore

| Restored patch | Current file | Unsaved mark | Title |
|---|---|---|---|
| Had a file, file unchanged since | The same file | Set | `* drone.ron - LoomDSP` |
| Had a file, file newer or missing | None | Set | `* drone (recovered) - LoomDSP` |
| Never saved | None | Set | `* Untitled - LoomDSP` |

- The patch is **always marked unsaved**. *Why:* it differs from what is on disk, or there is nothing on disk. The mark tells the player to save it.
- It **keeps its file path** only when the file is unchanged since the copy's session last touched it. Then Save writes the recovered work where the player expects. Otherwise there is no current file, and Save acts as Save As, starting in the file's folder.
- The undo history is empty. *Why:* the history is not saved in the copy. The patch counts as changed, as after `History::clear` ([files.md](files.md#what-counts-as-an-unsaved-change)).
- Positions come from the copy, as for Open. Selection, pan, and zoom are reset.
- Restore builds a new engine and controller, as Open does ([files.md](files.md#open-replaces-the-patch)). If the copy does not load, the app shows `Could not restore the unsaved changes: <the patch error>. The copy is kept at <path to patch.ron>.`, keeps the orphan folder, and starts empty.

**The orphan is deleted only after the new session has its own copy.** Right after a successful restore, the app writes the new session's recovery copy at once (no 2 second wait), and deletes the orphan folder when that write succeeds. *Why:* if the app crashes again during the restore, the work must still be somewhere.

### Writing without stalling the UI

1. On the UI thread, the app builds the patch data with `Patch::from_graph` and the module positions, as Save does. This is a copy of plain data and is fast.
2. It sends that data to a **recovery writer thread**: one plain `std::thread` that the app starts with the session and stops at exit. The channel holds one pending copy. A newer copy replaces an older one that the writer has not started.
3. The writer turns the patch into RON text and writes `patch.ron` through `Patch::save` (a temporary file and a rename, so a crash during the write leaves the old copy whole), then `info.ron` the same way.
4. The writer sends the result back to the UI thread as a message.

*Why a thread of its own:* file writes can block for a long time on a slow or network disk, and the UI thread must keep drawing. The audio thread is never involved (hard rule 1). *Why latest wins:* only the newest copy matters. A queue of old copies would only delay it.

Deleting `patch.ron` when the patch becomes saved goes through the same writer, in order with the writes. *Why:* a delete must never overtake a write that was sent before it.

Save, Open, and New stay on the UI thread as today ([files.md](files.md)). Only recovery writes use the writer thread.

### When a write fails

The first failure shows in the status bar, with no modal:

> Autosave failed: <system reason>. Your changes are not protected against a crash. Save to keep them.

The app keeps trying on the normal schedule. It does not repeat the message while writes keep failing, and says `Autosave works again.` after the first write that succeeds. *Why:* a full disk should not interrupt playing, but the player must know their safety net is gone.

If the recovery folder cannot be created or locked at start, autosave is off for the session and the status bar says: `Autosave is off: <system reason>.` The app still starts. *Why:* the app must reach a playing state on stage even when its data folder is broken, as with [preferences](preferences.md#reading-the-file).

### Clean exit

On window close or quit, after any "Save changes?" question ([files.md](files.md#asking-before-losing-work)):

1. the app stops the writer thread and waits for its current write, if any, to finish;
2. it deletes `patch.ron` and `info.ron`;
3. it closes the `lock` file, which releases the lock, and deletes the session folder.

*Why this order:* Windows cannot delete a file that is still open, so the lock is closed before the folder goes. The copy is deleted first, so another instance that takes the free lock in that moment finds no `patch.ron` and only removes an empty folder.

**Don't Save** counts as a clean exit: the user chose to drop the changes, so the copy goes with them. **Cancel** keeps the app open and changes nothing. If deleting the folder fails, the next start finds an orphan; if it holds a `patch.ron`, the user is asked about it. *Why:* a leftover question is better than lost work.

On New or Open, after the question about unsaved changes, the session's `patch.ron` is deleted, because the new patch starts with no unsaved changes. The session folder and lock stay. *Why:* the session goes on; only its patch changed.

## Rules

1. **Recovery never deletes work the user did not choose to drop.** Only Discard, Don't Save, a save, or a confirmed restore removes a recovery copy. A failed dialog, a closed dialog, or a failed restore keeps it. *Why:* the feature exists to stop silent loss; it must not cause any.
2. **Only unsaved work is copied.** *Why:* a copy of a saved patch would only add a pointless question after a crash.
3. **One recovery folder per session, held by an OS file lock.** *Why:* it tells a crashed session from a running one, and lets instances run side by side.
4. **Writing never stalls the UI and never touches the audio thread.** The UI thread only builds plain patch data; a writer thread does the I/O. *Why:* hard rule 1 for audio, and a frozen UI on stage is as bad as a crash.
5. **Writes are atomic and in order.** Each file is written through a temporary file and a rename, and writes and deletes reach the disk in the order they were sent. *Why:* a crash during a write must leave the previous copy whole.
6. **A restored patch is always marked unsaved, and never silently overwrites a newer file.** *Why:* the player decides which version wins.
7. **Recovery lives in `loom-app` only.** No engine crate reads or writes recovery files. *Why:* hard rule 6.
8. **Non-interactive runs neither ask nor touch recovery files.** *Why:* tests and scripts must not block on a dialog or consume a user's copy.

## Acceptance criteria

Tests point `--recovery-dir` (or the function behind it) at a temporary folder. They never touch the real folder.

- [ ] Given a clean patch, when a knob turns once, then `patch.ron` appears 2 s later (test with a fake clock), and loading it gives the new knob value.
- [ ] Given changes every 500 ms without pause, then a copy is written no later than 10 s after the first change.
- [ ] Given selection, pan, zoom, transport, or note changes only, then no copy is written.
- [ ] Given unsaved changes and a copy on disk, when the patch is saved, then `patch.ron` is deleted. When an edit is made and undone back to the saved state, `patch.ron` is deleted at the next autosave.
- [ ] Given a session folder whose lock is held by another process, when the app starts, then it does not offer, change, or delete that folder.
- [ ] Given an orphan folder with a free lock and a `patch.ron`, when the app starts, then the offer appears with the file name and the `written` time.
- [ ] Given an orphan folder with a free lock and no `patch.ron`, when the app starts, then it is deleted without a question.
- [ ] Given the offer, when Discard is chosen, the orphan folder is gone. When Not Now is chosen, the folder is unchanged and the offer appears at the next start.
- [ ] Given an orphan with a current file unchanged since, when restored, then the current file is that file and the title shows the unsaved mark.
- [ ] Given an orphan whose current file was modified after `file_modified`, then the offer carries the "saved again" line, and after Restore there is no current file and the saved file is unchanged.
- [ ] Given an orphan for a never-saved patch, when restored, then the title is `* Untitled - LoomDSP`.
- [ ] After a restore, the new session's `patch.ron` exists before the orphan folder is deleted.
- [ ] Given an orphan whose `patch.ron` does not parse, when Restore is chosen, then the error appears, the app starts empty, and the orphan folder is unchanged.
- [ ] Given a recovery folder that cannot be written, when a change is made, then the status bar shows the failure once and the app keeps running.
- [ ] Given `--screenshot`, `--dump-ui`, `--control`, or `--no-recovery`, when an orphan exists, then no dialog appears and the orphan is unchanged.
- [ ] On clean exit, and on exit after Don't Save, the session folder is deleted.
- [ ] Given the default paths, then on each OS the folder matches the table in Where the recovery copy lives. (Unit test of the path function, with the environment variables set by the test.)
- [ ] With a 300-module patch (`--generate 300`), building the copy on the UI thread takes under 1 ms (benchmark, in [p1-canvas.md](../benchmarks/p1-canvas.md) or its own note).
- [ ] **Manual check.** On macOS, Windows, and Linux: make changes, wait 3 s, kill the app with the OS (Activity Monitor, Task Manager, `kill -9`). Start it again: the offer appears, Restore brings back the changes, and the title shows the unsaved mark.
- [ ] **Manual check.** On a slow disk (a network share or USB stick as `--recovery-dir`), turning a knob continuously for 30 s keeps the canvas responsive.

## Out of scope

- Autosaving to the patch's own file. Autosave only writes recovery copies; the user's file changes only when they save. *Why:* the user decides when their file changes.
- Restoring the undo history.
- A list of all orphans to restore or discard at once (see Open points).
- Recovery for several windows (there is one window per app; see [files.md](files.md#out-of-scope)).
- Survival of a copy written just before an OS crash or power loss. The atomic rename keeps the old copy whole, but the last copy may not have reached the disk. Flushing to disk is an open point.
- A preference to change the timing or turn autosave off. The [preferences](preferences.md) window can gain one later.
- Recovery in the web runtime and in `loom play` and `loom render`.

## Open points

1. **File lock API.** `std::fs::File::try_lock` is stable from Rust 1.89, and the workspace's `rust-version` is 1.88. *Leaning:* raise `rust-version` to 1.89 (mise already installs the latest Rust). The other option is a small lock crate under MIT or Apache-2.0, checked with `cargo deny`.
2. **Folder crate.** The same choice as preferences, plus the Linux state folder. *Leaning:* the [`dirs`](https://crates.io/crates/dirs) crate (its `state_dir` gives the XDG state folder on Linux), shared with preferences.
3. **Several orphans.** *Leaning:* offer the newest only, as above. If users collect many orphans through Not Now, add a "Recover unsaved patches..." window that lists them.
4. **Old orphans.** Not Now keeps a copy for ever. *Leaning:* keep them; a few kilobytes each. Revisit if the list window (point 3) comes.
5. **Flush to disk.** `Patch::save` does not call `fsync`, so a power loss can lose the last copy, or on some file systems leave an empty file after the rename. *Leaning:* the writer calls `sync_all` on the temporary file before the rename. It runs off the UI thread, so its cost does not matter, and the requirement names battery loss as a case.
6. **Panic in UI code.** NFR-REL-02 asks that a UI panic does not stop audio. If the app later survives UI panics, it could also write a copy at once from the panic hook. *Leaning:* not in this spec; the 10 s bound is enough for now.
7. **Restore and the command-line patch.** *Leaning:* the offer comes first and Restore wins, as above. The other option is to skip the offer when a patch is given on the command line, which helps scripts but risks the stage case.
