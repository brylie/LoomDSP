# Hot patching

Why a new cable into a running patch can click, what state has to carry over for it not to, and why a crossfade does not hide a cold start.

- **Specification:** [Changing a running patch](../design/live-patching.md), [Stereo cables](../design/stereo.md) rules 13 and 25 to 27
- **Decision:** [ADR-0022](../adr/0022-copies-take-state-from-the-running-instance.md)
- **Measurements:** `crates/loom-modules/tests/hot_patch.rs` (run with `--nocapture` to see the numbers)

## What a player expects

On a Eurorack case every module runs all the time. An LFO with nothing plugged into it still swings. A filter with no input still holds whatever charge its capacitors have. A cable does not start anything: it connects two things that are already running. So plugging the LFO into a filter's cutoff picks the LFO up mid-cycle, and the filter answers from where it was. VCV Rack behaves the same way.

LoomDSP mostly works like that. Every module lives in a slot on the audio thread and runs whether or not it is patched. When the player edits the patch, the engine compiles a new schedule and swaps it in at a buffer boundary. The modules stay in their slots and keep their state.

The trouble is state that is *not* in a module slot. There are two kinds: the one-sample delays the compiler inserts to break feedback loops, and the extra copy of a module that turns stereo.

## The signal: what "cold" sounds like

A filter's state is a few numbers: its integrators, here called `ic1` and `ic2`. They hold what the filter "remembers" about recent input. Feed a running lowpass a 110 Hz tone and its output is a smooth sine. Start a second, fresh lowpass at the same moment, on the same tone, and its integrators are 0. Its output starts at 0 and climbs towards the first filter's output, overshooting if the resonance is high.

Measured with the same signal on both sides, so the right side should equal the left exactly:

| Filter | How far the cold copy is off | How long until it agrees |
|---|---|---|
| Lowpass, 1000 Hz | -12 dB | 1 ms |
| Lowpass, 200 Hz | -6 dB | 13 ms |
| Lowpass, 500 Hz, resonance 0.9 | -3 dB | 20 ms |
| Highpass, 200 Hz | -12 dB, as a one-sample step | 14 ms |

A highpass is the harshest case. A fresh highpass passes its input straight through at first, so its output steps by the size of the input in one sample: a click.

The 1000 Hz row is short for a second reason. A new SVF is built with its cutoff at 1000 Hz, and `set_param` glides from there to the knob's value over 10 ms. So in the other rows the cold copy is also sweeping its cutoff for 10 ms. A fresh instance is not only empty, it is also mid-glide.

## Wrong turn: crossfade from the left side

The obvious fix is to fade the right side from the left copy's output to the new copy's output over a few milliseconds. It needs nothing from module authors.

It helps only when the new copy settles faster than the fade. A crossfade multiplies the error by a ramp. It does not make the error shorter. In the 1000 Hz row the copy is right after 1 ms, so a 5 ms fade hides it completely. In the 200 Hz row the copy is still wrong at 13 ms, so after the fade the right side plays the wrong signal at full level. With a delay line it is worse: the right side would have no echoes for the whole delay time, and the fade only moves the moment you notice.

## The fix for copies: hand over the state

The new copy should not start empty. At the swap it takes the running copy's state: the integrators, the smoothing, a delay line's buffer. Then it continues exactly where the left side was, and with the same signal on both sides the two are bit-identical. That needs one small method per module, which is why ADR-0022 makes it a decision.

One exception: a module with a random generator must not copy the generator as it is. Then the right side would play the same random numbers as the left, which is mono in disguise. It takes everything else and restarts its generator from its seed and its channel.

## Inserted delays: a value that was never there

A feedback loop needs a one-sample delay somewhere (ADR-0002). If the player does not place one, the compiler does. Its state is one number: the sample it is holding. Until #203, a new delay started at 0.

Often that is fine. Say the loop closes into an Add's B input. Before the cable, B read 0, because an unplugged Add input reads 0. A delay that starts at 0 gives 0 for one sample, then the signal. That is exactly what plugging the cable one sample later would give. Nothing extra is heard.

A delay that starts at 0 goes wrong when the input did not read 0 before:

- **A Multiply's B input reads 1 when nothing is plugged in.** That 1 is the input's default value, so a cable plugged into B also fades from 1, not from 0 (live patching spec, rule 9). Close a loop into it, and for one sample it reads 0 instead of 1 or the new signal. The Multiply's output drops out for one sample: a click. Measured error: -6 dB.
- **The compiler can move its delay.** Add a cable to a running loop, and the compiler may now break the loop on a different cable. The old delay is gone, and the new one started at 0, in the middle of a loop that was already ringing. Both placements make the same loop, so nothing should change. Measured error: -17 dB, for one sample.

The fix is to start a new delay with the value it would hold if it had always been there: the last sample its source wrote before the swap. That is what the cable would carry in hardware. The engine does this now (live patching spec, rule 5), and both cases above are bit-identical to the ideal.

## Cable changes: hold, then ramp

Rules 5 and 6 make the *new signal* right from its first sample. But plugging a cable is still a step: the input read one thing a moment ago and reads another now. A step in a signal is a click. On hardware the same step happens, and the player hears it. LoomDSP softens it (live patching spec, rule 9).

When a cable into an input is plugged, pulled, or moved, the input holds the last value it read and moves in a straight line to the new signal over 5 ms. Five milliseconds is long enough that the step stops being a click and short enough that nobody hears it as a fade.

*Why hold, and not crossfade from the old signal:* the old source may be gone, and keeping it running costs CPU for a sound that is leaving. Holding its last value keeps the input continuous, which is all a click needs.

*Why some changes do not ramp:* a ramp mixes a held value into the new signal for 5 ms. If the new signal is already continuous with the old one, that mix is a short dip, which is worse than nothing. So nothing ramps when the compiler only moves a delay (rule 5), when a source turns stereo (rule 6), or when the editor only rearranges cables around a mixer it adds, such as the Swap fade at the start and the end of a slot swap. When a mixer arrives or leaves with a real change of signal (a stack going from 1 copy to 2, Layer to Select), the player hears the ordinary 5 ms hold and ramp, not a crossfade, so a short dip on a sustained sound is intended.

*Why gates, triggers, and phases switch:* a ramped gate crosses its 0.5 threshold late and fires late, and a ramped phase passes through values that look like wraps.

What still steps, and why, is listed in [rule 9 of the live patching spec](../design/live-patching.md#rules).

## How the measurements work

Each test renders twice: once with the edit, and once as the ideal continuation, the same patch as if the new path had run all along. For copies, the ideal is the left side. For a new loop, it is the same patch with an explicit Unit Delay that was already running. For a moved delay, it is the patch with no edit. The difference is the error. If its peak is above -40 dBFS, the test calls it a click. Below -60 dBFS it is inaudible. These thresholds are a tool for tests. A listening check in the app confirms them.
