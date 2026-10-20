# App Preferences Specification

A per-machine preferences file and window for the desktop app, so a player sets things like output safety once and the app remembers them.

- **Kind:** living specification.
- **Status:** Draft
- **Decision behind it:** None. Builds on [ADR-0019](../adr/0019-output-safety-stage.md), which says the output safety mode belongs to the machine, not the patch.
- **Use case and requirements:** UC3 (perform live), FR-APP-01 (desktop app), FR-PERF-08, NFR-SND-02.
- **Background:** [Output safety explainer](../learn/output-safety.md), for the first setting.
- **Prior art:** most desktop apps keep settings in a file in the operating system's per-user settings folder, and open a settings window with Cmd+, on macOS. We use only those conventions.
- **Issues:** follows [#131](https://github.com/brylie/LoomDSP/issues/131). No issue yet.
- **Code:** none yet. Today the mode is the `--output-safety` flag in `crates/loom-app/src/main.rs` (`Args`). The app passes it into every `EngineConfig` it builds.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

"My audio interface sends control voltages to my modular, so I run with output safety off. I don't want to type a flag every time I start LoomDSP. And when I play through the PA at a gig, I want to turn safety back on from inside the app, without restarting."

The app needs one place for settings that belong to this computer and its hardware, not to a patch. Output safety is the first. Audio and MIDI devices, and others, can follow later (see Out of scope).

## Terms

- **Preference:** a setting that belongs to the machine (the operating-system user account on this computer). It is never stored in a patch.
- **Preferences file:** the file that holds the preferences.
- **Saved value:** the value in the preferences file.
- **Effective value:** the value the app uses now. It is the saved value, unless a command-line flag sets it for this run.

## Behaviour

### Where the file lives

One file, `preferences.ron`, in the operating system's per-user settings folder:

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/LoomDSP/preferences.ron` |
| Windows | `%LOCALAPPDATA%\LoomDSP\preferences.ron` |
| Linux and other Unix | `$XDG_CONFIG_HOME/loomdsp/preferences.ron`, or `~/.config/loomdsp/preferences.ron` when `XDG_CONFIG_HOME` is unset or empty |

The app creates the folder when it first writes the file.

A command-line flag `--preferences <file>` uses another file instead. *Why:* tests must not read or change the real file, and a player may keep one file per rig (for example "studio" and "stage").

### Format

RON, like patches (ADR-0008):

```ron
(
    version: 1,
    output_safety: "low-latency",
)
```

| Field | Values | Default |
|---|---|---|
| `version` | `1` | `1` |
| `output_safety` | `"low-latency"`, `"clean"`, `"off"` | `"low-latency"` |

Values use the same spelling as the `--output-safety` flag. *Why:* one name for each mode in the docs, the flag, and the file, so a player who edits the file by hand does not have to learn a second spelling.

A missing field takes its default. *Why:* a file written by this version stays valid when a later version adds fields, and a hand-written file can hold only the field the player cares about.

### Reading the file

The app reads the file once, at start, before it builds the first engine.

| Situation | What the app does | What the player sees |
|---|---|---|
| No file | Uses the defaults. Does not create the file. | Nothing. |
| File reads and parses | Uses its values. | Nothing. |
| Cannot read (no permission, a folder in its place) | Uses the defaults. | Status bar: `Could not read preferences "<path>": <system reason>. Using defaults.` |
| Does not parse | Uses the defaults for every field. | Status bar: `Could not read preferences "<path>": <the parse error, with line when known>. Using defaults.` |
| Parses, but one field has a value the app does not know (for example `output_safety: "loud"`) | Uses the default for that field and the file's values for the others. | Status bar: `Preferences: unknown output_safety "loud". Using "low-latency".` |
| Field the app does not know | Ignores it. | Status bar: `Preferences: ignored unknown setting "<name>".` |

A problem with the file never stops the app from starting, and never shows a modal. *Why:* on stage, the player must get to a playing app. A broken settings file is not worth blocking that, and the status bar is enough to notice it later.

When a field falls back to its default, it falls back to the safe choice. For output safety that is Low latency, so a broken file turns protection on, never off. *Why:* the stage exists for the case nobody planned for (ADR-0019, rule 1).

### Writing the file

- The app writes the file only when the player changes a preference. It never writes at start or at exit. *Why:* a player who never opens the window never gets a file, and a read-only home folder causes no errors.
- It writes the whole file, with every field, including defaults. *Why:* a player who opens the file sees every setting there is.
- It writes to a temporary file in the same folder and renames it, as Save does for patches ([files.md](files.md)). *Why:* a crash or a full disk cannot leave half a file.
- If the file could not be parsed at start, the first write first renames the old file to `<name>.broken.ron` in the same folder, where `<name>` is the file's name without `.ron`. So `preferences.ron` becomes `preferences.broken.ron`, and `--preferences stage.ron` gives `stage.broken.ron`. This replaces an older backup of the same file only. *Why:* the player may have edited it by hand. Their edits should not vanish just because they changed a setting, and one rig's backup must not overwrite another's.
- If the write fails, the new value still applies for this run. Status bar: `Could not save preferences "<path>": <system reason>. The change applies until you quit.`

### The command-line flag

`--output-safety low-latency|clean|off` stays. It sets the effective value for this run only and does not change the saved value. The flag has no default value. When it is left out, the saved value applies. When it is given, even as `low-latency`, it wins. *Why:* today the flag defaults to `low-latency`, so the app could not tell "not given" from "given as low-latency", and the saved value would never apply.

| Flag | Saved value | Effective value | File after the run |
|---|---|---|---|
| none | `clean` | Clean | `clean` |
| `--output-safety off` | `clean` | Off | `clean` |
| none | no file | Low latency | no file |

*Why the flag wins:* a flag is a deliberate choice for this one run, typed now. It is also how a player rescues a run without editing files, and how scripts and tests get a known mode whatever the machine has saved.

If the player changes the setting in the Preferences window during a run started with the flag, the new choice applies, is saved, and wins for the rest of the run. *Why:* the window is also a deliberate choice, and it is the newer one.

### The Preferences window

| How to open it | |
|---|---|
| Toolbar | A **Preferences** button, at the right end of the toolbar. |
| Shortcut | Cmd+, on macOS, Ctrl+, on Windows and Linux. Works anywhere in the window. |

It opens as a modal panel over the canvas, like the app's other dialogs. Esc or a **Close** button closes it. Audio keeps playing while it is open.

The panel has one section in this version:

**Output safety.** Three choices, one selected:

| Choice | Line under it |
|---|---|
| Low latency (default) | Catches accidents. No added latency. May distort briefly when it acts. |
| Clean | Transparent limiting. Adds 2 ms of latency. |
| Off | No protection. The raw patch output, clipped to -1 to 1. |

When the effective value comes from the command-line flag, a line under the section says: `Set by --output-safety for this run.` The selected choice shows the effective value.

A choice applies as soon as it is clicked. There is no Apply or OK button. *Why:* the player hears the change at once and can compare modes while a patch plays. One setting does not need a two-step form.

### Changing the mode at run time

When the player picks a new mode:

1. The app saves it (see Writing the file).
2. The app calls `Controller::set_output_safety` with the new mode, if an engine is running. The engine dips the output to silence over 10 ms, changes mode, and fades back over 10 ms ([output-safety.md](output-safety.md), rule 9). It never clicks.
3. Every engine the app builds afterwards (New, Open, a device change) uses the new mode.

Picking the mode that is already in effect does nothing: no write, no command, no dip.

If the command queue is full, the app tries again on its next update until the command is sent. *Why:* the queue drains every audio block, so it frees within milliseconds. Dropping the command would leave the window showing one mode while the engine runs another.

With `--no-audio`, there is no engine to tell. The choice is saved and used by the next engine.

## Rules

1. **Preferences belong to the machine, never the patch.** Opening, saving, or creating a patch never reads or writes preferences, and a patch cannot hold one. *Why:* ADR-0019, rule 2. A patch from someone else must not change the player's protection.
2. **The preferences store lives in `loom-app`.** No engine crate reads or writes the file. *Why:* hard rule 6. The engine crates stay portable and file-free. The app reads the file and passes plain values into `EngineConfig` and the `Controller`.
3. **A problem with the file never blocks start and never turns protection off.** A file that cannot be read or parsed gives the defaults for every field. A field with a value the app does not know falls back to its default alone, and the other fields keep their values. Defaults are the safe choice. *Why:* see Reading the file.
4. **The app writes only on a change, atomically.** *Why:* see Writing the file.
5. **A command-line flag sets the effective value for one run and is never saved.** *Why:* see The command-line flag.
6. **A change at run time reaches the running engine through `Controller::set_output_safety`, and the app never rebuilds the engine for it.** *Why:* rebuilding would restart every module's state and cut the sound. The command was built so the change is click-free.
7. **The format is versioned from the start.** *Why:* the patch format learned this (ADR-0008). Preferences are simpler, but a version field costs nothing now and is hard to add later.

## Acceptance criteria

Tests point `--preferences` (or the loading function behind it) at a temporary folder. They never touch the real file.

- [ ] Given no preferences file, when the app starts, then the mode is Low latency and no file is created.
- [ ] Given a file with `output_safety: "clean"`, when the app starts, then the first `EngineConfig` has `OutputSafety::Clean`.
- [ ] Given a file that does not parse, when the app starts, then the mode is Low latency, the status bar shows the error, and the file is unchanged.
- [ ] Given that broken file, when the player picks Clean, then the old file is now `preferences.broken.ron` and `preferences.ron` holds `clean`.
- [ ] Given broken files `studio.ron` and `stage.ron` in one folder, when the app runs once with each through `--preferences` and the player changes a setting each time, then the folder holds `studio.broken.ron` and `stage.broken.ron`, each with its own old contents.
- [ ] Given `output_safety: "loud"` and a valid later field, when the app starts, then the mode is Low latency, the later field is used, and the status bar names the bad value.
- [ ] Given a saved `clean` and `--output-safety low-latency`, when the app starts, then the engine runs Low latency. Given a saved `clean` and no flag, then it runs Clean.
- [ ] Given a saved `clean` and `--output-safety off`, when the app starts, then the engine runs Off, the window shows Off with the flag note, and after quitting the file still holds `clean`.
- [ ] Given a running engine, when the player picks Off, then the app sends one `SetOutputSafety(Off)` command, writes the file, and the next patch it opens builds its engine with Off.
- [ ] Given the mode already in effect, when the player picks it again, then no command is sent and the file's modification time does not change.
- [ ] Given a folder that cannot be written, when the player picks Clean, then the engine runs Clean and the status bar says the change applies until quit.
- [ ] Given the default paths, then on each OS the path matches the table in Where the file lives. (Unit test of the path function, with the environment variables set by the test.)
- [ ] **Manual check.** On macOS, Windows, and Linux: Cmd+, or Ctrl+, opens the window. With Noise at gain 4 playing, switching between all three modes dips and returns without a click, and the level changes as expected (under the ceiling in Low latency and Clean, clipped in Off). After restarting the app, the last choice is still selected.

## Out of scope

- Other preferences: audio device and MIDI device choice (FR-APP-01), sample rate and buffer size, theme, the last folder used by Open and Save ([files.md](files.md)). Each needs its own section here when it comes. The store and window are built so they can grow.
- The user's author name and default licence for new patches. Their behaviour is in the [patch metadata spec](patch-metadata.md); they get a section here when it is agreed.
- A native menu bar entry (LoomDSP > Settings on macOS). The app has no menu bar yet.
- Preferences for `loom play` and `loom render`. The CLI keeps its flags.
- Syncing preferences between machines.
- Several Preferences windows, or a window separate from the main one.
- Migrating a file with a version above 1. Version 1 is the only version until the first release.

## Open points

- **Windows: `%LOCALAPPDATA%` or `%APPDATA%`?** `%APPDATA%` roams with the user's account to other computers on a managed network. Leaning: `%LOCALAPPDATA%`, because these settings describe this computer's hardware. A setting like Off is right for one rig and wrong for another.
- **Which crate finds the folders?** Leaning: [`etcetera`](https://crates.io/crates/etcetera) (MIT or Apache-2.0), as [autosave](autosave.md#finding-the-folders) does: the paths in the table come from `Apple` `data_dir()` (its `config_dir()` is `~/Library/Preferences`), `Windows` `cache_dir()` (its `config_dir()` is the roaming folder), and `Xdg` `config_dir()`. Not `dirs`: it depends on `option-ext`, which is MPL-2.0 and not allowed by `deny.toml`. Writing it by hand from environment variables is the other option. The paths in the table are what matters; the crate must produce them.
- **macOS: Application Support, or `~/Library/Preferences`?** `~/Library/Preferences` is meant for files the system manages (property lists). Leaning: Application Support, because we write our own RON file.
- **Unknown fields: drop or keep on write?** This spec drops them, so a newer app's settings would be lost if an older app writes the file. Leaning: drop during the early alpha, and keep them once there is a release.
- **Confirm before choosing Off?** Choosing Off removes protection with one click. Leaning: no confirmation. The line under Off says what it does, and the change is easy to undo. A confirmation would get in the way of the CV-hardware player, who is the reason Off exists.
- **Two app instances.** Each reads the file at start, and the last one to write wins. Leaning: accept this. Settings change rarely, and the file is never half-written.
- **Toolbar space.** The toolbar is getting full. Leaning: a text button now, and a gear icon when the app has icons.
- **Does Panic (#25) or device recovery (FR-APP-04) need anything here?** Leaning: no. They build engines with the effective value like any other rebuild.
