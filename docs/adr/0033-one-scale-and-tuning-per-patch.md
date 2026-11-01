# ADR-0033: One scale and tuning per patch, read by modules without cables

- **Status:** Accepted
- **Date:** 2026-11-01
- **Related requirements:** FR-LIB-29, FR-SIG-05, FR-LIB-19
- **Related decisions:** ADR-0002 (graph execution), ADR-0007 (signal model), ADR-0015 (one transport per patch), ADR-0017 (traits), ADR-0032 (module data)
- **Specification:** [Scale and tuning](../design/scale-and-tuning.md)

## Context

Pitch modules (quantiser, sequencer pitch lanes, melody generators) should agree on which notes are in key (FR-LIB-29). A key change, such as at the end of a part, should reach all of them at once. LoomDSP does not assume 12-tone equal temperament, so the tuning (how the octave is divided) is part of the same question. OQ-22 asked whether this is a module or a patch setting.

ADR-0015 already answered the same question for time: the engine owns one transport per patch, modules read it without cables, and its controls live on one Transport module.

## Options considered

1. **A patch setting only,** edited in patch info. Simple, but nothing in the patch can change key.
2. **A module only, on cables.** The Scale module outputs the scale, and every pitch module takes a scale input. Visible, but every quantiser needs a cable, and the scale is no longer patch-wide.
3. **Like the transport.** The engine holds one scale and tuning per patch. Pitch modules read it without cables. A Scale and tuning module holds the settings and switches stored scales from a trigger.

## Decision

Option 3.

- The engine holds **one tuning and one active scale per patch**. Scale-aware modules (TR-08) read them through `ProcessContext`, as they read the transport.
- A **Scale and tuning module** holds the tuning and up to eight stored scales, and switches between them from a trigger or a control value. A patch has at most one, at any level. A patch without one uses 12-tone equal temperament, with every note in the scale.
- The **tuning is always the patch's.** A module may use its own scale over the patch tuning (TR-08), never its own tuning.
- The compiler treats the scale as a **hidden cable** from the Scale and tuning module to every module that follows the patch scale, so a change reaches them on the same sample.

## Consequences

- **One key change moves the whole patch,** on the sample the trigger arrives, at any block size.
- **Every pitch module agrees on what a note is,** because there is one tuning. A module with its own scale still names the same notes.
- **The compiler gains implicit edges.** A pitch module that feeds the Scale module's trigger makes a loop, which the compiler breaks with a unit delay and reports, as for any other loop (ADR-0002).
- **The scale and tuning are module data** (ADR-0032): stored with the patch, so imported tunings travel with it, and handed to the engine without allocation.
- **Plugin export** keeps the scale inside the patch; hosts have no say in it.
