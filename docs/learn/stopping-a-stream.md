# Stopping a stream without a click

Why the engine fades out before audio stops, why the host then waits for silent buffers, why a stop takes about four buffers, and why a stream at a new sample rate paused itself on macOS. For developers who work on the audio host, and for anyone who hears a click on Open or New.

- **Kind:** explainer.
- **Used by:** [output safety specification](../design/output-safety.md) (rule 14), [device changes](../design/device-changes.md).
- **Related:** [Output safety](output-safety.md), [Real-time safety](real-time-safety.md).
- **Code:** `Controller::fade_out`, `RunningAudio::stop`, `stop_limit`, `settle_rate`, `resume_after_switch` in `crates/loom-host-desktop/src/lib.rs`.

## A stop is a step

If the output is at 0.6 and the stream stops, the next thing the speaker gets is 0. That is a jump from 0.6 to 0.0, and a jump has energy at every frequency. It is the same click as a first sample that is not 0 ([output safety](output-safety.md#clicks-at-the-start)), only at the other end.

A held offset makes it worse. A Constant of 1.0 into Audio Out is silent but holds the speaker cone out. With the safety stage off, nothing removes that offset, and stopping the stream steps it from 1.0 to 0 in one sample. Even with the DC blocker on, the step happens after the blocker, so the blocker cannot help.

The fix is the one used at the start. Ramp the output to 0 first, then stop.

## Step 1: the engine fades out

`Controller::fade_out` asks the engine to ramp the output from where it is now to 0 in a straight line, at the same rate as the fade-in (0 to 1 takes 10 ms), and to stay at 0. It works in every safety mode, Off included. When the output reaches 0, the engine reports it is silent.

## Step 2: the host waits for two silent buffers

The tempting next step is "the engine says it is silent, so stop now". It clicks, because the engine **renders ahead** of what the device plays.

The device asks for a buffer, the engine fills it, and the device plays it a little later. When the engine says "silent", it means it has *rendered* silence. The end of the fade may still be sitting in a buffer the device has not played. Stopping then drops it, and the device never hears the last part of the ramp.

So the host keeps the stream running until it has given the device **two more buffers** of silence. By then the whole fade has been played.

## Why a stop takes about four buffers

Count what happens, in buffers:

| Step | Cost |
|---|---|
| The fade-out request waits for the engine to read it at the start of its next callback | up to 1 buffer |
| The fade is rendered | 1 buffer |
| Two silent buffers go to the device | 2 buffers |
| **Total** | **4 buffers**, plus the 10 ms fade itself |

The host also needs a **limit**. If no report comes (a stuck audio thread) or the device stops asking for buffers, the app must not hang. The limit is 100 ms, or 10 ms plus four buffers, whichever is longer (`stop_limit`).

### The bug that this explains

The limit was once a fixed 100 ms. That is enough for small buffers. But with 4096 frames at 44.1 kHz, one buffer is 93 ms, and four buffers are 372 ms. A 100 ms limit cut the stream off before the fade reached the device, and **Open clicked**. The #26 manual check found it. Now the limit grows with the buffer: at 4096 frames at 44.1 kHz it is about 0.38 s.

The lesson: a time limit that ignores buffer size is wrong for audio. Measure in buffers first.

A device that has already gone (unplugged) cannot be faded, because its stream no longer runs.

## Starting at a new sample rate

The same care applies when a stream starts. On macOS, building a stream at a different sample rate **switches the device's rate**. The audio library also listens for rate changes, and treats one as "the device changed under me", so it **pauses the stream**.

The notice of the stream's own switch could arrive after the stream began listening. The stream then paused itself, and the app went silent after a sample rate change.

The host handles it in two parts:

1. **`settle_rate`**: before building the real stream, build a short silent stream at the new rate. That makes the switch happen first. Wait until the device reports the new rate (up to 1 s). Then the real stream starts on a device that is already at its rate.
2. **`resume_after_switch`**: some devices report the new rate *before* sending the notice, so the notice can still pause the real stream. For 500 ms after a switch, the host starts the stream again each time it reports an error. That is right because this stream caused the switch. A rate change from outside later on still pauses it, as it should.

Both are macOS-only.

## Rules of thumb

- Fade before you stop. Never end a stream at a non-zero value.
- Count in buffers, not milliseconds. The device plays what was rendered earlier.
- Add a limit for every wait, and scale it with the buffer size.
- When a system reacts to a change you make, check the order of the notice and the change.
