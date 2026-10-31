# Split

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

Separates a stereo cable into its two sides.

| | |
|---|---|
| ID | `loom.mix.split` |
| Version | 1 |
| Category | Mix |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio, stereo | 0 | The stereo signal to split. |

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Left** (`left`) | audio | bipolar | The left side of the signal. |
| **Right** (`right`) | audio | bipolar | The right side of the signal. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Use Split to work with the left and right sides of a stereo signal separately.

## Example patch

This patch joins two constants and splits them again.

```ron
Patch(
    version: 1,
    name: "Stereo channels",
    modules: [
        PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 0.25}),
        PatchModule(id: 2, kind: "loom.math.constant@1", params: {"value": -0.5}),
        PatchModule(id: 3, kind: "loom.mix.join@1"),
        PatchModule(id: 4, kind: "loom.mix.split@1"),
        PatchModule(id: 6, kind: "loom.mix.join@1"),
        PatchModule(id: 5, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (6, "out"), to: (5, "in")),
        PatchCable(from: (1, "out"), to: (3, "left")),
        PatchCable(from: (2, "out"), to: (3, "right")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (4, "left"), to: (6, "left")),
        PatchCable(from: (4, "right"), to: (6, "right")),
    ],
)
```

## See also

- [Stereo cables](../learn/stereo-cables.md)
