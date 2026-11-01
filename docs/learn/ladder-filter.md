# How a ladder filter sings

Why a chain of simple filters with feedback gives a resonant low-pass, why four stages self-oscillate and two never do, why the loop needs no unit delay here, and why changing the slope while playing takes care. For players who turn resonance up and want to know what happens, and for people who work on the filter.

- **Kind:** explainer.
- **Used by:** [M1 modules specification, Ladder low-pass](../design/p1-modules.md#ladder-low-pass-loomfilterladder).
- **Related:** [Feedback loops and the unit delay](feedback-loops.md), [Pitch at 0.1 per octave](pitch.md), [Hot patching](hot-patching.md).
- **Code:** `loom.filter.ladder` in `crates/loom-modules/src/filter.rs`.
- **Sources:** T. Stilson and J. Smith, "Analyzing the Moog VCF with Considerations for Digital Implementation", ICMC 1996. V. Zavalishin, "The Art of VA Filter Design".

## The patch inside the module

A one-pole low-pass is the gentlest filter: above its cutoff, each octave up is 6 dB quieter. Put two in a row and it is 12 dB per octave, three give 18, four give 24. That is the "ladder": stages stacked like rungs.

Now take the output of the last stage, turn it upside down, and add it to the input:

```
in --> (+) --> stage 1 --> stage 2 --> stage 3 --> stage 4 --> out
        ^                                                   |
        +-------------------- × -k <------------------------+
```

`k` is the feedback gain. The `resonance` knob sets it: `k = resonance × k_max`.

## 1. Why inverted feedback makes a peak

Each one-pole stage delays the phase of a sine. At its own cutoff one stage delays by 45 degrees, and far above it by almost 90. Four stages at the cutoff delay by 4 × 45 = 180 degrees. A sine delayed by 180 degrees is upside down. The feedback turns it upside down once more, so at that one frequency the feedback **adds** to the input instead of taking away. Every other frequency is partly cancelled. The result is a peak at the cutoff.

Each stage at its cutoff is also 3 dB down, a gain of `1/√2`. Four stages give `(1/√2)^4 = 1/4`. So the loop gain at the cutoff is `k/4`. When `k` reaches 4, a sine at the cutoff comes round the loop exactly as loud as it went in. It keeps going with no input: the filter **self-oscillates**, and you hear a pure whistle.

## 2. Three stages sing higher, two never do

With three stages the phase reaches 180 degrees where each stage delays by 60 degrees. That is at `tan(60°) = √3 ≈ 1.73` times the cutoff. There each stage has a gain of `cos(60°) = 1/2`, so three give `1/8`, and the loop sings at `k = 8`. So at 18 dB the whistle is about 1.7 times higher than the cutoff knob says.

Two stages can delay by at most 2 × 90 = 180 degrees, and only at an infinite frequency, where the gain is 0. They never sing. At `k = 8` the 12 dB slope has a peak about 4 dB above its own level at low frequencies: strong, but not a whistle.

Stilson and Smith work these limits out from the poles of the loop: the loop rings forever when `(1 + s)^N = -k` has a root on the imaginary axis.

| Slope | Stages | Sings at | Feedback limit |
|---|---|---|---|
| 12 dB | 2 | never | none |
| 18 dB | 3 | about 1.7 × cutoff | 8 |
| 24 dB | 4 | the cutoff | 4 |

## 3. Why the bass gets thin

At very low frequencies the stages pass the signal straight through, with no phase change. Then the inverted feedback just subtracts: the output is `1 / (1 + k)` of the input. At `k = 4` that is a fifth, about 14 dB quieter. This is part of the sound of a classic ladder, so LoomDSP does not correct it. Turn `drive` up, or follow the filter with an amplifier, to get the level back.

The tile's response preview uses this same small-signal model. For `N` stages and a frequency ratio `r = frequency / cutoff`, it draws the magnitude of `1 / ((1 + jr)^N + k)`. The preview shows the slope, peak, and bass reduction together. It cannot show the exact response while drive or self-oscillation changes the signal, so it is a guide to the controls rather than a live measurement.

## 4. Why the limit is a little above 4

At exactly `k = 4` the loop only keeps an oscillation going. It does not grow one from a small start. And the saturation (next section) lowers the loop gain a little at any level above 0, so the whistle would slowly die. LoomDSP sets the limit 5% higher, 4.2 at 24 dB and 8.4 at 18 dB, so resonance 1 always sings. The saturation then stops it from growing past a steady level.

## 5. Saturation keeps it stable

With `k` above the limit, a purely linear loop would grow without end, until the numbers overflow. A real ladder made of transistors saturates instead: big signals are squashed. LoomDSP puts a `tanh` curve in the feedback path. For small signals `tanh(y) ≈ y`, so nothing changes. For big ones `tanh` never goes past 1, so the feedback is limited and the whistle settles at a steady level.

`drive` raises the input level, from 1 to 8 times (0 to +18 dB). A louder signal reaches the bend of the `tanh` sooner, so the resonance is squashed more and the sound gets thicker. Without oversampling, heavy drive can alias. That is a later quality setting.

## 6. A loop with no unit delay

LoomDSP's own cable loops need a [one-sample delay](feedback-loops.md), because a module cannot read an output that is not computed yet. Inside one module there is a better way. Zavalishin shows that each trapezoidal one-pole stage gives an output that is a straight line in its input:

```
y = G·x + s / (1 + g),   where g = tan(π · cutoff / sample rate), G = g / (1 + g)
```

and `s` is the stage's state from the last sample. Chain four of them, and the last output is also a straight line in the loop input `u`: `y4 = a·u + b`. The loop says `u = in − k·y4`. Two equations, two unknowns: solve for `u` with one division, then run the stages. No sample of delay sits in the loop. This is the **zero-delay feedback** form. *Why it matters:* a unit delay in the loop shifts the phase, so the peak drifts away from the cutoff at high frequencies and the self-oscillation limit is no longer 4. With zero-delay feedback the filter matches the analogue model, and the `tan` keeps the cutoff exactly where the knob says.

The `tanh` breaks the straight line. LoomDSP keeps the solve to one division by replacing `tanh(y)` with a straight line through the origin and the point on the curve at the last output: `tanh(y) ≈ y · tanh(y_last) / y_last`. The slope of that line is never above 1, so the approximation can only lower the loop gain, never push it past the limit.

## 7. Changing the slope while it plays

The slope sets which stage the output and the feedback come from. All four stages always run, so a switch finds them already moving. But a sudden switch would still click, because the stages have different outputs. So the old and new outputs crossfade over 10 ms.

The feedback gain needs care too. Going from 18 dB to 24 dB, the limit drops from 8.4 to 4.2. If the gain stayed at 8.4 while the 4-stage tap came in, the 24 dB loop would run at twice its limit and burst into a loud whistle. So the gain may never pass the limit of a tap that is heard:

- **To a lower limit** (18 or 12 dB to 24 dB): the gain first moves down to the new limit, over up to 10 ms, with only the old tap heard. Then the taps crossfade.
- **To a higher limit** (24 dB to 18 or 12 dB): the taps crossfade first, at the old gain. Then the gain moves up.

The gain moves at a steady rate and never jumps, because a jump in the feedback gain is a jump in the output.

## In other systems

Many hardware ladder filters have a fixed 24 dB slope, and some Eurorack modules add a second output for 12 dB. LoomDSP keeps one module with a `slope` choice, so a player can make the filter gentler without changing modules, and the same stages and the same code serve every slope.
