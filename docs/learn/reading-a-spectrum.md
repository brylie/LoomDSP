# Reading a Spectrum

What a spectrum display shows, why it needs a window, and why it does not flicker. For musicians and developers who have seen an analyser but never looked inside one.

- **Kind:** explainer.
- **Used by:** [panel widgets specification](../design/panel-widgets.md#spectrum), [M2 modules specification](../design/p2-modules.md#spectrum-mt-view-03).
- **Code:** `loom.display.spectrum` in `crates/loom-modules/src/display.rs`, `SpectrumAnalyser` in `crates/loom-app/src/spectrum.rs`.

## From samples to frequencies

A cable carries one number per sample. That says how the signal moves over time. It does not say which pitches are in it. A Fourier transform turns a run of samples into a list of frequencies, each with a level. A fast Fourier transform (FFT) is a quick way to compute it.

LoomDSP takes 4096 samples at a time. The result has one value, a **bin**, for every multiple of the sample rate divided by 4096. At 48 kHz that is about 11.7 Hz per bin: 0 Hz, 11.7 Hz, 23.4 Hz, and so on, up to 24 kHz.

| Choice | At 48 kHz | Cost |
|---|---|---|
| 1024 samples | 47 Hz per bin | Low notes blur together |
| 4096 samples | 11.7 Hz per bin | Each result covers 85 ms |
| 16384 samples | 2.9 Hz per bin | Each result covers 341 ms, so a fast note smears |

More samples tell frequencies apart better but react more slowly. No choice avoids this. 4096 is a usual middle for music.

## Why a window

An FFT treats its samples as one cycle of a loop. If a sine does not fit a whole number of times, the end of the run does not meet the start, and that jump looks like many other frequencies. A clean tone then spreads across the whole display. This is called leakage.

A **window** fades the run in and out, so the ends meet at zero. LoomDSP uses a Hann window, `0.5 - 0.5 cos(2πn / N)`. Its skirts fall away fast, so a tone stays a narrow peak.

The window costs two things, and each has a fix.

- **Level.** Fading the samples removes energy. A Hann window averages 0.5, so a full-scale sine gives half the bin value it would without one. The code makes this up, so a full-scale sine reads 0 dB.
- **Time.** The fade hides what happens at the ends of the run. So each analysis starts 2048 samples after the last one. What one analysis fades out, the next sees in its middle. This is the half overlap.

A sine between two bins still reads about 1.4 dB low at its peak. That is part of the window's shape. It is not an error in your signal.

## Why columns are not bins

Bins are evenly spaced in Hz, but we hear pitch by ratio. So the display spaces 256 columns evenly in octaves, from 20 Hz to 20 kHz. Each octave has the same width, which matches how a keyboard is laid out.

This makes two kinds of column.

- **High columns hold many bins.** Near 10 kHz one column spans over 20 bins. The column takes the highest one. An average would lower a narrow peak until it vanished.
- **Low columns hold no bin at all.** Near 30 Hz a column is under 1 Hz wide, and bins are 11.7 Hz apart. Most hold nothing. They read a level between the two nearest bins, in dB, so the line stays continuous. The display cannot tell you more about low frequencies than its bins know, so a low peak looks wider than a high one, even for a pure tone.

## Why the line falls slowly

Each analysis gives a different answer, because noise and music change. Drawn raw, the line jumps 23 times a second and is hard to read. So the line rises at once and falls at 20 dB per second. You see each peak, and then you see it fade. A hold line can stay at each peak for two seconds, for a sound too short to read.

The rising and falling also has a cost in code. Every message to the UI redraws the window. So the telemetry thread sends a new line only when a column moves by at least half a dB. On a steady tone, tiny rounding changes never reach the screen, and the window stays idle.

## What tilt is for

A typical full mix has less energy as frequency rises. On a flat display it looks like a slope falling to the right, and a problem at 8 kHz is hard to compare with one at 200 Hz. Tilt adds 4.5 dB per octave around 1 kHz, only in the drawing, so such a mix reads closer to level. It says nothing new about the signal, and it does not change the sound.

## How other systems do it

Most analysers, in hardware and software, use the same parts: an FFT, a window, overlap, a logarithmic axis, and a slow fall. They differ in size, in window, and in the speed of the fall. In LoomDSP the spectrum is a module on a tile, like the [scope](../design/panels.md#scope-module), so it saves with the patch and can sit in the signal path without changing the sound.

## Further reading

- Fredric J. Harris, "On the use of windows for harmonic analysis with the discrete Fourier transform", Proceedings of the IEEE, 66(1), 1978. The properties of the Hann window, including its gain and its dip between bins.
- Alan V. Oppenheim and Ronald W. Schafer, *Discrete-Time Signal Processing*. The discrete Fourier transform and leakage.
