# Abstraction: roles, slots, and swapping a part while it plays

A patch that works as one instrument is usually a chain: something makes a sound, something shapes it, something gives it a level over time. Most of what makes two synthesisers different is which module sits in each place. This explainer shows how LoomDSP lets you change one place in the chain without touching the rest, why that needs a contract between modules, and how the change can happen on stage without a click.

- **Specification:** [sub-patches](../design/sub-patches.md), the sections "Roles", "Slots", and "How a swap crossfades".
- **Decision:** [ADR-0017](../adr/0017-primitives-traits-and-abstractions.md).
- **Code:** `crates/loom-core/src/role.rs` (roles and declarations), `crates/loom-patch/src/slot.rs` and `hierarchy/slots.rs` (the slot in the file and in the editor), `crates/loom-modules/src/structure.rs` (the swap fade).

## Start from a real chain

Noise into a filter into the output. The filter is a state variable filter (SVF) with its cutoff at 300 Hz. You want to hear the same patch with a ladder filter instead.

Without slots, that is five steps: add a ladder, copy the cutoff over by hand, move the cable into the SVF to the ladder, move the cable out of the SVF's lowpass to the ladder's out, delete the SVF. If a knob on your front panel was bound to the SVF's cutoff, it is now bound to nothing. If a MIDI controller was learned on it, the mapping is gone. And the moment you move the cables, the sound jumps.

With a slot, it is one gesture: right-click the slot, **Swap to Ladder**. The cables stay, the cutoff goes with them, the knob keeps working, and the change takes 10 ms.

## What a role is

A swap can only keep the cables if the new module has somewhere for them to go. A role writes that down. It is a short contract: these inputs, these outputs, these core parameters, each with a kind.

| Filter role | Name | Kind | Required? |
|---|---|---|---|
| Input | `in` | audio | yes |
| Input | `cutoff` | pitch | no |
| Output | `out` | audio | yes |
| Parameter | `cutoff` | Hz | yes |
| Parameter | `resonance` | 0 to 1 | no |
| Parameter | `drive` | 0 to 1 | no |

A module **declares** that it fills a role, and says which of its own names stand for the role's. The SVF has four outputs and none is called `out`, so its declaration says the role's `out` is its `lowpass`. The one-pole filter has no resonance, and that is fine, because resonance is optional. The registry checks every declaration when a module is registered: a module that claims to be a filter but has no `cutoff` parameter does not load, and the error names the parameter. That check is why a slot can trust any part it is offered.

### Why kinds, and why optional

Two things would break a swap silently without them.

The first is units. If one filter's cutoff were in Hz and another's in pitch, the value 300 would carry across as 300 and mean something quite different. The role fixes the unit, so a carried value keeps its meaning. The minimum and maximum may still differ between parts; the value is clamped, which is what a knob turned past its end does too.

The second is a parameter that only some parts have. The catalogue's first draft listed `level` on every sound source, but no LoomDSP oscillator has one: an oscillator's level is set by the amplifier after it. Making every listed parameter required would have left the role with no part that fits. Making a parameter optional says: carry it when both parts have it, show it when the part has it, and never refuse a part for lacking it.

### Why a sound source takes a pitch and a phase

Every LoomDSP oscillator is driven by a **phase**, a ramp from 0 to 1 ([Phase and cycles](phase-and-cycles.md)). But it makes that phase itself from its `pitch` input, so a note cabled into an oscillator plays, as on any synthesizer. A cable into its `phase` input takes over, so one phase can drive several shapes in lock, or a bent phase can make a new wave ([oscillators](../design/oscillators.md)). The role has both inputs, and both are optional, because noise has neither. Its `tune` and `octave` knobs are optional role parameters, so a Saw swapped for a Pulse keeps its tuning.

## What a slot is, and what it is not

A slot is a tile with the role's ports. Behind it sits an ordinary module, the **part**, which the editor hides. Every cable, every modulation assignment, every MIDI mapping, and every exposed control on the slot is routed to the part by the role's names when the patch is flattened ([Flattening](flattening.md)). The engine never sees a slot. It sees a filter with cables on it, exactly as if you had wired the filter yourself.

This matters for what a slot costs: nothing. Encapsulation has no run-time cost (FR-ENCAP-07), and a slot is only a way of saying which module is wired in and how.

It also matters for what a slot remembers. When you swap the ladder out for an SVF, the slot keeps the ladder's own values under the ladder's name. Its slope is a parameter the Filter role does not have, so it is kept and comes back as it was. Its drive is a role parameter, but an optional one that the SVF lacks, so there is nothing to carry it to or from, and it comes back as it was too. Cutoff and resonance are different: both parts have them, so they carry across on every swap, and the ladder comes back with the cutoff and resonance the SVF had when it left. A sub-patch that leaves a slot is kept whole, inner edits and all. Nothing is ever converted from one part to another: a ladder's drive has no meaning on an SVF, so it waits for the ladder to come back.

## Why a swap must crossfade

Replacing one filter with another in a running patch is two cable changes at once. Even when both filters are fed the same input, their outputs differ at the frame of the change, and the output jumps by that difference. A jump of a few hundredths is a click ([Hot patching](hot-patching.md) measures where a step becomes audible).

