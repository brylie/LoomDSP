# ADR-0032: Module data lives in typed slots, with audio in a sidecar folder

- **Status:** Accepted
- **Date:** 2026-11-01
- **Related requirements:** FR-LIB-19, FR-LIB-02, FR-ENCAP-08, NFR-RT-01
- **Related decisions:** ADR-0001 (lock-free boundary), ADR-0008 (patch format), ADR-0017 (primitives, traits, and abstractions), ADR-0020 (inferred channel count), ADR-0024 (widgets follow the kind of value, shared data kinds)
- **Specification:** [Module data](../design/module-data.md)

## Context

Sequencers, envelopes with drawn shapes, wavetable readers, samplers, and loopers keep data that is not a parameter: points, steps, audio (FR-LIB-19, OQ-17). ADR-0024 already chose three shared data kinds with fixed capacities: curve, pattern, and buffer. Still open: how a module declares data, how it is written in the patch file, how it reaches the audio thread, and how undo treats it.

Forces:

- The audio thread may not allocate, free, lock, or wait (AGENTS.md rule 1).
- A patch file should stay plain text that diffs well (ADR-0008).
- A buffer can hold minutes of audio, hundreds of megabytes.
- Some modules write data on the audio thread (recording), and the control thread must still be able to save and draw it.

## Options considered

**Where audio is saved.**

1. **A sidecar folder** next to the patch, with WAV files referenced by relative path. The patch stays text. A patch without audio stays one file.
2. **A single bundle file** (a zip of the patch and the audio). One file to share, but not diffable, and a second format.
3. **Inline** (encoded audio in the RON). One file, but megabytes of text and useless diffs.
4. **References only.** The patch never writes audio, so recorded loops are lost.

**How data reaches the audio thread.**

1. **Values in the command queue.** Simple, but a pattern is a few kilobytes and a buffer is far too large.
2. **Allocate on the control thread, swap in, return the old one for freeing,** as schedules are (ADR-0001). For buffers, storage the audio thread can write and the control thread can read without locks.

## Decision

- A module declares **data slots** by name and kind in its descriptor, as it declares parameters. The kinds are ADR-0024's curve, pattern, and buffer.
- Curve and pattern data are written **inline** in the patch file, under the module.
- Buffer data is written as WAV files in a **sidecar folder** next to the patch (`Song.ron` and `Song.audio/`), referenced by relative path.
- New data is **allocated on the control thread, sent to the engine, and the old data is sent back for freeing**. Buffers are fixed-size storage, allocated at the module's declared maximum, which the audio thread writes and the control thread reads without locks.
- **Undo covers edits the player makes**, not audio the engine records.
- Each slot declares **whether it is saved**.

## Consequences

- **Real-time rules hold.** The audio thread never allocates or frees data. Recording writes into space that already exists.
- **Patches stay text.** A patch with curves and patterns is still one diffable file. Audio is in ordinary WAV files any editor can open.
- **Sharing a patch with audio means sharing two things.** The app can offer "export as zip" later; the format does not need it.
- **A missing audio file does not stop loading.** The buffer is empty, and the status bar says which file was missing.
- **Undo memory stays bounded.** A looper set does not fill memory with takes. A module that wants overdub undo offers it as its own control.
- **Lock-free shared buffers need care.** The type is decided in implementation, with a `// SAFETY:` comment if it uses `unsafe` (AGENTS.md rule 7). The engine core stays portable: the sidecar folder is file I/O in the app and CLI, not in `loom-core` (AGENTS.md rule 6).
- **Buffer data comes forward to P1,** so the wavetable reader in FR-LIB-02 can ship in P1. Curve and pattern data, and the editors for all three kinds, stay in P2.
