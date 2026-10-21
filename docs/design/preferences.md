# App Preferences Specification

A per-machine preferences file and window for the desktop app, so a player sets things like output safety once and the app remembers them.

- **Kind:** living specification.
- **Status:** Draft, and built. The file, its location, `--preferences`, the window, and the Output safety section were built with [Audio and MIDI devices](#audio-and-midi-devices) (#26), which needs them. Each [open point](#open-points) was built as its leaning says. Agreeing the open points is still to do; a different answer changes the code.
- **Decision behind it:** None. Builds on [ADR-0019](../adr/0019-output-safety-stage.md), which says the output safety mode belongs to the machine, not the patch.
- **Use case and requirements:** UC3 (perform live), FR-APP-01 (desktop app, with audio and MIDI device selection), FR-MIDI-01, FR-PERF-08, NFR-SND-02.
- **Background:** [Output safety explainer](../learn/output-safety.md), for the first setting.
- **Prior art:** most desktop apps keep settings in a file in the operating system's per-user settings folder, and open a settings window with Cmd+, on macOS. We use only those conventions.
- **Issues:** follows [#131](https://github.com/brylie/LoomDSP/issues/131); [#201](https://github.com/brylie/LoomDSP/issues/201) (output safety), [#26](https://github.com/brylie/LoomDSP/issues/26) (devices).
- **Code:** the file in `crates/loom-app/src/preferences.rs`; the window and what its choices do in `crates/loom-app/src/settings.rs`; devices in `crates/loom-app/src/devices.rs`. Tests: the `tests` modules of those three files.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

"My audio interface sends control voltages to my modular, so I run with output safety off. I don't want to type a flag every time I start LoomDSP. And when I play through the PA at a gig, I want to turn safety back on from inside the app, without restarting."

The app needs one place for settings that belong to this computer and its hardware, not to a patch. Output safety is the first. Audio and MIDI devices are the second ([their own section](#audio-and-midi-devices)). Others can follow later (see Out of scope).

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

The panel has two sections: Output safety, described here, and [Audio and MIDI devices](#audio-and-midi-devices).

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

- [x] Given no preferences file, when the app starts, then the mode is Low latency and no file is created.
- [x] Given a file with `output_safety: "clean"`, when the app starts, then the first `EngineConfig` has `OutputSafety::Clean`.
- [x] Given a file that does not parse, when the app starts, then the mode is Low latency, the status bar shows the error, and the file is unchanged.
- [x] Given that broken file, when the player picks Clean, then the old file is now `preferences.broken.ron` and `preferences.ron` holds `clean`.
- [x] Given broken files `studio.ron` and `stage.ron` in one folder, when the app runs once with each through `--preferences` and the player changes a setting each time, then the folder holds `studio.broken.ron` and `stage.broken.ron`, each with its own old contents.
- [x] Given `output_safety: "loud"` and a valid later field, when the app starts, then the mode is Low latency, the later field is used, and the status bar names the bad value.
- [x] Given a saved `clean` and `--output-safety low-latency`, when the app starts, then the engine runs Low latency. Given a saved `clean` and no flag, then it runs Clean.
- [x] Given a saved `clean` and `--output-safety off`, when the app starts, then the engine runs Off, the window shows Off with the flag note, and after quitting the file still holds `clean`.
- [x] Given a running engine, when the player picks Off, then the app sends one `SetOutputSafety(Off)` command, writes the file, and the next patch it opens builds its engine with Off.
- [x] Given the mode already in effect, when the player picks it again, then no command is sent and the file's modification time does not change.
- [x] Given a folder that cannot be written, when the player picks Clean, then the engine runs Clean and the status bar says the change applies until quit.
- [x] Given the default paths, then on each OS the path matches the table in Where the file lives. (Unit test of the path function, with the environment variables set by the test.)
- [ ] **Manual check.** On macOS, Windows, and Linux: Cmd+, or Ctrl+, opens the window. With Noise at gain 4 playing, switching between all three modes dips and returns without a click, and the level changes as expected (under the ceiling in Low latency and Clean, clipped in Off). After restarting the app, the last choice is still selected.

## Audio and MIDI devices

The second section of the Preferences window: which audio output plays the patch, at which sample rate and buffer size, and which MIDI inputs play it. The app remembers the choice between runs.

- **Status:** Agreed, and built in #26, except the manual check. It builds on the file, flag, and window rules above, which are still Draft. Those parts were built with it.
- **Use case and requirements:** UC3 (perform live), FR-APP-01, FR-MIDI-01.
- **Issues:** [#26](https://github.com/brylie/LoomDSP/issues/26). Related, not built here: [#27](https://github.com/brylie/LoomDSP/issues/27) (MIDI hot-plugging, FR-MIDI-08), [#28](https://github.com/brylie/LoomDSP/issues/28) (audio device loss, FR-APP-04), [#29](https://github.com/brylie/LoomDSP/issues/29) (audio input), [#163](https://github.com/brylie/LoomDSP/issues/163) (`loom play` opens every MIDI input).
- **Code:** `crates/loom-app/src/devices.rs` (the `Host` trait, what is offered, the choice at start, and a fake host for tests), `crates/loom-app/src/settings.rs` (the window, restarting audio, MIDI inputs), and `crates/loom-host-desktop` (`outputs`, `RunningAudio::stop`, `MidiConnection::close`). `loom play` keeps its own `--device`, `--sample-rate`, `--buffer`, `--midi`, and `--no-midi`.

### Goal

"At home I play through my laptop speakers. In the studio I use my USB interface at 96 kHz with a small buffer, so the keys feel tight. I want to pick the interface once and have LoomDSP use it next time. I have two keyboards plugged in, and one of them is a drum pad I use with another program, so I want to turn that one off."

### What is saved

Two new fields in `preferences.ron`. They hold the player's choice, not what the app found at start (see Rule D2).

```ron
(
    version: 1,
    output_safety: "low-latency",
    audio: (
        device: Named("MOTU M4"),
        sample_rate: Hz(96000),
        buffer: Frames(128),
    ),
    midi: (
        off: ["Pad Controller MIDI 1"],
    ),
)
```

| Field | Values | Default |
|---|---|---|
| `audio.device` | `SystemDefault`, or `Named("<full device name>")` | `SystemDefault` |
| `audio.sample_rate` | `DeviceDefault`, or `Hz(<rate>)` | `DeviceDefault` |
| `audio.buffer` | `DeviceDefault`, or `Frames(<size>)` | `DeviceDefault` |
| `midi.off` | A list of full MIDI input names | `[]` (every input on) |

- `SystemDefault` follows the operating system's choice, and changes when the player changes it there. `Named` sticks to one device. *Why:* a laptop player wants "whatever macOS is set to". A studio player wants "the interface, never the speakers". They are different choices, and both need a way to say so.
- A device is matched by its full name, exactly. *Why:* `loom play --device` matches part of a name, which is quick to type but can pick the wrong one of two similar interfaces. A saved choice is made by clicking, so the full name is known.
- MIDI saves the inputs that are **off**, not the ones that are on. *Why:* a controller the app has never seen plays at once, with no trip to the Preferences window. This is what the app does today, what #163 proposes for `loom play`, and what #27 needs when a controller is plugged in on stage.
- A bad value in one of these fields falls back to that field's default, like any other field (Reading the file).

### The window section

Below Output safety:

**Audio output**

| Control | Choices |
|---|---|
| Device | **System default (`<name of the current system default>`)**, then every output device by name. |
| Sample rate | **Device default (`<rate>` Hz)**, then the rates this device offers (see Rule D4). |
| Buffer size | **Device default**, then the sizes this device offers. Each size shows its time at the chosen rate, for example `128 frames · 1.3 ms`. |

A line under the section shows what is playing now: `Playing on "MOTU M4" at 96000 Hz, 128 frames (1.3 ms).` With `--no-audio` it says `Audio is off for this run (--no-audio).` With `--fake-audio`, `No device for this run (--fake-audio).` In both cases the controls still change the saved value.

The device list is read when the window opens, and again from a **Refresh** button next to it. *Why:* asking the system for devices can take a moment on some systems, so it is not done every frame. Live device changes are #27 and #28.

**MIDI inputs**

One row per MIDI input, with a check box: checked is on. Under them: `New MIDI inputs are on until you turn them off.` An input that is in `midi.off` but not plugged in now is not listed. It stays in the file.

With no MIDI inputs: `No MIDI inputs found.`

There is no channel filter here. *Why:* the MIDI Note module already has a Channel parameter, and a MIDI learn mapping matches one channel ([midi-learn.md](midi-learn.md), rule 5). A second filter on the input would be a second place to look when a note does not play.

### What a change does

Each choice applies as soon as it is made, and is saved, like Output safety.

| Change | What happens |
|---|---|
| Turn a MIDI input on or off | That input is opened or closed. Notes held on an input that is turned off are released. Audio does not stop. |
| Device, sample rate, or buffer size | The app restarts audio on the new settings (below). |
| A value that is already in effect | Nothing: no write, no restart. |

**Restarting audio.** The engine runs inside the audio stream and is built for one sample rate, so a new device, rate, or buffer size needs a new stream and a new engine. The app:

1. Opens the new device first. If that fails, the old audio keeps playing, nothing is saved, and the status bar says why: `Could not use "MOTU M4" at 192000 Hz: <reason>. Still playing on "MacBook Pro Speakers".`
2. Fades the old engine out over 10 ms, then stops it ([output-safety.md](output-safety.md), rule 14). *Why:* stopping a stream mid-signal is a click, and a thump when the output holds a DC offset.
3. Builds a new engine from the current patch, as it is now, with every unsaved edit, at the new rate. The new engine fades in over 10 ms, as every new engine does.
4. Keeps everything that is not the engine: the patch file, unsaved changes, undo history, selection, view, MIDI learn mappings, and open MIDI inputs.

The player hears a short gap, from about 20 ms to a few hundred, depending on the device. **This gap is accepted.** *Why:* a device change is setup, not performance. Keeping sound playing across two devices would need the engine to run on both at once, which is a large piece of work for a rare case.

**State that does not survive a restart.** Every module starts again from its initial state, as when a patch is opened: oscillator phases, envelopes, filter memory, delay lines and their tails, sequencer positions, and held notes. Parameter values, cables, and modulation are kept, because they are in the patch. The transport keeps its tempo and time signature and whether it is running, and its position starts at beat 0. *Why:* module state is often measured in samples (a delay line of 48000 samples is 1 second at 48 kHz and 0.5 seconds at 96 kHz), so carrying it across a rate change would need each module to convert it. A clean start is what opening a patch already does, so it holds no surprise. A buffer-size change alone could in principle keep the engine, but one path for all three changes is simpler to test.

### At start

The app opens audio with the effective values: the saved values, unless a flag sets them for this run (below).

| Situation | What the app does | Status bar |
|---|---|---|
| Saved device is plugged in | Plays on it. | Nothing. |
| Saved `Named` device is not plugged in | Plays on the system default device. The saved choice is unchanged. | `Audio: "MOTU M4" is not connected. Playing on "MacBook Pro Speakers". Choose a device in Preferences.` |
| Saved rate or buffer size is not offered by the device that opened | Uses the device default for that value. The saved choice is unchanged. | `Audio: "MacBook Pro Speakers" does not offer 96000 Hz. Using 48000 Hz.` For a buffer size: `Audio: "<device>" does not offer 128 frames. Using the device default.` |
| No output device at all | Starts with no audio, as today. | As today. |
| An input in `midi.off` | Not opened. | Nothing. |
| Any other input | Opened, as today. A port that fails to open is reported, and the others still play. | As today. |

The app always gets to a running patch, and it plays on the system default device rather than starting silent. *Why:* the same reason as Reading the file. A missing interface must not stop the player from starting, and the message says what happened and where to fix it. Silence would look like a broken app, and output safety still protects the speakers.

### Command-line flags

The app takes the same flags as `loom play`, with the same rule as `--output-safety`: a flag sets the effective value for this run and is never saved.

| Flag | Effect for this run |
|---|---|
| `--device <part of name>` | Plays on the first output device whose name contains it, case-insensitive. If none matches, the app starts with no audio and says so. |
| `--preferences <file>` | Reads and writes this file instead of the per-user one (see Where the file lives). |
| `--sample-rate <Hz>` | Uses that rate. |
| `--buffer <frames>` | Uses that buffer size. |
| `--midi <part of name>` | Opens only the matching inputs, whatever `midi.off` says. |
| `--no-midi` | Opens no MIDI input. |

When a flag sets a value, a line under that control says `Set by --device for this run.`, as Output safety does. Changing it in the window applies, is saved, and wins for the rest of the run.

*Why the flags:* scripts and agents use the control channel with `--fake-audio` and need a known setup, and a player can rescue a run on a strange machine without editing files. Matching `loom play` keeps one set of names.

### Rules

- **D1. Devices belong to the machine, never the patch.** A patch cannot name a device, rate, buffer size, or MIDI input. *Why:* the same patch must play on any machine. The issue asks for it.
- **D2. A fallback is never saved.** When the saved device, rate, or buffer size cannot be used at start, the app plays with something else and leaves the file alone. *Why:* the interface left at the studio is plugged in again tomorrow, and the player should not have to choose it again.
- **D3. The app always starts and always plays if any output exists.** A missing saved device is a status bar message, not a modal and not silence. *Why:* the same as Rule 3.
- **D4. Only what the device reports is offered.** Sample rates are the common rates (44100, 48000, 88200, 96000, 176400, 192000 Hz) that fall in one of the device's reported 32-bit float ranges. Buffer sizes are the powers of two from 16 to 4096 in the device's reported range. When a device does not report a buffer range, the app offers 64 to 2048 and falls back to the device default if the device refuses one. *Why:* a choice that fails when picked teaches the player to distrust the menu. Some systems report a huge rate range, so the list of common rates keeps the menu short. The engine gives the same output at any block size (hard rule 2), so the buffer size only changes latency and load, never the sound.
- **D5. A device, rate, or buffer change opens the new device before stopping the old one, and fades out before it stops.** *Why:* a failed change must leave sound playing, and a stop mid-signal clicks. Open, New, and quit fade out the same way ([output-safety.md](output-safety.md), rule 14).
- **D6. A restart keeps the patch and the session, and resets module state.** *Why:* see State that does not survive a restart.
- **D7. MIDI inputs are on unless turned off.** *Why:* see What is saved.
- **D8. The window shows the buffer time, not the full output latency.** *Why:* the device and its driver add more delay, which some systems report and others do not, or report wrongly. A number that is right on one system and wrong on another is worse than a number that is clearly only the buffer. The full latency can be added when the host reports it reliably.

### Acceptance criteria

The host is behind a small trait in tests, with a fake list of devices, rates, and buffer ranges, so these run without hardware.

- [x] Given no `audio` or `midi` field, when the app starts, then it plays on the system default device at its default rate and buffer size, and opens every MIDI input.
- [x] Given `device: Named("Fake B")` and devices `Fake A` (default) and `Fake B`, when the app starts, then it plays on `Fake B`.
- [x] Given `device: Named("Fake B")` and only `Fake A`, when the app starts, then it plays on `Fake A`, the status bar names both, and the file is unchanged.
- [x] Given `sample_rate: Hz(96000)` and a device that offers 44100 to 48000, when the app starts, then it plays at the device default rate, and the file is unchanged.
- [x] Given a device that reports 32-bit float from 8000 to 384000 Hz, then the Sample rate menu lists Device default and the six common rates, and nothing else.
- [x] Given a playing patch with unsaved edits and three undo steps, when the player picks another rate, then the new engine runs at that rate, the edits and the three undo steps are still there, the file is saved, and the transport position is 0.
- [x] Given a playing patch, when the player picks a device that fails to open, then the old engine is still playing, the file is unchanged, and the status bar says why.
- [x] Given a playing patch, when the player changes the device, opens a patch, makes a new one, or quits, then the old engine is asked to fade out, and its stream is stopped only after it reports silence and two more buffers have gone to the device, or after 100 ms. (App test with the fake host. The fade itself is tested in [output-safety.md](output-safety.md#acceptance-criteria).)
- [x] Given two MIDI inputs, when the player turns one off while a note is held on it, then that note is released, the input is closed, its name is in `midi.off`, and the other input still plays.
- [x] Given `midi: (off: ["Pad"])`, when an input named `Pad` and one named `Keys` exist at start, then only `Keys` is opened.
- [x] Given a saved `Named("Fake B")` and `--device "Fake A"`, when the app starts, then it plays on `Fake A`, the window shows the flag note, and the file still holds `Fake B`.
- [x] Given `--midi Keys`, then only inputs whose names contain `keys` are opened, even if they are in `midi.off`.
- [ ] **Manual check.** On macOS, Windows, and Linux, with a USB audio interface and two MIDI controllers: pick the interface, 96 kHz, and the smallest offered buffer while a patch plays. The sound moves to the interface after a short gap with no click. With output safety Off and a Constant of 0.5 into Audio Out, opening another patch gives no thump. Quit and start again: it plays on the interface. Unplug the interface and start again: it plays on the built-in output, with the message, and the window still shows the interface as the saved choice. Turn one controller off: it stops playing the patch, and after a restart it is still off.

### Out of scope

- Audio input device (#29). Its spec decides one duplex stream or two, and clock drift between two devices. When it is agreed, the input device gets its own row in this section, saved the same way.
- Reacting to devices plugged in or out while the app runs: MIDI (#27) and audio (#28). This section reads the lists at start, when the window opens, and on Refresh.
- Output channel choice (for example outputs 3 and 4 of an 8-channel interface). Today the engine plays on the first two channels. This needs its own section.
- A MIDI channel filter on an input. See The window section.
- Choosing the host audio API (for example ASIO or JACK instead of WASAPI or ALSA). The app uses the system's default API.
- Per-device MIDI mappings (FR-MIDI-06, P2).
- Device choice in `loom play`. It keeps its flags; #163 changes its MIDI default.

### Open points

None. Settled in review:

- **Missing interface at start:** fall back to the system default device, not silence (see At start).
- **Fade-out:** a new engine command, used for Open, New, quit, and device changes ([output-safety.md](output-safety.md), rule 14).
- **Transport across a restart:** it keeps running, from beat 0 (see State that does not survive a restart).
- **Format of the default choices:** RON enum names (`SystemDefault`, `Named("...")`). They say what they mean when the file is edited by hand, and a device name can never be mistaken for the default (ALSA has a real device called `default`).
- **Latency shown:** the buffer time only (Rule D8).

## Out of scope

- Other preferences: theme, the last folder used by Open and Save ([files.md](files.md)). Each needs its own section here when it comes. The store and window are built so they can grow.
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