So for 10 ms the old part and the new part both run. A small fade module between them outputs `a · cos θ + b · sin θ`, where `a` is the leaving part, `b` the new one, and `θ` goes from 0 to a quarter turn over the fade. At the first frame the output is `a` alone. At the last it is `b` exactly. In between, the two gains sit on a quarter circle, so the sum of their squares is 1 at every frame.

### Why equal power, not a straight line

A straight line, `a · (1 − t) + b · t`, feels like the obvious fade. It is right when `a` and `b` are the same signal, because then the output is just `a`. Two different filters do not give the same signal. Their outputs are not correlated sample by sample, and uncorrelated signals add in **power**, not in amplitude. Halfway through a linear fade each has a gain of 0.5, which is a quarter of the power each, half the power in total: a dip of 3 dB in the middle of the swap. On the quarter circle each gain is 0.707 at the middle, half the power each, and the total holds at 1. The ear hears a steady level while the colour changes.

Sample values at 48 kHz, with `a` a constant 1 and `b` a constant −0.5, so the law is easy to read off (`crates/loom-modules/src/structure/tests.rs`):

| Frame | θ | Output |
|---|---|---|
| 0 | 0 | 1.000 |
| 120 | π/8 | 0.733 |
| 240 | π/4 | 0.354 |
| 360 | 3π/8 | −0.079 |
| 480 | π/2 | −0.500 |

C. Roads, *The Computer Music Tutorial* (MIT Press, 1996) sets out crossfading and the equal-power law; U. Zölzer (ed.), *DAFX: Digital Audio Effects*, 2nd ed. (Wiley, 2011) covers constant-power laws for panning, which is the same geometry.

### Why 10 ms

Shorter than a few milliseconds, a gain change is a click. Longer than a few tens of milliseconds, it is heard as a fade, and a swap on stage should feel like a switch, not a sweep. The cable fades of [hot patching](hot-patching.md) use 5 ms, because they only soften a step on one input; a swap changes a whole module's output, so it gets twice that.

### Why the fade is a module and not an engine feature

The engine already knows how to add a module at a buffer boundary, run two modules at once, and remove a module later ([Changing a running patch](../design/live-patching.md)). The swap uses exactly that: add the new part and the fade, wire them, commit; when the fade reports it is done, remove the leaving part and the fade, commit. Nothing on the audio thread allocates, because no new mechanism runs there. The fade module counts samples, so it is the same at every block size, and it publishes a display value when it is done, which the app reads on its next tick. That is also why a test can step the engine by hand and see the fade finish on the frame the arithmetic says it should.

The removal is silent for a plain reason: once the fade is done, its output is `b` and nothing else. Taking it out and wiring the new part straight to the output changes no sample. A test renders the patch both ways and compares them bit for bit.

## How other systems do it

Bitwig Studio's hybrid devices, such as Polymer, let the player swap one section of a device for another Grid module, and were the prompt for this design. VCV Rack has no slots: a module is a module, and changing one means re-patching by hand, which is also what Eurorack asks of you. Max abstractions and Reaktor macros are reusable boxes, but the contract between a box and what it can be replaced with is left to the person doing the replacing. LoomDSP writes the contract down, so the computer can check it and the swap can be one gesture.

## Swapping control signals: a straight line

The quarter circle is right when the two parts make different sounds. A Modulator or Envelope slot's output is not a sound but a value, such as a filter's cutoff offset. If both parts sit at 0.5 when the swap starts, equal power lifts the output to 0.707 in the middle of the fade, and a pitch output would go sharp for a few milliseconds. So a pitch or modulation output fades in a straight line, `a · (1 − p) + b · p`, whose gains sum to 1: a value both parts share stays put. The swap reads the role output's signal type and sets the fade's law ([sub-patches](../design/sub-patches.md#how-a-swap-crossfades-settled-2026-11-08), point 6).

## Empty slots and the built-ins

A slot can be empty. The module browser lists one empty slot per role a part fills, such as `Filter slot`. An empty slot behaves like a pulled cable: whatever it feeds reads its own unconnected value, its normal ([Normals](normals.md)). An empty Filter or Effect slot is a thru: it passes its input on, as an empty insert on a mixing desk does, so taking an effect out of a chain leaves the chain playing.

A swap into or out of an empty thru slot crossfades between the dry signal and the signal through the part, in a straight line, because those two are nearly the same signal and equal power would swell. A swap into or out of any other empty slot is a plain cable change, which will ramp once click-free cable changes are built ([#21](https://github.com/brylie/LoomDSP/issues/21)).

Two abstractions come with LoomDSP. The **Synth voice** holds two oscillators, a filter, and two envelopes, each in a slot. Its tile's menu offers `Swap Filter to Ladder`, because its cutoff and resonance controls are bound to the Filter slot. The **Effect chain** holds three effect slots, each followed by its own mix. That mix is a straight line too: a dry signal and its EQ'd or compressed version are strongly correlated, and the straight blend is the usual dry and wet practice. Insert either one and open it to see how it is built.

## What is still open

- A front panel's sub-panel for the parameters only the current part has waits for the panel editor.
- Undo of a swap fades too, by swapping back. The part that comes back is a new module at its stored values, so its running state, such as a filter's memory, starts from reset, as undo of a delete does.
