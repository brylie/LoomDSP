# ADR-0012: Arrangement from phase-driven primitives

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-15
- **Related requirements:** FR-LIB-20 to FR-LIB-22, FR-LIB-24
- **Related decisions:** ADR-0002 (feedback), ADR-0007 (signals), ADR-0011 (everything is a module)
- **Specification:** [Arrangement](../design/arrangement.md)

## Context

LoomDSP exists partly to make arrangements that a clip launcher cannot express. For example: play the loops of group A round robin, each loop twice; after two rounds, play group B the same way; then return to A.

Bitwig Studio's clip launcher has next actions, but its logic is fixed, it cannot count rounds or chain groups by rules, and it lives outside the Grid, so modulation and logic cannot drive it.

ADR-0011 decided that arrangement is made of modules. This ADR decides what kind of modules.

Bitwig's own Grid shows a different path: its Ø Counter turns triggers into phase values, Clock Divide fires every N pulses, and its sequencers are read by a phase signal ([Grid modules](https://www.bitwig.com/userguide/latest/grid_modules/)). When a counter outputs a phase, a step sequencer can follow it, so "rotate through loopers" becomes a step sequencer whose steps are loopers.

We also want users to work close to their musical ideas. Pure primitives (as in Pure Data) are powerful but slow; fixed devices are quick but closed.

## Options considered

1. **A fixed clip launcher with next actions.** Familiar and quick. Rejected: its logic is closed, which is the limit this project exists to remove.
2. **A "Group" module with built-in next-action modes** (round robin, random, sequential). Quick to use, but every new rule needs a new mode, and the logic cannot be patched or changed.
3. **Phase-driven primitives, with groups and parts shipped as open abstractions made from them.** One mental model at every level, the same signal rules as everything else, no special cases in the engine.

## Decision

Option 3.

- Arrangement is built from small phase-driven modules: loop players (cycles) that read the clock's bar phase and send an end trigger, plus counting and selecting logic (a counter that outputs a phase, a divider, a step selector).
- The same primitives work at every level: loops within a part, parts within a song.
- Groups, parts, and song structure ship as **built-in abstractions**: patches made from those primitives, usable as one module and open to inspect and change. They use the same format as user abstractions.

The modules, their ports, and how they compose are specified in [docs/design/arrangement.md](../design/arrangement.md), which changes as they are built.

## Consequences

- Arrangement needs no engine subsystem. Scheduling, saving, undo, and sharing work as for every other module.
- Cycles depend on module data (FR-LIB-19, OQ-17) and are only practical with sub-patches (FR-ENCAP-*). Both come in P2, so cycles land in P3.
- The logic modules need neither, so they can be built in P1, and they are good first issues.
- Triggers need a convention (OQ-19), because counters and dividers count them.
- Arrangement logic forms feedback loops in the graph (end → counter → select → play). The compiler handles them (ADR-0002), but their per-sample cost must be measured.
- The P3 exit criterion becomes the example in the context above.
