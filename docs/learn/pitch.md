# Pitch at 0.1 per octave

How LoomDSP writes pitch as a number on a cable, how that relates to 1 volt per octave, why the scale keeps 20 octaves between -1 and 1, and how to convert to hertz and MIDI notes. For musicians and developers who patch pitch.

- **Kind:** explainer.
- **Used by:** [ADR-0007](../adr/0007-polymorphic-signal-model.md) (signal model), [Feedback FM](feedback-fm.md).
- **Code:** `loom_core::signal::pitch` (`to_hz`, `from_hz`, `from_midi_note`, `to_midi_note`).

## Why pitch is exponential

We hear pitch as ratios. Going up an octave doubles the frequency, whatever note you start from. A (440 Hz) to the next A is 440 Hz more. The A below goes only 220 Hz up. If a cable carried hertz, a vibrato of "+5 Hz" would be a wide wobble on a low note and nearly nothing on a high one.

So a pitch cable carries the **octave count**. Adding 1 octave to any note gives the same musical step. This is the idea behind **1 volt per octave** in Eurorack: the voltage is the logarithm of the frequency, and adding two voltages multiplies two frequencies.

## The scale

LoomDSP uses **0.1 per octave**, with **0.0 = middle C** (MIDI note 60, 261.63 Hz).

| Pitch value | Octaves from middle C | Note | Frequency |
|---|---|---|---|
| -0.2 | -2 | C2 | 65.41 Hz |
| -0.1 | -1 | C3 | 130.81 Hz |
| 0.0 | 0 | C4 (middle C) | 261.63 Hz |
| 0.1 | +1 | C5 | 523.25 Hz |
| 0.2 | +2 | C6 | 1046.50 Hz |

One semitone is 0.1 / 12, about 0.00833.

### Why 0.1 and not 1

Every cable in LoomDSP lives in a range around -1 to 1 (an LFO swings that far, an envelope goes 0 to 1). With the "1.0 = 10 V" model of [ADR-0007](../adr/0007-polymorphic-signal-model.md), 0.1 is exactly **1 V per octave**. A Eurorack player's intuition carries over.

And 0.1 per octave means the full range of -1 to 1 covers **20 octaves**: 10 up and 10 down from middle C. That is about 0.26 Hz to 268 kHz, wider than hearing (about 20 Hz to 20 kHz, ten octaves). So pitch fits in the same range as every other signal. An LFO into a pitch input needs no scaling module, and a pitch can modulate a filter or a level. At 1 per octave, a normal signal of -1 to 1 would only reach one octave and 20 octaves would not fit. The cost is the opposite surprise: a full-range LFO into pitch swings **10 octaves** each way. That is why the modulation depth exists ([modulation](modulation-without-cables.md)), and why a feedback depth of 1 is far from gentle ([Feedback FM](feedback-fm.md)).

## Converting

With `p` the pitch value:

| To get | Formula |
|---|---|
| Hz from pitch | `261.6256 * 2^(p / 0.1)` |
| Pitch from Hz | `log2(hz / 261.6256) * 0.1` |
| Pitch from MIDI note `n` | `(n - 60) * 0.1 / 12` |
| MIDI note from pitch | `p / (0.1 / 12) + 60` |

Check with A4: note 69 is `9 * 0.1 / 12 = 0.075`. Then `261.6256 * 2^0.75 = 440.0 Hz`. The code has these as `pitch::to_hz`, `pitch::from_hz`, `pitch::from_midi_note`, and `pitch::to_midi_note`. Use them and do not write the formulas again.

The pitch value can be a fraction of a note. MIDI note 60.5 is a quarter tone up. A pitch bend is just an addition.

## Wrong turns

- **Hertz on cables.** Not musical, as above. A modulation depth would have to change with the note.
- **MIDI note numbers on cables.** Musical, but 60 is far outside the -1 to 1 range every other signal uses.
- **1.0 per octave.** Pitch could not share a range with other signals ([ADR-0007](../adr/0007-polymorphic-signal-model.md)).

## In other systems

- **Eurorack:** 1 V per octave is the common standard. Which note sits at 0 V differs between modules.
- **VCV Rack:** follows 1 V per octave, with 0 V at C4 ([voltage standards](https://vcvrack.com/manual/VoltageStandards)).
