# Compare

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Makes a gate from two signals: high while A is above B, or while A is below B. Hysteresis keeps a noisy signal from chattering.

| | |
|---|---|
| ID | `loom.logic.compare` |
| Version | 1 |
| Category | Logic |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | modulation | 0 | The signal to test. |
| **B** (`b`) | modulation | 0 | The threshold A is compared with. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Gate** (`gate`) | gate | unipolar | High while the comparison holds. Equal values count as low. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Mode** (`mode`) | 0 to 1, whole numbers, choices Above, Below | 0 | Above: high while A is above B. Below: high while A is below B. |
| **Hysteresis** (`hysteresis`) | 0 to 1 | 0 | A dead band around B. Once the gate is high, A must go this far past B the other way to end it. |

## Panel

9 by 4 grid units.

- Radio buttons: **Mode** (`mode`) (Above, Below)
- Knob: **Hysteresis** (`hysteresis`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Compare to turn continuous signals into gates by testing if signal A is above or below signal B. Built-in hysteresis prevents noisy signals from flickering the output gate near the threshold.

## Example patch

Triggers an envelope whenever an LFO rises above a fixed threshold.

```ron
Patch(
    version: 1,
    name: "LFO Comparator",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"rate": 2}),
        PatchModule(id: 2, kind: "loom.math.constant@1", params: {"value": 0.5}),
        PatchModule(id: 3, kind: "loom.logic.compare@1", params: {"mode": 0, "hysteresis": 0.05}),
        PatchModule(id: 4, kind: "loom.env.adsr@1"),
        PatchModule(id: 5, kind: "loom.osc.noise@1"),
        PatchModule(id: 6, kind: "loom.level.amp@1"),
        PatchModule(id: 7, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (3, "a")),
        PatchCable(from: (2, "out"), to: (3, "b")),
        PatchCable(from: (3, "gate"), to: (4, "gate")),
        PatchCable(from: (5, "out"), to: (6, "in")),
        PatchCable(from: (4, "out"), to: (6, "level")),
        PatchCable(from: (6, "out"), to: (7, "left")),
    ],
)
```

## See also

- [Triggers and gates](../learn/triggers-and-gates.md)
- [Logic](loom.logic.logic.md)
