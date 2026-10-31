# Envelope

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A level from 0 to 1, started by a gate. ADSR holds while the gate is high; AD runs once, so a trigger also starts it. Curve bends every stage.

| | |
|---|---|
| ID | `loom.env.envelope` |
| Version | 1 |
| Category | Envelope |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Gate** (`gate`) | gate | 0 | Going high starts the envelope. In ADSR mode, going low releases it. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | unipolar | The envelope level, from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Attack** (`attack`) | 0.0005 to 10 s, logarithmic | 0.01 s | Time to rise to 1 after the gate goes high. |
| **Decay** (`decay`) | 0.0005 to 10 s, logarithmic | 0.2 s | Time to fall from 1 to the sustain level, or to 0 in AD mode. |
| **Sustain** (`sustain`) | 0 to 1 | 0.7 | The level held while the gate stays high. AD mode ignores it. |
| **Release** (`release`) | 0.0005 to 10 s, logarithmic | 0.3 s | Time to fall to 0 after the gate goes low. AD mode ignores it. |
| **Mode** (`mode`) | 0 to 1, whole numbers, choices ADSR, AD | 0 | ADSR follows the gate. AD rises and falls once, whatever the gate does next. |
| **Curve** (`curve`) | -1 to 1 | 0 | Bends every stage. 0 is straight, above 0 starts fast with a slow tail, below 0 starts slow. |

## Panel

15 by 10 grid units.

- Select: **Mode** (`mode`)
- Light: **Level** (`level`): The envelope's level, from dark at 0 to full at 1.
- Shape preview: **Shape** (`out`) (read-only)
- Knob: **Attack** (`attack`)
- Knob: **Decay** (`decay`)
- Knob: **Sustain** (`sustain`)
- Knob: **Release** (`release`)
- Knob: **Curve** (`curve`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Envelope to shape volume, filter cutoff, or modulation depth over time. In ADSR mode it starts when a gate goes high, holds the sustain level, and releases when the gate goes low. In AD mode it rises and falls once, so a one-sample trigger from a clock or sequencer plays a whole pluck.

Curve bends every stage. At 0 the stages are straight lines. Turn it up for a fast start and a slow tail, like a plucked string or a classic analogue envelope. Turn it down for slow swells. A new gate in the middle of a stage starts the attack again from the current level, so it never clicks.

## Example patch

Applies an amplitude envelope to a sine oscillator driven by MIDI notes.

```ron
Patch(
    version: 1,
    name: "Envelope",
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.sine@1"),
        PatchModule(id: 4, kind: "loom.env.envelope@1", params: {"attack": 0.02, "decay": 0.3, "sustain": 0.5, "release": 0.4, "curve": 0.5}),
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
