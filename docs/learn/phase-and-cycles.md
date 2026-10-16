# Phase and Cycles

What a phase signal is, how LoomDSP finds the end of a cycle, and why the obvious ways to do it fail. For musicians and developers who are new to phase-driven patching.

- **Kind:** explainer.
- **Used by:** [clock and transport specification](../design/clock.md) (Phase Trigger, Clock Divide), [arrangement specification](../design/arrangement.md), [ADR-0007](../adr/0007-polymorphic-signal-model.md) (signal model), [ADR-0012](../adr/0012-arrangement-from-phase-driven-primitives.md) (arrangement from phase).
- **Code:** `loom_core::signal::WrapDetector`, `loom.phase.generator`, `loom.phase.trigger`.

## What a phase is

A phase says *where you are in a cycle*. It is a number that rises from 0.0 to 1.0 and then wraps back to 0.0. At 0.25 you are a quarter of the way through the cycle; at 0.5, halfway.

It helps to picture a circle, like a clock face. The phase is the position of the hand. Going past 1.0 is the same as going past 12 o'clock: you are back at the start. On a scope, a phase that runs forward at a steady speed looks like a sawtooth: a steady rise, then a drop.

Many things in a modular patch are phases:

| Thing | Its phase |
|---|---|
| An oscillator | Where it is in one wave. A sine module turns the phase into a sine shape. |
| The transport | Where it is in the bar (bar phase) or the beat (beat phase). |
| A looper or clip | Its read position: which part of the audio or MIDI to play now. |
| An LFO | Where it is in one slow wave. |

