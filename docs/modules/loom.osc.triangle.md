# Triangle

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

A triangle wave with few harmonics, shaped from a phase input. Its corners are smoothed so high notes stay clean.

| | |
|---|---|
| ID | `loom.osc.triangle` |
| Version | 1 |
| Family | Sources |
| Category | Oscillator |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **Phase** (`phase`) | phase | 0 | Position in the wave, from 0 to 1. One ramp is one cycle. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | Triangle wave from -1 to 1. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Triangle for a soft tone with few harmonics, between a sine and a square. It suits gentle leads and flute-like sounds.

It needs a phase input, usually from Phase. Its corners are smoothed, so high notes stay clean.

## Example patch

Plays a mellow triangle tone through audio out.

```ron
Patch(
    version: 1,
    name: "Triangle Voice",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.triangle@1"),
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
- [Sine](loom.osc.sine.md)
