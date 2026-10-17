# Module Test Harness Specification

A test harness for module authors: render one module with given inputs and parameters, check it against a stored reference render, and check that block size does not change its output.

- **Kind:** living specification.
- **Status:** Implemented, P1
- **Decision behind it:** [ADR-0002](../adr/0002-graph-execution-and-feedback.md) ("The SDK harness must cover this").
- **Use case and requirements:** FR-SDK-02, NFR-OSS-05.
- **Issues:** [#42](https://github.com/brylie/LoomDSP/issues/42)
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

`reference!("name")` gives `tests/reference/name.wav` inside the crate that calls it.

### Reference files

- A reference is a 32-bit float WAV file with one channel per output, in descriptor order, at the render's sample rate. *Why:* a reviewer can open it in any audio editor and listen to it.
- With `LOOM_UPDATE_REFERENCES=1` in the environment, `check_reference` writes the file instead of comparing, and passes. Run the tests once this way to create or update references, then listen to them and commit them.
- Without it, a missing file fails with a message that says how to create it.
- A file with a different sample rate, channel count, or length fails, and the message says what differs.
- When a sample differs by more than the tolerance, the test fails. The message names the output, the first frame that differs, the expected and actual values, and the largest difference.
- Keep references short, normally 0.25 s or less, so each file stays well under 100 KB.

## Rules

1. **Each render uses a new module instance**, made by the descriptor's factory, with every parameter set to its default first and then to the test's values. *Why:* this is how the engine creates a module (`Controller::add_module`), and no render can leak state into the next.
2. **The module is created with `max_block` 512** for every render. *Why:* the largest block in the check is 512, and a module's output must not depend on `max_block`.
3. **Unconnected inputs read their port default**, as the engine does: a fixed value, bar phase, beat phase, or playing, computed from the same transport the module sees. *Why:* a module that follows the transport must be testable without a patch.
4. **Each block gets the transport, MIDI, and inputs for exactly its frames.** The transport for a block that starts at frame `k` is the render's transport moved on by `k` frames (`TransportInfo::later`). *Why:* the harness must split a render the way the engine does, or the block-size check would test the harness instead of the module.
5. **Block sizes must give bit-identical output.** *Why:* AGENTS.md hard rule 2. Block size 1 is how feedback loops call a module, which is the second entry point that ADR-0002 asks the harness to cover.
6. **The reference comparison allows a small tolerance.** *Why:* `sin`, `exp`, and `tan` come from each platform's maths library, so macOS, Linux, and Windows can differ in the last bits. Block sizes are compared on one machine, so they need no tolerance.
7. **Outputs must be finite.** A NaN or infinite sample fails every check, with the output and frame. *Why:* a NaN in audio is always a bug, and it would also make every comparison fail with a confusing message.

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

## Out of scope

- Changing parameters, tempo, or the play state during a render. Use the engine tests in `loom-modules/tests/engine.rs`, or add it here when a module needs it.
- Golden-audio tests for whole example patches (the other half of NFR-OSS-05). They run the engine, not one module.
- Testing `reset`.
- Plugin host tests.

## Open points

- None yet.