*Why separate timing from shape:* one phase can drive any shape, and you can bend time before it becomes sound. Speed a phase up, slow it down, reverse it, or curve it, and everything it drives follows. A [Fugue Machine](https://alexandernaut.com/fugue-machine-classic/)-style patch is one clip read by several phases at different speeds and directions.

**In Eurorack** a phase is just a ramp or sawtooth voltage. **In Bitwig Grid**, phase is a signal type of its own, with a whole family of modules that change it (the Ø modules: shift, reverse, mirror, bend, and others). LoomDSP follows Grid here: everything is a number on a cable, and phase is one way to read that number.

## A phase on a cable is a list of samples

The engine does not see a smooth line. It sees one number per sample, 48,000 times a second at 48 kHz. A 440 Hz phase moves about 0.009 per sample, so the numbers around a wrap look like this:

| Sample | 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|
| Phase (about) | 0.976 | 0.985 | 0.994 | 0.003 | 0.012 |

The phase passed 0 *between* samples 3 and 4. No sample is exactly 0. This is normal, and it is the root of every problem below.

## Finding the end of a cycle

Many modules need to know when a cycle ends: a clock that fires on each beat, a looper's end trigger, a divider that counts bars. LoomDSP uses one rule for all of them:

> A cycle ends where the phase **jumps by more than half a cycle** (0.5) from one sample to the next, in either direction.

In the table above, sample 3 to sample 4 is a jump of 0.991. That can only be a wrap.

### Why "more than 0.5"

A phase moving normally changes by a small amount per sample. It only moves 0.5 or more per sample if it runs faster than half the sample rate (24 kHz at 48 kHz). At that speed the samples cannot show its direction anyway (this is the Nyquist limit). So any phase we can follow moves less than 0.5 per sample, and a bigger jump must be a wrap.

### Why not "the phase equals 0"?

Because, as the table shows, it almost never does. A rule of "fire when the phase is 0" would miss nearly every cycle. A phase that *stays* at 0 is also common (a stopped transport, an unconnected input), and that rule would then fire on every sample.

### Why not "the phase went down"?

Because phases can run backward. A reversed phase goes 0.5, 0.25, 0.0, then wraps *up* to near 1.0. It goes down all the time, and its wrap is a jump *up*. With "more than 0.5 in either direction", forward and backward phases both work.

### When it fires

The trigger fires on the **first sample of the new cycle** (sample 4 above). That sample is also the end of the old cycle, so "start of cycle" and "end of cycle" are the same moment. Eurorack modules call this output **EOC** (end of cycle); the Make Noise Maths is a well-known example.

### The first sample

When a module starts, there is no earlier sample to compare with. LoomDSP fires on the first sample only if the phase is exactly 0. A phase that starts on a bar line (the transport starts at exactly 0) marks bar 1. A module added halfway through a bar does not fire a trigger as if a bar had just started.

### Every case at a glance

| Before | After | Fires? | Why |
|---|---|---|---|
| 0.994 | 0.003 | Yes | A forward wrap. |
| 0.004 | 0.997 | Yes | A backward wrap. |
| 0.8 | 0.0 | Yes | A reset that jumps more than half a cycle looks like a wrap on the cable. |
| 0.3 | 0.0 | No | A reset, but the cycle did not finish. Use the trigger that caused the reset if you need an event. |
| 0.45 | 0.55 | No | A normal step, or a small wobble. |
| (none) | 0.0 | Yes | The first sample, on a bar line. |
| (none) | 0.37 | No | The first sample, halfway through a cycle. |
| 0.0 | 0.0 | No | A phase standing still. |

## Phases that never wrap

Some phases move smoothly back and forth and never jump:

- **Ping-pong.** The phase goes 0 to 1 to 0 to 1, like Grid's Ø Mirror. A looper set to ping-pong reads its clip this way.
- **Scrubbing.** An LFO, envelope, or knob moves a read position back and forth over part of a clip, like scratching a record.
- **Holding.** The phase stops, or drifts slowly.

A sawtooth phase goes *around* the circle. These go back and forth along *part* of it. They never pass the point where 1 meets 0, so the wrap rule never fires. That is correct: nothing wrapped. But you may still want an event. There are two useful ones.

### Crossing a point: shift, then detect

To fire when the phase passes a position *p*, shift the phase by *-p* and then look for a wrap. The point *p* moves to 0, and passing it becomes a wrap.

Example with *p* = 0.3. A scrub moves from 0.29 to 0.31. After shifting by -0.3 (and wrapping into 0 to 1), it moves from 0.99 to 0.01. That is a jump of 0.98, so it fires. It works in both directions, and for phases that never go all the way around.

So a "crossing detector" is not a new idea. It is a phase shift followed by Phase Trigger.

### Turnarounds: look upstream

A ping-pong phase turns around at the ends. A turnaround is not a jump, so the wrap rule does not see it. Looking for "the direction changed" does not work either: a scrubbing LFO changes direction in the middle of a clip, and the detector cannot tell the two apart.

The answer is to look at what *makes* the ping-pong. A mirror is driven by an ordinary sawtooth phase. Its output turns around at the bottom when the input wraps, and at the top when the input passes 0.5. So:

- Phase Trigger on the mirror's **input** marks the bottom turnaround.
- A shift by 0.5, then Phase Trigger, on the input marks the top one.

In a phase-driven patch there is almost always a plain wrapping phase upstream. Take events from it, not from the shaped output.

## Who should report the end of a cycle

The module that *makes* the motion knows best when its cycle ends. A looper knows when it has played its length; an LFO knows its own cycle. Such modules should have their own end trigger, as the Maths has its EOC output, and they use the same rule inside (`WrapDetector`), so they agree with Phase Trigger.

Phase Trigger is the general tool for every other phase: one you made yourself, bent, scaled, or received from another module.

## Triggers, in short

The event that marks the end of a cycle is a **trigger**. In LoomDSP a trigger output is high for exactly one sample, and trigger inputs count rising edges (a sample above 0.5 after one at or below it). *Why one sample:* two triggers close together never merge, so a counter never misses one. See [ADR-0007](../adr/0007-polymorphic-signal-model.md#triggers).

## In LoomDSP

| Piece | What it does |
|---|---|
| `loom.phase.generator` | Makes a phase at a pitch. |
| `loom.phase.trigger` (Phase Trigger) | A trigger on each wrap of its input phase. |
| `loom.logic.divide` (Clock Divide) | Passes on every Nth trigger. |
| `loom_core::signal::WrapDetector` | The wrap rule, shared by every module that marks the end of a cycle. |
| `loom_core::signal::EdgeDetector` | Finds rising edges on trigger and gate inputs. |
| Phase Scale, fold, reset ([#31](https://github.com/brylie/LoomDSP/issues/31)) | Planned: multiply, mirror, and reset a phase. |

## Further reading

- [Bitwig Grid phase modules](https://polarity.me/topics/grid-phase-modules/), a tour of Grid's Ø modules by Polarity.
- [VCV Rack voltage standards](https://vcvrack.com/manual/VoltageStandards), how VCV Rack treats triggers, gates, and clocks.
