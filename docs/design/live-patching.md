# Changing a Running Patch Specification

What carries over when the player edits a patch while it plays, so a new cable joins the running sound with no cold start, click, or gap.

- **Kind:** living specification.
- **Status:** Rules 1 to 4 Implemented. Rules 5 and 6 Agreed, from the research in [#203](https://github.com/brylie/LoomDSP/issues/203), not built yet. Rule 6 is built with stereo copies (#23).
- **Decision behind it:** [ADR-0002](../adr/0002-graph-execution-and-feedback.md) (schedules and inserted delays), [ADR-0022](../adr/0022-copies-take-state-from-the-running-instance.md) (state of channel copies).
- **Use case and requirements:** FR-GRAPH-01, FR-GRAPH-02, FR-GRAPH-03, FR-SIG-07, NFR-RT-01, NFR-RT-05, NFR-SND-02
- **Background:** [Hot patching](../learn/hot-patching.md): why a new path clicks, and why a crossfade does not fix it.
- **Prior art:** in Eurorack, every module runs all the time, and a cable only connects what is already running. VCV Rack behaves the same way. Named only to explain the expectation.
- **Issues:** #203, #23
- **Code:** `crates/loom-core/src/compile.rs` (`Schedule::take_tap_state_from`, `take_smooth_state_from`), `engine.rs` (`Command::SwapSchedule`). Measurements: `crates/loom-modules/tests/hot_patch.rs`.

## Goal

"I plug a cable into a running patch. The new path should join the sound that is playing, as it does on hardware. If it starts from reset, it sounds like a bug."

## Behaviour

Every edit that changes the graph compiles a new schedule on the control thread. The engine swaps it in at a buffer boundary (NFR-RT-05). The player hears the change at that boundary and nothing else.

| What the swap touches | Carried across | How |
|---|---|---|
| Module instances | Yes | They live in engine slots, not in the schedule. A module runs whether or not anything is patched to it. |
| Parameter smoothing, engine side | Yes | By fill key (`take_smooth_state_from`). A new fill starts from `Module::param_glide`. |
| Unit delays (user and inserted) | Yes, while the key stays | By key (`take_tap_state_from`). The key is the module ID, the delayed input, or the assignment ID. |
| Transport, output safety, MIDI state | Yes | They live in the engine. |
| Port buffers | Not needed | Every buffer is written before it is read in each block. |
| A new inserted delay | **No: starts at 0** | Rule 5 fixes it (not built yet). |
| Copy 1 of a module that turns stereo | **No: starts from reset** | Not built yet (#23). Rule 6 fixes it. |
| A module the player adds | No, by design | A new module is new hardware: it starts from reset. |

## Rules

1. **Module instances outlive schedules.** A swap never resets, rebuilds, or reorders a module's state. *Why:* plugging a running LFO into a new input must pick it up mid-cycle, as in hardware.

2. **Parameter smoothing carries across a swap.** Engine-side smoothing moves to the new schedule by key. *Why:* a recompile while a knob glides must not step the knob.

3. **A unit delay keeps its state while its key stays the same.** *Why:* a recompile must not touch a running feedback loop. A test checks that adding an unrelated module leaves a running loop bit-identical.

4. **A new module starts from reset.** *Why:* it has no history. This matches hardware, where a module is plugged in before it is patched.

5. **A unit delay with no state to carry starts with the last value its source output wrote before the swap.** This covers a delay the compiler inserts when a cable closes a new loop, a delay that moves to another cable when a loop changes, a new assignment delay, and a new Unit Delay module. If the source output did not run in the old schedule, the delay starts at 0, as today. *Why:* measured in `hot_patch.rs`:

   | Case | Today (starts at 0) | With rule 5 |
   |---|---|---|
   | New loop closed into an input that reads 0 when unplugged (Add B) | The cable acts as if plugged one sample later. No extra step. | Same, one sample earlier. |
   | New loop closed into an input with another default or fallback (Multiply B reads 1) | The signal drops out for one sample: error -6 dB. | Bit-identical to a delay that was already running. |
   | A new cable moves the inserted delay to another cable of a running loop | The feedback drops out for one sample: error -17 dB. | Bit-identical to no move, because both placements give the same loop. |

   The value is what a delay that had always been on that cable would hold, so the new delay continues the signal instead of inventing a 0. *How:* the old schedule keeps a table of its output buffers, sorted by output. At the swap, each new delay looks up its source there and reads the last frame of the last block. A binary search per new delay, no allocation.

6. **Copy 1 of a module that turns stereo takes copy 0's state.** The rules are in the [stereo spec](stereo.md), rules 13 and 25 to 27, and the decision in ADR-0022. *Why:* measured with the SVF and the same signal on both sides, a cold copy 1 is off by -2 to -24 dB and takes 1 to 20 ms to settle. A 5 ms crossfade scales that down but does not shorten it. Copied state is exact.

### Click threshold

The measurements compare what the engine plays with the ideal continuation: what the patch would play if the new path had been running all along. An error that peaks above -40 dBFS (0.01) counts as a click. One below -60 dBFS (0.001) counts as inaudible. *Why:* a one-sample error near -40 dBFS is audible over quiet material, and -60 dBFS is below the noise of most rooms. The numbers are a test tool, not a perception study. A listening check confirms them (see Acceptance criteria).

## Acceptance criteria

- [x] Given a running feedback loop, when an unrelated module is added, then the output is bit-identical to the patch without the edit. (`hot_patch.rs`)
- [ ] Given a running Multiply whose B input is then patched from a new loop, then the output is bit-identical to the same patch with an explicit Unit Delay that was already running. (Today: a one-sample dropout, pinned by `hot_patch.rs`.)
- [ ] Given a running loop, when a new cable moves the inserted delay to another cable, then the output is bit-identical to the patch without the edit, as long as the new cable adds nothing. (Today: -17 dB error, pinned by `hot_patch.rs`.)
- [ ] Given a running SVF that turns stereo, with the same signal on both sides, then its right output is bit-identical to its left from the first frame after the swap. (Needs #23.)
- [ ] Given each case above, then the `no_alloc` tests pass, and the output is the same at block sizes 1, 77, and 512 when the edit lands on the same frame.
- [ ] Listening check: in the app, with a resonant filter and a sustained sound, patch a stereo source into the chain, close a feedback loop into a Multiply's B input, and add a second cable to a running loop. No click is heard. (Manual check in the pull request that builds rules 5 and 6.)

## Out of scope

- **Fading cables in and out** (FR-GRAPH-05). A fade softens the step that plugging a cable makes in any system, hardware included. This spec is about state, so that the step is no bigger than plugging the cable should make.
- **Keeping the state of a deleted module for undo.** Undo of a delete creates a new instance (rule 4). A later change could hand the old state over through the same method as ADR-0022.
- **Replacing a module with a newer version** while it plays.

## Open points

1. **Time to copy large state.** Copying a 2 second delay line takes tens of microseconds. A chain of many long delays turning stereo at once adds up inside one buffer. Watch it with the engine benchmark when MT-DELAY-02 lands. If it is too slow, the copy could be spread over several buffers while the right side still plays copy 0's output, which would need its own rule.
