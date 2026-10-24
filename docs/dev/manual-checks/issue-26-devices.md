# Manual check: audio and MIDI devices (#26)

A script for the two manual checks in [preferences.md](../../design/preferences.md): output safety in the Preferences window, and audio and MIDI device selection. Run it once on each of macOS, Windows, and Linux. It takes about 30 minutes per system.

The automated tests already cover the logic with a fake host. This script checks what only real hardware shows: clicks, thumps, device names, and the real preferences file.

## What you need

- The repository at the commit you are checking, with Rust installed.
- Headphones or monitors. **Keep the volume low.** Part 1 plays noise far over full scale.
- A USB audio interface that offers 96 kHz.
- Two MIDI controllers. Call them **Keys** (the one you keep on) and **Pad** (the one you turn off).
- The built-in output of the computer (speakers or headphone jack).

## Results to report

Copy this into a comment on [#26](https://github.com/brylie/LoomDSP/issues/26) for each system, and fill it in as you go.

```text
OS and version:
Commit:
Built-in output name:
Audio interface (name as listed):
Smallest buffer offered at 96 kHz:
MIDI controllers (names as listed):

Part 1, output safety:     1.1 [ ]  1.2 [ ]  1.3 [ ]  1.4 [ ]
Part 2, device change:     2.1 [ ]  2.2 [ ]  2.3 [ ]  2.4 [ ]
Part 3, fade-out:          3.1 [ ]  3.2 [ ]
Part 4, restart:           4.1 [ ]  4.2 [ ]
Part 5, MIDI inputs:       5.1 [ ]  5.2 [ ]  5.3 [ ]
Notes (anything odd, with the step number):
```

## Part 0: set up

**0.1. Build once.** All commands run from the repository root.

```sh
cargo build --release -p loom-app -p loom-cli
```

**0.2. List the devices.** Plug in the interface and both controllers first.

```sh
cargo run --release -p loom-cli -- devices
```

Write down the exact names of the built-in output, the interface, and both controllers.

**0.3. Set the preferences file aside.** This check needs a fresh start, and it writes to the real per-user file, because remembering it between runs is part of the check.

| System | File |
|---|---|
| macOS | `~/Library/Application Support/LoomDSP/preferences.ron` |
| Windows | `%LOCALAPPDATA%\LoomDSP\preferences.ron` |
| Linux | `$XDG_CONFIG_HOME/loomdsp/preferences.ron`, or `~/.config/loomdsp/preferences.ron` |

If the file exists, rename it to `preferences.ron.before-check`. Put it back at the end.

**0.4. Set the system default output to the built-in output** in the operating system's sound settings, not the interface.

## Part 1: output safety in the window

**1.1. Open the window.**

```sh
cargo run --release -p loom-app -- examples/dev/check-loud-noise.ron
```

Press **Cmd+,** (macOS) or **Ctrl+,** (Windows, Linux).

- Look: the Preferences window opens, with Output safety at the top and Low latency chosen.
- Listen: loud noise, but not harsh or distorted. Low latency holds it under -1 dBFS.

**1.2. Switch modes.** Choose Clean, then Off, then Low latency, then Clean. Wait two seconds between each.

- Listen: at each switch the sound dips and comes back. There is no click.
- Listen: in Off the noise is louder and harsher, because it is clipped at full scale. In Low latency and Clean it is quieter and clean.

**1.3. Leave it on Clean** and quit the app (close the window).

**1.4. Start again** with the same command, and press Cmd+, or Ctrl+,.

- Look: Clean is still chosen.
- Look: the preferences file from step 0.3 now exists and contains `output_safety: "clean"`.

Choose Low latency again before you go on, so the rest of the script runs in the default mode.

## Part 2: change the device while a patch plays

**2.1. Start the drone and record what the engine plays.**

```sh
cargo run --release -p loom-app -- examples/dev/check-drone.ron --record target/check-device.wav
```

- Listen: a quiet steady tone (middle C) on the built-in output.
- Look: in Preferences, the line under Audio output says `Playing on "<built-in output>" at <rate> Hz, <n> frames (<ms> ms).`
- Look: Device shows `System default (<built-in output>)`.

**2.2. Pick the interface at the same rate.** In Device, choose the interface by name. Leave Sample rate and Buffer size on Device default.

- Listen: the tone moves from the built-in output to the interface after a short gap. There is no click at the end of the old sound or at the start of the new one.
- Look: the "Playing on" line names the interface.

If the interface's default rate is the same as the built-in output's, the recording keeps going. Quit the app now, then check the recording:

```sh
cargo run --release -p loom-cli -- analyze target/check-device.wav --max-step 0.01
```

- It passes (exit status 0) if the engine faded out and faded in. A failure names the time of the largest step. The recording holds what the engine plays, so it does not show the gap itself or a click added by the device. Your ears check those.

If the rates differ, the recording ended at the change, and only your ears check this step. Start the app again with the drone (2.1, without `--record`) and choose the interface before going on.

**2.3. Pick 96 kHz.** In Sample rate, choose 96000 Hz.

- Look: Sample rate lists Device default and only common rates (from 44100, 48000, 88200, 96000, 176400, 192000 Hz).
- Listen: a short gap, then the tone again at the same pitch, with no click.
- Look: the "Playing on" line says 96000 Hz.

**2.4. Pick the smallest buffer.** In Buffer size, choose the smallest size offered.

- Look: each size shows its time, for example `64 frames · 0.7 ms`.
- Listen: a short gap, then the tone, with no click and no crackle after it settles. Write the size down.
- Look: if the device refused the size, the status bar says why and the tone still plays.

Leave the app running for Part 3.

## Part 3: fade-out when opening a patch

This checks that the old engine fades out before it stops (output safety rule 14). A DC offset with no safety stage is the worst case: a hard stop is a thump.

**3.1. Open a DC patch with output safety Off.** Quit the app, then:

```sh
cargo run --release -p loom-app -- examples/dev/check-dc.ron --output-safety off --record target/check-dc.wav
```

- Listen: almost nothing, maybe a soft thump at start on some devices. This is right: a constant is not a sound, only an offset that pushes the speaker cone out and holds it there. It plays on the interface at 96 kHz, as saved in Part 2.
- Look: in Preferences, Output safety shows `Set by --output-safety for this run.`
- **Do not touch the Constant's knob.** The recording must hold only the stops. A knob move shows up as a large step and fails the check below, even though it is smoothed and not a click.

**3.2. Open another patch.** Use Open in the app to open `examples/dev/check-drone.ron`, from the same checkout you run the app from. Then make a new patch with New. Then quit.

The check patches are new on this branch, so another checkout of the repository may not have them. In a worktree under `.claude/worktrees/`, the dialog hides the `.claude` folder because its name starts with a dot. On macOS, press Cmd+Shift+G in the dialog and type the full path, or Cmd+Shift+. to show hidden folders. On Linux, Ctrl+L types a path, and Ctrl+H shows hidden folders.

- Listen: no thump at Open, at New, or at quit. You hear the drone between Open and New.
- Check the recording:

```sh
cargo run --release -p loom-cli -- analyze target/check-dc.wav --max-step 0.01
```

It passes if every stop faded out. A hard stop from 0.5 would be a step of 0.5. A fade over 10 ms takes steps of about 0.001, and the drone's own largest step is under 0.008.

- Look: the peak is 0.5, and nothing is over the ceiling. A peak of 1.0 means the knob was moved: record again.
- Look: the run is as long as you expect, and the drone is in it. If you remember an Open but the file has no drone, write that in Notes. The recording should continue across Open and New.

## Part 4: the choice is remembered, and a missing interface falls back

**4.1. Start again with the interface plugged in.**

```sh
cargo run --release -p loom-app -- examples/dev/check-drone.ron
```

- Listen: the tone plays on the interface.
- Look: Preferences shows the interface, 96000 Hz, and the buffer from step 2.4. The status bar has no device message.

Quit.

**4.2. Unplug the interface** and start again with the same command.

- Listen: the tone plays on the built-in output.
- Look: the status bar says `Audio: "<interface>" is not connected. Playing on "<built-in output>". Choose a device in Preferences.` If the built-in output does not offer 96 kHz or the buffer size, a second message says which default it uses instead.
- Look: the preferences file still names the interface, 96000 Hz, and the buffer. Do not choose anything in the window before you look.

Quit, and plug the interface back in.

## Part 5: MIDI inputs

**5.1. Both controllers play.**

```sh
cargo run --release -p loom-app -- examples/patches/subtractive.ron
```

- Play a note on Keys, then on Pad. Both play the patch.
- Look: Preferences lists both controllers with their check boxes checked, and the line `New MIDI inputs are on until you turn them off.`

**5.2. Turn Pad off while a note is held.** Hold a note on Pad. While you hold it, uncheck Pad in Preferences.

- Listen: the held note stops, even though you still hold the key. Audio does not stop.
- Play Pad again: nothing. Play Keys: it plays.
- Look: the preferences file has `off: ["<Pad name>"]`.

Quit.

**5.3. Start again.** Use the same command.

- Look: Pad is still unchecked in Preferences.
- Play Pad: nothing. Play Keys: it plays.

## Clean up

- Turn Pad back on in Preferences, or leave it off if you like.
- Put back the preferences file you set aside in step 0.3, if there was one.
- Delete `target/check-device.wav` and `target/check-dc.wav`.
- Set the system default output back to what you use.

## When a step fails

Write the step number, what you heard or saw, and what you expected in Notes. Copy the status bar text exactly. For a click, record it again with `--record` and attach the `loom analyze` output. A failure the fake host cannot show, such as a device that opens and then refuses to start, is a new issue linked to #26.
