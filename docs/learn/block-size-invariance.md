# Block-size invariance

Why a module must give the same output whether the engine calls it with 1 frame or with 512, what breaks it, and how the test harness catches it. For developers who write modules, and for anyone who wonders why a patch must sound the same at any buffer size.

- **Kind:** explainer.
- **Used by:** [AGENTS.md](../../AGENTS.md#hard-rules) (hard rule 2), [module test harness](../design/module-test-harness.md), [ADR-0002](../adr/0002-graph-execution-and-feedback.md).
- **Related:** [Feedback loops and the unit delay](feedback-loops.md), [Triggers and gates](triggers-and-gates.md).
- **Code:** `crates/loom-test` (`check_block_sizes`), `loom_core::signal::EdgeDetector`.

## Blocks

Audio does not move one sample at a time between the engine and the device. It moves in **blocks**. The device asks for a buffer of 64, 128, or 512 frames, and the engine splits that into blocks of at most `max_block` frames. A module's `process` gets one block of every input and fills one block of every output.

The block size is not a musical setting. It is a choice of the player's audio interface, made for latency and CPU. A patch that sounds different at 64 than at 512 would make the player's hardware part of the sound. LoomDSP promises the opposite: the same patch gives **bit-identical** output at any block size (hard rule 2, with determinism, rule 3).

There is a second reason. Inside a feedback loop the engine calls each module with **one frame**, because the loop needs the last output before it can compute the next ([feedback loops](feedback-loops.md)). A module that is only right for big blocks would be wrong in a loop.

## The wrong turn: a smoother that steps once per block

Say a module smooths a gain change so it does not click. The tempting code moves the smoothed value toward the target **once per call**:

```rust
// Wrong: one step per block, however long the block is.
self.gain += (self.target - self.gain) * 0.1;
for x in out { *x = input * self.gain; }
```

The target jumps from 0 to 1. After 8 frames the smoothed gain is:

| Block size | Steps taken | Gain after 8 frames |
|---|---|---|
| 1 | 8 | 0.570 |
| 4 | 2 | 0.190 |
| 8 | 1 | 0.100 |

The glide is three times slower at block 4 than at block 1, and almost six times slower at block 8. At 512 it would be slower still, and the gain would also move in steps you can hear. The fix is to step **per frame**, inside the loop, so the number of steps depends on the number of frames and not the number of calls. The same mistake appears in many forms:

- Computing an LFO value once per block and using it for every frame.
- Advancing a phase by `block_length * step` and reading it only at the start of the block.
- Reading `input[0]` and ignoring the rest of the block.
- Keeping a trigger detector's state in a local variable, so it forgets the last sample of the previous block. Keep it in the module, as `EdgeDetector` does.
- Using the block length as a time, for example "fade over 4 blocks".

The rule behind all of them: **a module's state must advance once per frame, and the output for frame `i` may depend only on the inputs at frames up to `i` and on the state, never on where the block starts or ends.**

## Parameters and blocks

An unmodulated parameter is one value per block. That is fine, because the value only changes between blocks, at a point the control thread chooses. But the point it changes at depends on block size, so a module that wants a smooth change smooths it itself, per frame, as above. A modulated parameter is a buffer with one value per frame, and the module reads it per frame ([modulation](modulation-without-cables.md)).

## How the harness checks it

`Harness::check_block_sizes` ([module test harness](../design/module-test-harness.md)) renders the module in blocks of **1, 77, and 512** frames and fails unless the three outputs are bit-identical.

- **1** is how feedback loops call a module.
- **512** is a normal large block.
- **77** is not a power of two on purpose. A module that happens to work when blocks are 64 or 128 long can break at 77. For example, a block that is not a multiple of an internal chunk size, or a loop that assumes the block length is even.

The check needs no tolerance. The three renders run on one machine, with the same maths library, so any difference is a bug. Every built-in module has the check, and a new module must pass it before a pull request is merged.

## In other systems

- **VCV Rack:** as we understand it, runs every module one sample at a time, so a module never sees a block. The rule is met by design, at a CPU cost.
- **Plugin hosts (VST, AU, CLAP):** call a plugin with whatever buffer size the host chooses, and may change it while running. Plugins that are not invariant sound different in different hosts. LoomDSP will export plugins, so the rule matters there too.
