# Repeatable randomness

Why a random part of a patch plays the same each time it loads, and the choices that keep it that way. For musicians who patch with chance, and developers who write random modules.

- **Kind:** explainer.
- **Used by:** [M2 modules specification](../design/p2-modules.md) (Random, Chance), [stereo cables](../design/stereo.md) (rules 15 and 27).
- **Related:** [Block-size invariance](block-size-invariance.md), [Triggers and gates](triggers-and-gates.md).
- **Code:** `Rng` in `crates/loom-modules/src/rand.rs`.

## Random, but the same each time

A computer's random numbers come from a formula. Each number is made from the one before, starting from a **seed**. The same seed always gives the same list:

| Seed | First three draws, every time |
|---|---|
| 1 | 0.122, 0.670, 0.554 |
| 2 | 0.727, 0.833, 0.529 |

So "random" here means *unrelated to each other*, not *different every time*. This matters because a patch is a piece of music. If a hi-hat pattern you like came from chance, it should still be there when you open the patch tomorrow. To hear something new, change the seed.

## A wrong turn: small seeds

The obvious way is to use the seed as the first state of the formula. It fails for the small seeds people try first. An xorshift generator moves only a few bits in its first step, so its first values from small seeds are tiny:

| Seed, with no mixing | First three draws |
|---|---|
| 1 | 0.0001, 0.0157, 0.6164 |
| 2 | 0.0001, 0.0313, 0.1625 |

A patch would open with its first random values near 0, and seed 2 would start like seed 1.

So the seed goes through a mixing step first, which spreads similar numbers far apart. That gives the first table above.

## A wrong turn: one list for everything

Suppose all four lanes of Chance shared one list of numbers. Lane 1 takes the first number, lane 2 the next, in the order the triggers arrive. Now add a cable to lane 2. It takes more numbers, so every other lane gets different ones, and a pattern you liked on lane 3 is gone. You touched lane 2.

So each lane has its **own stream**, made from the seed and the lane's number. A lane's choices depend only on its own triggers. Random does the same with its two outputs.

There is a smaller version of the same trap. If Centred takes two draws and Uniform takes one, then changing the shape moves every later number. So Random always takes two, and uses what it needs.

## Why a walk bounces

A walk moves a little from where it was. What happens at the edge of the range?

| Rule | What you hear |
|---|---|
| Stop at the edge | The value sticks there for a while, so the ends of the range are played far too often. |
| Jump to the other side | A large step, which a walk is meant to avoid. |
| **Bounce** | From 0.05 with a move of -0.2, go to 0.15. Values spread over the whole range. |

The walk also keeps its place as a position from 0 to 1, not as a value. So turning `low` or `high` rescales it at once, and it can never end up outside the range.

## Below 1 means below 1

Chance sends a trigger to A when the draw is below `chance`, so a chance of 1 must always pick A. A draw must therefore never equal 1. Dividing a 32-bit number by the largest 32-bit number can round up to exactly 1 in an `f32`, which would make "always" fail about once in 30 million triggers. That is rare enough to pass every test, and common enough to happen on stage. So a draw uses only the top 24 bits, which an `f32` holds exactly.

## Stereo

If a module runs as two copies, one per side, with one list, both sides play the same numbers: mono in disguise. So a copy mixes its channel number into the seed. See [stereo cables](stereo-cables.md). Selector does not: its numbers pick which input to play, and both sides of a stereo signal must pick the same one.

## How other tools handle it

A hardware random source often reads electrical noise, and so has no seed. That is lively, but a patch can never repeat. LoomDSP always keeps a seed, because a saved patch should sound as it did when it was saved.
