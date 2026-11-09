# Sine

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A sine wave at the pitch, or shaped from a phase cable.

| | |
|---|---|
| ID | `loom.osc.sine` |
| Version | 1 |
| Family | Sources |
| Category | Oscillator |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Pitch** (`pitch`) | pitch | 0 | Sets the frequency. 0 is middle C, and +0.1 is one octave up. Ignored while a phase cable is connected. |
| **Phase** (`phase`) | phase | own phase | Position in the wave, from 0 to 1. With no cable, the oscillator runs its own phase at the pitch. A cable replaces it. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | Sine wave from -1 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Tune** (`tune`) | -48 to 48 st | 0 st | Shifts the pitch in semitones. Ignored while a phase cable is connected. |
| **Octave** (`octave`) | -10 to 10 oct | 0 oct | Shifts the pitch in octaves. Ignored while a phase cable is connected. |

## Panel

6 by 3 grid units.

- Knob: **Tune** (`tune`)
- Knob: **Octave** (`octave`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Sine for a pure tone with no harmonics: a deep sub bass, a test tone, or either side of frequency modulation.

Patch a pitch into it and it plays, at middle C with nothing connected. Tune and Octave shift the pitch. A cable into Phase replaces its own phase, and then the wave follows the cable: share one Phase between several oscillators to keep them in step, or bend the phase first for a new wave.

## Example patch

Plays a pure sine tone at middle C, with no cables in.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Pure Sine"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
    ],
)
```

## See also

- [Oscillators](../design/oscillators.md)
- [Phase and cycles](../learn/phase-and-cycles.md)
- [Why feedback FM turns to noise](../learn/feedback-fm.md)
- [Sine patch](../../examples/patches/sine.ron)
