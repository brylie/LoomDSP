# Changing a Running Patch Specification

What carries over when the player edits a patch while it plays, so a new cable joins the running sound with no cold start, click, or gap.

- **Kind:** living specification.
- **Status:** Rules 1 to 5, 7 ([#230](https://github.com/brylie/LoomDSP/issues/230)), and 8 Implemented. Rule 6 Agreed, from the research in [#203](https://github.com/brylie/LoomDSP/issues/203), and built with stereo copies (#23). Rules 9 ([#21](https://github.com/brylie/LoomDSP/issues/21)) and 10 ([#416](https://github.com/brylie/LoomDSP/issues/416)) Agreed, M2 (2026-11-01).
- **Decision behind it:** [ADR-0002](../adr/0002-graph-execution-and-feedback.md) (schedules and inserted delays), [ADR-0022](../adr/0022-copies-take-state-from-the-running-instance.md) (state of channel copies).
- **Use case and requirements:** FR-GRAPH-01, FR-GRAPH-02, FR-GRAPH-03, FR-GRAPH-05, FR-GRAPH-09, FR-SIG-07, NFR-RT-01, NFR-RT-05, NFR-SND-02
- **Background:** [Hot patching](../learn/hot-patching.md): why a new path clicks, and why a crossfade does not fix it. See also [Feedback loops and the unit delay](../learn/feedback-loops.md).
- **Prior art:** in Eurorack, every module runs all the time, and a cable only connects what is already running. VCV Rack behaves the same way. Named only to explain the expectation.
- **Issues:** #203, #23, [#230](https://github.com/brylie/LoomDSP/issues/230), [#22](https://github.com/brylie/LoomDSP/issues/22) (rule 8)
- **Code:** `crates/loom-core/src/compile.rs` (`CompileReport::skipped`), `compile/order.rs` (`reaches_a_root`), `compile/schedule.rs` (`Schedule::take_tap_state_from`, `Schedule::sources`, `take_smooth_state_from`), `engine.rs` (`Command::SwapSchedule`, `last_frames`), `controller.rs` (new modules wait in `Controller::fresh` until the commit, rule 7). Measurements: `crates/loom-modules/tests/hot_patch.rs`.

## Goal

"I plug a cable into a running patch. The new path should join the sound that is playing, as it does on hardware. If it starts from reset, it sounds like a bug."

## Behaviour

Every edit that changes the graph compiles a new schedule on the control thread. The engine swaps it in at a buffer boundary (NFR-RT-05). The player hears the change at that boundary and nothing else.

| What the swap touches | Carried across | How |
|---|---|---|
| Module instances | Yes | They live in engine slots, not in the schedule. A module runs whether or not anything is patched to it. |
| Parameter smoothing, engine side | Yes | By fill key (`take_smooth_state_from`). A new fill starts from `Module::param_glide`. |
| Unit delays (user and inserted) | Yes, while the key stays | By key (`take_tap_state_from`). The key is the module ID, the delayed input, or the assignment ID. |
| A new unit delay, or one that moved | Its source's last value | Rule 5. |
| Transport, output safety, MIDI state | Yes | They live in the engine. |
| Port buffers | Not needed | Every buffer is written before it is read in each block. |
| Copy 1 of a module that turns stereo | **No: starts from reset** | Not built yet (#23). Rule 6 fixes it. |
| A module the player adds | No, by design | A new module is new hardware: it starts from reset. |
| A module that reaches nothing | Yes, paused | It is not run while nothing hears or shows it (rule 8). |

## Rules

1. **Module instances outlive schedules.** A swap never resets, rebuilds, or reorders a module's state. *Why:* plugging a running LFO into a new input must pick it up mid-cycle, as in hardware.

2. **Parameter smoothing carries across a swap.** Engine-side smoothing moves to the new schedule by key. *Why:* a recompile while a knob glides must not step the knob.

3. **A unit delay keeps its state while its key stays the same.** *Why:* a recompile must not touch a running feedback loop. A test checks that adding an unrelated module leaves a running loop bit-identical.

4. **A new module starts from reset.** *Why:* it has no history. This matches hardware, where a module is plugged in before it is patched.

5. **A unit delay with no state to carry starts with the last value its source output wrote before the swap.** This covers a delay the compiler inserts when a cable closes a new loop, a delay that moves to another cable when a loop changes, a new assignment delay, and a new Unit Delay module. If the source output did not run in the old schedule, the delay starts at 0. So does every new delay when two swaps land before one block runs, because the middle schedule never wrote its buffers. *Why:* measured in `hot_patch.rs`:

   | Case | Starting at 0 (before rule 5) | With rule 5 |
   |---|---|---|
   | New loop closed into an input that reads 0 when unplugged (Add B) | The cable acts as if plugged one sample later. No extra step. | Bit-identical to a delay that was already running. |
   | New loop closed into an input with another default or fallback (Multiply B reads 1) | The signal drops out for one sample: error -6 dB. | Bit-identical to a delay that was already running. |
   | A new cable moves the inserted delay to another cable of a running loop | The feedback drops out for one sample: error -17 dB. | Bit-identical to no move, because both placements give the same loop. |

   The value is what a delay that had always been on that cable would hold, so the new delay continues the signal instead of inventing a 0. *How:* the old schedule keeps a table of its output buffers, sorted by output. At the swap, each new delay looks up its source there and reads the last frame of the last block. The engine remembers that block's length. A binary search per new delay, no allocation.

6. **Copy 1 of a module that turns stereo takes copy 0's state.** The rules are in the [stereo spec](stereo.md), rules 13 and 25 to 27, and the decision in ADR-0022. *Why:* measured with the SVF and the same signal on both sides, a cold copy 1 is off by -2 to -24 dB and takes 1 to 20 ms to settle. A 5 ms crossfade scales that down but does not shorten it. Copied state is exact.

7. **A module made with non-default values starts at those values.** `reset()` snaps all smoothing to its targets (Constant already does this). After a module is built and its first parameter values are set, and before it is sent to the engine, the `Controller` calls `reset()` once. So the `Controller` keeps a new module until the next commit: values set before it go straight to the module, and the commit resets it and sends it in, before the new schedule. The `loom-test` harness does the same before it renders. *Why:* a factory builds the module with built-in values, then `set_param` sets the real ones. A smoothed parameter treats that as a new target and glides. A pasted filter at 200 Hz would sweep down from 1 kHz for 10 ms, with nothing to hide it (loading a patch has the output fade-in). *Why this option, not a flag per module or a factory that takes values:* the call is made in one place, so a module author cannot forget it, and `reset()` on a module that has just been built clears nothing that matters. Modules must keep this convention: **`reset()` snaps smoothing to the current targets.** A test in the harness checks it for every registered module: a module built with defaults, given a changed value and `reset()`, matches one that held the value for a long time, from the first sample.

8. **A module that reaches nothing the player hears or sees is not run, and keeps its state** (FR-GRAPH-09). The compiler leaves it out of the schedule and lists it in `CompileReport::skipped`. Its instance stays in its engine slot, so a cable that connects it again resumes it where it stopped: not reset, and not moved on by the time it was skipped. A module runs if a path of cables or modulation assignments leads from it to one of these:
   - the audio output;
   - a probed output, such as a meter or scope on screen ([telemetry](telemetry.md));
   - a module with display values, or with a light or meter on its panel ([panels](panels.md)), so its lights tell the truth with no cable. MIDI Note has a gate light, so it hears every note on and off while unplugged, and a cable added later finds no stuck gate;
   - a module with a modulated parameter, whose knob shows a moving live dot ([modulation](modulation.md)).

   A feedback loop that reaches nothing is still in the compile report with its inserted delays, so the canvas shows them, but it does not run. *Why:* the requirement saves CPU in large patches, and a module nobody hears or sees changes nothing the player can tell. *Why these roots:* the specs for telemetry and panels promise that what the screen shows is live, and the specs do not say a light may freeze. *Why pause and not reset:* a player who unplugs and replugs an LFO expects it to keep its settings and phase, not start cold. *Cost:* a paused module resumes with old state. A delay line unplugged at its output and plugged in again a minute later first plays the audio it held a minute ago, where hardware would play the last seconds of its input. A module that will one day have side effects (MIDI out) must count as a root.

9. **A cable change fades over 5 ms** (FR-GRAPH-05). When a schedule swap changes what feeds an input (a cable added, removed, or moved to another output), the input holds the last value it read before the swap and moves in a straight line to the new signal over 5 ms, sample by sample. The same holds for a modulation assignment that is added or removed: its contribution ramps in or out over 5 ms.
   - **Only audio, modulation, and pitch inputs fade** (by the port's `SignalType`). Gate, trigger, and phase inputs switch on the swap frame. *Why:* a ramped gate crosses 0.5 late and fires late, and a ramped phase passes through values that look like wraps.
   - **Nothing fades on the first schedule** after a load or a new engine, nor on any schedule swapped in before the engine has played its first block. *Why:* a patch renders the same whether or not it was edited live first, and however many commits it took to load; nobody heard a step before the first block. The output fade-in covers the start ([output safety](output-safety.md)). (Widened on 2026-11-14 from "the first schedule", because a load in the editor can commit more than once before it plays.)
   - The fade state is a fixed slot per input, prepared on the control thread with the schedule. The engine adds no allocation, lock, or wait (AGENTS.md rule 1).
   - Fixed at 5 ms, with no setting. *Why:* long enough to soften the step, short enough that nobody hears it as a fade.
   - *Why hold the last value, not crossfade from the old signal:* the old source may be gone, and keeping it running costs CPU for a sound that is leaving. Holding the last value keeps the input continuous, which is all a click needs. The rules above still make the new signal right from its first sample (rules 5 and 6); the fade only softens the step the cable itself makes.
   - **The ramp.** N = 5 ms of frames, rounded, at least 1 (240 at 48 kHz). On the first frame after the swap the input reads the held value exactly. On frame k it reads `held · (1 − k/N) + signal · k/N`. From frame N on, it reads exactly the same samples as the signal. The ramp never makes a value that is not finite. A held value that is NaN or infinite does not fade: the input switches, because holding it would only spread a broken value into a clean cable (settled 2026-11-14).
   - **What counts as a cable change** (settled 2026-11-14). The input's cable in the graph: plugged, pulled, or moved to another output. Every channel of that input fades: each copy, the right side of a stereo input, and each voice of a bundle input. These are **not** cable changes and do not fade, because rules 5 and 6 already keep the signal continuous, and a fade would put a short dip into a signal that did not step:
     - a delay the compiler inserts, or one that moves;
     - a downmix that appears, or a source that changes width with the same cable.
   - **New and paused modules.** A new module has nothing to hold, so its inputs read their signals from the first frame (rule 4). A new copy of a running module, such as the right side of a module that turns stereo, fades together with copy 0 on an input whose cable changed. A module that resumes after it was skipped (rule 8) read nothing while paused, so on a changed cable it fades from 0.
   - **Which inputs fade.** Audio, modulation, and pitch inputs of every module, the Audio Out input (so pulling the cable to the output fades it out), and the Unit Delay input. Gate, trigger, and phase inputs switch.
   - **The mixers the editor adds** (Sum, Stereo spread sum, Select, and Swap fade; [sub-patches](sub-patches.md)). A mixer's inputs carry any signal, so whether one of its inputs fades follows the first source upstream that is not a mixer. A gate through a Sum switches. An input's own gate, trigger, or phase port always switches. A Select's member input never fades.
     - **An exact rewire switches.** Moving a cable onto a new mixer that plays the old source alone on its first frame switches. So does moving a cable off a leaving mixer that played the new source alone on its last frame. This covers the start and the finish of a slot swap, Single to Layer of one member, and Single to Select. A running fade on that input goes on toward the new source.
     - **A Layer that adds members.** When a Layer of one member gets more members, the cable moves onto a new Sum at gain 1. It switches, and each new member fades in from 0, as a new cable does.
     - **Every other change of mixer fades once.** A stack going from 1 copy to 2, or from 2 to 1, and Layer to Select fade from the held value to the new output, whose own inputs read their signals from the first frame. There is never a fade into a fade. A slot swap during a fade fades too, so the step the first fade had left is softened.
   - **Modulation assignments** (settled 2026-11-14). The contribution, `depth · source` in the parameter's 0 to 1 range, fades:
     - **Added:** it ramps in from 0.
     - **Removed:** its last contribution is held and ramps to 0.
     - **Moved** to another source: from the held contribution to the new one. **Moved** to another target: it ramps out on the old parameter and in on the new one.
     - An assignment from a gate or a phase source onto a continuous parameter ramps too. The target is a value, not a gate.
     - **Exceptions, each with its reason.** A whole-number or choice parameter switches, because a ramp across separate values adds steps instead of softening one. An assignment onto a module that is new in this swap applies from the first frame, so a new part, a paste, or an undo plays its modulated value at once (rule 7). An exact rewire through a mixer switches, as above.
   - **A swap while a fade runs.** If the cable did not change again, the fade goes on where it was. If it changed again, a new fade starts from the value the input read last, so nothing steps. When two swaps land before one block, the middle schedule never ran. An input that was already fading keeps that fade's held value, an input that read a fixed default holds that value, and anything else holds 0, as for rule 5's delays.
   - **What still steps.** A player meets these, each for its reason:
     - gate, trigger, and phase inputs, including a phase cable moved on an oscillator: its audio jumps, because no audio cable changed;
     - turning an assignment off, or changing its depth or polarity, which is not a schedule swap;
     - a module that picks another source in its own code when an input has no cable, such as a sidechain;
     - the extra voices of a bundle that gets narrower, such as lowering a voice count while notes ring;
     - deleting the Audio Out module itself: the output stops at once;
     - a Select within 10 ms of a member change, or with a modulated or MIDI-mapped member, during Single to Select and back;
     - two swaps before one block, for an input that was not already fading;
     - a choice input, such as a Selector's `select`, passes through the values in between for up to 5 ms, so it can briefly play the inputs between the old and the new choice. Output safety still limits the level.

10. **Staged modules are retried when the queue has room** ([#416](https://github.com/brylie/LoomDSP/issues/416)). When a commit has more new modules than the bounded command queue can hold, the rest are staged on the control side, as today. While anything is staged, a watcher thread reads the queue's free space every 10 ms and wakes the UI when there is room for the next staged module. The UI then commits again. The watcher stops when nothing is staged. *Why a watcher:* the audio thread may not signal a waiting thread, because that takes a lock (AGENTS.md rule 1). Reading a counter from another thread is safe, and the app already wakes this way for audio state and MIDI learn.
    - The longest wait after the engine makes room is one audio buffer plus 10 ms.
    - A queue that stays full causes no compile and no repeated attempt: the watcher wakes the UI only when free space grows.
    - A staged module stays staged until it is committed; nothing is lost. The status bar keeps saying how many modules are waiting, and the undo step for the edit is recorded once, when the edit was made.

### Click threshold

The measurements compare what the engine plays with the ideal continuation: what the patch would play if the new path had been running all along. An error that peaks above -40 dBFS (0.01) counts as a click. One below -60 dBFS (0.001) counts as inaudible. *Why:* a one-sample error near -40 dBFS is audible over quiet material, and -60 dBFS is below the noise of most rooms. The numbers are a test tool, not a perception study. A listening check confirms them (see Acceptance criteria).

## Acceptance criteria

- [x] Given a running feedback loop, when an unrelated module is added, then the output is bit-identical to the patch without the edit. (`hot_patch.rs`)
- [x] Given a running Multiply whose B input is then patched from a new loop, then the output is bit-identical to the same patch with an explicit Unit Delay that was already running. The same holds for an Add's B input. (`hot_patch.rs`)
- [x] Given a running loop, when a new cable moves the inserted delay to another cable, then the output is bit-identical to the patch without the edit, as long as the new cable adds nothing. (`hot_patch.rs`)
- [x] Given the delay cases above, then the output is the same at block sizes 1, 64, 77, and 512 when the edit lands on the same frame (`hot_patch.rs`), and the `no_alloc` test passes while it closes loops on modules that already ran.
- [ ] Given a running SVF that turns stereo, with the same signal on both sides, then its right output is bit-identical to its left from the first frame after the swap. (Needs #23.)
- [ ] Given the stereo case, then the `no_alloc` tests pass, and the output is the same at block sizes 1, 77, and 512 when the edit lands on the same frame. (Needs #23.)
- [x] Given a module added with non-default values (the SVF at 200 Hz, Multiply with gain 3), then its output from the first frame matches a copy that held the values for a long time, at block sizes 1, 77, and 512 (rule 7). The harness check runs for every registered module. (`loom-modules/tests/first_values.rs`; `Harness::check_reset` on every module in `tests/reset.rs`)
- [x] Pasting a filter while the patch plays adds no glide (an app test, or a manual check listed in the pull request). Reference renders that contained the glide are updated in the same pull request. (`first_values.rs` adds modules to a running engine with `set_params` before the commit, the calls paste and undo make in `clipboard.rs` and `undo.rs`. A listening check in the app is in the pull request.)
- [x] Given a chain that reaches nothing, then it is not processed, and a cable that connects it again resumes its state (rule 8). Lights, probes, and modulated parameters keep their modules running, and an unreachable loop is reported but not run. (`loom-core/tests/reach.rs`, `compile/tests.rs`, `engine.rs` in `loom-modules`)
- [ ] Listening check, rule 5: in the app, with a sustained sound, close a feedback loop into a Multiply's B input, and add a second cable to a running loop. No click is heard. (Manual check.)
- [ ] Given a running sine into a filter, when its cable is removed, then the filter input ramps from the last value to 0 over 5 ms, at block sizes 1, 77, and 512 when the edit lands on the same frame (test, rule 9).
- [ ] A gate, trigger, or phase input switches on the swap frame with no ramp (test, rule 9).
- [ ] Adding a modulation assignment ramps its contribution in over 5 ms (test, rule 9).
- [ ] A patch rendered from load has no fades (test: bit-identical to the current reference renders, rule 9).
- [ ] The fades add no allocation on the audio thread (`no_alloc` test, rule 9).
- [ ] The staged retry adds no allocation on the audio thread (`no_alloc` test, rule 10).
- [ ] With a small command queue and a paste larger than it, every staged module is committed after the engine drains the queue, with no edit and no periodic tick, and the audio matches a commit with enough capacity (app test, rule 10).
- [ ] A queue that stays full causes no repeated compile (test, rule 10).
- [ ] Listening check, rule 6: with a resonant filter and a sustained sound, patch a stereo source into the chain. No click is heard. (Manual check, with #23.)

## Out of scope

- **Keeping the state of a deleted module for undo.** Undo of a delete creates a new instance (rule 4). A later change could hand the old state over through the same method as ADR-0022.
- **Replacing a module with a newer version** while it plays.

## Open points

1. **Time to copy large state.** Copying a 2 second delay line takes tens of microseconds. A chain of many long delays turning stereo at once adds up inside one buffer. Watch it with the engine benchmark when MT-DELAY-02 lands. If it is too slow, the copy could be spread over several buffers while the right side still plays copy 0's output, which would need its own rule.
