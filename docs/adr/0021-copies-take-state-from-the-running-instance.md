# ADR-0021: A new channel copy takes its state from the running instance

- **Status:** Accepted
- **Date:** 2026-10-19
- **Related requirements:** FR-GRAPH-01, FR-GRAPH-02, FR-SIG-07, NFR-RT-01, NFR-SND-02
- **Specification:** [Changing a running patch](../design/live-patching.md), [Stereo cables](../design/stereo.md) rules 13 and 25 to 27
- **Background:** [Hot patching](../learn/hot-patching.md)
- **Refines:** [ADR-0020](0020-inferred-channel-count.md), which left copy 1 starting from reset.

## Context

When a cable change makes a running chain stereo, each module in it gets a second instance, copy 1, for the right channel (ADR-0020). Copy 1 is built on the control thread and starts from reset, while copy 0 carries on. Players who patch hardware expect a new cable to join the sound that is already playing. A right side that starts cold sounds like a bug.

Measurements for #203 (`crates/loom-modules/tests/hot_patch.rs`, same signal on both sides, so the ideal right side equals the left):

| SVF output, cutoff, resonance | Cold copy 1: peak error | Settles in |
|---|---|---|
| Lowpass, 200 Hz, 0.2 | -6 dB | 13 ms |
| Lowpass, 1000 Hz, 0.2 | -12 dB | 1 ms |
| Lowpass, 500 Hz, 0.9 | -3 dB | 20 ms |
| Highpass, 200 Hz, 0.2 | -12 dB | 14 ms |

Most of the long settle times come from parameter smoothing: a new instance glides from its built-in values to the current ones. A delay line (MT-DELAY-02, P1) would be worse: its right side would miss every echo for the whole delay time.

Constraints: nothing on the audio thread may allocate (hard rule 1). Output must stay deterministic and independent of block size (rules 2 and 3). A mono patch must cost nothing extra, so copy 1 cannot be kept alive all the time (ADR-0020).

## Options considered

1. **Accept the cold start.** No work. Every measured case is above the click threshold (-40 dBFS), and long-memory modules are wrong for seconds.
2. **Crossfade the right side** from copy 0's output to copy 1's over a few milliseconds. Works for every module with no author effort. It only scales the error down while the fade runs. When copy 1 takes longer to settle than the fade, the right side is still wrong afterwards (the 200 Hz and 500 Hz rows above, and any delay line). It also needs a fade stage in the engine and an extra buffer per copied output.
3. **Copy state from copy 0** into copy 1 at the swap, through an optional `Module` method. Exact: copy 1 continues where copy 0 was, including its smoothing. Each module with state must implement it, and the copy runs on the audio thread at the swap, in time that grows with the size of the state.
4. **Run copy 1 in the background** on the control thread, fed with copy 0's input history, until it catches up. Needs the input history, cannot work for modules whose state depends on more than a short window, and is complex.

## Decision

Option 3. `Module` gets an optional method, `copy_state_from`, with a default that does nothing. At the swap that first runs copy 1, the engine calls it with copy 0, before copy 1 processes. A module that implements it continues on the right exactly where the left was. A module that does not starts copy 1 from reset, as before. No crossfade.

The exact signature and the rules for seeded modules are in the [stereo spec](../design/stereo.md), rules 25 to 27.

## Consequences

- **A chain that turns stereo keeps its sound on both sides.** With the same signal on both sides, the right side is bit-identical to the left.
- **Module authors write one more method** for each module with state: copy the fields, and copy buffers into the ones copy 1 already has. The module test harness checks it, and every built-in module with state implements it. Third-party modules that skip it still work, with a cold right side.
- **The copy costs time on the audio thread,** once per module that turns stereo. Copying a 2 second delay line at 48 kHz moves 384 KB. That is tens of microseconds, inside one buffer. The spec lists it as an open point to watch with the benchmarks.
- **Seeded modules must not copy their random state as it is,** or the copy would play the same numbers as the left side, which is mono in disguise (stereo rule 15).
- **The same method can serve later features** that need to hand state from one instance to another, such as replacing a module with a newer version. Those need their own decisions.
