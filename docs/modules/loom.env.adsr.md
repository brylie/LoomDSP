# ADSR

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Envelope from 0 to 1, triggered by a gate. Linear attack, exponential decay and release.

| | |
|---|---|
| ID | `loom.env.adsr` |
| Version | 1 |
| Category | Envelope |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Gate** (`gate`) | gate | 0 | Going high starts the envelope. Going low releases it. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | unipolar | The envelope level, from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Attack** (`attack`) | 0.0005 to 10 s, logarithmic | 0.01 s | Time to rise from 0 to 1 after the gate goes high. |
| **Decay** (`decay`) | 0.0005 to 10 s, logarithmic | 0.2 s | Time to fall from 1 to the sustain level. |
| **Sustain** (`sustain`) | 0 to 1 | 0.7 | The level held while the gate stays high. |
| **Release** (`release`) | 0.0005 to 10 s, logarithmic | 0.3 s | Time to fall to 0 after the gate goes low. |

## Panel

6 by 8 grid units.

- Knob: **Attack** (`attack`)
- Knob: **Decay** (`decay`)
- Knob: **Sustain** (`sustain`)
- Knob: **Release** (`release`)
- Light: **Level** (`level`): The envelope's level, from dark at 0 to full at 1.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use ADSR to generate an envelope that contours volume, filter cutoff, or modulation depth over time. It triggers when a gate goes high and releases when the gate goes low. Exponential decay and release curves match human hearing.

## Example patch

Applies an amplitude envelope to a sine oscillator driven by MIDI notes.

```ron
Patch(
    version: 1,
    name: "ADSR Envelope",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.sine@1"),
        PatchModule(id: 4, kind: "loom.env.adsr@1", params: {"attack": 0.02, "decay": 0.3, "sustain": 0.5, "release": 0.4}),
        PatchModule(id: 5, kind: "loom.level.amp@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (1, "gate"), to: (4, "gate")),
        PatchCable(from: (3, "out"), to: (5, "in")),
        PatchCable(from: (4, "out"), to: (5, "level")),
        PatchCable(from: (5, "out"), to: (6, "left")),
    ],
)
```

## See also

- [Triggers and gates](../learn/triggers-and-gates.md)
- [Sine patch](../../examples/patches/sine.ron)
