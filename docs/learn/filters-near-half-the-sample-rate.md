# Filters near half the sample rate

Why a digital bell gets narrower near the top of the range, why a filter's curve depends on the sample rate, and what can be done about it.

- **Kind:** explainer.
- **Used by:** [Dynamics, EQ, and Crossover specification](../design/dynamics-eq-crossover.md).
- **Code:** `Shape::gain_db` in `crates/loom-dsp/src/filter/eq.rs`, `response_preview` in `crates/loom-modules/src/eq.rs`.
- **Sources:** R. Bristow-Johnson, "Audio EQ Cookbook". V. Zavalishin, "The Art of VA Filter Design". M. Vicanek, ["Matched Second Order Digital Filters"](https://www.vicanek.de/articles/BiquadFits.pdf), 2016, which describes Orfanidis's design.

## The top of the range is squeezed

An analogue filter's frequencies go up forever. A digital filter's frequencies stop at half the sample rate, because a sampled signal cannot hold anything higher. So a digital filter must fit an endless range into a limited one.

The common way to do this is the bilinear transform. It keeps a filter stable, and it moves each frequency. A frequency `f` in the digital filter behaves like this frequency in the analogue design:

```text
tan(pi × f / sample_rate)
```

Well below half the sample rate, the tangent is almost a straight line, so little changes. Near half the sample rate, it climbs toward infinity. So all of the analogue range above that point is squeezed into the last part of the digital range. This is called cramping or warping.

## What this does to a bell

The filter is set up so that its centre lands exactly where the knob says. This step is called pre-warping. So the centre and its gain are always right. Everything around the centre still moves, and the bell gets narrower.

A bell at 16 kHz, +12 dB, and Q 1 measures like this:

| Sample rate | At 8 kHz | At 16 kHz | At 20 kHz |
|---|---|---|---|
| Analogue design | 4.0 dB | 12.0 dB | 9.6 dB |
| 96 kHz | 3.4 dB | 12.0 dB | 8.7 dB |
| 48 kHz | 1.8 dB | 12.0 dB | 3.4 dB |
| 44.1 kHz | 1.4 dB | 12.0 dB | 1.6 dB |

At 44.1 kHz, the bell has almost gone by 20 kHz. There is 8 dB less air than the analogue design would give.

A high cut goes the other way. It reaches silence exactly at half the sample rate, because that is where the analogue filter's infinity lands. So it falls faster near the top than its slope says.

## Why the curve uses the sample rate

The wrong turn here is to draw the analogue response, which needs no sample rate and looks tidy. It is what the filter was designed from, and low in the range it is correct. But then the panel can show 9.6 dB where you hear 1.6 dB, which is worse than showing nothing. You would turn the knob for a change that is not happening.

So the EQ's curve works out the same warp as the filter, at the sample rate of the running engine. The same knobs draw a different curve at 44.1 kHz than at 96 kHz, because they make a different sound. To see it, draw a bell at 16 kHz at both rates.

## What can fix the sound

LoomDSP does not fix this yet. It shows it truthfully. There are three known ways to fix it, and each has a cost.

- **Oversampling.** Run the filter at a higher rate, as the 96 kHz row shows. This costs CPU and adds latency, so it waits for [#538](https://github.com/brylie/LoomDSP/issues/538).
- **A different mapping of the design.** Orfanidis's design and Vicanek's matched filters choose a filter whose response follows the analogue one up to half the sample rate. They are usually written as direct-form filters. Rule 4 of the spec requires stability when a knob moves at audio rate, which is why the EQ uses a state variable filter. Whether one of these designs keeps that stability is still an open question.
- **Accept it.** Many digital equalisers do. A boost at 16 kHz at 44.1 kHz is narrower, and it is still a boost at the frequency on the knob.

## Why the ladder's curve is different

The Ladder's curve does not use the sample rate. It is a schematic: its drive and saturation depend on the input, so no curve could be exact. Its text says so.
