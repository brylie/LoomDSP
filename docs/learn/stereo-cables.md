# Stereo cables

Why LoomDSP works out which cables carry stereo instead of making every cable stereo, why most modules run as two copies, and what happens when stereo meets an input that wants mono.

- **Specification:** [Stereo cables](../design/stereo.md)
- **Decision:** [ADR-0020](../adr/0020-inferred-channel-count.md)

## What a stereo signal is

A stereo signal is two signals that belong together: one for the left speaker and one for the right. Where a sound seems to sit between the speakers depends on how the two differ.

| Left | Right | What you hear |
|---|---|---|
| 0.5 | 0.5 | A sound in the centre. |
| 0.5 | 0.0 | A sound on the left. |
| 0.5 | 0.4 | Slightly left of centre. |
| 0.5 | -0.5 | Wide and hollow, hard to place. |

In Eurorack, stereo is two cables: one per side, often into a "dual" module that has two of everything. That works, but a stereo chain of five modules is ten cables and five dual modules, and turning one cutoff knob means turning two.

## Why not stereo on every cable

Most cables in a modular patch are not audio. They carry LFOs, envelopes, pitch, gates, and phases. An LFO is the same on both sides, so a stereo LFO cable is two copies of the same numbers. Computing it twice wastes CPU and memory, and in a large patch the waste is most of the work.

So LoomDSP lets stereo exist only where something makes it: a stereo sample, a panner, a stereo reverb. From there it spreads only through inputs that ask for it, which are usually audio inputs. A module that receives stereo there runs once per side, so all its outputs become stereo, even an envelope or a gate it makes from the audio. Control inputs and modulation never start this on their own: a stereo cable into a cutoff input is averaged to one value, and the LFO cables around it stay mono.

## Why it is worked out when the patch compiles

The engine could look at the samples and ask "are left and right the same?". That would make the CPU cost change with the music. A patch that runs fine in a quiet passage could overload in a loud one, mid-performance. The audio thread needs to know its work in advance.

The patch itself already holds the answer. If a stereo source feeds a filter, the filter's output is stereo, whatever the samples are. So the engine works out every cable's width once, on the control thread, when the patch changes. Then the audio thread just runs the plan.

## Why most modules run as copies

A filter does not care about stereo. Filtering the left side and filtering the right side are two separate jobs that happen to use the same knob settings. So the engine runs two copies of the filter, one per side, each with its own memory of past samples. The module's code is the same mono code it always was.

This is the same idea as a dual module in Eurorack, except the engine does the doubling, and both copies share one panel.

Some modules do care about both sides at once:

- A **panner** turns one signal into two.
- A **stereo reverb** mixes the sides on purpose, to sound wide.
- A **linked limiter** must turn both sides down together, or the sound would lean to one side when it acts.

These declare stereo ports and handle both channels themselves. The engine never copies them.

## When stereo meets a mono input

A filter's cutoff input wants one number per sample. What should it read if a stereo cable arrives?

The obvious answer is to add the two sides. It fails for the most common case, a mono sound sent to both speakers:

| Left | Right | Left + Right | (Left + Right) / 2 | Left only |
|---|---|---|---|---|
| 0.5 | 0.5 | 1.0 (twice as loud) | 0.5 | 0.5 |
| 0.5 | 0.0 | 0.5 | 0.25 | 0.5 |
| 0.0 | 0.5 | 0.5 | 0.25 | 0.0 (lost) |

Adding doubles the centred signal, which is a jump of 6 dB. Taking the left side silently loses anything panned right. The mean keeps a centred signal at its level and hears both sides, so LoomDSP downmixes stereo to mono with the mean. (This is not wave folding, which bends a waveform to add harmonics. A downmix only mixes the two sides.)

### A downmix can cancel

If the two sides are opposite, the mean is zero:

| Left | Right | Mean |
|---|---|---|
| 0.5 | -0.5 | 0.0 |

This is not a bug in the downmix. It is what happens when any stereo signal with opposite sides is played on one speaker, and why some wide stereo effects vanish on a phone speaker. If a control input seems dead when fed from a wide stereo source, this is likely why. Patch from one side with a split, or from a mono point earlier in the chain.

## Copies of random things

Two copies of a noise source with the same seed give the same numbers on both sides. That is mono that only looks like stereo. So a copy can ask which channel it is, and a module with a seed mixes the channel into it. The left and right sides then differ, and the patch still gives the same result every time it runs.

## How other systems do it

- **Eurorack:** two cables and dual modules. Nothing is automatic, and nothing is hidden.
- **VCV Rack:** one cable can carry several channels, mostly for polyphony. Each module decides what to do with them, so the behaviour is per module.
- **Bitwig Grid:** signals are stereo by default, so the player rarely thinks about it, and mono signals still travel as two channels.

LoomDSP sits between these. Like Grid, the player does not manage stereo. Like Eurorack, a mono cable costs one channel.
