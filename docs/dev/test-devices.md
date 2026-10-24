# Test devices

Some checks need audio or MIDI hardware: the ignored hardware tests, the control scripts in `examples/dev/`, and the [manual checks](manual-checks/). None of them assumes a particular device. You name the device in an environment variable, and a test whose device is missing prints `skipped:` and passes. This page lists what to install on each system, so you can run them on your machine or in CI.

The normal test suite (`cargo test --workspace`) needs none of this. App and QA tests play on a fake host and a stepped tape, with no device and no sound.

## Environment variables

| Variable | Used by | Set it to |
|---|---|---|
| `LOOM_DEVICE` | `device_tests` in `crates/loom-host-desktop`, `examples/dev/check-devices-part-2.jsonl` | Part of an output device's name, as `loom devices` lists it |
| `LOOM_LOOPBACK` | The capture tests in `device_tests` | Part of a loopback device's name (see below) |
| `LOOM_RATE`, `LOOM_BUFFER` | `examples/dev/check-devices-part-2.jsonl` | A sample rate and buffer size the device offers |

A control script stops with exit status 2 when a variable it uses is not set ([control channel, Script variables](../design/control-channel.md#script-variables)). A hardware test skips instead, because a test run should pass on a machine without the device.

Run the hardware tests one at a time, so no two of them use a device at once:

```sh
LOOM_DEVICE="<part of a name>" LOOM_LOOPBACK="<part of a name>" cargo test --workspace -- --ignored --test-threads=1
```

## Kinds of device

- **An output device** plays the test streams. Any output works, including the built-in speakers: the streams are silent.
- **A virtual output** is a device with no speaker behind it. It is useful on a machine with no sound card, such as a CI runner, or when you do not want to hear the tests.
- **A loopback device** is a virtual output whose input side gives back what was played. `loom capture` records it, so a test can check what really reached the device, not only what the engine rendered ([audio QA, Device capture](../design/audio-qa.md#device-capture)).
- **Virtual MIDI ports** let a test act as a MIDI controller ([audio QA, Virtual MIDI controllers](../design/audio-qa.md#virtual-midi-controllers)).

## macOS

| Need | Use |
|---|---|
| Output | The built-in output, or any interface |
| Virtual output and loopback | BlackHole 2ch, for example `brew install blackhole-2ch`. Set its volume to full in Audio MIDI Setup |
| Virtual MIDI ports | Built in (CoreMIDI). Nothing to install |

The capture tests also need microphone access for the terminal, because macOS treats any input, BlackHole too, as a microphone.

BlackHole is GPL-3.0. Install it as a separate program, in CI or on your machine. Never add it as a dependency or ship it (hard rule 4, ADR-0006).

## Linux

| Need | Use |
|---|---|
| Output | Any ALSA, PulseAudio, or PipeWire output |
| Virtual output and loopback | A PipeWire or PulseAudio null sink: `pactl load-module module-null-sink sink_name=loom-loop`, captured from its monitor. CI does this on a PipeWire null sink (#335). Or the ALSA loopback kernel module: `sudo modprobe snd-aloop` |
| Virtual MIDI ports | The ALSA sequencer: `/dev/snd/seq` must exist (`sudo modprobe snd-seq` if it does not). Containers often lack it, and then the MIDI tests skip |

## Windows

| Need | Use |
|---|---|
| Output | Any WASAPI output |
| Virtual output and loopback | Windows has none built in. Third-party virtual audio drivers exist. Check a driver's licence before installing it in CI, and note that hosted CI runners have no sound card (#337) |
| Virtual MIDI ports | Not available: `midir` cannot create virtual ports on Windows, so the MIDI tests skip and the manual check uses hardware controllers |

The control socket (`--control`) needs Unix sockets, so on Windows use `--control-script` (#233).

## When a test skips

A skip is not a pass of the check. In a manual check, write down which tests skipped and why. In CI, a job that installs a device should fail if its test skips, for example by checking for `skipped:` in the output.
