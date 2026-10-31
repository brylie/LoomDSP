# Control Channel Specification

A development-only way for a script or an AI agent to drive the running app by name and read back what it shows.

- **Kind:** living specification.
- **Status:** Implemented
- **Decision behind it:** None. It builds on the plain-data UI snapshot ([ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md)).
- **Use case and requirements:** Developer tooling. No user requirement.
- **Issues:** [#207](https://github.com/brylie/LoomDSP/issues/207) (part 3), `press` and `arm`: [#256](https://github.com/brylie/LoomDSP/issues/256); pointer commands: [#251](https://github.com/brylie/LoomDSP/issues/251); lights, meters, and screenshot areas: [#254](https://github.com/brylie/LoomDSP/issues/254); stats and cut labels in the snapshot: [#234](https://github.com/brylie/LoomDSP/issues/234); places in the snapshot: [#250](https://github.com/brylie/LoomDSP/issues/250); `sample`: [#255](https://github.com/brylie/LoomDSP/issues/255); devices and stream health: [#26](https://github.com/brylie/LoomDSP/issues/26); script variables: [#340](https://github.com/brylie/LoomDSP/issues/340); related [#190](https://github.com/brylie/LoomDSP/issues/190) (accessibility).
- **Code:** `crates/loom-app/src/control.rs`, `pointer.rs` (pointer commands), `targets.rs` (names); the clock and recorder it relies on are in `crates/loom-host-desktop`.

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

Add `--record out.wav` to also save what the app plays, across `open` commands, until `quit`. `loom analyze out.wav` then measures it, for example to check that opening a patch gives no click ([audio QA harness](audio-qa.md#recording-the-whole-run)).

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
| `set` | `module`, `param`, and `value` (in the parameter's units) or `position` (0 to 1, as the knob) | Sets a parameter, as the knob does: one undo step | The value as stored (clamped, rounded for whole numbers). A `value` is stored exactly as sent, not through a knob position |
| `add` | `kind` (with or without `@1`), optional `at` (`[x, y]` on the canvas; default: below the patch) | Adds a module, as the browser does | `module`: the new module ID |
| `remove` | `modules` | Removes modules, as Delete does | |
| `connect` | `from`, `to` (`"3:out"`, `"4:in"`) | Connects a cable | |
| `disconnect` | `to` | Removes the cable into this input | |
| `probe` | `from` (an output, `"3:out"`), `kind` (`scope` or `meter`) | Adds a scope or a level meter fed by the output, as the probe menu's Add scope and Add meter do ([panels](panels.md#add-scope-and-add-meter)) | `module`: the new module ID |
| `undo`, `redo` | | As the toolbar buttons | The next `undo` and `redo` step names |
| `panic` | | Runs panic, as the toolbar button and Cmd+. do ([panic](panic.md)). Not an edit | |
| `open` | `path` | Opens a patch, as File > Open does after the dialog | |
| `save` | `path` | Saves the patch, as Save As does after the dialog | |
| `note` | `note`, optional `velocity` (default 100), `on` | A MIDI note on or off on channel 1, as a MIDI port sends it | |
| `cc` | `controller`, `value`, optional `channel` (1 to 16, default 1) | A MIDI control change. Replies once the engine has taken it and the app has applied it: mapped controls moved, or the armed parameter learned it | |
| `key` | `key` (a character, or `escape`, `enter`, `tab`, `space`, `delete`, `backspace`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`), optional `modifiers` (`cmd`, `ctrl`, `shift`, `alt`) | A key press and release, each on its own frame, sent to the window view at the pointer as a real key is. A widget can take it (the canvas takes Escape while a badge is pinned); what none takes goes to the app as the keyboard listener sends it | |
| `toolbar` | `button`: `learn` (MIDI learn), `hold` (Hold note), `panic` (Panic), `undo`, `redo` | Clicks a toolbar button by name, through the message it sends. Refused when the button is disabled, opens a dialog (rule 9), or is Add module (use `add`). Was `press` before [#392](https://github.com/brylie/LoomDSP/issues/392); `press` is now only the mouse button ([Pointer commands](#pointer-commands)) | |
| `arm` | `target`: a parameter (`"4:cutoff"`) | In MIDI learn mode, arms the parameter, or disarms it if armed, as its inspector row does. Refused when learn mode is off | |
| `locate` | `target`: a module (`"4"`), a control, port, light, or meter by name (`"4:cutoff"`, `"3:out"`, `"5:level"`, rule 10), with `display:`, `input:`, `output:`, or `badge:` to name a light or meter, a port, or an output's badge directly (`"11:display:out"`, `"4:input:cutoff"`, `"5:badge:out"`), or any other text shown in the window (`"Cutoff"`) | Nothing | The centre and size, in logical window pixels |
| `screenshot` | `path`, and optionally `target` (as `locate` takes it) with `margin` (logical pixels on each side, default 0), or `rect` (`[x, y, width, height]` in logical window pixels) | Saves a PNG of the whole window as it was last drawn: canvas, inspector, and bars. With `target` or `rect`, only that area, cut to the window | The image size in pixels, and the scale factor. With an area, also `area`: the area saved, as `x`, `y`, `width`, `height` in logical window pixels |
| `sample` | `target`: a control, light, or meter by the name it is bound to (`"4:resonance"`), `ms` (1 to 30000) | Records what the canvas draws for this widget, once per frame, for this long | `frames`: one entry per drawn frame. `t`: milliseconds since the first entry. `position`: the knob position, light brightness, or meter level, from 0 to 1. `live`: a knob's live modulated value, as a position, while its dot is drawn. `peak`: a meter's peak level |
| `wait` | `ms` | Replies after this long, with the app running | |
| `quit` | | Closes the app, without asking to save | |

### Pointer commands

**Status:** Agreed ([#251](https://github.com/brylie/LoomDSP/issues/251)), and built. The points below the table came up while building it, and were agreed in [#392](https://github.com/brylie/LoomDSP/issues/392).

Commands for the checks that need a hand today: hover lines, pinning a badge and pressing Escape, the drop highlight, and a range arc drag. A target is the same text as `locate` takes, or window coordinates `x` and `y` in logical pixels.

| Command | Fields | Does |
|---|---|---|
| `hover` | `target` or `x`, `y` | Moves the pointer there |
| `press`, `release` | `target` or `x`, `y`, optional `button` (`left` default, `right`, `middle`), optional `modifiers` | Mouse button down or up at the pointer, or at the target |
| `drag` | `from`, `to` (each a `target` or `[x, y]`), optional `steps` (default 10), `button`, `modifiers` | Press at `from`, move in `steps` equal moves to `to`, release |
| `scroll` | `target` or `x`, `y`, `dy` (lines, negative is down), optional `dx`, `modifiers` | A wheel step |

- **Same path as the UI.** Each command becomes the pointer events a real pointer sends, in order, through the same event handling as the window (rule 3). A `drag` is a press, `steps` moves, and a release, each applied and drawn before the next. So a cable drag, an arc drag, a tile move, and a drop all work.
- **How events are injected** is an implementation choice (a message into the iced event stream, or the `Ui` harness run against the live `App::view`). The rules above must hold either way: the same handlers, undo steps, and status messages.
- **`target`** is resolved at the time of the event. A target that is not on screen is an error. A `drag` with a `to` target resolves it at the last move, so a target that appears during the drag (a highlight) works.
- **Reply.** `ok`, and the pointer position. The snapshot gains `pointer`: `x`, `y`, `buttons` (down), `hover` (the name of the control, port, or badge under it, if any), `pinned` (the pinned output), and `drag` (what is being dragged, if anything), so a script can check each step.
- **`key`** with `escape` already exists, so "pin a badge, press Escape" is `hover`, `press`, `release`, `key`.
- **Still no dialogs** (rule 9), and no events outside the window.
- **Idle.** A pointer command wakes the app and asks for frames until it is drawn. `--measure-idle` counts no frames when no command arrives (rule 7).

How it is built: `pointer::Injector` wraps the whole window view. A command's events wait in a queue. On each redraw the injector sends the next one to the view, with the script's pointer as the cursor, and asks for another frame. The runtime can send one redraw to the view up to three times; only the first takes an event. Answers come two frames after the last event, as for other commands (rule 5).

Points settled while building, agreed in [#392](https://github.com/brylie/LoomDSP/issues/392) (2026-10-28):

- **`press` is only the mouse button.** The toolbar command is `toolbar` (Commands, above). *Why:* one command with two meanings is easy to misread in a script: `{"cmd": "press", "button": "learn"}` and `{"cmd": "press", "button": "left"}` look alike and do different things. The early alpha allows the break; scripts and docs that used the toolbar `press` change in the same pull request as the code.
- **`key` goes through the view.** Before, `key` went straight to the app. The canvas never saw it, so "pin a badge, press Escape" could not work. Now it is a real key event at the pointer, and what no widget takes reaches the app as before. *Why:* the canvas must see the key to unpin a badge, as a real Escape does (rule 3). So `key` now needs a window: a script that sends `key` cannot run headless.
- **A badge target.** `MODULE:badge:OUTPUT` (`"5:badge:out"`) is an output's assignment count badge, for `locate`, `screenshot`, and the pointer commands. It is an error when the output shows no badge. *Why:* pinning a badge needs a click on it, and its place on the canvas is only in world units in the snapshot.
- **Modifiers last one command.** `modifiers` are pressed before the command's first event and let go after its last. A `press` with Shift and a later `release` each need their own `modifiers`. Modifier keys reach the view only, not the keyboard listener. *Why:* a held modifier that outlives its command would change what every later command does, and a script could not see it.
- **A real pointer takes over.** While a command's events go in, real pointer events in the window are dropped. After that, the script's pointer stays where it is until a real pointer event arrives; then the script's held buttons are let go where its pointer is, and the real pointer leads again. *Why:* a person at the computer must always win, and a button the script holds must never stay down after they take over.
- **An error ends the command, and nothing stays held.** When a target is not found or not on screen, the command's remaining moves are dropped, but its releases and modifier changes still go in. A `drag` whose `to` is not found yet stays where it is, and fails only if `to` is still not found at the last move. *Why:* a button or modifier left down would turn the next real click into a drag or a Shift click.
- **Pop-ups.** Tooltips and other overlays get the real cursor, not the script's. *Why:* Iced gives overlays the cursor itself, and the injector cannot reach it without changing Iced. Pointer support in overlays is a follow-up.

The snapshot's `pointer` field is `null` without the control channel and in `--dump-ui`. With it on, it is read from the canvas once per drawn frame:

| Field | What it is |
|---|---|
| `x`, `y` | The pointer in logical window pixels, or `null` when it is outside the window |
| `buttons` | Mouse buttons held down: `left`, `right`, `middle` |
| `hover` | What is under the pointer on the canvas, or `null`: `{"kind": K, "target": T}` with `kind` one of `output`, `input`, `badge`, `control`, `display`, `tile`, and `target` a name `locate` takes that finds it again (`"5:out"`, `"5:badge:out"`, `"4:cutoff"`, `"4:input:cutoff"`, `"4"`). A light or a meter is hit as part of its tile. A cable is `{"kind": "cable", "from": "3:out", "to": "4:in"}` |
| `pinned` | The output whose badge is pinned (`"5:out"`), or `null` |
| `drag` | The drag in progress, or `null`: `{"kind": K}` with `kind` one of `pan`, `move` (with `modules`), `knob`, `button`, `depth` (each with `target`), `wire` (with `from` and `pulled`), `select` |

Example, from `examples/dev/check-pin-badge.jsonl`:

```text
> {"cmd": "hover", "target": "5:badge:out"}
< {"ok": true, "x": 401.5, "y": 287.0}
> {"cmd": "press"}
> {"cmd": "release"}
> {"cmd": "snapshot"}
< {"ok": true, "snapshot": {..., "pointer": {"x": 401.5, "y": 287.0, "buttons": [], "hover": {"kind": "badge", "target": "5:badge:out"}, "pinned": "5:out", "drag": null}}, ...}
> {"cmd": "key", "key": "escape"}
```

### Places in the snapshot

The snapshot from `snapshot` and `--dump-ui` says where things are on the canvas, in world units: the canvas's own coordinates, before pan and zoom. They come from the same scene the canvas draws and hits, so they cannot drift from it. *Why:* a test or script can check a layout rule, such as "no badge covers another tile's port", and aim `--pointer` or `--center`, without a screenshot.

| Field | What it is |
|---|---|
| `port_hit_radius` | How far from a port's centre a click still hits it. Zoomed out below about 0.78, the canvas widens it so it stays 7 pixels on screen |
| `tiles[].bounds` | The tile rectangle |
| `tiles[].inputs[].centre`, `tiles[].outputs[].centre` | A port's centre, as `{x, y}` |
| `tiles[].outputs[].badge_bounds` | The assignment count badge, or `null` when the output shows none |
| `tiles[].inputs[].follows_transport` | The transport signal an input follows while it has no cable, which shows its [clock mark](panels.md#clock-mark-on-transport-inputs): `bar_phase`, `beat_phase`, or `playing`. Left out when it shows none |
| `tiles[].widgets[].bounds` | A widget's rectangle on the panel |
| `cables[]` | Each cable the canvas draws, by its two ends: `from` (an output) and `to` (an input), each with `module`, `index` (from the top of its side), and `point` (the port's centre) |

A rectangle is `{x, y, width, height}`, from its top-left corner. `locate` answers in window pixels instead, because that is where pixel-based automation clicks.

### Stats and cut labels in the snapshot

**Status:** Agreed ([#234](https://github.com/brylie/LoomDSP/issues/234), with #258 folded in), and built.

| Field | What it is |
|---|---|
| `stats.label` | The stats bar's audio label, such as `no device (clock) at 48000 Hz` |
| `stats.playing` | True when a stream plays. Only then does the bar show load and dropouts |
| `stats.load`, `stats.max_load` | The latest and highest callback load, as a share of the time budget (1.0 is 100%). 0 when nothing plays |
| `stats.dropouts` | The dropout count the bar shows |
| `stats.telemetry_dropped` | Meter and scope readings the engine dropped |
| `tiles[].inputs[].shown`, `tiles[].outputs[].shown`, `tiles[].widgets[].shown` | The label as the tile draws it, when it is cut with "…" (`{"label": "Resonance", "shown": "Reson…"}`). Left out when the whole label shows |

`App::stats_bar` draws from the same `stats` model, and `shown` comes from the canvas's own tile text layout, so the snapshot cannot disagree with the window. The frame rate is left out, so two snapshots of an idle app are equal. Titles are not listed: they are clipped at the tile edge, not cut with "…". A mapping label shown in place of a caption in MIDI learn mode is not listed either.

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
< {"ok": true, "snapshot": {...}, "audio": {"device": "BlackHole 2ch", "rate": 96000, "buffer": 64, "callbacks": 1520, "advanced": true, "stream_errors": 0, "start_overloads": 0, "overruns": 0}}
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
| `stream_errors` | Errors the backend reported since the stream started, except `start_overloads`. The stats bar counts these as dropouts |
| `start_overloads` | Overload notices that came before the stream's first callback. They drop no audio and are not dropouts ([panels.md](panels.md#audio-load-and-dropouts-in-the-stats-bar)) |
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

- [x] A control script hovers an output, pins its badge, presses Escape, and the snapshot shows each step (`hover`, `pinned`). App test `a_script_hovers_pins_a_badge_and_unpins_it_with_escape`, and `examples/dev/check-pin-badge.jsonl` in the real app.
- [x] A control script drags a knob's range arc, with Shift, and the assignment's depth changes as in [#217](https://github.com/brylie/LoomDSP/issues/217). App test `a_script_drags_a_range_arc`.
- [x] A `drag` from an output to a tile control makes an assignment, and one undo step undoes it. App test `a_script_drags_an_output_onto_a_control`. An inspector row as the drop target is not tested yet.
- [x] `--measure-idle` still counts no frames when no command arrives.

- [x] App tests, through the control harness and the fake host: `output_safety`, `audio` (device, rate, buffer, and `"default"`), `midi`, and `new` change what the Preferences window shows, and replies give `playing` and `status`.
- [x] A refused name, rate, or buffer gets `ok: false` with the offered values.
- [x] With the stalling fake device, `advanced` is true and then false, and `callbacks` stops growing.
- [x] `examples/dev/check-devices-part-2.jsonl` runs Part 2 of the manual check (device, 96 kHz, smallest buffer) with `--control-script`, and its replies show `advanced: true` after each change. Checked on macOS with BlackHole 2ch, 2026-10-24.
- [x] The manual check says which steps the script replaces, and which still need ears.

### Out of scope

- **A warning in the app** when the device stops calling back. The app could say so in the status bar, but that is a user feature for the preferences spec, not this tool. It is an open point there.
- **Listening.** The script cannot hear a click at the device. `--record` and `loom analyze` cover what the engine plays.

## Script variables

**Status: Agreed** (2026-10-25). Issue: [#340](https://github.com/brylie/LoomDSP/issues/340).

A script that names a device runs only on a machine with that device. `examples/dev/check-devices-part-2.jsonl` sent `"device": "BlackHole"`, `"rate": 96000`, and `"buffer": 32`: one virtual device on one Mac, and a rate and buffer size that another device may not offer, so rule S2 refuses them. A script file is plain JSON lines and cannot read the environment, so every other developer had to edit it.

### Behaviour

`--control-script` replaces `${NAME}` in the string values of a command with the environment variable `NAME`, after the line is read as JSON and before the command runs.

| In the script | `LOOM_RATE=96000` | Becomes |
|---|---|---|
| `"rate": "${LOOM_RATE}"` | the whole string is one variable | `"rate": 96000`, a number |
| `"device": "${LOOM_DEVICE}"` with `LOOM_DEVICE=MOTU` | the whole string is one variable | `"device": "MOTU"`, a string |
| `"path": "${HOME}/out.png"` | the variable is part of a string | `"path": "/Users/ada/out.png"`, text |

Example, Part 2 of the manual check on any machine:

```sh
LOOM_DEVICE="MOTU" LOOM_RATE=96000 LOOM_BUFFER=64 \
  cargo run --release -p loom-app -- examples/dev/check-drone.ron \
  --preferences target/check-prefs.ron --control-script examples/dev/check-devices-part-2.jsonl
```

### Rules

- **V1. Script files only.** The socket (`--control`) takes commands as they are sent. *Why:* a client on the socket is a program that can build its own command. A script file is the only form that cannot.
- **V2. Inside string values only.** The line is parsed as JSON first, and only string values are searched, never keys or the raw text. *Why:* a value with a quote or a backslash in it, such as a Windows path, cannot break the JSON.
- **V3. A whole-string variable keeps its type.** When a string is exactly one `${NAME}`, the value becomes a JSON number or `true` / `false` if it reads as one, and a string otherwise. A variable inside a longer string is always text. *Why:* `rate`, `buffer`, and `value` take numbers, and `on` takes a boolean. Without this, a script could not choose them.
- **V4. An unset variable stops the script.** The reply is `ok: false` and names the variable, the script ends there, and the app quits with status 2. An empty variable counts as unset. There are no default values. *Why:* a silent default such as BlackHole is the bug this section fixes. Status 2 is "could not run", not "a check failed" (status 1), as for `loom analyze`.
- **V5. Names are `[A-Z0-9_]`, starting with a letter.** `$` with anything else, such as `$5` or `${x}`, is left as it is. *Why:* environment variable names on every system fit this, and a price or a regular expression in a string stays as written.
- **V6. The reply says what was used.** A reply to a command that had variables gets a `vars` field: each name and the value it had, for example `"vars": {"LOOM_DEVICE": "MOTU"}`. *Why:* the person reading the output sees which device the run used, without the replies growing for commands that had none.

### Acceptance criteria

- [x] Unit tests for the replacement: a whole-string number, boolean, and string; a variable inside a longer string; two variables in one string; a quote and a backslash in a value; `$5` and `${x}` left alone; an unset and an empty variable refused with the variable's name.
- [x] An app test runs a script file through `--control-script` with a variable set, and sees the command applied (for example `set` with `"value": "${LOOM_TEST_VALUE}"`).
- [x] `examples/dev/check-devices-part-2.jsonl` uses `${LOOM_DEVICE}`, `${LOOM_RATE}`, and `${LOOM_BUFFER}`, and the manual check says to set them. Checked on macOS with two different devices, 2026-10-25: BlackHole 2ch at 96 kHz with 32 frames, and External Headphones at 96 kHz with 64 frames. Every snapshot after a change had `advanced: true`, and the app exited with status 0.
- [x] A script with an unset variable exits with status 2 and names it. Checked on macOS with `--fake-audio`, 2026-10-25.

### Out of scope

- Variables on the socket (V1).
- Default values (`${NAME:-default}`), loops, and conditions. A script that needs those is better written in a real language against the socket.

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
10. **`:name` looks for a parameter first,** then an output, then an input, then a display value (a light or a meter). `MODULE:display:NAME` looks for a display value only: the Level Meter's meter shows its output `out`, so `11:out` is the output and `11:display:out` the meter. `MODULE:input:NAME` and `MODULE:output:NAME` look for a port only: the SVF's `4:cutoff` is its knob, and `4:input:cutoff` its Cutoff input. `MODULE:badge:OUTPUT` is an output's badge. `--pointer` and `--center` use the same names. A name the snapshot gives (`hover`, `pinned`, `drag`) always finds the same part again: it has a prefix exactly when the plain name finds something else. *Why:* a module's parameter and port names rarely clash; when they do, the control is what a script most often wants. A display value comes last because it often repeats a port's name.
11. **A script file runs the same commands.** `--control-script FILE` reads commands from a file, one per line (blank lines skipped), and prints each reply to stdout. It can take values from the environment ([Script variables](#script-variables)). At the end of the file the app keeps running, unless the last command is `quit`. *Why:* the same code serves a quick check with no client, and a check that can be saved next to a patch.
12. **Unix only, for now.** The socket needs Unix domain sockets (macOS and Linux). On Windows `--control` says it is not supported; `--control-script` works everywhere. *Why:* Rust's standard library has Unix sockets on Unix only. TCP on loopback can come later if Windows needs it.
13. **Short socket paths.** A socket path must be shorter than about 100 bytes (104 on macOS, 108 on Linux). A relative path is fine. A socket file left by an earlier run is replaced; any other file at that path is never touched, not even when the app quits. The app removes its socket when it quits. *Why:* the limit is the operating system's; a long temporary folder path fails with "path must be shorter than SUN_LEN".
14. **`sample` measures, a person judges.** It gives one entry per frame with the frame's time, so a script can find the largest jump between frames, frames where the value did not change, and gaps between frames. It starts after the two frames of rule 5. Opening a patch or making a new one ends a running `sample` with the error "another patch was opened while sampling"; other waiting commands still get their answer. The live values are read each frame it records, even when no modulation asks for frames. A new patch can have a module with the same ID and widget name, and the numbers would then describe the wrong module. *Why:* "does the dot move smoothly" then becomes numbers; a person only has to judge how it looks.

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

- **Touch and multi-pointer events.** Not modelled. Pointer commands cover one mouse.
- **Remote access** over the network, and authentication.
- **A scripting language.** Clients are any language that writes JSON lines.
- **Users.** It is not a feature of the app for musicians, and is not in the help.

## Open points

None. The window capture includes the canvas's custom wgpu primitive (checked on macOS, 2026-10-20).
