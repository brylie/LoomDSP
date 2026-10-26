# Triangle

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Band-limited triangle wave shaped from a phase input (polyBLAMP).

| | |
|---|---|
| ID | `loom.osc.triangle` |
| Version | 1 |
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

Use Triangle to produce a mellow waveform containing only odd harmonics that roll off quickly. It provides a soft, warm timbre for flutes, gentle leads, and acoustic modeling.

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
        PatchCable(from: (2, "out"), to: (3, "left")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Sine](loom.osc.sine.md)
