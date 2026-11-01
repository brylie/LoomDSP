# Module Data Specification

How modules keep data beyond parameters (curves, patterns, and audio), how it is saved with a patch, how it reaches the audio thread, and how undo treats it.

- **Kind:** living specification.
- **Status:** Agreed. [Buffer data](#buffer-data) and [Loading audio into a slot](#loading-audio-into-a-slot) are P1. The rest is P2. The choices marked "Settled" were made with the maintainer on 2026-11-01. Nothing is built yet.
- **Decision behind it:** [ADR-0030](../adr/0030-module-data-in-typed-slots.md) (typed slots, sidecar audio, swap and return). [ADR-0024](../adr/0024-widgets-follow-the-kind-of-value.md) for the three data kinds and their editors.
- **Use case and requirements:** FR-LIB-19, FR-LIB-02 (wavetable reader), FR-ENCAP-08. Settles OQ-17.
- **Builds on:** [Panel widgets: data kinds](panel-widgets.md#data-kinds) (what each kind holds and its limits), [Undo](undo.md), [Open, Save, and Save As](files.md), [Autosave](autosave.md), [Copy, paste, and duplicate](clipboard.md), [Stereo cables](stereo.md).
- **Background:** none yet.
- **Prior art:** DAWs keep audio in a project folder next to the project file. Named only to explain the convention.
- **Issues:** [#50](https://github.com/brylie/LoomDSP/issues/50). Users of the data: [#281](https://github.com/brylie/LoomDSP/issues/281) (curve), [#287](https://github.com/brylie/LoomDSP/issues/287) (pattern), [#288](https://github.com/brylie/LoomDSP/issues/288) (buffer and waveform).
- **Code:** none yet.

## Goal

> I draw an LFO shape, program a 16-step pattern, and load a wavetable. I save, close, and reopen: all three are back. I undo my last step edit and only that step changes. I record a loop into a buffer during a set, save, and the loop is there next time.

## Data slots

A module declares data slots in its descriptor, next to its parameters.

| Field | Meaning |
|---|---|
| `name` | Stable ID within the module, like a parameter name. |
| `label`, `description` | Text, as for ports and parameters (ADR-0016). |
| `kind` | Curve, pattern, or buffer, with the options of [Panel widgets: data kinds](panel-widgets.md#data-kinds): wrap and symmetry for a curve; lane count, step count, and gate or value lanes for a pattern; channels, maximum length in seconds, and frame length for a buffer. |
| `saved` | Whether the slot is written with the patch. **Settled:** each slot decides. A loaded or drawn slot is normally saved; a scratch buffer can be declared never saved. |

- The registry refuses two slots with one name, and a widget bound to a slot of another kind (ADR-0024).
- A new module starts with each slot's default: a straight curve from (0, 0) to (1, 1), an empty pattern, or a silent buffer.
- The Buffer module (MT-SAMPLE-01) gets a **Keep with patch** setting, on by default, that sets its slot's `saved`.

## In the patch file

Curve and pattern data are written inline, under the module. The module IDs below are examples:

```ron
PatchModule(id: 4, kind: "loom.seq.lanes@1", data: {
    "pattern": Pattern(lanes: [
        Gates(steps: 16, on: [0, 4, 8, 12], chance: {6: 0.5}, repeat: {12: 2}, accent: [0, 8]),
        Values(steps: 16, values: [0.0, 0.25, 0.5, 0.25, /* ... */]),
    ]),
}),
PatchModule(id: 5, kind: "loom.mod.curve@1", data: {
    "shape": Curve(points: [(0.0, 0.0, 0.0), (0.5, 1.0, -0.3), (1.0, 0.0, 0.0)], markers: {}),
}),
PatchModule(id: 6, kind: "loom.osc.wavetable@1", data: {
    "table": Buffer(file: "Song.audio/6-table.wav", frame_length: Some(2048), slices: []),
}),
```

- Step attributes are written only where they differ from their defaults (chance 1, repeat 1, no accent).
- A slot that still has its default is not written. *Why:* patches stay short, and the example patches do not change when a module gains a slot.
- A slot with `saved: false` is never written.
- A value that does not fit the slot (too many points, a step past the end) is cut to fit when loading, and the status bar says so. *Why:* a patch must always open.

## Buffer data

**Settled:** audio is saved in a sidecar folder next to the patch.

- Saving `Song.ron` writes each saved buffer to `Song.audio/<module id>-<slot>.wav`. A patch with no saved buffers has no folder.
- Files are 32-bit float WAV at the engine's sample rate when saved, mono or stereo as the slot holds. *Why:* no loss, and every audio editor reads them.
- Paths in the patch are relative to the patch file, so the patch and its folder can move together.
- **Save** rewrites only buffers that changed since the last save, and removes files in the folder that the patch no longer uses. It removes only files whose names follow the pattern above. *Why:* a player's own files in that folder are never deleted.
- **Save As** writes a new folder for the new name. The old folder is left as it was.
- Writing follows the [files](files.md) rule: a temporary file, then a rename.
- **Loading:** a file at another sample rate is resampled on the control thread when the patch loads. A missing or unreadable file leaves the buffer silent. The status bar says: `Missing audio for "Wavetable": Song.audio/6-table.wav`. A later save does not write that slot until it has audio again. *Why:* saving silence over a file that was only moved would lose it.
- **Autosave** writes buffers that changed into the recovery folder beside the recovered patch ([autosave](autosave.md)).
- A buffer's maximum length is declared by the module ([panel widgets](panel-widgets.md#data-kinds)). Loading a longer file keeps the start and says so in the status bar.

## Loading audio into a slot

P1, so the wavetable reader can ship (FR-LIB-02).

- The inspector shows each buffer slot as a row: label, length, channels, and a **Load audio file** button. It opens a file dialog for WAV files.
- Loading replaces the slot's contents. It is one undo step.
- The audio is decoded and resampled on the control thread, then sent to the engine. Audio keeps playing.
- The waveform widget that draws a buffer is P2 ([#288](https://github.com/brylie/LoomDSP/issues/288)).

## Reaching the audio thread

Rules for implementers, from ADR-0001 and ADR-0030.

1. **Curve and pattern:** a new value is built on the control thread in a fixed-capacity box, sent to the engine through the command queue, and swapped in at a buffer boundary. The old box goes back on the return queue to be freed on the control thread.
2. **Buffer:** storage is allocated on the control thread at the slot's maximum when the module is created or its maximum changes. Loading a file fills new storage and swaps it in, as in rule 1.
3. **Recording:** a module that records writes into its existing storage on the audio thread, and publishes how much it has written. The control thread reads that for saving and drawing, without locks and without stopping the audio thread.
4. A data change applies at a buffer boundary. *Why:* the same rule as schedule swaps. Block-size invariance (AGENTS.md rule 2) holds because a data change is an edit, like a parameter change from the screen.
5. Same patch, same data, same output (AGENTS.md rule 3).

## Undo

**Settled:** undo covers what the player does, not what the engine records.

- Drawing on a curve, editing steps, loading a file, and clearing a buffer are undo steps. One gesture is one step ([undo](undo.md)).
- Audio a module records live is not an undo step. *Why:* recording is a performance action, like playing a note, and keeping every take would grow memory without limit. A module that wants overdub undo offers it as its own control.
- Curve and pattern steps keep the slot before and after.
- Buffer steps keep the previous audio in memory. Buffer steps together may hold up to **256 MB**. When a new step would pass that, the oldest buffer steps are dropped, and their history entries say "can no longer be undone".

## Copies

- **Copy and paste** of a module copies its data. Curve and pattern data are in the clipboard text. Buffer audio is not: pasting in the same app copies the audio from memory, and pasting into another app gives a silent buffer with a status message. *Why:* the clipboard is text ([clipboard](clipboard.md)), and megabytes of audio do not belong there.
- **Stereo copies** of a module share its data (ADR-0020). A stereo buffer is one buffer with two channels.
- **Stack copies and select members** in a sub-patch share the data of their one definition ([sub-patches](sub-patches.md#modes)).
- **Slot swaps** (OQ-21): each part's data stays in the patch, keyed by the part, and comes back when the player swaps back. Data is never converted between parts ([sub-patches](sub-patches.md#slots)).

## Rules

1. **The audio thread never allocates or frees data.** *Why:* AGENTS.md rule 1.
2. **A patch always opens.** Data that does not fit is cut, and missing audio is silent, with a message. *Why:* a moved file must not lose a set.
3. **Save never deletes what it did not write.** *Why:* the sidecar folder is in the player's files.
4. **Undo is bounded.** *Why:* a long session must not run out of memory.

## Acceptance criteria

P1 (buffer):

- [ ] A module with a buffer slot, loaded from a WAV file, saves to the sidecar folder and reloads with the same samples (test).
- [ ] Loading a 48 kHz file into a 44.1 kHz engine resamples it on the control thread (test).
- [ ] A missing sidecar file opens the patch with a silent buffer and a status message; saving does not write that slot (test).
- [ ] Save removes an unused file it wrote, and leaves a file of another name (test).
- [ ] Loading a file is one undo step; undo restores the earlier audio (test).
- [ ] No allocation on the audio thread when a buffer is swapped in (`no_alloc` test).

P2 (curve and pattern, recording):

- [ ] Curve and pattern data round-trip through the patch file; default slots are not written (test).
- [ ] A pattern with more steps than its slot holds is cut on load with a message (test).
- [ ] One step-grid gesture is one undo step (`Ui` harness, with #287).
- [ ] A module that records into a buffer during playback saves what it recorded; recording is not an undo step (test).
- [ ] Undo memory for buffer steps stays under 256 MB, and dropped steps say so (test).
- [ ] Pasting a module with a curve copies the curve; pasting a buffer module in the same app copies its audio (test).
- [ ] A slot swap and swap back restores the first part's pattern (test, with sub-patches).

## Out of scope

- A single-file bundle (zip) for patches with audio. Leaning: an "export as zip" command later, if players ask.
- Converting data between kinds or between parts.
- Editing audio (cut, fade, normalise). The waveform widget only sets markers.
- Compressed audio formats (FLAC) in the sidecar folder.

## Open points

- **Resampling quality.** Which resampler, and from which permissive crate (ADR-0006). Decide in implementation; it runs only on the control thread.
