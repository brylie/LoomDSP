# Phase wrap and fold

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Brings any signal into the phase range, 0 to 1. Wrap goes round the edge; fold turns back at the edges.

| | |
|---|---|
| ID | `loom.phase.wrap` |
| Version | 1 |
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
| **Mode** (`mode`) | 0 to 1, whole numbers, choices Wrap, Fold | 0 | Wrap goes round the edge. Fold turns back at the edges. |

## Panel

3 by 3 grid units.

- Radio buttons: **Mode** (`mode`) (Wrap, Fold)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Phase wrap and fold to constrain arbitrary modulation signals into the valid phase domain (0 to 1). In Wrap mode, signals wrap circularly past the boundaries. In Fold mode, signals reflect back at 0 and 1, creating wavefolding timbres.

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
        PatchCable(from: (5, "out"), to: (6, "left")),
    ],
)
```

## See also

- [Phase and cycles](../learn/phase-and-cycles.md)
- [Why feedback FM turns to noise](../learn/feedback-fm.md)
