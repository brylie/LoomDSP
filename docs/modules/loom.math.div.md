# Divide

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Divides signal A by signal B. Outputs 0 while B is near zero.

| | |
|---|---|
| ID | `loom.math.div` |
| Version | 1 |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | modulation | 0 | The signal to divide. |
| **B** (`b`) | modulation | 0 | The divisor. Reads 1 while no cable is connected. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | A divided by B, or 0 while B is within 1e-9 of zero. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Divide to divide signal A by signal B. When signal B drops near zero (within 1e-9), the module safely outputs 0 rather than producing infinity or NaN.

## Example patch

Divides an LFO signal by a constant to scale down its amplitude.

```ron
Patch(
    version: 1,
    name: "Divide Signal",
    modules: [
        PatchModule(id: 1, kind: "loom.mod.lfo@1", params: {"rate": 2, "shape": 1}),
        PatchModule(id: 2, kind: "loom.math.constant@1", params: {"value": 2}),
        PatchModule(id: 3, kind: "loom.math.div@1"),
        PatchModule(id: 4, kind: "loom.phase.generator@1"),
        PatchModule(id: 5, kind: "loom.osc.sine@1"),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (3, "a")),
        PatchCable(from: (2, "out"), to: (3, "b")),
        PatchCable(from: (3, "out"), to: (4, "pitch")),
        PatchCable(from: (4, "phase"), to: (5, "phase")),
        PatchCable(from: (5, "out"), to: (6, "in")),
    ],
)
```

## See also

- [Multiply](loom.math.mul.md)
- [Scale](loom.level.scale.md)
