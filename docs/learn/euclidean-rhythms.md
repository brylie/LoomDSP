# Euclidean rhythms

How to spread hits evenly over a number of steps with one line of arithmetic, why so many traditional rhythms come out of it, and why a clocked pattern waits for its first clock. For musicians and developers who are new to generative rhythm.

- **Kind:** explainer.
- **Used by:** [M2 modules specification](../design/p2-modules.md#euclidean-rhythm-mt-gate-02).
- **Code:** `loom_dsp::euclid::Pattern`, `loom.gate.euclid`.

## The idea

Take 8 steps and 3 hits, and spread the hits as evenly as you can. You get `x..x..x.`: gaps of 3, 3, and 2. Say it aloud and it is a rhythm heard in much of the world's music. Godfried Toussaint showed in 2005 that this is no accident. Euclid's algorithm for the greatest common divisor, when it is used to share hits among steps, produces a large family of traditional rhythms. Several of them are famous: 3 in 8, 5 in 8, and 5 in 16 among them.

The module is [Euclidean Rhythm](../modules/loom.gate.euclid.md).

## Spreading hits with one line

The published method repeatedly splits groups of hits and rests, as Euclid's algorithm repeatedly splits a remainder. That builds a whole pattern at once, in a list. A module on the audio thread would rather not build lists.

There is a shorter way. Imagine a counter that goes up by `hits` on every step, and wraps around at `steps`. Each wrap means one hit has been earned. Over the whole pattern it wraps exactly `hits` times, and since the counter moves by the same amount each step, the wraps are as evenly spaced as whole steps allow.

A step is a hit when the counter has just wrapped, which is when its value is below `hits`:

```text
step i is a hit when (i × hits) mod steps < hits
```

For 3 in 8, the counter reads 0, 3, 6, 1, 4, 7, 2, 5. Below 3 on steps 0, 3, and 6: `x..x..x.`.

This gives the same patterns as the published method, up to where the pattern starts. Since a rhythm is a cycle, that is a difference of rotation only. The closed form also fixes which rotation counts as 0: step 0 is always a hit. So tests can be exact, and the first hit has an obvious meaning, which is what the `accent` output marks.

## Rotation

Rotation moves the whole pattern later. `x..x..x.` rotated by 2 is `x.x..x..`. The gaps stay the same, in the same order, but they land on different beats, and the feel changes a lot. Two patterns with the same steps and hits and different rotations often interlock.

## Two ways to know the step

A pattern needs to know which step it is on. There are two sources, and they behave differently.

A **phase** gives the step directly: `floor(phase × steps)`. A phase always says where you are, so there is no memory to get wrong. Jump the transport, and the pattern is at the right step on the next frame. See [phase and cycles](phase-and-cycles.md).

A **clock** only says "move on". The module has to remember where it is, so it has to decide where it starts.

## Why a clocked pattern waits

The obvious rule is that the module starts on step 0 and each clock moves one step. It fails in a way that is easy to miss.

A new engine's transport starts running, so a clock can land on the very first frame. That is the one case the obvious rule can be made to work for. Now look at three others:

- A **reset arrives ahead of a clock.** A bar clock on `reset` and a beat clock on `clock` that drift by a frame are enough.
- A **module is added during playback.** It starts on step 0 between two clocks.
- A **clock is patched in later.** The same.

In each case the obvious rule fires step 0 at once, at the wrong moment, and the next clock moves to step 1. The pattern now runs one clock ahead of everything else. It is steady and repeatable, so it sounds like a wrong pattern, not like a fault.

So in Clock mode the module starts *before* step 0, and a reset goes back there. While it waits, all outputs are low. The next clock enters step 0, whenever it comes. "A clock on the first frame" and "a reset and a clock together" stop being special cases: both are simply the next clock arriving during the wait.

Many hardware sequencers work this way, and the cost is the same as theirs. A reset alone no longer fires at once, and a pattern is silent until its first clock.

Phase mode has no wait, because a phase always knows the step. A pattern that starts on a hit fires on the first frame.

## Why knobs wait for the next step

The pattern for a step is taken when the step is entered. So turning `hits` while a long gate plays cannot cut it in half. The change is heard at the next step, which on a stage is what you want: a knob should change what comes next, not what is sounding.

## Further reading

- G. Toussaint, "The Euclidean Algorithm Generates Traditional Musical Rhythms", BRIDGES 2005, pages 47 to 56.
- [Triggers and gates](triggers-and-gates.md), for why a trigger is one sample and a gate lasts.
