# Constant

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Outputs a constant value. Useful for pitch offsets, gates, and testing.

| | |
|---|---|
| ID | `loom.math.constant` |
| Version | 1 |
| Category | Math |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | The value set by the parameter. |

## Parameters

| Parameter | Range | Default | Description |
|---|---|---|---|
| **Value** (`value`) | -10 to 10, control ranges -0.1 to 0.1, -1 to 1, 0 to 1, -5 to 5, 0 to 5, -10 to 10, 0 to 10 (default -1 to 1) | 0 | The number the output holds. |

## Panel

3 by 3 grid units.

- Knob: **Value** (`value`)

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Constant to output a fixed numerical voltage. It is useful for supplying manual pitch offsets, reference thresholds for comparators, or fixed modulation biases across the patch.

## Example patch

Supplies a fixed control voltage to open a filter cutoff.

```ron
Patch(
    version: 1,
    name: "Fixed Offset",
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.saw@1"),
        PatchModule(id: 3, kind: "loom.math.constant@1", params: {"value": 0.2}),
        PatchModule(id: 4, kind: "loom.filter.svf@1"),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (4, "in")),
        PatchCable(from: (3, "out"), to: (4, "cutoff")),
        PatchCable(from: (4, "lowpass"), to: (5, "in")),
    ],
)
```

## See also

- [Constant (pitch)](loom.pitch.constant.md)
- [Constant (level)](loom.level.constant.md)
