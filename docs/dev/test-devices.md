# Test devices

Some checks need audio or MIDI hardware: the ignored hardware tests, the control scripts in `examples/dev/`, and the [manual checks](manual-checks/). None of them assumes a particular device. You name the device in an environment variable, and a test whose device is missing prints `skipped:` and passes. This page lists what to install on each system, so you can run them on your machine or in CI.

The normal test suite (`cargo test --workspace`) needs none of this. App and QA tests play on a fake host and a stepped tape, with no device and no sound.

## Environment variables

| Variable | Used by | Set it to |
|---|---|---|
| `LOOM_DEVICE` | `device_tests` in `crates/loom-host-desktop`, `examples/dev/check-devices-part-2.jsonl` | Part of an output device's name, as `loom devices` lists it |
| `LOOM_LOOPBACK` | The capture and audio input tests in `device_tests` | Part of a loopback device's name (see below) |
| `LOOM_RATE`, `LOOM_BUFFER` | `examples/dev/check-devices-part-2.jsonl` | A sample rate and buffer size the device offers |

A control script stops with exit status 2 when a variable it uses is not set ([control channel, Script variables](../design/control-channel.md#script-variables)). A hardware test skips instead, because a test run should pass on a machine without the device.

Run the hardware tests one at a time, so no two of them use a device at once:

```sh
export LOOM_DEVICE="<part of a name>" LOOM_LOOPBACK="<part of a name>"
cargo test -p loom-host-desktop --lib -- --ignored --test-threads=1
cargo test -p loom-app --lib -- --ignored --test-threads=1 system_input_adapter_uses_loopback_device turning_a_real_input_off_releases_its_notes_and_is_saved
cargo test -p loom-cli --test integration -- --ignored --test-threads=1 capture::
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

## Unattended playback and the offline soak

`loom play` keeps playing when standard input closes. An input line, Ctrl+C,
or SIGTERM on Unix requests the output fade and exit. When stderr is
redirected, it records timestamped starts, stops, callback stalls, and
changes to the overrun or stream-error counts.

A 12-hour offline run is available without an audio device. It plays the
modulation example at 8 kHz to check for silent or stuck output, non-finite
samples, DC, and bar timing. Run it in release mode:

```sh
cargo test --release -p loom-cli --bin loom modulated_patch_survives_a_twelve_hour_offline_soak -- --ignored
```
