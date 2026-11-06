# ADR-0038: Notes travel as per-voice signals in a polyphonic bundle, which is an inferred cable width

- **Status:** Accepted
- **Date:** 2026-11-06
- **Related requirements:** FR-SIG-08, FR-VOICE-01 to FR-VOICE-04, FR-LIB-13, FR-LIB-20, NFR-RT-01, NFR-PERF-01, NFR-PERF-03
- **Related decisions:** ADR-0007 (every cable is `f32`), ADR-0011 (everything is a module), ADR-0020 (inferred channel count), ADR-0022 (copies take state from the running instance), ADR-0035 (pure algorithms in `loom-dsp`)
- **Specification:** [Polyphony and voice allocation](../design/polyphony.md)

## Context

M2 ends with an 8-voice polysynth built from primitives. M3 builds note effects (arpeggiator, chord, strum, FR-LIB-13) and note clips that play chords (FR-LIB-20, UC10). Two questions were open:

- **OQ-16:** how notes travel between modules. The roadmap asked for a design spike before the first note effect ([#666](https://github.com/brylie/LoomDSP/issues/666)).
- **FR-SIG-08:** how a cable carries one value per voice, when every cable is `f32` at the sample rate (ADR-0007).

Forces:

- The audio thread may not allocate, lock, or wait (NFR-RT-01). Any note model has to place a note on its exact sample at every block size (AGENTS.md rules 1 and 2) and give the same output every time (rule 3).
- Notes come from modules, not only from a keyboard: a chord sequencer, a note cycle with overlapping notes, a piano roll, Chord, Strum, and Arpeggiator. A chord has a note count that changes from step to step.
- The engine already runs a module as two copies when a stereo cable reaches it (ADR-0020), carries state into a new copy (ADR-0022), and gives each port a downmix rule. That machinery exists in `crates/loom-core/src/width.rs` and `compile.rs`.
- NFR-PERF-01 budgets about 100 modules at 8 voices in under a quarter of a core. NFR-PERF-03 asks for SIMD across voices where possible.
- One cable kind is simpler to learn, draw, and schedule than two.

## Options considered

**How notes travel (OQ-16).**

1. **Per-voice signals.** Pitch, gate, velocity, and expression as ordinary signals, one value per voice, in a polyphonic bundle. For: one signal model, so anything can modulate a note, and a note's timing is a gate rising on its sample. Against: every module that makes or changes notes needs voice allocation, and the voice count is a fixed capacity.
2. **Event cables.** A second cable kind carrying sample-stamped note events. For: any number of notes at once, exact timing, natural for MIDI effects. Against: a second kind of cable, its own scheduling and queue sizes, its own drawing, and a rule for every place where an event meets a signal.
3. **Both, with converters.** Each module uses the form that fits it. For: no module is forced into the wrong form. Against: two models to learn, and the converters are where the hard questions move to.

**How a bundle runs (FR-SIG-08).**

- **a. Voices as copies.** A bundle is a cable width, inferred at compile time as stereo is, and a module fed a bundle runs as one copy per voice. For: reuses width inference, downmix, and state copy; a module needs no change to become polyphonic. Against: N copies cost N times one module, and SIMD across voices needs the copies run in lockstep later.
- **b. Voices inside the module.** Each module processes N voices itself, with SIMD. For: the fastest path. Against: every module is rewritten for N, including ones that have no reason to know about voices.
- **c. A separate polyphonic module set.** For: nothing existing changes. Against: two libraries, and a player must learn which is which.

## Decision

**Notes travel as per-voice signals (option 1), and a polyphonic bundle is an inferred cable width whose voices run as module copies (option a).** There is no event cable kind.

- A cable's width is a voice count from 1 to 16, found when the graph compiles from the module that makes the bundle, never stored in a patch. Stereo stays as it is: a voice can be stereo, so a cable's channel count is voices times sides.
- A module fed a bundle on a `PerChannel` input runs as one copy per voice. A `Downmix` input reads one value from the voices by its signal type. A mono cable into a polyphonic module feeds every voice. A new voice copy starts from `reset`, not from another voice's state.
- A note is a gate that rises on its sample, with a pitch, a velocity, and expression held on sibling cables of the same width. Every module that makes or changes notes allocates voices with one shared algorithm in `loom-dsp`, so the cost of option 1 is paid once.
- The voice count lives on the module that allocates (Note input, Voice assigner, and the note sources), not on the patch.

The rules, the voice modules, and three sources followed end to end are in the [specification](../design/polyphony.md).

## Consequences

- **The hard rules hold with no new machinery.** Timing is a gate on its sample, so a note lands the same at any block size. Allocation is a fixed table inside `process`. Same events in the same order give the same voices.
- **Existing modules become polyphonic for free.** A filter, an envelope, or an oscillator fed a bundle runs per voice as it runs per stereo side today. The engine change is in width inference, the copy count, and the voice modules, not in the module library.
- **Cost is linear in voices.** An 8-voice patch costs 8 copies of each module after the allocator, which is what the NFR-PERF-01 reference patch assumes. SIMD across voices (NFR-PERF-03) stays possible as a later engine optimisation that runs copies in lockstep; it is not needed for M2.
- **No second cable kind.** The canvas draws one more width (FR-CANVAS-12), not a new colour of cable. Players learn one model. The snapping, playable keyboard, and retuning that waited on OQ-16 can now be specified on per-voice signals.
- **A changing note count is handled by stealing.** A source with more notes than voices steals by the allocator's rule. An unused voice holds its gate low. A chord sequencer therefore needs a voice count, as a keyboard does.
- **The design spike is not built.** Decision 1 needs no prototype, because the engine already runs copies and places events on their sample. The first M3 note effect is the real test of the model (risk R-12). If note effects turn out to need events, a later ADR supersedes this one and adds an event cable kind beside, not instead of, per-voice signals; nothing built on bundles would change.
- **Module data reserves room for stored notes.** A note cycle stores a clip, and a chord sequencer stores chord symbols. The [module data](../design/module-data.md#room-for-notes-and-chords) format leaves a variant for each.

## Prior art

Per-voice signals are how a modular grid such as Bitwig's Grid carries polyphony: each cable is one value per voice, and ordinary modules run per voice. Named only to explain the idea. The rules and the voice modules are our own, from the requirements and the catalogue.
