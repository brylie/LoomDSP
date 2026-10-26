# Phase

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Ramp from 0 to 1 at the pitch frequency. Drives oscillators and LFOs.

| | |
|---|---|
| ID | `loom.phase.generator` |
| Version | 1 |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Pitch** (`pitch`) | pitch | 0 | Sets the frequency. 0 is middle C, and +0.1 is one octave up. |
| **Reset** (`reset`) | gate | 0 | A rising edge sets the phase back to 0. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | unipolar | A ramp from 0 up to 1, once per cycle. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Tune** (`tune`) | -48 to 48 st | 0 st | Shifts the pitch in semitones. |
| **Octave** (`octave`) | -10 to 10 oct | 0 oct | Shifts the pitch in octaves. |

## Panel

6 by 3 grid units.

- Knob: **Tune** (`tune`)
- Knob: **Octave** (`octave`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Phase Generator as the master oscillator engine. It produces a normalized ramp from 0 to 1 at audio or sub-audio rates, driven by pitch (0.1 per octave). One phase generator can drive multiple oscillator wave shapers simultaneously.

## Example patch

Drives both a sine oscillator and a saw oscillator from a single phase source.

```ron
Patch(
    version: 1,
    name: "Dual Oscillator Phase",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.mix.crossfade@1", params: {"mix": 0.5}),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (1, "phase"), to: (3, "phase")),
        PatchCable(from: (2, "out"), to: (4, "a")),
        PatchCable(from: (3, "out"), to: (4, "b")),
        PatchCable(from: (4, "out"), to: (5, "left")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Pitch at 0.1 per octave](../learn/pitch.md)
