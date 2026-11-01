# Phase Wrap and Fold

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Brings any signal into the phase range, 0 to 1. Wrap goes round the edge; fold turns back at the edges.

| | |
|---|---|
| ID | `loom.phase.wrap` |
| Version | 1 |
| Family | Time and rhythm |
| Category | Phase |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | modulation | 0 | The signal to bring into the phase range. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | phase | unipolar | A phase from 0 to 1. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | Wrap or Fold | Wrap | Wrap goes round the edge. Fold turns back at the edges. |

## Panel

6 by 3 grid units.

- Radio buttons: **Mode** (`mode`) (Wrap, Fold)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Phase Wrap and Fold to bring any signal into the phase range, 0 to 1, so it can drive a wave shaper.

Wrap goes round the edge, like a clock. Fold turns back at 0 and 1, which on an oscillator makes new harmonics as the signal grows.

## Example patch

Folds an amplified sine wave to generate wavefolded distortion harmonics.

```ron
Patch(
    version: 1,
    name: "Wavefolder",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.level.amp@1", params: {"gain": 2}),
        PatchModule(id: 4, kind: "loom.phase.wrap@1", params: {"mode": 1}),
        PatchModule(id: 5, kind: "loom.osc.sine@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "out"), to: (5, "phase")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Why feedback FM turns to noise](../learn/feedback-fm.md)
