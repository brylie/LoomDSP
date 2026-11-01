# Subtract

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Subtracts signal B from signal A.

| | |
|---|---|
| ID | `loom.math.sub` |
| Version | 1 |
| Family | Logic and math |
| Category | Math |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **A** (`a`) | modulation | 0 | The signal to subtract from. |
| **B** (`b`) | modulation | 0 | The signal to subtract. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | modulation | bipolar | A minus B. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Subtract to take one signal away from another: find how far apart two pitches are, or remove one part of a mix by subtracting it.

## Example patch

Subtracts an envelope from a steady value to create an inverted modulation contour.

```ron
Patch(
    version: 1,
    name: "Inverted Envelope",
    modules: [
        PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 1}),
        PatchModule(id: 2, kind: "loom.env.envelope@1", params: {"attack": 0.05, "decay": 0.2, "sustain": 0.3, "release": 0.3}),
        PatchModule(id: 3, kind: "loom.math.sub@1"),
        PatchModule(id: 4, kind: "loom.phase.generator@1"),
        PatchModule(id: 5, kind: "loom.osc.saw@1"),
        PatchModule(id: 6, kind: "loom.filter.svf@1"),
        PatchModule(id: 7, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (3, "a")),
        PatchCable(from: (2, "out"), to: (3, "b")),
        PatchCable(from: (3, "out"), to: (6, "cutoff")),
        PatchCable(from: (4, "phase"), to: (5, "phase")),
        PatchCable(from: (5, "out"), to: (6, "in")),
        PatchCable(from: (6, "lowpass"), to: (7, "in")),
    ],
)
```

## See also

- [Add](loom.math.add.md)
- [Scale](loom.level.scale.md)
