# Why feedback FM turns to noise

Why a sine that modulates its own pitch is a clean, bright tone at low depth and harsh noise at high depth, and why modulating its phase instead stays tame. For players who turn a knob and hear the sound fall apart.

- **Kind:** explainer.
- **Used by:** [control ranges specification](../design/control-ranges.md), the example patch `examples/patches/fm-feedback.ron`.
- **Related:** [Feedback loops and the unit delay](feedback-loops.md), [ADR-0002](../adr/0002-graph-execution-and-feedback.md), [ADR-0007](../adr/0007-polymorphic-signal-model.md), [Pitch at 0.1 per octave](pitch.md).
- **Code:** `loom.phase.generator`, `loom.osc.sine`, `loom.math.mul`.

## The patch

In `fm-feedback.ron` the output of a sine goes through Multiply and back into the pitch of the Phase generator that drives the same sine. The Multiply gain is the **feedback depth**.

```
note pitch ---> Add ---> Phase generator ---> Sine ---> out
                 ^                              |
                 +------ Multiply (depth) <-----+
```

At a depth of 0.08 it sounds like a sine with a brighter edge. At 1 it is rough. At 3 it is noise. Nothing is broken. Four ideas explain it.

## 1. Pitch is exponential, so a small number is a big swing

Pitch on a cable is 0.1 per octave ([pitch explainer](pitch.md)). The sine's output swings between -1 and 1, so after Multiply the pitch moves by plus or minus the depth:

| Depth | Pitch swing | Octaves each way |
|---|---|---|
| 0.08 | 0.08 | 0.8 |
| 0.3 | 0.3 | 3 |
| 1 | 1 | 10 |
| 10 | 10 | 100 |

A depth of 1 looks small on a knob. It sends the frequency of middle C (261.6 Hz) as low as 0.26 Hz and as high as 268 kHz. A depth of 10 asks for 100 octaves, which is a frequency no `f32` can hold. The Phase generator limits its step to 0.49 of a cycle per sample, so that the phase stays a number ([phase explainer](phase-and-cycles.md#why-more-than-05)). The limit protects the patch. It does not make the sound gentler.

## 2. The loop has a one-sample delay

The sine needs the phase, and the phase needs the sine's last output. Something has to come first, so the loop has a one-sample delay ([ADR-0002](../adr/0002-graph-execution-and-feedback.md)). Run `loom check` on the patch to see where the compiler put it.

Here is a simple model of the loop, middle C at 48 kHz. Each frequency comes from the sine output one sample earlier:

| Sample | Frequency at depth 0.08 | Frequency at depth 1 | Frequency at depth 3 |
|---|---|---|---|
| 0 | 261.6 Hz | 261.6 Hz | 261.6 Hz |
| 1 | 261.6 Hz | 261.6 Hz | 261.6 Hz |
| 2 | 266.6 Hz | 331.7 Hz | 533.2 Hz |
| 3 | 271.7 Hz | 420.4 Hz | 1086 Hz |
| 4 | 277.0 Hz | 567.4 Hz | 4598 Hz |
| 5 | 282.5 Hz | 827.8 Hz | 82 621 Hz (capped at 23.5 kHz) |

At depth 0.08 each step nudges the frequency by about 2%. The sine moves slowly, so the nudges stay small, and the loop settles into a steady wave that repeats. The wave is no longer a pure sine, because the frequency leans one way when the sine is high and the other way when it is low. That lean is the bright edge you hear.

At depth 1 and above the feedback is strong enough that one sample of output changes the next step by a large factor. The frequency can jump from hundreds of hertz to thousands within a few samples. Then the sine is read at a very different place, which gives a very different output, which gives another jump. The loop no longer repeats. It is **chaotic**: tiny differences grow fast, and the result looks random. Strong feedback in nonlinear loops does this in many systems, not only audio.

## 3. Frequencies above Nyquist fold back

Even a perfectly regular loop would sound bad when it runs fast. A sampled signal can only hold frequencies below half the sample rate, 24 kHz at 48 kHz (the Nyquist limit). A sine that moves 0.49 of a cycle per sample is the fastest one the samples can show. A faster sine is not lost. It is **aliased**: it shows up at a lower frequency, folded back around Nyquist. A 30 kHz sine at 48 kHz sampling is stored as an 18 kHz sine.

In the loop at depth 3, the frequency spends much of its time above Nyquist. Each fold lands at a new, unrelated frequency, and the folds do not line up with the note. They sound like noise, not like a high pitch.

This is not special to this patch. Any FM with a wide sweep, in any digital system, makes energy above Nyquist.

## 4. Feeding back the phase is tamer

The other way to build feedback FM is to add the output to the **phase**, not the pitch. The sine then reads `phase + depth * previous output`. The output is at most 1, so the phase moves by at most `depth` of a cycle, with no exponent. A depth of 1 shifts the read position by one cycle at most, which is the same wave again. Nothing multiplies the frequency by 1000.

| | Pitch feedback (this patch) | Phase feedback |
|---|---|---|
| What the output moves | The speed of the phase | The read position |
| Effect of depth 1 | Up to 10 octaves each way | At most one cycle of shift |
| Large depth | Chaos, aliasing | A brighter wave, then a rough one |
| Where you meet it | This patch | FM synths that feed an operator back into itself, such as the DX7 |

Phase feedback still gets brighter as depth rises, and at high depth it also turns rough. But its growth is linear, so a knob works over its whole range. Tomisawa's feedback FM patent (Yamaha, 1981) describes this method, and it is why a feedback knob on a classic FM synth is useful across its whole range.

A phase-feedback patch in LoomDSP needs a way to add to a phase before the sine. Phase math modules ([#31](https://github.com/brylie/LoomDSP/issues/31)) are planned for this.

## What to do about it

- **Use a small depth, or a smaller knob range.** [Control ranges](../design/control-ranges.md) lets Multiply be fine at the bottom: a range of -0.1 to 0.1 puts the useful part of the knob under your fingers.
- **Treat high depth as a noise source.** It is a valid sound. It is not a bug.
- **To hear what the loop does, render it.** `loom render` at depths 0.08, 0.3, 1, and 3 shows the change: in a 0.7 s render of the example, the signal crosses zero about 490 times a second at 0.08 and about 29 000 times a second at 3.

## Further reading

- John Chowning, "The Synthesis of Complex Audio Spectra by Means of Frequency Modulation", *Journal of the Audio Engineering Society*, 1973.
- Julius O. Smith, *Physical Audio Signal Processing* and *Spectral Audio Signal Processing*, on aliasing and the sampling theorem (free online at ccrma.stanford.edu/~jos).
