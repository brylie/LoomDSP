# Feedback loops and the unit delay

Why a loop in a patch needs a one-sample delay, why the engine runs loops one sample at a time, and what that costs. For musicians who patch outputs back to inputs, and for developers who work on the compiler.

- **Kind:** explainer.
- **Used by:** [ADR-0002](../adr/0002-graph-execution-and-feedback.md) (graph execution and feedback), [live patching specification](../design/live-patching.md), [modulation specification](../design/modulation.md) (rule 8).
- **Related:** [Block-size invariance](block-size-invariance.md), [Hot patching](hot-patching.md), [Delays and interpolation](delays-and-interpolation.md), [Feedback FM](feedback-fm.md).
- **Code:** `crates/loom-core/src/compile.rs`, `loom.time.delay1`.

## A loop has no first step

Patch an oscillator's output into its own pitch. To make sample 100 of the output, the engine needs the pitch at sample 100. To get that pitch, it needs the output at sample 100. Each waits for the other. There is no place to start, so the engine cannot compute it as written.

A real circuit solves this with time. A signal takes a moment to travel round, so the input always sees the output from a moment ago. In a digital engine that moment must be a whole number of samples. The smallest is **one sample**.

> A loop in a patch needs at least one **unit delay**: a module that outputs its input from one sample ago. The loop then reads "the output so far".

The textbook name is `z^-1` (Julius O. Smith, *Introduction to Digital Filters*). Every digital filter with feedback has one inside.

## Why exactly one sample

Why not delay by a whole block, which is cheaper? Because then the delay depends on the block size ([ADR-0002](../adr/0002-graph-execution-and-feedback.md)). Take the smallest feedback: `y = x + 0.5 * y_delayed`. Send a single 1.0 through it.

| Delay | Output at samples | What you hear |
|---|---|---|
| 1 sample | 1, 0.5, 0.25, 0.125, ... at every sample | A short smooth decay |
| 64-sample blocks | 1 at sample 0, 0.5 at 64, 0.25 at 128, ... | An echo every 64 samples |
| 512-sample blocks | 1 at sample 0, 0.5 at 512, ... | An echo every 512 samples |

The same patch is a smooth decay at one buffer size and a rhythmic echo at another. A feedback loop sets the pitch of a plucked string, the ring of a filter, and the color of a comb, all from the length of its delay, so one sample more or less is audible. The delay must not come from the buffer.

## So loops run one sample at a time

Outside a loop, the engine runs each module on a whole block: it is faster, and the order does not matter beyond "sources first". Inside a loop that cannot work. Frame 2 needs the delayed result of frame 1, which needs frame 0.

So the compiler ([ADR-0002](../adr/0002-graph-execution-and-feedback.md)):

1. Finds the loops. A loop is a **strongly connected component** (Tarjan's algorithm): a group of modules that all depend on each other.
2. Runs the acyclic parts in blocks, in order.
3. Runs each loop as an inner loop that calls its modules with **one frame** each, for every frame of the block.

The modules in a loop are called with block size 1, so they must give the same output for any block size ([block-size invariance](block-size-invariance.md)).

**The cost:** a call per module per sample. We measured about 5 to 15 ns per module per sample, and a loop of 50 filters uses about 12% of one core's real-time budget. Only the modules inside the loop pay. Patches without loops run at full block speed.

## Where the delay goes

You can place the delay yourself with a **1-Sample Delay** module (`loom.time.delay1`). Then you choose where the loop is broken, and that matters for sound: the delay moves the whole loop's timing, and some places add it where you hear it less.

If a loop has no delay, the compiler inserts one, and says so:

- It picks the module in the loop with the fewest unresolved inputs, and the lowest ID if tied, so the choice is the same every time.
- `loom check` prints where. The UI shows it.
- When it has a choice, it delays a modulation assignment before a cable ([modulation without cables](modulation-without-cables.md)).

The example patch `fm-feedback.ron` has no delay of its own. `loom check examples/patches/fm-feedback.ron` shows the one the compiler adds.

When the patch is edited while it plays, the delay's state carries over by key, so a running loop does not restart ([hot patching](hot-patching.md)).

## In other systems

- **VCV Rack:** as we understand its engine, modules run one sample at a time and each cable adds one sample of delay between modules, so every loop has a delay already. It is simple, but the delay is on every cable, whether you want it there or not.
- **Bitwig Grid:** as we understand it, a loop needs a feedback module that you place to mark the path. LoomDSP's 1-Sample Delay does the same job, and when you forget it, the compiler adds one and tells you.

## Further reading

- Julius O. Smith, *Introduction to Digital Filters*, on the unit delay and feedback (free online at ccrma.stanford.edu/~jos).
- Udo Zölzer (ed.), *DAFX: Digital Audio Effects*, on feedback delay structures.
