# ADR-0046: Empty-slot swaps fade at every copy, from each copy's own thru ends

- **Status:** Accepted
- **Date:** 2026-11-15
- **Related requirements:** FR-ENCAP-08, FR-ENCAP-10, FR-GRAPH-05
- **Specification:** [Sub-patches, Empty-slot swaps inside a stack](../design/sub-patches.md#empty-slot-swaps-inside-a-stack-agreed-on-the-plan-for-762-2026-11-15), [Empty slots](../design/sub-patches.md#empty-slots-agreed-2026-11-14)
- **Refines:** [ADR-0044](0044-swap-fades-are-transient-at-every-copy.md). Its decision stands. Its consequence "A swap into or out of an empty slot inside a stack does not use the swap fade. In every copy it is a cable change, which live-patching rule 9 ramps over 5 ms" no longer holds: this ADR is the follow-up it names ([#768](https://github.com/brylie/LoomDSP/issues/768)). ADR-0044 stays Accepted, because a status line holds one value and the rest of it still holds.

## Context

Outside a stack, a swap into or out of an empty Filter or Effect slot crossfades over 10 ms between the thru signal and the part, with the linear law (ADR-0043, sub-patches spec). Inside a stack it was a cable change in every copy, ramped over 5 ms by live-patching rule 9. So the same swap sounded different inside and outside a stack.

ADR-0044 already adds a fade in every copy for a swap between two parts. A swap between two parts needs no new wiring data: the new part's consumers move onto the fade, and the flattener already wires each copy's consumers. A swap out to the thru is different. The slot is empty after it, so the flattener wires the thru source straight to what the slot fed, and the fade must take over only those inputs, not every input the thru source feeds. Copy 0 finds them from the slot's links at each sync. In another copy, a link names copy 0's ends, and a path moves an end into copy *k* only when the end is inside the stack. A consumer outside the stack, such as the stack's Sum, reads each copy on its own input, which no path names.

## Options considered

1. **Keep the cable change inside a stack.** No new code. Rejected: the swap sounds different inside a stack, and a 5 ms ramp from a held value is not the crossfade the thru role promises.
2. **Find each copy's thru ends by path at every sync, as for copy 0.** No new state. Rejected: it gets a consumer outside the stack wrong, and wires every copy's fade into copy 0's input on the Sum.
3. **Have the flattener mark the cables it makes for each empty thru slot, per copy.** No recorded state, and it follows the player's edits by itself. Not chosen: it changes the flattener and its output for the sake of a 10 ms moment in one session, which ADR-0044 kept out of the flattener.
4. **Record each copy's thru ends in the slot's fade record at the swap,** read from the flat graph before the sync: what feeds the leaving part's `in` in copy *k*, and the inputs its output feeds. At each sync during the fade, the fade's `out` replaces the thru cable into each of those inputs, only while that cable is still there. (Chosen.)

## Decision

Option 4. The transient fades of ADR-0044 cover swaps into and out of an empty thru slot in every copy. Into the slot, each copy's new part is an implied module as before, and the wiring is the same as for a swap between two parts. Out of the slot, the fade record holds each copy's thru ends, read from the graph before the sync, and the replacement applies only to a cable that is still there.

## Consequences

- A swap into or out of an empty Filter or Effect slot sounds the same inside and outside a stack, and so do undo and redo.
- The fade record holds per-copy ends, which are transient and never saved, as the rest of the record is.
- The record can go out of date if the player edits cables during the 10 ms. Replacing only a cable that is still there means the edit wins, and the record never puts back a cable the player removed.
- Copy 0's fade must not take over the other copies of the slot's own stacks. It still takes the copies of a stack downstream, which do not hold the slot.
- Whether a swap fades with the thru signal no longer depends on whether the slot is inside a stack.
