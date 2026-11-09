# Unison: copies, spread, and why a stack sums at 1/√N

A sub-patch can run its inside more than once. Run it as several copies and mix them, and one oscillator becomes a thick, wide unison sound. Run several different insides and pick one, and a tile becomes a switch between sounds. This explainer shows what happens to loudness when copies are mixed, why the copies must differ, how they spread across the stereo field, and how a select changes member without a click.

- **Specification:** [sub-patches](../design/sub-patches.md), the section "Modes".
- **Decision:** [ADR-0040](../adr/0040-stack-copies-and-mode-modules-are-implied.md), with [ADR-0031](../adr/0031-sub-patches-stored-as-definitions-and-flattened.md) and [ADR-0039](../adr/0039-editor-keeps-the-hierarchy-as-an-overlay.md).
- **Code:** `crates/loom-dsp/src/blend.rs` (the gain laws and the member fade), `crates/loom-modules/src/structure/modes.rs` (the hidden Sum, Stereo spread sum, Member, and Select), `crates/loom-patch/src/flatten.rs` (copies and members flattened), `crates/loom-patch/src/hierarchy/modes.rs` (the editor's view of them).

## Four modes

| Mode | What runs | What comes out |
|---|---|---|
| Single | The inside, once. | Its outputs. |
| Layer | Every member. | The sum of the members. |
| Select | Every member. | The chosen member, faded over 10 ms when the choice changes. |
| Stack | N copies of the inside. | The sum of the copies at 1/√N, mono or spread across stereo. |

In a Eurorack system a stack is like patching four identical oscillators into a mixer, and a select is like a sequential switch. Here neither needs any modules or cables: the flattener adds the mixer and the switch, hidden, when it turns the sub-patch into plain modules ([Flattening](flattening.md)).

## Two kinds of sum

Add two signals and the result depends on how alike they are.

- **Identical signals add in amplitude.** Two copies of the same sine, in phase, give a sine twice as high: +6 dB. Four give four times as high: +12 dB.
- **Unrelated signals add in power.** Two noise sources with different seeds give a signal whose *power* is twice one source's, so its level is √2 times as high: +3 dB. Four give +6 dB.

The second rule is a fact of statistics: the variance of a sum of independent variables is the sum of their variances (A. Papoulis and S. U. Pillai, *Probability, Random Variables and Stochastic Processes*, 4th ed., McGraw-Hill, 2002). The power of an audio signal is its variance around zero.

A stack's copies are meant to differ: each is detuned a little, or plays its own random numbers. So they add in power, and the stack scales the sum by **1/√N** to keep the loudness of one copy:

| Copies | Sum of N unrelated copies | Times 1/√N |
|---|---|---|
| 1 | 0 dB | 0 dB |
| 2 | +3 dB | 0 dB |
| 4 | +6 dB | 0 dB |
| 16 | +12 dB | 0 dB |

A layer is different: its members are different sounds that the player chose to hear together, so a layer adds them at full level, as a mixer with every fader at 1 does.

## Why the copies must differ

Four copies with nothing to tell them apart start from reset together and play the same samples. They add in amplitude: 4 × 1/√4 = 2, so the stack is **6 dB louder** than one copy and sounds the same. That is not unison, only a louder sound.

Two things make copies differ:

- **Spread** gives each copy its own value of a parameter. With a spread on detune, copy 0 sits a little flat, the last copy a little sharp, and the rest evenly between. The copies drift in and out of phase, and the sum beats and shimmers. That is the unison sound.
- **Seeds.** A module with a seed, such as noise, gets the seed plus the copy's number in each copy, so the copies play different random numbers, for the same reason the right side of a stereo copy does ([stereo cables](stereo-cables.md), [repeatable randomness](repeatable-randomness.md)).

The offsets run evenly from -1 to +1: with four copies, -1, -1/3, +1/3, +1. With an odd count the middle copy has offset 0 and keeps the plain value.

## Why spread is a share of the knob

A spread amount of 0.1 means "a tenth of the knob", as a modulation depth does ([modulation without cables](modulation-without-cables.md)). On a cutoff, whose knob follows a logarithmic curve, that is a musical interval, the same at 200 Hz as at 2 kHz. On a whole-number parameter it rounds the way modulation does.

The flattener builds spread out of modulation: one hidden constant source of 1.0, and on each copy's parameter an assignment of depth amount × offset. That choice is not only convenient. A MIDI controller sets a parameter's base value on the audio thread, in every copy. If the offsets were written into each copy's value, the first control change would wipe them. As modulation, the offsets sit on top of whatever the base value is, so a knob, MIDI, and outside modulation all move the copies together and keep them apart.

## Spreading across stereo

With stereo spread on, copy k is also panned by its offset: -1 hard left, +1 hard right. The pan uses the **constant-power law**: with θ = (offset + 1)·π/4, the left gain is cos θ and the right gain is sin θ.

| Offset | Left | Right | Power (left² + right²) |
|---|---|---|---|
| -1 | 1.000 | 0.000 | 1 |
| -1/3 | 0.866 | 0.500 | 1 |
| 0 | 0.707 | 0.707 | 1 |
| +1/3 | 0.500 | 0.866 | 1 |
| +1 | 0.000 | 1.000 | 1 |

The power is 1 wherever a copy sits, so nothing gets louder in the middle. In the centre each side gets 0.707, which is -3 dB. A linear pan (left 1 - p, right p) would put 0.5 on each side in the middle, -6 dB, and an unrelated copy panned there would sound quieter than one at the edge (C. Roads, *The Computer Music Tutorial*, MIT Press, 1996, on panning; U. Zölzer (ed.), *DAFX: Digital Audio Effects*, 2nd ed., Wiley, 2011, on constant-power laws). Each side is then scaled by 1/√N, as for mono.

One copy with stereo spread sits in the centre, so it is 3 dB quieter on each side than the same copy in mono, and just as loud in total.

## How a select changes member without a click

A select switches between members while they all keep running. Switching in one sample would put a step in the output, a click. So the Select fades over 10 ms with equal-power gains, as a slot swap does ([abstraction](abstraction.md)): `out = from·cos θ + to·sin θ`, θ from 0 to π/2 over 480 samples at 48 kHz.

A swap is one gesture, but Member is a parameter that can be modulated at audio rate. An LFO that hovers between member 1 and member 2 can change its mind every few samples. Two rules keep that click-free:

1. **Turning back reverses the fade from where it is.** If Member goes back to the member that is fading out, the two swap roles and the count goes from n to N - n. The gains on that frame do not change, because cos((π/2)(N - n)/N) = sin((π/2)n/N). The fade simply runs the other way.
2. **A third member waits.** If Member goes somewhere else during a fade, the fade finishes first, then a new one starts toward wherever Member is then. Only the latest choice counts, so a fast sweep ends where the control stopped, never more than 10 ms late.

Gate, trigger, and phase outputs are not faded: a half-faded gate fires late, and a half-faded phase looks like a wrap ([hot patching](hot-patching.md)). They switch on the frame.

## What every member running costs

Every member of a select runs, even the ones you do not hear, so a select of 8 costs about what a layer of 8 does. *Why not pause the others:* a paused member would restart cold when chosen, and deciding per block what to run would make the output depend on the block size ([block-size invariance](block-size-invariance.md)). The benchmark is in [sub-patch modes](../benchmarks/2026-11-09-sub-patch-modes.md).
