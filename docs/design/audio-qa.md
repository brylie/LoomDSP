# Audio QA Harness Specification

Tools that let tests, scripts, and QA agents check what the app actually plays, so safety checks on the final output do not depend on a person listening.

- **Kind:** living specification.
- **Status:** Draft. Parts 1 to 3 are proposed together and can be built in any order. Part 3 has no dependency on the others.
- **Decision behind it:** None. It builds on [output safety](output-safety.md) (ADR-0019), the [control channel](control-channel.md), and the `devices::Host` trait from the [preferences spec](preferences.md).
- **Use case and requirements:** Developer tooling. It protects UC3 (perform live), FR-PERF-08, NFR-SND-02, and NFR-SND-03 by testing them in the app, not only in the engine.
- **Background:** [Output safety explainer](../learn/output-safety.md).
- **Issues:** Part 1 [#318](https://github.com/brylie/LoomDSP/issues/318), Part 2 [#319](https://github.com/brylie/LoomDSP/issues/319), Part 3 [#317](https://github.com/brylie/LoomDSP/issues/317). Comes from [#200](https://github.com/brylie/LoomDSP/issues/200) (the listening checks for output safety). Related: [#130](https://github.com/brylie/LoomDSP/issues/130) (a meter or warning for the final output).
- **Code:** not built yet. Planned: a stepped test output in `crates/loom-host-desktop` and `crates/loom-app/src/devices.rs`; the recording in `crates/loom-app/src/settings.rs`; measurements in `crates/loom-test`; `loom analyze` in `crates/loom-cli`.

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

"Output safety is the most important promise the app makes on stage. I want every pull request to prove it still holds in the app, not only in the engine. A QA agent should be able to run a patch, open another one, and get numbers back: how loud, any clipping, any click, any DC."

Today the engine tests cover every output safety rule (see the [acceptance criteria](output-safety.md#acceptance-criteria)). What they cannot see is the app around the engine:

| Gap | Why it is not tested today |
|---|---|
| The `--output-safety` flag and the saved preference reach the engine. | Engine tests build the engine directly. |
| Opening a patch, New, quit, and a device change give no click. The old engine fades out, the stream stops, and a new engine fades in. | `App::process` throws its output away. The `--fake-audio` clock runs on a timer, so a test can only sleep and hope. `--record` stops at the first Open. |
| Peak, clipping, steps, and DC of what the app played. | Only by hand, with a recording and a script outside the project. |

## Behaviour

### Part 1: a stepped test output

A fake audio output for Rust tests in `loom-app`, offered through the `devices::Host` fake. Nothing plays it by itself. The test calls `step(frames)`, and the output plays the current engine for that many frames, in blocks of the device's buffer size. It keeps every sample it plays, on a tape the test can read.

| What the app does | What the stepped output does |
|---|---|
| Starts a stream (a new engine) | The next `step` plays the new engine. |
| Stops a stream (Open, New, quit, device change) | Plays the old engine until it reports silence and then the usual extra silent buffers, or until the stop limit is reached, counted in frames. Then the stream ends. |
| Changes the output safety mode | Nothing special. The next `step` plays the dip and the new mode. |

The tape is one list of stereo samples for the whole test, across every stream, in the order they were played. Each stream's start is marked on the tape by its frame index, so a test can look at the frames around a switch.

### Part 2: `--record` covers the whole run

Today `--record out.wav` saves one stream and stops when another patch is opened. With this change the recording belongs to the app, not to one stream. It keeps going across Open, New, a device change, and a mode change, and it ends when the app quits.

The file holds what the app sent to the output, stream after stream. It does not add the silence between streams, because the length of that gap depends on the machine.

A QA script on the control channel can then check a patch switch in the real app:

```text
{"cmd":"press","button":"hold"}
{"cmd":"wait","ms":1000}
{"cmd":"open","path":"examples/patches/fm-feedback.ron"}
{"cmd":"press","button":"hold"}
{"cmd":"wait","ms":1000}
{"cmd":"open","path":"examples/patches/sine.ron"}
{"cmd":"wait","ms":500}
{"cmd":"quit"}
```

```sh
loom-app examples/patches/sine.ron --fake-audio --record /tmp/switch.wav --control-script switch.jsonl
loom analyze /tmp/switch.wav
```

### Part 3: `loom analyze`

A `loom` subcommand that reads a WAV file (16-bit, 24-bit, or 32-bit float) and prints measurements for each channel:

| Measurement | Meaning |
|---|---|
| Length | In frames and seconds, with the sample rate. |
| First sample | So a fade-in from 0 can be seen. |
| Peak | Largest absolute sample, linear and in dBFS, with its time. |
| Over the ceiling | Samples larger in size than the output safety ceiling (-1 dBFS). |
| At full scale | Samples of size 1.0 or more, which is what a hard clip leaves. |
| Not finite | NaN and infinite samples. |
| Largest step | Largest difference between two neighbouring samples, with its time. A click shows up here. |
| DC | The mean of the whole file, and the largest mean of any 100 ms window. |
| RMS | The level, linear and in dBFS. |

Options:

| Option | Effect |
|---|---|
| `--from SECONDS`, `--to SECONDS` | Measure only this part of the file, for example after a fade-in. |
| `--json` | Print JSON instead of text, for scripts and agents. |
| `--max-peak X`, `--max-step X`, `--max-dc X` | Checks. When one fails, the command says which, and exits with status 1. |

The measurements are functions in `loom-test`, so the app tests of Part 1 and the command of Part 3 measure in the same way.

## Rules

1. **Time in the stepped output is counted in frames, never in wall-clock time.** A stop that waits for silence waits a number of frames. *Why:* the same test then gives the same tape on a fast laptop and a busy CI machine, and it needs no sleeps.
2. **The stepped output runs the app's real code paths.** Tests reach it through `App::new` and `App::update`, with the same `Args`, preferences file, and messages the window sends. *Why:* the gap we want to close is the app's wiring. A test that builds its own engine would miss it.
3. **The same test gives a bit-identical tape.** *Why:* determinism is a hard rule for the engine. It lets a failing tape be compared with a passing one, and saved as a reference when that helps.
4. **Click checks are relative to the patch.** At a switch, the old stream ends at 0, the new stream starts at 0, and no step in the switch is larger than the largest step in the steady audio just before it. *Why:* a fixed limit would fail on bright patches and pass clicks on quiet ones.
5. **The recording is what was sent to the output, nothing more.** No silence is added between streams. *Why:* the gap depends on the machine and the device, and adding it would make files differ from run to run.
6. **A recording keeps one sample rate.** If a new stream runs at a different rate (a device change), the recording ends, and the status bar and stderr say why. *Why:* a WAV file has one sample rate. Resampling would change the very samples we want to check.
7. **Recording never waits on the audio thread.** The existing writer thread and its dropped-frame count stay. *Why:* real-time safety (hard rule 1).
8. **`loom analyze` only measures.** It does not judge whether a sound is good. Checks pass or fail only on the limits given. *Why:* the same rule as `sample` on the control channel: tools measure, and a person or the test decides what is right for a patch.
9. **Measurements are plain arithmetic on the samples.** Peak is not oversampled, and DC is a plain mean. *Why:* the numbers must be easy to check by hand, and must match what the engine's output safety stage limits (it also works on samples, not true peaks).

## Acceptance criteria

### Part 1

Each is a `loom-app` test that uses the stepped output, through `App::new` and `App::update`.

- [ ] **Mode reaches the engine.** Given full-scale noise at gain 4, when the mode comes from `--output-safety`, and again from the preferences file, then Low latency and Clean never go over the ceiling, and Off has samples at full scale.
- [ ] **DC through the app.** Given a Constant of 1.0 into Audio Out, then after 0.5 s every sample is under 0.001 in size in Low latency and Clean, and Off holds 1.0.
- [ ] **No click on Open.** Given `sine.ron` with a held note, when `fm-feedback.ron` is opened and a note held, and then `sine.ron` is opened again, then each switch follows rule 4.
- [ ] **No click on New, quit, or a device change.** The same check for each.
- [ ] **No click on a mode change** from the preferences window: the output dips to 0 and comes back, with no step larger than rule 4 allows.
- [ ] **Determinism.** Running the Open test twice gives bit-identical tapes.

### Part 2

- [ ] Given `--fake-audio --record FILE`, when a script opens two more patches and quits, then the file holds all three streams in order, and the app exits with status 0.
- [ ] A change of device that changes the sample rate ends the recording, with a message in the status bar and on stderr.
- [ ] The `--record` help text says the recording lasts until the app quits.

### Part 3

- [ ] Each measurement is a unit test in `loom-test` on small hand-made signals: a known peak, one step, a DC offset, a NaN.
- [ ] `loom analyze` reads 16-bit, 24-bit, and 32-bit float files.
- [ ] `--json` prints every measurement, and a test parses it.
- [ ] A failed `--max-peak`, `--max-step`, or `--max-dc` check exits with status 1 and names the check. A passing one exits with status 0.

### Manual

- [ ] Run the Part 2 script and `loom analyze` on its file, and list the result in the pull request.

## Out of scope

- **What happens past the app's output.** Thuds from a device powering on or off, and a speaker cone held by DC, depend on the hardware. They stay on a short manual checklist before each release.
- **How Clean mode feels to play.** That needs a person.
- **A meter or warning for the final output in the app.** That is [#130](https://github.com/brylie/LoomDSP/issues/130).
- **True-peak (oversampled) measurement, loudness (LUFS), and spectrum analysis.** Add them when a check needs them.
- **Starting and stopping a recording from the control channel.** A run records from start to quit.

## Open points

- **Name of the stepped output.** Leaning: `AudioOutput::stepped(sample_rate, block)` next to `AudioOutput::clock`.
- **Should the tape or the recording mark where each stream starts?** Leaning: the tape does (Part 1). The WAV file does not, because `loom analyze` finds the switch from the steps.
- **Is "the steady audio just before" in rule 4 the last 100 ms?** Leaning: yes, long enough to hold several cycles of a low note.
