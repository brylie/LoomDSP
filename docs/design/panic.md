# Panic Specification

One key, one button, and one MIDI trigger that silence everything and clear stuck notes at once, without a click.

- **Kind:** living specification.
- **Status:** Agreed on merge. Not built.
- **Decision behind it:** none. It builds on [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (commands) and [ADR-0019](../adr/0019-output-safety-stage.md) (output safety).
- **Use case and requirements:** UC3, FR-PERF-05.
- **Background:** [Output safety](../learn/output-safety.md).
- **Prior art:** "all notes off" and "panic" buttons on hardware and software synthesizers. Named only to explain the expectation.
- **Issues:** [#25](https://github.com/brylie/LoomDSP/issues/25).
- **Code:** a new engine command next to `ResetModule` in `crates/loom-core/src/engine.rs`, `Controller::panic`, a toolbar button and key in `crates/loom-app`.

## Goal

> A note hangs, or a feedback loop runs away, in the middle of a set. I press one key. The sound stops without a click and the patch is ready to play again, with my knobs, cables, and tempo as they were.

## Behaviour

**What panic does, in order**, all in the engine, from one command:

1. **Fades the output to silence over 10 ms.** The fade belongs to panic and runs whatever the output safety mode is (even Off). *Why:* a hard stop clicks, and a runaway signal can be loud. The same fade-out as device changes is used ([output safety](output-safety.md), rule 14).
2. **At silence, releases all held notes and resets every module.** Held notes are released first (every MIDI Note module sees a note off for every note it holds). Then `reset()` is called on every module slot. That clears envelopes, filters, delay lines and their tails, reverb tails, oscillator phases, sequencer positions, and feedback state, and snaps any smoothing to its targets ([live patching, rule 7](live-patching.md#rules)). Unit-delay state is cleared. *Why all state and not only notes:* a runaway is usually in a feedback loop or a long tail, not in a held note. Letting tails ring would defeat the point.
3. **Clears the output stage.** The DC blocker and the limiter start fresh.
4. **Fades back in over 10 ms,** as a new engine does.

The whole thing takes about 20 ms.

**What panic keeps:** parameter values, cables, modulation assignments, the patch, undo history, the selection, MIDI input connections, MIDI learn mappings, and the **latest values of controllers** (a mod wheel is where the hand left it; that is hardware state, not sound). **The transport keeps running** and keeps its position. *Why:* tempo and position belong to the performance. If the clock should stop, Space stops it. Panic repeated twice in a row is harmless.

**What panic does not do:** it is not an edit. It is not an undo step, does not mark the patch as changed, and is not saved.

**Notes held on a controller** are not held again: a key still down on the keyboard sends no new note on. The player lifts and presses it.

## How to run it

| Way | Detail |
|---|---|
| Key | **Cmd+.** on macOS, **Ctrl+.** on Windows and Linux. It works whatever has focus, including a text field, and during a drag. *Why:* a panic key must always work. |
| Button | **Panic** in the toolbar, next to **Hold note**. A press runs it. |
| MIDI | A trigger set in Preferences (below). |
| Control channel | `{"cmd": "panic"}`, and `press` with `button: "panic"`. |

### MIDI trigger

Panic is an app action, not a parameter, so [MIDI learn](midi-learn.md) (which maps controls to parameters) does not cover it. It has its own mapping in **Preferences**, in a new **Panic** section: a **Learn** button, the mapping shown as "Channel 1, controller 123" or "Channel 10, note 36", and a **Clear** button. Press Learn, then move a control or press a pad: the next control change or note on sets the mapping.

- It is saved in `preferences.ron` as `panic: Some(Cc(channel, controller))` or `Some(Note(channel, note))`, default `None`. *Why preferences and not the patch:* a panic pad belongs to the player's rig. It must work on every patch, and a patch from someone else must not be able to take over a pad.
- A control change triggers when the value rises through 64. A note triggers on note on. The same control or note keeps going to the modules as well (the controller's other uses are not hidden).
- Panic through MIDI works while learn mode is on or off, and with no patch changes.
- The mapping is looked up in the engine's MIDI path, so it needs no UI thread: the engine runs panic at the start of the buffer in which the message arrived.

## Rules

1. **One command.** `Command::Panic` goes through the command queue, takes effect at the start of a buffer, and does all four steps in the audio thread without allocating. *Why:* hard rule 1, and one place for the order.
2. **Steps 1 and 4 are sample-exact ramps, and the reset is sample-exact too.** The fade is 10 ms counted in frames from the frame where panic takes effect. The reset happens at the frame where the fade reaches 0, inside a block if need be (the engine splits the block there, as it does for other sample-exact changes), and the fade-in starts on the next frame. The result is bit-identical at block sizes 1, 77, and 512 for the same command at the same frame. *Why:* resetting at the next buffer boundary would start the fade-in at a different frame for each block size, which breaks hard rules 2 and 3.
3. **A second panic during the fade** restarts nothing: it is ignored until the first one finishes. *Why:* mashing the key must not lengthen the silence.
4. **Panic needs no audio device.** With `--fake-audio` or no device, the engine handles the command the next time it is processed. *Why:* tests.

## Acceptance criteria

- [ ] Given a held note on MIDI Note into an envelope into Audio Out, when panic runs, then the output reaches silence within 10 ms with no step larger than the click threshold ([audio QA](audio-qa.md)), the gate is low, and the envelope is at 0.
- [ ] Given a delay with feedback ringing, when panic runs, then after the fade the delay output is 0 and stays 0.
- [ ] Given a running transport, when panic runs, then the transport is still running at a continuous position.
- [ ] Given a parameter edited and unsaved, when panic runs, then the value, the cables, and the unsaved mark are unchanged, and there is no new undo step.
- [ ] Given a sound that returns after panic, then it fades in over 10 ms, and a tone at a fixed pitch restarts from phase 0.
- [ ] Output with panic at frame N is the same at block sizes 1, 77, and 512 (engine test). The `no_alloc` tests cover it.
- [ ] The key works with a text field focused. The button and the control channel command run it.
- [ ] Given a MIDI trigger set to CC 123, when a control change of 127 arrives, then panic runs; a value of 0 does nothing; the mapping is in `preferences.ron` and is not in the patch (test with the fake host).
- [ ] Manual: a stuck-note and a runaway-feedback patch, on a real device, stop cleanly with the key and with a MIDI pad.

## Out of scope

- Stopping the transport or resetting its position.
- A "soft" panic that lets tails ring.
- Mapping panic to a computer-keyboard shortcut other than the default.
- Panic from a plugin host (all-notes-off from the host is a later plugin feature).

## Open points

- **Should panic also stop the transport?** Leaning: no (see above). A player who wants both can patch a Button or press Space. Revisit after playing with it.
