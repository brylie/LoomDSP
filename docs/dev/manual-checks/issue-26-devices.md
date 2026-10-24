# Manual check: audio and MIDI devices (#26)

A script for the two manual checks in [preferences.md](../../design/preferences.md): output safety in the Preferences window, and audio and MIDI device selection. Run it once on each of macOS, Windows, and Linux. It takes about 30 minutes per system.

The automated tests already cover the logic with a fake host. This script checks what only real hardware shows: clicks, thumps, device names, and the real preferences file.

`--record` saves what the engine rendered, before the device, so it cannot show a stream cut before the device played its fade. Where a click or thump at a stop is the question, the script also plays into a loopback device and records what reached it with `loom capture` ([audio QA, Part 4](../../design/audio-qa.md#part-4-capture-what-reached-a-device)). Your ears still check the real interface and the built-in output.

## What you need

- The repository at the commit you are checking, with Rust installed.
- Headphones or monitors. **Keep the volume low.** Part 1 plays noise far over full scale.
- A USB audio interface that offers 96 kHz.
- Two MIDI controllers. Call them **Keys** (the one you keep on) and **Pad** (the one you turn off).
- The built-in output of the computer (speakers or headphone jack).
- A loopback device: BlackHole 2ch on macOS, or a PipeWire or PulseAudio null sink on Linux (`pactl load-module module-null-sink sink_name=loom-loop`, captured from its monitor). On Windows, skip the steps marked **(loopback)**.

## Results to report

Copy this into a comment on [#26](https://github.com/brylie/LoomDSP/issues/26) for each system, and fill it in as you go.

Each step has a short name, such as `device-tests`, which the results and Notes use. Steps have names, not numbers, so a new step does not renumber the others, and two changes that each add a step do not clash. Mark a step `[x]` when it passes, and write a failure in Notes.

```text
OS and version:
Commit:
Built-in output name:
Audio interface (name as listed):
Smallest buffer offered at 96 kHz:
MIDI controllers (names as listed):
Loopback device (name as listed, or none):

Set up:
[ ] device-tests

Output safety:
[ ] safety-window
[ ] safety-switch
[ ] safety-leave-clean
[ ] safety-remembered

Device change:
[ ] drone-record
[ ] interface-same-rate
[ ] rate-96k
[ ] smallest-buffer
[ ] device-script
[ ] device-change-on-loopback

Fade-out:
[ ] dc-safety-off
[ ] open-another
[ ] stops-on-loopback

Restart:
[ ] choice-remembered
[ ] interface-unplugged

MIDI inputs:
[ ] both-controllers
[ ] pad-off-while-held
[ ] pad-stays-off

Notes (anything odd, with the step's name):
```

## Part 0: set up

**`build`: Build once.** All commands run from the repository root.

```sh
cargo build --release -p loom-app -p loom-cli
```

**`list-devices`: List the devices.** Plug in the interface and both controllers first.

```sh
cargo run --release -p loom-cli -- devices
```

Write down the exact names of the built-in output, the interface, both controllers, and the loopback device. The loopback is listed under both Audio outputs and Audio inputs.

**`set-aside-preferences`: Set the preferences file aside.** This check needs a fresh start, and it writes to the real per-user file, because remembering it between runs is part of the check.

| System | File |
|---|---|
| macOS | `~/Library/Application Support/LoomDSP/preferences.ron` |
| Windows | `%LOCALAPPDATA%\LoomDSP\preferences.ron` |
| Linux | `$XDG_CONFIG_HOME/loomdsp/preferences.ron`, or `~/.config/loomdsp/preferences.ron` |

If the file exists, rename it to `preferences.ron.before-check`. Put it back at the end.

**`device-tests`: Run the device tests.** One plays silent streams at 44.1, 48, 96, and 44.1 kHz on the interface and checks that each one runs. On macOS a stream at a new rate used to stop at once, so the app went silent after a sample rate change.

The other two **(loopback)** play a DC offset into the loopback device with 4096-frame buffers, stop it, and capture what reached the device. One checks that the stop faded out. The other stops at a fixed 100 ms limit, as before the fix in #26, and checks that the capture shows the cut, so the first check is known to be able to fail. They are skipped when `LOOM_LOOPBACK` is not set.

Set the loopback's output volume to full first (on macOS: Audio MIDI Setup, BlackHole 2ch, Output, volume 1.0), so later steps can use fixed limits. The tests measure relative to the level, so they do not need it.

```sh
LOOM_DEVICE="<part of the interface name>" LOOM_LOOPBACK="<part of the loopback name>" cargo test -p loom-host-desktop -- --ignored --test-threads=1
```

On Windows, in PowerShell (there is no loopback on Windows yet, so the loopback tests skip):

```powershell
$env:LOOM_DEVICE = "<part of the interface name>"; cargo test -p loom-host-desktop -- --ignored --test-threads=1
```

On macOS, the first capture asks for microphone access for the terminal. Allow it: without it, input devices deliver silence, and the loopback tests fail with "the loopback delivered sound".

**`default-output`: Set the system default output to the built-in output** in the operating system's sound settings, not the interface.

## Part 1: output safety in the window

**`safety-window`: Open the window.**

```sh
cargo run --release -p loom-app -- examples/dev/check-loud-noise.ron
```

Press **Cmd+,** (macOS) or **Ctrl+,** (Windows, Linux).

- Look: the Preferences window opens, with Output safety at the top and Low latency chosen.
- Listen: loud noise, but not harsh or distorted. Low latency holds it under -1 dBFS.

**`safety-switch`: Switch modes.** Choose Clean, then Off, then Low latency, then Clean. Wait two seconds between each.

- Listen: at each switch the sound dips and comes back. There is no click.
- Listen: in Off the noise is louder and harsher, because it is clipped at full scale. In Low latency and Clean it is quieter and clean.

**`safety-leave-clean`: Leave it on Clean** and quit the app (close the window).

**`safety-remembered`: Start again** with the same command, and press Cmd+, or Ctrl+,.

- Look: Clean is still chosen.
- Look: the preferences file from `set-aside-preferences` now exists and contains `output_safety: "clean"`.

Choose Low latency again before you go on, so the rest of the script runs in the default mode.

## Part 2: change the device while a patch plays

**`drone-record`: Start the drone and record what the engine plays.**

```sh
cargo run --release -p loom-app -- examples/dev/check-drone.ron --record target/check-device.wav
```

- Listen: a quiet steady tone (middle C) on the built-in output.
- Look: in Preferences, the line under Audio output says `Playing on "<built-in output>" at <rate> Hz, <n> frames (<ms> ms).`
- Look: Device shows `System default (<built-in output>)`.

**`interface-same-rate`: Pick the interface at the same rate.** In Device, choose the interface by name. Leave Sample rate and Buffer size on Device default.

- Listen: the tone moves from the built-in output to the interface after a short gap. There is no click at the end of the old sound or at the start of the new one.
- Look: the "Playing on" line names the interface.

If the interface's default rate is the same as the built-in output's, the recording keeps going. Quit the app now, then check the recording:

```sh
cargo run --release -p loom-cli -- analyze target/check-device.wav --max-step 0.01
```

- It passes (exit status 0) if the engine faded out and faded in. A failure names the time of the largest step. The recording holds what the engine plays, so it does not show the gap itself or a click added by the device. Your ears check those.

If the rates differ, the recording ended at the change, and only your ears check this step. `device-change-on-loopback` checks a device change on the device side.

Either way, start the app again with the drone (`drone-record`, without `--record`) and choose the interface before going on, if it is not running.

**`rate-96k`: Pick 96 kHz.** In Sample rate, choose 96000 Hz.

- Look: Sample rate lists Device default and only common rates (from 44100, 48000, 88200, 96000, 176400, 192000 Hz).
- Listen: a short gap, then the tone again at the same pitch, with no click.
- Look: the "Playing on" line says 96000 Hz.

**`smallest-buffer`: Pick the smallest buffer.** In Buffer size, choose the smallest size offered.

- Look: each size shows its time, for example `64 frames · 0.7 ms`.
- Listen: a short gap, then the tone, with no click and no crackle after it settles. Write the size down.
- Look: if the device refused the size, the status bar says why and the tone still plays.

**`device-script`: Or run Part 2 as a script.** The [control channel](../../design/control-channel.md#devices-and-stream-health) can make the choices in `interface-same-rate`, `rate-96k`, and `smallest-buffer` itself, and tell whether the device really calls back. It does not listen: your ears still check for clicks and crackle. The script picks the first device whose name contains `BlackHole` and the smallest buffer it offers, 32 frames. For your interface, copy the file and change the `device` and `buffer` lines. A refused size gets an error that lists the sizes offered.

```sh
cargo run --release -p loom-app -- examples/dev/check-drone.ron --preferences target/check-prefs.ron --control-script examples/dev/check-devices-part-2.jsonl | jq -c 'if .snapshot then {id, audio} else . end'
```

`--preferences` keeps the real preferences file out of it. Each `audio` reply gives the "Playing on" line. Each `snapshot` gives `audio.callbacks` and `audio.advanced`. The step passes when every snapshot after a change has `advanced: true`, the right `rate` and `buffer`, and `stream_errors` that stays the same after the first second. If `advanced` is false, the device stopped calling back while the app said it played: the bug fixed in `settle_rate`.

**`device-change-on-loopback`: (loopback) A device change, as the device played it.** Quit the app. Set the loopback to 48 kHz first (on macOS in Audio MIDI Setup). Both commands below use 48 kHz: `rate-96k` saved 96 kHz, and if the app switched the loopback's rate under the capture, the capture would end with an error. In one terminal, start a capture, then start the drone on the loopback:

```sh
cargo run --release -p loom-cli -- capture --device "<loopback>" --sample-rate 48000 -o target/check-device-side.wav --seconds 30
```

Wait for `Capturing "<loopback>" at <rate> Hz`, then in a second terminal:

```sh
cargo run --release -p loom-app -- examples/dev/check-drone.ron --device "<loopback>" --sample-rate 48000 --buffer 4096
```

In Preferences, choose the interface, wait two seconds, choose the loopback again, wait two seconds, and quit, all within the 30 seconds. Wait for the capture in the first terminal to end. It must end with `Wrote <n> frames` and no `error:` line: a capture that dropped frames or saw a stream error proves nothing. Then:

```sh
cargo run --release -p loom-cli -- analyze target/check-device-side.wav --max-step 0.01
```

- It passes (exit status 0) if the old stream faded out on the loopback before it stopped, and the new one faded in. A device-side cut shows as a step near the drone's level.
- Look: the peak is about the drone's level, not 0. A peak of 0 means the capture heard nothing: check microphone access (`device-tests`) and the volume.

Start the app again with the drone on the interface (`drone-record`, without `--record`, choosing the interface in Preferences) and leave it running for Part 3. Your choice of the interface, 96 kHz, and the buffer from `rate-96k` and `smallest-buffer` must be saved for Part 4: if the window shows the loopback, choose them again.

## Part 3: fade-out when opening a patch

This checks that the old engine fades out before it stops (output safety rule 14). A DC offset with no safety stage is the worst case: a hard stop is a thump.

**`dc-safety-off`: Open a DC patch with output safety Off.** Quit the app, then:

```sh
cargo run --release -p loom-app -- examples/dev/check-dc.ron --output-safety off --record target/check-dc.wav
```

- Listen: almost nothing, maybe a soft thump at start on some devices. This is right: a constant is not a sound, only an offset that pushes the speaker cone out and holds it there. It plays on the interface at 96 kHz, as saved in Part 2.
- Look: in Preferences, Output safety shows `Set by --output-safety for this run.`
- **Do not touch the Constant's knob.** The recording must hold only the stops. A knob move shows up as a large step and fails the check below, even though it is smoothed and not a click.

**`open-another`: Open another patch.** Use Open in the app to open `examples/dev/check-drone.ron`, from the same checkout you run the app from. Then make a new patch with New. Then quit.

The check patches are new on this branch, so another checkout of the repository may not have them. In a worktree under `.claude/worktrees/`, the dialog hides the `.claude` folder because its name starts with a dot. On macOS, press Cmd+Shift+G in the dialog and type the full path, or Cmd+Shift+. to show hidden folders. On Linux, Ctrl+L types a path, and Ctrl+H shows hidden folders.

- Listen: no thump at Open, at New, or at quit. You hear the drone between Open and New.
- Check the recording:

```sh
cargo run --release -p loom-cli -- analyze target/check-dc.wav --max-step 0.01
```

It passes if every stop faded out. A hard stop from 0.5 would be a step of 0.5. A fade over 10 ms takes steps of about 0.001, and the drone's own largest step is under 0.008.

- Look: the peak is 0.5, and nothing is over the ceiling. A peak of 1.0 means the knob was moved: record again.
- Look: the run is as long as you expect, and the drone is in it. If you remember an Open but the file has no drone, write that in Notes. The recording should continue across Open and New.

**`stops-on-loopback`: (loopback) The same stops, as the device played them.** The recording in `open-another` is what the engine rendered. With large buffers the stream used to stop before the device played the fade (#26), and the recording was clean anyway. This step captures what reached the device, with the large buffer that showed the bug. It needs no window clicks: a control script opens the patches and quits. Save this as `target/check-stops.jsonl`, with the full path to your checkout in place of `<checkout>`:

```text
{"cmd":"wait","ms":1000}
{"cmd":"open","path":"<checkout>/examples/dev/check-drone.ron"}
{"cmd":"wait","ms":1000}
{"cmd":"open","path":"<checkout>/examples/dev/check-dc.ron"}
{"cmd":"wait","ms":1000}
{"cmd":"quit"}
```

Quit the app. Start the capture in one terminal:

```sh
cargo run --release -p loom-cli -- capture --device "<loopback>" --sample-rate 48000 -o target/check-dc-device.wav --seconds 10
```

The capture runs at the loopback's current rate. The app's `--sample-rate 48000` switches the loopback to 48 kHz when it starts, so set the loopback to 48 kHz first (on macOS in Audio MIDI Setup), or the capture ends with an error. At 48 kHz a 4096-frame buffer is 85 ms long, as when Open clicked in #26. At 96 kHz the same buffer is short enough to hide the bug.

Wait for `Capturing`, then in a second terminal:

```sh
cargo run --release -p loom-app -- examples/dev/check-dc.ron --output-safety off --device "<loopback>" --sample-rate 48000 --buffer 4096 --control-script target/check-stops.jsonl
```

When the capture has ended, with `Wrote <n> frames` and no `error:` line:

```sh
cargo run --release -p loom-cli -- analyze target/check-dc-device.wav --max-step 0.01
```

- It passes if every stop faded out on the device: at the Open of the drone, at the Open of the DC patch, and at quit. A cut from the DC offset is a step of 0.5.
- Look: the peak is 0.5. A lower peak means the loopback's volume is not full (`device-tests`): the check is then too easy, so set the volume and capture again.
- `--device`, `--sample-rate`, and `--buffer` are for this run only, so the saved interface choice stays for Part 4.

## Part 4: the choice is remembered, and a missing interface falls back

**`choice-remembered`: Start again with the interface plugged in.**

```sh
cargo run --release -p loom-app -- examples/dev/check-drone.ron
```

- Listen: the tone plays on the interface.
- Look: Preferences shows the interface, 96000 Hz, and the buffer from `smallest-buffer`. The status bar has no device message.

Quit.

**`interface-unplugged`: Unplug the interface** and start again with the same command.

- Listen: the tone plays on the built-in output.
- Look: the status bar says `Audio: "<interface>" is not connected. Playing on "<built-in output>". Choose a device in Preferences.` If the built-in output does not offer 96 kHz or the buffer size, a second message says which default it uses instead.
- Look: the preferences file still names the interface, 96000 Hz, and the buffer. Do not choose anything in the window before you look.

Quit, and plug the interface back in.

## Part 5: MIDI inputs

**`both-controllers`: Both controllers play.**

```sh
cargo run --release -p loom-app -- examples/patches/subtractive.ron
```

- Play a note on Keys, then on Pad. Both play the patch.
- Look: Preferences lists both controllers with their check boxes checked, and the line `New MIDI inputs are on until you turn them off.`

**`pad-off-while-held`: Turn Pad off while a note is held.** Hold a note on Pad. While you hold it, uncheck Pad in Preferences.

- Listen: the held note stops, even though you still hold the key. Audio does not stop.
- Play Pad again: nothing. Play Keys: it plays.
- Look: the preferences file has `off: ["<Pad name>"]`.

Quit.

**`pad-stays-off`: Start again.** Use the same command.

- Look: Pad is still unchecked in Preferences.
- Play Pad: nothing. Play Keys: it plays.

## Clean up

- Turn Pad back on in Preferences, or leave it off if you like.
- Put back the preferences file you set aside in `set-aside-preferences`, if there was one.
- Delete `target/check-device.wav`, `target/check-dc.wav`, `target/check-prefs.ron`, `target/check-device-side.wav`, `target/check-dc-device.wav`, and `target/check-stops.jsonl`.
- Set the system default output back to what you use.

## When a step fails

Write the step's name, what you heard or saw, and what you expected in Notes. Copy the status bar text exactly. For a click, record it again with `--record`, and capture it on the loopback if you can, and attach both `loom analyze` outputs. A click in the capture but not in the recording happened after the engine: in the stop, the stream, or the device. A failure the fake host cannot show, such as a device that opens and then refuses to start, is a new issue linked to #26.
