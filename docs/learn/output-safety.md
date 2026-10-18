# Output safety

Why a signal can be "legal" and still harm speakers or ears, and what a safety stage on the final output does about it.

- **Used by:** [Output safety spec](../design/output-safety.md), [ADR-0019](../adr/0019-output-safety-stage.md).
- **Code:** `crates/loom-core/src/safety.rs`.

## What the clip already does

ADR-0007 says cables are never clipped. Only the final output is, to -1 to 1, the range the audio device accepts. NaN and infinity become 0. That protects the device from numbers it cannot play.

But a number in range can still do damage. Here are three cases, each a value the clip lets through.

## DC: a sound nobody hears

Patch a Constant of 1.0 into Audio Out. Every sample is 1.0:

| Sample | 0 | 1 | 2 | 3 | ... |
|---|---|---|---|---|---|
| Value | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 |

It is in range, so the clip passes it. A speaker turns this into a cone pushed all the way out and held there. You hear one click when it starts, then nothing. Meanwhile the voice coil carries a steady current that makes no sound, only heat. Left long enough, the coil burns out.

A constant offset like this is called **DC** (direct current). In modular patching it is easy to make: a Constant, an envelope or LFO patched to the output, or an unbalanced waveshaper.

**The fix is a high-pass filter at a very low frequency.** A first-order high-pass at 5 Hz lets through everything we can hear and slowly pulls any constant offset back to 0. Its time constant is `1 / (2 * pi * 5)`, about 32 ms. After a step to 1.0, the output jumps to 1.0, then decays: about 0.37 after 32 ms, about 0.007 after 160 ms, and nothing measurable after half a second.

At 20 Hz, the lowest note anyone hears, the filter cuts only about 0.3 dB.

**A wrong turn:** "just subtract the average". The average of what? A running average over a window is itself a low-pass filter, and subtracting it is a high-pass, only a more expensive one.

Eurorack modules usually have DC-coupled outputs on purpose, because the same jack carries audio and control voltages. Audio interfaces usually block DC at their outputs, but not all do: some are DC-coupled on purpose, so they can send control voltages to a modular. That is why LoomDSP's safety stage can be turned off.

## Loud, but legal

Full-scale white noise, or a square wave at 1.0, is in range. It is also as loud as the system can go, and much louder than music, because it has no quiet moments. On headphones or a PA it hurts.

A hard clip does not help here: it only acts above 1.0. And it adds a problem of its own. The converter in the audio device does not draw straight lines between samples. It draws a smooth curve, and that curve can rise above the samples on either side. A signal whose samples all sit at 1.0 can produce a curve that goes over full scale between them. These are called **inter-sample peaks**, and they can distort in the converter.

**The fix is a limiter with a ceiling a little below full scale.** A limiter watches the level and turns the gain down when a sample would go over the ceiling, then lets the gain recover slowly. A ceiling of -1 dBFS (0.891) leaves room for most inter-sample peaks without the cost of measuring them exactly.

## What lookahead buys, and what it costs

A limiter has to decide how fast to turn the gain down.

- **No lookahead.** The limiter sees a loud sample only when it arrives. To keep that sample under the ceiling, the gain must drop at once, in the same sample. A sudden gain change is a small distortion: for those first peaks, the limiter acts almost like a clip. Then the gain recovers smoothly. No latency is added.
- **Lookahead.** The limiter delays the audio by a few milliseconds and looks at the samples coming up. It can start turning the gain down before the loud sample arrives, so the change is smooth and inaudible. The price is the delay: every sound comes out that much later.

For a live player, 2 ms of extra latency is noticeable on top of the device's own buffer. For a safety net that should only act on accidents, a moment of distortion during an accident is the better trade. That is why LoomDSP's default has no lookahead, and *Clean* mode is a choice.

## Clicks at the start

When audio starts, the output goes from silence to whatever the patch makes. If the patch starts with a square wave at 1.0, the first sample jumps from 0 to 1.0. A jump like that has energy at every frequency, which is what a click is.

**The fix is a short fade-in.** Ramp the output gain from 0 to 1 over 10 ms. The ear hears a fast start, not a click. The same idea is behind the short fades when a cable is connected (FR-GRAPH-05).

## Order matters

The stage first turns NaN and infinity into 0, then runs the DC blocker, the limiter, the fade, and the clip:

- **DC blocker before the limiter.** Otherwise a DC offset uses up the limiter's headroom, and the limiter turns the music down to make room for a sound nobody hears.
- **Limiter before the clip.** A limiter that meets an already-clipped wave cannot undo the flat tops. Seeing the unclipped signal, it lowers the gain instead.
- **Clip last.** If everything else works, the clip never acts. It stays as the last line of defence, and it is all that runs when the stage is off.
