# ADR-0011: Everything is a module

- **Status:** Proposed
- **Date:** 2026-10-13
- **Related requirements:** FR-LIB-12 to FR-LIB-23, FR-SIG-*, FR-FILE-*
- **Related decisions:** ADR-0007 (signal model), ADR-0008 (patch format), ADR-0010 (own canvas)

## Context

Bitwig Studio's Grid shows how much freedom comes from patching small modules. But Bitwig keeps many things outside the Grid: [modulators](https://www.bitwig.com/userguide/latest/modulator/), note effects and other [devices](https://www.bitwig.com/userguide/latest/device_descriptions/), the clip launcher with its next actions, and the arranger timeline. Each lives in its own part of the program, with its own rules, and they cannot drive each other freely.

LoomDSP is not a DAW. It is a flexible interface for building **living musical systems**: patches that play, evolve, and arrange themselves through modulation, randomness, texture, sequencing, looping, and logic, with common and novel audio and note effects.

## Decision (proposed)

Every feature of LoomDSP is a module in the graph. There are no separate subsystems for modulation, note processing, sequencing, looping, or arrangement.

- **Modulators** (LFOs, envelopes, drawn curves, random sources, macros, step and parameter sequences) are modules.
- **Note and MIDI effects** (arpeggiator, strum, echo, harmonise, randomise, quantise) are modules.
- **Sequencers** are modules, including polymetric multi-output step sequencers.
- **Device-style processors** (EQ, dynamics, reverb, delay, sampler, granular) are modules.
- **Loopers** (circular audio and MIDI) are modules, like clips in Bitwig's clip launcher.
- **Arrangement is modules too.** There is no arranger timeline. Sections change through:
  - **groups and next actions:** a group of loopers plays round robin, at random, or in sequence, and reports when a loop or a round finishes. ADR-0012 builds groups from phase-driven primitives (cycles, Clock Divide, Counter, Step Select) and ships them as open abstractions;
  - **counters and logic:** count loops or rounds, compare, and trigger other groups or scenes;
  - **scenes:** recall a stored state of chosen modules, with optional morphing;
  - **modulation curves** that run over bars or sections;
  - **clock and transport** modules.
- **Panels and larger editors** (sequencer grids, curve editors, looper waveforms, front panels) are views of a module's state. Each module carries its own panel on its tile (ADR-0013); larger editors open from it.

### Example: arrangement by logic

ADR-0012 decides how arrangement modules work. The modules and a fuller example are in the [arrangement specification](../design/arrangement.md).

Four loopers form group A, playing round robin. Group A outputs a trigger each time it completes a round. A counter counts those triggers. When the count reaches 4, the counter fires a trigger into group B's "start" input and group A's "stop" input, and resets.

In LoomDSP this is ordinary patching: the triggers are gate signals, and the counter is a small module. A fixed clip launcher or a timeline cannot express this kind of rule.

## Why

1. **One mental model.** Users learn one way to connect things. A sequencer can modulate a filter, an LFO can drive a sequencer's position, a counter can change the section.
2. **New kinds of arrangement.** Arrangement becomes something you patch, so it can respond to logic, randomness, and performance input.
3. **No special cases in the engine.** Scheduling, real-time safety, saving, undo, and export work the same for every feature.
4. **Exports stay complete.** A plugin or standalone export carries everything, including its loopers, sequencers, and arrangement logic, because they are part of the patch.
5. **The ecosystem grows in one place.** Module developers add any kind of feature through the same SDK.

## Consequences and open questions

- **Triggers already fit the signal model.** "Loop finished", "round finished", and "start" are gate signals (ADR-0007), so counters and logic need no new cable kind.
- **Notes on cables (OQ-16).** Note effects need notes to travel between modules. Options: Grid-style per-voice signals, a new event cable kind with sample-accurate timestamps, or both with converter modules. Decide with a design spike before P3.
- **Module data beyond parameters (OQ-17).** Patterns, loops, wavetables, and samples are module state, not parameters. The patch format needs a per-module data section (ADR-0008), with undo, migration, and real-time-safe hand-over to the engine.
- **Looper recording (OQ-15).** Loopers record into pre-allocated memory on the audio thread. Saving loops to disk happens on a control thread.
- **Clock and transport.** In plugins, the host transport must drive the clock modules.
- **Library size.** "Everything" is a very large library. It grows by phase, and through the module SDK and community contributions (FR-SDK-*).
- **Canvas.** Many of these modules need rich views inside the canvas (ADR-0010).
