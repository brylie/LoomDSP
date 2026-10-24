# Audio QA Harness Specification

Tools that let tests, scripts, and QA agents check what the app actually plays, so safety checks on the final output do not depend on a person listening.

- **Kind:** living specification.
- **Status:** Parts 1 to 3: Implemented. Agreed and built in [#320](https://github.com/brylie/LoomDSP/pull/320). Part 4: Implemented. Agreed and built in the pull request that adds `loom capture`.
- **Decision behind it:** None. It builds on [output safety](output-safety.md) (ADR-0019), the [control channel](control-channel.md), and the `devices::Host` trait from the [preferences spec](preferences.md).
- **Use case and requirements:** Developer tooling. It protects UC3 (perform live), FR-PERF-08, NFR-SND-02, and NFR-SND-03 by testing them in the app, not only in the engine.
- **Background:** [Output safety explainer](../learn/output-safety.md).
- **Issues:** Part 1 [#318](https://github.com/brylie/LoomDSP/issues/318), Part 2 [#319](https://github.com/brylie/LoomDSP/issues/319), Part 3 [#317](https://github.com/brylie/LoomDSP/issues/317). Comes from [#200](https://github.com/brylie/LoomDSP/issues/200) (the listening checks for output safety). Related: [#130](https://github.com/brylie/LoomDSP/issues/130) (a meter or warning for the final output).
- **Code:** stepped output: `AudioOutput::stepped` and `Tape` in `crates/loom-host-desktop/src/stepped.rs`, offered by the fake host in `crates/loom-app/src/devices.rs`. App tests: `crates/loom-app/src/qa_tests.rs`. Recording: `Recorder` in `crates/loom-host-desktop/src/record.rs`, owned by `settings::Rig`. Measurements: `crates/loom-test/src/analysis.rs`. `loom analyze`: `crates/loom-cli/src/analyze.rs`, tests in `crates/loom-cli/tests/analyze.rs`. Capture: `Capture` in `crates/loom-host-desktop/src/capture.rs`, its loopback tests in `device_tests` in `crates/loom-host-desktop/src/lib.rs`, and `loom capture` in `crates/loom-cli/src/capture.rs`, tests in `crates/loom-cli/tests/capture.rs`.

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
| Stops a stream (Open, New, quit, device change) | Plays the old engine until it reports silence and then the usual extra silent buffers, or until the stop limit is reached: the stop limit of output safety rule 14 (100 ms, or 10 ms plus four blocks if that is longer) as frames at the stream's rate, rounded up to whole blocks. Then the stream ends. A stream cut at the limit ends with its last samples as they were. The tape is not filled with zeros, so the cut fails the click check of rule 4. |
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
| Not finite | NaN and infinite samples. The other measurements leave them out (see below). |
| Largest step | Largest difference between two neighbouring samples, with its time. A click shows up here. |
| DC | The mean of the whole file, and the largest mean of any 100 ms window. |
| RMS | The level, linear and in dBFS. |

Options:

| Option | Effect |
|---|---|
| `--from SECONDS`, `--to SECONDS` | Measure only this part of the file, for example after a fade-in. |
| `--json` | Print JSON instead of text, for scripts and agents. |
| `--max-peak X`, `--max-step X`, `--max-dc X` | Checks. When one fails, the command says which, and exits with status 1. |

When the file cannot be read, or an option is not usable, the command exits with status 2. Unusable options are a NaN or infinite time or limit, and `--from` after `--to`. *Why:* a script can then tell a failed check from a broken run. A NaN limit would also pass every check, because every comparison with NaN is false.

**NaN and infinite samples.** Peak, Over the ceiling, At full scale, DC, and RMS leave them out and measure the finite samples. Largest step compares only two finite neighbours, so a step next to a broken sample is not counted. A DC window counts a broken sample as 0. Not finite counts them. Each check given (`--max-peak`, `--max-step`, `--max-dc`) fails for a channel with any such sample, whatever the other measurements say. With no checks, the file is only measured and the command exits with status 0. *Why:* the same rule as the module test harness ([rule 7](module-test-harness.md#rules)): a NaN in audio is always a bug, and broken audio must not pass a check because it was left out of the arithmetic.

**JSON.** `--json` prints every measurement as a number. JSON has no infinity, so a level of silence in dBFS (negative infinity) prints as `null`. No other measurement can be infinite, because broken samples are left out.

The measurements are functions in `loom-test`, so the app tests of Part 1 and the command of Part 3 measure in the same way.

### Part 4: capture what reached a device

*Why:* `--record` saves what the engine rendered, before the device. It cannot see what happens after: a stream stopped before the device played its last buffers, a device that goes silent after a rate change, a gap at a device change. In the manual check for #26, Open with 4096-frame buffers clicked because a fixed 100 ms stop limit cut the stream before the fade reached the device. The `--record` file was clean, and only ears caught it.

A loopback device closes that gap. It is a virtual device whose output is also an input: what an app plays into it can be recorded from it, sample for sample.

| System | Loopback device |
|---|---|
| macOS | BlackHole (free, MIT licence), or any aggregate device that loops back. |
| Linux | A PipeWire or PulseAudio null sink, captured from its monitor; or ALSA `snd-aloop`. |
| Windows | Not covered yet. WASAPI loopback capture of an output is a different API in `cpal`; add it when a check needs it. |

A new `loom` subcommand records an input device to a WAV file:

```sh
loom capture --device <part of name> -o out.wav --seconds N
```

| Option | Effect |
|---|---|
| `--device NAME` | Case-insensitive part of the input device name, as for `loom play`. Required. *Why:* the default input is usually a microphone, which is never what a QA run wants. |
| `-o FILE`, `--output FILE` | The WAV file to write: 32-bit float, stereo, at the device's rate. |
| `--seconds N` | How long to record. Required. |
| `--sample-rate HZ` | Ask the device for this rate. The device default otherwise. |

The file holds the device's first two channels. A mono device is written to both. When the stream is running, `loom capture` prints one line to stderr, `Capturing "<device>" at <rate> Hz`, so a script can wait for it before it starts the app. `loom devices` also lists audio inputs, so the name can be found.

A QA run starts the capture, plays the app into the loopback output, and checks the file with `loom analyze`:

```sh
loom capture --device blackhole -o /tmp/device.wav --seconds 6 2> /tmp/capture.log &
until grep -q Capturing /tmp/capture.log; do sleep 0.1; done
loom-app examples/dev/check-dc.ron --output-safety off --device blackhole --buffer 4096 --control-script open-new-quit.jsonl
wait
loom analyze /tmp/device.wav --max-step 0.01
```

**Exit status.** 0 when the file was written with every frame. 2 when the device cannot be opened, the file cannot be written, frames were dropped, or the input stream reported an error (such as the device going away). *Why:* a dropped frame is a jump in the file that the device never played, so a step check on it would fail for the wrong reason. A script can tell a broken run (2) from a failed check in `loom analyze` (1).

**Relation to audio input (#29).** Capture is a development tool, not audio input for patches. It opens a `cpal` input stream in `loom-host-desktop` and writes it to a file through the same ring and writer thread as `--record` (`Recorder`). Nothing reaches an engine, and there is no Audio In module. When #29 is built, it can reuse the device lookup and the input stream setup, but it has its own spec: latency, channel routing, and running input and output on two clocks are design questions for patches, not for this tool.

**Clock drift.** A loopback device has one clock for both sides, so capture and playback cannot drift apart. When the input and output are different devices (an interface's output wired to another device's input), their clocks drift a little. Capture never resamples (rule 10). The checks of `loom analyze` (peak, step, DC) do not compare the capture with anything sample by sample, so drift does not change them. For the same reason, a capture is never compared sample by sample with a `--record` file or a reference render: the device adds latency, and the start time depends on the machine.

**CI.** The Loopback capture job runs the stop that must fade out (`a_stop_with_large_buffers_fades_out_on_the_device`) on an Ubuntu runner. It starts PipeWire, loads a null sink (`pactl load-module module-null-sink`), makes it the default sink and its monitor the default source, and sets `LOOM_LOOPBACK=pipewire`, so `cpal` plays and captures through the PipeWire ALSA plugin. It passed five runs out of five. The test waits for the capture's first frames before it plays, because some inputs take seconds to start. Three limits, found on the runner in October 2026:

- **The cut check does not run in CI.** On PipeWire, its partner test (`a_fixed_100_ms_stop_limit_shows_as_a_cut`) fails: the ALSA plugin plays out what it was handed after a stream closes, so a stop never cuts there. The job can still catch a missing fade, a dropout, or a step inside the stream, but not a stop cut too early. That check still needs a loopback that cuts, such as BlackHole on macOS.
- **PulseAudio is not used, because LoomDSP clicks on it.** With PulseAudio and its ALSA plugin, the capture took about 2 s to start, and two real faults showed up. At the start, the first 9 ms of the 10 ms fade-in never reached the sink: the capture jumps from 0 to 0.45. At the stop, the queue in the sound server holds more than the 2 silent buffers the stop waits for (`SILENT_BUFFERS_BEFORE_STOP`), so the fade was cut at 0.32. An extra 500 ms wait before the stop let the fade play out. Fixing this (waiting for the device's real latency) is a follow-up issue. Once it is fixed, both tests can run on a PulseAudio null sink in CI, where the cut check can fail.
- **No other systems.** macOS runners have no loopback device, and Windows capture is not covered yet (see the table above).

## Rules

1. **Time in the stepped output is counted in frames, never in wall-clock time.** A stop that waits for silence waits a number of frames. *Why:* the same test then gives the same tape on a fast laptop and a busy CI machine, and it needs no sleeps.
2. **The stepped output runs the app's real code paths.** Tests reach it through `App::new` and `App::update`, with the same `Args`, preferences file, and messages the window sends. *Why:* the gap we want to close is the app's wiring. A test that builds its own engine would miss it.
3. **The same test gives a bit-identical tape.** *Why:* determinism is a hard rule for the engine. It lets a failing tape be compared with a passing one, and saved as a reference when that helps.
4. **Click checks are relative to the patch.** At a switch, the old stream ends at 0 and the new stream starts at 0. No step in the fade-out is larger than the largest step in the 100 ms before the switch, and no step in the fade-in is larger than the largest step in the 100 ms after it. Each limit has a margin of one fade step times the peak of that 100 ms, because the fade gain moves as well as the signal. *Why:* a fixed limit would fail on bright patches and pass clicks on quiet ones. The fade-in plays the new patch, so it is compared with the new patch: a sine followed by an FM patch must not fail because the FM patch is brighter.
5. **The recording is what was sent to the output, nothing more.** No silence is added between streams, and the file does not mark where a stream starts. *Why:* the gap depends on the machine and the device, and adding it would make files differ from run to run. Rule 4 is checked on the tape of Part 1, which marks each stream's start. A recording gives a coarser check: `loom analyze` reports the largest step in the whole file, or between `--from` and `--to`, so a click at any switch shows up there. It does not find switches or apply rule 4 to each one.
6. **A recording keeps one sample rate.** If a new stream runs at a different rate (a device change), the recording ends, and the status bar and stderr say why. *Why:* a WAV file has one sample rate. Resampling would change the very samples we want to check.
7. **Recording never waits on the audio thread.** The existing writer thread and its dropped-frame count stay. *Why:* real-time safety (hard rule 1).
8. **`loom analyze` only measures.** It does not judge whether a sound is good. Checks pass or fail only on the limits given. *Why:* the same rule as `sample` on the control channel: tools measure, and a person or the test decides what is right for a patch.
9. **Measurements are plain arithmetic on the samples.** Peak is not oversampled, and DC is a plain mean. *Why:* the numbers must be easy to check by hand, and must match what the engine's output safety stage limits (it also works on samples, not true peaks).
10. **Capture writes what the device delivered, nothing more** (Part 4). It does not resample, align, trim, or fill gaps. Dropped frames and input stream errors are counted and reported, and make the run fail with status 2. *Why:* the point is to see what the device really did. Any repair would hide the very fault we look for.
11. **Capture never waits on the audio thread** (Part 4). Its input callback only pushes into a ring, as `--record` does. *Why:* real-time safety (hard rule 1), and a slow callback could itself drop input.

## Acceptance criteria

### Part 1

Each is a `loom-app` test that uses the stepped output, through `App::new` and `App::update`.

- [x] **Mode reaches the engine.** Given full-scale noise at gain 4, when the mode comes from `--output-safety`, and again from the preferences file, then Low latency and Clean never go over the ceiling, and Off has samples at full scale.
- [x] **DC through the app.** Given a Constant of 1.0 into Audio Out, then after 0.5 s every sample is under 0.001 in size in Low latency and Clean, and Off holds 1.0.
- [x] **No click on Open.** Given `sine.ron` with a held note, when `fm-feedback.ron` is opened and a note held, and then `sine.ron` is opened again, then each switch follows rule 4.
- [x] **No click on New, quit, or a device change.** The same check for each.
- [x] **No click on a mode change** from the preferences window: the output dips to 0 and comes back, with no step larger than rule 4 allows.
- [x] **Determinism.** Running the Open test twice gives bit-identical tapes.

### Part 2

- [x] Given `--fake-audio --record FILE`, when a script opens two more patches and quits, then the file holds all three streams in order, and the app exits with status 0.
- [x] A change of device that changes the sample rate ends the recording, with a message in the status bar and on stderr.
- [x] A device at another sample rate that fails to start does not end the recording: the old device plays again, and its stream goes into the same file.
- [x] The `--record` help text says the recording lasts until the app quits.

### Part 3

- [x] Each measurement is a unit test in `loom-test` on small hand-made signals: a known peak, one step, a DC offset, a NaN.
- [x] `loom analyze` reads 16-bit, 24-bit, and 32-bit float files.
- [x] `--json` prints every measurement, and a test parses it.
- [x] A failed `--max-peak`, `--max-step`, or `--max-dc` check exits with status 1 and names the check. A passing one exits with status 0.
- [x] A NaN sample fails every check given; with no checks, the file is only measured.
- [x] A NaN or infinite time or limit, and `--from` after `--to`, exit with status 2.

### Part 4

- [x] `loom capture` writes a 32-bit float stereo WAV at the device's rate, with the device's first two channels, or a mono channel on both.
- [x] The writer is tested with a fake input in CI: frames pushed are written in order, and dropped frames are counted.
- [x] A device that does not match exits with status 2 and names what it looked for. `loom devices` lists audio inputs.
- [x] **Hardware test, ignored by default.** Given a Constant of 0.5 into Audio Out with output safety Off, playing into a loopback device (`LOOM_LOOPBACK`, for example `BlackHole`) with 4096-frame buffers, when the engine fades out and the stream stops, then the capture of the loopback holds the DC and the fade, and no step is larger than a 10 ms fade from 0.5 allows. A second test stops the same stream at a fixed 100 ms limit, as before the #26 fix, and checks that the capture does show the cut. *Why:* a check that cannot fail proves nothing.
- [x] CI runs the first hardware test on a PipeWire null sink on Linux.
- [x] The manual check for #26 (`docs/dev/manual-checks/issue-26-devices.md`) uses capture where it now relies on ears for clicks at a stop or a device change.

### Manual

- [x] Run the Part 2 script and `loom analyze` on its file, and list the result in the pull request.

## Out of scope

- **What happens past the app's output.** Thuds from a device powering on or off, and a speaker cone held by DC, depend on the hardware. They stay on a short manual checklist before each release.
- **How Clean mode feels to play.** That needs a person.
- **A meter or warning for the final output in the app.** That is [#130](https://github.com/brylie/LoomDSP/issues/130).
- **True-peak (oversampled) measurement, loudness (LUFS), and spectrum analysis.** Add them when a check needs them.
- **Starting and stopping a recording from the control channel.** A run records from start to quit.
- **Audio input for patches** ([#29](https://github.com/brylie/LoomDSP/issues/29)). Capture only writes a file (Part 4).
- **Windows loopback capture, the cut check in CI, and the PulseAudio faults found by capture.** Follow-up issues (Part 4, CI).
- **Finding silences.** A device that goes silent shows up today as a short file peak, an RMS near zero, or a peak of 0 in a `--from`/`--to` window. A measurement of the longest silent stretch, with a check, can be added to `loom analyze` when a check needs it.

## Open points

- None. *Settled:* the stepped output is `AudioOutput::stepped(sample_rate, block, &tape)`, next to `AudioOutput::clock`. The tape marks where each stream starts; the WAV file does not (rule 5). The windows of rule 4 are 100 ms, long enough to hold several cycles of a low note.
