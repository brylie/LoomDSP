# Devices Plugged In and Out Specification

What the app does when a MIDI controller is plugged in or removed, and when the audio interface disappears, while a patch plays.

- **Kind:** living specification.
- **Status:** MIDI inputs: Implemented ([#27](https://github.com/brylie/LoomDSP/issues/27)); the manual checks on real controllers are still to do. `midir` reports no error on an open connection, so a removal is seen as a missing name or a new port identity. Where the system keeps the same identity across a replug, a quick replug within one poll is the accepted limit below. Audio device loss: Agreed, not built ([#28](https://github.com/brylie/LoomDSP/issues/28)).
- **Decision behind it:** none. It builds on [App preferences, Audio and MIDI devices](preferences.md#audio-and-midi-devices) and [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md).
- **Use case and requirements:** UC3, FR-MIDI-08, FR-APP-04, NFR-UX-05.
- **Issues:** [#27](https://github.com/brylie/LoomDSP/issues/27) (MIDI), [#28](https://github.com/brylie/LoomDSP/issues/28) (audio loss). Exit criteria for P1.
- **Code:** `crates/loom-app/src/devices.rs` (the `Host` trait and its fake), `App::poll_midi` in `crates/loom-app/src/settings.rs`, `midi_inputs` and `connect_midi_input` in `crates/loom-host-desktop`, and the held-note sets (`InputNotes`) in `crates/loom-core/src/engine.rs`.

## Goal

> I trip over the USB cable of my keyboard in the middle of a song. I plug it back in and keep playing, without opening any window. Later the audio interface drops out. The app says so, keeps my patch, and sound returns when the interface does.

## MIDI inputs

### Behaviour

- **The app watches the list of MIDI inputs** four times a second, on a thread that is not the audio thread (the system offers no portable callback, so the app polls). The Preferences window shows the same list and updates when it changes, so MIDI needs no Refresh button.
- **A new input** that is not in `midi.off` is opened at once. The status bar says `MIDI: "Keys" connected.` An input in `midi.off` stays closed.
- **A removed input** is closed. Every note it was holding is released, in the engine, with a note off for each. The status bar says `MIDI: "Keys" disconnected.` The app keeps running.
- **An input that comes back** is opened as a new connection. Its notes are not doubled, because the old connection was closed when it was removed and a connection never outlives its device.
- **Two inputs with the same name** (two identical controllers) are told apart by their order on the system, and shown as `Pad` and `Pad (2)`. A saved `midi.off` entry matches by full name, so turning off `Pad` turns off the first. *Why:* the system gives no better identity portably.
- **Any sign that a connection is gone counts as a removal,** even if the name is listed again: a change of the port's identity (the system's port ID, where the host gives one) seen in a poll. `midir` reports no error on an open connection, so an error cannot be a sign. The app closes the old connection, releases its notes, and opens the new one. *Why:* a key held when the cable comes out and released while it is unplugged never sends its note off. Waiting for the name to leave the list would miss a quick replug and leave the note stuck. There is no minimum gap: a removal is a removal.
- **The limit.** A gap shorter than one poll (250 ms) with no identity change cannot be seen. That is the cost of polling, and it is accepted. A note held across such a gap is released the next time its key is pressed and released, or by panic ([panic](panic.md)).

### Held notes belong to an input

Each MIDI event carries the index of its input. The engine keeps, for each open input, a set of the notes it holds (16 channels by 128 notes, 2 KB), allocated when the input is opened, on the control thread. A removal sends `ReleaseInput(index)`: for every held note on that input, the engine delivers a note off to the modules, then clears the set. *Why:* the modules see only a stream of messages, and "release what this device held" needs to know which device sent what. Two inputs playing the same note: the note ends only when the last holder releases it (a count per note), so removing one keyboard does not cut a chord that another is still holding.

Hold note (the app's own button) is its own source and is not released by a device removal.

### Acceptance criteria

The fake host has a list of inputs that tests change.

- [ ] Given a running app, when an input appears, then it is opened without opening Preferences and a note on it plays.
- [ ] Given an input in `midi.off`, when it appears, then it is not opened.
- [ ] Given a held note on an input, when the input is removed, then a note off reaches the modules, the app keeps running, and the engine's held-note set for it is empty.
- [ ] Given the same input back, then notes play once (not doubled).
- [ ] Given two inputs holding the same note, when one is removed, then the note stays on until the other releases it.
- [ ] Given an input removed and added 10 times quickly (with the fake host), then there is no panic, no stuck note, and no leaked connection.
- [ ] Given a held note, when the input is removed and re-added with the same name before the next poll but with a new port identity (or after a connection error), then the old connection is closed, a note off is delivered, and notes play once on the new connection.
- [ ] The allocation tests pass while inputs come and go.
- [ ] Manual, on macOS, Windows, and Linux, with a real controller (list the OS version and model in the pull request): plug in while the app runs and play; unplug with a held note and hear it stop; plug back in and play; unplug and plug 10 times quickly.

## Audio device loss

### Behaviour

**Noticing.** The app notices loss in three ways: the stream reports an error (device removed or not available); the callback stops for more than one second while the stream is believed to be running (a device that fails silently, or the computer waking from sleep); or the host reports that the output device list no longer contains the playing device. Any of them starts the same path. The audio thread is not involved: the stream error handler sends a message to the app through a channel.

**What happens next**, by what the player chose in Preferences:

| Chosen device | What the app does |
|---|---|
| **System default** | At once, opens the current system default (which the system has probably moved to the speakers). The status bar says `Audio: "<old>" was lost. Now playing on "<new>".` |
| **Named** | Plays nothing, keeps the engine idle, and the status bar says `Audio: "MOTU M4" was lost. Plug it in again, or choose another device in Preferences.` The app watches the device list once a second. When the device is back, it reopens it with the saved rate and buffer size, and says `Audio: "MOTU M4" is back.` |
| No device was playing | Nothing to do. |

- **The patch and every edit are kept.** The controller owns the graph; the engine is only rebuilt. While no audio runs, the engine stays idle exactly as it does with `--no-audio`, and edits still work. *Why:* the player must be able to keep working, or save, after a drop-out.
- **A new engine starts fresh,** as in [restarting audio](preferences.md#what-a-change-does): module state resets, the transport keeps its settings and starts at beat 0, and the engine fades in over 10 ms. *Why:* the old engine's last block never finished.
- **Choosing another device** in Preferences while the old one is lost works as a normal change, except that there is no old stream to fade out. The saved choice changes to the new device.
- **No fade-out is possible** for a lost device (there is nothing to play on). The safety stage ([output safety](output-safety.md)) protects the new stream.
- **Sleep and wake.** On wake, a stream that stalled or errors is treated as lost: with System default, the app reopens it at once; with Named, it finds the device again within a second. If the device is not there, the message from the table is shown. *Why:* the same rule for every cause is easier to test and to explain.
- **The message stays** in the status bar until the audio is back or the player chooses a device. A lost device is not a quiet event.
- **No crash and no modal.** *Why:* a dialog on stage is another thing to dismiss.
- **A device that fails to reopen** repeats once a second for up to 10 tries, then stops and shows `Audio: could not reopen "<name>": <reason>. Choose a device in Preferences.`

### Rules

1. **Never write the fallback.** A device chosen automatically because the Named one is lost is not saved ([preferences D2](preferences.md#rules)).
2. **Reconnecting never changes a saved choice.**
3. **The engine is never rebuilt twice for one loss.** A second error from the same dead stream is ignored.
4. **A restart of audio does not touch MIDI.** Open inputs stay open.

### Acceptance criteria

The fake host can emit a `Lost` error for a device, remove a device from the list, and add it back.

- [ ] Given a playing patch on a Named device, when the device is lost, then the app does not crash, the status bar names the device, the engine is idle, and edits made now are kept.
- [ ] Given that, when the device returns, then audio plays on it again with the saved rate and buffer size, fading in over 10 ms, and the edits are still in the patch.
- [ ] Given System default, when the device is lost, then the app plays on the new default at once.
- [ ] Given a lost Named device, when the player chooses another output, then sound plays there and the saved choice is the new device.
- [ ] Given a stream that stops calling back for more than one second, then it is treated as lost.
- [ ] A second error from the same stream does not rebuild the engine twice.
- [ ] Manual, on macOS, Windows, and Linux, with the OS version and audio device listed in the pull request: unplug the interface while a patch plays (no crash, a message names the device); plug it in (sound returns); with the device gone choose another output (sound plays there); edits made while it was gone are kept; sleep the computer while playing and wake it (sound returns, or the message says why not).

## Out of scope

- Moving a playing stream to another device without a gap.
- Audio input loss (see [audio input](audio-input.md)).
- Notifying the player outside the app (a system notification).
- Plug and play of an audio device while the Preferences window is open beyond updating its list.

## Open points

- Whether a lost Named device should, after the 10 tries, fall back to the system default. Leaning: no, silence plus a clear message is better than sound from a surprise device on stage.
