# Control Channel Specification

A development-only way for a script or an AI agent to drive the running app by name and read back what it shows.

- **Kind:** living specification.
- **Status:** Draft
- **Decision behind it:** None. It builds on the plain-data UI snapshot ([ADR-0021](../adr/0021-plain-data-view-models-and-keyed-identity.md)).
- **Use case and requirements:** Developer tooling. No user requirement.
- **Issues:** [#207](https://github.com/brylie/LoomDSP/issues/207) (part 3), related [#190](https://github.com/brylie/LoomDSP/issues/190) (accessibility).
- **Code:** not yet.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

An agent building a feature wants to check it in the real app: "select the filter, turn Cutoff to 800 Hz, play middle C, and show me the window". Today it has three partial tools:

- `--screenshot` and `--dump-ui` show a still frame of a patch at start. They cannot show what happens after an edit, a note, or some time passing.
- The headless `Ui` harness drives the real view, but only inside a Rust test.
- Desktop automation can see and click the window of `LoomDSP.app`, but by pixel only: the window has no accessibility tree (#190), so a control cannot be found by name or read.

The control channel fills the gap: commands by name into the running app, and replies with what the UI shows. It also tells pixel-based automation where a named control is, so its clicks land.

## Behaviour

Started with a flag, for example:

```sh
loom-app examples/patches/subtractive.ron --fake-audio --control /tmp/loom.sock
```

A client sends one JSON object per line and gets one JSON object per line back, in the same order:

```text
> {"id": 1, "cmd": "select", "modules": [4]}
< {"id": 1, "ok": true}
> {"id": 2, "cmd": "set", "module": 4, "param": "cutoff", "value": 800}
< {"id": 2, "ok": true, "value": 800.0}
> {"id": 3, "cmd": "locate", "target": "4:cutoff"}
< {"id": 3, "ok": true, "x": 512.0, "y": 240.0, "width": 22.0, "height": 22.0, "scale": 2.0}
> {"id": 4, "cmd": "set", "module": 4, "param": "nonsense", "value": 1}
< {"id": 4, "ok": false, "error": "module 4 (SVF) has no parameter `nonsense`"}
```

### Commands

| Command | Fields | Does | Reply |
|---|---|---|---|
| `snapshot` | | Nothing | The UI snapshot, the same JSON as `--dump-ui`, with live values |
| `select` | `modules` | Selects these modules, as clicks would | |
| `set` | `module`, `param`, `value` | Sets a parameter, as the inspector does: one undo step | The value as stored (clamped) |
| `add` | `kind`, optional `at` | Adds a module, as the browser does | The new module ID |
| `remove` | `modules` | Removes modules, as Delete does | |
| `connect` | `from`, `to` (`"3:out"`, `"4:in"`) | Connects a cable | |
| `disconnect` | `to` | Removes the cable into this input | |
| `undo`, `redo` | | As the toolbar buttons | The step name |
| `open` | `path` | Opens a patch, as File > Open does after the dialog | |
| `save` | `path` | Saves the patch, as Save As does after the dialog | |
| `note` | `note`, `velocity`, `on` | A MIDI note on or off, as a MIDI port sends it | |
| `cc` | `controller`, `value`, optional `channel` | A MIDI control change | |
| `key` | `key`, optional `modifiers` | A key press, as the keyboard listener sends it | |
| `locate` | `target`: a module (`"4"`), port (`"3:out"`), control (`"4:cutoff"`), or text | Nothing | Where it is in the window, in logical pixels, and the scale factor |
| `screenshot` | `path` | Saves a PNG of the window as it is now | The image size |
| `wait` | `ms` | Replies after this long, with the app running | |
| `quit` | | Closes the app, without asking to save | |

## Rules

1. **Off unless asked for.** Nothing listens unless the app is started with `--control`. *Why:* a music app should not open a door on a user's machine without being asked.
2. **Local only.** It listens on a local socket, never on the network. *Why:* commands can open and save files; only processes of the same user should reach it.
3. **Same path as the UI.** Each command becomes the `Message` the UI would send, and goes through `App::update`. So undo steps, the status bar, and errors are the ones a person would get. *Why:* a check through the channel should test what users get, not a side door.
4. **Names, not indices.** Parameters and ports are named as in patch files (`"4:cutoff"`, `"3:out"`). *Why:* names are stable IDs, and a script that reads like the patch is easy to check.
5. **One at a time, in order.** A reply comes once the command has been applied and the window has drawn it. *Why:* the next `snapshot`, `locate`, or `screenshot` then sees the result, with no sleeps in the script.
6. **Nothing on the audio thread.** MIDI goes through the same shared sender as a MIDI port. The engine is reached only through the `Controller`. *Why:* real-time safety (hard rule 1).
7. **No messages while idle.** The listener wakes the app only when a command arrives. *Why:* every message redraws the window, and `--measure-idle` must still count zero frames for an idle patch.
8. **Errors are replies, not crashes.** A bad command gets `ok: false` and a message naming what was wrong. The app keeps running. *Why:* an agent learns from the error and tries again.

## Acceptance criteria

- [ ] Without `--control`, the app opens no socket.
- [ ] A test starts the app with `--fake-audio --control`, and through the channel selects a module, sets a parameter, sends a note, reads the snapshot (the parameter and a live value changed), locates a control by name, and saves a screenshot.
- [ ] `set` on a mapped parameter, then `undo`, restores the value, as in the UI.
- [ ] A bad command gets `ok: false` with a message, and the next command works.
- [ ] With `--control` and `--measure-idle` and no commands, an idle patch counts no extra frames.
- [ ] `tests/no_alloc.rs` still passes.
- [ ] AGENTS.md lists the command, and an example session.

## Out of scope

- **Pointer events.** Pressing, dragging, and hovering at window coordinates: desktop automation does that, with `locate` to aim. The headless `Ui` harness covers it in tests.
- **Remote access** over the network, and authentication.
- **A scripting language.** Clients are any language that writes JSON lines.
- **Users.** It is not a feature of the app for musicians, and is not in the help.

## Open points

1. **Transport.** A Unix domain socket (`--control /tmp/loom.sock`; works with `nc -U` and `socat`; not in Rust's standard library on Windows), JSON lines on stdin and stdout (simplest; but stdout also carries the `--bench` report), or TCP on `127.0.0.1` (works everywhere; any local process can connect). Leaning: Unix socket, with TCP on loopback later if Windows needs it.
2. **Value or knob position for `set`.** `value` in the parameter's units (800 Hz) reads like a patch file. `position` (0 to 1) is what the knob does, and is how the app sends it today. Leaning: `value`, with `position` as an option.
3. **A script file.** `--control-script steps.jsonl` would run the same commands from a file and print the replies, with no client needed. Leaning: yes, after the socket works: it is the same code reading another source.
4. **Screenshot capture.** Iced can capture the window's last frame (`window::screenshot`). Check that it includes the canvas's custom wgpu primitive before relying on it.
