# Saw

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Band-limited saw wave shaped from a phase input (polyBLEP).

| | |
|---|---|
| ID | `loom.osc.saw` |
| Version | 1 |
| Category | Oscillator |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | 0 | Position in the wave, from 0 to 1. One ramp is one cycle. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | Saw wave from -1 to 1. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Saw to generate a band-limited sawtooth wave rich in all integer harmonics. It is the primary building block for subtractive synthesis, basslines, and brass sounds. It expects a phase ramp input from Phase Generator.

## Example patch

Plays a raw sawtooth wave driven by a phase generator.

```ron
Patch(
    version: 1,
    name: "Saw Voice",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1", params: {"tune": -12}),
        PatchModule(id: 2, kind: "loom.osc.saw@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Subtractive patch](../../examples/patches/subtractive.ron)
