# Control Channel Specification

A development-only way for a script or an AI agent to drive the running app by name and read back what it shows.

- **Kind:** living specification.
- **Status:** Implemented
- **Decision behind it:** None. It builds on the plain-data UI snapshot ([ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md)).
- **Use case and requirements:** Developer tooling. No user requirement.
- **Issues:** [#207](https://github.com/brylie/LoomDSP/issues/207) (part 3), `press` and `arm`: [#256](https://github.com/brylie/LoomDSP/issues/256); places in the snapshot: [#250](https://github.com/brylie/LoomDSP/issues/250); `sample`: [#255](https://github.com/brylie/LoomDSP/issues/255); devices and stream health: [#26](https://github.com/brylie/LoomDSP/issues/26); related [#190](https://github.com/brylie/LoomDSP/issues/190) (accessibility).
- **Code:** `crates/loom-app/src/control.rs`; the clock and recorder it relies on are in `crates/loom-host-desktop`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

An agent building a feature wants to check it in the real app: "select the filter, turn Cutoff to 800 Hz, play middle C, and show me the window". Today it has three partial tools:

- `--screenshot`, `--screenshot-window`, and `--dump-ui` show a still frame of a patch at start. With `--frames`, `--dump-ui` and `--screenshot-window` can run the engine for a while first, but they cannot show what happens after an edit or a note.
- The headless `Ui` harness drives the real view, but only inside a Rust test.
- Desktop automation can see and click the window of `LoomDSP.app`, but by pixel only: the window has no accessibility tree (#190), so a control cannot be found by name or read.

The control channel fills the gap: commands by name into the running app, and replies with what the UI shows. It also tells pixel-based automation where a named control is, so its clicks land.

## Behaviour

Started with a flag naming a Unix domain socket, for example:

```sh
loom-app examples/patches/subtractive.ron --fake-audio --control /tmp/loom.sock
```

Add `--record out.wav` to also save what the app plays, across `open` commands, until `quit`. `loom analyze out.wav` then measures it, for example to check that opening a patch gives no click ([audio QA harness](audio-qa.md#part-2---record-covers-the-whole-run)).

Any client that can write lines to a Unix socket works, such as `nc -U /tmp/loom.sock` or `socat`. A client sends one JSON object per line and gets one JSON object per line back, in the same order. `id` is optional and comes back as it was sent:

```text
> {"id": 1, "cmd": "select", "modules": [4]}
< {"id": 1, "ok": true}
> {"id": 2, "cmd": "set", "module": 4, "param": "cutoff", "value": 800}
< {"id": 2, "ok": true, "value": 800.0}
> {"id": 3, "cmd": "locate", "target": "4:cutoff"}
< {"id": 3, "ok": true, "x": 512.0, "y": 240.0, "width": 22.0, "height": 22.0}
> {"id": 4, "cmd": "set", "module": 4, "param": "nonsense", "value": 1}
< {"id": 4, "ok": false, "error": "module 4 (SVF) has no parameter `nonsense`"}
```

### Commands

| Command | Fields | Does | Reply |
|---|---|---|---|
| `snapshot` | | Nothing | `snapshot`: the UI snapshot, the same JSON as `--dump-ui`, with live values |
| `select` | `modules` | Selects these modules, as clicks would | |
| `set` | `module`, `param`, and `value` (in the parameter's units) or `position` (0 to 1, as the knob) | Sets a parameter, as the knob does: one undo step | The value as stored (clamped, rounded for whole numbers) |
| `add` | `kind` (with or without `@1`), optional `at` (`[x, y]` on the canvas; default: below the patch) | Adds a module, as the browser does | `module`: the new module ID |
| `remove` | `modules` | Removes modules, as Delete does | |
| `connect` | `from`, `to` (`"3:out"`, `"4:in"`) | Connects a cable | |
| `disconnect` | `to` | Removes the cable into this input | |
| `undo`, `redo` | | As the toolbar buttons | The next `undo` and `redo` step names |
| `open` | `path` | Opens a patch, as File > Open does after the dialog | |
| `save` | `path` | Saves the patch, as Save As does after the dialog | |
| `note` | `note`, optional `velocity` (default 100), `on` | A MIDI note on or off on channel 1, as a MIDI port sends it | |
| `cc` | `controller`, `value`, optional `channel` (1 to 16, default 1) | A MIDI control change. Replies once the engine has taken it and the app has applied it: mapped controls moved, or the armed parameter learned it | |
| `key` | `key` (a character, or `escape`, `enter`, `tab`, `space`, `delete`, `backspace`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`), optional `modifiers` (`cmd`, `ctrl`, `shift`, `alt`) | A key press and release, as the keyboard listener sends them | |
| `press` | `button`: `learn` (MIDI learn), `hold` (Hold note), `undo`, `redo` | Clicks a toolbar button by name, through the message it sends. Refused when the button is disabled, opens a dialog (rule 9), or is Add module (use `add`) | |
| `arm` | `target`: a parameter (`"4:cutoff"`) | In MIDI learn mode, arms the parameter, or disarms it if armed, as its inspector row does. Refused when learn mode is off | |
| `locate` | `target`: a module (`"4"`), a control by parameter name or a port by name (`"4:cutoff"`, `"3:out"`), or any other text shown in the window (`"Cutoff"`) | Nothing | The centre and size, in logical window pixels |
| `screenshot` | `path` | Saves a PNG of the whole window as it was last drawn: canvas, inspector, and bars | The image size in pixels, and the scale factor |
| `sample` | `target`: a control, light, or meter by the name it is bound to (`"4:resonance"`), `ms` (1 to 30000) | Records what the canvas draws for this widget, once per frame, for this long | `frames`: one entry per drawn frame. `t`: milliseconds since the first entry. `position`: the knob position, light brightness, or meter level, from 0 to 1. `live`: a knob's live modulated value, as a position, while its dot is drawn. `peak`: a meter's peak level |
| `wait` | `ms` | Replies after this long, with the app running | |
| `quit` | | Closes the app, without asking to save | |

### Places in the snapshot

The snapshot from `snapshot` and `--dump-ui` says where things are on the canvas, in world units: the canvas's own coordinates, before pan and zoom. They come from the same scene the canvas draws and hits, so they cannot drift from it. *Why:* a test or script can check a layout rule, such as "no badge covers another tile's port", and aim `--pointer` or `--center`, without a screenshot.

| Field | What it is |
|---|---|
| `port_hit_radius` | How far from a port's centre a click still hits it. Zoomed out below about 0.78, the canvas widens it so it stays 7 pixels on screen |
| `tiles[].bounds` | The tile rectangle |
| `tiles[].inputs[].centre`, `tiles[].outputs[].centre` | A port's centre, as `{x, y}` |
| `tiles[].outputs[].badge_bounds` | The assignment count badge, or `null` when the output shows none |
| `tiles[].widgets[].bounds` | A widget's rectangle on the panel |
| `cables[]` | Each cable the canvas draws, by its two ends: `from` (an output) and `to` (an input), each with `module`, `index` (from the top of its side), and `point` (the port's centre) |

A rectangle is `{x, y, width, height}`, from its top-left corner. `locate` answers in window pixels instead, because that is where pixel-based automation clicks.

## Devices and stream health

**Status: Agreed** (2026-10-24), and built. Issue: [#26](https://github.com/brylie/LoomDSP/issues/26) (its manual check).

The [manual check for audio and MIDI devices](../dev/manual-checks/issue-26-devices.md) needs a person to click through Preferences, press New, and listen. Running it found two bugs that the app could not see: it said "Playing" while the device made no callbacks. These commands let a script do the clicking, and the `audio` field in the `snapshot` reply lets it see whether sound is really being made. A person still listens for clicks; the script does the rest.

### Commands

| Command | Fields | Does | Reply |
|---|---|---|---|
| `output_safety` | `mode`: `low-latency`, `clean`, or `off` | Chooses the mode, as its radio button in Preferences does | `playing`, `status` |
| `audio` | Exactly one of `device`, `rate`, or `buffer`. `device`: `"default"` (System default) or a device name. `rate`: `"default"` (Device default) or Hz. `buffer`: `"default"` (Device default) or frames | Picks one entry in one list, as the Device, Sample rate, or Buffer size list does. The stream restarts, and the choice is saved | `playing`, `status`, `audio` |
| `midi` | `input`: a MIDI input name, `on`: true or false | Checks or unchecks the input, as its check box does. The choice is saved | `status`, `inputs`: the open inputs |
| `new` | | Starts an empty patch, as the toolbar New button does after the unsaved-changes dialog | |

`playing` is the line under the audio controls, for example `Playing on "BlackHole 2ch" at 96000 Hz, 64 frames (0.7 ms).` `status` is the status bar text. `audio` is the same object as in the `snapshot` reply.

Example, the rate and buffer steps of Part 2:

```text
> {"cmd": "audio", "device": "BlackHole"}
< {"ok": true, "playing": "Playing on \"BlackHole 2ch\" at 48000 Hz, 512 frames (10.7 ms).", "status": "...", "audio": {...}}
> {"cmd": "audio", "rate": 96000}
> {"cmd": "audio", "buffer": 64}
> {"cmd": "wait", "ms": 500}
> {"cmd": "snapshot"}
< {"ok": true, "snapshot": {...}, "audio": {"device": "BlackHole 2ch", "rate": 96000, "buffer": 64, "callbacks": 1520, "advanced": true, "stream_errors": 0, "overruns": 0}}
```

### The `audio` field

The `snapshot` reply gets a second field, `audio`, next to `snapshot`. It reads the callback counters of the stream that plays now (`CallbackStats` in `loom-host-desktop`).

| Field | What it is |
|---|---|
| `device` | The device name, or `null` when no device plays (`--fake-audio`, `--no-audio`, or a device that failed) |
| `rate` | The sample rate in Hz the stream was opened at, or `null` |
| `buffer` | The largest buffer the device has asked for so far, in frames, or `null` before the first callback |
| `callbacks` | Callbacks since this stream started. With `--fake-audio`, the timer's callbacks |
| `advanced` | True when `callbacks` grew since the previous `snapshot` reply, or since the stream started if this stream had no `snapshot` yet. `null` when nothing plays |
| `stream_errors` | Errors the backend reported since the stream started |
| `overruns` | Callbacks that took longer than their buffer |

`--dump-ui` stays as it is: it has no stream to read.

### Rules for these commands

- **S1. Same path as the window.** Each command becomes the `settings::Message` the Preferences window sends, and goes through `App::update` (rule 3). It works whether the window is open or not, and does not open it. With the window closed, the command reads the device lists for the change, as opening the window would, and drops them after. *Why:* the check should test the code a person uses.
- **S2. Only what the window offers.** A name matches a listed device or MIDI input exactly, or else by a case-insensitive part of the name that matches exactly one entry (as `--device` does, but never picking the first of several). `rate` and `buffer` must be in the list the window shows for the chosen device. Anything else is refused, and the error lists what is offered. *Why:* a value the window cannot send tests a path no user reaches.
- **S3. The reply comes after the restart.** `audio` replies once the stream has restarted (or failed and kept the old one, which the status says) and the window has drawn it (rule 5). The reply does not wait for the new stream's first callback; a script that needs one waits, then reads `snapshot`. *Why:* the restart is quick and synchronous; waiting for callbacks is the script's question to ask, with `advanced`.
- **S4. Saved like the window saves.** These commands write the preferences file, as the window does. A script should start the app with `--preferences FILE`, so it never changes the real per-user file. A device flag (`--device`, `--sample-rate`, `--buffer`) is cleared by the command, as by the window.
- **S5. `new` has no dialog,** like `open` (rule 9). Unsaved changes are dropped. `press` still refuses the `new` button, and points to this command.
- **S6. Off when there is nothing to change.** With `--no-audio` or `--fake-audio`, `audio` is refused with the reason (the window shows no device lists then). `output_safety` and `midi` still work.

### Fake host

The fake host in `crates/loom-app/src/devices.rs` gets a device that starts, makes a few callbacks, and then stops calling back, with no error. This is what the macOS rate change did. An app test opens it through `audio`, reads `snapshot` (`advanced` is true), and reads it again (`advanced` is false). The fake counts in steps, not on the clock, so the test needs no sleeps.

### Acceptance criteria

- [x] App tests, through the control harness and the fake host: `output_safety`, `audio` (device, rate, buffer, and `"default"`), `midi`, and `new` change what the Preferences window shows, and replies give `playing` and `status`.
- [x] A refused name, rate, or buffer gets `ok: false` with the offered values.
- [x] With the stalling fake device, `advanced` is true and then false, and `callbacks` stops growing.
- [x] `examples/dev/check-devices-part-2.jsonl` runs Part 2 of the manual check (device, 96 kHz, smallest buffer) on BlackHole with `--control-script`, and its replies show `advanced: true` after each change. Checked on macOS with BlackHole 2ch, 2026-10-24.
- [x] The manual check says which steps the script replaces, and which still need ears.

### Out of scope

- **A warning in the app** when the device stops calling back. The app could say so in the status bar, but that is a user feature for the preferences spec, not this tool. It is an open point there.
- **Listening.** The script cannot hear a click at the device. `--record` and `loom analyze` cover what the engine plays.

## Rules

1. **Off unless asked for.** Nothing listens unless the app is started with `--control`. *Why:* a music app should not open a door on a user's machine without being asked.
2. **Local only.** It listens on a local socket, never on the network. *Why:* commands can open and save files; only processes of the same user should reach it.
3. **Same path as the UI.** Each command becomes the `Message` the UI would send, and goes through `App::update`. So undo steps, the status bar, and errors are the ones a person would get. *Why:* a check through the channel should test what users get, not a side door.
4. **Names, not indices.** Parameters and ports are named as in patch files (`"4:cutoff"`, `"3:out"`). Toolbar buttons have fixed names, not their labels: a label can change while the app runs (Hold note becomes Release note). *Why:* names are stable IDs, and a script that reads like the patch is easy to check.
5. **One at a time, in order.** A reply comes once the command has been applied and the window has drawn it: the app waits two frames (the one already on its way, then the one with the change) before it answers, captures, or searches the window. A `cc` also waits for the engine to take its control change (up to about two seconds; a change from a MIDI port in between is not taken for it), so a script that arms a parameter, sends a `cc`, and then turns learn mode off keeps the mapping. An error, `snapshot`, and `quit` answer at once. *Why:* the next `snapshot`, `locate`, or `screenshot` then sees the result, with no sleeps in the script.
6. **Nothing on the audio thread.** MIDI goes through the same shared sender as a MIDI port. The engine is reached only through the `Controller`. *Why:* real-time safety (hard rule 1).
7. **No messages while idle.** The listener wakes the app only when a command arrives. The app asks for frames only while a command waits to be drawn or a `sample` runs. *Why:* every message redraws the window, and `--measure-idle` must still count zero frames for an idle patch.
8. **Errors are replies, not crashes.** A bad command gets `ok: false` and a message naming what was wrong. The app keeps running. *Why:* an agent learns from the error and tries again.
9. **No dialogs.** While the channel is on, errors that would open a dialog (a failed open or save) go to the reply and the status bar instead. Commands never open a file dialog or ask to save: `key` refuses the New, Open, and Save shortcuts (Cmd or Ctrl with N, O, or S), and `press` refuses the `new`, `open`, `save`, and `save-as` buttons. Both point to the `new`, `open`, and `save` commands. *Why:* a modal dialog would stop the app, and nobody is there to close it.
10. **`:name` looks for a parameter first,** then an output, then an input. *Why:* a module's parameter and port names rarely clash; when they do, the control is what a script most often wants.
11. **A script file runs the same commands.** `--control-script FILE` reads commands from a file, one per line (blank lines skipped), and prints each reply to stdout. At the end of the file the app keeps running, unless the last command is `quit`. *Why:* the same code serves a quick check with no client, and a check that can be saved next to a patch.
12. **Unix only, for now.** The socket needs Unix domain sockets (macOS and Linux). On Windows `--control` says it is not supported; `--control-script` works everywhere. *Why:* Rust's standard library has Unix sockets on Unix only. TCP on loopback can come later if Windows needs it.
13. **Short socket paths.** A socket path must be shorter than about 100 bytes (104 on macOS, 108 on Linux). A relative path is fine. A socket file left by an earlier run is replaced; any other file at that path is never touched, not even when the app quits. The app removes its socket when it quits. *Why:* the limit is the operating system's; a long temporary folder path fails with "path must be shorter than SUN_LEN".
14. **`sample` measures, a person judges.** It gives one entry per frame with the frame's time, so a script can find the largest jump between frames, frames where the value did not change, and gaps between frames. It starts after the two frames of rule 5. The live values are read each frame it records, even when no modulation asks for frames. *Why:* "does the dot move smoothly" then becomes numbers; a person only has to judge how it looks.

## Acceptance criteria

- [ ] Without `--control`, the app opens no socket.
- [ ] Tests run commands on an app with `--fake-audio`: select a module, set a parameter by value and by position, send a note, and read the snapshot that shows the change.
- [ ] A test locates a control, a port, and a text by name, at the same place the headless `Ui` harness finds them.
- [ ] A manual check drives the real window through the socket with `nc -U`, and saves a screenshot that shows the canvas.
- [ ] A script file with a `quit` at the end runs and exits with status 0.
- [ ] `set` on a mapped parameter, then `undo`, restores the value, as in the UI.
- [ ] A bad command gets `ok: false` with a message, and the next command works.
- [ ] A test with `--fake-audio`, a note held, and the filter envelope modulating Resonance samples the knob and sees its live value change from frame to frame. Afterwards the app asks for no more frames.
- [ ] With `--control` and `--measure-idle` and no commands, an idle patch counts no extra frames.
- [ ] `tests/no_alloc.rs` still passes.
- [ ] AGENTS.md lists the command, and an example session.
- [ ] The snapshot's badge, port, widget, and cable places match the scene the canvas draws and hits (`the_snapshot_places_what_the_canvas_draws`).

## Out of scope

- **Pointer events.** Pressing, dragging, and hovering at window coordinates: desktop automation does that, with `locate` to aim. The headless `Ui` harness covers it in tests.
- **Remote access** over the network, and authentication.
- **A scripting language.** Clients are any language that writes JSON lines.
- **Users.** It is not a feature of the app for musicians, and is not in the help.

## Open points

None. The window capture includes the canvas's custom wgpu primitive (checked on macOS, 2026-10-20).
