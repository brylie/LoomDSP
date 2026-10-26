# Module Test Harness Specification

A test harness for module authors: render one module with given inputs and parameters, check it against a stored reference render, and check that block size does not change its output.

- **Kind:** living specification.
- **Status:** Implemented, P1
- **Decision behind it:** [ADR-0002](../adr/0002-graph-execution-and-feedback.md) ("The SDK harness must cover this").
- **Use case and requirements:** FR-SDK-02, NFR-OSS-05.
- **Issues:** [#42](https://github.com/brylie/LoomDSP/issues/42), [#208](https://github.com/brylie/LoomDSP/issues/208) (test modules), [#127](https://github.com/brylie/LoomDSP/issues/127) (non-finite inputs)
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

### Running it

| Call | What it does |
|---|---|
| `.render()` | Renders in blocks of 512 frames and returns every output, by name. |
| `.render_with_block(n)` | The same, in blocks of `n` frames. |
| `.check_block_sizes()` | Renders with block sizes 1, 77, and 512 and fails unless all three are bit-identical. |
| `.check_reference(path)` | Renders in blocks of 512 and compares with the reference file. |
| `.check(path)` | Both checks. This is what most tests call. |
| `.check_non_finite_recovery()` | For each input in turn, and for each of NaN, infinity, and minus infinity, puts that value on the input for 8 frames from frame 1000, as a connected cable. Renders in blocks of 1 frame (as a feedback loop calls the module) and of 512. Fails unless every output is finite again 10 ms after the bad frames (480 frames at 48 kHz, scaled with the sample rate), up to the end of the render. Every other frame and input reads what the render reads without the check. |

`reference!("name")` gives `tests/reference/name.wav` inside the crate that calls it.

### Reference files

- A reference is a 32-bit float WAV file with one channel per output, in descriptor order, at the render's sample rate. *Why:* a reviewer can open it in any audio editor and listen to it.
- With `LOOM_UPDATE_REFERENCES=1` in the environment, `check_reference` writes the file instead of comparing, and passes. Run the tests once this way to create or update references, then listen to them and commit them.
- Without it, a missing file fails with a message that says how to create it.
- A file with a different sample rate, channel count, or length fails, and the message says what differs.
- When a sample differs by more than the tolerance, the test fails. The message names the output, the first frame that differs, the expected and actual values, and the largest difference.
- Keep references short, normally 0.25 s or less, so each file stays well under 100 KB.

### Test modules

`loom_test::modules` holds small modules for tests only. The built-in modules do not cover every panel widget or parameter mapping yet, so engine, app, and canvas tests share these instead of each declaring its own descriptor.

- `test.panel` has one of every widget kind (a knob for a continuous and a whole-number parameter, a toggle, radio buttons with three options, a button, a light, and a meter), every parameter mapping (linear, logarithmic, whole-number, and one smoothed parameter), and one unipolar and one bipolar output.
- `test.linear`, `test.log`, `test.steps`, and `test.smooth` each have one parameter and one bipolar output `out`, which writes what the engine gave that parameter. A test reads it to see the modulated value.
- `loom_test::modules::register` adds them all to a registry, which checks their panels.
- Their IDs start with `test.`, so they never clash with a real module.
- The app registers them in its own tests, and in a normal run only with `--test-modules`. Only with that flag does its module browser list them. `examples/dev/test-widgets.ron` uses them: one of each widget, with an assignment on each control. Open it with `--test-modules`, `--dump-ui`, `--screenshot-window`, or the control channel to check a widget by hand. *Why:* some widgets are on no built-in module yet, so a manual check of them needs a patch, and a normal user should never see these modules.

### Example patch references

Golden-audio tests for whole patches, the other half of NFR-OSS-05 ([#93](https://github.com/brylie/LoomDSP/issues/93)), live in `crates/loom-modules/tests/patches.rs`. They run the engine, not one module, so they do not use `Harness`.

- Every `.ron` file in `examples/patches/` is loaded into an engine with output safety off and plays middle C from 5 ms to 150 ms. The render is 0.2 s, so each file stays under 80 KB. *Why safety off:* the reference should hold what the patch makes, not what the limiter does to it. The safety stage has its own tests.
- The render is played in host buffers of 1, 77, 256, and 1,000 frames, and all four must be bit-identical.
- The stereo output is compared with `tests/reference/patches/<name>.wav` by `loom_test::check_channels`, which follows the same file rules as `check_reference`, including `LOOM_UPDATE_REFERENCES=1`. The tolerance is 1e-4. *Why larger:* a patch runs several modules in a row, some in feedback loops, so platform maths differences can grow more than in one module.
- A new example patch without a reference fails the test. A reference without a patch fails too, so a removed or renamed patch leaves no stale file.

## Rules

1. **Each render uses a new module instance**, made by the descriptor's factory, with every parameter set to its default first and then to the test's values. *Why:* this is how the engine creates a module (`Controller::add_module`), and no render can leak state into the next.
2. **The module is created with `max_block` 512** for every render. *Why:* the largest block in the check is 512, and a module's output must not depend on `max_block`.
3. **Unconnected inputs read their port default**, as the engine does: a fixed value, bar phase, beat phase, or playing, computed from the same transport the module sees. *Why:* a module that follows the transport must be testable without a patch.
4. **Each block gets the transport, MIDI, and inputs for exactly its frames.** The transport for a block that starts at frame `k` is the render's transport moved on by `k` frames (`TransportInfo::later`). *Why:* the harness must split a render the way the engine does, or the block-size check would test the harness instead of the module.
5. **Block sizes must give bit-identical output.** *Why:* AGENTS.md hard rule 2. Block size 1 is how feedback loops call a module, which is the second entry point that ADR-0002 asks the harness to cover.
6. **The reference comparison allows a small tolerance.** *Why:* `sin`, `exp`, and `tan` come from each platform's maths library, so macOS, Linux, and Windows can differ in the last bits. Block sizes are compared on one machine, so they need no tolerance.
7. **Outputs must be finite.** A NaN or infinite sample fails every check, with the output and frame. *Why:* a NaN in audio is always a bug, and it would also make every comparison fail with a confusing message.
8. **A module must not keep a non-finite input in its state.** It may ignore the value, or recover from it within 10 ms. While the bad value is on the input, its output may be anything. *Why:* cables carry any `f32` (ADR-0007). A filter or delay that keeps one NaN stays broken after its input is sane again, until the patch is reloaded. The final output turns NaN into silence, so the patch would go quiet with no sign of why. The check renders for the length the test sets, so a module that holds samples, such as a delay, needs a render longer than its delay time. The check feeds bad values only on inputs, because parameters cannot carry them: the engine fills a modulated parameter through `ParamSpec::denormalize`, which turns NaN into the parameter's default and clamps infinity to its range (`crates/loom-core/tests/modulation_non_finite.rs`). `crates/loom-modules/tests/non_finite.rs` runs it on every built-in DSP module, with inputs on their defaults and on a busy signal.

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

## Out of scope

- Changing parameters, tempo, or the play state during a render. Use the engine tests in `loom-modules/tests/engine.rs`, or add it here when a module needs it.
- Testing `reset`.
- Plugin host tests.

## Open points

- None yet.
