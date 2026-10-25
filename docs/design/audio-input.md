# Audio Input Specification

An Audio In module that brings sound from an interface into the patch, so LoomDSP can be a live effect.

- **Kind:** living specification.
- **Status:** Agreed on merge. Not built.
- **Decision behind it:** none. It builds on [ADR-0001](../adr/0001-lock-free-control-audio-boundary.md) (queues between threads), [ADR-0020](../adr/0020-inferred-channel-count.md) (stereo), and [App preferences, Audio and MIDI devices](preferences.md#audio-and-midi-devices).
- **Use case and requirements:** UC3, FR-APP-03, FR-LIB-07. Catalogue: MT-IO-02 Audio input.
- **Background:** [Output safety](../learn/output-safety.md) (why a loud loop must be stopped at the output).
- **Prior art:** none needed.
- **Issues:** [#29](https://github.com/brylie/LoomDSP/issues/29).
- **Code:** `loom.io.audio-in` in `crates/loom-modules/src/io.rs`; the input stream in `crates/loom-host-desktop`; the `Host` trait in `crates/loom-app/src/devices.rs`.

## Goal

> I plug a guitar into my interface, open a patch with a filter and a delay, and play through it. I choose which input, and I am not surprised by a screech of feedback.

## The module

`loom.io.audio-in`, category I/O.

| | |
|---|---|
| Inputs | none |
| Output | `out`, one **stereo** port (`Channels::Stereo`, [stereo cables](stereo.md)) |
| Parameters | `pair`: whole number 1 to 4, default 1 (channels 1 and 2, 3 and 4, and so on). `gain`: 0 to 4, linear, default 1, smoothed over 5 ms. `mute`: toggle, default 0 |
| Display value | `active`: 1 when the input is delivering sound, else 0 (a light) |
| Panel | A knob for `gain`, a toggle for `mute`, radio or a select list for `pair`, the `active` light |

- **Mono devices.** A device with one channel feeds both sides with it. *Why:* a single microphone is expected in the middle.
- **A missing pair** (`pair` 3 on a stereo device) gives silence, and `active` is 0.
- **One per patch is not required.** Several modules read the same stream, each with its own `pair` and `gain`.
- **No device, no permission, or a stream that failed:** the module outputs silence and `active` is 0. The patch plays normally. The status bar says why, once (below).
- The module is a source: it reads the input ring (below) at the start of each block and does nothing else. The audio is not a parameter and does not depend on block size beyond how it is delivered.
- Gain in dB comes with the decibel mapping ([#99](https://github.com/brylie/LoomDSP/issues/99)); until then it is linear.

## One duplex stream, or two?

**Two streams, joined by a lock-free ring.** The output stream plays the patch; a separate input stream records. The input callback writes frames into a single-producer single-consumer ring (`rtrb`, ADR-0001); the engine reads frames from it at the start of each block.

*Why two:* the audio library offers one stream per direction and device, and there is no duplex stream across devices. Two streams also let the input and the output come from different devices. A stream on its own failing then does not take the other with it.

- **Same device for both** is the supported, drift-free case: both streams run from one clock.
- **The input opens at the output stream's sample rate.** If the input device does not offer it, the module is silent and the status bar says `Audio input: "<device>" does not offer 48000 Hz.`
- **Ring size** is 4 buffers of the larger stream, allocated when the stream opens, on the control thread.
- **Target fill** is one buffer. The engine counts how many frames it needs per block and takes them.

### Clock drift between two devices

Two different devices run on two crystals, so their clocks drift by a few parts per million. P1 handles it without resampling:

- **Underrun** (the ring has too few frames for the block): the block is zero, reading pauses until the ring is back at the target fill (one buffer), and `input_underruns` goes up by 1.
- **Overrun** (the ring is more than 2 buffers full): the oldest frames are dropped down to the target, and `input_overruns` goes up by 1.

Both corrections restore the target fill, so they behave the same way, and a slow input clock does not click on every block. Each correction is about one buffer of sound, so the interval between corrections is one buffer divided by the drift. At 20 ppm and 48 kHz the drift is about 1 frame a second, so a 256-frame buffer gives a correction about every 4 minutes, with one small click. The counters appear in the stats bar next to the dropouts, in amber, only while they are above 0 and have grown in the last 10 seconds. *Why not resample:* a drift-following resampler is a real piece of work with its own quality questions. Same-device operation, the common case on stage, never corrects. Resampling is out of scope and can be added behind the same ring.

## Latency

- **Shown, not compensated.** The Preferences window shows `Input latency about X ms` under the input row, from the input buffer and one block of the ring (buffers only, as for the output: [preferences, rule D8](preferences.md#rules)). A line says `The patch adds no delay of its own.`
- **No compensation.** Compensation would delay the other tracks of a live set, which is the wrong thing on stage.

## Choosing the input

A new **Audio input** row in the Audio section of Preferences:

| Control | Choices |
|---|---|
| Input device | **System default (`<name>`)**, every input device by name, or **Off** |
| (rate and buffer) | Follow the output stream |

- Saved as `audio.input`: `SystemDefault` (default), `Named("<full name>")`, or `Off`. The same rules as the output apply: a missing Named device falls back to the system default input at start, and the fallback is not saved.
- **The input stream is opened lazily:** only while the patch has at least one Audio In module and the input is not Off. *Why:* opening an input triggers the system's microphone permission prompt. It should appear when the player first uses the module, not when the app starts. Deleting the last module closes the stream.
- **Device loss** follows [device changes](device-changes.md): the module goes silent, and the stream is reopened when the device returns.
- **Flags:** `--input <part of name>` matches by name for the run, like `--device`. `--no-input` turns it off for the run. Neither is saved.

## Permission

- **macOS asks for microphone permission** the first time an input stream opens. The app bundle ([`scripts/bundle-macos.sh`](../../scripts/bundle-macos.sh)) must carry `NSMicrophoneUsageDescription` ("LoomDSP uses audio input so patches can process live sound."). Without it, macOS stops the app.
- **If the player denies access, or the OS blocks it,** the module is silent and the status bar says `Audio input: access to the microphone was denied. Allow LoomDSP in System Settings, Privacy and Security, Microphone.` (The text names the right place for each OS.) The message shows once per run, and again if the player changes the input device.
- **Windows and Linux** may have no prompt; the same message shows if the stream cannot open.

## Feedback

Live input into speakers can feed back. The app cannot know where the microphone and speakers are, so it does three things:

1. **The output limiter protects the output** ([output safety](output-safety.md)). The default Low latency mode is enough to stop a screech from being harmful; a Clean or Off setting gives less protection.
2. **A warning once.** The first time an Audio In module goes live in a session (it has audio, its `mute` is 0, and the output is not Off), the status bar says `Audio input is live. Use headphones, or keep the gain low, to avoid feedback.` It shows once per run.
3. **The same device warning.** When the input and the output are the same device, the Preferences window shows `Input and output use the same device. Speakers can feed the microphone.` under the input row.

The module does not start muted. *Why:* a module that makes no sound when added looks broken, and the warning and the limiter cover the risk.

## Rules

1. **No allocation or waiting in the audio thread.** The ring is allocated on the control thread and moved to the engine ([ADR-0001](../adr/0001-lock-free-control-audio-boundary.md)). Reading is wait-free.
2. **Determinism.** For testing, a `Host` supplies the input from a recorded buffer through the same ring. A patch given the same input frames at the same block positions gives bit-identical output. Live audio itself cannot be reproduced, and that is expected.
3. **Silence is the safe state.** Anything that goes wrong gives silence on `out`, never a repeated block.
4. **Input never changes a saved patch.** The device, the permission, and the counters are the machine's, not the patch's (preferences D1). A patch with an Audio In module opens on any machine.

## Acceptance criteria

- [ ] The descriptor has the ports and parameters above, with labels and descriptions, and a stereo `out`. The `pair`, `gain`, and `mute` parameters work. The registry accepts it.
- [ ] With the fake host feeding a recorded stereo buffer, the module outputs those frames, scaled by `gain`, with `mute` fading over 5 ms, bit-identical at block sizes 1, 77, and 512.
- [ ] A mono input feeds both sides. A missing pair gives silence and `active` 0.
- [ ] With no device, or a denied permission (fake host error), the patch plays, `out` is silent, and the status bar says why, once.
- [ ] The input stream opens when the first Audio In module is added and closes when the last is removed.
- [ ] An underrun gives zeros and counts; an overrun drops to the target and counts. (Test with a fake input clock running 1% fast and slow.)
- [ ] Input and output on one device show the same-device note. The first live Audio In shows the feedback note once per run.
- [ ] `audio.input` is saved and read like the output device, and a fallback is not saved.
- [ ] The `no_alloc` tests pass with a patch that has an Audio In module.
- [ ] Manual, on macOS, Windows, and Linux, with a real interface (list the OS and model in the pull request): play an instrument through a filter with the same device for both directions; the permission prompt shows on macOS the first time; denying it leaves the patch playing, silent on the input, with the message.

## Out of scope

- Resampling to follow the clock of a second device.
- Latency compensation.
- More than 8 input channels, and choosing channels one by one (only pairs).
- Recording the input to a file.
- Input monitoring controls beyond `gain` and `mute`.
- Host input in a plugin (with plugin export, later).

## Open points

- Whether `gain` should become dB when #99 lands. Leaning: yes, 0 dB default, −60 to +12 dB, and −inf as mute stays separate.
