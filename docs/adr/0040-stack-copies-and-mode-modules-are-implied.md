# ADR-0040: Stack copies and the modules a mode adds are implied by the flattener

- **Status:** Accepted
- **Date:** 2026-11-09
- **Related requirements:** FR-ENCAP-07, FR-ENCAP-10, FR-GRAPH-05
- **Related decisions:** ADR-0031 (sub-patches stored as definitions and flattened), ADR-0039 (the editor keeps the hierarchy as an overlay), extended here; ADR-0020 (stereo), ADR-0022 (copies take state)
- **Specification:** [Sub-patches](../design/sub-patches.md), section "Modes"
- **Background:** [Unison: copies, spread, and why a stack sums at 1/√N](../learn/unison.md), [Flattening](../learn/flattening.md)
- **Issue:** [#51](https://github.com/brylie/LoomDSP/issues/51)

## Context

ADR-0039 made the flat graph the only home of parameters, cables, assignments, and MIDI mappings, and kept the sub-patch hierarchy as an overlay on it. Every tile is one flat module, and every edit path in the app works on flat ids.

Slice 6 of [#51](https://github.com/brylie/LoomDSP/issues/51) adds the modes of FR-ENCAP-10. Two of them break the "one tile, one flat module" picture:

- **Stack** runs N copies of one inside. One inner module on the canvas is N flat modules, and an edit inside must reach all of them.
- **Layer, select, and stack** need modules nobody placed: a sum for each output, a selector, the Member parameter's home, and the sources that give each copy its spread.

These must keep the overlay's invariants: one id space, one writer of the file, the flattener as the one definition of what the hierarchy means, and one undo step per edit.

## Options considered

1. **Every copy is a tile.** The editor shows all N copies and the player edits each. Rejected: the point of a stack is that the copies are one design, and editing N times is what FR-ENCAP-10 removes.
2. **The engine runs a module N times** (as it runs stereo and voice copies, ADR-0020, ADR-0038). Rejected: copies of a whole sub-patch, with its cables and feedback loops, are a graph, not a module, and the spread values differ per copy. Polyphony may later share code with stack mode, but stack mode must not wait for it.
3. **Copy 0 is the editor's; the rest is implied.** The editor shows and edits copy 0. Copies 1 to N-1 and every module a mode adds are computed by the flattener, the same way the flattener already computes boundary cables, and `sync` applies the difference to the controller.

## Decision

**Option 3.** It adds four invariants to ADR-0039's six.

7. **Implied modules come from the flattener.** Stack copies 1 to N-1 and every module a mode adds are implied: `sync` writes the hierarchy out, flattens it on a scratch table, and makes the controller's implied modules, their values, cables, assignments, and MIDI mappings the ones the flattener gave. No edit path adds, edits, or removes one by hand. *Why:* invariant 3 again. The flattener is the one definition of what a stack or a layer is.
8. **The editor sees copy 0 only.** Copy 0 keeps the paths a single sub-patch has. Copy k is `[instance, Copy(k), inner…]`, and an added module is `[instance, Added(n)]`, where `Copy` and `Added` are path steps no module id can take. Tiles, selection, the inspector, probes, lights, meters, the clipboard, the file, and undo snapshots never hold an implied id. *Why:* one tile stays one flat module, so no edit path learns a second handle. Copy 0 never moves, so turning a sub-patch into a stack, or raising Copies, keeps every copy's state.
9. **A parameter write reaches every copy at once.** `App::set_param`, the one place every control writes through, writes the same value to the same parameter of each copy, with no commit, so a knob drag moves every copy during the drag. Everything else reaches the copies through `sync`, before the commit, so one edit is one schedule swap and one undo step. A seeded module is the one exception to "the same value": copy k's seed is copy 0's plus k.
10. **What differs between copies is modulation the flattener adds.** A spread is a hidden constant source and one assignment per copy, with depth `amount × offset(k)`. The base value stays the same in every copy, so it stays the one home of the exposed value (invariant 6), and MIDI and modulation, which act on the base, keep the offsets. *Why not write each copy's value:* a MIDI control change is applied on the audio thread to each copy's parameter and would wipe a written offset.

## Consequences

- **A single sub-patch is unchanged.** A stack of one copy, a layer of one member, and a select of one member play the same as the sub-patch they came from, and a mode change alone never changes the sound.
- **`sync` does more work in a patch with a stack:** it compares modules, values, assignments, and mappings, not only cables. A patch with no stack pays nothing new.
- **Probes and meters inside a stack show copy 0**, not the sum. The outer tile's outputs carry the sum.
- **Undo is unchanged.** Snapshots hold no implied module, and `sync` rebuilds them after a restore.
- **Hidden assignments** from a spread source are left out of the matrix, the inspector, the range arc, and the file, as the swap fades of slice 5 are left out of snapshots.
- **The limit is 16** copies and 16 members, as for voices (polyphony rule 1): the added modules have a fixed number of inputs.
