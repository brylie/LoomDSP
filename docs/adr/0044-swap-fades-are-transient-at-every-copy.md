# ADR-0044: Swap fades are transient at every copy

- **Status:** Accepted
- **Date:** 2026-11-14
- **Related requirements:** FR-ENCAP-08, FR-ENCAP-10, FR-GRAPH-05
- **Specification:** [Sub-patches, How a swap crossfades](../design/sub-patches.md#how-a-swap-crossfades-settled-2026-11-08), [Slots inside a stack](../design/sub-patches.md#how-modes-meet-the-rest-settled-2026-11-09)
- **Refines:** [ADR-0040](0040-stack-copies-and-mode-modules-are-implied.md), invariant 7. See Decision.

## Context

A slot swap is click-free because, for 10 ms, both the leaving part and the new part run, and a small fade module blends them. The swap adds the fade and keeps the leaving part. When the fade reports it is done, the app removes both. These modules are **transient**: they belong to one moment in a session, never to the patch, so the file and undo snapshots leave them out.

Until now a slot could not sit inside a stack of more than one copy. Stack copies 1 to N−1 are **implied** (ADR-0040): `sync` flattens the hierarchy and makes the controller's implied modules the ones the flattener gives. Invariant 7 says no edit path adds, edits, or removes an implied module by hand.

A swap inside a stack needs a fade in every copy. The flattener cannot make those fades, because a fade is not in the file and the flattener reads only the file. The code also calls a module implied by its path: everything under `[…, Copy(k), …]` is implied. So copy *k*'s fade and leaving part sit at implied paths, but the swap has to add them by hand.

## Options considered

1. **Teach the flattener about fades.** The hierarchy writes the fades and leaving parts into the patch it flattens. Rejected. A fade's path sits under the slot, which no definition owns. Writing it as a module would feed an input twice (flattening rule 17). And a fade in the flattener's input makes a moment of a session look like a fact about the patch.
2. **Refuse slots inside a stack.** Rejected by the maintainer: the Synth voice must stack for unison.
3. **Transient at every copy.** The swap adds each copy's fade and keeps each copy's leaving part, and the slot's fade record names them all. `sync` leaves them alone, and finishing removes them. (Chosen.)

## Decision

Option 3.

- **The fade record names every copy's transient modules.** A swap inside a stack adds, for each copy *k*, one fade per faded output at copy *k*'s path. It keeps each copy's leaving part with its id and state. All of these are named by the slot's fade record.
- **The exception to invariant 7.** The modules a slot's fade record names may sit at implied paths and are added by the swap, not the flattener. `sync` does not manage them, and does not manage the assignments onto them. Finishing the fade removes them, as for copy 0. Every other implied module still comes only from the flattener, so invariant 7 holds for everything else.
- **New parts are still implied.** Copy *k*'s new part is an ordinary implied module that the flattener makes from the definition, with its spread assignments.
- **The file and undo snapshots never hold a transient module,** at any copy.

## Consequences

- A swap between two parts inside a stack fades in every copy, so unison of a voice with slots is click-free on a swap.
- `sync` needs one more set, the fading modules, and skips them in both `sync_implied_modules` and `sync_implied_links`.
- Restoring an undo snapshot and finishing a fade must reserve and forget every copy's paths, not only copy 0's.
- A swap into or out of an empty slot inside a stack does not use the swap fade. In every copy it is a cable change, which live-patching rule 9 ramps over 5 ms. A follow-up may design the wiring for each copy's thru, so it gets the 10 ms swap fade ([#768](https://github.com/brylie/LoomDSP/issues/768)).
