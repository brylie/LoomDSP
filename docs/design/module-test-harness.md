# Module Test Harness Specification

A test harness for module authors: render one module with given inputs and parameters, check it against a stored reference render, and check that block size does not change its output.

- **Kind:** living specification.
- **Status:** Implemented, M1. [Blind spots in the shared checks](#blind-spots-in-the-shared-checks), [All inputs at once](#all-inputs-at-once), and [Changes during a render](#changes-during-a-render) Implemented, M2 (2026-11-04, [#564](https://github.com/brylie/LoomDSP/issues/564), [#575](https://github.com/brylie/LoomDSP/issues/575), [#96](https://github.com/brylie/LoomDSP/issues/96)).
- **Decision behind it:** [ADR-0002](../adr/0002-graph-execution-and-feedback.md) ("The SDK harness must cover this"), [ADR-0037](../adr/0037-finite-inputs-hold-at-the-largest-finite-value.md) (the overflow rule).
- **Background:** [Block-size invariance](../learn/block-size-invariance.md): why a module must sound the same at any block size, and what breaks it.
- **Use case and requirements:** FR-SDK-02, NFR-OSS-05.
- **Issues:** [#42](https://github.com/brylie/LoomDSP/issues/42), [#208](https://github.com/brylie/LoomDSP/issues/208) (test modules), [#127](https://github.com/brylie/LoomDSP/issues/127) (non-finite inputs), [#552](https://github.com/brylie/LoomDSP/issues/552) (huge inputs), [#564](https://github.com/brylie/LoomDSP/issues/564), [#575](https://github.com/brylie/LoomDSP/issues/575), [#96](https://github.com/brylie/LoomDSP/issues/96)
- **Code:** `crates/loom-test`; the built-in modules use it in `crates/loom-modules/tests/reference.rs`.

## Goal

A module author wants to know three things before opening a pull request: the module sounds the same as last time, it does not depend on block size, and a reviewer can hear what it does. Today `loom_core::testing::run` runs a module once, and every test builds its buffers by hand.

```rust
use loom_test::{Harness, Signal, reference};

#[test]
fn sine() {
    Harness::new(&loom_modules::osc::SINE)
        .seconds(0.05)
        .input("phase", Signal::phase(440.0))
        .check(reference!("sine"));
}
```

## Behaviour

### Where it lives

The harness is its own crate, `loom-test`, used as a dev-dependency. *Why:* it reads and writes files, and `loom-core` must not (AGENTS.md hard rule 6). `loom_core::testing::run` stays for small unit tests.

### Setting up a render

| Call | Default | Meaning |
|---|---|---|
| `Harness::new(&DESCRIPTOR)` | | The module to test. It must be a DSP module; unit delays and audio outputs are run by the engine, not by `process`. |
| `.sample_rate(hz)` | 48 000 | Sample rate of the render and of the reference file. |
| `.seconds(s)` or `.frames(n)` | 0.1 s | Length of the render. |
| `.input(name, signal)` | not connected | Connects an input to a signal. `is_connected` is true for it. |
| `.param(name, value)` | descriptor default | Sets a parameter before the render, clamped as `Controller::set_param` clamps it. |
| `.midi(events)` | none | MIDI events. Each event's `frame` counts from the start of the render. |
| `.transport(settings)` | stopped, 120 BPM, 4/4 | The transport the module sees, starting at position 0. |
| `.param_at(seconds, name, value)` | none | Changes a parameter during the render. See [Changes during a render](#changes-during-a-render). |
| `.transport_at(seconds, settings)` | none | Changes the transport during the render. See [Changes during a render](#changes-during-a-render). |
| `.tolerance(t)` | 1e-5 | Largest allowed difference from the reference, per sample. |
| `.allow_undocumented()` | off | Lets a module with no label or description on a port or parameter render, for a quick experiment. |

An unknown input or parameter name fails the test at once, with the names the descriptor has.

Every render fails if a port or parameter has an empty label or description, and lists each one ([port and parameter text](port-and-parameter-text.md)). *Why:* module authors outside this repository get the same check that built-in modules get at compile time.

### Signals

| Signal | Meaning |
|---|---|
| `Signal::constant(v)` | The same value at every frame. |
| `Signal::samples(vec)` | These samples. Fewer than the render length fails the test. |
| `Signal::from_fn(f)` | `f(frame)` for each frame. |
| `Signal::phase(hz)` | A phase ramp from 0 up to 1 at `hz`, computed in `f64` from the frame number. |
| `Signal::gate(on, off)` | 1.0 from `on` seconds up to `off` seconds, 0.0 elsewhere. |
| `Signal::non_finite_busy()` | A 50 Hz wave timed for `check_non_finite_recovery`: it rises through 0.5 inside the bad frames. See [Blind spots in the shared checks](#blind-spots-in-the-shared-checks). |
| `Signal::extreme_busy()` | A 2 Hz wave timed for `check_extreme_recovery`. See [Huge finite inputs](#huge-finite-inputs). |

### Running it

| Call | What it does |
|---|---|
| `.render()` | Renders in blocks of 512 frames and returns every output, by name. |
| `.render_with_block(n)` | The same, in blocks of `n` frames. |
| `.check_block_sizes()` | Renders with block sizes 1, 77, and 512 and fails unless all three are bit-identical. |
| `.check_reference(path)` | Renders in blocks of 512 and compares with the reference file. |
| `.check(path)` | Both checks. This is what most tests call. |
| `.check_reset()` | Renders a new instance, calls `reset()`, renders again with the same inputs, MIDI, and transport, and fails unless the second render is bit-identical to the first. `reset()` must snap smoothing to its targets and clear the state `process` changes ([live patching](live-patching.md), rule 7). `crates/loom-modules/tests/reset.rs` runs it on every built-in DSP module for 1 s, with every parameter away from its default and a note held across the reset, on busy inputs (each input at its own frequency) and on port defaults, with the transport stopped and running. |
| `.check_copy_state(at)` | At block sizes 1, 77, and 512: runs a new instance for `at` frames, copies its state into a second new instance with `Module::copy_state_from`, runs both to the end, and fails unless they are bit-identical from frame `at`, and their display values match at the copy ([stereo](stereo.md), rules 25 and 26). A module with no state passes without copying. `crates/loom-modules/tests/copy_state.rs` runs it on every built-in DSP module, with busy inputs (each input at its own frequency), held notes, and every parameter away from its default, and on port defaults with the transport playing, copying on the bar wrap ([Blind spots in the shared checks](#blind-spots-in-the-shared-checks)). It also has focused tests for Clock and Transport on a playing transport, because a stopped transport skips their wrap detectors. Those copy on the frame a trigger fires and on the frame after, at a tempo with no whole-frame beats: a wrap detector looks back only one frame, so a copy in the middle of a cycle passes even if it loses the detector. |
| `.check_copy_state_repeatable(at)` | For a seeded module, which re-derives its random state when it copies (rule 27): two copies taken at frame `at` play the same, and the copy returns `true`. |
| `.check_non_finite_recovery()` | For each input in turn, and for each of NaN, infinity, and minus infinity, puts that value on the input for 8 frames from frame 1068, as a connected cable. Renders in blocks of 1 frame (as a feedback loop calls the module) and of 512. Fails unless every output is finite again 10 ms after the bad frames (480 frames at 48 kHz, scaled with the sample rate), up to the end of the render. Every other frame and input reads what the render reads without the check. |
| `.check_extreme_recovery()` | See [Huge finite inputs](#huge-finite-inputs). |
| `.check_extreme_recovery_on(input)`, `.check_extreme_stays_bounded_on(input)` | The check for one input, and the same without the comparison, for an input with a stated exception. |
| `.check_extreme_recovery_all()`, `.check_extreme_stays_bounded_all()` | The check with the huge value on every input at once, and the same without the comparison, for a module with a stated exception. See [All inputs at once](#all-inputs-at-once). |
| `.check_extreme_finite()`, `.check_extreme_finite_all()` | Only the finite part of the check, each input in turn and every input at once, for a module with its parameters turned up. See [All inputs at once](#all-inputs-at-once). |

`reference!("name")` gives `tests/reference/name.wav` inside the crate that calls it.

### Reference files

- A reference is a 32-bit float WAV file with one channel per output, in descriptor order, at the render's sample rate. *Why:* a reviewer can open it in any audio editor and listen to it.
- With `LOOM_UPDATE_REFERENCES=1` in the environment, `check_reference` writes the file instead of comparing, and passes. Run the tests once this way to create or update references, then listen to them and commit them.
- Without it, a missing file fails with a message that says how to create it.
- A file with a different sample rate, channel count, or length fails, and the message says what differs.
- When a sample differs by more than the tolerance, the test fails. The message names the output, the first frame that differs, the expected and actual values, and the largest difference.
- Keep references short, normally 0.25 s or less, so each file stays well under 100 KB.

### Test modules

`loom_modules::test_modules` holds small modules for tests only. It builds only with the `test-modules` feature of `loom-modules`, which is off by default. *Why not in `loom-test`:* the app ships them for `--test-modules`, and the harness should not ship. The built-in modules do not cover every panel widget or parameter mapping yet, so engine, app, and canvas tests share these instead of each declaring its own descriptor.

- `test.panel` has one of every widget kind (a knob for a continuous and a whole-number parameter, a toggle, radio buttons with three options, a button, a light, and a meter), every parameter mapping (linear, logarithmic, whole-number, and one smoothed parameter), and one unipolar and one bipolar output.
- `test.linear`, `test.log`, `test.steps`, and `test.smooth` each have one parameter and one bipolar output `out`, which writes what the engine gave that parameter. A test reads it to see the modulated value.
- `loom_modules::test_modules::register` adds them all to a registry, which checks their panels.
- Their IDs start with `test.`, so they never clash with a real module.
- The app registers them in its own tests, and in a normal run only with `--test-modules`. Only with that flag does its module browser list them. `examples/dev/test-widgets.ron` uses them: one of each widget, with an assignment on each control. Open it with `--test-modules`, `--dump-ui`, `--screenshot-window`, or the control channel to check a widget by hand. *Why:* some widgets are on no built-in module yet, so a manual check of them needs a patch, and a normal user should never see these modules.

### Example patch references

Golden-audio tests for whole patches, the other half of NFR-OSS-05 ([#93](https://github.com/brylie/LoomDSP/issues/93)), live in `crates/loom-modules/tests/patches.rs`. They run the engine, not one module, so they do not use `Harness`.

- Every `.ron` file in `examples/patches/` is loaded into an engine with output safety off and plays middle C from 5 ms to 150 ms. The render is 0.2 s, so each file stays under 80 KB. *Why safety off:* the reference should hold what the patch makes, not what the limiter does to it. The safety stage has its own tests.
- The render is played in host buffers of 1, 77, 256, and 1,000 frames, and all four must be bit-identical.
- The stereo output is compared with `tests/reference/patches/<name>.wav` by `loom_test::check_channels`, which follows the same file rules as `check_reference`, including `LOOM_UPDATE_REFERENCES=1`. The tolerance is 1e-4. *Why larger:* a patch runs several modules in a row, some in feedback loops, so platform maths differences can grow more than in one module.
- A new example patch without a reference fails the test. A reference without a patch fails too, so a removed or renamed patch leaves no stale file.

## Rules

1. **Each render uses a new module instance**, made by the descriptor's factory, with every parameter set to its default first and then to the test's values, and then `reset()` once, so smoothing starts at those values. *Why:* this is how the engine creates a module (`Controller::add_module`), and no render can leak state into the next.
2. **The module is created with `max_block` 512** for every render. *Why:* the largest block in the check is 512, and a module's output must not depend on `max_block`.
3. **Unconnected inputs read their port default**, as the engine does: a fixed value, bar phase, beat phase, or playing, computed from the same transport the module sees. *Why:* a module that follows the transport must be testable without a patch.
4. **Each block gets the transport, MIDI, and inputs for exactly its frames.** The transport for a block that starts at frame `k` is the transport in force at `k`, moved on by the frames since it took effect (`TransportInfo::later`). A render with no scheduled change has one transport for its whole length, anchored at frame 0. *Why:* the harness must split a render the way the engine does, or the block-size check would test the harness instead of the module.
5. **Block sizes must give bit-identical output.** *Why:* AGENTS.md hard rule 2. Block size 1 is how feedback loops call a module, which is the second entry point that ADR-0002 asks the harness to cover.
6. **The reference comparison allows a small tolerance.** *Why:* `sin`, `exp`, and `tan` come from each platform's maths library, so macOS, Linux, and Windows can differ in the last bits. Block sizes are compared on one machine, so they need no tolerance.
7. **Outputs must be finite.** A NaN or infinite sample fails every check, with the output and frame. *Why:* a NaN in audio is always a bug, and it would also make every comparison fail with a confusing message.
8. **A module must not keep a non-finite input in its state.** It may ignore the value, or recover from it within 10 ms. While the bad value is on the input, its output may be anything. *Why:* cables carry any `f32` (ADR-0007). A filter or delay that keeps one NaN stays broken after its input is sane again, until the patch is reloaded. The final output turns NaN into silence, so the patch would go quiet with no sign of why. The check renders for the length the test sets, so a module that holds samples, such as a delay, needs a render longer than its delay time. The check feeds bad values only on inputs, because parameters cannot carry them: the engine fills a modulated parameter through `ParamSpec::denormalize`, which turns NaN into the parameter's default and clamps infinity to its range (`crates/loom-core/tests/modulation_non_finite.rs`). `crates/loom-modules/tests/non_finite.rs` runs it on every built-in DSP module, with inputs on their defaults and on a busy signal, with the transport stopped and playing.

## Acceptance criteria

- [x] A module renders with given input signals and parameters for a given length at a given sample rate.
- [x] `check_reference` passes against a matching file, and fails when a sample differs by more than the tolerance, naming the output and frame.
- [x] With `LOOM_UPDATE_REFERENCES=1`, a missing or different reference is written, and the test passes.
- [x] A missing reference fails with a message that says how to create it. A reference with a different sample rate, channel count, or length fails.
- [x] `check_block_sizes` passes for a block-size invariant module, and fails for a module whose output depends on block size.
- [x] Unconnected inputs read their port default, including transport signals; connected inputs read their signal and report `is_connected`.
- [x] MIDI events reach the module in the block and frame they belong to, at every block size.
- [x] A non-finite output fails with the output and frame.
- [x] Every built-in DSP module has a reference test and a block-size check.
- [x] The module checklist in CONTRIBUTING.md points to the harness.
- [x] `check_non_finite_recovery` fails for a module that keeps a NaN in its state, naming the module, input, value, output, and frame, and passes for a module without state. It fails when the render is too short to show recovery.
- [x] Every built-in DSP module in `loom_modules::ALL` passes `check_non_finite_recovery`.
- [x] Every built-in DSP module in `loom_modules::ALL` passes `check_reset`.
- [x] Every built-in DSP module in `loom_modules::ALL` passes `check_extreme_recovery` on every input, except inputs with a stated reason, which pass `check_extreme_stays_bounded_on`.
- [x] `check_extreme_recovery` fails for a module that stores a huge input as it comes, for each of the two faults found in Phase Reset.
- [x] Every built-in DSP module in `loom_modules::ALL` passes `check_copy_state`, except seeded ones, which pass `check_copy_state_repeatable`. Copying allocates nothing (`no_alloc.rs`).
- [x] `check_non_finite_recovery` fails for a module that stores a bad input on another input's rising edge, on `Signal::non_finite_busy` (test).
- [x] Every built-in DSP module passes `check_non_finite_recovery` with the transport playing, on port defaults and on busy inputs.
- [x] Every built-in DSP module passes `check_copy_state` on port defaults with the transport playing, copied mid-cycle, on the bar wrap, and on the frame after it, except seeded ones.
- [x] Every built-in DSP module passes the all-input extreme check at its defaults and in each option of each choice parameter, except modules with a stated exception, which stay bounded.
- [x] Every built-in DSP module stays finite with its continuous parameters at their maximum, each input in turn and all inputs at once.
- [x] A parameter change and a transport change land on the same frame at block sizes 1, 77, and 512, for every built-in DSP module.
- [x] A scheduled time signature change is a jump for one call, and a tempo or running change is not (test).

## Huge finite inputs

**Status:** Implemented ([#552](https://github.com/brylie/LoomDSP/issues/552)).

Cables carry any `f32` and are never clipped ([ADR-0007](../adr/0007-polymorphic-signal-model.md)), so a feedback patch can send very large values into a module. `crates/loom-modules/tests/extreme.rs` runs this check on every built-in DSP module, on port defaults and on busy inputs, with the transport stopped and running.

- **What it sends.** For each input in turn: `f32::MAX`, `-f32::MAX`, and `f32::MAX` flipping to `-f32::MAX`, held for 960 frames from frame 2300. The flip is there because subtracting a new huge value from a stored one of the other sign overflows, and holding one sign never shows that.
- **Finite.** Every output is finite for the whole render.
- **Recovers.** After a settling time of 0.25 s, up to the next rising edge of the busy signal, each output matches a clean render within 0.1% of its peak. A phase output matches by its speed, modulo 1, not its value: a phase captured during the hold rightly has another offset, but it must keep moving at the input's rate. A check on size alone would pass a module whose output is wrong inside 0 to 1, as Phase Reset's was ([#531](https://github.com/brylie/LoomDSP/pull/531)).
- **Normal size.** After that edge, each output is at most 10 times the clean render's peak.
- **Why the busy signal is slow.** The test uses `Signal::extreme_busy`, a 2 Hz wave. Its first rising edge comes during the hold, so state captured on a trigger sees the huge value. Its next edge overwrites that state and hides the bug, so it must come after the comparison. At an audio rate the next edge comes within 2 ms, the settling time has not passed, and the check passes a module that keeps a bad value until then. Do not raise the frequency to speed up the test. A test in `loom-test` checks this timing.
- **Exceptions.** A table in `extreme.rs` names each input where a huge value rightly leaves a module different from a clean render, as module, input, and reason. Every other input of that module gets the full check, so a reason only excuses the input it is about. Each listed input still runs `.check_extreme_stays_bounded_on`, and the test fails if one no longer needs its exception.

## All inputs at once

**Status:** Implemented, M2 ([#575](https://github.com/brylie/LoomDSP/issues/575)).

The check for one input cannot see a bug that needs two huge inputs together. Two inputs at `f32::MAX` overflow when they are multiplied, or summed, even though each is finite.

- **What it sends.** The same three passes as [Huge finite inputs](#huge-finite-inputs), with the huge value on every input at once. Every input counts as connected, in the clean render and in the huge one.
- **Where it runs.** `crates/loom-modules/tests/extreme.rs` runs it on every built-in DSP module, on port defaults and on busy inputs, with the transport stopped and running. It runs at the module's defaults, and again for each option of each choice parameter, with the other parameters at their defaults. *Why each option:* a bug can live in one mode only. Selector's fade while every input is huge cannot happen in Step mode, because its first edge only clears the wait.
- **Exceptions.** A second table in `extreme.rs` names each case where every input being huge rightly leaves a module different from a clean render, with a reason. An entry names one option of a choice parameter, so the module's other options get the full check, or the whole module, if it needs the exception at its defaults. *Why:* a bug can live in one mode, and a mode can be named, though an input cannot in a run with every input huge. A listed case must still stay finite and return to a normal size, and a test fails if one no longer needs its exception. One narrower exception: on port defaults, Sample and Hold's trigger rises once and never again, so it rightly keeps the huge value, and needs only to stay finite there. On busy inputs its next trigger replaces the value, so it stays bounded.
- **Parameters turned up.** A gain above 1 overflows on one huge input. So each module also runs with its continuous parameters at their maximum, each input in turn and all inputs at once. Whole-number and choice parameters stay at their defaults, because a mute at its maximum would silence the input under test, and modes have their own runs. This run requires only that outputs stay finite. *Why not recovery:* a delay at full feedback rightly rings for a long time.
- **The overflow rule** ([ADR-0037](../adr/0037-finite-inputs-hold-at-the-largest-finite-value.md)). When finite inputs overflow in a module's own arithmetic, the result holds at the largest finite value of that sign. This is not clipping: normal values never reach it, and a non-finite input still passes through. *Why:* cables are never clipped (ADR-0007), and infinity on a cable is allowed to pass through a module. But finite values turning into infinity is the module's own doing, and infinity times a gain of 0 is NaN.
- **What this means for a runaway feedback loop.** A loop through these modules holds at the largest finite value and does not reach infinity. So it stays recoverable: when its gain falls, it settles again, where infinity would stay stuck until the patch reloads. The cost shows at the output. With output safety off, the clip at the end gives full-scale DC until someone breaks the loop, not silence. With output safety on, the limiter holds the burst at its ceiling, the DC blocker removes what stays, and the sound dies away. In every mode the stats bar says **Output clipped** ([output safety](output-safety.md), rule 7). A module that really makes a non-finite value still reaches the output as silence (ADR-0007).

## Blind spots in the shared checks

**Status:** Implemented, M2 ([#564](https://github.com/brylie/LoomDSP/issues/564)).

The checks over every module in `ALL` must reach the state they claim to test. These rules exist because a timing gap passes silently.

- **The bad frames cover a rising edge.** `check_non_finite_recovery` puts bad values on frames 1068 to 1075. `Signal::non_finite_busy` is a 50 Hz wave that rises through 0.5 at frame 1072, so a module that stores an input on a trigger on another input sees the bad value. Its next rise is at frame 2032, and the check requires finite output from frame 1556, so the module must recover before that edge overwrites the bad state. A test in `loom-test` checks this timing. Do not change the frequency without moving the frames.
- **Playing runs use port defaults.** Only Clock and Transport read the transport directly. Every other module follows it through a `BarPhase` port default, and a connected input replaces the default. So a playing run on busy inputs does not reach those modules, and `non_finite.rs` and `copy_state.rs` also run on port defaults.
- **A bar short enough to wrap.** The playing runs use 130 BPM in 1/4, so a bar is one beat, and the bar wraps at frame 22,154. *Why 130:* a beat at this tempo is not a whole number of frames, so the frame after a wrap has a phase above 0, and a lost wrap detector shows. *Why 1/4:* at 4/4 the first bar wrap is at frame 88,615, far past these renders.
- **Copies on the wrap.** The playing copy run copies at frame 2,500, on the wrap, and on the frame after it. A wrap detector looks back only one frame, so a copy in the middle of a cycle passes even if it loses the detector.
- **Each input has its own frequency.** `copy_state.rs` and `reset.rs` give each input a different busy frequency, with none a multiple of another. With one signal on every input, a routing module gives the same output whichever input it picks, and a wrong pick does not show. `non_finite.rs` keeps one signal on every input, because its edges must line up with the bad frames.

Known limits, so nobody assumes more than these runs show:

- `Signal::non_finite_busy` holds below 95,600 Hz. Its edges are fixed in frames, but the recovery time scales with the rate, so at 96 kHz the next edge overwrites a bad value before the check looks. The shared runs use 48 kHz. A test records this limit.
- The bad frames do not overlap the bar wrap, so a module that stores a bad value only on a wrap is not covered.
- Clock has no inputs, so `check_non_finite_recovery` has nothing to send it, and passes without testing anything.
- `changed()` puts Selector in Select mode, which never draws from its random stream. So the shared copy check does not cover that stream. A focused test in `copy_state.rs` does.
- A lost wrap detector does not show on Euclidean rhythm at more than one step, because the step changes on a wrap anyway. It shows on Phase Trigger, so the run can see this kind of bug.

## Changes during a render

**Status:** Implemented, M2 ([#96](https://github.com/brylie/LoomDSP/issues/96)).

Modules that smooth parameters, or follow tempo and play state, must sound the same at every block size when something changes during a render.

- **`.param_at(seconds, name, value)`** sets a parameter at that time. The value is clamped as `.param` clamps it, and an unknown name fails as `.param` does.
- **`.transport_at(seconds, settings)`** changes the transport at that time.
- **Times.** A time becomes a frame by rounding, as `.seconds` does. A time at or past the end of the render fails the test. Changes on the same frame apply in the order they were added.
- **Blocks split at a change.** A change applies at the start of the frame it names, and the harness ends a block there. So it lands on the same frame at block sizes 1, 77, and 512. *Why:* otherwise the check would compare different renders and test the harness instead of the module.
- **What a transport change does.** A scheduled transport change takes effect on its frame, and the position stays where it is. A tempo or running change is not a jump. A time signature change is a jump for one call ([clock](clock.md), rule 10). Setting the same value again changes nothing. A scheduled reset is out of scope. An unconnected input that follows the transport follows the same timeline.
- **Not with `check_reset` or `check_copy_state`.** They fail with a clear message on a harness with scheduled changes. A reset or a new copy cannot correctly replay a schedule that has already run. Add it when a module needs it.
- `crates/loom-modules/tests/live_changes.rs` moves each parameter of every built-in DSP module, in turn, to a value away from its default at a frame no block size lands on, and checks all block sizes. It also has focused tests for Clock, an LFO in sync mode, and Transport on tempo, running, and time signature changes.

## Reset returns a module to its start

**Status:** Implemented ([#108](https://github.com/brylie/LoomDSP/issues/108)).

- **The rule:** after `reset()`, a module renders bit-identical to a new instance given the current parameters and `reset()` once. That is how the Controller starts every module ([live patching](live-patching.md), rule 7). So nothing glides after a reset, and state such as MIDI Note's last note and velocity is cleared (its gate is low).
- **`Harness::check_reset()`** renders a module with the test's inputs, calls `reset()`, renders again, and compares the second render with the first render of a new instance. It fails unless both are bit-identical. It replaces an earlier check that held a module on silent inputs before the reset: the runs on port defaults cover the same ground.
- `crates/loom-modules/tests/reset.rs` runs it on every built-in module, on busy inputs and on port defaults, with the transport stopped and running. A note stays held across the reset, or MIDI Note's state would not be tested. A module that cannot pass lists its exception here, with a reason; there are none.
- *Why:* Panic ([panic](panic.md)) and a transport reset rely on `reset()`, and a module that keeps hidden state across it would play differently after a panic than after a load.
- [x] **Acceptance:** every built-in module passes `check_reset`, including MIDI Note and the seeded modules (test).

## Out of scope

- Modulating a parameter through a cable during a render. Use the engine tests in `loom-modules/tests/engine.rs`.
- A scheduled transport reset or jump to another position.
- Plugin host tests.

## Open points

- None yet.
