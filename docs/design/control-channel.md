# Control Channel Specification

A development-only way for a script or an AI agent to drive the running app by name and read back what it shows.

- **Kind:** living specification.
- **Status:** Implemented
- **Decision behind it:** None. It builds on the plain-data UI snapshot ([ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md)).
- **Use case and requirements:** Developer tooling. No user requirement.
- **Issues:** [#207](https://github.com/brylie/LoomDSP/issues/207) (part 3), `press` and `arm`: [#256](https://github.com/brylie/LoomDSP/issues/256); places in the snapshot: [#250](https://github.com/brylie/LoomDSP/issues/250); related [#190](https://github.com/brylie/LoomDSP/issues/190) (accessibility).
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

## Rules

1. **Off unless asked for.** Nothing listens unless the app is started with `--control`. *Why:* a music app should not open a door on a user's machine without being asked.
2. **Local only.** It listens on a local socket, never on the network. *Why:* commands can open and save files; only processes of the same user should reach it.
3. **Same path as the UI.** Each command becomes the `Message` the UI would send, and goes through `App::update`. So undo steps, the status bar, and errors are the ones a person would get. *Why:* a check through the channel should test what users get, not a side door.
4. **Names, not indices.** Parameters and ports are named as in patch files (`"4:cutoff"`, `"3:out"`). Toolbar buttons have fixed names, not their labels: a label can change while the app runs (Hold note becomes Release note). *Why:* names are stable IDs, and a script that reads like the patch is easy to check.
5. **One at a time, in order.** A reply comes once the command has been applied and the window has drawn it: the app waits two frames (the one already on its way, then the one with the change) before it answers, captures, or searches the window. A `cc` also waits for the engine to take its control change (up to about two seconds; a change from a MIDI port in between is not taken for it), so a script that arms a parameter, sends a `cc`, and then turns learn mode off keeps the mapping. An error, `snapshot`, and `quit` answer at once. *Why:* the next `snapshot`, `locate`, or `screenshot` then sees the result, with no sleeps in the script.
6. **Nothing on the audio thread.** MIDI goes through the same shared sender as a MIDI port. The engine is reached only through the `Controller`. *Why:* real-time safety (hard rule 1).
7. **No messages while idle.** The listener wakes the app only when a command arrives. *Why:* every message redraws the window, and `--measure-idle` must still count zero frames for an idle patch.
8. **Errors are replies, not crashes.** A bad command gets `ok: false` and a message naming what was wrong. The app keeps running. *Why:* an agent learns from the error and tries again.
9. **No dialogs.** While the channel is on, errors that would open a dialog (a failed open or save) go to the reply and the status bar instead. Commands never open a file dialog or ask to save: `key` refuses the New, Open, and Save shortcuts (Cmd or Ctrl with N, O, or S), and `press` refuses the `new`, `open`, `save`, and `save-as` buttons. Both point to the `open` and `save` commands. *Why:* a modal dialog would stop the app, and nobody is there to close it.
10. **`:name` looks for a parameter first,** then an output, then an input. *Why:* a module's parameter and port names rarely clash; when they do, the control is what a script most often wants.
11. **A script file runs the same commands.** `--control-script FILE` reads commands from a file, one per line (blank lines skipped), and prints each reply to stdout. At the end of the file the app keeps running, unless the last command is `quit`. *Why:* the same code serves a quick check with no client, and a check that can be saved next to a patch.
12. **Unix only, for now.** The socket needs Unix domain sockets (macOS and Linux). On Windows `--control` says it is not supported; `--control-script` works everywhere. *Why:* Rust's standard library has Unix sockets on Unix only. TCP on loopback can come later if Windows needs it.
13. **Short socket paths.** A socket path must be shorter than about 100 bytes (104 on macOS, 108 on Linux). A relative path is fine. A socket file left by an earlier run is replaced; any other file at that path is never touched, not even when the app quits. The app removes its socket when it quits. *Why:* the limit is the operating system's; a long temporary folder path fails with "path must be shorter than SUN_LEN".

## Acceptance criteria

- [ ] Without `--control`, the app opens no socket.
- [ ] Tests run commands on an app with `--fake-audio`: select a module, set a parameter by value and by position, send a note, and read the snapshot that shows the change.
- [ ] A test locates a control, a port, and a text by name, at the same place the headless `Ui` harness finds them.
- [ ] A manual check drives the real window through the socket with `nc -U`, and saves a screenshot that shows the canvas.
- [ ] A script file with a `quit` at the end runs and exits with status 0.
- [ ] `set` on a mapped parameter, then `undo`, restores the value, as in the UI.
- [ ] A bad command gets `ok: false` with a message, and the next command works.
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
